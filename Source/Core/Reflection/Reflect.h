// Harpia Engine — reflection registration
//
// Macro-based on purpose. libclang codegen (the road TucanoEngine took) buys
// zero boilerplate and costs a build dependency, a generation step and a parser
// to maintain. One line per field is the better trade for this team size.
//
// C++26 static reflection (P2996) would remove the macro entirely, but compiler
// support is still experimental — the registry contract below is what lets us
// switch later without touching a single consumer.
//
// Usage, at global scope:
//
//   struct Transform { Vec3 position; Vec3 scale; };
//
//   HARPIA_REFLECT_BEGIN(Transform, 1)
//       HARPIA_FIELD(position)
//       HARPIA_FIELD_RANGE(scale, 0.01, 100.0)
//   HARPIA_REFLECT_END(Transform)
#pragma once

#include "Core/Reflection/TypeInfo.h"
#include "Core/Reflection/TypeRegistry.h"

#include <cstdint>
#include <string>
#include <type_traits>
#include <utility>
#include <vector>

namespace harpia::reflect {

// --- type mapping ----------------------------------------------------------

template <typename T>
struct IsStdVector : std::false_type {};

template <typename T, typename A>
struct IsStdVector<std::vector<T, A>> : std::true_type {};

template <typename T>
struct VectorElement { using Type = void; };

template <typename T, typename A>
struct VectorElement<std::vector<T, A>> { using Type = T; };

template <typename T>
[[nodiscard]] constexpr FieldKind fieldKindOf() noexcept
{
    using U = std::remove_cv_t<T>;

    if constexpr (std::is_same_v<U, bool>)                 return FieldKind::Bool;
    else if constexpr (std::is_same_v<U, std::int8_t>)     return FieldKind::Int8;
    else if constexpr (std::is_same_v<U, std::int16_t>)    return FieldKind::Int16;
    else if constexpr (std::is_same_v<U, std::int32_t>)    return FieldKind::Int32;
    else if constexpr (std::is_same_v<U, std::int64_t>)    return FieldKind::Int64;
    else if constexpr (std::is_same_v<U, std::uint8_t>)    return FieldKind::UInt8;
    else if constexpr (std::is_same_v<U, std::uint16_t>)   return FieldKind::UInt16;
    else if constexpr (std::is_same_v<U, std::uint32_t>)   return FieldKind::UInt32;
    else if constexpr (std::is_same_v<U, std::uint64_t>)   return FieldKind::UInt64;
    else if constexpr (std::is_same_v<U, float>)           return FieldKind::Float;
    else if constexpr (std::is_same_v<U, double>)          return FieldKind::Double;
    else if constexpr (std::is_same_v<U, std::string>)     return FieldKind::String;
    else if constexpr (std::is_enum_v<U>)                  return FieldKind::Enum;
    else if constexpr (IsStdVector<U>::value)              return FieldKind::Vector;
    else                                                    return FieldKind::Struct;
}

template <typename T>
[[nodiscard]] VectorOps makeVectorOps() noexcept
{
    static_assert(!std::is_same_v<T, bool>,
                  "std::vector<bool> is a bitset and has no addressable elements; "
                  "use std::vector<std::uint8_t>");

    VectorOps ops;
    ops.size = [](const void* vec) {
        return static_cast<const std::vector<T>*>(vec)->size();
    };
    ops.resize = [](void* vec, std::size_t n) {
        static_cast<std::vector<T>*>(vec)->resize(n);
    };
    ops.at = [](void* vec, std::size_t index) -> void* {
        return &(*static_cast<std::vector<T>*>(vec))[index];
    };
    ops.constAt = [](const void* vec, std::size_t index) -> const void* {
        return &(*static_cast<const std::vector<T>*>(vec))[index];
    };
    return ops;
}

// --- member pointer traits -------------------------------------------------

template <typename T>
struct MemberPointer;

template <typename C, typename M>
struct MemberPointer<M C::*> {
    using Class  = C;
    using Member = M;
};

// --- builder ---------------------------------------------------------------

class TypeInfoBuilder {
public:
    TypeInfoBuilder(std::string name,
                    std::size_t size,
                    std::size_t alignment,
                    std::uint32_t version)
    {
        info_.name      = std::move(name);
        info_.size      = size;
        info_.alignment = alignment;
        info_.version   = version;
    }

