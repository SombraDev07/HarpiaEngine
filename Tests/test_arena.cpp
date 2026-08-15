#include <doctest/doctest.h>

#include "Core/Memory/Arena.h"

#include <cstdint>

using namespace harpia;

TEST_CASE("Arena allocates and tracks usage")
{
    Arena arena(1024, MemTag::Scratch);

    CHECK(arena.capacity() >= 1024);
    CHECK(arena.used() == 0);

    void* a = arena.allocate(100);
    REQUIRE(a != nullptr);
    CHECK(arena.used() >= 100);

    void* b = arena.allocate(100);
    REQUIRE(b != nullptr);
    CHECK(a != b);
}

TEST_CASE("Arena honours alignment")
{
    Arena arena(4096, MemTag::Scratch);

    // Deliberately unaligned first bump so the next allocation has to move.
    void* small = arena.allocate(1, 1);
    REQUIRE(small != nullptr);

    for (const std::size_t alignment : {std::size_t{8}, std::size_t{16},
                                        std::size_t{64}, std::size_t{256}}) {
        void* ptr = arena.allocate(32, alignment);
        REQUIRE(ptr != nullptr);
        CHECK(reinterpret_cast<std::uintptr_t>(ptr) % alignment == 0);
    }
}

TEST_CASE("Arena returns nullptr when exhausted instead of growing")
{
    Arena arena(256, MemTag::Scratch);

    void* big = arena.allocate(arena.capacity());
    REQUIRE(big != nullptr);

    CHECK(arena.allocate(1) == nullptr);
    CHECK(arena.remaining() == 0);
}

TEST_CASE("Arena rejects an allocation larger than capacity without wrapping")
{
    Arena arena(256, MemTag::Scratch);

    CHECK(arena.allocate(SIZE_MAX) == nullptr);
    CHECK(arena.allocate(SIZE_MAX - 64, 64) == nullptr);
    CHECK(arena.used() == 0);
}

TEST_CASE("Arena reset frees everything but keeps the high-water mark")
{
    Arena arena(1024, MemTag::Scratch);

    REQUIRE(arena.allocate(512) != nullptr);
    const std::size_t peak = arena.highWaterMark();
    CHECK(peak >= 512);

    arena.reset();

    CHECK(arena.used() == 0);
    CHECK(arena.highWaterMark() == peak);
    CHECK(arena.allocate(512) != nullptr);
}

TEST_CASE("Arena::Scope restores the offset and nests")
{
    Arena arena(1024, MemTag::Scratch);
    REQUIRE(arena.allocate(64) != nullptr);
    const std::size_t base = arena.used();

    {
        Arena::Scope outer(arena);
        REQUIRE(arena.allocate(128) != nullptr);
        const std::size_t afterOuter = arena.used();
        CHECK(afterOuter > base);

        {
            Arena::Scope inner(arena);
            REQUIRE(arena.allocate(256) != nullptr);
            CHECK(arena.used() > afterOuter);
        }
        CHECK(arena.used() == afterOuter);
    }
    CHECK(arena.used() == base);
}

TEST_CASE("Arena::create constructs a value")
{
    Arena arena(1024, MemTag::Scratch);

    struct Point { float x; float y; };

    Point* p = arena.create<Point>(1.5f, -2.5f);
    REQUIRE(p != nullptr);
    CHECK(p->x == doctest::Approx(1.5f));
    CHECK(p->y == doctest::Approx(-2.5f));
}

TEST_CASE("Arena::createArray zero-initialises")
{
    Arena arena(1024, MemTag::Scratch);

    std::span<int> values = arena.createArray<int>(16);
    REQUIRE(values.size() == 16);
    for (const int v : values) {
        CHECK(v == 0);
    }

    values[3] = 42;
    CHECK(values[3] == 42);
}

TEST_CASE("Arena::createArray fails cleanly when it does not fit")
{
    Arena arena(64, MemTag::Scratch);

    std::span<int> values = arena.createArray<int>(1000);
    CHECK(values.empty());
}

TEST_CASE("Arena releases its block on destruction")
{
    const MemStats before = MemoryTracker::stats(MemTag::Scratch);

    {
        Arena arena(4096, MemTag::Scratch);
        REQUIRE(arena.allocate(128) != nullptr);
        CHECK(MemoryTracker::stats(MemTag::Scratch).current > before.current);
    }

    CHECK(MemoryTracker::stats(MemTag::Scratch).current == before.current);
}

TEST_CASE("Arena is movable and leaves the source empty")
{
    Arena source(1024, MemTag::Scratch);
    REQUIRE(source.allocate(128) != nullptr);

    Arena moved(std::move(source));

    CHECK(moved.capacity() >= 1024);
    CHECK(moved.used() >= 128);
    CHECK(source.capacity() == 0);       // NOLINT(bugprone-use-after-move)
    CHECK(source.allocate(1) == nullptr);
}
