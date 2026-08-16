// Harpia Engine — CPU-side radiance image
//
// Deliberately not a TextureAsset. That type expands everything to RGBA8
// because eight bits after an sRGB curve is enough for a material texture, and
// the one decision it refuses to make is colour space. Neither holds here: an
// environment map carries radiance, not colour, and radiance has no upper bound
// to quantise against — a sun is thousands of times brighter than the sky
// beside it, and rounding that to 255 throws away exactly the range that makes
// image-based lighting look lit rather than painted.
//
// So the two stay separate types rather than one type with a mode flag. A
// function that takes an HdrImage cannot be handed 8-bit colour by mistake.
#pragma once

#include "Core/Assets/AssetManager.h"

#include <cstddef>
#include <cstdint>
#include <vector>

namespace harpia {

class HdrImageAsset final : public Asset {
public:
    // Tightly packed RGBA32F. The alpha channel is padding: three-channel float
    // formats are poorly supported on GPUs, the same reason TextureAsset pads.
    std::vector<float> pixels;
    std::uint32_t      width  = 0;
    std::uint32_t      height = 0;

    [[nodiscard]] bool empty() const noexcept { return pixels.empty(); }
    [[nodiscard]] std::size_t sizeBytes() const noexcept
    {
        return pixels.size() * sizeof(float);
    }

    // Bounds-clamped fetch. Clamping rather than returning black matters for an
    // equirectangular map, where the caller walks right up to the seam.
    [[nodiscard]] const float* texel(std::uint32_t x, std::uint32_t y) const noexcept;
};

} // namespace harpia
