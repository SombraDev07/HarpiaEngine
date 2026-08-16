// The path from an imported file to a drawable scene. Every piece of it was
// already tested alone — the glTF importer, the asset database, texture upload,
// the bindless heap — and none of them had ever met. This is the seam.

#include <doctest/doctest.h>

#include "Core/Assets/AssetDatabase.h"
#include "Core/Assets/AssetManager.h"
#include "Core/Assets/GltfLoader.h"
#include "Core/Assets/TextureLoader.h"
#include "RHI/GpuScene.h"
#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanRenderer.h"

#include <stb_image_write.h>

#include <cstdint>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

using namespace harpia;
namespace fs = std::filesystem;

namespace {

std::string base64(const std::vector<std::uint8_t>& bytes)
{
    static constexpr char kTable[] =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    std::string out;
    for (std::size_t i = 0; i < bytes.size(); i += 3) {
        const std::uint32_t a = bytes[i];
        const std::uint32_t b = i + 1 < bytes.size() ? bytes[i + 1] : 0u;
        const std::uint32_t c = i + 2 < bytes.size() ? bytes[i + 2] : 0u;
        const std::uint32_t triple = (a << 16) | (b << 8) | c;

        out += kTable[(triple >> 18) & 0x3F];
        out += kTable[(triple >> 12) & 0x3F];
        out += i + 1 < bytes.size() ? kTable[(triple >> 6) & 0x3F] : '=';
        out += i + 2 < bytes.size() ? kTable[triple & 0x3F] : '=';
    }
    return out;
}

void appendFloats(std::vector<std::uint8_t>& out, const std::vector<float>& values)
{
    const auto* bytes = reinterpret_cast<const std::uint8_t*>(values.data());
    out.insert(out.end(), bytes, bytes + values.size() * sizeof(float));
}

// One triangle with positions, normals and UVs, plus 16-bit indices.
std::string makeTexturedGltf()
{
    std::vector<std::uint8_t> buffer;
    appendFloats(buffer, {0, 0, 0, 1, 0, 0, 0, 1, 0});          // positions
    appendFloats(buffer, {0, 0, 1, 0, 0, 1, 0, 0, 1});          // normals
    appendFloats(buffer, {0, 0, 1, 0, 0, 1});                   // uvs
    const std::vector<std::uint16_t> indices{0, 1, 2};
    const auto* indexBytes = reinterpret_cast<const std::uint8_t*>(indices.data());
    buffer.insert(buffer.end(), indexBytes, indexBytes + indices.size() * sizeof(std::uint16_t));

    return std::string(R"({
  "asset": {"version": "2.0"},
  "scene": 0,
  "scenes": [{"nodes": [0]}],
  "nodes": [{"mesh": 0}],
  "meshes": [{"primitives": [{
    "attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2},
    "indices": 3, "material": 0}]}],
  "images": [{"uri": "surface.png"}],
  "textures": [{"source": 0}],
  "materials": [{
    "name": "Textured",
    "pbrMetallicRoughness": {
      "baseColorTexture": {"index": 0},
      "metallicRoughnessTexture": {"index": 0}
    }
  }],
  "buffers": [{"byteLength": )") + std::to_string(buffer.size()) +
        R"(, "uri": "data:application/octet-stream;base64,)" + base64(buffer) + R"("}],
  "bufferViews": [
    {"buffer": 0, "byteOffset": 0,  "byteLength": 36},
    {"buffer": 0, "byteOffset": 36, "byteLength": 36},
    {"buffer": 0, "byteOffset": 72, "byteLength": 24},
    {"buffer": 0, "byteOffset": 96, "byteLength": 6}
  ],
  "accessors": [
    {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
     "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
    {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"},
    {"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC2"},
    {"bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR"}
  ]
})";
}

struct TempScene {
    fs::path root;
    fs::path gltf;

    TempScene()
    {
        root = fs::temp_directory_path() / ("harpia_scene_" + AssetId::generate().toString());
        fs::create_directories(root);

        std::vector<std::uint8_t> pixels(4 * 4 * 4, 0);
        for (std::size_t i = 0; i < pixels.size(); i += 4) {
            pixels[i + 0] = 200;
            pixels[i + 1] = 120;
            pixels[i + 2] = 60;
            pixels[i + 3] = 255;
        }
        const fs::path png = root / "surface.png";
        REQUIRE(stbi_write_png(png.string().c_str(), 4, 4, 4, pixels.data(), 4 * 4) != 0);

        gltf = root / "mesh.gltf";
        std::ofstream out(gltf, std::ios::trunc);
        out << makeTexturedGltf();
    }

    ~TempScene()
    {
        std::error_code error;
        fs::remove_all(root, error);
    }

    TempScene(const TempScene&)            = delete;
    TempScene& operator=(const TempScene&) = delete;
};

} // namespace

TEST_SUITE_BEGIN("gpu");

TEST_CASE("an imported glTF becomes a drawable scene with its textures resident")
{
    rhi::VulkanDevice::resetValidationErrorCount();

    rhi::DeviceDesc desc;
    desc.enableValidation = true;
    desc.window           = nullptr;

    rhi::VulkanDevice device;
    if (!device.create(desc)) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    rhi::VulkanRenderer renderer;
    rhi::GpuUploader    uploader;
    REQUIRE(renderer.createOffscreen(device, 16, 16));
    REQUIRE(uploader.create(device));

    const TempScene scene;

    AssetDatabase database;
    REQUIRE(database.open(scene.root));
    database.scan();

    AssetManager manager;
    manager.attach(&database);
    registerTextureLoader(manager);

    // The importer needs the database to turn "surface.png" into a GUID. Without
    // it the reference stays invalid, which is the untextured path.
    const GltfImportResult imported = importGltf(scene.gltf, &database);
    REQUIRE_MESSAGE(imported, imported.error);
    REQUIRE(imported.mesh != nullptr);

    SUBCASE("the importer resolved the texture reference to a GUID")
    {
        REQUIRE(imported.mesh->materials.size() == 1);
        CHECK(imported.mesh->materials[0].baseColorTexture.valid());
        CHECK(imported.mesh->materials[0].metallicRoughnessTexture.valid());
    }

    rhi::GpuScene gpu;
    REQUIRE(gpu.create(device, uploader, renderer.bindless(), *imported.mesh,
                       &manager, "TestScene"));

    SUBCASE("the scene is drawable and its materials are reachable by index")
    {
        CHECK(gpu.valid());
        CHECK(gpu.mesh().vertexCount() == 3);
        CHECK(gpu.mesh().indexCount() == 3);
        CHECK(gpu.materialCount() == 1);
        CHECK(gpu.materialBufferIndex() != rhi::VulkanBindless::kInvalidIndex);
        CHECK(gpu.mesh().vertexBufferIndex() != rhi::VulkanBindless::kInvalidIndex);
    }

    SUBCASE("one file used as colour and as data becomes two GPU images")
    {
        // The same PNG fills both slots. They cannot share an image: base colour
        // carries the sRGB curve and roughness does not, and sharing would apply
        // that curve to numbers which are not colour. The failure is invisible
        // in a screenshot and consistently wrong in the lighting, which is why
        // the count is asserted rather than eyeballed.
        CHECK(gpu.textureCount() == 2);
    }

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);

    gpu.destroy();
    uploader.destroy();
    renderer.destroy();
    device.destroy();
}

TEST_CASE("a material naming a texture nobody imported still renders")
{
    rhi::VulkanDevice::resetValidationErrorCount();

    rhi::DeviceDesc desc;
    desc.enableValidation = true;
    desc.window           = nullptr;

    rhi::VulkanDevice device;
    if (!device.create(desc)) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    rhi::VulkanRenderer renderer;
    rhi::GpuUploader    uploader;
    REQUIRE(renderer.createOffscreen(device, 16, 16));
    REQUIRE(uploader.create(device));

    const TempScene scene;

    // No database this time, so every texture reference stays unresolved. A
    // broken reference is a fact of real content, not a reason to refuse the
    // mesh: the factors still describe a surface.
    const GltfImportResult imported = importGltf(scene.gltf);
    REQUIRE_MESSAGE(imported, imported.error);

    rhi::GpuScene gpu;
    REQUIRE(gpu.create(device, uploader, renderer.bindless(), *imported.mesh,
                       nullptr, "UntexturedScene"));

    CHECK(gpu.valid());
    CHECK(gpu.textureCount() == 0);
    CHECK(gpu.materialBufferIndex() != rhi::VulkanBindless::kInvalidIndex);

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);

    gpu.destroy();
    uploader.destroy();
    renderer.destroy();
    device.destroy();
}

TEST_SUITE_END();
