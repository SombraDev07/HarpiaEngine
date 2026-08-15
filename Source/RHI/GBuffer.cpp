#include "RHI/GBuffer.h"

#include "RHI/Vulkan/VulkanDevice.h"

#include <cstdio>
#include <initializer_list>

namespace harpia::rhi {
namespace {

[[nodiscard]] bool supportsColorAttachment(VkPhysicalDevice device, VkFormat format)
{
    VkFormatProperties properties{};
    vkGetPhysicalDeviceFormatProperties(device, format, &properties);
    return (properties.optimalTilingFeatures & VK_FORMAT_FEATURE_COLOR_ATTACHMENT_BIT) != 0
        && (properties.optimalTilingFeatures & VK_FORMAT_FEATURE_SAMPLED_IMAGE_BIT) != 0;
}

[[nodiscard]] bool supportsDepthAttachment(VkPhysicalDevice device, VkFormat format)
{
    VkFormatProperties properties{};
    vkGetPhysicalDeviceFormatProperties(device, format, &properties);
    return (properties.optimalTilingFeatures
            & VK_FORMAT_FEATURE_DEPTH_STENCIL_ATTACHMENT_BIT) != 0;
}

// Returns the first supported format, or the last candidate as a last resort so
// the caller still gets something to report against.
[[nodiscard]] VkFormat firstSupportedColor(VkPhysicalDevice                 device,
                                           std::initializer_list<VkFormat>  candidates)
{
    VkFormat fallback = VK_FORMAT_UNDEFINED;
    for (const VkFormat format : candidates) {
        fallback = format;
        if (supportsColorAttachment(device, format)) {
            return format;
        }
    }
    return fallback;
}

} // namespace

GBufferFormats GBufferFormats::select(const VulkanDevice& device)
{
    const VkPhysicalDevice physical = device.physicalDevice();

    GBufferFormats formats;

    formats.albedo = firstSupportedColor(physical,
        {VK_FORMAT_R8G8B8A8_UNORM, VK_FORMAT_B8G8R8A8_UNORM});

    // SNORM spends its whole range on [-1,1], which is exactly what octahedral
    // produces. Half float is the fallback: same channel count, slightly worse
    // distribution near the poles.
    formats.normal = firstSupportedColor(physical,
        {VK_FORMAT_R16G16_SNORM, VK_FORMAT_R16G16_SFLOAT, VK_FORMAT_R16G16B16A16_SFLOAT});

    formats.material = firstSupportedColor(physical,
        {VK_FORMAT_R8G8_UNORM, VK_FORMAT_R8G8B8A8_UNORM});

    formats.motion = firstSupportedColor(physical,
        {VK_FORMAT_R16G16_SFLOAT, VK_FORMAT_R32G32_SFLOAT});

    for (const VkFormat candidate : {VK_FORMAT_D32_SFLOAT, VK_FORMAT_D24_UNORM_S8_UINT,
                                     VK_FORMAT_D16_UNORM}) {
        if (supportsDepthAttachment(physical, candidate)) {
            formats.depth = candidate;
            break;
        }
    }

    return formats;
}

GBufferHandles declareGBuffer(RgBuilder&            builder,
                              const GBufferFormats& formats,
                              std::uint32_t         width,
                              std::uint32_t         height)
{
    const auto describe = [width, height](VkFormat format) {
        RgTextureDesc desc;
        desc.width  = width;
        desc.height = height;
        desc.format = format;
        return desc;
    };

    GBufferHandles handles;

    handles.color[0] = builder.createTexture("GBuffer_Albedo",   describe(formats.albedo));
    handles.color[1] = builder.createTexture("GBuffer_Normal",   describe(formats.normal));
    handles.color[2] = builder.createTexture("GBuffer_Material", describe(formats.material));
    handles.color[3] = builder.createTexture("GBuffer_Motion",   describe(formats.motion));
    handles.depth    = builder.createTexture("GBuffer_Depth",    describe(formats.depth));

    VkClearValue black{};
    black.color = {{0.0f, 0.0f, 0.0f, 0.0f}};

    for (const RgHandle handle : handles.color) {
        builder.writeColor(handle, VK_ATTACHMENT_LOAD_OP_CLEAR, black);
    }

    // Reverse-Z clears to 0: the far plane. Clearing to 1 would keep nothing.
    VkClearValue farPlane{};
    farPlane.depthStencil = {0.0f, 0};
    builder.writeDepth(handles.depth, VK_ATTACHMENT_LOAD_OP_CLEAR, farPlane);

    return handles;
}

} // namespace harpia::rhi
