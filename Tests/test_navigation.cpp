#include <doctest/doctest.h>

#include "Core/Assets/MeshAsset.h"
#include "Core/ECS/World.h"
#include "Core/Memory/MemoryTracker.h"
#include "Core/Reflection/TypeRegistry.h"
#include "Core/Threading/JobSystem.h"
#include "Navigation/BehaviorTree.h"
#include "Navigation/NavigationComponents.h"
#include "Navigation/NavigationWorld.h"
#include "Navigation/PerceptionLod.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <memory>
#include <vector>

using namespace harpia;

namespace {

void makeFloor(std::vector<Vec3>& verts, std::vector<std::uint32_t>& indices, float half = 10.0f)
{
    verts = {
        Vec3{-half, 0.0f, -half},
        Vec3{ half, 0.0f, -half},
        Vec3{ half, 0.0f,  half},
        Vec3{-half, 0.0f,  half},
    };
    // CCW from +Y so Recast sees a floor, not a ceiling.
    indices = {0, 2, 1, 0, 3, 2};
}

MeshAsset makeFloorAsset(float half = 10.0f)
{
    MeshAsset mesh;
    std::vector<Vec3>          verts;
    std::vector<std::uint32_t> indices;
    makeFloor(verts, indices, half);
    mesh.vertices.resize(verts.size());
    for (std::size_t i = 0; i < verts.size(); ++i) {
        mesh.vertices[i].position = verts[i];
        mesh.vertices[i].normal   = Vec3{0.0f, 1.0f, 0.0f};
    }
    mesh.indices = std::move(indices);
    mesh.recomputeBounds();
    return mesh;
}

} // namespace

TEST_CASE("a flat floor bakes into a walkable navmesh")
{
    std::vector<Vec3>          verts;
    std::vector<std::uint32_t> indices;
    makeFloor(verts, indices);

    NavMesh nav;
    REQUIRE(nav.bake(verts, indices));
    CHECK(nav.valid());
    CHECK(nav.polygonCount() > 0);
}

TEST_CASE("findPath crosses an open floor")
{
    std::vector<Vec3>          verts;
    std::vector<std::uint32_t> indices;
    makeFloor(verts, indices);

    NavMesh nav;
    REQUIRE(nav.bake(verts, indices));

    std::vector<Vec3> path;
    REQUIRE(nav.findPath(Vec3{-8.0f, 0.1f, -8.0f}, Vec3{8.0f, 0.1f, 8.0f}, path));
    REQUIRE(path.size() >= 2);

    const Vec3& end = path.back();
    CHECK(std::abs(end.x - 8.0f) < 1.5f);
    CHECK(std::abs(end.z - 8.0f) < 1.5f);
}

TEST_CASE("a point off the mesh has no path")
{
    std::vector<Vec3>          verts;
    std::vector<std::uint32_t> indices;
    makeFloor(verts, indices, 4.0f);

    NavMesh nav;
    REQUIRE(nav.bake(verts, indices));

    std::vector<Vec3> path;
    CHECK_FALSE(nav.findPath(Vec3{0.0f, 0.1f, 0.0f}, Vec3{80.0f, 0.1f, 80.0f}, path));
    CHECK(path.empty());
}

TEST_CASE("MeshAsset geometry is read, not owned")
{
    const MeshAsset mesh = makeFloorAsset();
    REQUIRE_FALSE(mesh.empty());

    NavMesh nav;
    REQUIRE(nav.bake(mesh));
    CHECK(nav.polygonCount() > 0);

    std::vector<Vec3> path;
    CHECK(nav.findPath(Vec3{-6.0f, 0.1f, 0.0f}, Vec3{6.0f, 0.1f, 0.0f}, path));
}

TEST_CASE("bake runs through the JobSystem")
{
    JobSystem& jobs = JobSystem::get();
    REQUIRE(jobs.initialized());
    const auto before = jobs.stats();

    std::vector<Vec3>          verts;
    std::vector<std::uint32_t> indices;
    makeFloor(verts, indices);

    NavMesh nav;
    REQUIRE(nav.bake(verts, indices));

    const auto after = jobs.stats();
    CHECK(after.submitted > before.submitted);
    CHECK(after.executed >= before.executed);
}

TEST_CASE("Recast allocations carry MemTag::Scene")
{
    const std::size_t before = MemoryTracker::stats(MemTag::Scene).current;

    std::vector<Vec3>          verts;
    std::vector<std::uint32_t> indices;
    makeFloor(verts, indices);

    {
        NavMesh nav;
        REQUIRE(nav.bake(verts, indices));
        CHECK(MemoryTracker::stats(MemTag::Scene).current > before);
    }

    CHECK(MemoryTracker::stats(MemTag::Scene).current == before);
}

TEST_CASE("a crowd agent walks toward its target")
{
    NavigationWorld world;
    std::vector<Vec3>          verts;
    std::vector<std::uint32_t> indices;
    makeFloor(verts, indices);
    REQUIRE(world.bake(verts, indices));

    CrowdAgentParams params;
    params.radius   = 0.3f;
    params.maxSpeed = 3.5f;

    const int agent = world.crowd().addAgent(Vec3{-6.0f, 0.0f, 0.0f}, params);
    REQUIRE(agent != Crowd::kInvalid);
    REQUIRE(world.crowd().requestMove(agent, Vec3{6.0f, 0.0f, 0.0f}));

    for (int i = 0; i < 240; ++i) {
        world.update(1.0f / 60.0f);
    }

    const Vec3 pos = world.crowd().position(agent);
    CHECK(pos.x > 2.0f);
}

