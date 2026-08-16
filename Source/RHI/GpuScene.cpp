#include "RHI/GpuScene.h"

#include "Core/Assets/AssetManager.h"
#include "Core/Assets/TextureAsset.h"
#include "RHI/RenderTypes.h"
#include "RHI/Vulkan/VulkanDevice.h"

#include <cstdio>

namespace harpia::rhi {

GpuScene::~GpuScene()
{
    destroy();
}

std::uint32_t GpuScene::resolveTexture(VulkanDevice&     device,
                                       GpuUploader&      uploader,
                                       VulkanBindless&   bindless,
                                       AssetManager*     manager,
                                       const AssetId&    id,
                                       TextureColorSpace colorSpace)
{
    if (manager == nullptr || !id.valid()) {
        return kInvalidTextureIndex;
    }

    // Colour space belongs in the key. The same file can be a base colour in
    // one material and a data map in another, and those are two GPU images —
    // sharing them would apply the sRGB curve to numbers that are not colour.
    const std::string key = id.toString()
                          + (colorSpace == TextureColorSpace::Srgb ? "#srgb" : "#linear");

    if (const auto found = textureCache_.find(key); found != textureCache_.end()) {
        return found->second;
    }

    const std::shared_ptr<TextureAsset> pixels = manager->load<TextureAsset>(id);
    if (pixels == nullptr || pixels->empty()) {
        // A material naming a texture nobody imported is a broken reference,
        // not a reason to fail the whole mesh: the factor still renders.
        std::fprintf(stderr, "[scene] texture %s could not be loaded; using the factor\n",
                     id.toString().c_str());
        return kInvalidTextureIndex;
    }

    auto texture = std::make_unique<GpuTexture>();
    if (!texture->create(device, uploader, bindless, *pixels, colorSpace, true, "SceneTexture")) {
        return kInvalidTextureIndex;
    }

    const std::uint32_t index = texture->bindlessIndex();
    textures_.push_back(std::move(texture));
    textureCache_.emplace(key, index);
    return index;
}

bool GpuScene::create(VulkanDevice&    device,
                      GpuUploader&     uploader,
                      VulkanBindless&  bindless,
                      const MeshAsset& asset,
                      AssetManager*    manager,
                      const char*      debugName)
{
    if (!mesh_.create(device, uploader, bindless, asset, debugName)) {
        return false;
    }

    // A mesh with no material list still needs one entry, or the GBuffer would
    // index an empty buffer. Its defaults are a white dielectric.
    std::vector<GpuMaterialData> materials;
    materials.reserve(asset.materials.empty() ? 1 : asset.materials.size());

    for (const MeshMaterial& source : asset.materials) {
        GpuMaterialData material;
        material.baseColorFactor = source.baseColorFactor;
        material.emissiveFactor  = Vec4(source.emissiveFactor, 0.0f);
        material.metallicFactor  = source.metallicFactor;
        material.roughnessFactor = source.roughnessFactor;

        // Which slots are colour and which are data is the one thing the file
        // cannot tell us, and getting it wrong is invisible in a screenshot.
        material.baseColorTexture = resolveTexture(device, uploader, bindless, manager,
                                                   source.baseColorTexture,
                                                   TextureColorSpace::Srgb);
        material.emissiveTexture  = resolveTexture(device, uploader, bindless, manager,
                                                   source.emissiveTexture,
                                                   TextureColorSpace::Srgb);
        material.normalTexture    = resolveTexture(device, uploader, bindless, manager,
                                                   source.normalTexture,
                                                   TextureColorSpace::Linear);
        material.metallicRoughnessTexture =
            resolveTexture(device, uploader, bindless, manager,
                           source.metallicRoughnessTexture, TextureColorSpace::Linear);

        materials.push_back(material);
    }

    if (materials.empty()) {
        materials.emplace_back();
    }
    materialCount_ = materials.size();

    BufferDesc desc;
    desc.size      = sizeof(GpuMaterialData) * materials.size();
    desc.usage     = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;
    desc.memory    = BufferMemory::DeviceLocal;
    desc.debugName = "Scene_Materials";

    if (!materialBuffer_.create(device, desc)
        || !uploader.upload(materialBuffer_, materials.data(), desc.size)) {
        destroy();
        return false;
    }

    materialIndex_ = bindless.registerStorageBuffer(materialBuffer_.handle(), 0, desc.size);
    if (materialIndex_ == VulkanBindless::kInvalidIndex) {
        std::fprintf(stderr, "[scene] bindless storage buffer slots exhausted\n");
        destroy();
        return false;
    }
    return true;
}

void GpuScene::destroy()
{
    for (std::unique_ptr<GpuTexture>& texture : textures_) {
        if (texture != nullptr) {
            texture->destroy();
        }
    }
    textures_.clear();
    textureCache_.clear();

    materialBuffer_.destroy();
    mesh_.destroy();

    materialIndex_ = kInvalidTextureIndex;
    materialCount_ = 0;
}

} // namespace harpia::rhi
