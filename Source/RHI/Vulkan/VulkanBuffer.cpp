#include "RHI/Vulkan/VulkanBuffer.h"

#include "RHI/Vulkan/VulkanDevice.h"

#include <vk_mem_alloc.h>

#include <cstring>
#include <utility>

namespace harpia::rhi {

VulkanBuffer::~VulkanBuffer()
{
    destroy();
}

VulkanBuffer::VulkanBuffer(VulkanBuffer&& other) noexcept
    : device_(other.device_)
    , buffer_(other.buffer_)
    , allocation_(other.allocation_)
    , size_(other.size_)
    , mapped_(other.mapped_)
{
    other.device_     = nullptr;
    other.buffer_     = VK_NULL_HANDLE;
    other.allocation_ = nullptr;
    other.size_       = 0;
    other.mapped_     = nullptr;
}

VulkanBuffer& VulkanBuffer::operator=(VulkanBuffer&& other) noexcept
{
    if (this != &other) {
        destroy();
        device_     = std::exchange(other.device_, nullptr);
        buffer_     = std::exchange(other.buffer_, VK_NULL_HANDLE);
        allocation_ = std::exchange(other.allocation_, nullptr);
        size_       = std::exchange(other.size_, VkDeviceSize{0});
        mapped_     = std::exchange(other.mapped_, nullptr);
    }
    return *this;
}

bool VulkanBuffer::create(VulkanDevice& device, const BufferDesc& desc)
{
    if (desc.size == 0) {
        return false;
    }

    device_ = &device;
    size_   = desc.size;

    VkBufferCreateInfo bufferInfo{};
    bufferInfo.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO;
    bufferInfo.size  = desc.size;
    bufferInfo.usage = desc.usage;
    if (desc.memory == BufferMemory::DeviceLocal) {
        // Device-local memory can only be filled through a copy, and TRANSFER_SRC
        // keeps it readable back for tests and debugging.
        bufferInfo.usage |= VK_BUFFER_USAGE_TRANSFER_DST_BIT
                          | VK_BUFFER_USAGE_TRANSFER_SRC_BIT;
    }

    VmaAllocationCreateInfo allocInfo{};
    allocInfo.usage = VMA_MEMORY_USAGE_AUTO;
    if (desc.memory == BufferMemory::HostVisible) {
        allocInfo.flags = VMA_ALLOCATION_CREATE_HOST_ACCESS_SEQUENTIAL_WRITE_BIT
                        | VMA_ALLOCATION_CREATE_MAPPED_BIT;
    }

    VmaAllocationInfo allocationInfo{};
    const VkResult result = vmaCreateBuffer(device.allocator(), &bufferInfo, &allocInfo,
                                            &buffer_, &allocation_, &allocationInfo);
    if (result != VK_SUCCESS) {
        std::fprintf(stderr, "[vulkan] vmaCreateBuffer failed: %s\n", resultToString(result));
        device_ = nullptr;
        size_   = 0;
        return false;
    }

    mapped_ = allocationInfo.pMappedData;
    setDebugName(device.device(), VK_OBJECT_TYPE_BUFFER, buffer_, desc.debugName);
    return true;
}

void VulkanBuffer::destroy()
{
    if (device_ != nullptr && buffer_ != VK_NULL_HANDLE) {
        vmaDestroyBuffer(device_->allocator(), buffer_, allocation_);
    }
    device_     = nullptr;
    buffer_     = VK_NULL_HANDLE;
    allocation_ = nullptr;
    size_       = 0;
    mapped_     = nullptr;
}

VkDeviceAddress VulkanBuffer::deviceAddress() const
{
    if (device_ == nullptr || buffer_ == VK_NULL_HANDLE) {
        return 0;
    }
    VkBufferDeviceAddressInfo info{};
    info.sType  = VK_STRUCTURE_TYPE_BUFFER_DEVICE_ADDRESS_INFO;
    info.buffer = buffer_;
    return vkGetBufferDeviceAddress(device_->device(), &info);
}

// --- GpuUploader ------------------------------------------------------------

bool GpuUploader::create(VulkanDevice& device)
{
    device_ = &device;

    VkCommandPoolCreateInfo poolInfo{};
    poolInfo.sType            = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
    poolInfo.flags            = VK_COMMAND_POOL_CREATE_TRANSIENT_BIT;
    poolInfo.queueFamilyIndex = device.graphics().family;

    if (vkCreateCommandPool(device.device(), &poolInfo, nullptr, &pool_) != VK_SUCCESS) {
        device_ = nullptr;
        return false;
    }
    setDebugName(device.device(), VK_OBJECT_TYPE_COMMAND_POOL, pool_, "Uploader_CmdPool");
    return true;
}

void GpuUploader::destroy()
{
    if (device_ != nullptr && pool_ != VK_NULL_HANDLE) {
        vkDestroyCommandPool(device_->device(), pool_, nullptr);
    }
    pool_   = VK_NULL_HANDLE;
    device_ = nullptr;
}

bool GpuUploader::upload(VulkanBuffer& destination,
                         const void*   data,
                         VkDeviceSize  size,
                         VkDeviceSize  offset)
{
    if (device_ == nullptr || !destination.valid() || data == nullptr || size == 0) {
        return false;
    }
    if (offset + size > destination.size()) {
        std::fprintf(stderr, "[vulkan] upload of %llu bytes at %llu overruns a %llu byte buffer\n",
                     static_cast<unsigned long long>(size),
                     static_cast<unsigned long long>(offset),
                     static_cast<unsigned long long>(destination.size()));
        return false;
    }

    // A host-visible destination needs no staging step at all.
    if (destination.mapped() != nullptr) {
        std::memcpy(static_cast<std::uint8_t*>(destination.mapped()) + offset, data,
                    static_cast<std::size_t>(size));
        return true;
    }

    BufferDesc stagingDesc;
    stagingDesc.size      = size;
    stagingDesc.usage     = VK_BUFFER_USAGE_TRANSFER_SRC_BIT;
    stagingDesc.memory    = BufferMemory::HostVisible;
    stagingDesc.debugName = "Staging_Upload";

    VulkanBuffer staging;
    if (!staging.create(*device_, stagingDesc) || staging.mapped() == nullptr) {
        return false;
    }
    std::memcpy(staging.mapped(), data, static_cast<std::size_t>(size));

    VkCommandBuffer cmd = VK_NULL_HANDLE;
    VkCommandBufferAllocateInfo allocInfo{};
    allocInfo.sType              = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    allocInfo.commandPool        = pool_;
    allocInfo.level              = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    allocInfo.commandBufferCount = 1;
    HARPIA_VK_CHECK(vkAllocateCommandBuffers(device_->device(), &allocInfo, &cmd));

    VkCommandBufferBeginInfo begin{};
    begin.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    HARPIA_VK_CHECK(vkBeginCommandBuffer(cmd, &begin));

    VkBufferCopy region{};
    region.dstOffset = offset;
    region.size      = size;
    vkCmdCopyBuffer(cmd, staging.handle(), destination.handle(), 1, &region);

    // Make the copy visible to every later shader read. This is a load-time
    // path, so one broad barrier costs nothing measurable and avoids the
    // caller having to know what will read the buffer.
    VkBufferMemoryBarrier2 barrier{};
    barrier.sType         = VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER_2;
    barrier.srcStageMask  = VK_PIPELINE_STAGE_2_COPY_BIT;
    barrier.srcAccessMask = VK_ACCESS_2_TRANSFER_WRITE_BIT;
    barrier.dstStageMask  = VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT;
    barrier.dstAccessMask = VK_ACCESS_2_MEMORY_READ_BIT;
    barrier.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.buffer        = destination.handle();
    barrier.offset        = offset;
    barrier.size          = size;

    VkDependencyInfo dependency{};
    dependency.sType                    = VK_STRUCTURE_TYPE_DEPENDENCY_INFO;
    dependency.bufferMemoryBarrierCount = 1;
    dependency.pBufferMemoryBarriers    = &barrier;
    vkCmdPipelineBarrier2(cmd, &dependency);

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

    vkFreeCommandBuffers(device_->device(), pool_, 1, &cmd);
    return true;
}

bool GpuUploader::download(const VulkanBuffer& source,
                           void*               destination,
                           VkDeviceSize        size,
                           VkDeviceSize        offset)
{
    if (device_ == nullptr || !source.valid() || destination == nullptr || size == 0) {
        return false;
    }
    if (offset + size > source.size()) {
        return false;
    }

    if (source.mapped() != nullptr) {
        std::memcpy(destination,
                    static_cast<const std::uint8_t*>(source.mapped()) + offset,
                    static_cast<std::size_t>(size));
        return true;
    }

    BufferDesc stagingDesc;
    stagingDesc.size      = size;
    stagingDesc.usage     = VK_BUFFER_USAGE_TRANSFER_DST_BIT;
    stagingDesc.memory    = BufferMemory::HostVisible;
    stagingDesc.debugName = "Staging_Download";

    VulkanBuffer staging;
    if (!staging.create(*device_, stagingDesc) || staging.mapped() == nullptr) {
        return false;
    }

    VkCommandBuffer cmd = VK_NULL_HANDLE;
    VkCommandBufferAllocateInfo allocInfo{};
    allocInfo.sType              = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    allocInfo.commandPool        = pool_;
    allocInfo.level              = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    allocInfo.commandBufferCount = 1;
    HARPIA_VK_CHECK(vkAllocateCommandBuffers(device_->device(), &allocInfo, &cmd));

    VkCommandBufferBeginInfo begin{};
    begin.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    HARPIA_VK_CHECK(vkBeginCommandBuffer(cmd, &begin));

    VkBufferCopy region{};
    region.srcOffset = offset;
    region.size      = size;
    vkCmdCopyBuffer(cmd, source.handle(), staging.handle(), 1, &region);

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

    std::memcpy(destination, staging.mapped(), static_cast<std::size_t>(size));
    vkFreeCommandBuffers(device_->device(), pool_, 1, &cmd);
    return true;
}

bool GpuUploader::downloadImage(VkImage                    image,
                                VkImageLayout              currentLayout,
                                VkExtent2D                 extent,
                                std::uint32_t              texelBytes,
                                std::vector<std::uint8_t>& outTexels,
                                VkImageAspectFlags         aspect,
                                std::uint32_t              baseArrayLayer,
                                std::uint32_t              mipLevel)
{
    if (device_ == nullptr || image == VK_NULL_HANDLE || texelBytes == 0) {
        return false;
    }

    const VkDeviceSize size = VkDeviceSize{extent.width} * extent.height * texelBytes;

    BufferDesc stagingDesc;
    stagingDesc.size      = size;
    stagingDesc.usage     = VK_BUFFER_USAGE_TRANSFER_DST_BIT;
    stagingDesc.memory    = BufferMemory::HostVisible;
    stagingDesc.debugName = "Staging_ImageDownload";

    VulkanBuffer staging;
    if (!staging.create(*device_, stagingDesc) || staging.mapped() == nullptr) {
        return false;
    }

    VkCommandBuffer cmd = VK_NULL_HANDLE;
    VkCommandBufferAllocateInfo allocInfo{};
    allocInfo.sType              = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    allocInfo.commandPool        = pool_;
    allocInfo.level              = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    allocInfo.commandBufferCount = 1;
    HARPIA_VK_CHECK(vkAllocateCommandBuffers(device_->device(), &allocInfo, &cmd));

    VkCommandBufferBeginInfo begin{};
    begin.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    HARPIA_VK_CHECK(vkBeginCommandBuffer(cmd, &begin));

    // The caller says where the image is; we bring it to TRANSFER_SRC and put
    // it back, so the graph's own layout tracking stays true.
    imageBarrier(cmd, image, currentLayout, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                 VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT, VK_ACCESS_2_MEMORY_WRITE_BIT,
                 VK_PIPELINE_STAGE_2_COPY_BIT, VK_ACCESS_2_TRANSFER_READ_BIT, aspect);

    VkBufferImageCopy region{};
    region.imageSubresource.aspectMask     = aspect;
    region.imageSubresource.mipLevel       = mipLevel;
    region.imageSubresource.baseArrayLayer = baseArrayLayer;
    region.imageSubresource.layerCount     = 1;
    region.imageExtent = VkExtent3D{extent.width, extent.height, 1};
    vkCmdCopyImageToBuffer(cmd, image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                           staging.handle(), 1, &region);

    imageBarrier(cmd, image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, currentLayout,
                 VK_PIPELINE_STAGE_2_COPY_BIT, VK_ACCESS_2_TRANSFER_READ_BIT,
                 VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT, VK_ACCESS_2_MEMORY_READ_BIT, aspect);

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

    outTexels.resize(static_cast<std::size_t>(size));
    std::memcpy(outTexels.data(), staging.mapped(), static_cast<std::size_t>(size));

    vkFreeCommandBuffers(device_->device(), pool_, 1, &cmd);
    return true;
}

} // namespace harpia::rhi
