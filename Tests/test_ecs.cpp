#include <doctest/doctest.h>

#include "Core/ECS/World.h"
#include "Core/Reflection/Reflect.h"

#include <algorithm>
#include <atomic>
#include <string>
#include <vector>

namespace harpia_ecs_test {

struct Position { float x = 0.0f; float y = 0.0f; float z = 0.0f; };
struct Velocity { float x = 0.0f; float y = 0.0f; float z = 0.0f; };
struct Health   { int current = 100; int maximum = 100; };
struct Tag      { std::string name; };   // non-trivial: exercises move on transition

} // namespace harpia_ecs_test

HARPIA_REFLECT_BEGIN(harpia_ecs_test::Position, 1)
    HARPIA_FIELD(x) HARPIA_FIELD(y) HARPIA_FIELD(z)
HARPIA_REFLECT_END(harpia_ecs_test::Position)

HARPIA_REFLECT_BEGIN(harpia_ecs_test::Velocity, 1)
    HARPIA_FIELD(x) HARPIA_FIELD(y) HARPIA_FIELD(z)
HARPIA_REFLECT_END(harpia_ecs_test::Velocity)

HARPIA_REFLECT_BEGIN(harpia_ecs_test::Health, 1)
    HARPIA_FIELD(current) HARPIA_FIELD(maximum)
HARPIA_REFLECT_END(harpia_ecs_test::Health)

HARPIA_REFLECT_BEGIN(harpia_ecs_test::Tag, 1)
    HARPIA_FIELD(name)
HARPIA_REFLECT_END(harpia_ecs_test::Tag)

using namespace harpia;
using namespace harpia::ecs;
using namespace harpia_ecs_test;

TEST_CASE("entities are created and destroyed")
{
    World world;
    CHECK(world.size() == 0);

    const Entity entity = world.create();
    CHECK(entity.valid());
    CHECK(world.alive(entity));
    CHECK(world.size() == 1);

    world.destroy(entity);
    CHECK_FALSE(world.alive(entity));
    CHECK(world.size() == 0);
}

TEST_CASE("a stale entity handle does not resolve after the slot is reused")
{
    World world;

    const Entity first = world.create();
    world.destroy(first);

    const Entity second = world.create();
    CHECK(second.index == first.index);
    CHECK(second.generation != first.generation);

    CHECK_FALSE(world.alive(first));
    CHECK(world.alive(second));

    // Operations on the stale handle are no-ops, not corruption.
    world.add<Position>(first, Position{1, 2, 3});
    CHECK(world.get<Position>(first) == nullptr);
    CHECK_FALSE(world.has<Position>(second));
}

TEST_CASE("a default Entity is invalid everywhere")
{
    World world;
    Entity none;

    CHECK_FALSE(none.valid());
    CHECK_FALSE(world.alive(none));
    CHECK(world.get<Position>(none) == nullptr);
    CHECK_FALSE(world.has<Position>(none));
    world.destroy(none); // must not crash
}

TEST_CASE("components are added, read, written and removed")
{
    World world;
    const Entity entity = world.create();

    CHECK_FALSE(world.has<Position>(entity));

    world.add<Position>(entity, Position{1.0f, 2.0f, 3.0f});
    REQUIRE(world.has<Position>(entity));

    Position* position = world.get<Position>(entity);
    REQUIRE(position != nullptr);
    CHECK(position->y == doctest::Approx(2.0f));

    position->y = 42.0f;
    CHECK(world.get<Position>(entity)->y == doctest::Approx(42.0f));

    world.remove<Position>(entity);
    CHECK_FALSE(world.has<Position>(entity));
    CHECK(world.get<Position>(entity) == nullptr);
}

TEST_CASE("an archetype transition preserves the components already held")
{
    World world;
    const Entity entity = world.create();

    world.add<Position>(entity, Position{1.0f, 2.0f, 3.0f});
    world.add<Health>(entity, Health{55, 200});
    world.add<Tag>(entity, Tag{"boss"});

    // Adding a fourth component moves the entity to a new archetype; the other
    // three must arrive intact, including the one that owns heap memory.
    world.add<Velocity>(entity, Velocity{-1.0f, 0.0f, 1.0f});

    REQUIRE(world.get<Position>(entity) != nullptr);
    CHECK(world.get<Position>(entity)->x == doctest::Approx(1.0f));
    CHECK(world.get<Health>(entity)->current == 55);
    CHECK(world.get<Health>(entity)->maximum == 200);
    CHECK(world.get<Tag>(entity)->name == "boss");
    CHECK(world.get<Velocity>(entity)->z == doctest::Approx(1.0f));

    // Removing one leaves the rest alone.
    world.remove<Health>(entity);
    CHECK_FALSE(world.has<Health>(entity));
    CHECK(world.get<Tag>(entity)->name == "boss");
    CHECK(world.get<Position>(entity)->x == doctest::Approx(1.0f));
}

TEST_CASE("create with components in one call")
{
    World world;
    const Entity entity = world.create(Position{7.0f, 8.0f, 9.0f}, Health{10, 20});

    REQUIRE(world.has<Position>(entity));
    REQUIRE(world.has<Health>(entity));
    CHECK(world.get<Position>(entity)->z == doctest::Approx(9.0f));
    CHECK(world.get<Health>(entity)->current == 10);
    CHECK(world.archetypeCount() >= 1);
}