TEST_CASE("two agents do not occupy the same point")
{
    NavigationWorld world;
    std::vector<Vec3>          verts;
    std::vector<std::uint32_t> indices;
    makeFloor(verts, indices);
    REQUIRE(world.bake(verts, indices));

    CrowdAgentParams params;
    params.radius      = 0.4f;
    params.maxSpeed    = 3.0f;
    params.separation  = 3.0f;

    const int a = world.crowd().addAgent(Vec3{-5.0f, 0.0f, 0.0f}, params);
    const int b = world.crowd().addAgent(Vec3{ 5.0f, 0.0f, 0.0f}, params);
    REQUIRE(a != Crowd::kInvalid);
    REQUIRE(b != Crowd::kInvalid);
    REQUIRE(world.crowd().requestMove(a, Vec3{ 5.0f, 0.0f, 0.0f}));
    REQUIRE(world.crowd().requestMove(b, Vec3{-5.0f, 0.0f, 0.0f}));

    float minDist = 1.0e9f;
    for (int i = 0; i < 240; ++i) {
        world.update(1.0f / 60.0f);
        minDist = std::min(minDist, distance(world.crowd().position(a), world.crowd().position(b)));
    }

    CHECK(minDist > 0.55f);
    CHECK(world.crowd().count() == 2);
}

TEST_CASE("sequence succeeds only when every child does")
{
    auto seq = std::make_unique<bt::Sequence>();
    seq->add(std::make_unique<bt::Condition>([](const bt::Blackboard& b) {
        return b.getInt("ready") != 0;
    }));
    seq->add(std::make_unique<bt::Action>([](bt::Blackboard&, float) {
        return bt::Status::Success;
    }));

    bt::Blackboard board;
    CHECK(seq->tick(board, 0.0f) == bt::Status::Failure);

    board.setInt("ready", 1);
    CHECK(seq->tick(board, 0.0f) == bt::Status::Success);
}

TEST_CASE("selector falls through to the first success")
{
    auto sel = std::make_unique<bt::Selector>();
    sel->add(std::make_unique<bt::Condition>([](const bt::Blackboard& b) {
        return b.getInt("a") != 0;
    }));
    sel->add(std::make_unique<bt::Condition>([](const bt::Blackboard& b) {
        return b.getInt("b") != 0;
    }));

    bt::Blackboard board;
    CHECK(sel->tick(board, 0.0f) == bt::Status::Failure);

    board.setInt("b", 1);
    CHECK(sel->tick(board, 0.0f) == bt::Status::Success);
}

TEST_CASE("inverter flips a leaf and a running child stays running")
{
    int runs = 0;
    auto inv = std::make_unique<bt::Inverter>(std::make_unique<bt::Action>([&runs](bt::Blackboard&, float) {
        ++runs;
        return runs < 2 ? bt::Status::Running : bt::Status::Success;
    }));

    bt::Blackboard board;
    CHECK(inv->tick(board, 0.016f) == bt::Status::Running);
    CHECK(inv->tick(board, 0.016f) == bt::Status::Failure);
}

TEST_CASE("perception LOD thinks less the farther the agent is")
{
    PerceptionLod lod;
    lod.fullRadius    = 10.0f;
    lod.reducedRadius = 40.0f;
    lod.reducedEvery  = 4;
    lod.sleepEvery    = 16;

    CHECK(lod.classify(0.0f) == PerceptionTier::Full);
    CHECK(lod.classify(25.0f) == PerceptionTier::Reduced);
    CHECK(lod.classify(80.0f) == PerceptionTier::Sleep);

    CHECK(lod.shouldThink(PerceptionTier::Full, 7));
    CHECK_FALSE(lod.shouldThink(PerceptionTier::Reduced, 1));
    CHECK(lod.shouldThink(PerceptionTier::Reduced, 4));
    CHECK_FALSE(lod.shouldThink(PerceptionTier::Sleep, 1));
    CHECK(lod.shouldThink(PerceptionTier::Sleep, 16));

    Perception perception;
    perception.lod = lod;
    perception.observe(80.0f);
    CHECK(perception.tier == PerceptionTier::Sleep);
    perception.tick = 1;
    CHECK_FALSE(perception.shouldThink());
}

TEST_CASE("NavAgent and Perception live on the World")
{
    ecs::World world;
    const ecs::Entity e = world.create();
    world.add(e, NavAgent{});
    world.add(e, Perception{});

    NavAgent* agent = world.get<NavAgent>(e);
    REQUIRE(agent != nullptr);
    agent->destination = Vec3{1.0f, 0.0f, 2.0f};
    CHECK(agent->crowdIndex == Crowd::kInvalid);

    Perception* perception = world.get<Perception>(e);
    REQUIRE(perception != nullptr);
    perception->observe(5.0f);
    CHECK(perception->tier == PerceptionTier::Full);

    CHECK(reflect::TypeRegistry::find("harpia::NavAgent") != nullptr);
    CHECK(reflect::TypeRegistry::find("harpia::Perception") != nullptr);
}
