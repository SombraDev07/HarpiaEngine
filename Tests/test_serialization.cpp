// The point of this file is schema evolution. Round-tripping is the easy half;
// what matters is that data written by an older build still loads.

#include <doctest/doctest.h>

#include "Core/Reflection/Reflect.h"
#include "Core/Serialization/Serializer.h"

#include <string>
#include <vector>

namespace harpia_test {

struct Point {
    float x = 0.0f;
    float y = 0.0f;
};

// Current shape of the type. An earlier build shipped it without `mana`, and
// with `health` meaning something on a 0..100 scale rather than 0..1.
struct SaveGame {
    std::string        playerName;
    float              health = 1.0f;
    float              mana   = 1.0f;
    std::vector<Point> waypoints;
    std::uint32_t      level = 1;
};

// Migration from any version < 2: health used to be stored 0..100.
inline void migrateSaveGame(void* object, std::uint32_t fromVersion)
{
    auto* save = static_cast<SaveGame*>(object);
    if (fromVersion < 2) {
        save->health = save->health / 100.0f;
    }
}

} // namespace harpia_test

HARPIA_REFLECT_BEGIN(harpia_test::Point, 1)
    HARPIA_FIELD(x)
    HARPIA_FIELD(y)
HARPIA_REFLECT_END(harpia_test::Point)

HARPIA_REFLECT_BEGIN(harpia_test::SaveGame, 2)
    HARPIA_FIELD(playerName)
    HARPIA_FIELD(health)
    HARPIA_FIELD(mana)
    HARPIA_FIELD(waypoints)
    HARPIA_FIELD(level)
    HARPIA_MIGRATE(&harpia_test::migrateSaveGame)
HARPIA_REFLECT_END(harpia_test::SaveGame)

using namespace harpia;
using namespace harpia_test;

namespace {

// Builds an unregistered TypeInfo describing an older shape of SaveGame:
// same name, version 1, no `mana`, no `level`.
reflect::TypeInfo makeVersion1Type()
{
    reflect::TypeInfoBuilder builder("harpia_test::SaveGame",
                                     sizeof(SaveGame), alignof(SaveGame), 1);
    builder.constructors<SaveGame>();
    builder.field<&SaveGame::playerName>("playerName");
    builder.field<&SaveGame::health>("health");
    builder.field<&SaveGame::waypoints>("waypoints");
    return builder.build();
}

// An imagined future shape that carries a field this build does not know.
reflect::TypeInfo makeVersion3Type()
{
    reflect::TypeInfoBuilder builder("harpia_test::SaveGame",
                                     sizeof(SaveGame), alignof(SaveGame), 3);
    builder.constructors<SaveGame>();
    builder.field<&SaveGame::playerName>("playerName");
    builder.field<&SaveGame::health>("health");
    builder.field<&SaveGame::mana>("mana");
    builder.field<&SaveGame::waypoints>("waypoints");
    builder.field<&SaveGame::level>("level");
    // Stands in for a field added after this build shipped.
    builder.field<&SaveGame::level>("prestige");
    return builder.build();
}

} // namespace

TEST_CASE("round trip preserves every field")
{
    SaveGame original;
    original.playerName = "Bruno";
    original.health     = 0.75f;
    original.mana       = 0.5f;
    original.level      = 42;
    original.waypoints  = {Point{1.0f, 2.0f}, Point{3.0f, 4.0f}, Point{-5.0f, 6.5f}};

    const std::vector<std::uint8_t> bytes = serial::saveToBytes(original);
    CHECK(bytes.size() > 0);

    SaveGame loaded;
    const serial::LoadResult result = serial::loadFromBytes(loaded, bytes);

    REQUIRE(result);
    CHECK(result.sourceVersion == 2);
    CHECK(result.skippedFields == 0);
    CHECK(result.defaultedFields == 0);
    CHECK_FALSE(result.migrated);

    CHECK(loaded.playerName == "Bruno");
    CHECK(loaded.health == doctest::Approx(0.75f));
    CHECK(loaded.mana == doctest::Approx(0.5f));
    CHECK(loaded.level == 42);
    REQUIRE(loaded.waypoints.size() == 3);
    CHECK(loaded.waypoints[2].x == doctest::Approx(-5.0f));
    CHECK(loaded.waypoints[2].y == doctest::Approx(6.5f));
}

TEST_CASE("an empty vector and an empty string round trip")
{
    SaveGame original;
    original.playerName = "";
    original.waypoints.clear();

    const std::vector<std::uint8_t> bytes = serial::saveToBytes(original);

    SaveGame loaded;
    loaded.playerName = "should be cleared";
    loaded.waypoints  = {Point{9.0f, 9.0f}};

    REQUIRE(serial::loadFromBytes(loaded, bytes));
    CHECK(loaded.playerName.empty());
    CHECK(loaded.waypoints.empty());
}

