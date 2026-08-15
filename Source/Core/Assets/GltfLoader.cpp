#include "Core/Assets/GltfLoader.h"

#include "Core/Assets/AssetDatabase.h"

#include <cgltf.h>
#include <glm/gtc/type_ptr.hpp>

#include <cmath>
#include <cstring>

namespace harpia {
namespace {

namespace fs = std::filesystem;

// cgltf writes column-major floats, which is exactly glm::mat4's layout.
using Matrix4 = Mat4;

[[nodiscard]] Vec3 transformPosition(const Matrix4& matrix, const Vec3& point)
{
    return Vec3(matrix * Vec4(point, 1.0f));
}

// Directions ignore translation. Using normalMatrix here would be the strictly
// correct thing for non-uniform scale; glTF content is overwhelmingly uniform,
// and normalising after the rotation is right for that case.
[[nodiscard]] Vec3 transformNormal(const Matrix4& matrix, const Vec3& direction)
{
    const Vec3 rotated = Vec3(matrix * Vec4(direction, 0.0f));
    return glm::dot(rotated, rotated) > 1e-16f ? normalize(rotated) : Vec3(0.0f);
}

[[nodiscard]] AssetId resolveTexture(const cgltf_texture* texture,
                                     const fs::path&      baseDirectory,
                                     const AssetDatabase* database)
{
    if (texture == nullptr || texture->image == nullptr || database == nullptr) {
        return AssetId{};
    }
    const char* uri = texture->image->uri;
    if (uri == nullptr) {
        return AssetId{}; // embedded image; a texture importer will own this
    }
    return database->idOf(baseDirectory / uri);
}

void readMaterial(const cgltf_material* source,
                  const fs::path&       baseDirectory,
                  const AssetDatabase*  database,
                  MeshMaterial&         out)
{
    if (source == nullptr) {
        return;
    }
    out.name = source->name != nullptr ? source->name : "";

    if (source->has_pbr_metallic_roughness) {
        const cgltf_pbr_metallic_roughness& pbr = source->pbr_metallic_roughness;
        out.baseColorFactor = Vec4{pbr.base_color_factor[0], pbr.base_color_factor[1],
                                   pbr.base_color_factor[2], pbr.base_color_factor[3]};
        out.metallicFactor  = pbr.metallic_factor;
        out.roughnessFactor = pbr.roughness_factor;
        out.baseColorTexture =
            resolveTexture(pbr.base_color_texture.texture, baseDirectory, database);
        out.metallicRoughnessTexture =
            resolveTexture(pbr.metallic_roughness_texture.texture, baseDirectory, database);
    }

    out.emissiveFactor = Vec3{source->emissive_factor[0], source->emissive_factor[1],
                              source->emissive_factor[2]};
    out.normalTexture   = resolveTexture(source->normal_texture.texture, baseDirectory, database);
    out.emissiveTexture = resolveTexture(source->emissive_texture.texture, baseDirectory, database);
}

// Appends one primitive, transformed into world space.
void readPrimitive(const cgltf_primitive& primitive,
                   const Matrix4&         transform,
                   const cgltf_data&      data,
                   MeshAsset&             mesh)
{
    if (primitive.type != cgltf_primitive_type_triangles) {
        return; // lines and points are not geometry the renderer draws
    }

    const auto vertexOffset = static_cast<std::uint32_t>(mesh.vertices.size());
    const auto firstIndex   = static_cast<std::uint32_t>(mesh.indices.size());

    std::size_t vertexCount = 0;
    for (cgltf_size a = 0; a < primitive.attributes_count; ++a) {
        if (primitive.attributes[a].type == cgltf_attribute_type_position) {
            vertexCount = primitive.attributes[a].data->count;
            break;
        }
    }
    if (vertexCount == 0) {
        return;
    }

    mesh.vertices.resize(mesh.vertices.size() + vertexCount);
    MeshVertex* vertices = mesh.vertices.data() + vertexOffset;

    for (cgltf_size a = 0; a < primitive.attributes_count; ++a) {
        const cgltf_attribute&  attribute = primitive.attributes[a];
        const cgltf_accessor*   accessor  = attribute.data;
        if (accessor == nullptr || accessor->count != vertexCount) {
            continue;
        }

        switch (attribute.type) {
            case cgltf_attribute_type_position: {
                for (cgltf_size v = 0; v < vertexCount; ++v) {
                    float values[3]{};
                    cgltf_accessor_read_float(accessor, v, values, 3);
                    vertices[v].position =
                        transformPosition(transform, Vec3{values[0], values[1], values[2]});
                }
                break;
            }
            case cgltf_attribute_type_normal: {
                for (cgltf_size v = 0; v < vertexCount; ++v) {
                    float values[3]{};
                    cgltf_accessor_read_float(accessor, v, values, 3);
                    vertices[v].normal =
                        transformNormal(transform, Vec3{values[0], values[1], values[2]});
                }
                break;
            }
            case cgltf_attribute_type_tangent: {
                for (cgltf_size v = 0; v < vertexCount; ++v) {
                    float values[4]{};
                    cgltf_accessor_read_float(accessor, v, values, 4);
                    const Vec3 direction =
                        transformNormal(transform, Vec3{values[0], values[1], values[2]});
                    // w is handedness, not a direction — it must not be rotated.
                    vertices[v].tangent = Vec4{direction.x, direction.y, direction.z, values[3]};
                }
                break;
            }
            case cgltf_attribute_type_texcoord: {
                if (attribute.index != 0) {
                    break; // one UV set until a material needs more
                }
                for (cgltf_size v = 0; v < vertexCount; ++v) {
                    float values[2]{};
                    cgltf_accessor_read_float(accessor, v, values, 2);
                    vertices[v].uv = Vec2{values[0], values[1]};
                }
                break;
            }
            default:
                break;
        }
    }

    if (primitive.indices != nullptr) {
        const cgltf_accessor* accessor = primitive.indices;
        mesh.indices.reserve(mesh.indices.size() + accessor->count);
        for (cgltf_size i = 0; i < accessor->count; ++i) {
            mesh.indices.push_back(
                static_cast<std::uint32_t>(cgltf_accessor_read_index(accessor, i)));
        }
    } else {
        // Non-indexed primitives still get an index buffer so the renderer has
        // exactly one draw path.
        mesh.indices.reserve(mesh.indices.size() + vertexCount);
        for (std::size_t i = 0; i < vertexCount; ++i) {
            mesh.indices.push_back(static_cast<std::uint32_t>(i));
        }
    }

    SubMesh subMesh;
    subMesh.firstIndex   = firstIndex;
    subMesh.indexCount   = static_cast<std::uint32_t>(mesh.indices.size()) - firstIndex;
    subMesh.vertexOffset = vertexOffset;
    subMesh.material     = primitive.material != nullptr
                         ? static_cast<std::int32_t>(primitive.material - data.materials)
                         : -1;
    mesh.subMeshes.push_back(subMesh);
}

void readNode(const cgltf_node& node, const cgltf_data& data, MeshAsset& mesh)
{
    Matrix4 transform;
    cgltf_node_transform_world(&node, glm::value_ptr(transform));

    if (node.mesh != nullptr) {
        for (cgltf_size p = 0; p < node.mesh->primitives_count; ++p) {
            readPrimitive(node.mesh->primitives[p], transform, data, mesh);
        }
    }
    for (cgltf_size c = 0; c < node.children_count; ++c) {
        readNode(*node.children[c], data, mesh);
    }
}

} // namespace

void MeshAsset::recomputeBounds() noexcept
{
    bounds = AABB{};
    for (SubMesh& subMesh : subMeshes) {
        subMesh.bounds = AABB{};
        // Indices are primitive-local; vertexOffset is what vkCmdDrawIndexed
        // adds on the GPU. Anything walking the CPU arrays has to add it too.
        for (std::uint32_t i = 0; i < subMesh.indexCount; ++i) {
            const std::uint32_t index = indices[subMesh.firstIndex + i] + subMesh.vertexOffset;
            if (index < vertices.size()) {
                subMesh.bounds.grow(vertices[index].position);
            }
        }
        if (subMesh.bounds.valid()) {
            bounds.grow(subMesh.bounds.min);
            bounds.grow(subMesh.bounds.max);
        }
    }
}

GltfImportResult importGltf(const fs::path& path, const AssetDatabase* database)
{
    GltfImportResult result;

    cgltf_options options{};
    cgltf_data*   data = nullptr;

    cgltf_result parsed = cgltf_parse_file(&options, path.string().c_str(), &data);
    if (parsed != cgltf_result_success) {
        result.error = "cgltf_parse_file failed for " + path.string();
        return result;
    }

    // Buffers may be external files or data: URIs; cgltf handles both, but only
    // if asked.
    parsed = cgltf_load_buffers(&options, data, path.string().c_str());
    if (parsed != cgltf_result_success) {
        cgltf_free(data);
        result.error = "cgltf_load_buffers failed for " + path.string();
        return result;
    }

    if (cgltf_validate(data) != cgltf_result_success) {
        cgltf_free(data);
        result.error = "cgltf_validate rejected " + path.string();
        return result;
    }

    auto mesh = std::make_shared<MeshAsset>();
    const fs::path baseDirectory = path.parent_path();

    mesh->materials.resize(data->materials_count);
    for (cgltf_size m = 0; m < data->materials_count; ++m) {
        readMaterial(&data->materials[m], baseDirectory, database, mesh->materials[m]);
    }

    // Walk scene roots so node transforms are honoured. A file with no scene
    // still has meshes worth reading, so fall back to every node.
    if (data->scene != nullptr) {
        for (cgltf_size n = 0; n < data->scene->nodes_count; ++n) {
            readNode(*data->scene->nodes[n], *data, *mesh);
        }
    } else {
        for (cgltf_size n = 0; n < data->nodes_count; ++n) {
            if (data->nodes[n].parent == nullptr) {
                readNode(data->nodes[n], *data, *mesh);
            }
        }
    }

    cgltf_free(data);

    if (mesh->empty()) {
        result.error = "no triangle geometry in " + path.string();
        return result;
    }

    mesh->recomputeBounds();
    result.mesh = std::move(mesh);
    return result;
}

void registerGltfLoader(AssetManager& manager, const AssetDatabase* database)
{
    manager.registerLoader(AssetType::Mesh,
        [database](const fs::path& path) -> std::shared_ptr<Asset> {
            GltfImportResult imported = importGltf(path, database);
            return imported.mesh;
        });
}

} // namespace harpia
