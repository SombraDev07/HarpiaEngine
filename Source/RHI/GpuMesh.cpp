#include "RHI/GpuMesh.h"

#include "RHI/Vulkan/VulkanDevice.h"

#include <cstdio>

namespace harpia::rhi {

GpuMesh::~GpuMesh()
{
    destroy();
}

bool GpuMesh::create(VulkanDevice&    device,
                     GpuUploader&     uploader,
                     VulkanBindless&  bindless,
                     const MeshAsset& mesh,
                     const char*      debugName)
{
    if (mesh.vertices.empty() || mesh.indices.empty()) {
        return false;
    }

    bindless_    = &bindless;
    vertexCount_ = static_cast<std::uint32_t>(mesh.vertices.size());
    indexCount_  = static_cast<std::uint32_t>(mesh.indices.size());
    bounds_      = mesh.bounds;

    const VkDeviceSize vertexBytes = sizeof(MeshVertex) * mesh.vertices.size();
    const VkDeviceSize indexBytes  = sizeof(std::uint32_t) * mesh.indices.size();

    char name[96];

    BufferDesc vertexDesc;
    vertexDesc.size   = vertexBytes;
    // STORAGE_BUFFER, not VERTEX_BUFFER: the shader indexes this itself.
    vertexDesc.usage  = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT
                      | VK_BUFFER_USAGE_SHADER_DEVICE_ADDRESS_BIT;
    vertexDesc.memory = BufferMemory::DeviceLocal;
    std::snprintf(name, sizeof(name), "%s_Vertices", debugName);
    vertexDesc.debugName = name;

    if (!vertexBuffer_.create(device, vertexDesc)) {
        return false;
    }
    if (!uploader.upload(vertexBuffer_, mesh.vertices.data(), vertexBytes)) {
        destroy();
        return false;
    }

    BufferDesc indexDesc;
    indexDesc.size   = indexBytes;
    indexDesc.usage  = VK_BUFFER_USAGE_INDEX_BUFFER_BIT;
    indexDesc.memory = BufferMemory::DeviceLocal;
    std::snprintf(name, sizeof(name), "%s_Indices", debugName);
    indexDesc.debugName = name;

    if (!indexBuffer_.create(device, indexDesc)) {
        destroy();
        return false;
    }
    if (!uploader.upload(indexBuffer_, mesh.indices.data(), indexBytes)) {
        destroy();
        return false;
    }

    vertexIndex_ = bindless.registerStorageBuffer(vertexBuffer_.handle(), 0, vertexBytes);
    if (vertexIndex_ == VulkanBindless::kInvalidIndex) {
        std::fprintf(stderr, "[mesh] bindless storage buffer slots exhausted\n");
        destroy();
        return false;
    }

    subMeshes_.reserve(mesh.subMeshes.size());
    for (const SubMesh& source : mesh.subMeshes) {
        GpuSubMesh target;
        target.firstIndex   = source.firstIndex;
        target.indexCount   = source.indexCount;
        target.vertexOffset = source.vertexOffset;
        target.material     = source.material;
        subMeshes_.push_back(target);
    }

    return true;
}

void GpuMesh::destroy()
{
    if (bindless_ != nullptr && vertexIndex_ != VulkanBindless::kInvalidIndex) {
        bindless_->releaseStorageBuffer(vertexIndex_);
    }
    vertexIndex_ = VulkanBindless::kInvalidIndex;
    bindless_    = nullptr;

    vertexBuffer_.destroy();
    indexBuffer_.destroy();
    subMeshes_.clear();
    vertexCount_ = 0;
    indexCount_  = 0;
}

void GpuMesh::bindIndices(VkCommandBuffer cmd) const
{
    vkCmdBindIndexBuffer(cmd, indexBuffer_.handle(), 0, VK_INDEX_TYPE_UINT32);
}

void GpuMesh::drawSubMesh(VkCommandBuffer cmd, std::size_t index) const
{
    if (index >= subMeshes_.size()) {
        return;
    }
    const GpuSubMesh& subMesh = subMeshes_[index];

    // vertexOffset is passed to the GPU rather than baked into the indices,
    // which is why the importer keeps indices primitive-local.
    vkCmdDrawIndexed(cmd, subMesh.indexCount, 1, subMesh.firstIndex,
                     static_cast<std::int32_t>(subMesh.vertexOffset), 0);
}

MeshDrawConstants GpuMesh::drawConstants(std::size_t subMesh) const
{
    MeshDrawConstants constants;
    constants.vertexBufferIndex = vertexIndex_;
    if (subMesh < subMeshes_.size()) {
        constants.vertexOffset  = subMeshes_[subMesh].vertexOffset;
        constants.materialIndex = subMeshes_[subMesh].material >= 0
                                ? static_cast<std::uint32_t>(subMeshes_[subMesh].material)
                                : 0u;
    }
    return constants;
}

} // namespace harpia::rhi
