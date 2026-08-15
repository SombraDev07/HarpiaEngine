// Harpia Engine — glTF 2.0 import
//
// Roadmap F2 reference: Dagor reads glTF through cgltf too. Copying their
// dependency choice is the cheap part of learning from them.
//
// The importer flattens the node hierarchy into world-space vertices. A scene
// graph belongs to the ECS, not to a mesh asset — keeping transforms out of
// here is what stops a mesh from carrying a second, competing hierarchy.
#pragma once

#include "Core/Assets/AssetManager.h"
#include "Core/Assets/MeshAsset.h"

#include <filesystem>
#include <memory>
#include <string>

namespace harpia {

class AssetDatabase;

struct GltfImportResult {
    std::shared_ptr<MeshAsset> mesh;
    std::string                error;

    [[nodiscard]] explicit operator bool() const noexcept { return mesh != nullptr; }
};

// `database`, when given, is used to resolve texture URIs to GUIDs.
[[nodiscard]] GltfImportResult importGltf(const std::filesystem::path& path,
                                          const AssetDatabase*         database = nullptr);

// Registers importGltf as the loader for AssetType::Mesh.
void registerGltfLoader(AssetManager& manager, const AssetDatabase* database = nullptr);

} // namespace harpia
