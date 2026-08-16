#include "RHI/IblResources.h"

#include "RHI/RenderTypes.h"
#include "RHI/Vulkan/VulkanBuffer.h"
#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanPipeline.h"

#include <vk_mem_alloc.h>

#include <cstdio>
#include <cstring>
#include <vector>

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

namespace {

// IEEE 754 binary32 to binary16. The cube stores half floats because
// R16G16B16A16_SFLOAT is the widest format Vulkan *requires* to support linear
// filtering; R32G32B32A32_SFLOAT is not, and a nearest-filtered environment
// projects the equirect as visible blocks.
std::uint16_t floatToHalf(float value)
{
    std::uint32_t bits = 0;
    std::memcpy(&bits, &value, sizeof(bits));

    const std::uint32_t sign     = (bits >> 16) & 0x8000u;
    const std::int32_t  exponent = static_cast<std::int32_t>((bits >> 23) & 0xFFu) - 127;
    const std::uint32_t mantissa = bits & 0x7FFFFFu;

    if (exponent > 15) {                     // overflow, and NaN keeps its payload
        return static_cast<std::uint16_t>(sign | 0x7C00u | (mantissa != 0 ? 1u : 0u));
    }
    if (exponent < -14) {                    // underflow to zero rather than denormal
        return static_cast<std::uint16_t>(sign);
    }
    return static_cast<std::uint16_t>(
        sign | (static_cast<std::uint32_t>(exponent + 15) << 10) | (mantissa >> 13));
}

} // namespace

