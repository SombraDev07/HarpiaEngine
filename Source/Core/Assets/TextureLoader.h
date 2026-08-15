// Harpia Engine — image import
//
// stb_image decodes; Dagor reaches for the same class of single-header decoder.
// KTX2 and Basis belong in the offline cook (roadmap 1.7 stage 3), not here:
// this path exists so a raw PNG or JPEG works straight from a checkout.
#pragma once

#include "Core/Assets/AssetManager.h"
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

void registerTextureLoader(AssetManager& manager);

} // namespace harpia
