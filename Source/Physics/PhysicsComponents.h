// Harpia Engine — physics components
//
// Registered at the point of use: World is templated, there is no central
// component list. RigidBody / CharacterController / CollisionMesh are the
// 4.3 surface. Transform lives in Core/Math and is reflected here so physics
// can write it back without inventing a second pose type.
#pragma once

#include "Core/Math/Math.h"
#include "Core/Reflection/Reflect.h"

#include <cstdint>

namespace harpia {

enum class BodyMotion : std::uint8_t {
    Static    = 0,
    Kinematic = 1,
    Dynamic   = 2,
};

enum class CollisionShape : std::uint8_t {
    Box     = 0,
    Sphere  = 1,
    Capsule = 2,
};

struct RigidBody {
    BodyMotion      motion        = BodyMotion::Dynamic;
    CollisionShape  shape         = CollisionShape::Box;
    float           halfExtentX   = 0.5f;
    float           halfExtentY   = 0.5f;
    float           halfExtentZ   = 0.5f;
    float           radius        = 0.5f;
    float           capsuleHalfHeight = 0.5f;
    float           mass          = 1.0f;
    float           friction      = 0.5f;
    float           restitution   = 0.0f;
    std::uint32_t   joltBodyId    = 0;
    bool            spawned       = false;
};

struct CharacterController {
    float         radius             = 0.3f;
    float         height             = 1.8f;
    float         maxSlopeDegrees    = 45.0f;
    float         desiredVelocityX   = 0.0f;
    float         desiredVelocityY   = 0.0f;
    float         desiredVelocityZ   = 0.0f;
    std::uint32_t characterIndex     = 0;
    std::uint32_t characterGeneration = 0;
    bool          spawned            = false;
};

struct CollisionMesh {
    std::uint32_t joltBodyId = 0;
    bool          spawned    = false;
};

} // namespace harpia

HARPIA_REFLECT_BEGIN(harpia::Vec3, 1)
    HARPIA_FIELD(x) HARPIA_FIELD(y) HARPIA_FIELD(z)
HARPIA_REFLECT_END(harpia::Vec3)

HARPIA_REFLECT_BEGIN(harpia::Quat, 1)
    HARPIA_FIELD(w) HARPIA_FIELD(x) HARPIA_FIELD(y) HARPIA_FIELD(z)
HARPIA_REFLECT_END(harpia::Quat)

HARPIA_REFLECT_BEGIN(harpia::Transform, 1)
    HARPIA_FIELD(position)
    HARPIA_FIELD(rotation)
    HARPIA_FIELD(scale)
HARPIA_REFLECT_END(harpia::Transform)

HARPIA_REFLECT_BEGIN(harpia::RigidBody, 1)
    HARPIA_FIELD(motion)
    HARPIA_FIELD(shape)
    HARPIA_FIELD(halfExtentX)
    HARPIA_FIELD(halfExtentY)
    HARPIA_FIELD(halfExtentZ)
    HARPIA_FIELD(radius)
    HARPIA_FIELD(capsuleHalfHeight)
    HARPIA_FIELD(mass)
    HARPIA_FIELD(friction)
    HARPIA_FIELD(restitution)
    HARPIA_FIELD_HIDDEN(joltBodyId)
    HARPIA_FIELD_HIDDEN(spawned)
HARPIA_REFLECT_END(harpia::RigidBody)

HARPIA_REFLECT_BEGIN(harpia::CharacterController, 1)
    HARPIA_FIELD(radius)
    HARPIA_FIELD(height)
    HARPIA_FIELD(maxSlopeDegrees)
    HARPIA_FIELD(desiredVelocityX)
    HARPIA_FIELD(desiredVelocityY)
    HARPIA_FIELD(desiredVelocityZ)
    HARPIA_FIELD_HIDDEN(characterIndex)
    HARPIA_FIELD_HIDDEN(characterGeneration)
    HARPIA_FIELD_HIDDEN(spawned)
HARPIA_REFLECT_END(harpia::CharacterController)

HARPIA_REFLECT_BEGIN(harpia::CollisionMesh, 1)
    HARPIA_FIELD_HIDDEN(joltBodyId)
    HARPIA_FIELD_HIDDEN(spawned)
HARPIA_REFLECT_END(harpia::CollisionMesh)
