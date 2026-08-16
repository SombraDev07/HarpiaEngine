// Harpia Engine — Recast bake + Detour query
//
// We do not write a navmesh generator. Recast rasterizes the walkable surface;
// Detour answers the queries. This file is the seam: triangle soup in, path out.
#pragma once

#include "Core/Math/Math.h"

#include <cstddef>
#include <span>
#include <vector>

class dtNavMesh;
class dtNavMeshQuery;

namespace harpia {

class MeshAsset;

struct AgentParams {
    float radius     = 0.3f;
    float height     = 1.8f;
    float maxClimb   = 0.4f;
    float maxSlope   = 45.0f;
    float cellSize   = 0.2f;
    float cellHeight = 0.2f;
};

class NavMesh {
public:
    NavMesh();
    ~NavMesh();

    NavMesh(const NavMesh&)            = delete;
    NavMesh& operator=(const NavMesh&) = delete;

    // Bake runs on the JobSystem when it is up. The Recast pipeline itself is
    // the library's; we only feed it triangles and keep the Detour result.
    // Winding is CCW when seen from +Y — the opposite is a ceiling.
    [[nodiscard]] bool bake(std::span<const Vec3>          verts,
                            std::span<const std::uint32_t> indices,
                            const AgentParams&             agent = {});

    [[nodiscard]] bool bake(const MeshAsset& mesh, const AgentParams& agent = {});

    void clear();

    [[nodiscard]] bool        valid() const noexcept { return mesh_ != nullptr; }
    [[nodiscard]] int         polygonCount() const noexcept;
    [[nodiscard]] AgentParams agent() const noexcept { return agent_; }

    // Straight-line corridor. Empty `out` and false when either end is off-mesh.
    [[nodiscard]] bool findPath(Vec3 start, Vec3 end, std::vector<Vec3>& out) const;

    [[nodiscard]] bool findNearest(Vec3 pos, Vec3& out) const;

    [[nodiscard]] dtNavMesh*      mesh() noexcept { return mesh_; }
    [[nodiscard]] const dtNavMesh* mesh() const noexcept { return mesh_; }
    [[nodiscard]] dtNavMeshQuery* query() noexcept { return query_; }

private:
    [[nodiscard]] bool bakeImpl(std::span<const float>       packedVerts,
                                std::span<const int>         tris,
                                const AgentParams&           agent);

    dtNavMesh*      mesh_     = nullptr;
    dtNavMeshQuery* query_    = nullptr;
    AgentParams     agent_{};
    int             polyCount_ = 0;
};

} // namespace harpia
