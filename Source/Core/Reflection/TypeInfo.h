// Harpia Engine — runtime type description
//
// Roadmap 1.3, the most important line in the foundation. Editor inspector,
// serialisation, undo, prefabs and hot reload are all consumers of this one
// structure. Built wrong or built late, all five come out wrong together.
//
// The registry (this file) is the stable contract. The registration mechanism
// (Reflect.h macros) is deliberately swappable: it can become codegen later
// without any consumer noticing.
#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

namespace harpia::reflect {

struct TypeInfo;

// What a field is, at the granularity serialisation and the inspector care about.
enum class FieldKind : std::uint8_t {
    Bool,
    Int8, Int16, Int32, Int64,
    UInt8, UInt16, UInt32, UInt64,
    Float, Double,
    String,
    Enum,
    Struct,
    Vector,   // dynamic array; elementType describes the element
};

[[nodiscard]] const char* toString(FieldKind kind) noexcept;
[[nodiscard]] bool        isScalar(FieldKind kind) noexcept;

// Type-erased access to a dynamic array, so serialisation does not need to know
// the concrete std::vector instantiation.
struct VectorOps {
    std::size_t (*size)(const void* vec)             = nullptr;
    void        (*resize)(void* vec, std::size_t n)  = nullptr;
    void*       (*at)(void* vec, std::size_t index)  = nullptr;
    const void* (*constAt)(const void* vec, std::size_t index) = nullptr;
};

struct FieldInfo {
    const char* name = "";
    FieldKind   kind = FieldKind::Int32;

    // Resolves the field's address inside an object. Generated from a member
    // pointer, so no offsetof and no undefined behaviour.
    void*       (*get)(void* object)             = nullptr;
    const void* (*constGet)(const void* object)  = nullptr;

    std::size_t size = 0;

    // Set for Struct and for Vector-of-Struct.
    const TypeInfo* structType = nullptr;

    // Set for Vector.
    FieldKind       elementKind = FieldKind::Int32;
    std::size_t     elementSize = 0;
    VectorOps       vectorOps{};

    // Editor metadata. Costs nothing when unused and is what separates an
    // inspector from a debug dump.
    bool        hidden   = false;
    const char* tooltip  = nullptr;
    double      rangeMin = 0.0;
    double      rangeMax = 0.0;
    bool        hasRange = false;
};

// Migration hook: called after loading an object written by an older version.
using MigrateFn = void (*)(void* object, std::uint32_t fromVersion);

struct TypeInfo {
    std::string name;
    std::size_t size      = 0;
    std::size_t alignment = 0;

    // Bumped by hand whenever the field layout changes in a way old data must
    // be migrated across. Serialisation refuses silently-lossy loads without it.
    std::uint32_t version = 1;

    std::vector<FieldInfo> fields;

    void (*construct)(void* memory) = nullptr;
    void (*destruct)(void* memory)  = nullptr;

    // Needed by the ECS: adding or removing a component moves an entity between
    // archetypes, which means relocating every component it already had.
    void (*moveConstruct)(void* destination, void* source) = nullptr;
    void (*copyConstruct)(void* destination, const void* source) = nullptr;

    MigrateFn migrate = nullptr;

    [[nodiscard]] const FieldInfo* findField(std::string_view fieldName) const noexcept;
};

} // namespace harpia::reflect
