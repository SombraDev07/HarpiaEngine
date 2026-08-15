// Harpia Engine — global bindless descriptor heap
//
// Roadmap 1.5: bindless from the first triangle, not as a later optimisation.
// One descriptor set for the whole engine; a texture is a uint32 index that
// travels in a push constant or a buffer, never a bound slot.
#pragma once

#include "RHI/Vulkan/VulkanCommon.h"

#include <cstdint>
#include <vector>

namespace harpia::rhi {

class VulkanDevice;

// Binding slots inside the global set. Shaders declare arrays at these points.
enum class BindlessBinding : std::uint32_t {
    SampledImages  = 0,
    StorageBuffers = 1,
    Samplers       = 2,
    StorageImages  = 3,
};

class VulkanBindless {
public:
    static constexpr std::uint32_t kInvalidIndex = 0xFFFFFFFFu;

    VulkanBindless() = default;
    ~VulkanBindless();

    VulkanBindless(const VulkanBindless&)            = delete;
    VulkanBindless& operator=(const VulkanBindless&) = delete;

    [[nodiscard]] bool create(const VulkanDevice& device);
    void destroy();

    [[nodiscard]] VkDescriptorSetLayout layout() const noexcept { return layout_; }
    [[nodiscard]] VkDescriptorSet        set() const noexcept    { return set_; }

    // Each returns kInvalidIndex when that array is exhausted.
    [[nodiscard]] std::uint32_t registerSampledImage(VkImageView view, VkImageLayout layout);
    [[nodiscard]] std::uint32_t registerStorageImage(VkImageView view);
    [[nodiscard]] std::uint32_t registerSampler(VkSampler sampler);
    [[nodiscard]] std::uint32_t registerStorageBuffer(VkBuffer buffer,
                                                      VkDeviceSize offset,
                                                      VkDeviceSize range);

    void releaseSampledImage(std::uint32_t index);
    void releaseStorageImage(std::uint32_t index);
    void releaseSampler(std::uint32_t index);
    void releaseStorageBuffer(std::uint32_t index);

    struct Capacity {
        std::uint32_t sampledImages  = 0;
        std::uint32_t storageBuffers = 0;
        std::uint32_t samplers       = 0;
        std::uint32_t storageImages  = 0;
    };
    [[nodiscard]] const Capacity& capacity() const noexcept { return capacity_; }

    struct Usage {
        std::uint32_t sampledImages  = 0;
        std::uint32_t storageBuffers = 0;
        std::uint32_t samplers       = 0;
        std::uint32_t storageImages  = 0;
    };
    [[nodiscard]] Usage usage() const noexcept;

private:
    // A simple index freelist per array. Indices are stable and reused.
    struct IndexAllocator {
        std::uint32_t              capacity = 0;
        std::uint32_t              next     = 0;
        std::vector<std::uint32_t> freed;

        [[nodiscard]] std::uint32_t allocate();
        void                        release(std::uint32_t index);
        [[nodiscard]] std::uint32_t used() const noexcept;
    };

    VkDevice              device_ = VK_NULL_HANDLE;
    VkDescriptorPool      pool_   = VK_NULL_HANDLE;
    VkDescriptorSetLayout layout_ = VK_NULL_HANDLE;
    VkDescriptorSet       set_    = VK_NULL_HANDLE;

    Capacity capacity_;

    IndexAllocator sampledImages_;
    IndexAllocator storageBuffers_;
    IndexAllocator samplers_;
    IndexAllocator storageImages_;
};

} // namespace harpia::rhi
