#include "RHI/MeshPrimitives.h"

#include <cmath>

namespace harpia::rhi {
namespace {

void finish(MeshAsset& mesh)
{
    SubMesh sub;
    sub.firstIndex   = 0;
    sub.indexCount   = static_cast<std::uint32_t>(mesh.indices.size());
    sub.vertexOffset = 0;
    sub.material     = 0;
    mesh.subMeshes.push_back(sub);

    mesh.materials.emplace_back();
    mesh.recomputeBounds();
}

} // namespace

MeshAsset makeSphere(float radius, std::uint32_t segments, std::uint32_t rings)
{
    MeshAsset mesh;

    segments = segments < 3 ? 3 : segments;
    rings    = rings < 2 ? 2 : rings;

    // Rings and segments both get one extra row so the UV seam has its own
    // vertices; sharing them would wrap the texture backwards across the seam.
    for (std::uint32_t ring = 0; ring <= rings; ++ring) {
        const float v     = static_cast<float>(ring) / static_cast<float>(rings);
        const float theta = v * kPi;
        const float sinTheta = std::sin(theta);
        const float cosTheta = std::cos(theta);

        for (std::uint32_t segment = 0; segment <= segments; ++segment) {
            const float u   = static_cast<float>(segment) / static_cast<float>(segments);
            const float phi = u * 2.0f * kPi;

            const Vec3 normal{sinTheta * std::cos(phi), cosTheta, sinTheta * std::sin(phi)};

            MeshVertex vertex;
            vertex.position = normal * radius;
            vertex.normal   = normal;
            // Tangent runs along increasing longitude.
            vertex.tangent  = Vec4(-std::sin(phi), 0.0f, std::cos(phi), 1.0f);
            vertex.uv       = Vec2(u, v);
            mesh.vertices.push_back(vertex);
        }
    }

    const std::uint32_t stride = segments + 1;
    for (std::uint32_t ring = 0; ring < rings; ++ring) {
        for (std::uint32_t segment = 0; segment < segments; ++segment) {
            const std::uint32_t current = ring * stride + segment;
            const std::uint32_t below   = current + stride;

            // Wound so the outward face is front-facing once the projection's
            // Y flip is accounted for. Getting this backwards renders the
            // inside of the sphere, which looks like a lighting bug rather
            // than a topology one.
            mesh.indices.push_back(current);
            mesh.indices.push_back(current + 1);
            mesh.indices.push_back(below);

            mesh.indices.push_back(current + 1);
            mesh.indices.push_back(below + 1);
            mesh.indices.push_back(below);
        }
    }

    finish(mesh);
    return mesh;
}

MeshAsset makeCube(float size)
{
    MeshAsset mesh;
    const float h = size * 0.5f;

    struct Face {
        Vec3 normal;
        Vec3 tangent;
        Vec3 corners[4];
    };

    const Face faces[6] = {
        {{ 0,  0,  1}, {1, 0, 0}, {{-h,-h, h}, { h,-h, h}, { h, h, h}, {-h, h, h}}},
        {{ 0,  0, -1}, {-1,0, 0}, {{ h,-h,-h}, {-h,-h,-h}, {-h, h,-h}, { h, h,-h}}},
        {{ 1,  0,  0}, {0, 0,-1}, {{ h,-h, h}, { h,-h,-h}, { h, h,-h}, { h, h, h}}},
        {{-1,  0,  0}, {0, 0, 1}, {{-h,-h,-h}, {-h,-h, h}, {-h, h, h}, {-h, h,-h}}},
        {{ 0,  1,  0}, {1, 0, 0}, {{-h, h, h}, { h, h, h}, { h, h,-h}, {-h, h,-h}}},
        {{ 0, -1,  0}, {1, 0, 0}, {{-h,-h,-h}, { h,-h,-h}, { h,-h, h}, {-h,-h, h}}},
    };

    for (const Face& face : faces) {
        const auto base = static_cast<std::uint32_t>(mesh.vertices.size());

        const Vec2 uvs[4] = {{0, 0}, {1, 0}, {1, 1}, {0, 1}};
        for (int i = 0; i < 4; ++i) {
            MeshVertex vertex;
            vertex.position = face.corners[i];
            vertex.normal   = face.normal;
            vertex.tangent  = Vec4(face.tangent, 1.0f);
            vertex.uv       = uvs[i];
            mesh.vertices.push_back(vertex);
        }

        for (const std::uint32_t offset : {0u, 1u, 2u, 0u, 2u, 3u}) {
            mesh.indices.push_back(base + offset);
        }
    }

    finish(mesh);
    return mesh;
}

MeshAsset makePlane(float size)
{
    MeshAsset mesh;
    const float h = size * 0.5f;

    const Vec3 corners[4] = {{-h, 0, -h}, {h, 0, -h}, {h, 0, h}, {-h, 0, h}};
    const Vec2 uvs[4]     = {{0, 0}, {1, 0}, {1, 1}, {0, 1}};

    for (int i = 0; i < 4; ++i) {
        MeshVertex vertex;
        vertex.position = corners[i];
        vertex.normal   = Vec3(0, 1, 0);
        vertex.tangent  = Vec4(1, 0, 0, 1);
        vertex.uv       = uvs[i] * (size * 0.25f); // tile rather than stretch
        mesh.vertices.push_back(vertex);
    }

    // Wound so the face points at +Y.
    for (const std::uint32_t index : {0u, 2u, 1u, 0u, 3u, 2u}) {
        mesh.indices.push_back(index);
    }

    finish(mesh);
    return mesh;
}

} // namespace harpia::rhi
