// Harpia Engine — image import
//
// stb_image decodes; Dagor reaches for the same class of single-header decoder.
// KTX2 and Basis belong in the offline cook (roadmap 1.7 stage 3), not here:
// this path exists so a raw PNG or JPEG works straight from a checkout.
#pragma once

#include "Core/Assets/AssetManager.h"
#include "Core/Assets/HdrImage.h"
#include "Core/Assets/TextureAsset.h"

#include <cstdint>
#include <filesystem>
#include <memory>
#include <span>
#include <string>

namespace harpia {

struct TextureImportResult {
    std::shared_ptr<TextureAsset> texture;
    std::string                   error;

    [[nodiscard]] explicit operator bool() const noexcept { return texture != nullptr; }
};

[[nodiscard]] TextureImportResult importTexture(const std::filesystem::path& path);

// For glTF images embedded as a data URI or a buffer view.
[[nodiscard]] TextureImportResult importTextureFromMemory(std::span<const std::uint8_t> bytes);

struct HdrImportResult {
    std::shared_ptr<HdrImageAsset> image;
    std::string                    error;

    [[nodiscard]] explicit operator bool() const noexcept { return image != nullptr; }
};

// Radiance .hdr, the format environment maps ship in. Separate entry point
// rather than a flag on importTexture: the return types differ because the
// pixel data differs, and a caller that wants radiance should not be able to
// silently receive quantised colour.
[[nodiscard]] HdrImportResult importHdrImage(const std::filesystem::path& path);

void registerTextureLoader(AssetManager& manager);

} // namespace harpia