bool IblResources::loadEnvironment(VulkanDevice&        device,
                                   VulkanBindless&      bindless,
                                   const HdrImageAsset& equirect,
                                   const std::string&   shaderDirectory)
{
    if (equirect.empty() || equirect.width == 0 || equirect.height == 0) {
        std::fprintf(stderr, "[ibl] environment source is empty\n");
        return false;
    }

    device_   = &device;
    bindless_ = &bindless;

    constexpr VkFormat kFormat = VK_FORMAT_R16G16B16A16_SFLOAT;

    // --- the equirectangular source, resident only for this conversion -------
    const std::size_t texelCount = static_cast<std::size_t>(equirect.width)
                                 * static_cast<std::size_t>(equirect.height) * 4;

    std::vector<std::uint16_t> halves(texelCount);
    for (std::size_t i = 0; i < texelCount; ++i) {
        halves[i] = floatToHalf(equirect.pixels[i]);
    }

    VkImageCreateInfo sourceInfo{};
    sourceInfo.sType         = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
    sourceInfo.imageType     = VK_IMAGE_TYPE_2D;
    sourceInfo.format        = kFormat;
    sourceInfo.extent        = VkExtent3D{equirect.width, equirect.height, 1};
    sourceInfo.mipLevels     = 1;
    sourceInfo.arrayLayers   = 1;
    sourceInfo.samples       = VK_SAMPLE_COUNT_1_BIT;
    sourceInfo.tiling        = VK_IMAGE_TILING_OPTIMAL;
    sourceInfo.usage         = VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_SAMPLED_BIT;
    sourceInfo.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;

    VmaAllocationCreateInfo allocInfo{};
    allocInfo.usage = VMA_MEMORY_USAGE_AUTO;

    VkImage       sourceImage      = VK_NULL_HANDLE;
    VmaAllocation sourceAllocation = nullptr;
    if (vmaCreateImage(device.allocator(), &sourceInfo, &allocInfo,
                       &sourceImage, &sourceAllocation, nullptr) != VK_SUCCESS) {
        return false;
    }

    BufferDesc stagingDesc;
    stagingDesc.size      = halves.size() * sizeof(std::uint16_t);
    stagingDesc.usage     = VK_BUFFER_USAGE_TRANSFER_SRC_BIT;
    stagingDesc.memory    = BufferMemory::HostVisible;
    stagingDesc.debugName = "Staging_Equirect";

    VulkanBuffer staging;
    if (!staging.create(device, stagingDesc) || staging.mapped() == nullptr) {
        vmaDestroyImage(device.allocator(), sourceImage, sourceAllocation);
        return false;
    }
    std::memcpy(staging.mapped(), halves.data(), static_cast<std::size_t>(stagingDesc.size));

    VkImageViewCreateInfo sourceViewInfo{};
    sourceViewInfo.sType    = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
    sourceViewInfo.image    = sourceImage;
    sourceViewInfo.viewType = VK_IMAGE_VIEW_TYPE_2D;
    sourceViewInfo.format   = kFormat;
    sourceViewInfo.subresourceRange.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    sourceViewInfo.subresourceRange.levelCount = 1;
    sourceViewInfo.subresourceRange.layerCount = 1;

    VkImageView sourceView = VK_NULL_HANDLE;
    HARPIA_VK_CHECK(vkCreateImageView(device.device(), &sourceViewInfo, nullptr, &sourceView));

    setDebugName(device.device(), VK_OBJECT_TYPE_IMAGE, sourceImage, "Ibl_EquirectSource");

    // --- the cube ------------------------------------------------------------
    VkImageCreateInfo cubeInfo{};
    cubeInfo.sType         = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
    cubeInfo.flags         = VK_IMAGE_CREATE_CUBE_COMPATIBLE_BIT;
    cubeInfo.imageType     = VK_IMAGE_TYPE_2D;
    cubeInfo.format        = kFormat;
    cubeInfo.extent        = VkExtent3D{kEnvironmentSize, kEnvironmentSize, 1};
    cubeInfo.mipLevels     = 1;
    cubeInfo.arrayLayers   = kCubeFaces;
    cubeInfo.samples       = VK_SAMPLE_COUNT_1_BIT;
    cubeInfo.tiling        = VK_IMAGE_TILING_OPTIMAL;
    cubeInfo.usage         = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT
                           | VK_IMAGE_USAGE_SAMPLED_BIT
                           | VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
    cubeInfo.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;

    if (vmaCreateImage(device.allocator(), &cubeInfo, &allocInfo,
                       &envImage_, &envAllocation_, nullptr) != VK_SUCCESS) {
        vkDestroyImageView(device.device(), sourceView, nullptr);
        vmaDestroyImage(device.allocator(), sourceImage, sourceAllocation);
        return false;
    }

    setDebugName(device.device(), VK_OBJECT_TYPE_IMAGE, envImage_, "Ibl_Environment");

    // One 2D view per face to render into, plus a cube view to sample from.
    // Rendering through the cube view is not possible: an attachment is a single
    // layer, and the face is which layer.
    for (std::uint32_t face = 0; face < kCubeFaces; ++face) {
        VkImageViewCreateInfo faceInfo{};
        faceInfo.sType    = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
        faceInfo.image    = envImage_;
        faceInfo.viewType = VK_IMAGE_VIEW_TYPE_2D;
        faceInfo.format   = kFormat;
        faceInfo.subresourceRange.aspectMask     = VK_IMAGE_ASPECT_COLOR_BIT;
        faceInfo.subresourceRange.levelCount     = 1;
        faceInfo.subresourceRange.baseArrayLayer = face;
        faceInfo.subresourceRange.layerCount     = 1;
        HARPIA_VK_CHECK(vkCreateImageView(device.device(), &faceInfo, nullptr,
                                          &envFaceViews_[face]));
    }

    VkImageViewCreateInfo cubeViewInfo{};
    cubeViewInfo.sType    = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
    cubeViewInfo.image    = envImage_;
    cubeViewInfo.viewType = VK_IMAGE_VIEW_TYPE_CUBE;
    cubeViewInfo.format   = kFormat;
    cubeViewInfo.subresourceRange.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    cubeViewInfo.subresourceRange.levelCount = 1;
    cubeViewInfo.subresourceRange.layerCount = kCubeFaces;
    HARPIA_VK_CHECK(vkCreateImageView(device.device(), &cubeViewInfo, nullptr, &envCubeView_));

    setDebugName(device.device(), VK_OBJECT_TYPE_IMAGE_VIEW, envCubeView_, "Ibl_EnvironmentCube");

    // --- project -------------------------------------------------------------
    const std::uint32_t sourceIndex = bindless.registerSampledImage(
        sourceView, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    if (sourceIndex == VulkanBindless::kInvalidIndex) {
        std::fprintf(stderr, "[ibl] bindless sampled image slots exhausted\n");
        vkDestroyImageView(device.device(), sourceView, nullptr);
        vmaDestroyImage(device.allocator(), sourceImage, sourceAllocation);
        destroyEnvironment();
        return false;
    }

    GraphicsPipelineDesc pipelineDesc;
    pipelineDesc.vertexSpirvPath     = shaderDirectory + "/Fullscreen.vert.spv";
    pipelineDesc.fragmentSpirvPath   = shaderDirectory + "/EquirectToCube.frag.spv";
    pipelineDesc.colorFormats        = {kFormat};
    pipelineDesc.descriptorSetLayout = bindless.layout();
    pipelineDesc.pushConstantBytes   = sizeof(CubePushConstants);
    pipelineDesc.debugName           = "Pipeline_EquirectToCube";

    VulkanPipeline pipeline;
    if (!pipeline.create(device, pipelineDesc)) {
        bindless.releaseSampledImage(sourceIndex);
        vkDestroyImageView(device.device(), sourceView, nullptr);
        vmaDestroyImage(device.allocator(), sourceImage, sourceAllocation);
        destroyEnvironment();
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

    // The source moves host bytes to sampled; the cube moves to attachment. Both
    // are whole-image transitions, so the six-layer range is one barrier.
    imageBarrier(cmd, sourceImage, VK_IMAGE_LAYOUT_UNDEFINED,
                 VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                 VK_PIPELINE_STAGE_2_TOP_OF_PIPE_BIT, 0,
                 VK_PIPELINE_STAGE_2_COPY_BIT, VK_ACCESS_2_TRANSFER_WRITE_BIT);

    VkBufferImageCopy region{};
    region.imageSubresource.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    region.imageSubresource.layerCount = 1;
    region.imageExtent = VkExtent3D{equirect.width, equirect.height, 1};
    vkCmdCopyBufferToImage(cmd, staging.handle(), sourceImage,
                           VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);

    imageBarrier(cmd, sourceImage, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                 VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                 VK_PIPELINE_STAGE_2_COPY_BIT, VK_ACCESS_2_TRANSFER_WRITE_BIT,
                 VK_PIPELINE_STAGE_2_FRAGMENT_SHADER_BIT, VK_ACCESS_2_SHADER_SAMPLED_READ_BIT);

    VkImageMemoryBarrier2 toAttachment{};
    toAttachment.sType         = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER_2;
    toAttachment.srcStageMask  = VK_PIPELINE_STAGE_2_TOP_OF_PIPE_BIT;
    toAttachment.dstStageMask  = VK_PIPELINE_STAGE_2_COLOR_ATTACHMENT_OUTPUT_BIT;
    toAttachment.dstAccessMask = VK_ACCESS_2_COLOR_ATTACHMENT_WRITE_BIT;
    toAttachment.oldLayout     = VK_IMAGE_LAYOUT_UNDEFINED;
    toAttachment.newLayout     = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
    toAttachment.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    toAttachment.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    toAttachment.image = envImage_;
    toAttachment.subresourceRange.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
    toAttachment.subresourceRange.levelCount = 1;
    toAttachment.subresourceRange.layerCount = kCubeFaces;

    VkDependencyInfo dependency{};
    dependency.sType                   = VK_STRUCTURE_TYPE_DEPENDENCY_INFO;
    dependency.imageMemoryBarrierCount = 1;
    dependency.pImageMemoryBarriers    = &toAttachment;
    vkCmdPipelineBarrier2(cmd, &dependency);

    const VkDescriptorSet descriptorSet = bindless.set();
    vkCmdBindDescriptorSets(cmd, VK_PIPELINE_BIND_POINT_GRAPHICS, pipeline.layout(),
                            0, 1, &descriptorSet, 0, nullptr);

    for (std::uint32_t face = 0; face < kCubeFaces; ++face) {
        VkRenderingAttachmentInfo attachment{};
        attachment.sType       = VK_STRUCTURE_TYPE_RENDERING_ATTACHMENT_INFO;
        attachment.imageView   = envFaceViews_[face];
        attachment.imageLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
        attachment.loadOp      = VK_ATTACHMENT_LOAD_OP_DONT_CARE;
        attachment.storeOp     = VK_ATTACHMENT_STORE_OP_STORE;

        VkRenderingInfo rendering{};
        rendering.sType                = VK_STRUCTURE_TYPE_RENDERING_INFO;
        rendering.renderArea.extent    = VkExtent2D{kEnvironmentSize, kEnvironmentSize};
        rendering.layerCount           = 1;
        rendering.colorAttachmentCount = 1;
        rendering.pColorAttachments    = &attachment;

        CubePushConstants push;
        push.sourceTexture = sourceIndex;
        push.face          = face;

        vkCmdBeginRendering(cmd, &rendering);
        pipeline.bind(cmd);
        pipeline.setViewportAndScissor(cmd, VkExtent2D{kEnvironmentSize, kEnvironmentSize});
        vkCmdPushConstants(cmd, pipeline.layout(), VK_SHADER_STAGE_ALL, 0,
                           sizeof(push), &push);
        vkCmdDraw(cmd, 3, 1, 0, 0);
        vkCmdEndRendering(cmd);
    }

    VkImageMemoryBarrier2 toSampled = toAttachment;
    toSampled.srcStageMask  = VK_PIPELINE_STAGE_2_COLOR_ATTACHMENT_OUTPUT_BIT;
    toSampled.srcAccessMask = VK_ACCESS_2_COLOR_ATTACHMENT_WRITE_BIT;
    toSampled.dstStageMask  = VK_PIPELINE_STAGE_2_FRAGMENT_SHADER_BIT;
    toSampled.dstAccessMask = VK_ACCESS_2_SHADER_SAMPLED_READ_BIT;
    toSampled.oldLayout     = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
    toSampled.newLayout     = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
    dependency.pImageMemoryBarriers = &toSampled;
    vkCmdPipelineBarrier2(cmd, &dependency);

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

    // The equirect existed only to be projected; nothing samples it afterwards.
    bindless.releaseSampledImage(sourceIndex);
    vkDestroyImageView(device.device(), sourceView, nullptr);
    vmaDestroyImage(device.allocator(), sourceImage, sourceAllocation);
    staging.destroy();

    envIndex_ = bindless.registerSampledImage(envCubeView_,
                                              VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    if (envIndex_ == VulkanBindless::kInvalidIndex) {
        std::fprintf(stderr, "[ibl] bindless sampled image slots exhausted\n");
        destroyEnvironment();
        return false;
    }
    return true;
}

void IblResources::destroyEnvironment()
{
    if (device_ == nullptr) {
        return;
    }
    if (bindless_ != nullptr && envIndex_ != VulkanBindless::kInvalidIndex) {
        bindless_->releaseSampledImage(envIndex_);
        envIndex_ = VulkanBindless::kInvalidIndex;
    }
    for (VkImageView& face : envFaceViews_) {
        if (face != VK_NULL_HANDLE) {
            vkDestroyImageView(device_->device(), face, nullptr);
            face = VK_NULL_HANDLE;
        }
    }
    if (envCubeView_ != VK_NULL_HANDLE) {
        vkDestroyImageView(device_->device(), envCubeView_, nullptr);
        envCubeView_ = VK_NULL_HANDLE;
    }
    if (envImage_ != VK_NULL_HANDLE) {
        vmaDestroyImage(device_->allocator(), envImage_, envAllocation_);
        envImage_      = VK_NULL_HANDLE;
        envAllocation_ = nullptr;
    }
}

void IblResources::destroy()
{
    destroyEnvironment();

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
