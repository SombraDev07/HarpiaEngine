// Harpia Engine — Jolt types at the physics boundary
//
// Roadmap 4.3 is explicit: Jolt is used directly. These helpers exist so glm
// and Jolt do not silently disagree on quaternion layout (glm ctor is wxyz,
// Jolt ctor is xyzw). They are not a backend abstraction.
#pragma once

#include "Core/Math/Math.h"

#include <Jolt/Jolt.h>
#include <Jolt/Physics/Body/BodyID.h>
#include <Jolt/Physics/Collision/ObjectLayer.h>
#include <Jolt/Physics/Collision/BroadPhase/BroadPhaseLayer.h>

#include <cstdint>

namespace harpia::phys {

// Two layers is the minimum that keeps the static broad-phase tree from being
// rebuilt every time a dynamic body moves.
namespace layers {
inline constexpr JPH::ObjectLayer     nonMoving = 0;
inline constexpr JPH::ObjectLayer     moving    = 1;
inline constexpr JPH::ObjectLayer     count     = 2;

inline constexpr JPH::BroadPhaseLayer bpNonMoving{0};
inline constexpr JPH::BroadPhaseLayer bpMoving{1};
inline constexpr JPH::uint            bpCount = 2;
} // namespace layers

struct CharacterHandle {
    std::uint32_t index      = 0;
    std::uint32_t generation = 0;

    [[nodiscard]] constexpr bool valid() const noexcept { return generation != 0; }

    [[nodiscard]] friend constexpr bool operator==(CharacterHandle a, CharacterHandle b) noexcept
    {
        return a.index == b.index && a.generation == b.generation;
    }
};

struct RayHit {
    bool          hit      = false;
    float         fraction = 1.0f;
    Vec3          point{};
    Vec3          normal{};
    JPH::BodyID   bodyId;
};

[[nodiscard]] inline JPH::Vec3 toJolt(Vec3 v) noexcept
{
    return JPH::Vec3(v.x, v.y, v.z);
}

[[nodiscard]] inline Vec3 fromJolt(JPH::Vec3Arg v) noexcept
{
    return Vec3(v.GetX(), v.GetY(), v.GetZ());
}

[[nodiscard]] inline JPH::Quat toJolt(Quat q) noexcept
{
    return JPH::Quat(q.x, q.y, q.z, q.w);
}

[[nodiscard]] inline Quat fromJolt(JPH::QuatArg q) noexcept
{
    return Quat(q.GetW(), q.GetX(), q.GetY(), q.GetZ());
}

#if defined(JPH_DOUBLE_PRECISION)
[[nodiscard]] inline JPH::RVec3 toJoltR(Vec3 v) noexcept
{
    return JPH::RVec3(v.x, v.y, v.z);
}

[[nodiscard]] inline Vec3 fromJoltR(JPH::RVec3Arg v) noexcept
{
    return Vec3(static_cast<float>(v.GetX()),
                static_cast<float>(v.GetY()),
                static_cast<float>(v.GetZ()));
}
#else
[[nodiscard]] inline JPH::RVec3 toJoltR(Vec3 v) noexcept
{
    return toJolt(v);
}

[[nodiscard]] inline Vec3 fromJoltR(JPH::RVec3Arg v) noexcept
{
    return fromJolt(v);
}
#endif

} // namespace harpia::phys
