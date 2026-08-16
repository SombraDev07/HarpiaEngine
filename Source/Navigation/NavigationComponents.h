// Harpia Engine — navigation components
//
// Registered at the point of use. The World is templated; this file does not
// need a central component list, and no other agent has to touch it.
#pragma once

#include "Core/Math/Math.h"
#include "Core/Reflection/Reflect.h"
#include "Navigation/Crowd.h"
#include "Navigation/PerceptionLod.h"

#include <cstdint>

namespace harpia {

struct NavAgent {
    int   crowdIndex = Crowd::kInvalid;
    Vec3  destination{};
    float radius   = 0.3f;
    float height   = 1.8f;
    float maxSpeed = 3.5f;
};

struct Perception {
    PerceptionLod  lod{};
    PerceptionTier tier            = PerceptionTier::Full;
    float          distanceToFocus = 0.0f;
    std::uint32_t  tick            = 0;

    [[nodiscard]] bool shouldThink() const noexcept
    {
        return lod.shouldThink(tier, tick);
    }

    void observe(float distance) noexcept
    {
        distanceToFocus = distance;
        tier            = lod.classify(distance);
    }
};

} // namespace harpia

HARPIA_REFLECT_BEGIN(harpia::NavAgent, 1)
    HARPIA_FIELD(crowdIndex)
    HARPIA_FIELD(radius)
    HARPIA_FIELD(height)
    HARPIA_FIELD(maxSpeed)
HARPIA_REFLECT_END(harpia::NavAgent)

HARPIA_REFLECT_BEGIN(harpia::Perception, 1)
    HARPIA_FIELD(distanceToFocus)
    HARPIA_FIELD(tick)
HARPIA_REFLECT_END(harpia::Perception)
