// Harpia Engine — image-based lighting resources
//
// Owns the split-sum BRDF table, which is generated once at startup and never
// changes: it depends only on NoV and roughness, not on the material or the
// environment. Every material in the engine reads the same texture.
//
// A prefiltered environment cubemap belongs here too once an HDR loader exists;
// the split-sum maths and this table are identical either way.
#pragma once

#include "RHI/Vulkan/VulkanBindless.h"
#include "RHI/Vulkan/VulkanCommon.h"

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

    // Renders the table and registers it in the bindless heap.
    [[nodiscard]] bool create(VulkanDevice&      device,
                              VulkanBindless&    bindless,
                              const std::string& shaderDirectory);
    void destroy();

    [[nodiscard]] std::uint32_t brdfLutIndex() const noexcept { return brdfLutIndex_; }
    [[nodiscard]] VkImage       brdfLutImage() const noexcept { return image_; }
    [[nodiscard]] VkImageView   brdfLutView() const noexcept  { return view_; }
    [[nodiscard]] bool          valid() const noexcept { return image_ != VK_NULL_HANDLE; }

private:
    VulkanDevice*   device_   = nullptr;
    VulkanBindless* bindless_ = nullptr;

    VkImage       image_      = VK_NULL_HANDLE;
    VkImageView   view_       = VK_NULL_HANDLE;
    VmaAllocation allocation_ = nullptr;
    std::uint32_t brdfLutIndex_ = VulkanBindless::kInvalidIndex;
};

} // namespace harpia::rhi
