// Harpia Engine — asset identity
//
// Roadmap 1.7, the one column-4 decision that has to be in place from the
// start: an asset's identity is a GUID, never its path. Renaming or moving a
// file must not break a single reference to it.
//
// This is the piece that is cheap now and brutal later — retrofitting stable
// ids means rewriting every scene, prefab and material that ever shipped.
#pragma once

#include <cstdint>
#include <functional>
#include <string>
#include <string_view>

namespace harpia {

struct AssetId {
    std::uint64_t high = 0;
    std::uint64_t low  = 0;

    [[nodiscard]] constexpr bool valid() const noexcept { return high != 0 || low != 0; }

    // Random 128-bit. Collision probability is negligible at any project size
    // we will ever reach, and it needs no central authority to hand out ids.
    [[nodiscard]] static AssetId generate();

    // 32 lowercase hex characters, no dashes — one token, easy to grep for in
    // a scene file or a diff.
    [[nodiscard]] std::string toString() const;
    [[nodiscard]] static AssetId parse(std::string_view text);

    [[nodiscard]] friend constexpr bool operator==(AssetId a, AssetId b) noexcept
    {
        return a.high == b.high && a.low == b.low;
    }

    [[nodiscard]] friend constexpr bool operator<(AssetId a, AssetId b) noexcept
    {
        return a.high != b.high ? a.high < b.high : a.low < b.low;
    }
};

} // namespace harpia

template <>
struct std::hash<harpia::AssetId> {
    [[nodiscard]] std::size_t operator()(const harpia::AssetId& id) const noexcept
    {
        // The bits are already random; xor-folding them is enough and avoids
        // paying for a mixer on every lookup.
        return static_cast<std::size_t>(id.high ^ (id.low * 0x9E3779B97F4A7C15ull));
    }
};
