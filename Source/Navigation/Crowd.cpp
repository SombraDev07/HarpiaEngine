#include "Navigation/Crowd.h"

#include "Navigation/RecastAllocator.h"

#include <DetourCrowd.h>
#include <DetourNavMeshQuery.h>

#include <cstring>

namespace harpia {
namespace {

constexpr unsigned char kAvoidQuality = 3;

void fillAgentParams(dtCrowdAgentParams& out, const CrowdAgentParams& in)
{
    std::memset(&out, 0, sizeof(out));
    out.radius                = in.radius;
    out.height                = in.height;
    out.maxAcceleration       = in.maxAcceleration;
    out.maxSpeed              = in.maxSpeed;
    out.collisionQueryRange   = in.radius * 12.0f;
    out.pathOptimizationRange = in.radius * 30.0f;
    out.separationWeight      = in.separation;
    out.updateFlags           = static_cast<unsigned char>(
        DT_CROWD_ANTICIPATE_TURNS | DT_CROWD_OPTIMIZE_VIS | DT_CROWD_OPTIMIZE_TOPO |
        DT_CROWD_OBSTACLE_AVOIDANCE | DT_CROWD_SEPARATION);
    out.obstacleAvoidanceType = kAvoidQuality;
}

} // namespace

Crowd::Crowd()
{
    nav::installRecastAllocator();
}

Crowd::~Crowd()
{
    shutdown();
}

void Crowd::shutdown()
{
    if (crowd_ != nullptr) {
        dtFreeCrowd(crowd_);
        crowd_ = nullptr;
    }
    nav_ = nullptr;
}

bool Crowd::init(NavMesh& nav, int maxAgents)
{
    shutdown();
    if (!nav.valid() || maxAgents <= 0) {
        return false;
    }

    crowd_ = dtAllocCrowd();
    if (crowd_ == nullptr) {
        return false;
    }
    if (!crowd_->init(maxAgents, nav.agent().radius, nav.mesh())) {
        dtFreeCrowd(crowd_);
        crowd_ = nullptr;
        return false;
    }

    dtObstacleAvoidanceParams params{};
    params.velBias       = 0.4f;
    params.weightDesVel  = 2.0f;
    params.weightCurVel  = 0.75f;
    params.weightSide    = 0.75f;
    params.weightToi     = 2.5f;
    params.horizTime     = 2.5f;
    params.gridSize      = 33;
    params.adaptiveDivs  = 7;
    params.adaptiveRings = 2;
    params.adaptiveDepth = 5;
    crowd_->setObstacleAvoidanceParams(kAvoidQuality, &params);

    nav_ = &nav;
    return true;
}

int Crowd::addAgent(Vec3 position, const CrowdAgentParams& params)
{
    if (crowd_ == nullptr) {
        return kInvalid;
    }
    dtCrowdAgentParams ap{};
    fillAgentParams(ap, params);
    const float p[3] = {position.x, position.y, position.z};
    return crowd_->addAgent(p, &ap);
}

void Crowd::removeAgent(int index)
{
    if (crowd_ != nullptr && index >= 0) {
        crowd_->removeAgent(index);
    }
}

bool Crowd::requestMove(int index, Vec3 target)
{
    if (crowd_ == nullptr || nav_ == nullptr || nav_->query() == nullptr) {
        return false;
    }

    const float t[3]   = {target.x, target.y, target.z};
    const float ext[3] = {2.0f, 4.0f, 2.0f};
    dtQueryFilter filter;
    dtPolyRef    ref = 0;
    float        nearest[3]{};
    if (dtStatusFailed(nav_->query()->findNearestPoly(t, ext, &filter, &ref, nearest)) ||
        ref == 0) {
        return false;
    }
    return crowd_->requestMoveTarget(index, ref, nearest);
}

void Crowd::update(float dt)
{
    if (crowd_ != nullptr) {
        crowd_->update(dt, nullptr);
    }
}

int Crowd::count() const noexcept
{
    if (crowd_ == nullptr) {
        return 0;
    }
    int live = 0;
    const int max = crowd_->getAgentCount();
    for (int i = 0; i < max; ++i) {
        const dtCrowdAgent* agent = crowd_->getAgent(i);
        if (agent != nullptr && agent->active) {
            ++live;
        }
    }
    return live;
}

Vec3 Crowd::position(int index) const
{
    if (crowd_ == nullptr) {
        return {};
    }
    const dtCrowdAgent* agent = crowd_->getAgent(index);
    if (agent == nullptr || !agent->active) {
        return {};
    }
    return Vec3{agent->npos[0], agent->npos[1], agent->npos[2]};
}

Vec3 Crowd::velocity(int index) const
{
    if (crowd_ == nullptr) {
        return {};
    }
    const dtCrowdAgent* agent = crowd_->getAgent(index);
    if (agent == nullptr || !agent->active) {
        return {};
    }
    return Vec3{agent->vel[0], agent->vel[1], agent->vel[2]};
}

bool Crowd::active(int index) const
{
    if (crowd_ == nullptr) {
        return false;
    }
    const dtCrowdAgent* agent = crowd_->getAgent(index);
    return agent != nullptr && agent->active;
}

} // namespace harpia
