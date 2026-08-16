// Harpia Engine — navigation facade
//
// Owns the baked mesh and the crowd that walks on it. Gameplay talks to this,
// not to Recast types. Bake is the only expensive call; update is the tick.
#pragma once

#include "Navigation/Crowd.h"
#include "Navigation/NavMesh.h"

namespace harpia {

class MeshAsset;

class NavigationWorld {
public:
    [[nodiscard]] bool bake(std::span<const Vec3>          verts,
                            std::span<const std::uint32_t> indices,
                            const AgentParams&             agent    = {},
                            int                            maxCrowd = 128);

    [[nodiscard]] bool bake(const MeshAsset&   mesh,
                            const AgentParams& agent    = {},
                            int                maxCrowd = 128);

    void update(float dt) { crowd_.update(dt); }

    [[nodiscard]] NavMesh&       mesh() noexcept { return mesh_; }
    [[nodiscard]] const NavMesh& mesh() const noexcept { return mesh_; }
    [[nodiscard]] Crowd&         crowd() noexcept { return crowd_; }
    [[nodiscard]] const Crowd&   crowd() const noexcept { return crowd_; }

private:
    NavMesh mesh_;
    Crowd   crowd_;
};

} // namespace harpia
