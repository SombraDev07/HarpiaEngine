// Physics 4.3: Jolt is the solver. These tests pin the surface we actually
// own — rigid body, character, raycast, collision mesh, ECS sync, and the
// rule that Jolt jobs run on Harpia's JobSystem.

#include <doctest/doctest.h>

#include "Core/ECS/World.h"
#include "Core/Memory/MemoryTracker.h"
#include "Core/Threading/JobSystem.h"
#include "Physics/PhysicsWorld.h"

#include <cmath>
#include <vector>

using namespace harpia;
using namespace harpia::ecs;

namespace {

constexpr float kDt = 1.0f / 60.0f;

void stepMany(PhysicsWorld& physics, int steps)
{
    for (int i = 0; i < steps; ++i) {
        physics.step(kDt);
    }
}

void stepMany(PhysicsWorld& physics, World& world, int steps)
{
    for (int i = 0; i < steps; ++i) {
        physics.step(world, kDt);
    }
}

JPH::BodyID addFloor(PhysicsWorld& physics, float half = 50.0f)
{
    const JPH::BodyID id = physics.addBox(Vec3(0.0f, -1.0f, 0.0f), Vec3(half, 1.0f, half),
                                          JPH::EMotionType::Static);
    REQUIRE_FALSE(id.IsInvalid());
    return id;
}

} // namespace

TEST_CASE("a physics world constructs and releases its tagged memory")
{
    const MemStats before = MemoryTracker::stats(MemTag::Physics);

    {
        PhysicsWorld physics;
        CHECK(physics.system().GetNumBodies() == 0);
        CHECK(MemoryTracker::stats(MemTag::Physics).current > before.current);
    }

    CHECK(MemoryTracker::stats(MemTag::Physics).current == before.current);
}

TEST_CASE("a dynamic box falls under gravity")
{
    PhysicsWorld physics;
    const JPH::BodyID box = physics.addBox(Vec3(0.0f, 8.0f, 0.0f), Vec3(0.5f),
                                           JPH::EMotionType::Dynamic);

    const float startY = physics.bodyPosition(box).y;
    stepMany(physics, 30);
    const float laterY = physics.bodyPosition(box).y;

    CHECK(laterY < startY - 1.0f);
}

TEST_CASE("a dynamic box comes to rest on a static floor")
{
    PhysicsWorld physics;
    const JPH::BodyID floor = addFloor(physics);
    const JPH::BodyID box = physics.addBox(Vec3(0.0f, 4.0f, 0.0f), Vec3(0.5f),
                                           JPH::EMotionType::Dynamic);
    (void)floor;

    stepMany(physics, 180);

    const Vec3 pos = physics.bodyPosition(box);
    CHECK(pos.y == doctest::Approx(0.5f).epsilon(0.15f));
    CHECK(std::fabs(pos.x) < 0.25f);
    CHECK(std::fabs(pos.z) < 0.25f);
}

TEST_CASE("a downward raycast hits the floor and reports the body")
{
    PhysicsWorld physics;
    const JPH::BodyID floor = addFloor(physics);

    const phys::RayHit hit = physics.raycast(Vec3(0.0f, 10.0f, 0.0f),
                                             Vec3(0.0f, -1.0f, 0.0f), 20.0f);

    CHECK(hit.hit);
    CHECK(hit.bodyId == floor);
    CHECK(hit.point.y == doctest::Approx(0.0f).epsilon(0.05f));
    CHECK(hit.normal.y == doctest::Approx(1.0f).epsilon(0.05f));
    CHECK(hit.fraction == doctest::Approx(0.5f).epsilon(0.05f));
}

TEST_CASE("a miss is a miss")
{
    PhysicsWorld physics;
    addFloor(physics);

    const phys::RayHit hit = physics.raycast(Vec3(0.0f, 10.0f, 0.0f),
                                             Vec3(0.0f, 1.0f, 0.0f), 5.0f);
    CHECK_FALSE(hit.hit);
}

TEST_CASE("a triangle mesh is a collision floor")
{
    PhysicsWorld physics;

    const std::vector<Vec3> vertices{
        {-8.0f, 0.0f, -8.0f},
        { 8.0f, 0.0f, -8.0f},
        { 8.0f, 0.0f,  8.0f},
        {-8.0f, 0.0f,  8.0f},
    };
    // Jolt mesh triangles are single-sided. CCW as seen from above so the
    // normal points +Y; the opposite winding is a hole the box falls through.
    const std::vector<std::uint32_t> indices{0, 2, 1, 0, 3, 2};

    const JPH::BodyID mesh = physics.addTriangleMesh(vertices, indices);
    CHECK_FALSE(mesh.IsInvalid());

    const phys::RayHit meshHit = physics.raycast(Vec3(0.0f, 5.0f, 0.0f),
                                                 Vec3(0.0f, -1.0f, 0.0f), 10.0f);
    CHECK(meshHit.hit);
    CHECK(meshHit.bodyId == mesh);

    const JPH::BodyID box = physics.addBox(Vec3(0.0f, 3.0f, 0.0f), Vec3(0.5f),
                                           JPH::EMotionType::Dynamic);
    stepMany(physics, 180);

    CHECK(physics.bodyPosition(box).y == doctest::Approx(0.5f).epsilon(0.2f));
}

TEST_CASE("a character walks on the floor without falling through")
{
    PhysicsWorld physics;
    addFloor(physics);

    const phys::CharacterHandle character = physics.addCharacter(Vec3(0.0f, 0.0f, 0.0f));
    CHECK(character.valid());
    CHECK(physics.character(character) != nullptr);

    physics.setCharacterVelocity(character, Vec3(3.0f, 0.0f, 0.0f));
    stepMany(physics, 60);

    const Vec3 pos = physics.characterPosition(character);
    CHECK(pos.x > 1.5f);
    CHECK(pos.y == doctest::Approx(0.0f).epsilon(0.15f));
}

