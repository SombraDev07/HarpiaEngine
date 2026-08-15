#include "RHI/GpuTexture.h"

#include "RHI/Vulkan/VulkanDevice.h"

#include <vk_mem_alloc.h>

#include <cstdio>
#include <cstring>

namespace harpia::rhi {
namespace {

[[nodiscard]] bool supportsLinearBlit(VkPhysicalDevice device, VkFormat format)
{
    VkFormatProperties properties{};
    vkGetPhysicalDeviceFormatProperties(device, format, &properties);
    return (properties.optimalTilingFeatures
            & VK_FORMAT_FEATURE_SAMPLED_IMAGE_FILTER_LINEAR_BIT) != 0;
}

} // namespace

GpuTexture::~GpuTexture()
{
    destroy();
}

bool GpuTexture::create(VulkanDevice&       device,
                        GpuUploader&        uploader,
                        VulkanBindless&     bindless,
                        const TextureAsset& asset,
                        TextureColorSpace   colorSpace,
                        bool                generateMips,
                        const char*         debugName)
{
    if (asset.empty() || asset.width == 0 || asset.height == 0) {
        return false;
    }
    return upload(device, uploader, bindless, asset.pixels.data(),
                  asset.width, asset.height, colorSpace, generateMips, debugName);
}

bool GpuTexture::createSolid(VulkanDevice&     device,
                             GpuUploader&      uploader,
                             VulkanBindless&   bindless,
                             std::uint32_t     rgba,
                             TextureColorSpace colorSpace,
                             const char*       debugName)
{
    return upload(device, uploader, bindless, &rgba, 1, 1, colorSpace, false, debugName);
}

bool GpuTexture::upload(VulkanDevice&     device,
                        GpuUploader&      uploader,
                        VulkanBindless&   bindless,
                        const void*       pixels,
                        std::uint32_t     width,
                        std::uint32_t     height,
                        TextureColorSpace colorSpace,
                        bool              generateMips,
                        const char*       debugName)
{
    device_   = &device;
    bindless_ = &bindless;
    width_    = width;
    height_   = height;

    format_ = colorSpace == TextureColorSpace::Srgb ? VK_FORMAT_R8G8B8A8_SRGB
                                                    : VK_FORMAT_R8G8B8A8_UNORM;

    // The blit chain needs linear filtering on this format; without it a single
    // level is the honest answer rather than a broken chain.
    if (generateMips && !supportsLinearBlit(device.physicalDevice(), format_)) {
        generateMips = false;
    }

    mipLevels_ = 1;
    if (generateMips) {
        std::uint32_t largest = width > height ? width : height;
        while (largest > 1) {
            largest /= 2;
            ++mipLevels_;
        }
    }

    VkImageCreateInfo imageInfo{};
    imageInfo.sType         = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
    imageInfo.imageType     = VK_IMAGE_TYPE_2D;
    imageInfo.format        = format_;
    imageInfo.extent        = VkExtent3D{width, height, 1};
    imageInfo.mipLevels     = mipLevels_;
    imageInfo.arrayLayers   = 1;
    imageInfo.samples       = VK_SAMPLE_COUNT_1_BIT;
    imageInfo.tiling        = VK_IMAGE_TILING_OPTIMAL;
    imageInfo.usage         = VK_IMAGE_USAGE_SAMPLED_BIT
                            | VK_IMAGE_USAGE_TRANSFER_DST_BIT
                            | VK_IMAGE_USAGE_TRANSFER_SRC_BIT; // blit reads level n-1
    imageInfo.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;

    VmaAllocationCreateInfo allocInfo{};
    allocInfo.usage = VMA_MEMORY_USAGE_AUTO;

    if (vmaCreateImage(device.allocator(), &imageInfo, &allocInfo,
                       &image_, &allocation_, nullptr) != VK_SUCCESS) {
        destroy();
        return false;
    }

    // --- staging ------------------------------------------------------------
    const VkDeviceSize sizeBytes = VkDeviceSize{width} * height * 4;

    BufferDesc stagingDesc;
    stagingDesc.size      = sizeBytes;
    stagingDesc.usage     = VK_BUFFER_USAGE_TRANSFER_SRC_BIT;
    stagingDesc.memory    = BufferMemory::HostVisible;
    stagingDesc.debugName = "Staging_Texture";

    VulkanBuffer staging;
    if (!staging.create(device, stagingDesc) || staging.mapped() == nullptr) {
        destroy();
        return false;
    }
    std::memcpy(staging.mapped(), pixels, static_cast<std::size_t>(sizeBytes));

    // --- copy and mip chain -------------------------------------------------
    VkCommandPool pool = VK_NULL_HANDLE;
    VkCommandPoolCreateInfo poolInfo{};
    poolInfo.sType            = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
    poolInfo.flags            = VK_COMMAND_POOL_CREATE_TRANSIENT_BIT;
    poolInfo.queueFamilyIndex = device.graphics().family;
    HARPIA_VK_CHECK(vkCreateCommandPool(device.device(), &poolInfo, nullptr, &pool));

    VkCommandBuffer cmd = VK_NULL_HANDLE;
    VkCommandBufferAllocateInfo cmdAlloc{};
    cmdAlloc.sType              = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    cmdAlloc.commandPool        = pool;
    cmdAlloc.level              = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    cmdAlloc.commandBufferCount = 1;
    HARPIA_VK_CHECK(vkAllocateCommandBuffers(device.device(), &cmdAlloc, &cmd));

    VkCommandBufferBeginInfo begin{};
    begin.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    HARPIA_VK_CHECK(vkBeginCommandBuffer(cmd, &begin));

    // Per-level barriers, because the chain moves each level between
    // TRANSFER_DST and TRANSFER_SRC independently.
    const auto barrierLevel = [&](std::uint32_t level,
                                  VkImageLayout oldLayout, VkImageLayout newLayout,
                                  VkPipelineStageFlags2 srcStage, VkAccessFlags2 srcAccess,
                                  VkPipelineStageFlags2 dstStage, VkAccessFlags2 dstAccess) {
        VkImageMemoryBarrier2 barrier{};
        barrier.sType         = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER_2;
        barrier.srcStageMask  = srcStage;
        barrier.srcAccessMask = srcAccess;
        barrier.dstStageMask  = dstStage;
        barrier.dstAccessMask = dstAccess;
        barrier.oldLayout     = oldLayout;
        barrier.newLayout     = newLayout;
        barrier.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        barrier.image         = image_;
        barrier.subresourceRange.aspectMask   = VK_IMAGE_ASPECT_COLOR_BIT;
        barrier.subresourceRange.baseMipLevel = level;
        barrier.subresourceRange.levelCount   = 1;
        barrier.subresourceRange.layerCount   = 1;

        VkDependencyInfo dependency{};
        dependency.sType                   = VK_STRUCTURE_TYPE_DEPENDENCY_INFO;
        dependency.imageMemoryBarrierCount = 1;
        dependency.pImageMemoryBarriers    = &barrier;
        vkCmdPipelineBarrier2(cmd, &dependency);
    };

    barrierLevel(0, VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                 VK_PIPELINE_STAGE_2_TOP_OF_PIPE_BIT, 0,
                 VK_PIPELINE_STAGE_2_COPY_BIT, VK_ACCESS_2_TRANSFER_WRITE_BIT);

    VkBufferImageCopy region{};
    region.imageSubresource.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    region.imageSubresource.layerCount = 1;
    region.imageExtent = VkExtent3D{width, height, 1};
    vkCmdCopyBufferToImage(cmd, staging.handle(), image_,
                           VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);

    std::int32_t mipWidth  = static_cast<std::int32_t>(width);
    std::int32_t mipHeight = static_cast<std::int32_t>(height);

    for (std::uint32_t level = 1; level < mipLevels_; ++level) {
        // Source level becomes readable, destination becomes writable.
        barrierLevel(level - 1, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                     VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                     VK_PIPELINE_STAGE_2_COPY_BIT, VK_ACCESS_2_TRANSFER_WRITE_BIT,
                     VK_PIPELINE_STAGE_2_BLIT_BIT, VK_ACCESS_2_TRANSFER_READ_BIT);
        barrierLevel(level, VK_IMAGE_LAYOUT_UNDEFINED,
                     VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                     VK_PIPELINE_STAGE_2_TOP_OF_PIPE_BIT, 0,
                     VK_PIPELINE_STAGE_2_BLIT_BIT, VK_ACCESS_2_TRANSFER_WRITE_BIT);

        const std::int32_t nextWidth  = mipWidth > 1 ? mipWidth / 2 : 1;
        const std::int32_t nextHeight = mipHeight > 1 ? mipHeight / 2 : 1;

        VkImageBlit blit{};
        blit.srcOffsets[1] = VkOffset3D{mipWidth, mipHeight, 1};
        blit.srcSubresource.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
        blit.srcSubresource.mipLevel   = level - 1;
        blit.srcSubresource.layerCount = 1;
        blit.dstOffsets[1] = VkOffset3D{nextWidth, nextHeight, 1};
        blit.dstSubresource.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
        blit.dstSubresource.mipLevel   = level;
        blit.dstSubresource.layerCount = 1;

        vkCmdBlitImage(cmd,
                       image_, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                       image_, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                       1, &blit, VK_FILTER_LINEAR);

        // The level we just read from is finished; hand it to the shader.
        barrierLevel(level - 1, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                     VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                     VK_PIPELINE_STAGE_2_BLIT_BIT, VK_ACCESS_2_TRANSFER_READ_BIT,
                     VK_PIPELINE_STAGE_2_FRAGMENT_SHADER_BIT,
                     VK_ACCESS_2_SHADER_SAMPLED_READ_BIT);

        mipWidth  = nextWidth;
        mipHeight = nextHeight;
    }

    // The last level was never a blit source, so it is still TRANSFER_DST.
    barrierLevel(mipLevels_ - 1, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                 VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                 VK_PIPELINE_STAGE_2_COPY_BIT | VK_PIPELINE_STAGE_2_BLIT_BIT,
                 VK_ACCESS_2_TRANSFER_WRITE_BIT,
                 VK_PIPELINE_STAGE_2_FRAGMENT_SHADER_BIT,
                 VK_ACCESS_2_SHADER_SAMPLED_READ_BIT);

    HARPIA_VK_CHECK(vkEndCommandBuffer(cmd));

    VkCommandBufferSubmitInfo cmdSubmit{};
    cmdSubmit.sType         = VK_STRUCTURE_TYPE_COMMAND_BUFFER_SUBMIT_INFO;
    cmdSubmit.commandBuffer = cmd;

    VkSubmitInfo2 submit{};
    submit.sType                  = VK_STRUCTURE_TYPE_SUBMIT_INFO_2;
    submit.commandBufferInfoCount = 1;
    submit.pCommandBufferInfos    = &cmdSubmit;

    HARPIA_VK_CHECK(vkQueueSubmit2(device.graphics().queue, 1, &submit, VK_NULL_HANDLE));
    HARPIA_VK_CHECK(vkQueueWaitIdle(device.graphics().queue));

    vkDestroyCommandPool(device.device(), pool, nullptr);
    (void)uploader; // the chain needs its own command buffer, not the shared path

    // --- view and bindless slot --------------------------------------------
    VkImageViewCreateInfo viewInfo{};
    viewInfo.sType    = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
    viewInfo.image    = image_;
    viewInfo.viewType = VK_IMAGE_VIEW_TYPE_2D;
    viewInfo.format   = format_;
    viewInfo.subresourceRange.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    viewInfo.subresourceRange.levelCount = mipLevels_;
    viewInfo.subresourceRange.layerCount = 1;
    HARPIA_VK_CHECK(vkCreateImageView(device.device(), &viewInfo, nullptr, &view_));

    setDebugName(device.device(), VK_OBJECT_TYPE_IMAGE, image_, debugName);
    setDebugName(device.device(), VK_OBJECT_TYPE_IMAGE_VIEW, view_, debugName);

    bindlessIndex_ = bindless.registerSampledImage(
        view_, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    if (bindlessIndex_ == VulkanBindless::kInvalidIndex) {
        std::fprintf(stderr, "[texture] bindless sampled image slots exhausted\n");
        destroy();
        return false;
    }
    return true;
}

void GpuTexture::destroy()
{
    if (bindless_ != nullptr && bindlessIndex_ != VulkanBindless::kInvalidIndex) {
        bindless_->releaseSampledImage(bindlessIndex_);
        bindlessIndex_ = VulkanBindless::kInvalidIndex;
    }
    if (device_ != nullptr) {
        if (view_ != VK_NULL_HANDLE) {
            vkDestroyImageView(device_->device(), view_, nullptr);
            view_ = VK_NULL_HANDLE;
        }
        if (image_ != VK_NULL_HANDLE) {
            vmaDestroyImage(device_->allocator(), image_, allocation_);
            image_ = VK_NULL_HANDLE;
            allocation_ = nullptr;
        }
    }
    device_    = nullptr;
    bindless_  = nullptr;
    width_     = 0;
    height_    = 0;
    mipLevels_ = 1;
}

} // namespace harpia::rhi
