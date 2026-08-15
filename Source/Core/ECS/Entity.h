// Harpia Engine — entity identity and component ids
//
// Roadmap 4.1: the capability ladder puts archetype ECS near the frontier,
// which is wrong. It is a day-one decision — migrating an OOP entity tree to
// ECS later is rewriting the game. So it lives in F0, next to memory and jobs.
//
// Components must be reflected. That requirement is the integration the Dagor
// audit called out: there, component registration and DataBlock are separate
// systems. Here one TypeRegistry serves the ECS, the inspector and saving.
#pragma once

#include "Core/Reflection/TypeInfo.h"
#include "Core/Reflection/TypeRegistry.h"

#include <bitset>
#include <cstdint>

namespace harpia::ecs {

using ComponentId = std::uint16_t;

inline constexpr ComponentId kInvalidComponent  = 0xFFFFu;
inline constexpr std::size_t kMaxComponentTypes = 256;

using ComponentMask = std::bitset<kMaxComponentTypes>;

// generation == 0 is always invalid, so a value-initialised Entity is null.
struct Entity {
    std::uint32_t index      = 0;
    std::uint32_t generation = 0;

    [[nodiscard]] constexpr bool valid() const noexcept { return generation != 0; }

    [[nodiscard]] friend constexpr bool operator==(Entity a, Entity b) noexcept
    {
        return a.index == b.index && a.generation == b.generation;
    }
};

// Assigns a dense id per component type, first come first served. Ids are
// per-process and must not be serialised — the type name is the stable key.
class ComponentRegistry {
public:
    template <typename T>
    [[nodiscard]] static ComponentId id()
    {
        static_assert(std::is_trivially_copyable_v<T> || std::is_move_constructible_v<T>,
                      "components must be movable so archetype transitions can relocate them");
        static const ComponentId value = add(&reflect::TypeOf<T>::info());
        return value;
    }

    [[nodiscard]] static const reflect::TypeInfo* typeInfo(ComponentId id) noexcept;
    [[nodiscard]] static ComponentId              findByName(std::string_view name) noexcept;
    [[nodiscard]] static std::size_t              count() noexcept;

private:
    static ComponentId add(const reflect::TypeInfo* type);
};

template <typename... Cs>
[[nodiscard]] ComponentMask maskOf()
{
    ComponentMask mask;
    (mask.set(ComponentRegistry::id<Cs>()), ...);
    return mask;
}

} // namespace harpia::ecs
