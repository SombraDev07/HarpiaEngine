#include "RHI/Vulkan/VulkanRenderer.h"

#include "RHI/Vulkan/VulkanDevice.h"

#include <stb_image_write.h>
#include <vk_mem_alloc.h>

#include <cstring>
#include <vector>

namespace harpia::rhi {

VulkanRenderer::~VulkanRenderer()
{
    destroy();
}

bool VulkanRenderer::create(VulkanDevice& device, std::uint32_t width, std::uint32_t height)
{
    device_   = &device;
    headless_ = false;

    if (!swapchain_.create(device, width, height)) {
        return false;
    }
    if (!createFrames()) {
        return false;
    }
    if (!bindless_.create(device) || !createDefaultSamplers()) {
        return false;
    }

    renderFinished_.resize(swapchain_.imageCount(), VK_NULL_HANDLE);
    for (std::uint32_t i = 0; i < swapchain_.imageCount(); ++i) {
        VkSemaphoreCreateInfo info{};
        info.sType = VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO;
        HARPIA_VK_CHECK(vkCreateSemaphore(device_->device(), &info, nullptr,
                                          &renderFinished_[i]));

        char name[48];
        std::snprintf(name, sizeof(name), "Sem_RenderFinished_%u", i);
        setDebugName(device_->device(), VK_OBJECT_TYPE_SEMAPHORE, renderFinished_[i], name);
    }
    return true;
}

bool VulkanRenderer::createOffscreen(VulkanDevice& device,
                                     std::uint32_t width,
                                     std::uint32_t height)
{
    device_   = &device;
    headless_ = true;

    if (!createOffscreenTarget(width, height)) {
        return false;
    }
    if (!createFrames()) {
        return false;
    }
    if (!bindless_.create(device) || !createDefaultSamplers()) {
        return false;
    }
    return true;
}

bool VulkanRenderer::createDefaultSamplers()
{
    const VkDevice device = device_->device();

    VkSamplerCreateInfo linear{};
    linear.sType        = VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO;
    linear.magFilter    = VK_FILTER_LINEAR;
    linear.minFilter    = VK_FILTER_LINEAR;
    linear.mipmapMode   = VK_SAMPLER_MIPMAP_MODE_LINEAR;
    linear.addressModeU = VK_SAMPLER_ADDRESS_MODE_REPEAT;
    linear.addressModeV = VK_SAMPLER_ADDRESS_MODE_REPEAT;
    linear.addressModeW = VK_SAMPLER_ADDRESS_MODE_REPEAT;
    linear.maxLod       = VK_LOD_CLAMP_NONE;
    linear.maxAnisotropy = 1.0f;
    HARPIA_VK_CHECK(vkCreateSampler(device, &linear, nullptr, &linearRepeat_));

    // GBuffer channels carry encoded values — an octahedral normal filtered
    // across a silhouette is not a normal. Point sampling is not a quality
    // compromise here, it is correctness.
    VkSamplerCreateInfo point{};
    point.sType        = VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO;
    point.magFilter    = VK_FILTER_NEAREST;
    point.minFilter    = VK_FILTER_NEAREST;
    point.mipmapMode   = VK_SAMPLER_MIPMAP_MODE_NEAREST;
    point.addressModeU = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE;
    point.addressModeV = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE;
    point.addressModeW = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE;
    point.maxAnisotropy = 1.0f;
    HARPIA_VK_CHECK(vkCreateSampler(device, &point, nullptr, &pointClamp_));

    setDebugName(device, VK_OBJECT_TYPE_SAMPLER, linearRepeat_, "Sampler_LinearRepeat");
    setDebugName(device, VK_OBJECT_TYPE_SAMPLER, pointClamp_, "Sampler_PointClamp");

    // Slot order must match SamplerSlot in RenderTypes.h.
    const std::uint32_t linearSlot = bindless_.registerSampler(linearRepeat_);
    const std::uint32_t pointSlot  = bindless_.registerSampler(pointClamp_);
    return linearSlot == 0 && pointSlot == 1;
}

bool VulkanRenderer::createFrames()
{
    const VkDevice device = device_->device();

    frames_.resize(kFramesInFlight);
    for (std::uint32_t i = 0; i < kFramesInFlight; ++i) {
        Frame& frame = frames_[i];

        VkCommandPoolCreateInfo poolInfo{};
        poolInfo.sType            = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
        poolInfo.flags            = VK_COMMAND_POOL_CREATE_TRANSIENT_BIT;
        poolInfo.queueFamilyIndex = device_->graphics().family;
        HARPIA_VK_CHECK(vkCreateCommandPool(device, &poolInfo, nullptr, &frame.pool));

        VkCommandBufferAllocateInfo allocInfo{};
        allocInfo.sType              = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
        allocInfo.commandPool        = frame.pool;
        allocInfo.level              = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
        allocInfo.commandBufferCount = 1;
        HARPIA_VK_CHECK(vkAllocateCommandBuffers(device, &allocInfo, &frame.cmd));

        VkSemaphoreCreateInfo semInfo{};
        semInfo.sType = VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO;
        HARPIA_VK_CHECK(vkCreateSemaphore(device, &semInfo, nullptr, &frame.imageAvailable));

        // Created signalled so the first beginFrame does not wait forever.
        VkFenceCreateInfo fenceInfo{};
        fenceInfo.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
        fenceInfo.flags = VK_FENCE_CREATE_SIGNALED_BIT;
        HARPIA_VK_CHECK(vkCreateFence(device, &fenceInfo, nullptr, &frame.inFlight));

        char name[48];
        std::snprintf(name, sizeof(name), "Frame%u_CmdPool", i);
        setDebugName(device, VK_OBJECT_TYPE_COMMAND_POOL, frame.pool, name);
        std::snprintf(name, sizeof(name), "Frame%u_Cmd", i);
        setDebugName(device, VK_OBJECT_TYPE_COMMAND_BUFFER, frame.cmd, name);
        std::snprintf(name, sizeof(name), "Frame%u_Fence", i);
        setDebugName(device, VK_OBJECT_TYPE_FENCE, frame.inFlight, name);
        std::snprintf(name, sizeof(name), "Frame%u_Sem_ImageAvailable", i);
        setDebugName(device, VK_OBJECT_TYPE_SEMAPHORE, frame.imageAvailable, name);
    }
    return true;
}

bool VulkanRenderer::createOffscreenTarget(std::uint32_t width, std::uint32_t height)
{
    offscreenExtent_ = VkExtent2D{width, height};

    VkImageCreateInfo imageInfo{};
    imageInfo.sType         = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
    imageInfo.imageType     = VK_IMAGE_TYPE_2D;
    imageInfo.format        = offscreenFormat_;
    imageInfo.extent        = VkExtent3D{width, height, 1};
    imageInfo.mipLevels     = 1;
    imageInfo.arrayLayers   = 1;
    imageInfo.samples       = VK_SAMPLE_COUNT_1_BIT;
    imageInfo.tiling        = VK_IMAGE_TILING_OPTIMAL;
    imageInfo.usage         = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT
                            | VK_IMAGE_USAGE_TRANSFER_SRC_BIT
                            | VK_IMAGE_USAGE_SAMPLED_BIT;
    imageInfo.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;

    VmaAllocationCreateInfo allocInfo{};
    allocInfo.usage = VMA_MEMORY_USAGE_AUTO;

    HARPIA_VK_CHECK(vmaCreateImage(device_->allocator(), &imageInfo, &allocInfo,
                                   &offscreenImage_, &offscreenAlloc_, nullptr));

    VkImageViewCreateInfo viewInfo{};
    viewInfo.sType    = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
    viewInfo.image    = offscreenImage_;
    viewInfo.viewType = VK_IMAGE_VIEW_TYPE_2D;
    viewInfo.format   = offscreenFormat_;
    viewInfo.subresourceRange.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    viewInfo.subresourceRange.levelCount = 1;
    viewInfo.subresourceRange.layerCount = 1;
    HARPIA_VK_CHECK(vkCreateImageView(device_->device(), &viewInfo, nullptr, &offscreenView_));

    setDebugName(device_->device(), VK_OBJECT_TYPE_IMAGE, offscreenImage_, "Offscreen_Color");
    setDebugName(device_->device(), VK_OBJECT_TYPE_IMAGE_VIEW, offscreenView_,
                 "Offscreen_ColorView");
    return true;
}

void VulkanRenderer::destroyOffscreenTarget()
{
    if (device_ == nullptr) {
        return;
    }
    if (offscreenView_ != VK_NULL_HANDLE) {
        vkDestroyImageView(device_->device(), offscreenView_, nullptr);
        offscreenView_ = VK_NULL_HANDLE;
    }
    if (offscreenImage_ != VK_NULL_HANDLE) {
        vmaDestroyImage(device_->allocator(), offscreenImage_, offscreenAlloc_);
        offscreenImage_ = VK_NULL_HANDLE;
        offscreenAlloc_ = nullptr;
    }
}

void VulkanRenderer::destroy()
{
    if (device_ == nullptr) {
        return;
    }
    const VkDevice device = device_->device();
    vkDeviceWaitIdle(device);

    if (linearRepeat_ != VK_NULL_HANDLE) {
        vkDestroySampler(device, linearRepeat_, nullptr);
        linearRepeat_ = VK_NULL_HANDLE;
    }
    if (pointClamp_ != VK_NULL_HANDLE) {
        vkDestroySampler(device, pointClamp_, nullptr);
        pointClamp_ = VK_NULL_HANDLE;
    }

    bindless_.destroy();

    for (VkSemaphore semaphore : renderFinished_) {
        if (semaphore != VK_NULL_HANDLE) {
            vkDestroySemaphore(device, semaphore, nullptr);
        }
    }
    renderFinished_.clear();

    for (Frame& frame : frames_) {
        if (frame.inFlight != VK_NULL_HANDLE) {
            vkDestroyFence(device, frame.inFlight, nullptr);
        }
        if (frame.imageAvailable != VK_NULL_HANDLE) {
            vkDestroySemaphore(device, frame.imageAvailable, nullptr);
        }
        if (frame.pool != VK_NULL_HANDLE) {
            vkDestroyCommandPool(device, frame.pool, nullptr); // frees its buffers
        }
    }
    frames_.clear();

    destroyOffscreenTarget();
    swapchain_.destroy();

    device_ = nullptr;
}

VkExtent2D VulkanRenderer::extent() const noexcept
{
    return headless_ ? offscreenExtent_ : swapchain_.extent();
}

void VulkanRenderer::onResize(std::uint32_t width, std::uint32_t height)
{
    if (headless_) {
        return;
    }
    pendingWidth_  = width;
    pendingHeight_ = height;
    needsResize_   = true;
}

bool VulkanRenderer::beginFrame(FrameInfo& outFrame)
{
    const VkDevice device = device_->device();
    Frame&         frame  = frames_[frameIndex_];

    HARPIA_VK_CHECK(vkWaitForFences(device, 1, &frame.inFlight, VK_TRUE, UINT64_MAX));

    if (!headless_) {
        if (needsResize_) {
            if (pendingWidth_ == 0 || pendingHeight_ == 0) {
                return false; // minimised
            }
            if (!swapchain_.recreate(pendingWidth_, pendingHeight_)) {
                return false;
            }
            needsResize_ = false;

            // Image count can change across a rebuild.
            for (VkSemaphore semaphore : renderFinished_) {
                if (semaphore != VK_NULL_HANDLE) {
                    vkDestroySemaphore(device, semaphore, nullptr);
                }
            }
            renderFinished_.assign(swapchain_.imageCount(), VK_NULL_HANDLE);
            for (std::uint32_t i = 0; i < swapchain_.imageCount(); ++i) {
                VkSemaphoreCreateInfo info{};
                info.sType = VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO;
                HARPIA_VK_CHECK(vkCreateSemaphore(device, &info, nullptr, &renderFinished_[i]));
            }
        }

        const VkResult acquired = swapchain_.acquireNext(frame.imageAvailable, imageIndex_);
        if (acquired == VK_ERROR_OUT_OF_DATE_KHR) {
            needsResize_   = true;
            pendingWidth_  = swapchain_.extent().width;
            pendingHeight_ = swapchain_.extent().height;
            return false;
        }
        if (acquired != VK_SUCCESS && acquired != VK_SUBOPTIMAL_KHR) {
            std::fprintf(stderr, "[vulkan] acquire failed: %s\n", resultToString(acquired));
            return false;
        }
    }

    HARPIA_VK_CHECK(vkResetFences(device, 1, &frame.inFlight));
    HARPIA_VK_CHECK(vkResetCommandPool(device, frame.pool, 0));

    VkCommandBufferBeginInfo beginInfo{};
    beginInfo.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    beginInfo.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    HARPIA_VK_CHECK(vkBeginCommandBuffer(frame.cmd, &beginInfo));

    VkImage     target = headless_ ? offscreenImage_ : swapchain_.image(imageIndex_);
    VkImageView view   = headless_ ? offscreenView_ : swapchain_.imageView(imageIndex_);

    imageBarrier(frame.cmd, target,
                 VK_IMAGE_LAYOUT_UNDEFINED,
                 VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
                 VK_PIPELINE_STAGE_2_TOP_OF_PIPE_BIT, 0,
                 VK_PIPELINE_STAGE_2_COLOR_ATTACHMENT_OUTPUT_BIT,
                 VK_ACCESS_2_COLOR_ATTACHMENT_WRITE_BIT);

    outFrame.cmd         = frame.cmd;
    outFrame.targetImage = target;
    outFrame.targetView  = view;
    outFrame.extent      = extent();
    outFrame.frameIndex  = frameIndex_;
    outFrame.frameNumber = frameNumber_;

    frameOpen_ = true;
    return true;
}

void VulkanRenderer::endFrame()
{
    if (!frameOpen_) {
        return;
    }
    frameOpen_ = false;

    Frame&        frame  = frames_[frameIndex_];
    const VkImage target = headless_ ? offscreenImage_ : swapchain_.image(imageIndex_);

    // Headless leaves the image in TRANSFER_SRC so captureToPng can copy it
    // without another submit; windowed goes straight to PRESENT_SRC.
    const VkImageLayout finalLayout = headless_ ? VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL
                                                : VK_IMAGE_LAYOUT_PRESENT_SRC_KHR;
    const VkPipelineStageFlags2 dstStage = headless_ ? VK_PIPELINE_STAGE_2_COPY_BIT
                                                     : VK_PIPELINE_STAGE_2_BOTTOM_OF_PIPE_BIT;
    const VkAccessFlags2 dstAccess = headless_ ? VK_ACCESS_2_TRANSFER_READ_BIT : 0;

    imageBarrier(frame.cmd, target,
                 VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
                 finalLayout,
                 VK_PIPELINE_STAGE_2_COLOR_ATTACHMENT_OUTPUT_BIT,
                 VK_ACCESS_2_COLOR_ATTACHMENT_WRITE_BIT,
                 dstStage, dstAccess);

    HARPIA_VK_CHECK(vkEndCommandBuffer(frame.cmd));

    VkCommandBufferSubmitInfo cmdInfo{};
    cmdInfo.sType         = VK_STRUCTURE_TYPE_COMMAND_BUFFER_SUBMIT_INFO;
    cmdInfo.commandBuffer = frame.cmd;

    VkSemaphoreSubmitInfo waitInfo{};
    waitInfo.sType     = VK_STRUCTURE_TYPE_SEMAPHORE_SUBMIT_INFO;
    waitInfo.semaphore = frame.imageAvailable;
    waitInfo.stageMask = VK_PIPELINE_STAGE_2_COLOR_ATTACHMENT_OUTPUT_BIT;

    VkSemaphoreSubmitInfo signalInfo{};
    signalInfo.sType     = VK_STRUCTURE_TYPE_SEMAPHORE_SUBMIT_INFO;
    signalInfo.stageMask = VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT;
    if (!headless_) {
        signalInfo.semaphore = renderFinished_[imageIndex_];
    }

    VkSubmitInfo2 submit{};
    submit.sType                    = VK_STRUCTURE_TYPE_SUBMIT_INFO_2;
    submit.commandBufferInfoCount   = 1;
    submit.pCommandBufferInfos      = &cmdInfo;
    submit.waitSemaphoreInfoCount   = headless_ ? 0u : 1u;
    submit.pWaitSemaphoreInfos      = headless_ ? nullptr : &waitInfo;
    submit.signalSemaphoreInfoCount = headless_ ? 0u : 1u;
    submit.pSignalSemaphoreInfos    = headless_ ? nullptr : &signalInfo;

    HARPIA_VK_CHECK(vkQueueSubmit2(device_->graphics().queue, 1, &submit, frame.inFlight));

    if (!headless_) {
        const VkResult presented = swapchain_.present(device_->graphics().queue,
                                                      renderFinished_[imageIndex_],
                                                      imageIndex_);
        if (presented == VK_ERROR_OUT_OF_DATE_KHR || presented == VK_SUBOPTIMAL_KHR) {
            needsResize_   = true;
            pendingWidth_  = swapchain_.extent().width;
            pendingHeight_ = swapchain_.extent().height;
        } else if (presented != VK_SUCCESS) {
            std::fprintf(stderr, "[vulkan] present failed: %s\n", resultToString(presented));
        }
    }

    frameIndex_ = (frameIndex_ + 1) % kFramesInFlight;
    ++frameNumber_;
}

bool VulkanRenderer::readback(std::vector<std::uint8_t>& outRgba)
{
    if (!headless_ || offscreenImage_ == VK_NULL_HANDLE) {
        std::fprintf(stderr, "[vulkan] readback requires the offscreen renderer\n");
        return false;
    }

    const VkDevice device = device_->device();
    vkDeviceWaitIdle(device);

    const std::uint32_t width  = offscreenExtent_.width;
    const std::uint32_t height = offscreenExtent_.height;
    const VkDeviceSize  size   = VkDeviceSize{width} * height * 4;

    VkBufferCreateInfo bufferInfo{};
    bufferInfo.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO;
    bufferInfo.size  = size;
    bufferInfo.usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT;

    VmaAllocationCreateInfo allocInfo{};
    allocInfo.usage = VMA_MEMORY_USAGE_AUTO;
    allocInfo.flags = VMA_ALLOCATION_CREATE_HOST_ACCESS_RANDOM_BIT
                    | VMA_ALLOCATION_CREATE_MAPPED_BIT;

    VkBuffer          staging      = VK_NULL_HANDLE;
    VmaAllocation     stagingAlloc = nullptr;
    VmaAllocationInfo stagingInfo{};
    HARPIA_VK_CHECK(vmaCreateBuffer(device_->allocator(), &bufferInfo, &allocInfo,
                                    &staging, &stagingAlloc, &stagingInfo));

    // One-shot copy on its own pool; this path is a capture, not a hot loop.
    VkCommandPool pool = VK_NULL_HANDLE;
    VkCommandPoolCreateInfo poolInfo{};
    poolInfo.sType            = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
    poolInfo.flags            = VK_COMMAND_POOL_CREATE_TRANSIENT_BIT;
    poolInfo.queueFamilyIndex = device_->graphics().family;
    HARPIA_VK_CHECK(vkCreateCommandPool(device, &poolInfo, nullptr, &pool));

    VkCommandBuffer cmd = VK_NULL_HANDLE;
    VkCommandBufferAllocateInfo cmdAlloc{};
    cmdAlloc.sType              = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    cmdAlloc.commandPool        = pool;
    cmdAlloc.level              = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    cmdAlloc.commandBufferCount = 1;
    HARPIA_VK_CHECK(vkAllocateCommandBuffers(device, &cmdAlloc, &cmd));

    VkCommandBufferBeginInfo begin{};
    begin.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    HARPIA_VK_CHECK(vkBeginCommandBuffer(cmd, &begin));

    VkBufferImageCopy region{};
    region.imageSubresource.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    region.imageSubresource.layerCount = 1;
    region.imageExtent = VkExtent3D{width, height, 1};

    vkCmdCopyImageToBuffer(cmd, offscreenImage_,
                           VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                           staging, 1, &region);

    HARPIA_VK_CHECK(vkEndCommandBuffer(cmd));

    VkCommandBufferSubmitInfo cmdSubmit{};
    cmdSubmit.sType         = VK_STRUCTURE_TYPE_COMMAND_BUFFER_SUBMIT_INFO;
    cmdSubmit.commandBuffer = cmd;

    VkSubmitInfo2 submit{};
    submit.sType                  = VK_STRUCTURE_TYPE_SUBMIT_INFO_2;
    submit.commandBufferInfoCount = 1;
    submit.pCommandBufferInfos    = &cmdSubmit;

    HARPIA_VK_CHECK(vkQueueSubmit2(device_->graphics().queue, 1, &submit, VK_NULL_HANDLE));
    HARPIA_VK_CHECK(vkQueueWaitIdle(device_->graphics().queue));

    // Offscreen is R8G8B8A8_UNORM, so the mapped bytes are already RGBA8.
    outRgba.resize(static_cast<std::size_t>(size));
    std::memcpy(outRgba.data(), stagingInfo.pMappedData, static_cast<std::size_t>(size));

    vkDestroyCommandPool(device, pool, nullptr);
    vmaDestroyBuffer(device_->allocator(), staging, stagingAlloc);
    return true;
}

bool VulkanRenderer::captureToPng(const std::string& path)
{
    std::vector<std::uint8_t> pixels;
    if (!readback(pixels)) {
        return false;
    }

    const int width  = static_cast<int>(offscreenExtent_.width);
    const int height = static_cast<int>(offscreenExtent_.height);

    if (stbi_write_png(path.c_str(), width, height, 4, pixels.data(), width * 4) == 0) {
        std::fprintf(stderr, "[vulkan] stbi_write_png failed for %s\n", path.c_str());
        return false;
    }
    return true;
}

} // namespace harpia::rhi
