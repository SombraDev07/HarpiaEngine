// Harpia Engine — image-based lighting resources
//
// Owns the split-sum BRDF table, which is generated once at startup and never
// changes: it depends only on NoV and roughness, not on the material or the
// environment. Every material in the engine reads the same texture.
//
// A prefiltered environment cubemap belongs here too once an HDR loader exists;
// the split-sum maths and this table are identical either way.
#pragma once

#include "Core/Assets/HdrImage.h"
#include "RHI/Vulkan/VulkanBindless.h"
#include "RHI/Vulkan/VulkanCommon.h"

#include <array>
#include <cstdint>
#include <string>

struct VmaAllocation_T;
using VmaAllocation = VmaAllocation_T*;

namespace harpia::rhi {

class VulkanDevice;

class IblResources {
public:
    // 256 is the size Karis used and what every implementation since has
    // settled on: the function is smooth enough that more resolution buys
    // nothing measurable.
    static constexpr std::uint32_t kBrdfLutSize = 256;

    IblResources() = default;
    ~IblResources();

    IblResources(const IblResources&)            = delete;
    IblResources& operator=(const IblResources&) = delete;

    // Face edge at mip 0. Radiance is low-frequency by nature — the detail a
    // reflection shows comes from the roughness mip it lands on, not from
    // resolution here, and 128 is where added size stops being visible.
    static constexpr std::uint32_t kEnvironmentSize = 128;

    static constexpr std::uint32_t kCubeFaces = 6;

    // Renders the table and registers it in the bindless heap.
    [[nodiscard]] bool create(VulkanDevice&      device,
                              VulkanBindless&    bindless,
                              const std::string& shaderDirectory);

    // Projects an equirectangular radiance map onto a cube. Separate from
    // create() because the table has no environment to depend on: it exists
    // whether or not an .hdr was ever loaded, which is what lets a scene render
    // with no environment at all.
    [[nodiscard]] bool loadEnvironment(VulkanDevice&        device,
                                       VulkanBindless&      bindless,
                                       const HdrImageAsset& equirect,
                                       const std::string&   shaderDirectory);

    void destroy();

    [[nodiscard]] std::uint32_t brdfLutIndex() const noexcept { return brdfLutIndex_; }
    [[nodiscard]] VkImage       brdfLutImage() const noexcept { return image_; }
    [[nodiscard]] VkImageView   brdfLutView() const noexcept  { return view_; }
    [[nodiscard]] bool          valid() const noexcept { return image_ != VK_NULL_HANDLE; }

    [[nodiscard]] std::uint32_t environmentIndex() const noexcept { return envIndex_; }
    [[nodiscard]] VkImage       environmentImage() const noexcept { return envImage_; }
    [[nodiscard]] bool hasEnvironment() const noexcept { return envImage_ != VK_NULL_HANDLE; }

private:
    void destroyEnvironment();

    VulkanDevice*   device_   = nullptr;
    VulkanBindless* bindless_ = nullptr;

    VkImage       image_      = VK_NULL_HANDLE;
    VkImageView   view_       = VK_NULL_HANDLE;
    VmaAllocation allocation_ = nullptr;
    std::uint32_t brdfLutIndex_ = VulkanBindless::kInvalidIndex;

    VkImage       envImage_      = VK_NULL_HANDLE;
    VkImageView   envCubeView_   = VK_NULL_HANDLE;
    VmaAllocation envAllocation_ = nullptr;
    std::uint32_t envIndex_      = VulkanBindless::kInvalidIndex;
    std::array<VkImageView, kCubeFaces> envFaceViews_{};
};

} // namespace harpia::rhi
