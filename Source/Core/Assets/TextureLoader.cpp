#include "Core/Assets/TextureLoader.h"

#include <stb_image.h>

#include <bit>
#include <cstdio>

namespace harpia {

std::uint32_t TextureAsset::mipLevels() const noexcept
{
    if (width == 0 || height == 0) {
        return 0;
    }
    const std::uint32_t largest = width > height ? width : height;
    return static_cast<std::uint32_t>(std::bit_width(largest));
}

std::uint32_t TextureAsset::texel(std::uint32_t x, std::uint32_t y) const noexcept
{
    if (x >= width || y >= height) {
        return 0;
    }
    const std::size_t offset = (static_cast<std::size_t>(y) * width + x) * 4;
    return static_cast<std::uint32_t>(pixels[offset])
         | static_cast<std::uint32_t>(pixels[offset + 1]) << 8
         | static_cast<std::uint32_t>(pixels[offset + 2]) << 16
         | static_cast<std::uint32_t>(pixels[offset + 3]) << 24;
}

TextureImportResult importTexture(const std::filesystem::path& path)
{
    TextureImportResult result;

    int width    = 0;
    int height   = 0;
    int channels = 0;

    // Forcing 4 channels: three-channel images have no universally supported
    // GPU format, and padding once here beats a branch at every sample site.
    stbi_uc* decoded = stbi_load(path.string().c_str(), &width, &height, &channels,
                                 STBI_rgb_alpha);
    if (decoded == nullptr) {
        const char* reason = stbi_failure_reason();
        result.error = "stbi_load failed for " + path.string()
                     + (reason != nullptr ? std::string(": ") + reason : std::string());
        return result;
    }

    if (width <= 0 || height <= 0) {
        stbi_image_free(decoded);
        result.error = "degenerate image size in " + path.string();
        return result;
    }

    auto texture = std::make_shared<TextureAsset>();
    texture->width          = static_cast<std::uint32_t>(width);
    texture->height         = static_cast<std::uint32_t>(height);
    texture->sourceChannels = static_cast<std::uint32_t>(channels);

    const std::size_t byteCount = static_cast<std::size_t>(width)
                                * static_cast<std::size_t>(height) * 4;
    texture->pixels.assign(decoded, decoded + byteCount);

    stbi_image_free(decoded);

    result.texture = std::move(texture);
    return result;
}

TextureImportResult importTextureFromMemory(std::span<const std::uint8_t> bytes)
{
    TextureImportResult result;

    int width    = 0;
    int height   = 0;
    int channels = 0;

    stbi_uc* decoded = stbi_load_from_memory(bytes.data(),
                                             static_cast<int>(bytes.size()),
                                             &width, &height, &channels, STBI_rgb_alpha);
    if (decoded == nullptr) {
        const char* reason = stbi_failure_reason();
        result.error = std::string("stbi_load_from_memory failed")
                     + (reason != nullptr ? std::string(": ") + reason : std::string());
        return result;
    }

    auto texture = std::make_shared<TextureAsset>();
    texture->width          = static_cast<std::uint32_t>(width);
    texture->height         = static_cast<std::uint32_t>(height);
    texture->sourceChannels = static_cast<std::uint32_t>(channels);
    const std::size_t byteCount = static_cast<std::size_t>(width)
                                * static_cast<std::size_t>(height) * 4;
    texture->pixels.assign(decoded, decoded + byteCount);

    stbi_image_free(decoded);

    result.texture = std::move(texture);
    return result;
}

void registerTextureLoader(AssetManager& manager)
{
    manager.registerLoader(AssetType::Texture,
        [](const std::filesystem::path& path) -> std::shared_ptr<Asset> {
            TextureImportResult imported = importTexture(path);
            if (!imported) {
                std::fprintf(stderr, "[texture] %s\n", imported.error.c_str());
            }
            return imported.texture;
        });
}

} // namespace harpia
