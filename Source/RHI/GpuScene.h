// Harpia Engine — an imported mesh made resident, materials and all
//
// Every piece of this path already existed and none of them had ever met: the
// glTF importer resolves texture references to GUIDs, the asset manager turns a
// GUID into pixels, GpuTexture makes pixels resident, and the GBuffer shader
// reads a material by bindless index. What was missing is the thing that walks
// a material list and connects those four.
//
// That gap is where the surprises live. A mesh whose material has no normal map
// still needs a normal slot the shader can read; a texture used by three
// materials must upload once; base colour is sRGB while roughness beside it is
// linear, and the file cannot tell you which. None of those show up when the
// pieces are tested separately, which is exactly why they are worth a type.
#pragma once

#include "Core/Assets/AssetId.h"
#include "Core/Assets/MeshAsset.h"
#include "RHI/GpuMesh.h"
#include "RHI/GpuTexture.h"
#include "RHI/Vulkan/VulkanBuffer.h"

#include <memory>
#include <unordered_map>
#include <vector>

namespace harpia {
class AssetManager;
}

namespace harpia::rhi {

class VulkanDevice;

class GpuScene {
public:
    GpuScene() = default;
    ~GpuScene();

    GpuScene(const GpuScene&)            = delete;
    GpuScene& operator=(const GpuScene&) = delete;

    // `manager` resolves the texture GUIDs the importer recorded. Passing none
    // is legal and means every material falls back to its factors, which is
    // what an untextured mesh already did before this type existed.
    [[nodiscard]] bool create(VulkanDevice&    device,
                              GpuUploader&     uploader,
                              VulkanBindless&  bindless,
                              const MeshAsset& asset,
                              AssetManager*    manager   = nullptr,
                              const char*      debugName = "Scene");
    void destroy();

    [[nodiscard]] const GpuMesh& mesh() const noexcept { return mesh_; }
    [[nodiscard]] std::uint32_t materialBufferIndex() const noexcept { return materialIndex_; }
    [[nodiscard]] std::size_t   materialCount() const noexcept { return materialCount_; }

    // How many distinct images were uploaded. Lower than the number of material
    // slots whenever a texture is shared, which is the point of the cache.
    [[nodiscard]] std::size_t textureCount() const noexcept { return textures_.size(); }

    [[nodiscard]] bool valid() const noexcept { return mesh_.valid(); }

private:
    // Resolves one texture slot, uploading it if this is the first material to
    // ask for it. Returns kInvalidTextureIndex when there is nothing to load —
    // the shader then uses the factor alone.
    [[nodiscard]] std::uint32_t resolveTexture(VulkanDevice&     device,
                                               GpuUploader&      uploader,
                                               VulkanBindless&   bindless,
                                               AssetManager*     manager,
                                               const AssetId&    id,
                                               TextureColorSpace colorSpace);

    GpuMesh      mesh_;
    VulkanBuffer materialBuffer_;

    std::vector<std::unique_ptr<GpuTexture>> textures_;

    // Keyed by GUID so a texture shared between materials uploads once. The
    // colour space is part of the key: the same PNG can legitimately be a base
    // colour in one material and a roughness map in another, and those are two
    // different GPU images.
    std::unordered_map<std::string, std::uint32_t> textureCache_;

    std::uint32_t materialIndex_ = 0xFFFFFFFFu;
    std::size_t   materialCount_ = 0;
};

} // namespace harpia::rhi