TEST_CASE("data from an older version loads, defaults new fields and migrates")
{
    // Write as version 1: health on the old 0..100 scale, no mana, no level.
    const reflect::TypeInfo v1 = makeVersion1Type();

    SaveGame old;
    old.playerName = "veteran";
    old.health     = 80.0f; // old scale
    old.waypoints  = {Point{7.0f, 8.0f}};

    const std::vector<std::uint8_t> bytes = serial::saveToBytes(v1, &old);

    // Read with the current version 2 type.
    SaveGame loaded;
    loaded.mana  = 0.25f; // default that must survive
    loaded.level = 7;

    const serial::LoadResult result =
        serial::loadFromBytes(reflect::TypeOf<SaveGame>::info(), &loaded, bytes);

    REQUIRE(result);
    CHECK(result.sourceVersion == 1);
    CHECK(result.migrated);

    // mana and level were absent from the data and kept their values.
    CHECK(result.defaultedFields == 2);
    CHECK(loaded.mana == doctest::Approx(0.25f));
    CHECK(loaded.level == 7);

    CHECK(loaded.playerName == "veteran");
    REQUIRE(loaded.waypoints.size() == 1);
    CHECK(loaded.waypoints[0].x == doctest::Approx(7.0f));

    // The migration hook rescaled health rather than loading a wrong value.
    CHECK(loaded.health == doctest::Approx(0.8f));
}

TEST_CASE("a field this build does not know is skipped, not fatal")
{
    const reflect::TypeInfo v3 = makeVersion3Type();

    SaveGame future;
    future.playerName = "from the future";
    future.health     = 0.9f;
    future.mana       = 0.4f;
    future.level      = 99;

    const std::vector<std::uint8_t> bytes = serial::saveToBytes(v3, &future);

    SaveGame loaded;
    const serial::LoadResult result =
        serial::loadFromBytes(reflect::TypeOf<SaveGame>::info(), &loaded, bytes);

    REQUIRE(result);
    CHECK(result.sourceVersion == 3);
    CHECK(result.skippedFields == 1); // "prestige"

    // Everything the build does understand still arrived intact.
    CHECK(loaded.playerName == "from the future");
    CHECK(loaded.health == doctest::Approx(0.9f));
    CHECK(loaded.mana == doctest::Approx(0.4f));
    CHECK(loaded.level == 99);
}

TEST_CASE("corrupt input is rejected rather than half-applied")
{
    SaveGame original;
    original.playerName = "Bruno";
    const std::vector<std::uint8_t> good = serial::saveToBytes(original);

    SUBCASE("bad magic")
    {
        std::vector<std::uint8_t> bad = good;
        bad[0] ^= 0xFF;

        SaveGame loaded;
        const serial::LoadResult result = serial::loadFromBytes(loaded, bad);
        CHECK_FALSE(result);
        CHECK(result.status == serial::LoadStatus::BadMagic);
    }

    SUBCASE("truncated")
    {
        std::vector<std::uint8_t> bad(
            good.begin(),
            good.begin() + static_cast<std::ptrdiff_t>(good.size() / 2));

        SaveGame loaded;
        const serial::LoadResult result = serial::loadFromBytes(loaded, bad);
        CHECK_FALSE(result);
        CHECK(result.status == serial::LoadStatus::Truncated);
    }

    SUBCASE("empty")
    {
        SaveGame loaded;
        const serial::LoadResult result = serial::loadFromBytes(loaded, {});
        CHECK_FALSE(result);
        CHECK(result.status == serial::LoadStatus::Truncated);
    }

    SUBCASE("wrong type name")
    {
        Point point;
        const serial::LoadResult result =
            serial::loadFromBytes(reflect::TypeOf<Point>::info(), &point, good);
        CHECK_FALSE(result);
        CHECK(result.status == serial::LoadStatus::TypeMismatch);
    }
}

TEST_CASE("a large vector survives the round trip")
{
    SaveGame original;
    original.waypoints.reserve(10000);
    for (int i = 0; i < 10000; ++i) {
        original.waypoints.push_back(Point{static_cast<float>(i),
                                           static_cast<float>(-i)});
    }

    const std::vector<std::uint8_t> bytes = serial::saveToBytes(original);

    SaveGame loaded;
    REQUIRE(serial::loadFromBytes(loaded, bytes));
    REQUIRE(loaded.waypoints.size() == 10000);
    CHECK(loaded.waypoints[9999].x == doctest::Approx(9999.0f));
    CHECK(loaded.waypoints[9999].y == doctest::Approx(-9999.0f));
}
