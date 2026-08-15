// Harpia Engine — the type registry
//
// One lookup surface for every consumer. Deserialising a scene needs to find a
// type by the name stored in the file, which is why registration is eager
// rather than on first use.
#pragma once

#include "Core/Reflection/TypeInfo.h"

#include <string_view>
#include <vector>

namespace harpia::reflect {

// Specialised by HARPIA_REFLECT_BEGIN/END. The primary template is left
// undefined so using an unreflected type is a compile error with a readable
// message rather than a link error.
template <typename T>
struct TypeOf;

class TypeRegistry {
public:
    // Registers and returns the stored pointer. Registering the same name twice
    // with a different definition is a programming error and asserts in debug.
    static const TypeInfo* add(TypeInfo&& info);

    [[nodiscard]] static const TypeInfo* find(std::string_view name) noexcept;
    [[nodiscard]] static std::vector<const TypeInfo*> all();
    [[nodiscard]] static std::size_t count() noexcept;

    template <typename T>
    [[nodiscard]] static const TypeInfo* get()
    {
        return &TypeOf<T>::info();
    }
};

// Forces registration at static-init time so find() works before any code has
// touched the type. Instantiated by HARPIA_REFLECT_END.
template <typename T>
struct AutoRegister {
    AutoRegister() { (void)TypeOf<T>::info(); }
};

} // namespace harpia::reflect
