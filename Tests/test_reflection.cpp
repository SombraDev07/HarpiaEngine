#include <doctest/doctest.h>

#include "Core/Reflection/Reflect.h"

#include <string>
#include <vector>

// --- types under test ------------------------------------------------------

namespace harpia_test {

struct Vec3 {
    float x = 0.0f;
    float y = 0.0f;
    float z = 0.0f;
};

enum class Visibility : std::uint8_t { Hidden = 0, Visible = 1, Ghost = 2 };

struct Transform {
    Vec3 position;
    Vec3 scale{1.0f, 1.0f, 1.0f};
    float uniform = 1.0f;
};

struct Entity {
    std::string        name;
    Transform          transform;
    std::vector<int>   tags;
    std::vector<Vec3>  points;
    Visibility         visibility = Visibility::Visible;
    bool               enabled    = true;
    std::uint64_t      guid       = 0;
    int                internalId = -1;
};

} // namespace harpia_test

HARPIA_REFLECT_BEGIN(harpia_test::Vec3, 1)
    HARPIA_FIELD(x)
    HARPIA_FIELD(y)
    HARPIA_FIELD(z)
HARPIA_REFLECT_END(harpia_test::Vec3)

HARPIA_REFLECT_BEGIN(harpia_test::Transform, 1)
    HARPIA_FIELD(position)
    HARPIA_FIELD_RANGE(scale, 0.01, 100.0)
    HARPIA_FIELD_TOOLTIP(uniform, "Uniform scale multiplier")
HARPIA_REFLECT_END(harpia_test::Transform)

HARPIA_REFLECT_BEGIN(harpia_test::Entity, 1)
    HARPIA_FIELD(name)
    HARPIA_FIELD(transform)
    HARPIA_FIELD(tags)
    HARPIA_FIELD(points)
    HARPIA_FIELD(visibility)
    HARPIA_FIELD(enabled)
    HARPIA_FIELD(guid)
    HARPIA_FIELD_HIDDEN(internalId)
HARPIA_REFLECT_END(harpia_test::Entity)

// --- tests -----------------------------------------------------------------

using namespace harpia;
using namespace harpia_test;

TEST_CASE("a reflected type reports its name, size and version")
{
    const reflect::TypeInfo* info = reflect::TypeRegistry::get<Vec3>();
    REQUIRE(info != nullptr);

    CHECK(info->name == "harpia_test::Vec3");
    CHECK(info->size == sizeof(Vec3));
    CHECK(info->alignment == alignof(Vec3));
    CHECK(info->version == 1);
    CHECK(info->fields.size() == 3);
}

TEST_CASE("types register themselves before anyone asks")
{
    // AutoRegister runs at static-init, so find() works without the type
    // having been touched. Deserialising a scene depends on this.
    CHECK(reflect::TypeRegistry::find("harpia_test::Entity") != nullptr);
    CHECK(reflect::TypeRegistry::find("harpia_test::Transform") != nullptr);
    CHECK(reflect::TypeRegistry::find("does::Not::Exist") == nullptr);
}

TEST_CASE("fields expose kind and order")
{
    const reflect::TypeInfo* info = reflect::TypeRegistry::get<Entity>();
    REQUIRE(info->fields.size() == 8);

    CHECK(info->fields[0].kind == reflect::FieldKind::String);
    CHECK(info->fields[1].kind == reflect::FieldKind::Struct);
    CHECK(info->fields[2].kind == reflect::FieldKind::Vector);
    CHECK(info->fields[3].kind == reflect::FieldKind::Vector);
    CHECK(info->fields[4].kind == reflect::FieldKind::Enum);
    CHECK(info->fields[5].kind == reflect::FieldKind::Bool);
    CHECK(info->fields[6].kind == reflect::FieldKind::UInt64);
    CHECK(info->fields[7].kind == reflect::FieldKind::Int32);
}

TEST_CASE("a nested struct field points at its own type")
{
    const reflect::TypeInfo* entity = reflect::TypeRegistry::get<Entity>();
    const reflect::FieldInfo* field = entity->findField("transform");
    REQUIRE(field != nullptr);
    REQUIRE(field->structType != nullptr);

    CHECK(field->structType->name == "harpia_test::Transform");
    CHECK(field->structType->fields.size() == 3);
}

