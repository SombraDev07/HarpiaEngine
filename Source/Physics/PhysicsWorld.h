// Harpia Engine — owns a JPH::PhysicsSystem
//
// This is not a physics backend. It is the HelloWorld boilerplate (factory,
// layers, temp allocator) plus the two things the engine already owns: jobs
// and tagged allocation. Callers that want more than the helpers talk to
// system() / bodies() — those are Jolt.
#pragma once

#include "Physics/PhysicsComponents.h"
#include "Physics/PhysicsTypes.h"

#include "Core/Assets/MeshAsset.h"
#include "Core/ECS/World.h"
#include "Core/Math/Math.h"

#include <Jolt/Jolt.h>
#include <Jolt/Physics/Body/BodyInterface.h>
#include <Jolt/Physics/Character/CharacterVirtual.h>
#include <Jolt/Physics/PhysicsSystem.h>

#include <cstdint>
#include <memory>
#include <span>

namespace harpia {

class PhysicsWorld {
public:
    struct Config {
        std::uint32_t maxBodies              = 1024;
        std::uint32_t maxBodyPairs           = 1024;
        std::uint32_t maxContactConstraints  = 1024;
        std::size_t   tempAllocatorBytes     = 4 * 1024 * 1024;
        Vec3          gravity{0.0f, -9.81f, 0.0f};
    };

    PhysicsWorld();
    explicit PhysicsWorld(const Config& config);
    ~PhysicsWorld();

    PhysicsWorld(const PhysicsWorld&)            = delete;
    PhysicsWorld& operator=(const PhysicsWorld&) = delete;

    void step(float deltaTime, int collisionSteps = 1);
    void step(ecs::World& world, float deltaTime, int collisionSteps = 1);

    [[nodiscard]] JPH::BodyID addBox(Vec3 position, Vec3 halfExtent,
                                     JPH::EMotionType motion,
                                     float mass = 1.0f,
                                     float friction = 0.5f,
                                     float restitution = 0.0f);

    [[nodiscard]] JPH::BodyID addSphere(Vec3 position, float radius,
                                        JPH::EMotionType motion,
                                        float mass = 1.0f,
                                        float friction = 0.5f,
                                        float restitution = 0.0f);

    [[nodiscard]] JPH::BodyID addCapsule(Vec3 position,
                                         float halfHeight, float radius,
                                         JPH::EMotionType motion,
                                         float mass = 1.0f,
                                         float friction = 0.5f,
                                         float restitution = 0.0f);

    // Triangles are single-sided. Provide them counter-clockwise as seen
    // from the outside; the opposite winding is invisible to simulation.
    [[nodiscard]] JPH::BodyID addTriangleMesh(std::span<const Vec3>          vertices,
                                              std::span<const std::uint32_t> indices,
                                              Vec3 position = {},
                                              Quat rotation = Quat{1.0f, 0.0f, 0.0f, 0.0f});

    [[nodiscard]] JPH::BodyID addCollisionMesh(const MeshAsset& mesh,
                                               Vec3 position = {},
                                               Quat rotation = Quat{1.0f, 0.0f, 0.0f, 0.0f});

    void removeBody(JPH::BodyID id);

    [[nodiscard]] phys::CharacterHandle addCharacter(Vec3 feetPosition,
                                                     float radius = 0.3f,
                                                     float height = 1.8f,
                                                     float maxSlopeDegrees = 45.0f);
    void removeCharacter(phys::CharacterHandle handle);
    void setCharacterVelocity(phys::CharacterHandle handle, Vec3 velocity);
    [[nodiscard]] Vec3 characterPosition(phys::CharacterHandle handle) const;
    [[nodiscard]] JPH::CharacterVirtual* character(phys::CharacterHandle handle) noexcept;
    [[nodiscard]] const JPH::CharacterVirtual* character(phys::CharacterHandle handle) const noexcept;

    [[nodiscard]] phys::RayHit raycast(Vec3 origin, Vec3 direction, float maxDistance) const;

    [[nodiscard]] JPH::PhysicsSystem&       system() noexcept;
    [[nodiscard]] const JPH::PhysicsSystem& system() const noexcept;
    [[nodiscard]] JPH::BodyInterface&       bodies() noexcept;
    [[nodiscard]] const JPH::BodyInterface& bodies() const noexcept;

    [[nodiscard]] Vec3 bodyPosition(JPH::BodyID id) const;
    [[nodiscard]] Quat bodyRotation(JPH::BodyID id) const;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace harpia
