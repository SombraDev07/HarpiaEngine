#include "Navigation/NavigationWorld.h"

namespace harpia {

bool NavigationWorld::bake(std::span<const Vec3>          verts,
                           std::span<const std::uint32_t> indices,
                           const AgentParams&             agent,
                           int                            maxCrowd)
{
    if (!mesh_.bake(verts, indices, agent)) {
        return false;
    }
    return crowd_.init(mesh_, maxCrowd);
}

bool NavigationWorld::bake(const MeshAsset&   mesh,
                           const AgentParams& agent,
                           int                maxCrowd)
{
    if (!mesh_.bake(mesh, agent)) {
        return false;
    }
    return crowd_.init(mesh_, maxCrowd);
}

} // namespace harpia
