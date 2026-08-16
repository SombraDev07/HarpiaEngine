#include "RHI/Vulkan/VulkanBindless.h"

#include "RHI/Vulkan/VulkanDevice.h"

#include <array>
#include <cstdio>

namespace harpia::rhi {

std::uint32_t VulkanBindless::IndexAllocator::allocate()
{
    if (!freed.empty()) {
        const std::uint32_t index = freed.back();
        freed.pop_back();
        return index;
    }
    if (next >= capacity) {
        return VulkanBindless::kInvalidIndex;
    }
    return next++;
}

void VulkanBindless::IndexAllocator::release(std::uint32_t index)
{
    if (index < next) {
        freed.push_back(index);
    }
}

std::uint32_t VulkanBindless::IndexAllocator::used() const noexcept
{
    return next - static_cast<std::uint32_t>(freed.size());
}

VulkanBindless::~VulkanBindless()
{
    destroy();
}

bool VulkanBindless::create(const VulkanDevice& device)
{
    device_ = device.device();

    const VulkanDevice::Limits& limits = device.limits();

    // Shrinking to fit the device would leave the shaders clamping against a
    // bound that no longer exists, which is the one failure this whole scheme
    // is meant to rule out. Refuse instead.
    if (limits.maxSampledImages < kMaxSampledImages
        || limits.maxStorageBuffers < kMaxStorageBuffers
        || limits.maxSamplers < kMaxSamplers) {
        std::fprintf(stderr,
                     "[bindless] device grants fewer descriptors than the shaders "
                     "assume: images %u/%u, buffers %u/%u, samplers %u/%u\n",
                     limits.maxSampledImages, kMaxSampledImages,
                     limits.maxStorageBuffers, kMaxStorageBuffers,
                     limits.maxSamplers, kMaxSamplers);
        return false;
    }

    capacity_.sampledImages  = kMaxSampledImages;
    capacity_.storageBuffers = kMaxStorageBuffers;
    capacity_.samplers       = kMaxSamplers;
    capacity_.storageImages  = kMaxStorageImages;

    sampledImages_.capacity  = capacity_.sampledImages;
    storageBuffers_.capacity = capacity_.storageBuffers;
    samplers_.capacity       = capacity_.samplers;
    storageImages_.capacity  = capacity_.storageImages;

    const std::array<VkDescriptorPoolSize, 4> poolSizes{{
        {VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,  capacity_.sampledImages},
        {VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, capacity_.storageBuffers},
        {VK_DESCRIPTOR_TYPE_SAMPLER,        capacity_.samplers},
        {VK_DESCRIPTOR_TYPE_STORAGE_IMAGE,  capacity_.storageImages},
    }};

    VkDescriptorPoolCreateInfo poolInfo{};
    poolInfo.sType         = VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO;
    poolInfo.flags         = VK_DESCRIPTOR_POOL_CREATE_UPDATE_AFTER_BIND_BIT;
    poolInfo.maxSets       = 1;
    poolInfo.poolSizeCount = static_cast<std::uint32_t>(poolSizes.size());
    poolInfo.pPoolSizes    = poolSizes.data();
    HARPIA_VK_CHECK(vkCreateDescriptorPool(device_, &poolInfo, nullptr, &pool_));

    const std::array<VkDescriptorSetLayoutBinding, 4> bindings{{
        {static_cast<std::uint32_t>(BindlessBinding::SampledImages),
         VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE, capacity_.sampledImages,
         VK_SHADER_STAGE_ALL, nullptr},
        {static_cast<std::uint32_t>(BindlessBinding::StorageBuffers),
         VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, capacity_.storageBuffers,
         VK_SHADER_STAGE_ALL, nullptr},
        {static_cast<std::uint32_t>(BindlessBinding::Samplers),
         VK_DESCRIPTOR_TYPE_SAMPLER, capacity_.samplers,
         VK_SHADER_STAGE_ALL, nullptr},
        {static_cast<std::uint32_t>(BindlessBinding::StorageImages),
         VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, capacity_.storageImages,
         VK_SHADER_STAGE_ALL, nullptr},
    }};

    // PARTIALLY_BOUND is what lets the array hold holes: the shader may index a
    // slot that was never written as long as it does not read it.
    constexpr VkDescriptorBindingFlags kFlags =
        VK_DESCRIPTOR_BINDING_PARTIALLY_BOUND_BIT
        | VK_DESCRIPTOR_BINDING_UPDATE_AFTER_BIND_BIT
        | VK_DESCRIPTOR_BINDING_UPDATE_UNUSED_WHILE_PENDING_BIT;

    const std::array<VkDescriptorBindingFlags, 4> bindingFlags{kFlags, kFlags, kFlags, kFlags};

    VkDescriptorSetLayoutBindingFlagsCreateInfo flagsInfo{};
    flagsInfo.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_BINDING_FLAGS_CREATE_INFO;
    flagsInfo.bindingCount  = static_cast<std::uint32_t>(bindingFlags.size());
    flagsInfo.pBindingFlags = bindingFlags.data();

    VkDescriptorSetLayoutCreateInfo layoutInfo{};
    layoutInfo.sType        = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO;
    layoutInfo.pNext        = &flagsInfo;
    layoutInfo.flags        = VK_DESCRIPTOR_SET_LAYOUT_CREATE_UPDATE_AFTER_BIND_POOL_BIT;
    layoutInfo.bindingCount = static_cast<std::uint32_t>(bindings.size());
    layoutInfo.pBindings    = bindings.data();
    HARPIA_VK_CHECK(vkCreateDescriptorSetLayout(device_, &layoutInfo, nullptr, &layout_));

    VkDescriptorSetAllocateInfo allocInfo{};
    allocInfo.sType              = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO;
    allocInfo.descriptorPool     = pool_;
    allocInfo.descriptorSetCount = 1;
    allocInfo.pSetLayouts        = &layout_;
    HARPIA_VK_CHECK(vkAllocateDescriptorSets(device_, &allocInfo, &set_));

    setDebugName(device_, VK_OBJECT_TYPE_DESCRIPTOR_SET, set_, "Bindless_GlobalSet");
    setDebugName(device_, VK_OBJECT_TYPE_DESCRIPTOR_SET_LAYOUT, layout_, "Bindless_Layout");
    setDebugName(device_, VK_OBJECT_TYPE_DESCRIPTOR_POOL, pool_, "Bindless_Pool");

    return true;
}

void VulkanBindless::destroy()
{
    if (device_ == VK_NULL_HANDLE) {
        return;
    }
    if (layout_ != VK_NULL_HANDLE) {
        vkDestroyDescriptorSetLayout(device_, layout_, nullptr);
        layout_ = VK_NULL_HANDLE;
    }
    if (pool_ != VK_NULL_HANDLE) {
        vkDestroyDescriptorPool(device_, pool_, nullptr); // frees the set too
        pool_ = VK_NULL_HANDLE;
    }
    set_    = VK_NULL_HANDLE;
    device_ = VK_NULL_HANDLE;
}

std::uint32_t VulkanBindless::registerSampledImage(VkImageView view, VkImageLayout layout)
{
    const std::uint32_t index = sampledImages_.allocate();
    if (index == kInvalidIndex) {
        return kInvalidIndex;
    }

    VkDescriptorImageInfo imageInfo{};
    imageInfo.imageView   = view;
    imageInfo.imageLayout = layout;

    VkWriteDescriptorSet write{};
    write.sType           = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET;
    write.dstSet          = set_;
    write.dstBinding      = static_cast<std::uint32_t>(BindlessBinding::SampledImages);
    write.dstArrayElement = index;
    write.descriptorCount = 1;
    write.descriptorType  = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE;
    write.pImageInfo      = &imageInfo;

    vkUpdateDescriptorSets(device_, 1, &write, 0, nullptr);
    return index;
}

std::uint32_t VulkanBindless::registerStorageImage(VkImageView view)
{
    const std::uint32_t index = storageImages_.allocate();
    if (index == kInvalidIndex) {
        return kInvalidIndex;
    }

    VkDescriptorImageInfo imageInfo{};
    imageInfo.imageView   = view;
    imageInfo.imageLayout = VK_IMAGE_LAYOUT_GENERAL;

    VkWriteDescriptorSet write{};
    write.sType           = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET;
    write.dstSet          = set_;
    write.dstBinding      = static_cast<std::uint32_t>(BindlessBinding::StorageImages);
    write.dstArrayElement = index;
    write.descriptorCount = 1;
    write.descriptorType  = VK_DESCRIPTOR_TYPE_STORAGE_IMAGE;
    write.pImageInfo      = &imageInfo;

    vkUpdateDescriptorSets(device_, 1, &write, 0, nullptr);
    return index;
}

std::uint32_t VulkanBindless::registerSampler(VkSampler sampler)
{
    const std::uint32_t index = samplers_.allocate();
    if (index == kInvalidIndex) {
        return kInvalidIndex;
    }

    VkDescriptorImageInfo imageInfo{};
    imageInfo.sampler = sampler;

    VkWriteDescriptorSet write{};
    write.sType           = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET;
    write.dstSet          = set_;
    write.dstBinding      = static_cast<std::uint32_t>(BindlessBinding::Samplers);
    write.dstArrayElement = index;
    write.descriptorCount = 1;
    write.descriptorType  = VK_DESCRIPTOR_TYPE_SAMPLER;
    write.pImageInfo      = &imageInfo;

    vkUpdateDescriptorSets(device_, 1, &write, 0, nullptr);
    return index;
}

std::uint32_t VulkanBindless::registerStorageBuffer(VkBuffer     buffer,
                                                    VkDeviceSize offset,
                                                    VkDeviceSize range)
{
    const std::uint32_t index = storageBuffers_.allocate();
    if (index == kInvalidIndex) {
        return kInvalidIndex;
    }

    VkDescriptorBufferInfo bufferInfo{};
    bufferInfo.buffer = buffer;
    bufferInfo.offset = offset;
    bufferInfo.range  = range;

    VkWriteDescriptorSet write{};
    write.sType           = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET;
    write.dstSet          = set_;
    write.dstBinding      = static_cast<std::uint32_t>(BindlessBinding::StorageBuffers);
    write.dstArrayElement = index;
    write.descriptorCount = 1;
    write.descriptorType  = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER;
    write.pBufferInfo     = &bufferInfo;

    vkUpdateDescriptorSets(device_, 1, &write, 0, nullptr);
    return index;
}

void VulkanBindless::releaseSampledImage(std::uint32_t index)  { sampledImages_.release(index); }
void VulkanBindless::releaseStorageImage(std::uint32_t index)  { storageImages_.release(index); }
void VulkanBindless::releaseSampler(std::uint32_t index)       { samplers_.release(index); }
void VulkanBindless::releaseStorageBuffer(std::uint32_t index) { storageBuffers_.release(index); }

VulkanBindless::Usage VulkanBindless::usage() const noexcept
{
    Usage u;
    u.sampledImages  = sampledImages_.used();
    u.storageBuffers = storageBuffers_.used();
    u.samplers       = samplers_.used();
    u.storageImages  = storageImages_.used();
    return u;
}

} // namespace harpia::rhi
