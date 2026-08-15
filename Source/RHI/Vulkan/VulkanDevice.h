// Harpia Engine — Vulkan device
//
// Roadmap 1.4: one backend, no RHI abstraction. Vulkan 1.3 with dynamic
// rendering, synchronization2 and descriptor indexing required up front —
// these are the features the whole renderer is designed around, so a device
// that lacks them is not a device we support.
#pragma once

#include "RHI/Vulkan/VulkanCommon.h"

#include <cstdint>
#include <string>
#include <vector>

struct GLFWwindow;
struct VmaAllocator_T;
using VmaAllocator = VmaAllocator_T*;

namespace harpia::rhi {

struct DeviceDesc {
    const char* applicationName = "Harpia";
    bool        enableValidation = true;

    // Null runs headless: no surface, no swapchain, offscreen rendering only.
    GLFWwindow* window = nullptr;
};

struct QueueInfo {
    VkQueue       queue  = VK_NULL_HANDLE;
    std::uint32_t family = VK_QUEUE_FAMILY_IGNORED;
};

class VulkanDevice {
public:
    VulkanDevice() = default;
    ~VulkanDevice();

    VulkanDevice(const VulkanDevice&)            = delete;
    VulkanDevice& operator=(const VulkanDevice&) = delete;

    [[nodiscard]] bool create(const DeviceDesc& desc);
    void destroy();

    [[nodiscard]] VkInstance         instance() const noexcept       { return instance_; }
    [[nodiscard]] VkPhysicalDevice   physicalDevice() const noexcept { return physical_; }
    [[nodiscard]] VkDevice           device() const noexcept         { return device_; }
    [[nodiscard]] VkSurfaceKHR       surface() const noexcept        { return surface_; }
    [[nodiscard]] VmaAllocator       allocator() const noexcept      { return allocator_; }

    [[nodiscard]] const QueueInfo& graphics() const noexcept { return graphics_; }
    [[nodiscard]] const QueueInfo& compute() const noexcept  { return compute_; }
    [[nodiscard]] const QueueInfo& transfer() const noexcept { return transfer_; }

    [[nodiscard]] const std::string& deviceName() const noexcept { return deviceName_; }
    [[nodiscard]] bool headless() const noexcept { return surface_ == VK_NULL_HANDLE; }

    void waitIdle() const;

    // Rule: this must read zero at the end of every run. Tests assert on it,
    // which turns "validation layers em zero" from a hope into a gate.
    [[nodiscard]] static std::uint64_t validationErrorCount() noexcept;
    static void resetValidationErrorCount() noexcept;

    // Limits the bindless heap sizes are clamped against.
    struct Limits {
        std::uint32_t maxSampledImages = 0;
        std::uint32_t maxStorageBuffers = 0;
        std::uint32_t maxSamplers = 0;
    };
    [[nodiscard]] const Limits& limits() const noexcept { return limits_; }

private:
    bool createInstance(const DeviceDesc& desc);
    bool createDebugMessenger();
    bool createSurface(GLFWwindow* window);
    bool pickPhysicalDevice();
    bool createLogicalDevice();
    bool createAllocator();

    VkInstance               instance_  = VK_NULL_HANDLE;
    VkDebugUtilsMessengerEXT messenger_ = VK_NULL_HANDLE;
    VkSurfaceKHR             surface_   = VK_NULL_HANDLE;
    VkPhysicalDevice         physical_  = VK_NULL_HANDLE;
    VkDevice                 device_    = VK_NULL_HANDLE;
    VmaAllocator             allocator_ = nullptr;

    QueueInfo graphics_;
    QueueInfo compute_;
    QueueInfo transfer_;

    std::string deviceName_;
    Limits      limits_;
    bool        validationEnabled_ = false;
};

} // namespace harpia::rhi
