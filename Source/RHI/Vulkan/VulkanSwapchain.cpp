#include "RHI/Vulkan/VulkanSwapchain.h"

#include "RHI/Vulkan/VulkanDevice.h"

#include <algorithm>
#include <cstdio>

namespace harpia::rhi {

VulkanSwapchain::~VulkanSwapchain()
{
    destroy();
}

bool VulkanSwapchain::create(const VulkanDevice& device,
                             std::uint32_t       width,
                             std::uint32_t       height,
                             bool                vsync)
{
    device_ = &device;
    vsync_  = vsync;

    if (device.surface() == VK_NULL_HANDLE) {
        std::fprintf(stderr, "[vulkan] cannot create a swapchain without a surface\n");
        return false;
    }
    return build(width, height, VK_NULL_HANDLE);
}

bool VulkanSwapchain::recreate(std::uint32_t width, std::uint32_t height)
{
    if (device_ == nullptr) {
        return false;
    }

    vkDeviceWaitIdle(device_->device());

    VkSwapchainKHR old = swapchain_;
    destroyViews();
    swapchain_ = VK_NULL_HANDLE;

    const bool ok = build(width, height, old);

    if (old != VK_NULL_HANDLE) {
        vkDestroySwapchainKHR(device_->device(), old, nullptr);
    }
    return ok;
}

bool VulkanSwapchain::build(std::uint32_t width, std::uint32_t height,
                            VkSwapchainKHR oldSwapchain)
{
    const VkPhysicalDevice physical = device_->physicalDevice();
    const VkSurfaceKHR     surface  = device_->surface();
    const VkDevice         device   = device_->device();

    VkSurfaceCapabilitiesKHR caps{};
    HARPIA_VK_CHECK(vkGetPhysicalDeviceSurfaceCapabilitiesKHR(physical, surface, &caps));

    // --- format -----------------------------------------------------------
    std::uint32_t formatCount = 0;
    vkGetPhysicalDeviceSurfaceFormatsKHR(physical, surface, &formatCount, nullptr);
    std::vector<VkSurfaceFormatKHR> formats(formatCount);
    vkGetPhysicalDeviceSurfaceFormatsKHR(physical, surface, &formatCount, formats.data());

    if (formats.empty()) {
        std::fprintf(stderr, "[vulkan] surface reports no formats\n");
        return false;
    }

    VkSurfaceFormatKHR chosen = formats[0];
    for (const VkSurfaceFormatKHR& candidate : formats) {
        // UNORM, not SRGB: tonemapping writes final sRGB values itself, so an
        // sRGB swapchain would apply the curve twice.
        if (candidate.format == VK_FORMAT_B8G8R8A8_UNORM
            && candidate.colorSpace == VK_COLOR_SPACE_SRGB_NONLINEAR_KHR) {
            chosen = candidate;
            break;
        }
    }
    format_     = chosen.format;
    colorSpace_ = chosen.colorSpace;

    // --- present mode -----------------------------------------------------
    std::uint32_t modeCount = 0;
    vkGetPhysicalDeviceSurfacePresentModesKHR(physical, surface, &modeCount, nullptr);
    std::vector<VkPresentModeKHR> modes(modeCount);
    vkGetPhysicalDeviceSurfacePresentModesKHR(physical, surface, &modeCount, modes.data());

    presentMode_ = VK_PRESENT_MODE_FIFO_KHR; // always available
    if (!vsync_) {
        const auto hasMode = [&modes](VkPresentModeKHR mode) {
            return std::find(modes.begin(), modes.end(), mode) != modes.end();
        };
        if (hasMode(VK_PRESENT_MODE_MAILBOX_KHR)) {
            presentMode_ = VK_PRESENT_MODE_MAILBOX_KHR;
        } else if (hasMode(VK_PRESENT_MODE_IMMEDIATE_KHR)) {
            presentMode_ = VK_PRESENT_MODE_IMMEDIATE_KHR;
        }
    }

    // --- extent -----------------------------------------------------------
    if (caps.currentExtent.width != 0xFFFFFFFFu) {
        extent_ = caps.currentExtent;
    } else {
        extent_.width  = std::clamp(width, caps.minImageExtent.width, caps.maxImageExtent.width);
        extent_.height = std::clamp(height, caps.minImageExtent.height, caps.maxImageExtent.height);
    }

    if (extent_.width == 0 || extent_.height == 0) {
        return false; // minimised; caller retries when the window comes back
    }

    std::uint32_t imageCount = caps.minImageCount + 1;
    if (caps.maxImageCount > 0 && imageCount > caps.maxImageCount) {
        imageCount = caps.maxImageCount;
    }

    VkSwapchainCreateInfoKHR info{};
    info.sType            = VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR;
    info.surface          = surface;
    info.minImageCount    = imageCount;
    info.imageFormat      = format_;
    info.imageColorSpace  = colorSpace_;
    info.imageExtent      = extent_;
    info.imageArrayLayers = 1;
    info.imageUsage       = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT
                          | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    info.imageSharingMode = VK_SHARING_MODE_EXCLUSIVE; // graphics == present family
    info.preTransform     = caps.currentTransform;
    info.compositeAlpha   = VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR;
    info.presentMode      = presentMode_;
    info.clipped          = VK_TRUE;
    info.oldSwapchain     = oldSwapchain;

    const VkResult result = vkCreateSwapchainKHR(device, &info, nullptr, &swapchain_);
    if (result != VK_SUCCESS) {
        std::fprintf(stderr, "[vulkan] vkCreateSwapchainKHR failed: %s\n",
                     resultToString(result));
        return false;
    }
    setDebugName(device, VK_OBJECT_TYPE_SWAPCHAIN_KHR, swapchain_, "Swapchain_Main");

    std::uint32_t actualCount = 0;
    vkGetSwapchainImagesKHR(device, swapchain_, &actualCount, nullptr);
    images_.resize(actualCount);
    vkGetSwapchainImagesKHR(device, swapchain_, &actualCount, images_.data());

    views_.resize(actualCount);
    for (std::uint32_t i = 0; i < actualCount; ++i) {
        VkImageViewCreateInfo viewInfo{};
        viewInfo.sType    = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
        viewInfo.image    = images_[i];
        viewInfo.viewType = VK_IMAGE_VIEW_TYPE_2D;
        viewInfo.format   = format_;
        viewInfo.subresourceRange.aspectMask     = VK_IMAGE_ASPECT_COLOR_BIT;
        viewInfo.subresourceRange.levelCount     = 1;
        viewInfo.subresourceRange.layerCount     = 1;
        HARPIA_VK_CHECK(vkCreateImageView(device, &viewInfo, nullptr, &views_[i]));

        char name[48];
        std::snprintf(name, sizeof(name), "Swapchain_ImageView_%u", i);
        setDebugName(device, VK_OBJECT_TYPE_IMAGE_VIEW, views_[i], name);
    }

    return true;
}

void VulkanSwapchain::destroyViews()
{
    if (device_ == nullptr) {
        return;
    }
    for (VkImageView view : views_) {
        if (view != VK_NULL_HANDLE) {
            vkDestroyImageView(device_->device(), view, nullptr);
        }
    }
    views_.clear();
    images_.clear();
}

void VulkanSwapchain::destroy()
{
    if (device_ == nullptr) {
        return;
    }
    destroyViews();
    if (swapchain_ != VK_NULL_HANDLE) {
        vkDestroySwapchainKHR(device_->device(), swapchain_, nullptr);
        swapchain_ = VK_NULL_HANDLE;
    }
    device_ = nullptr;
}

VkResult VulkanSwapchain::acquireNext(VkSemaphore signal, std::uint32_t& outImageIndex)
{
    return vkAcquireNextImageKHR(device_->device(), swapchain_, UINT64_MAX,
                                 signal, VK_NULL_HANDLE, &outImageIndex);
}

VkResult VulkanSwapchain::present(VkQueue queue, VkSemaphore wait, std::uint32_t imageIndex)
{
    VkPresentInfoKHR info{};
    info.sType              = VK_STRUCTURE_TYPE_PRESENT_INFO_KHR;
    info.waitSemaphoreCount = wait != VK_NULL_HANDLE ? 1u : 0u;
    info.pWaitSemaphores    = wait != VK_NULL_HANDLE ? &wait : nullptr;
    info.swapchainCount     = 1;
    info.pSwapchains        = &swapchain_;
    info.pImageIndices      = &imageIndex;

    return vkQueuePresentKHR(queue, &info);
}

} // namespace harpia::rhi
