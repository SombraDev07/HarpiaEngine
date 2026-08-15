// Harpia Engine — mesh on the GPU
//
// Vertices live in a storage buffer reached by bindless index, not in a bound
// vertex buffer. The vertex shader reads them with SV_VertexID, which is what
// makes a single pipeline serve every mesh and is the layout GPU-driven
// submission needs in F7.
//
// Indices stay in a real index buffer: index fetch is still fixed function and
// still the fastest path.
#pragma once

#include "Core/Assets/MeshAsset.h"
#include "RHI/Vulkan/VulkanBindless.h"
#include "RHI/Vulkan/VulkanBuffer.h"

#include <cstdint>
#include <vector>

namespace harpia::rhi {

class VulkanDevice;

struct GpuSubMesh {
    std::uint32_t firstIndex   = 0;
    std::uint32_t indexCount   = 0;
    std::uint32_t vertexOffset = 0;
    std::int32_t  material     = -1;
};

// What a draw needs to know, small enough to travel in push constants.
struct MeshDrawConstants {
    std::uint32_t vertexBufferIndex = 0; // bindless slot
    std::uint32_t vertexOffset      = 0;
    std::uint32_t materialIndex     = 0;
    std::uint32_t padding           = 0;
};

class GpuMesh {
public:
    GpuMesh() = default;
    ~GpuMesh();

    GpuMesh(const GpuMesh&)            = delete;
    GpuMesh& operator=(const GpuMesh&) = delete;

    [[nodiscard]] bool create(VulkanDevice&   device,
                              GpuUploader&    uploader,
                              VulkanBindless& bindless,
                              const MeshAsset& mesh,
                              const char*     debugName = "Mesh");
    void destroy();

    void bindIndices(VkCommandBuffer cmd) const;

    // Records one vkCmdDrawIndexed per sub-mesh. Push constants are the
    // caller's business, so this stays usable from any pass.
    void drawSubMesh(VkCommandBuffer cmd, std::size_t index) const;

    [[nodiscard]] std::uint32_t vertexBufferIndex() const noexcept { return vertexIndex_; }
    [[nodiscard]] const std::vector<GpuSubMesh>& subMeshes() const noexcept { return subMeshes_; }
    [[nodiscard]] std::uint32_t vertexCount() const noexcept { return vertexCount_; }
    [[nodiscard]] std::uint32_t indexCount() const noexcept  { return indexCount_; }
    [[nodiscard]] const Bounds& bounds() const noexcept      { return bounds_; }
    [[nodiscard]] bool valid() const noexcept { return vertexBuffer_.valid(); }

    [[nodiscard]] MeshDrawConstants drawConstants(std::size_t subMesh) const;

private:
    VulkanBindless* bindless_ = nullptr;

    VulkanBuffer vertexBuffer_;
    VulkanBuffer indexBuffer_;

    std::uint32_t vertexIndex_ = VulkanBindless::kInvalidIndex;
    std::uint32_t vertexCount_ = 0;
    std::uint32_t indexCount_  = 0;

    std::vector<GpuSubMesh> subMeshes_;
    Bounds                  bounds_;
};

} // namespace harpia::rhi
