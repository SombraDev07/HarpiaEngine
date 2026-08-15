#include "RHI/Vulkan/VulkanCommon.h"

namespace harpia::rhi {

const char* resultToString(VkResult result) noexcept
{
    switch (result) {
        case VK_SUCCESS:                        return "VK_SUCCESS";
        case VK_NOT_READY:                      return "VK_NOT_READY";
        case VK_TIMEOUT:                        return "VK_TIMEOUT";
        case VK_EVENT_SET:                      return "VK_EVENT_SET";
        case VK_EVENT_RESET:                    return "VK_EVENT_RESET";
        case VK_INCOMPLETE:                     return "VK_INCOMPLETE";
        case VK_SUBOPTIMAL_KHR:                 return "VK_SUBOPTIMAL_KHR";
        case VK_ERROR_OUT_OF_HOST_MEMORY:       return "VK_ERROR_OUT_OF_HOST_MEMORY";
        case VK_ERROR_OUT_OF_DEVICE_MEMORY:     return "VK_ERROR_OUT_OF_DEVICE_MEMORY";
        case VK_ERROR_INITIALIZATION_FAILED:    return "VK_ERROR_INITIALIZATION_FAILED";
        case VK_ERROR_DEVICE_LOST:              return "VK_ERROR_DEVICE_LOST";
        case VK_ERROR_MEMORY_MAP_FAILED:        return "VK_ERROR_MEMORY_MAP_FAILED";
        case VK_ERROR_LAYER_NOT_PRESENT:        return "VK_ERROR_LAYER_NOT_PRESENT";
        case VK_ERROR_EXTENSION_NOT_PRESENT:    return "VK_ERROR_EXTENSION_NOT_PRESENT";
        case VK_ERROR_FEATURE_NOT_PRESENT:      return "VK_ERROR_FEATURE_NOT_PRESENT";
        case VK_ERROR_INCOMPATIBLE_DRIVER:      return "VK_ERROR_INCOMPATIBLE_DRIVER";
        case VK_ERROR_TOO_MANY_OBJECTS:         return "VK_ERROR_TOO_MANY_OBJECTS";
        case VK_ERROR_FORMAT_NOT_SUPPORTED:     return "VK_ERROR_FORMAT_NOT_SUPPORTED";
        case VK_ERROR_FRAGMENTED_POOL:          return "VK_ERROR_FRAGMENTED_POOL";
        case VK_ERROR_UNKNOWN:                  return "VK_ERROR_UNKNOWN";
        case VK_ERROR_OUT_OF_POOL_MEMORY:       return "VK_ERROR_OUT_OF_POOL_MEMORY";
        case VK_ERROR_INVALID_EXTERNAL_HANDLE:  return "VK_ERROR_INVALID_EXTERNAL_HANDLE";
        case VK_ERROR_FRAGMENTATION:            return "VK_ERROR_FRAGMENTATION";
        case VK_ERROR_SURFACE_LOST_KHR:         return "VK_ERROR_SURFACE_LOST_KHR";
        case VK_ERROR_NATIVE_WINDOW_IN_USE_KHR: return "VK_ERROR_NATIVE_WINDOW_IN_USE_KHR";
        case VK_ERROR_OUT_OF_DATE_KHR:          return "VK_ERROR_OUT_OF_DATE_KHR";
        case VK_ERROR_INCOMPATIBLE_DISPLAY_KHR: return "VK_ERROR_INCOMPATIBLE_DISPLAY_KHR";
        case VK_ERROR_VALIDATION_FAILED_EXT:    return "VK_ERROR_VALIDATION_FAILED_EXT";
        default:                                return "VK_ERROR_<unmapped>";
    }
}

void setDebugName(VkDevice device, VkObjectType type, std::uint64_t handle, const char* name)
{
    if (device == VK_NULL_HANDLE || name == nullptr) {
        return;
    }

    static auto setName = reinterpret_cast<PFN_vkSetDebugUtilsObjectNameEXT>(
        vkGetDeviceProcAddr(device, "vkSetDebugUtilsObjectNameEXT"));

    if (setName == nullptr) {
        return; // debug utils not enabled — naming is a no-op, never an error
    }

    VkDebugUtilsObjectNameInfoEXT info{};
    info.sType        = VK_STRUCTURE_TYPE_DEBUG_UTILS_OBJECT_NAME_INFO_EXT;
    info.objectType   = type;
    info.objectHandle = handle;
    info.pObjectName  = name;

    setName(device, &info);
}

void imageBarrier(VkCommandBuffer       cmd,
                  VkImage               image,
                  VkImageLayout         oldLayout,
                  VkImageLayout         newLayout,
                  VkPipelineStageFlags2 srcStage,
                  VkAccessFlags2        srcAccess,
                  VkPipelineStageFlags2 dstStage,
                  VkAccessFlags2        dstAccess,
                  VkImageAspectFlags    aspect)
{
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
    barrier.image         = image;
    barrier.subresourceRange.aspectMask = aspect;
    barrier.subresourceRange.levelCount = VK_REMAINING_MIP_LEVELS;
    barrier.subresourceRange.layerCount = VK_REMAINING_ARRAY_LAYERS;

    VkDependencyInfo dependency{};
    dependency.sType                   = VK_STRUCTURE_TYPE_DEPENDENCY_INFO;
    dependency.imageMemoryBarrierCount = 1;
    dependency.pImageMemoryBarriers    = &barrier;

    vkCmdPipelineBarrier2(cmd, &dependency);
}

namespace {

PFN_vkCmdBeginDebugUtilsLabelEXT g_beginLabel = nullptr;
PFN_vkCmdEndDebugUtilsLabelEXT   g_endLabel   = nullptr;
bool                             g_labelsResolved = false;

void resolveLabelFunctions(VkCommandBuffer cmd)
{
    if (g_labelsResolved || cmd == VK_NULL_HANDLE) {
        return;
    }
    g_labelsResolved = true;
    // Instance-level lookup: command buffers do not carry an instance, so this
    // relies on the loader's global dispatch, which is valid for these two.
    g_beginLabel = reinterpret_cast<PFN_vkCmdBeginDebugUtilsLabelEXT>(
        vkGetInstanceProcAddr(nullptr, "vkCmdBeginDebugUtilsLabelEXT"));
    g_endLabel = reinterpret_cast<PFN_vkCmdEndDebugUtilsLabelEXT>(
        vkGetInstanceProcAddr(nullptr, "vkCmdEndDebugUtilsLabelEXT"));
}

} // namespace

DebugLabel::DebugLabel(VkCommandBuffer cmd, const char* name, float r, float g, float b)
    : cmd_(cmd)
{
    resolveLabelFunctions(cmd);
    if (g_beginLabel == nullptr) {
        return;
    }
    VkDebugUtilsLabelEXT label{};
    label.sType      = VK_STRUCTURE_TYPE_DEBUG_UTILS_LABEL_EXT;
    label.pLabelName = name;
    label.color[0]   = r;
    label.color[1]   = g;
    label.color[2]   = b;
    label.color[3]   = 1.0f;
    g_beginLabel(cmd_, &label);
}

DebugLabel::~DebugLabel()
{
    if (g_endLabel != nullptr) {
        g_endLabel(cmd_);
    }
}

} // namespace harpia::rhi