TEST_CASE("a query visits exactly the entities that match")
{
    World world;

    const Entity both     = world.create(Position{1, 0, 0}, Velocity{1, 0, 0});
    const Entity onlyPos  = world.create(Position{2, 0, 0});
    const Entity onlyVel  = world.create(Velocity{3, 0, 0});
    (void)onlyVel;

    std::vector<Entity> visited;
    world.each<Position, Velocity>([&](Entity e, Position&, Velocity&) {
        visited.push_back(e);
    });

    REQUIRE(visited.size() == 1);
    CHECK(visited[0] == both);

    visited.clear();
    world.each<Position>([&](Entity e, Position&) { visited.push_back(e); });

    REQUIRE(visited.size() == 2);
    CHECK(std::find(visited.begin(), visited.end(), both) != visited.end());
    CHECK(std::find(visited.begin(), visited.end(), onlyPos) != visited.end());
}

TEST_CASE("a query writes through to component storage")
{
    World world;
    for (int i = 0; i < 100; ++i) {
        (void)world.create(Position{0, 0, 0},
                           Velocity{static_cast<float>(i), 0.0f, 0.0f});
    }

    world.each<Position, Velocity>([](Entity, Position& p, Velocity& v) {
        p.x += v.x;
    });

    int checked = 0;
    world.each<Position, Velocity>([&](Entity, Position& p, Velocity& v) {
        CHECK(p.x == doctest::Approx(v.x));
        ++checked;
    });
    CHECK(checked == 100);
}

TEST_CASE("10k entities across chunks, queried in parallel")
{
    World world;

    constexpr int kCount = 10000;
    for (int i = 0; i < kCount; ++i) {
        (void)world.create(Position{static_cast<float>(i), 0.0f, 0.0f},
                           Velocity{1.0f, 2.0f, 3.0f});
    }

    CHECK(world.size() == kCount);
    // 16 KB chunks cannot hold 10k entities, so this must have spilled.
    CHECK(world.chunkCount() > 1);

    std::atomic<int> visited{0};
    world.parallelEach<Position, Velocity>([&](Entity, Position& p, Velocity& v) {
        p.x += v.x;
        visited.fetch_add(1, std::memory_order_relaxed);
    });
    CHECK(visited.load() == kCount);

    // Every entity got exactly one increment: value equals its original index+1.
    int index = 0;
    std::vector<float> seen;
    seen.reserve(kCount);
    world.each<Position>([&](Entity, Position& p) { seen.push_back(p.x); ++index; });

    REQUIRE(seen.size() == kCount);
    std::sort(seen.begin(), seen.end());
    for (int i = 0; i < kCount; ++i) {
        CHECK(seen[static_cast<std::size_t>(i)] == doctest::Approx(static_cast<float>(i) + 1.0f));
    }
}

TEST_CASE("destroying from the middle keeps storage packed")
{
    World world;

    std::vector<Entity> entities;
    for (int i = 0; i < 500; ++i) {
        entities.push_back(world.create(Position{static_cast<float>(i), 0, 0}));
    }

    // Destroy every other entity: exercises the swap-remove path heavily.
    for (std::size_t i = 0; i < entities.size(); i += 2) {
        world.destroy(entities[i]);
    }

    CHECK(world.size() == 250);

    int visited = 0;
    world.each<Position>([&](Entity e, Position& p) {
        CHECK(world.alive(e));
        // Only odd indices survived.
        const int value = static_cast<int>(p.x);
        CHECK(value % 2 == 1);
        ++visited;
    });
    CHECK(visited == 250);

    // Survivors still resolve through their handles after all that shuffling.
    for (std::size_t i = 1; i < entities.size(); i += 2) {
        REQUIRE(world.alive(entities[i]));
        CHECK(world.get<Position>(entities[i])->x
              == doctest::Approx(static_cast<float>(i)));
    }
}

TEST_CASE("a non-trivial component survives many transitions")
{
    World world;
    const Entity entity = world.create();

    world.add<Tag>(entity, Tag{"a string long enough to be heap allocated, definitely"});

    for (int i = 0; i < 20; ++i) {
        world.add<Position>(entity, Position{static_cast<float>(i), 0, 0});
        world.remove<Position>(entity);
    }

    REQUIRE(world.get<Tag>(entity) != nullptr);
    CHECK(world.get<Tag>(entity)->name
          == "a string long enough to be heap allocated, definitely");
}

TEST_CASE("clear destroys everything")
{
    World world;
    std::vector<Entity> entities;
    for (int i = 0; i < 100; ++i) {
        entities.push_back(world.create(Position{}, Tag{"x"}));
    }

    world.clear();
    CHECK(world.size() == 0);
    for (const Entity entity : entities) {
        CHECK_FALSE(world.alive(entity));
    }

    int visited = 0;
    world.each<Position>([&](Entity, Position&) { ++visited; });
    CHECK(visited == 0);
}

TEST_CASE("component ids are stable and reflection is reachable from them")
{
    const ComponentId position = ComponentRegistry::id<Position>();
    CHECK(ComponentRegistry::id<Position>() == position);
    CHECK(ComponentRegistry::id<Velocity>() != position);

    const reflect::TypeInfo* type = ComponentRegistry::typeInfo(position);
    REQUIRE(type != nullptr);
    CHECK(type->name == "harpia_ecs_test::Position");
    CHECK(type->fields.size() == 3);

    // Name lookup is what a scene file will use; numeric ids are per-process.
    CHECK(ComponentRegistry::findByName("harpia_ecs_test::Position") == position);
    CHECK(ComponentRegistry::findByName("nope") == kInvalidComponent);
}
