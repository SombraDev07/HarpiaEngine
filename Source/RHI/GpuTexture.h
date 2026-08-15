// Harpia Engine — texture on the GPU
//
// The sRGB decision is made here rather than at import, because it depends on
// what a texture *means*: base colour is sRGB, a normal or roughness map is
// linear, and the same PNG can be either. Getting it wrong makes lighting
// subtly and consistently wrong — the kind of error that gets compensated for
// in art instead of fixed.
//
// Mip levels are generated on the GPU with a blit chain. That is fine for
// import-time work; the offline cook (roadmap 1.7 stage 3) will ship them
// precomputed and this path becomes the fallback for raw source files.
#pragma once

#include "Core/Assets/TextureAsset.h"
#include "RHI/Vulkan/VulkanBindless.h"
#include "RHI/Vulkan/VulkanBuffer.h"

#include <cstdint>

struct VmaAllocation_T;
using VmaAllocation = VmaAllocation_T*;

namespace harpia::rhi {

class VulkanDevice;

enum class TextureColorSpace : std::uint8_t {
    Srgb,    // base colour, emissive — anything authored as a colour
    Linear,  // normal, roughness, metallic, occlusion — anything authored as data
};

class GpuTexture {
public:
    GpuTexture() = default;
    ~GpuTexture();

    GpuTexture(const GpuTexture&)            = delete;
    GpuTexture& operator=(const GpuTexture&) = delete;

    [[nodiscard]] bool create(VulkanDevice&       device,
                              GpuUploader&        uploader,
                              VulkanBindless&     bindless,
                              const TextureAsset& asset,
                              TextureColorSpace   colorSpace,
                              bool                generateMips = true,
                              const char*         debugName = "Texture");

    // A 1x1 texture, for a material slot with no map. Cheaper than branching in
    // the shader and keeps every material on one code path.
    [[nodiscard]] bool createSolid(VulkanDevice&     device,
                                   GpuUploader&      uploader,
                                   VulkanBindless&   bindless,
                                   std::uint32_t     rgba,
                                   TextureColorSpace colorSpace,
                                   const char*       debugName = "SolidTexture");

    void destroy();

    [[nodiscard]] std::uint32_t bindlessIndex() const noexcept { return bindlessIndex_; }
    [[nodiscard]] VkImage       image() const noexcept  { return image_; }
    [[nodiscard]] VkImageView   view() const noexcept   { return view_; }
    [[nodiscard]] VkFormat      format() const noexcept { return format_; }
    [[nodiscard]] std::uint32_t width() const noexcept  { return width_; }
    [[nodiscard]] std::uint32_t height() const noexcept { return height_; }
    [[nodiscard]] std::uint32_t mipLevels() const noexcept { return mipLevels_; }
    [[nodiscard]] bool          valid() const noexcept { return image_ != VK_NULL_HANDLE; }

private:
    [[nodiscard]] bool upload(VulkanDevice&    device,
                              GpuUploader&     uploader,
                              VulkanBindless&  bindless,
                              const void*      pixels,
                              std::uint32_t    width,
                              std::uint32_t    height,
                              TextureColorSpace colorSpace,
                              bool             generateMips,
                              const char*      debugName);

    VulkanDevice*   device_   = nullptr;
    VulkanBindless* bindless_ = nullptr;

    VkImage       image_      = VK_NULL_HANDLE;
    VkImageView   view_       = VK_NULL_HANDLE;
    VmaAllocation allocation_ = nullptr;

    VkFormat      format_    = VK_FORMAT_UNDEFINED;
    std::uint32_t width_     = 0;
    std::uint32_t height_    = 0;
    std::uint32_t mipLevels_ = 1;
    std::uint32_t bindlessIndex_ = VulkanBindless::kInvalidIndex;
};

} // namespace harpia::rhi
