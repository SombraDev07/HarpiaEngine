// Harpia Engine — reflection-driven serialisation
//
// Roadmap 1.3: "the day you add a field to Transform and every saved scene
// breaks always arrives — the difference is whether you have migration."
//
// The format is name-keyed and length-prefixed per field, so:
//   - a field added later is simply absent from old data and keeps its default
//   - a field removed later is skipped by the reader without corrupting the rest
//   - a field renamed is a migration, which is exactly when you should bump the
//     type version and say so
#pragma once

#include "Core/Reflection/TypeInfo.h"
#include "Core/Reflection/TypeRegistry.h"
#include "Core/Serialization/ByteStream.h"

#include <cstdint>
#include <span>
#include <string>
#include <vector>

namespace harpia::serial {

inline constexpr std::uint32_t kMagic         = 0x41505248u; // "HRPA"
inline constexpr std::uint32_t kFormatVersion = 1;

enum class LoadStatus : std::uint8_t {
    Ok,
    BadMagic,
    UnsupportedFormat,
    UnknownType,
    Truncated,
    TypeMismatch,
};

[[nodiscard]] const char* toString(LoadStatus status) noexcept;

struct LoadResult {
    LoadStatus    status          = LoadStatus::Ok;
    std::uint32_t sourceVersion   = 0;
    std::uint32_t skippedFields   = 0;  // present in data, absent in the type
    std::uint32_t defaultedFields = 0;  // present in the type, absent in data
    bool          migrated        = false;

    [[nodiscard]] explicit operator bool() const noexcept { return status == LoadStatus::Ok; }
};

// Writes `object`, described by `type`, as a self-describing blob.
void save(const reflect::TypeInfo& type, const void* object, ByteWriter& writer);

[[nodiscard]] std::vector<std::uint8_t> saveToBytes(const reflect::TypeInfo& type,
                                                    const void*              object);

// Reads into `object`, which must already be constructed. Fields missing from
// the data keep whatever value they had.
[[nodiscard]] LoadResult load(const reflect::TypeInfo& type,
                              void*                    object,
                              ByteReader&              reader);

[[nodiscard]] LoadResult loadFromBytes(const reflect::TypeInfo&      type,
                                       void*                         object,
                                       std::span<const std::uint8_t> bytes);

template <typename T>
[[nodiscard]] std::vector<std::uint8_t> saveToBytes(const T& object)
{
    return saveToBytes(reflect::TypeOf<T>::info(), &object);
}

template <typename T>
[[nodiscard]] LoadResult loadFromBytes(T& object, std::span<const std::uint8_t> bytes)
{
    return loadFromBytes(reflect::TypeOf<T>::info(), &object, bytes);
}

} // namespace harpia::serial
