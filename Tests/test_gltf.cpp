// glTF import. The file under test is generated here rather than checked in:
// a binary fixture nobody can read is a fixture nobody maintains.

#include <doctest/doctest.h>

#include "Core/Assets/AssetDatabase.h"
#include "Core/Assets/AssetManager.h"
#include "Core/Assets/GltfLoader.h"

#include <array>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

using namespace harpia;
namespace fs = std::filesystem;

namespace {

std::string base64(const std::vector<std::uint8_t>& data)
{
    static constexpr char kAlphabet[] =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    std::string out;
    out.reserve(((data.size() + 2) / 3) * 4);

    for (std::size_t i = 0; i < data.size(); i += 3) {
        const std::uint32_t byte0 = data[i];
        const std::uint32_t byte1 = (i + 1 < data.size()) ? data[i + 1] : 0u;
        const std::uint32_t byte2 = (i + 2 < data.size()) ? data[i + 2] : 0u;
        const std::uint32_t triple = (byte0 << 16) | (byte1 << 8) | byte2;

        out += kAlphabet[(triple >> 18) & 0x3F];
        out += kAlphabet[(triple >> 12) & 0x3F];
        out += (i + 1 < data.size()) ? kAlphabet[(triple >> 6) & 0x3F] : '=';
        out += (i + 2 < data.size()) ? kAlphabet[triple & 0x3F] : '=';
    }
    return out;
}

template <typename T>
void append(std::vector<std::uint8_t>& buffer, const T& value)
{
    const auto* bytes = reinterpret_cast<const std::uint8_t*>(&value);
    buffer.insert(buffer.end(), bytes, bytes + sizeof(T));
}

// One triangle: positions, normals, UVs and 16-bit indices, in that order.
struct TriangleBuffer {
    std::vector<std::uint8_t> bytes;
    std::size_t positionsOffset = 0;
    std::size_t normalsOffset   = 0;
    std::size_t uvsOffset       = 0;
    std::size_t indicesOffset   = 0;
};

TriangleBuffer makeTriangleBuffer()
{
    TriangleBuffer buffer;

    buffer.positionsOffset = buffer.bytes.size();
    const std::array<std::array<float, 3>, 3> positions{{
        {{0.0f, 0.0f, 0.0f}}, {{1.0f, 0.0f, 0.0f}}, {{0.0f, 2.0f, 0.0f}}}};
    for (const auto& p : positions) {
        append(buffer.bytes, p[0]);
        append(buffer.bytes, p[1]);
        append(buffer.bytes, p[2]);
    }

    buffer.normalsOffset = buffer.bytes.size();
    for (int i = 0; i < 3; ++i) {
        append(buffer.bytes, 0.0f);
        append(buffer.bytes, 0.0f);
        append(buffer.bytes, 1.0f);
    }

    buffer.uvsOffset = buffer.bytes.size();
    const std::array<std::array<float, 2>, 3> uvs{{
        {{0.0f, 0.0f}}, {{1.0f, 0.0f}}, {{0.0f, 1.0f}}}};
    for (const auto& uv : uvs) {
        append(buffer.bytes, uv[0]);
        append(buffer.bytes, uv[1]);
    }

    buffer.indicesOffset = buffer.bytes.size();
    for (std::uint16_t index : {std::uint16_t{0}, std::uint16_t{1}, std::uint16_t{2}}) {
        append(buffer.bytes, index);
    }

    return buffer;
}

// `nodesJson` lets a test decide the scene graph while reusing the geometry.
std::string makeGltf(const std::string& nodesJson, const std::string& sceneNodes)
{
    const TriangleBuffer buffer = makeTriangleBuffer();

    return std::string(R"({
  "asset": {"version": "2.0"},
  "scene": 0,
  "scenes": [{"nodes": [)") + sceneNodes + R"(]}],
  "nodes": [)" + nodesJson + R"(],
  "meshes": [{
    "primitives": [{
      "attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2},
      "indices": 3,
      "material": 0
    }]
  }],
  "materials": [{
    "name": "TestMaterial",
    "pbrMetallicRoughness": {
      "baseColorFactor": [0.25, 0.5, 0.75, 1.0],
      "metallicFactor": 0.125,
      "roughnessFactor": 0.875
    },
    "emissiveFactor": [0.1, 0.2, 0.3]
  }],
  "buffers": [{"byteLength": )" + std::to_string(buffer.bytes.size()) +
        R"(, "uri": "data:application/octet-stream;base64,)" + base64(buffer.bytes) + R"("}],
  "bufferViews": [
    {"buffer": 0, "byteOffset": )" + std::to_string(buffer.positionsOffset) +
        R"(, "byteLength": 36},
    {"buffer": 0, "byteOffset": )" + std::to_string(buffer.normalsOffset) +
        R"(, "byteLength": 36},
    {"buffer": 0, "byteOffset": )" + std::to_string(buffer.uvsOffset) +
        R"(, "byteLength": 24},
    {"buffer": 0, "byteOffset": )" + std::to_string(buffer.indicesOffset) +
        R"(, "byteLength": 6}
  ],
  "accessors": [
    {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
     "min": [0.0, 0.0, 0.0], "max": [1.0, 2.0, 0.0]},
    {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"},
    {"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC2"},
    {"bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR"}
  ]
})";
}

struct TempGltf {
    fs::path root;
    fs::path file;

    explicit TempGltf(const std::string& contents,
                      const std::string& name = "triangle.gltf")
    {
        root = fs::temp_directory_path() / ("harpia_gltf_" + AssetId::generate().toString());
        fs::create_directories(root);
        file = root / name;
        std::ofstream out(file, std::ios::trunc);
        out << contents;
    }

    ~TempGltf()
    {
        std::error_code error;
        fs::remove_all(root, error);
    }
};

} // namespace

TEST_CASE("a single triangle imports with its attributes intact")
{
    const TempGltf gltf(makeGltf(R"({"mesh": 0})", "0"));

    const GltfImportResult result = importGltf(gltf.file);
    REQUIRE_MESSAGE(result, result.error);

    const MeshAsset& mesh = *result.mesh;
    CHECK(mesh.vertices.size() == 3);
    CHECK(mesh.indices.size() == 3);
    CHECK(mesh.triangleCount() == 1);
    CHECK(mesh.subMeshes.size() == 1);

    CHECK(mesh.vertices[1].position.x == doctest::Approx(1.0f));
    CHECK(mesh.vertices[2].position.y == doctest::Approx(2.0f));
    CHECK(mesh.vertices[0].normal.z == doctest::Approx(1.0f));
    CHECK(mesh.vertices[1].uv.x == doctest::Approx(1.0f));
    CHECK(mesh.vertices[2].uv.y == doctest::Approx(1.0f));

    CHECK(mesh.indices[0] == 0);
    CHECK(mesh.indices[2] == 2);
}

TEST_CASE("material factors survive the import")
{
    const TempGltf gltf(makeGltf(R"({"mesh": 0})", "0"));

    const GltfImportResult result = importGltf(gltf.file);
    REQUIRE_MESSAGE(result, result.error);

    REQUIRE(result.mesh->materials.size() == 1);
    const MeshMaterial& material = result.mesh->materials[0];

    CHECK(material.name == "TestMaterial");
    CHECK(material.baseColorFactor.x == doctest::Approx(0.25f));
    CHECK(material.baseColorFactor.z == doctest::Approx(0.75f));
    CHECK(material.metallicFactor == doctest::Approx(0.125f));
    CHECK(material.roughnessFactor == doctest::Approx(0.875f));
    CHECK(material.emissiveFactor.y == doctest::Approx(0.2f));

    CHECK(result.mesh->subMeshes[0].material == 0);

    // No texture was referenced, so the GUID stays invalid rather than
    // becoming a path we would have to fix up later.
    CHECK_FALSE(material.baseColorTexture.valid());
}

TEST_CASE("node transforms are baked into world space")
{
    const TempGltf gltf(makeGltf(
        R"({"mesh": 0, "translation": [10.0, 20.0, 30.0]})", "0"));

    const GltfImportResult result = importGltf(gltf.file);
    REQUIRE_MESSAGE(result, result.error);

    // Vertex 0 sits at the origin in mesh space, so it lands on the translation.
    CHECK(result.mesh->vertices[0].position.x == doctest::Approx(10.0f));
    CHECK(result.mesh->vertices[0].position.y == doctest::Approx(20.0f));
    CHECK(result.mesh->vertices[0].position.z == doctest::Approx(30.0f));

    // Translation must not move a direction.
    CHECK(result.mesh->vertices[0].normal.z == doctest::Approx(1.0f));
}

TEST_CASE("a nested child inherits its parent transform")
{
    const TempGltf gltf(makeGltf(
        R"({"children": [1], "translation": [100.0, 0.0, 0.0]},
            {"mesh": 0, "translation": [5.0, 0.0, 0.0]})", "0"));

    const GltfImportResult result = importGltf(gltf.file);
    REQUIRE_MESSAGE(result, result.error);

    CHECK(result.mesh->vertices[0].position.x == doctest::Approx(105.0f));
}

TEST_CASE("two nodes on one mesh become two sub-meshes")
{
    const TempGltf gltf(makeGltf(
        R"({"mesh": 0, "translation": [0.0, 0.0, 0.0]},
            {"mesh": 0, "translation": [50.0, 0.0, 0.0]})", "0, 1"));

    const GltfImportResult result = importGltf(gltf.file);
    REQUIRE_MESSAGE(result, result.error);

    const MeshAsset& mesh = *result.mesh;
    CHECK(mesh.subMeshes.size() == 2);
    CHECK(mesh.vertices.size() == 6);
    CHECK(mesh.indices.size() == 6);

    // The second sub-mesh's indices are offset into the shared vertex array.
    CHECK(mesh.subMeshes[0].vertexOffset == 0);
    CHECK(mesh.subMeshes[1].vertexOffset == 3);
    CHECK(mesh.subMeshes[1].firstIndex == 3);

    CHECK(mesh.vertices[3].position.x == doctest::Approx(50.0f));
}

TEST_CASE("bounds cover the whole mesh and each sub-mesh")
{
    const TempGltf gltf(makeGltf(
        R"({"mesh": 0}, {"mesh": 0, "translation": [50.0, 0.0, 0.0]})", "0, 1"));

    const GltfImportResult result = importGltf(gltf.file);
    REQUIRE_MESSAGE(result, result.error);

    const MeshAsset& mesh = *result.mesh;
    REQUIRE(mesh.bounds.valid());

    CHECK(mesh.bounds.min.x == doctest::Approx(0.0f));
    CHECK(mesh.bounds.max.x == doctest::Approx(51.0f));
    CHECK(mesh.bounds.max.y == doctest::Approx(2.0f));

    REQUIRE(mesh.subMeshes.size() == 2);
    CHECK(mesh.subMeshes[0].bounds.max.x == doctest::Approx(1.0f));
    CHECK(mesh.subMeshes[1].bounds.min.x == doctest::Approx(50.0f));
}

TEST_CASE("bad input is reported, not crashed on")
{
    SUBCASE("not glTF at all")
    {
        const TempGltf gltf("this is not json");
        const GltfImportResult result = importGltf(gltf.file);
        CHECK_FALSE(result);
        CHECK_FALSE(result.error.empty());
    }

    SUBCASE("valid glTF with no geometry")
    {
        const TempGltf gltf(R"({"asset": {"version": "2.0"}})");
        const GltfImportResult result = importGltf(gltf.file);
        CHECK_FALSE(result);
    }

    SUBCASE("missing file")
    {
        const GltfImportResult result = importGltf("/nonexistent/path/model.gltf");
        CHECK_FALSE(result);
    }
}

TEST_CASE("a mesh loads through the asset manager by GUID")
{
    const TempGltf gltf(makeGltf(R"({"mesh": 0})", "0"), "model.gltf");

    AssetDatabase database;
    REQUIRE(database.open(gltf.root));
    database.scan();

    AssetManager manager;
    manager.attach(&database);
    registerGltfLoader(manager, &database);

    const AssetId id = database.idOf(gltf.file);
    REQUIRE(id.valid());

    const std::shared_ptr<MeshAsset> mesh = manager.load<MeshAsset>(id);
    REQUIRE(mesh != nullptr);
    CHECK(mesh->triangleCount() == 1);
    CHECK(mesh->id() == id);
    CHECK(mesh->type() == AssetType::Mesh);

    // Second load comes from the cache, not from cgltf.
    CHECK(manager.load<MeshAsset>(id) == mesh);
    CHECK(manager.stats().loads == 1);
    CHECK(manager.stats().cacheHits == 1);
}

TEST_CASE("a texture URI resolves to the GUID of the imported file")
{
    TempGltf gltf(R"({
  "asset": {"version": "2.0"},
  "scene": 0,
  "scenes": [{"nodes": [0]}],
  "nodes": [{"mesh": 0}],
  "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "material": 0}]}],
  "materials": [{
    "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}
  }],
  "textures": [{"source": 0}],
  "images": [{"uri": "albedo.png"}],
  "buffers": [{"byteLength": 36, "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAAEAAAAAA"}],
  "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}],
  "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                 "min": [0,0,0], "max": [1,2,0]}]
})", "model.gltf");

    // The texture has to exist in the database for the URI to resolve.
    std::ofstream(gltf.root / "albedo.png") << "fake png";

    AssetDatabase database;
    REQUIRE(database.open(gltf.root));
    database.scan();

    const AssetId textureId = database.idOf(gltf.root / "albedo.png");
    REQUIRE(textureId.valid());

    const GltfImportResult result = importGltf(gltf.file, &database);
    REQUIRE_MESSAGE(result, result.error);
    REQUIRE(result.mesh->materials.size() == 1);

    // This is the payoff: the material references the texture by GUID, so
    // renaming albedo.png later does not break the material.
    CHECK(result.mesh->materials[0].baseColorTexture == textureId);
}
