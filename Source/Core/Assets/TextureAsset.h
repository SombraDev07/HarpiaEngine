// Harpia Engine — CPU-side image
//
// Decoded pixels only; Core never touches Vulkan. Always expanded to RGBA8
// regardless of what the file held, because a three-channel image has no
// universally supported GPU format and padding once at load beats branching
// everywhere afterwards.
//
// Colour space is deliberately NOT decided here. Whether a texture is sRGB
// depends on what it means, not on what it contains: base colour is sRGB, a
// normal map or a roughness map is linear, and the same PNG could be either.
// The material knows; the loader does not. GpuTexture takes it as a parameter.
#pragma once

#include "Core/Assets/AssetManager.h"

#include <cstdint>
#include <vector>

namespace harpia {

class TextureAsset final : public Asset {
public:
    std::vector<std::uint8_t> pixels;   // tightly packed RGBA8
    std::uint32_t width  = 0;
    std::uint32_t height = 0;
    std::uint32_t sourceChannels = 0;   // what the file actually had

    [[nodiscard]] bool empty() const noexcept { return pixels.empty(); }
    [[nodiscard]] std::size_t sizeBytes() const noexcept { return pixels.size(); }

    // Number of mip levels a full chain would have for this size.
    [[nodiscard]] std::uint32_t mipLevels() const noexcept;

    // Bounds-checked texel fetch; returns transparent black outside the image.
    [[nodiscard]] std::uint32_t texel(std::uint32_t x, std::uint32_t y) const noexcept;
};

} // namespace harpia