TEST_CASE("a vector of structs carries element type and ops")
{
    const reflect::TypeInfo*  entity = reflect::TypeRegistry::get<Entity>();
    const reflect::FieldInfo* field  = entity->findField("points");
    REQUIRE(field != nullptr);

    CHECK(field->kind == reflect::FieldKind::Vector);
    CHECK(field->elementKind == reflect::FieldKind::Struct);
    CHECK(field->elementSize == sizeof(Vec3));
    REQUIRE(field->structType != nullptr);
    CHECK(field->structType->name == "harpia_test::Vec3");

    REQUIRE(field->vectorOps.size != nullptr);
    REQUIRE(field->vectorOps.resize != nullptr);
    REQUIRE(field->vectorOps.at != nullptr);
}

TEST_CASE("fields can be read and written through reflection alone")
{
    Entity entity;
    entity.name = "player";
    entity.transform.position = Vec3{1.0f, 2.0f, 3.0f};

    const reflect::TypeInfo* info = reflect::TypeRegistry::get<Entity>();

    // Read
    const reflect::FieldInfo* nameField = info->findField("name");
    REQUIRE(nameField != nullptr);
    CHECK(*static_cast<const std::string*>(nameField->constGet(&entity)) == "player");

    // Write — this is the path the editor inspector and undo both take.
    *static_cast<std::string*>(nameField->get(&entity)) = "enemy";
    CHECK(entity.name == "enemy");

    // Reach into a nested struct
    const reflect::FieldInfo* transformField = info->findField("transform");
    void* transform = transformField->get(&entity);
    const reflect::FieldInfo* positionField = transformField->structType->findField("position");
    void* position = positionField->get(transform);
    const reflect::FieldInfo* xField = positionField->structType->findField("x");
    *static_cast<float*>(xField->get(position)) = 42.0f;

    CHECK(entity.transform.position.x == doctest::Approx(42.0f));
}

TEST_CASE("vector elements are reachable through the type-erased ops")
{
    Entity entity;
    entity.points = {Vec3{1, 2, 3}, Vec3{4, 5, 6}};

    const reflect::TypeInfo*  info  = reflect::TypeRegistry::get<Entity>();
    const reflect::FieldInfo* field = info->findField("points");
    void* vec = field->get(&entity);

    REQUIRE(field->vectorOps.size(vec) == 2);

    auto* second = static_cast<Vec3*>(field->vectorOps.at(vec, 1));
    CHECK(second->y == doctest::Approx(5.0f));

    field->vectorOps.resize(vec, 5);
    CHECK(entity.points.size() == 5);
}

TEST_CASE("editor metadata survives to the field")
{
    const reflect::TypeInfo* transform = reflect::TypeRegistry::get<Transform>();

    const reflect::FieldInfo* scale = transform->findField("scale");
    REQUIRE(scale != nullptr);
    CHECK(scale->hasRange);
    CHECK(scale->rangeMin == doctest::Approx(0.01));
    CHECK(scale->rangeMax == doctest::Approx(100.0));

    const reflect::FieldInfo* uniform = transform->findField("uniform");
    REQUIRE(uniform != nullptr);
    REQUIRE(uniform->tooltip != nullptr);
    CHECK(std::string{uniform->tooltip} == "Uniform scale multiplier");

    const reflect::TypeInfo*  entity     = reflect::TypeRegistry::get<Entity>();
    const reflect::FieldInfo* internalId = entity->findField("internalId");
    REQUIRE(internalId != nullptr);
    CHECK(internalId->hidden);
}

TEST_CASE("construct and destruct hooks work on raw memory")
{
    const reflect::TypeInfo* info = reflect::TypeRegistry::get<Entity>();
    REQUIRE(info->construct != nullptr);
    REQUIRE(info->destruct != nullptr);

    alignas(Entity) std::byte storage[sizeof(Entity)];
    info->construct(storage);

    auto* entity = reinterpret_cast<Entity*>(storage);
    CHECK(entity->enabled);
    CHECK(entity->internalId == -1);
    entity->name = "built through reflection";

    info->destruct(storage);
}

TEST_CASE("findField misses cleanly")
{
    const reflect::TypeInfo* info = reflect::TypeRegistry::get<Vec3>();
    CHECK(info->findField("w") == nullptr);
    CHECK(info->findField("") == nullptr);
}
