// Harpia Engine — local avoidance
//
// DetourCrowd is ORCA/RVO. Writing our own would be a research project and a
// worse result. This is the thin owner: agents in, positions out, no threads.
#pragma once

#include "Navigation/NavMesh.h"

#include <cstdint>

class dtCrowd;

namespace harpia {

struct CrowdAgentParams {
    float radius          = 0.3f;
    float height          = 1.8f;
    float maxAcceleration = 8.0f;
    float maxSpeed        = 3.5f;
    float separation      = 2.0f;
};

class Crowd {
public:
    static constexpr int kInvalid = -1;

    Crowd();
    ~Crowd();

    Crowd(const Crowd&)            = delete;
    Crowd& operator=(const Crowd&) = delete;

    [[nodiscard]] bool init(NavMesh& nav, int maxAgents = 128);
    void               shutdown();

    [[nodiscard]] int  addAgent(Vec3 position, const CrowdAgentParams& params = {});
    void               removeAgent(int index);
    [[nodiscard]] bool requestMove(int index, Vec3 target);
    void               update(float dt);

    [[nodiscard]] bool valid() const noexcept { return crowd_ != nullptr; }
    [[nodiscard]] int  count() const noexcept;

    [[nodiscard]] Vec3 position(int index) const;
    [[nodiscard]] Vec3 velocity(int index) const;
    [[nodiscard]] bool active(int index) const;

private:
    dtCrowd* crowd_ = nullptr;
    NavMesh* nav_   = nullptr;
};

} // namespace harpia
