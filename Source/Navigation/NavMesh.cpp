#include "Navigation/NavMesh.h"

#include "Navigation/RecastAllocator.h"
#include "Core/Assets/MeshAsset.h"
#include "Core/Threading/JobSystem.h"

#include <DetourNavMesh.h>
#include <DetourNavMeshBuilder.h>
#include <DetourNavMeshQuery.h>
#include <Recast.h>

#include <cmath>
#include <cstring>
#include <vector>

namespace harpia {
namespace {

class SilentContext final : public rcContext {
public:
    SilentContext() : rcContext(false) {}
};

struct RecastTemps {
    rcHeightfield*       solid = nullptr;
    rcCompactHeightfield* chf   = nullptr;
    rcContourSet*        cset  = nullptr;
    rcPolyMesh*          pmesh = nullptr;
    rcPolyMeshDetail*    dmesh = nullptr;

    ~RecastTemps()
    {
        rcFreeHeightField(solid);
        rcFreeCompactHeightfield(chf);
        rcFreeContourSet(cset);
        rcFreePolyMesh(pmesh);
        rcFreePolyMeshDetail(dmesh);
    }
};

constexpr unsigned short kWalkFlag = 0x01;

} // namespace

NavMesh::NavMesh()
{
    nav::installRecastAllocator();
}

NavMesh::~NavMesh()
{
    clear();
}

void NavMesh::clear()
{
    if (query_ != nullptr) {
        dtFreeNavMeshQuery(query_);
        query_ = nullptr;
    }
    if (mesh_ != nullptr) {
        dtFreeNavMesh(mesh_);
        mesh_ = nullptr;
    }
    polyCount_ = 0;
}

int NavMesh::polygonCount() const noexcept
{
    return polyCount_;
}

bool NavMesh::bake(std::span<const Vec3>          verts,
                   std::span<const std::uint32_t> indices,
                   const AgentParams&             agent)
{
    nav::installRecastAllocator();

    std::vector<float> packed;
    packed.reserve(verts.size() * 3u);
    for (const Vec3& v : verts) {
        packed.push_back(v.x);
        packed.push_back(v.y);
        packed.push_back(v.z);
    }

    std::vector<int> tris;
    tris.reserve(indices.size());
    for (const std::uint32_t index : indices) {
        tris.push_back(static_cast<int>(index));
    }

    bool ok = false;
    auto run = [&] { ok = bakeImpl(packed, tris, agent); };

    JobSystem& jobs = JobSystem::get();
    if (jobs.initialized()) {
        const JobHandle handle = jobs.submit(std::move(run), "navmesh.bake");
        jobs.wait(handle);
    } else {
        run();
    }
    return ok;
}

bool NavMesh::bake(const MeshAsset& mesh, const AgentParams& agent)
{
    if (mesh.empty()) {
        return false;
    }

    std::vector<Vec3> positions;
    positions.reserve(mesh.vertices.size());
    for (const MeshVertex& vertex : mesh.vertices) {
        positions.push_back(vertex.position);
    }
    return bake(positions, mesh.indices, agent);
}

bool NavMesh::bakeImpl(std::span<const float> packedVerts,
                       std::span<const int>   tris,
                       const AgentParams&     agent)
{
    clear();

    if (packedVerts.size() < 9u || tris.size() < 3u || (tris.size() % 3u) != 0u) {
        return false;
    }
    if (agent.cellSize <= 0.0f || agent.cellHeight <= 0.0f) {
        return false;
    }

    const int vertCount = static_cast<int>(packedVerts.size() / 3u);
    const int triCount  = static_cast<int>(tris.size() / 3u);

    SilentContext ctx;
    rcConfig cfg{};
    std::memset(&cfg, 0, sizeof(cfg));
    cfg.cs                     = agent.cellSize;
    cfg.ch                     = agent.cellHeight;
    cfg.walkableSlopeAngle     = agent.maxSlope;
    cfg.walkableHeight         = static_cast<int>(std::ceil(agent.height / cfg.ch));
    cfg.walkableClimb          = static_cast<int>(std::floor(agent.maxClimb / cfg.ch));
    cfg.walkableRadius         = static_cast<int>(std::ceil(agent.radius / cfg.cs));
    cfg.maxEdgeLen             = static_cast<int>(12.0f / cfg.cs);
    cfg.maxSimplificationError = 1.3f;
    cfg.minRegionArea          = static_cast<int>(rcSqr(8));
    cfg.mergeRegionArea        = static_cast<int>(rcSqr(20));
    cfg.maxVertsPerPoly        = DT_VERTS_PER_POLYGON;
    cfg.detailSampleDist       = cfg.cs * 6.0f;
    cfg.detailSampleMaxError   = cfg.ch * 1.0f;

    rcCalcBounds(packedVerts.data(), vertCount, cfg.bmin, cfg.bmax);
    rcCalcGridSize(cfg.bmin, cfg.bmax, cfg.cs, &cfg.width, &cfg.height);

    RecastTemps tmp;
    tmp.solid = rcAllocHeightfield();
    if (tmp.solid == nullptr) {
        return false;
    }
    if (!rcCreateHeightfield(&ctx, *tmp.solid, cfg.width, cfg.height,
                             cfg.bmin, cfg.bmax, cfg.cs, cfg.ch)) {
        return false;
    }

    std::vector<unsigned char> areas(static_cast<std::size_t>(triCount), 0);
    rcMarkWalkableTriangles(&ctx, cfg.walkableSlopeAngle,
                            packedVerts.data(), vertCount,
                            tris.data(), triCount, areas.data());
    if (!rcRasterizeTriangles(&ctx, packedVerts.data(), vertCount,
                              tris.data(), areas.data(), triCount,
                              *tmp.solid, cfg.walkableClimb)) {
        return false;
    }

    rcFilterLowHangingWalkableObstacles(&ctx, cfg.walkableClimb, *tmp.solid);
    rcFilterLedgeSpans(&ctx, cfg.walkableHeight, cfg.walkableClimb, *tmp.solid);
    rcFilterWalkableLowHeightSpans(&ctx, cfg.walkableHeight, *tmp.solid);

    tmp.chf = rcAllocCompactHeightfield();
    if (tmp.chf == nullptr) {
        return false;
    }
    if (!rcBuildCompactHeightfield(&ctx, cfg.walkableHeight, cfg.walkableClimb,
                                   *tmp.solid, *tmp.chf)) {
        return false;
    }
    rcFreeHeightField(tmp.solid);
    tmp.solid = nullptr;

    if (!rcErodeWalkableArea(&ctx, cfg.walkableRadius, *tmp.chf)) {
        return false;
    }

    // Monotone: guaranteed no holes, fast enough for a solo bake.
    if (!rcBuildRegionsMonotone(&ctx, *tmp.chf, 0, cfg.minRegionArea, cfg.mergeRegionArea)) {
        return false;
    }

    tmp.cset = rcAllocContourSet();
    if (tmp.cset == nullptr) {
        return false;
    }
    if (!rcBuildContours(&ctx, *tmp.chf, cfg.maxSimplificationError, cfg.maxEdgeLen, *tmp.cset)) {
        return false;
    }

    tmp.pmesh = rcAllocPolyMesh();
    if (tmp.pmesh == nullptr) {
        return false;
    }
    if (!rcBuildPolyMesh(&ctx, *tmp.cset, cfg.maxVertsPerPoly, *tmp.pmesh)) {
        return false;
    }

    tmp.dmesh = rcAllocPolyMeshDetail();
    if (tmp.dmesh == nullptr) {
        return false;
    }
    if (!rcBuildPolyMeshDetail(&ctx, *tmp.pmesh, *tmp.chf,
                               cfg.detailSampleDist, cfg.detailSampleMaxError,
                               *tmp.dmesh)) {
        return false;
    }

    rcFreeCompactHeightfield(tmp.chf);
    tmp.chf = nullptr;
    rcFreeContourSet(tmp.cset);
    tmp.cset = nullptr;

    if (tmp.pmesh->nverts == 0 || tmp.pmesh->npolys == 0) {
        return false;
    }

    for (int i = 0; i < tmp.pmesh->npolys; ++i) {
        if (tmp.pmesh->areas[i] == RC_WALKABLE_AREA) {
            tmp.pmesh->flags[i] = kWalkFlag;
        }
    }

    dtNavMeshCreateParams params{};
    std::memset(&params, 0, sizeof(params));
    params.verts            = tmp.pmesh->verts;
    params.vertCount        = tmp.pmesh->nverts;
    params.polys            = tmp.pmesh->polys;
    params.polyAreas        = tmp.pmesh->areas;
    params.polyFlags        = tmp.pmesh->flags;
    params.polyCount        = tmp.pmesh->npolys;
    params.nvp              = tmp.pmesh->nvp;
    params.detailMeshes     = tmp.dmesh->meshes;
    params.detailVerts      = tmp.dmesh->verts;
    params.detailVertsCount = tmp.dmesh->nverts;
    params.detailTris       = tmp.dmesh->tris;
    params.detailTriCount   = tmp.dmesh->ntris;
    params.walkableHeight   = agent.height;
    params.walkableRadius   = agent.radius;
    params.walkableClimb    = agent.maxClimb;
    rcVcopy(params.bmin, tmp.pmesh->bmin);
    rcVcopy(params.bmax, tmp.pmesh->bmax);
    params.cs          = cfg.cs;
    params.ch          = cfg.ch;
    params.buildBvTree = true;

    unsigned char* navData     = nullptr;
    int            navDataSize = 0;
    if (!dtCreateNavMeshData(&params, &navData, &navDataSize)) {
        return false;
    }

    dtNavMesh* mesh = dtAllocNavMesh();
    if (mesh == nullptr) {
        dtFree(navData);
        return false;
    }
    if (dtStatusFailed(mesh->init(navData, navDataSize, DT_TILE_FREE_DATA))) {
        dtFree(navData);
        dtFreeNavMesh(mesh);
        return false;
    }

    dtNavMeshQuery* query = dtAllocNavMeshQuery();
    if (query == nullptr) {
        dtFreeNavMesh(mesh);
        return false;
    }
    if (dtStatusFailed(query->init(mesh, 2048))) {
        dtFreeNavMeshQuery(query);
        dtFreeNavMesh(mesh);
        return false;
    }

    mesh_      = mesh;
    query_     = query;
    agent_     = agent;
    polyCount_ = tmp.pmesh->npolys;
    return true;
}

bool NavMesh::findNearest(Vec3 pos, Vec3& out) const
{
    if (query_ == nullptr) {
        return false;
    }

    const float p[3]   = {pos.x, pos.y, pos.z};
    const float ext[3] = {2.0f, 4.0f, 2.0f};
    dtQueryFilter filter;
    dtPolyRef    ref = 0;
    float        nearest[3]{};
    if (dtStatusFailed(query_->findNearestPoly(p, ext, &filter, &ref, nearest)) || ref == 0) {
        return false;
    }
    out = Vec3{nearest[0], nearest[1], nearest[2]};
    return true;
}

bool NavMesh::findPath(Vec3 start, Vec3 end, std::vector<Vec3>& out) const
{
    out.clear();
    if (query_ == nullptr) {
        return false;
    }

    const float s[3]   = {start.x, start.y, start.z};
    const float e[3]   = {end.x, end.y, end.z};
    const float ext[3] = {2.0f, 4.0f, 2.0f};
    dtQueryFilter filter;

    dtPolyRef startRef = 0;
    dtPolyRef endRef   = 0;
    float     nearestS[3]{};
    float     nearestE[3]{};
    if (dtStatusFailed(query_->findNearestPoly(s, ext, &filter, &startRef, nearestS)) ||
        startRef == 0) {
        return false;
    }
    if (dtStatusFailed(query_->findNearestPoly(e, ext, &filter, &endRef, nearestE)) ||
        endRef == 0) {
        return false;
    }

    dtPolyRef polys[256]{};
    int       npolys = 0;
    if (dtStatusFailed(query_->findPath(startRef, endRef, nearestS, nearestE,
                                        &filter, polys, &npolys, 256)) ||
        npolys <= 0) {
        return false;
    }

    float straight[256 * 3]{};
    int   nstraight = 0;
    if (dtStatusFailed(query_->findStraightPath(nearestS, nearestE, polys, npolys,
                                                straight, nullptr, nullptr,
                                                &nstraight, 256)) ||
        nstraight <= 0) {
        return false;
    }

    out.reserve(static_cast<std::size_t>(nstraight));
    for (int i = 0; i < nstraight; ++i) {
        out.emplace_back(straight[i * 3], straight[i * 3 + 1], straight[i * 3 + 2]);
    }
    return nstraight >= 2;
}

} // namespace harpia
