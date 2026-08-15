#include "RHI/Vulkan/VulkanDevice.h"

#include <GLFW/glfw3.h>
#include <vk_mem_alloc.h>

#include <algorithm>
#include <atomic>
#include <cstring>

namespace harpia::rhi {
namespace {

std::atomic<std::uint64_t> g_validationErrors{0};

constexpr const char* kValidationLayer = "VK_LAYER_KHRONOS_validation";

VKAPI_ATTR VkBool32 VKAPI_CALL debugCallback(
    VkDebugUtilsMessageSeverityFlagBitsEXT      severity,
    VkDebugUtilsMessageTypeFlagsEXT             /*types*/,
    const VkDebugUtilsMessengerCallbackDataEXT* data,
    void*                                       /*userData*/)
{
    if (data == nullptr) {
        return VK_FALSE;
    }

    const char* level = "info";
    if ((severity & VK_DEBUG_UTILS_MESSAGE_SEVERITY_ERROR_BIT_EXT) != 0) {
        level = "ERROR";
        g_validationErrors.fetch_add(1, std::memory_order_relaxed);
    } else if ((severity & VK_DEBUG_UTILS_MESSAGE_SEVERITY_WARNING_BIT_EXT) != 0) {
        level = "warning";
    } else {
        return VK_FALSE; // info/verbose is noise unless we are chasing something
    }

    std::fprintf(stderr, "[vulkan/%s] %s\n", level,
                 data->pMessage != nullptr ? data->pMessage : "<no message>");
    return VK_FALSE;
}

VkDebugUtilsMessengerCreateInfoEXT makeMessengerInfo()
{
    VkDebugUtilsMessengerCreateInfoEXT info{};
    info.sType = VK_STRUCTURE_TYPE_DEBUG_UTILS_MESSENGER_CREATE_INFO_EXT;
    info.messageSeverity = VK_DEBUG_UTILS_MESSAGE_SEVERITY_WARNING_BIT_EXT
                         | VK_DEBUG_UTILS_MESSAGE_SEVERITY_ERROR_BIT_EXT;
    info.messageType = VK_DEBUG_UTILS_MESSAGE_TYPE_GENERAL_BIT_EXT
                     | VK_DEBUG_UTILS_MESSAGE_TYPE_VALIDATION_BIT_EXT
                     | VK_DEBUG_UTILS_MESSAGE_TYPE_PERFORMANCE_BIT_EXT;
    info.pfnUserCallback = &debugCallback;
    return info;
}

[[nodiscard]] bool hasLayer(const char* name)
{
    std::uint32_t count = 0;
    vkEnumerateInstanceLayerProperties(&count, nullptr);
    std::vector<VkLayerProperties> layers(count);
    vkEnumerateInstanceLayerProperties(&count, layers.data());

    return std::any_of(layers.begin(), layers.end(), [name](const VkLayerProperties& l) {
        return std::strcmp(l.layerName, name) == 0;
    });
}

[[nodiscard]] bool hasInstanceExtension(const char* name)
{
    std::uint32_t count = 0;
    vkEnumerateInstanceExtensionProperties(nullptr, &count, nullptr);
    std::vector<VkExtensionProperties> extensions(count);
    vkEnumerateInstanceExtensionProperties(nullptr, &count, extensions.data());

    return std::any_of(extensions.begin(), extensions.end(),
                       [name](const VkExtensionProperties& e) {
                           return std::strcmp(e.extensionName, name) == 0;
                       });
}

[[nodiscard]] bool hasDeviceExtension(VkPhysicalDevice device, const char* name)
{
    std::uint32_t count = 0;
    vkEnumerateDeviceExtensionProperties(device, nullptr, &count, nullptr);
    std::vector<VkExtensionProperties> extensions(count);
    vkEnumerateDeviceExtensionProperties(device, nullptr, &count, extensions.data());

    return std::any_of(extensions.begin(), extensions.end(),
                       [name](const VkExtensionProperties& e) {
                           return std::strcmp(e.extensionName, name) == 0;
                       });
}

} // namespace

VulkanDevice::~VulkanDevice()
{
    destroy();
}

std::uint64_t VulkanDevice::validationErrorCount() noexcept
{
    return g_validationErrors.load(std::memory_order_relaxed);
}

void VulkanDevice::resetValidationErrorCount() noexcept
{
    g_validationErrors.store(0, std::memory_order_relaxed);
}

bool VulkanDevice::create(const DeviceDesc& desc)
{
    if (!createInstance(desc)) {
        return false;
    }
    if (validationEnabled_ && !createDebugMessenger()) {
        return false;
    }
    if (desc.window != nullptr && !createSurface(desc.window)) {
        return false;
    }
    if (!pickPhysicalDevice()) {
        return false;
    }
    if (!createLogicalDevice()) {
        return false;
    }
    if (!createAllocator()) {
        return false;
    }
    return true;
}

bool VulkanDevice::createInstance(const DeviceDesc& desc)
{
    VkApplicationInfo app{};
    app.sType              = VK_STRUCTURE_TYPE_APPLICATION_INFO;
    app.pApplicationName   = desc.applicationName;
    app.applicationVersion = VK_MAKE_VERSION(0, 1, 0);
    app.pEngineName        = "HarpiaEngine";
    app.engineVersion      = VK_MAKE_VERSION(0, 1, 0);
    app.apiVersion         = VK_API_VERSION_1_3;

    std::vector<const char*> extensions;
    std::vector<const char*> layers;

    if (desc.window != nullptr) {
        std::uint32_t glfwCount = 0;
        const char**  glfwExts  = glfwGetRequiredInstanceExtensions(&glfwCount);
        if (glfwExts == nullptr) {
            std::fprintf(stderr, "[vulkan] no window-system instance extensions available\n");
            return false;
        }
        extensions.insert(extensions.end(), glfwExts, glfwExts + glfwCount);
    }

    validationEnabled_ = desc.enableValidation
                      && hasLayer(kValidationLayer)
                      && hasInstanceExtension(VK_EXT_DEBUG_UTILS_EXTENSION_NAME);

    if (desc.enableValidation && !validationEnabled_) {
        std::fprintf(stderr, "[vulkan] validation requested but unavailable — continuing without it\n");
    }

    if (validationEnabled_) {
        layers.push_back(kValidationLayer);
        extensions.push_back(VK_EXT_DEBUG_UTILS_EXTENSION_NAME);
    }

    VkInstanceCreateInfo info{};
    info.sType                   = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO;
    info.pApplicationInfo        = &app;
    info.enabledExtensionCount   = static_cast<std::uint32_t>(extensions.size());
    info.ppEnabledExtensionNames = extensions.data();
    info.enabledLayerCount       = static_cast<std::uint32_t>(layers.size());
    info.ppEnabledLayerNames     = layers.data();

    // Chaining the messenger here catches problems raised during instance
    // creation and destruction, which the standalone messenger cannot see.
    VkDebugUtilsMessengerCreateInfoEXT messengerInfo = makeMessengerInfo();
    if (validationEnabled_) {
        info.pNext = &messengerInfo;
    }

    const VkResult result = vkCreateInstance(&info, nullptr, &instance_);
    if (result != VK_SUCCESS) {
        std::fprintf(stderr, "[vulkan] vkCreateInstance failed: %s\n", resultToString(result));
        return false;
    }
    return true;
}

bool VulkanDevice::createDebugMessenger()
{
    auto create = reinterpret_cast<PFN_vkCreateDebugUtilsMessengerEXT>(
        vkGetInstanceProcAddr(instance_, "vkCreateDebugUtilsMessengerEXT"));
    if (create == nullptr) {
        return true; // not fatal
    }

    const VkDebugUtilsMessengerCreateInfoEXT info = makeMessengerInfo();
    HARPIA_VK_CHECK(create(instance_, &info, nullptr, &messenger_));
    return true;
}

bool VulkanDevice::createSurface(GLFWwindow* window)
{
    const VkResult result = glfwCreateWindowSurface(instance_, window, nullptr, &surface_);
    if (result != VK_SUCCESS) {
        std::fprintf(stderr, "[vulkan] glfwCreateWindowSurface failed: %s\n",
                     resultToString(result));
        return false;
    }
    return true;
}

bool VulkanDevice::pickPhysicalDevice()
{
    std::uint32_t count = 0;
    vkEnumeratePhysicalDevices(instance_, &count, nullptr);
    if (count == 0) {
        std::fprintf(stderr, "[vulkan] no physical devices\n");
        return false;
    }

    std::vector<VkPhysicalDevice> devices(count);
    vkEnumeratePhysicalDevices(instance_, &count, devices.data());

    VkPhysicalDevice best      = VK_NULL_HANDLE;
    int              bestScore = -1;

    for (VkPhysicalDevice candidate : devices) {
        VkPhysicalDeviceProperties props{};
        vkGetPhysicalDeviceProperties(candidate, &props);

        if (props.apiVersion < VK_API_VERSION_1_3) {
            continue;
        }
        if (surface_ != VK_NULL_HANDLE
            && !hasDeviceExtension(candidate, VK_KHR_SWAPCHAIN_EXTENSION_NAME)) {
            continue;
        }

        // The renderer is designed around these; a device without them is not
        // a slower device, it is a different renderer.
        VkPhysicalDeviceVulkan13Features v13{};
        v13.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES;
        VkPhysicalDeviceVulkan12Features v12{};
        v12.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES;
        v12.pNext = &v13;
        VkPhysicalDeviceFeatures2 features{};
        features.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2;
        features.pNext = &v12;
        vkGetPhysicalDeviceFeatures2(candidate, &features);

        const bool supported = v13.dynamicRendering == VK_TRUE
                            && v13.synchronization2 == VK_TRUE
                            && v12.descriptorIndexing == VK_TRUE
                            && v12.runtimeDescriptorArray == VK_TRUE
                            && v12.descriptorBindingPartiallyBound == VK_TRUE
                            && v12.descriptorBindingSampledImageUpdateAfterBind == VK_TRUE
                            && v12.shaderSampledImageArrayNonUniformIndexing == VK_TRUE
                            && v12.bufferDeviceAddress == VK_TRUE
                            && v12.timelineSemaphore == VK_TRUE;
        if (!supported) {
            continue;
        }

        int score = 1;
        if (props.deviceType == VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU) {
            score += 1000;
        } else if (props.deviceType == VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU) {
            score += 100;
        }

        if (score > bestScore) {
            bestScore   = score;
            best        = candidate;
            deviceName_ = props.deviceName;

            limits_.maxSampledImages =
                props.limits.maxDescriptorSetSampledImages;
            limits_.maxStorageBuffers =
                props.limits.maxDescriptorSetStorageBuffers;
            limits_.maxSamplers =
                props.limits.maxDescriptorSetSamplers;
        }
    }

    if (best == VK_NULL_HANDLE) {
        std::fprintf(stderr,
                     "[vulkan] no device with Vulkan 1.3 + dynamic rendering + "
                     "sync2 + descriptor indexing\n");
        return false;
    }

    physical_ = best;
    return true;
}

bool VulkanDevice::createLogicalDevice()
{
    std::uint32_t familyCount = 0;
    vkGetPhysicalDeviceQueueFamilyProperties(physical_, &familyCount, nullptr);
    std::vector<VkQueueFamilyProperties> families(familyCount);
    vkGetPhysicalDeviceQueueFamilyProperties(physical_, &familyCount, families.data());

    constexpr std::uint32_t kNone = VK_QUEUE_FAMILY_IGNORED;

    // Graphics must also present when there is a surface, otherwise the
    // swapchain would need a second queue and an ownership transfer per frame.
    for (std::uint32_t i = 0; i < familyCount; ++i) {
        const bool isGraphics = (families[i].queueFlags & VK_QUEUE_GRAPHICS_BIT) != 0;
        if (!isGraphics) {
            continue;
        }
        if (surface_ != VK_NULL_HANDLE) {
            VkBool32 present = VK_FALSE;
            vkGetPhysicalDeviceSurfaceSupportKHR(physical_, i, surface_, &present);
            if (present != VK_TRUE) {
                continue;
            }
        }
        graphics_.family = i;
        break;
    }

    if (graphics_.family == kNone) {
        std::fprintf(stderr, "[vulkan] no graphics queue family with present support\n");
        return false;
    }

    // Prefer async queues on dedicated hardware families: a compute family
    // without graphics is what makes async compute actually overlap.
    for (std::uint32_t i = 0; i < familyCount; ++i) {
        const VkQueueFlags flags = families[i].queueFlags;
        if ((flags & VK_QUEUE_COMPUTE_BIT) != 0 && (flags & VK_QUEUE_GRAPHICS_BIT) == 0) {
            compute_.family = i;
            break;
        }
    }
    for (std::uint32_t i = 0; i < familyCount; ++i) {
        const VkQueueFlags flags = families[i].queueFlags;
        if ((flags & VK_QUEUE_TRANSFER_BIT) != 0
            && (flags & VK_QUEUE_GRAPHICS_BIT) == 0
            && (flags & VK_QUEUE_COMPUTE_BIT) == 0) {
            transfer_.family = i;
            break;
        }
    }
    if (compute_.family == kNone) {
        compute_.family = graphics_.family;
    }
    if (transfer_.family == kNone) {
        transfer_.family = compute_.family;
    }

    std::vector<std::uint32_t> uniqueFamilies{graphics_.family};
    if (compute_.family != graphics_.family) {
        uniqueFamilies.push_back(compute_.family);
    }
    if (transfer_.family != graphics_.family && transfer_.family != compute_.family) {
        uniqueFamilies.push_back(transfer_.family);
    }

    const float priority = 1.0f;
    std::vector<VkDeviceQueueCreateInfo> queueInfos;
    queueInfos.reserve(uniqueFamilies.size());
    for (const std::uint32_t family : uniqueFamilies) {
        VkDeviceQueueCreateInfo qi{};
        qi.sType            = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO;
        qi.queueFamilyIndex = family;
        qi.queueCount       = 1;
        qi.pQueuePriorities = &priority;
        queueInfos.push_back(qi);
    }

    std::vector<const char*> extensions;
    if (surface_ != VK_NULL_HANDLE) {
        extensions.push_back(VK_KHR_SWAPCHAIN_EXTENSION_NAME);
    }

    VkPhysicalDeviceVulkan13Features v13{};
    v13.sType            = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES;
    v13.dynamicRendering = VK_TRUE;
    v13.synchronization2 = VK_TRUE;
    v13.maintenance4     = VK_TRUE;

    VkPhysicalDeviceVulkan12Features v12{};
    v12.sType                  = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES;
    v12.pNext                  = &v13;
    v12.descriptorIndexing     = VK_TRUE;
    v12.runtimeDescriptorArray = VK_TRUE;
    v12.bufferDeviceAddress    = VK_TRUE;
    v12.timelineSemaphore      = VK_TRUE;
    v12.scalarBlockLayout      = VK_TRUE;
    v12.descriptorBindingPartiallyBound                    = VK_TRUE;
    v12.descriptorBindingVariableDescriptorCount           = VK_TRUE;
    v12.descriptorBindingSampledImageUpdateAfterBind       = VK_TRUE;
    v12.descriptorBindingStorageBufferUpdateAfterBind      = VK_TRUE;
    v12.descriptorBindingStorageImageUpdateAfterBind       = VK_TRUE;
    v12.descriptorBindingUpdateUnusedWhilePending          = VK_TRUE;
    v12.shaderSampledImageArrayNonUniformIndexing          = VK_TRUE;
    v12.shaderStorageBufferArrayNonUniformIndexing         = VK_TRUE;
    v12.shaderStorageImageArrayNonUniformIndexing          = VK_TRUE;

    VkPhysicalDeviceFeatures2 features{};
    features.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2;
    features.pNext = &v12;

    VkDeviceCreateInfo info{};
    info.sType                   = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO;
    info.pNext                   = &features;
    info.queueCreateInfoCount    = static_cast<std::uint32_t>(queueInfos.size());
    info.pQueueCreateInfos       = queueInfos.data();
    info.enabledExtensionCount   = static_cast<std::uint32_t>(extensions.size());
    info.ppEnabledExtensionNames = extensions.data();

    const VkResult result = vkCreateDevice(physical_, &info, nullptr, &device_);
    if (result != VK_SUCCESS) {
        std::fprintf(stderr, "[vulkan] vkCreateDevice failed: %s\n", resultToString(result));
        return false;
    }

    vkGetDeviceQueue(device_, graphics_.family, 0, &graphics_.queue);
    vkGetDeviceQueue(device_, compute_.family, 0, &compute_.queue);
    vkGetDeviceQueue(device_, transfer_.family, 0, &transfer_.queue);

    setDebugName(device_, VK_OBJECT_TYPE_QUEUE, graphics_.queue, "Queue_Graphics");
    setDebugName(device_, VK_OBJECT_TYPE_QUEUE, compute_.queue, "Queue_Compute");
    setDebugName(device_, VK_OBJECT_TYPE_QUEUE, transfer_.queue, "Queue_Transfer");
    setDebugName(device_, VK_OBJECT_TYPE_DEVICE, device_, "Device_Main");

    return true;
}

bool VulkanDevice::createAllocator()
{
    VmaVulkanFunctions functions{};
    functions.vkGetInstanceProcAddr = &vkGetInstanceProcAddr;
    functions.vkGetDeviceProcAddr   = &vkGetDeviceProcAddr;

    VmaAllocatorCreateInfo info{};
    info.physicalDevice   = physical_;
    info.device           = device_;
    info.instance         = instance_;
    info.vulkanApiVersion = VK_API_VERSION_1_3;
    info.pVulkanFunctions = &functions;
    info.flags            = VMA_ALLOCATOR_CREATE_BUFFER_DEVICE_ADDRESS_BIT;

    const VkResult result = vmaCreateAllocator(&info, &allocator_);
    if (result != VK_SUCCESS) {
        std::fprintf(stderr, "[vulkan] vmaCreateAllocator failed: %s\n", resultToString(result));
        return false;
    }
    return true;
}

void VulkanDevice::waitIdle() const
{
    if (device_ != VK_NULL_HANDLE) {
        vkDeviceWaitIdle(device_);
    }
}

void VulkanDevice::destroy()
{
    if (device_ != VK_NULL_HANDLE) {
        vkDeviceWaitIdle(device_);
    }
    if (allocator_ != nullptr) {
        vmaDestroyAllocator(allocator_);
        allocator_ = nullptr;
    }
    if (device_ != VK_NULL_HANDLE) {
        vkDestroyDevice(device_, nullptr);
        device_ = VK_NULL_HANDLE;
    }
    if (surface_ != VK_NULL_HANDLE) {
        vkDestroySurfaceKHR(instance_, surface_, nullptr);
        surface_ = VK_NULL_HANDLE;
    }
    if (messenger_ != VK_NULL_HANDLE) {
        auto destroyMessenger = reinterpret_cast<PFN_vkDestroyDebugUtilsMessengerEXT>(
            vkGetInstanceProcAddr(instance_, "vkDestroyDebugUtilsMessengerEXT"));
        if (destroyMessenger != nullptr) {
            destroyMessenger(instance_, messenger_, nullptr);
        }
        messenger_ = VK_NULL_HANDLE;
    }
    if (instance_ != VK_NULL_HANDLE) {
        vkDestroyInstance(instance_, nullptr);
        instance_ = VK_NULL_HANDLE;
    }
    physical_ = VK_NULL_HANDLE;
    graphics_ = {};
    compute_  = {};
    transfer_ = {};
}

} // namespace harpia::rhi
