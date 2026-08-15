// Harpia Engine — Vulkan shared plumbing
#pragma once

#include <vulkan/vulkan.h>

#include <cstdint>
#include <cstdio>
#include <cstdlib>

namespace harpia::rhi {

[[nodiscard]] const char* resultToString(VkResult result) noexcept;

// Vulkan failures are programmer errors nearly every time; failing loudly at
// the call site beats propagating a code nobody checks.
#define HARPIA_VK_CHECK(expr)                                                        \
    do {                                                                             \
        const VkResult harpia_vk_result_ = (expr);                                   \
        if (harpia_vk_result_ != VK_SUCCESS) {                                       \
            std::fprintf(stderr, "[vulkan] %s failed: %s (%s:%d)\n",                 \
                         #expr,                                                      \
                         ::harpia::rhi::resultToString(harpia_vk_result_),           \
                         __FILE__, __LINE__);                                        \
            std::abort();                                                            \
        }                                                                            \
    } while (false)

// Roadmap 1.9: a capture showing Buffer_0x7f3a costs an hour, one showing
// CSM_Cascade2_Depth costs a minute. Naming is not optional.
void setDebugName(VkDevice device, VkObjectType type, std::uint64_t handle, const char* name);

template <typename T>
void setDebugName(VkDevice device, VkObjectType type, T handle, const char* name)
{
    setDebugName(device, type, reinterpret_cast<std::uint64_t>(handle), name);
}

// Roadmap decision: barriers use synchronization2 everywhere. Stage and access
// masks are explicit because the render graph will generate them later, and a
// hand-written ALL_COMMANDS barrier now becomes a silent stall then.
void imageBarrier(VkCommandBuffer       cmd,
                  VkImage               image,
                  VkImageLayout         oldLayout,
                  VkImageLayout         newLayout,
                  VkPipelineStageFlags2 srcStage,
                  VkAccessFlags2        srcAccess,
                  VkPipelineStageFlags2 dstStage,
                  VkAccessFlags2        dstAccess,
                  VkImageAspectFlags    aspect = VK_IMAGE_ASPECT_COLOR_BIT);

// Scoped GPU marker; shows up as a labelled region in RenderDoc.
class DebugLabel {
public:
    DebugLabel(VkCommandBuffer cmd, const char* name, float r, float g, float b);
    ~DebugLabel();

    DebugLabel(const DebugLabel&)            = delete;
    DebugLabel& operator=(const DebugLabel&) = delete;

private:
    VkCommandBuffer cmd_;
};

} // namespace harpia::rhi
