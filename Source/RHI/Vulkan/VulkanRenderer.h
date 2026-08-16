// Harpia Engine — frame driver
//
// Owns frames in flight, command buffers and synchronisation, and presents
// either to a swapchain or to an offscreen image. The offscreen path is not a
// fallback: it is how golden images get produced (roadmap rule 1).
#pragma once

#include "RHI/Vulkan/VulkanBindless.h"
#include "RHI/Vulkan/VulkanCommon.h"
#include "RHI/Vulkan/VulkanSwapchain.h"

#include <cstdint>
#include <string>
#include <vector>

struct VmaAllocation_T;
using VmaAllocation = VmaAllocation_T*;

namespace harpia::rhi {

class VulkanDevice;

struct FrameInfo {
    VkCommandBuffer cmd         = VK_NULL_HANDLE;
    VkImage         targetImage = VK_NULL_HANDLE;
    VkImageView     targetView  = VK_NULL_HANDLE;
    VkExtent2D      extent{};
    std::uint32_t   frameIndex  = 0;   // 0..framesInFlight-1
    std::uint64_t   frameNumber = 0;   // monotonic since start
};

class VulkanRenderer {
public:
    static constexpr std::uint32_t kFramesInFlight = 2;

    VulkanRenderer() = default;
    ~VulkanRenderer();

    VulkanRenderer(const VulkanRenderer&)            = delete;
    VulkanRenderer& operator=(const VulkanRenderer&) = delete;

    // Windowed: presents to a swapchain sized to the window.
    [[nodiscard]] bool create(VulkanDevice& device, std::uint32_t width, std::uint32_t height);

    // Headless: renders to an offscreen image that captureToPng can read back.
    [[nodiscard]] bool createOffscreen(VulkanDevice& device,
                                       std::uint32_t width,
                                       std::uint32_t height);

    void destroy();

    // Returns false when the frame must be skipped (minimised or resizing).
    [[nodiscard]] bool beginFrame(FrameInfo& outFrame);
    void endFrame();

    void onResize(std::uint32_t width, std::uint32_t height);

    // Offscreen only. Call after endFrame(); waits for the GPU and reads the
    // image back as tightly packed 8-bit RGBA.
    [[nodiscard]] bool readback(std::vector<std::uint8_t>& outRgba);

    // readback() plus a PNG write. Golden images go through here.
    [[nodiscard]] bool captureToPng(const std::string& path);

    [[nodiscard]] VulkanBindless&  bindless() noexcept  { return bindless_; }
    [[nodiscard]] VkExtent2D       extent() const noexcept;
    [[nodiscard]] bool             headless() const noexcept { return headless_; }
    [[nodiscard]] std::uint64_t    frameNumber() const noexcept { return frameNumber_; }

private:
    struct Frame {
        VkCommandPool   pool           = VK_NULL_HANDLE;
        VkCommandBuffer cmd            = VK_NULL_HANDLE;
        VkSemaphore     imageAvailable = VK_NULL_HANDLE;
        VkFence         inFlight       = VK_NULL_HANDLE;
    };

    [[nodiscard]] bool createFrames();
    [[nodiscard]] bool createDefaultSamplers();
    [[nodiscard]] bool createOffscreenTarget(std::uint32_t width, std::uint32_t height);
    void destroyOffscreenTarget();

    VulkanDevice* device_ = nullptr;

    VulkanSwapchain swapchain_;
    VulkanBindless  bindless_;

    std::vector<Frame> frames_;
    // One per swapchain image, not per frame in flight: a semaphore signalled
    // for image N must not be waited on while image N is still being presented.
    std::vector<VkSemaphore> renderFinished_;

    // Registered into the bindless heap at the slots RenderTypes.h publishes,
    // so a shader names a sampler by constant rather than binding one.
    VkSampler linearRepeat_ = VK_NULL_HANDLE;
    VkSampler pointClamp_   = VK_NULL_HANDLE;
    VkSampler linearClamp_  = VK_NULL_HANDLE;

    std::uint32_t frameIndex_  = 0;
    std::uint64_t frameNumber_ = 0;
    std::uint32_t imageIndex_  = 0;
    bool          frameOpen_   = false;

    // Offscreen
    bool          headless_        = false;
    VkImage       offscreenImage_  = VK_NULL_HANDLE;
    VkImageView   offscreenView_   = VK_NULL_HANDLE;
    VmaAllocation offscreenAlloc_  = nullptr;
    VkExtent2D    offscreenExtent_{};
    VkFormat      offscreenFormat_ = VK_FORMAT_R8G8B8A8_UNORM;

    // Pending resize
    std::uint32_t pendingWidth_  = 0;
    std::uint32_t pendingHeight_ = 0;
    bool          needsResize_   = false;
};

} // namespace harpia::rhi
