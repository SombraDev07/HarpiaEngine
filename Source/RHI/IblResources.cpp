#include "RHI/IblResources.h"

#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanPipeline.h"

#include <vk_mem_alloc.h>

#include <cstdio>

namespace harpia::rhi {

IblResources::~IblResources()
{
    destroy();
}

bool IblResources::create(VulkanDevice&      device,
                          VulkanBindless&    bindless,
                          const std::string& shaderDirectory)
{
    device_   = &device;
    bindless_ = &bindless;

    // Two channels of half float: the table holds a scale and a bias, both in
    // [0,1], and 8 bits would band visibly in the grazing-angle rise.
    constexpr VkFormat kFormat = VK_FORMAT_R16G16_SFLOAT;

    VkImageCreateInfo imageInfo{};
    imageInfo.sType         = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
    imageInfo.imageType     = VK_IMAGE_TYPE_2D;
    imageInfo.format        = kFormat;
    imageInfo.extent        = VkExtent3D{kBrdfLutSize, kBrdfLutSize, 1};
    imageInfo.mipLevels     = 1;
    imageInfo.arrayLayers   = 1;
    imageInfo.samples       = VK_SAMPLE_COUNT_1_BIT;
    imageInfo.tiling        = VK_IMAGE_TILING_OPTIMAL;
    imageInfo.usage         = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT
                            | VK_IMAGE_USAGE_SAMPLED_BIT
                            | VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
    imageInfo.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;

    VmaAllocationCreateInfo allocInfo{};
    allocInfo.usage = VMA_MEMORY_USAGE_AUTO;

    if (vmaCreateImage(device.allocator(), &imageInfo, &allocInfo,
                       &image_, &allocation_, nullptr) != VK_SUCCESS) {
        return false;
    }

    VkImageViewCreateInfo viewInfo{};
    viewInfo.sType    = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
    viewInfo.image    = image_;
    viewInfo.viewType = VK_IMAGE_VIEW_TYPE_2D;
    viewInfo.format   = kFormat;
    viewInfo.subresourceRange.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    viewInfo.subresourceRange.levelCount = 1;
    viewInfo.subresourceRange.layerCount = 1;
    HARPIA_VK_CHECK(vkCreateImageView(device.device(), &viewInfo, nullptr, &view_));

    setDebugName(device.device(), VK_OBJECT_TYPE_IMAGE, image_, "Ibl_BrdfLut");
    setDebugName(device.device(), VK_OBJECT_TYPE_IMAGE_VIEW, view_, "Ibl_BrdfLutView");

    // --- render the table ---------------------------------------------------
    GraphicsPipelineDesc pipelineDesc;
    pipelineDesc.vertexSpirvPath   = shaderDirectory + "/Fullscreen.vert.spv";
    pipelineDesc.fragmentSpirvPath = shaderDirectory + "/BrdfLut.frag.spv";
    pipelineDesc.colorFormats      = {kFormat};
    pipelineDesc.debugName         = "Pipeline_BrdfLut";

    VulkanPipeline pipeline;
    if (!pipeline.create(device, pipelineDesc)) {
        destroy();
        return false;
    }

    VkCommandPool pool = VK_NULL_HANDLE;
    VkCommandPoolCreateInfo poolInfo{};
    poolInfo.sType            = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
    poolInfo.flags            = VK_COMMAND_POOL_CREATE_TRANSIENT_BIT;
    poolInfo.queueFamilyIndex = device.graphics().family;
    HARPIA_VK_CHECK(vkCreateCommandPool(device.device(), &poolInfo, nullptr, &pool));

    VkCommandBuffer cmd = VK_NULL_HANDLE;
    VkCommandBufferAllocateInfo cmdAlloc{};
    cmdAlloc.sType              = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    cmdAlloc.commandPool        = pool;
    cmdAlloc.level              = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    cmdAlloc.commandBufferCount = 1;
    HARPIA_VK_CHECK(vkAllocateCommandBuffers(device.device(), &cmdAlloc, &cmd));

    VkCommandBufferBeginInfo begin{};
    begin.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    HARPIA_VK_CHECK(vkBeginCommandBuffer(cmd, &begin));

    imageBarrier(cmd, image_, VK_IMAGE_LAYOUT_UNDEFINED,
                 VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
                 VK_PIPELINE_STAGE_2_TOP_OF_PIPE_BIT, 0,
                 VK_PIPELINE_STAGE_2_COLOR_ATTACHMENT_OUTPUT_BIT,
                 VK_ACCESS_2_COLOR_ATTACHMENT_WRITE_BIT);

    VkRenderingAttachmentInfo attachment{};
    attachment.sType       = VK_STRUCTURE_TYPE_RENDERING_ATTACHMENT_INFO;
    attachment.imageView   = view_;
    attachment.imageLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
    attachment.loadOp      = VK_ATTACHMENT_LOAD_OP_CLEAR;
    attachment.storeOp     = VK_ATTACHMENT_STORE_OP_STORE;

    VkRenderingInfo rendering{};
    rendering.sType                = VK_STRUCTURE_TYPE_RENDERING_INFO;
    rendering.renderArea.extent    = VkExtent2D{kBrdfLutSize, kBrdfLutSize};
    rendering.layerCount           = 1;
    rendering.colorAttachmentCount = 1;
    rendering.pColorAttachments    = &attachment;

    vkCmdBeginRendering(cmd, &rendering);
    pipeline.bind(cmd);
    pipeline.setViewportAndScissor(cmd, VkExtent2D{kBrdfLutSize, kBrdfLutSize});
    vkCmdDraw(cmd, 3, 1, 0, 0);
    vkCmdEndRendering(cmd);

    imageBarrier(cmd, image_, VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
                 VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                 VK_PIPELINE_STAGE_2_COLOR_ATTACHMENT_OUTPUT_BIT,
                 VK_ACCESS_2_COLOR_ATTACHMENT_WRITE_BIT,
                 VK_PIPELINE_STAGE_2_FRAGMENT_SHADER_BIT,
                 VK_ACCESS_2_SHADER_SAMPLED_READ_BIT);

    HARPIA_VK_CHECK(vkEndCommandBuffer(cmd));

    VkCommandBufferSubmitInfo cmdSubmit{};
    cmdSubmit.sType         = VK_STRUCTURE_TYPE_COMMAND_BUFFER_SUBMIT_INFO;
    cmdSubmit.commandBuffer = cmd;

    VkSubmitInfo2 submit{};
    submit.sType                  = VK_STRUCTURE_TYPE_SUBMIT_INFO_2;
    submit.commandBufferInfoCount = 1;
    submit.pCommandBufferInfos    = &cmdSubmit;

    HARPIA_VK_CHECK(vkQueueSubmit2(device.graphics().queue, 1, &submit, VK_NULL_HANDLE));
    HARPIA_VK_CHECK(vkQueueWaitIdle(device.graphics().queue));

    vkDestroyCommandPool(device.device(), pool, nullptr);
    pipeline.destroy();

    brdfLutIndex_ = bindless.registerSampledImage(
        view_, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    if (brdfLutIndex_ == VulkanBindless::kInvalidIndex) {
        std::fprintf(stderr, "[ibl] bindless sampled image slots exhausted\n");
        destroy();
        return false;
    }
    return true;
}

void IblResources::destroy()
{
    if (bindless_ != nullptr && brdfLutIndex_ != VulkanBindless::kInvalidIndex) {
        bindless_->releaseSampledImage(brdfLutIndex_);
        brdfLutIndex_ = VulkanBindless::kInvalidIndex;
    }
    if (device_ != nullptr) {
        if (view_ != VK_NULL_HANDLE) {
            vkDestroyImageView(device_->device(), view_, nullptr);
            view_ = VK_NULL_HANDLE;
        }
        if (image_ != VK_NULL_HANDLE) {
            vmaDestroyImage(device_->allocator(), image_, allocation_);
            image_ = VK_NULL_HANDLE;
            allocation_ = nullptr;
        }
    }
    device_   = nullptr;
    bindless_ = nullptr;
}

} // namespace harpia::rhi