    template <typename T>
    TypeInfoBuilder& constructors()
    {
        if constexpr (std::is_default_constructible_v<T>) {
            info_.construct = [](void* memory) { ::new (memory) T(); };
        }
        info_.destruct = [](void* memory) { static_cast<T*>(memory)->~T(); };

        if constexpr (std::is_move_constructible_v<T>) {
            info_.moveConstruct = [](void* destination, void* source) {
                ::new (destination) T(std::move(*static_cast<T*>(source)));
            };
        }
        if constexpr (std::is_copy_constructible_v<T>) {
            info_.copyConstruct = [](void* destination, const void* source) {
                ::new (destination) T(*static_cast<const T*>(source));
            };
        }
        return *this;
    }

    // Member pointer as a template parameter: no offsetof, no undefined
    // behaviour on non-standard-layout types.
    template <auto Member>
    TypeInfoBuilder& field(const char* name)
    {
        using Traits = MemberPointer<decltype(Member)>;
        using Class  = typename Traits::Class;
        using Type   = typename Traits::Member;

        FieldInfo f;
        f.name = name;
        f.kind = fieldKindOf<Type>();
        f.size = sizeof(Type);

        f.get = [](void* object) -> void* {
            return &(static_cast<Class*>(object)->*Member);
        };
        f.constGet = [](const void* object) -> const void* {
            return &(static_cast<const Class*>(object)->*Member);
        };

        if constexpr (fieldKindOf<Type>() == FieldKind::Struct) {
            f.structType = &TypeOf<Type>::info();
        } else if constexpr (fieldKindOf<Type>() == FieldKind::Vector) {
            using Element = typename VectorElement<Type>::Type;
            f.elementKind = fieldKindOf<Element>();
            f.elementSize = sizeof(Element);
            f.vectorOps   = makeVectorOps<Element>();
            if constexpr (fieldKindOf<Element>() == FieldKind::Struct) {
                f.structType = &TypeOf<Element>::info();
            }
        }

        info_.fields.push_back(f);
        return *this;
    }

    TypeInfoBuilder& range(double minimum, double maximum)
    {
        if (!info_.fields.empty()) {
            FieldInfo& f = info_.fields.back();
            f.rangeMin = minimum;
            f.rangeMax = maximum;
            f.hasRange = true;
        }
        return *this;
    }

    TypeInfoBuilder& tooltip(const char* text)
    {
        if (!info_.fields.empty()) {
            info_.fields.back().tooltip = text;
        }
        return *this;
    }

    TypeInfoBuilder& hidden()
    {
        if (!info_.fields.empty()) {
            info_.fields.back().hidden = true;
        }
        return *this;
    }

    TypeInfoBuilder& migrate(MigrateFn fn)
    {
        info_.migrate = fn;
        return *this;
    }

    [[nodiscard]] TypeInfo build() { return std::move(info_); }

private:
    TypeInfo info_;
};

} // namespace harpia::reflect

// --- macros ----------------------------------------------------------------

#define HARPIA_REFLECT_CONCAT_(a, b) a##b
#define HARPIA_REFLECT_CONCAT(a, b) HARPIA_REFLECT_CONCAT_(a, b)

#define HARPIA_REFLECT_BEGIN(TypeName, Version)                                  \
    namespace harpia::reflect {                                                  \
    template <>                                                                  \
    struct TypeOf<TypeName> {                                                    \
        using Self = TypeName;                                                   \
        static const TypeInfo& info()                                            \
        {                                                                        \
            static const TypeInfo* stored = [] {                                 \
                TypeInfoBuilder builder(#TypeName, sizeof(TypeName),             \
                                        alignof(TypeName), (Version));           \
                builder.constructors<TypeName>();

#define HARPIA_FIELD(Name) builder.field<&Self::Name>(#Name);

#define HARPIA_FIELD_RANGE(Name, Min, Max) \
    builder.field<&Self::Name>(#Name).range((Min), (Max));

#define HARPIA_FIELD_TOOLTIP(Name, Text) \
    builder.field<&Self::Name>(#Name).tooltip(Text);

#define HARPIA_FIELD_HIDDEN(Name) builder.field<&Self::Name>(#Name).hidden();

#define HARPIA_MIGRATE(Fn) builder.migrate(Fn);

#define HARPIA_REFLECT_END(TypeName)                                             \
                return TypeRegistry::add(builder.build());                       \
            }();                                                                 \
            return *stored;                                                      \
        }                                                                        \
    };                                                                           \
    namespace {                                                                  \
    const AutoRegister<TypeName>                                                 \
        HARPIA_REFLECT_CONCAT(harpiaAutoRegister_, __LINE__);                    \
    }                                                                            \
    } /* namespace harpia::reflect */