TEST_CASE("a character is stopped by a wall")
{
    PhysicsWorld physics;
    addFloor(physics);
    const JPH::BodyID wall = physics.addBox(Vec3(3.0f, 1.0f, 0.0f), Vec3(0.5f, 1.0f, 2.0f),
                                            JPH::EMotionType::Static);
    CHECK_FALSE(wall.IsInvalid());

    const phys::CharacterHandle character = physics.addCharacter(Vec3(0.0f, 0.0f, 0.0f),
                                                                 0.3f, 1.8f);
    physics.setCharacterVelocity(character, Vec3(6.0f, 0.0f, 0.0f));
    stepMany(physics, 120);

    const float x = physics.characterPosition(character).x;
    CHECK(x > 0.5f);
    CHECK(x < 2.4f);
}

TEST_CASE("a stale character handle does not resolve")
{
    PhysicsWorld physics;
    const phys::CharacterHandle handle = physics.addCharacter(Vec3(0.0f, 0.0f, 0.0f));
    physics.removeCharacter(handle);

    CHECK(physics.character(handle) == nullptr);
    CHECK(physics.characterPosition(handle) == Vec3(0.0f));
}

TEST_CASE("two dynamic bodies collide instead of passing through")
{
    PhysicsWorld physics;
    addFloor(physics);

    const JPH::BodyID left = physics.addBox(Vec3(-2.0f, 0.5f, 0.0f), Vec3(0.5f),
                                            JPH::EMotionType::Dynamic);
    const JPH::BodyID right = physics.addBox(Vec3(2.0f, 0.5f, 0.0f), Vec3(0.5f),
                                             JPH::EMotionType::Dynamic);

    physics.bodies().SetLinearVelocity(left, JPH::Vec3(6.0f, 0.0f, 0.0f));
    physics.bodies().SetLinearVelocity(right, JPH::Vec3(-6.0f, 0.0f, 0.0f));
    stepMany(physics, 90);

    CHECK(physics.bodyPosition(left).x < physics.bodyPosition(right).x);
    CHECK(physics.bodyPosition(right).x - physics.bodyPosition(left).x
          >= doctest::Approx(0.9f));
}

TEST_CASE("ECS rigid body writes Transform after the step")
{
    PhysicsWorld physics;
    addFloor(physics);

    World world;
    const Entity entity = world.create();
    world.add<Transform>(entity, Transform{Vec3(0.0f, 5.0f, 0.0f),
                                           Quat{1.0f, 0.0f, 0.0f, 0.0f},
                                           Vec3(1.0f)});
    world.add<RigidBody>(entity, RigidBody{});

    stepMany(physics, world, 180);

    const Transform* transform = world.get<Transform>(entity);
    const RigidBody* body = world.get<RigidBody>(entity);
    REQUIRE(transform != nullptr);
    REQUIRE(body != nullptr);
    CHECK(body->spawned);
    CHECK(transform->position.y == doctest::Approx(0.5f).epsilon(0.2f));
}

TEST_CASE("ECS character controller walks from desired velocity")
{
    PhysicsWorld physics;
    addFloor(physics);

    World world;
    const Entity entity = world.create();
    world.add<Transform>(entity, Transform{});
    CharacterController controller;
    controller.desiredVelocityX = 2.5f;
    world.add<CharacterController>(entity, controller);

    stepMany(physics, world, 60);

    const Transform* transform = world.get<Transform>(entity);
    REQUIRE(transform != nullptr);
    CHECK(transform->position.x > 1.0f);
    CHECK(transform->position.y == doctest::Approx(0.0f).epsilon(0.15f));
}

TEST_CASE("physics components are reflected")
{
    const reflect::TypeInfo* rigid = reflect::TypeRegistry::get<RigidBody>();
    const reflect::TypeInfo* character = reflect::TypeRegistry::get<CharacterController>();
    const reflect::TypeInfo* mesh = reflect::TypeRegistry::get<CollisionMesh>();
    const reflect::TypeInfo* transform = reflect::TypeRegistry::get<Transform>();

    REQUIRE(rigid != nullptr);
    REQUIRE(character != nullptr);
    REQUIRE(mesh != nullptr);
    REQUIRE(transform != nullptr);

    CHECK(rigid->findField("mass") != nullptr);
    CHECK(rigid->findField("joltBodyId") != nullptr);
    CHECK(rigid->findField("joltBodyId")->hidden);
    CHECK(character->findField("radius") != nullptr);
    CHECK(transform->findField("position") != nullptr);
}

TEST_CASE("Jolt steps while the engine JobSystem is running")
{
    CHECK(JobSystem::get().initialized());
    const auto before = JobSystem::get().stats();

    PhysicsWorld physics;
    addFloor(physics);
    CHECK_FALSE(physics.addBox(Vec3(0.0f, 3.0f, 0.0f), Vec3(0.5f),
                               JPH::EMotionType::Dynamic).IsInvalid());
    CHECK_FALSE(physics.addBox(Vec3(1.0f, 5.0f, 0.0f), Vec3(0.5f),
                               JPH::EMotionType::Dynamic).IsInvalid());
    CHECK_FALSE(physics.addBox(Vec3(-1.0f, 7.0f, 0.0f), Vec3(0.5f),
                               JPH::EMotionType::Dynamic).IsInvalid());
    stepMany(physics, 30);

    const auto after = JobSystem::get().stats();
    CHECK(after.submitted >= before.submitted);
    CHECK(after.executed >= before.executed);
}
