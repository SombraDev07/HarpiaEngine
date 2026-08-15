// Harpia Engine — CPU-side mesh
//
// Deliberately Vulkan-free: Core never depends on RHI. The renderer uploads
// this into GPU buffers; the asset layer only ever holds the data.
//
// Vertex layout is fixed and interleaved for now. F2 needs positions, normals,
// tangents and one UV set for PBR; splitting it into streams is a change the
// GPU upload path makes, not one the importer cares about.
#pragma once

#include "Core/Assets/AssetManager.h"
#include "Core/Math/Math.h"

#include <cstdint>
#include <string>
#include <vector>

namespace harpia {

struct MeshVertex {
    Vec3 position;
    Vec3 normal;
    Vec4 tangent;   // w carries the bitangent sign
    Vec2 uv;
};

// One draw's worth of the mesh. glTF primitives map onto these one to one.
struct SubMesh {
    std::uint32_t firstIndex   = 0;
    std::uint32_t indexCount   = 0;
    std::uint32_t vertexOffset = 0;
    std::int32_t  material     = -1;  // index into MeshAsset::materials
    AABB          bounds;
};

struct MeshMaterial {
    std::string name;
    Vec4  baseColorFactor{1.0f, 1.0f, 1.0f, 1.0f};
    float metallicFactor  = 1.0f;
    float roughnessFactor = 1.0f;
    Vec3  emissiveFactor;

    // Resolved to GUIDs by the importer when the referenced files are in the
    // database; a texture nobody imported yet stays invalid rather than
    // becoming a path we would have to fix up later.
    AssetId baseColorTexture;
    AssetId normalTexture;
    AssetId metallicRoughnessTexture;
    AssetId emissiveTexture;
};

class MeshAsset final : public Asset {
public:
    std::vector<MeshVertex>   vertices;
    std::vector<std::uint32_t> indices;
    std::vector<SubMesh>      subMeshes;
    std::vector<MeshMaterial> materials;
    AABB                      bounds;

    [[nodiscard]] std::size_t triangleCount() const noexcept { return indices.size() / 3; }
    [[nodiscard]] bool        empty() const noexcept { return indices.empty(); }

    // Recomputes bounds from the vertex data, per sub-mesh and overall.
    void recomputeBounds() noexcept;
};

} // namespace harpia
