// Harpia Engine — GPU buffers
//
// Buffers are allocated through VMA and, when they carry shader-visible data,
// registered in the global bindless heap so a shader reaches them by uint32
// index. Nothing binds a vertex buffer to a fixed-function slot: the pipeline
// has no vertex input state, and geometry is read from a storage buffer keyed
// off SV_VertexID.
#pragma once

#include "RHI/Vulkan/VulkanCommon.h"

#include <cstddef>
#include <cstdint>
#include <vector>

struct VmaAllocation_T;
using VmaAllocation = VmaAllocation_T*;

namespace harpia::rhi {

class VulkanDevice;

enum class BufferMemory : std::uint8_t {
    DeviceLocal,  // fastest for the GPU, needs a staging copy to fill
    HostVisible,  // mapped and written directly; for per-frame uniforms
};

struct BufferDesc {
    VkDeviceSize       size   = 0;
    VkBufferUsageFlags usage  = 0;
    BufferMemory       memory = BufferMemory::DeviceLocal;
    const char*        debugName = "Buffer";
};

class VulkanBuffer {
public:
    VulkanBuffer() = default;
    ~VulkanBuffer();

    VulkanBuffer(const VulkanBuffer&)            = delete;
    VulkanBuffer& operator=(const VulkanBuffer&) = delete;
    VulkanBuffer(VulkanBuffer&& other) noexcept;
    VulkanBuffer& operator=(VulkanBuffer&& other) noexcept;

    [[nodiscard]] bool create(VulkanDevice& device, const BufferDesc& desc);
    void destroy();

    [[nodiscard]] VkBuffer     handle() const noexcept { return buffer_; }
    [[nodiscard]] VkDeviceSize size() const noexcept   { return size_; }
    [[nodiscard]] bool         valid() const noexcept  { return buffer_ != VK_NULL_HANDLE; }

    // Non-null only for HostVisible buffers.
    [[nodiscard]] void* mapped() const noexcept { return mapped_; }

    // Requires VK_BUFFER_USAGE_SHADER_DEVICE_ADDRESS_BIT in the desc.
    [[nodiscard]] VkDeviceAddress deviceAddress() const;

private:
    VulkanDevice* device_     = nullptr;
    VkBuffer      buffer_     = VK_NULL_HANDLE;
    VmaAllocation allocation_ = nullptr;
    VkDeviceSize  size_       = 0;
    void*         mapped_     = nullptr;
};

// Staged upload into device-local memory.
//
// Synchronous and one submit per call. Mesh and texture uploads happen at load
// time, not per frame, so the simple version is the right one until streaming
// (roadmap stage 4) needs a ring buffer and a transfer-queue timeline.
class GpuUploader {
public:
    [[nodiscard]] bool create(VulkanDevice& device);
    void destroy();

    [[nodiscard]] bool upload(VulkanBuffer& destination,
                              const void*   data,
                              VkDeviceSize  size,
                              VkDeviceSize  offset = 0);

    // Reads device-local memory back to the host. Slow by construction — it
    // stalls the queue — and exists for tests, golden images and debugging,
    // never for a frame path.
    [[nodiscard]] bool download(const VulkanBuffer& source,
                                void*               destination,
                                VkDeviceSize        size,
                                VkDeviceSize        offset = 0);

    // Reads an image back as tightly packed texels. This is how a golden image
    // is produced for one GBuffer channel; like download(), it stalls the queue
    // and never belongs in a frame path.
    [[nodiscard]] bool downloadImage(VkImage                    image,
                                     VkImageLayout              currentLayout,
                                     VkExtent2D                 extent,
                                     std::uint32_t              texelBytes,
                                     std::vector<std::uint8_t>& outTexels,
                                     VkImageAspectFlags         aspect
                                         = VK_IMAGE_ASPECT_COLOR_BIT);

private:
    VulkanDevice* device_ = nullptr;
    VkCommandPool pool_   = VK_NULL_HANDLE;
};

} // namespace harpia::rhi
