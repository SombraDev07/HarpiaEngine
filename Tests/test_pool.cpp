#include <doctest/doctest.h>

#include "Core/Memory/Pool.h"

using namespace harpia;

namespace {

int g_liveCount = 0;

struct Tracked {
    int value;

    explicit Tracked(int v = 0) : value(v) { ++g_liveCount; }
    Tracked(const Tracked& other) : value(other.value) { ++g_liveCount; }
    Tracked& operator=(const Tracked&) = default;
    ~Tracked() { --g_liveCount; }
};

} // namespace

TEST_CASE("Pool create, get and destroy")
{
    Pool<Tracked> pool(16, MemTag::Scene);
    g_liveCount = 0;

    Handle<Tracked> h = pool.create(7);
    REQUIRE(h.valid());
    CHECK(pool.size() == 1);
    CHECK(g_liveCount == 1);

    Tracked* obj = pool.get(h);
    REQUIRE(obj != nullptr);
    CHECK(obj->value == 7);

    CHECK(pool.destroy(h));
    CHECK(pool.size() == 0);
    CHECK(g_liveCount == 0);
}

TEST_CASE("a default handle is invalid")
{
    Pool<Tracked> pool(4, MemTag::Scene);
    Handle<Tracked> h;

    CHECK_FALSE(h.valid());
    CHECK(pool.get(h) == nullptr);
    CHECK_FALSE(pool.destroy(h));
}

TEST_CASE("a stale handle does not resolve after the slot is reused")
{
    Pool<Tracked> pool(4, MemTag::Scene);

    Handle<Tracked> first = pool.create(1);
    REQUIRE(first.valid());
    REQUIRE(pool.destroy(first));

    // Same slot comes back with a new generation.
    Handle<Tracked> second = pool.create(2);
    REQUIRE(second.valid());
    CHECK(second.index == first.index);
    CHECK(second.generation != first.generation);

    CHECK(pool.get(first) == nullptr);
    REQUIRE(pool.get(second) != nullptr);
    CHECK(pool.get(second)->value == 2);
}

TEST_CASE("destroying twice reports the second attempt as stale")
{
    Pool<Tracked> pool(4, MemTag::Scene);

    Handle<Tracked> h = pool.create(1);
    CHECK(pool.destroy(h));
    CHECK_FALSE(pool.destroy(h));
}

TEST_CASE("a full pool returns an invalid handle")
{
    Pool<Tracked> pool(3, MemTag::Scene);

    Handle<Tracked> a = pool.create(1);
    Handle<Tracked> b = pool.create(2);
    Handle<Tracked> c = pool.create(3);
    CHECK(a.valid());
    CHECK(b.valid());
    CHECK(c.valid());
    CHECK(pool.full());

    Handle<Tracked> overflow = pool.create(4);
    CHECK_FALSE(overflow.valid());
    CHECK(pool.size() == 3);

    // Freeing one slot makes room again.
    REQUIRE(pool.destroy(b));
    Handle<Tracked> reused = pool.create(5);
    CHECK(reused.valid());
}

TEST_CASE("Pool runs destructors on clear and on its own destruction")
{
    g_liveCount = 0;

    {
        Pool<Tracked> pool(8, MemTag::Scene);
        for (int i = 0; i < 5; ++i) {
            REQUIRE(pool.create(i).valid());
        }
        CHECK(g_liveCount == 5);

        pool.clear();
        CHECK(g_liveCount == 0);
        CHECK(pool.size() == 0);
        CHECK(pool.empty());

        for (int i = 0; i < 3; ++i) {
            REQUIRE(pool.create(i).valid());
        }
        CHECK(g_liveCount == 3);
    }

    CHECK(g_liveCount == 0);
}

TEST_CASE("Pool releases its memory on destruction")
{
    const MemStats before = MemoryTracker::stats(MemTag::Scene);

    {
        Pool<Tracked> pool(64, MemTag::Scene);
        REQUIRE(pool.create(1).valid());
        CHECK(MemoryTracker::stats(MemTag::Scene).current > before.current);
    }

    CHECK(MemoryTracker::stats(MemTag::Scene).current == before.current);
}

TEST_CASE("handles survive many create/destroy cycles")
{
    Pool<Tracked> pool(4, MemTag::Scene);
    g_liveCount = 0;

    Handle<Tracked> stale;

    for (int cycle = 0; cycle < 100; ++cycle) {
        Handle<Tracked> h = pool.create(cycle);
        REQUIRE(h.valid());
        if (cycle == 0) {
            stale = h;
        }
        if (cycle > 0) {
            CHECK(pool.get(stale) == nullptr);
        }
        REQUIRE(pool.destroy(h));
    }

    CHECK(g_liveCount == 0);
    CHECK(pool.size() == 0);
}
