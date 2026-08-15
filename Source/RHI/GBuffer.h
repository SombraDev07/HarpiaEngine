// Harpia Engine — GBuffer layout
//
// Roadmap F2 step 2. Four targets plus depth:
//
//   Albedo    RGBA8      albedo.rgb, occlusion in a
//   Normal    RG16 snorm octahedral — a unit vector needs two channels, not
//                        three, and octahedral covers both hemispheres unlike
//                        storing xy and reconstructing z
//   Material  RG8        roughness, metallic
//   Motion    RG16 float screen-space motion in UV units
//   Depth     D32        reverse-Z
//
// Motion vectors are here from the first GBuffer rather than arriving with TAA
// in F6. They are only *used* later, but adding them later means reopening
// every geometry shader in the engine — the most expensive debt in the roadmap.
#pragma once

#include "RHI/RenderGraph/RenderGraph.h"
#include "RHI/Vulkan/VulkanCommon.h"

#include <array>
#include <cstdint>

namespace harpia::rhi {

class VulkanDevice;

enum class GBufferTarget : std::uint32_t {
    Albedo = 0,
    Normal,
    Material,
    Motion,
    Count
};

// Formats are resolved against the device once, because the preferred normal
// format is not universally supported as a colour attachment.
struct GBufferFormats {
    VkFormat albedo   = VK_FORMAT_R8G8B8A8_UNORM;
    VkFormat normal   = VK_FORMAT_R16G16_SNORM;
    VkFormat material = VK_FORMAT_R8G8_UNORM;
    VkFormat motion   = VK_FORMAT_R16G16_SFLOAT;
    VkFormat depth    = VK_FORMAT_D32_SFLOAT;

    // Picks the best supported format for each slot on this device.
    [[nodiscard]] static GBufferFormats select(const VulkanDevice& device);

    [[nodiscard]] std::array<VkFormat, static_cast<std::size_t>(GBufferTarget::Count)>
    colorFormats() const noexcept
    {
        return {albedo, normal, material, motion};
    }
};

// Graph handles for one frame's GBuffer.
struct GBufferHandles {
    std::array<RgHandle, static_cast<std::size_t>(GBufferTarget::Count)> color{
        kRgInvalid, kRgInvalid, kRgInvalid, kRgInvalid};
    RgHandle depth = kRgInvalid;

    [[nodiscard]] RgHandle operator[](GBufferTarget target) const noexcept
    {
        return color[static_cast<std::size_t>(target)];
    }
};

// Declares the transient targets and the writes for a GBuffer pass. Called from
// inside a pass setup callback.
[[nodiscard]] GBufferHandles declareGBuffer(RgBuilder&            builder,
                                            const GBufferFormats& formats,
                                            std::uint32_t         width,
                                            std::uint32_t         height);

} // namespace harpia::rhi
