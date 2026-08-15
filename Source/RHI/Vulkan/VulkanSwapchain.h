// Harpia Engine — swapchain
#pragma once

#include "RHI/Vulkan/VulkanCommon.h"

#include <cstdint>
#include <vector>

namespace harpia::rhi {

class VulkanDevice;

class VulkanSwapchain {
public:
    VulkanSwapchain() = default;
    ~VulkanSwapchain();

    VulkanSwapchain(const VulkanSwapchain&)            = delete;
    VulkanSwapchain& operator=(const VulkanSwapchain&) = delete;

    [[nodiscard]] bool create(const VulkanDevice& device,
                              std::uint32_t       width,
                              std::uint32_t       height,
                              bool                vsync = true);
    void destroy();

    // Tears down and rebuilds in place, reusing the old swapchain so the
    // driver can keep presenting during the transition.
    [[nodiscard]] bool recreate(std::uint32_t width, std::uint32_t height);

    // VK_ERROR_OUT_OF_DATE_KHR / VK_SUBOPTIMAL_KHR are returned rather than
    // asserted: a resize is normal, not a failure.
    [[nodiscard]] VkResult acquireNext(VkSemaphore signal, std::uint32_t& outImageIndex);
    [[nodiscard]] VkResult present(VkQueue queue, VkSemaphore wait, std::uint32_t imageIndex);

    [[nodiscard]] VkSwapchainKHR handle() const noexcept { return swapchain_; }
    [[nodiscard]] VkFormat       format() const noexcept { return format_; }
    [[nodiscard]] VkExtent2D     extent() const noexcept { return extent_; }
    [[nodiscard]] std::uint32_t  imageCount() const noexcept
    {
        return static_cast<std::uint32_t>(images_.size());
    }
    [[nodiscard]] VkImage     image(std::uint32_t index) const     { return images_[index]; }
    [[nodiscard]] VkImageView imageView(std::uint32_t index) const { return views_[index]; }

private:
    [[nodiscard]] bool build(std::uint32_t width, std::uint32_t height,
                             VkSwapchainKHR oldSwapchain);
    void destroyViews();

    const VulkanDevice* device_ = nullptr;

    VkSwapchainKHR           swapchain_ = VK_NULL_HANDLE;
    VkFormat                 format_    = VK_FORMAT_UNDEFINED;
    VkColorSpaceKHR          colorSpace_ = VK_COLOR_SPACE_SRGB_NONLINEAR_KHR;
    VkPresentModeKHR         presentMode_ = VK_PRESENT_MODE_FIFO_KHR;
    VkExtent2D               extent_{};
    bool                     vsync_ = true;

    std::vector<VkImage>     images_;
    std::vector<VkImageView> views_;
};

} // namespace harpia::rhi
