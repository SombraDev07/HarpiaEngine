// Texture import and residency. The sRGB question gets its own test because
// getting it wrong makes lighting consistently and subtly wrong — the kind of
// error that gets compensated for in art instead of fixed.

#include <doctest/doctest.h>

#include "Core/Assets/AssetDatabase.h"
#include "Core/Assets/AssetManager.h"
#include "Core/Assets/TextureLoader.h"
#include "RHI/GpuTexture.h"
#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanRenderer.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

using namespace harpia;
namespace fs = std::filesystem;

namespace {

// A minimal uncompressed PNG written by hand, so the test needs no fixture on
// disk and no encoder.
std::vector<std::uint8_t> makePng(std::uint32_t width, std::uint32_t height,
                                  const std::vector<std::uint8_t>& rgba);

std::uint32_t crc32(const std::uint8_t* data, std::size_t size)
{
    static std::uint32_t table[256];
    static bool ready = false;
    if (!ready) {
        for (std::uint32_t i = 0; i < 256; ++i) {
            std::uint32_t c = i;
            for (int k = 0; k < 8; ++k) {
                c = (c & 1) ? (0xEDB88320u ^ (c >> 1)) : (c >> 1);
            }
            table[i] = c;
        }
        ready = true;
    }
    std::uint32_t c = 0xFFFFFFFFu;
    for (std::size_t i = 0; i < size; ++i) {
        c = table[(c ^ data[i]) & 0xFFu] ^ (c >> 8);
    }
    return c ^ 0xFFFFFFFFu;
}

void appendBigEndian(std::vector<std::uint8_t>& out, std::uint32_t value)
{
    out.push_back(static_cast<std::uint8_t>(value >> 24));
    out.push_back(static_cast<std::uint8_t>(value >> 16));
    out.push_back(static_cast<std::uint8_t>(value >> 8));
    out.push_back(static_cast<std::uint8_t>(value));
}

void appendChunk(std::vector<std::uint8_t>& out, const char* type,
                 const std::vector<std::uint8_t>& payload)
{
    appendBigEndian(out, static_cast<std::uint32_t>(payload.size()));

    std::vector<std::uint8_t> typed(type, type + 4);
    typed.insert(typed.end(), payload.begin(), payload.end());
    out.insert(out.end(), typed.begin(), typed.end());

    appendBigEndian(out, crc32(typed.data(), typed.size()));
}

// zlib stream with stored (uncompressed) deflate blocks — valid, and avoids
// pulling a compressor into the test.
std::vector<std::uint8_t> storedZlib(const std::vector<std::uint8_t>& data)
{
    std::vector<std::uint8_t> out{0x78, 0x01};

    std::size_t offset = 0;
    while (offset < data.size()) {
        const std::size_t chunk = std::min<std::size_t>(data.size() - offset, 65535);
        const bool last = (offset + chunk) >= data.size();

        out.push_back(last ? 1 : 0);
        out.push_back(static_cast<std::uint8_t>(chunk & 0xFF));
        out.push_back(static_cast<std::uint8_t>(chunk >> 8));
        out.push_back(static_cast<std::uint8_t>(~chunk & 0xFF));
        out.push_back(static_cast<std::uint8_t>((~chunk >> 8) & 0xFF));
        out.insert(out.end(), data.begin() + static_cast<std::ptrdiff_t>(offset),
                   data.begin() + static_cast<std::ptrdiff_t>(offset + chunk));
        offset += chunk;
    }

    std::uint32_t a = 1;
    std::uint32_t b = 0;
    for (const std::uint8_t byte : data) {
        a = (a + byte) % 65521;
        b = (b + a) % 65521;
    }
    appendBigEndian(out, (b << 16) | a);
    return out;
}

std::vector<std::uint8_t> makePng(std::uint32_t width, std::uint32_t height,
                                  const std::vector<std::uint8_t>& rgba)
{
    std::vector<std::uint8_t> png{0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A};

    std::vector<std::uint8_t> ihdr;
    appendBigEndian(ihdr, width);
    appendBigEndian(ihdr, height);
    ihdr.push_back(8); // bit depth
    ihdr.push_back(6); // RGBA
    ihdr.push_back(0); // deflate
    ihdr.push_back(0); // no filter
    ihdr.push_back(0); // no interlace
    appendChunk(png, "IHDR", ihdr);

    std::vector<std::uint8_t> raw;
    for (std::uint32_t y = 0; y < height; ++y) {
        raw.push_back(0); // filter: none
        const std::size_t rowStart = static_cast<std::size_t>(y) * width * 4;
        raw.insert(raw.end(),
                   rgba.begin() + static_cast<std::ptrdiff_t>(rowStart),
                   rgba.begin() + static_cast<std::ptrdiff_t>(rowStart + width * 4));
    }
    appendChunk(png, "IDAT", storedZlib(raw));
    appendChunk(png, "IEND", {});
    return png;
}

// Radiance RGBE: one shared 8-bit exponent for the three mantissas, which is
// what buys the range in four bytes. Written by hand for the same reason the
// PNG above is — a fixture nobody reads is a fixture nobody maintains.
void encodeRgbe(const std::array<float, 3>& rgb, std::uint8_t out[4])
{
    const float largest = std::max({rgb[0], rgb[1], rgb[2]});
    if (largest < 1e-32f) {
        out[0] = out[1] = out[2] = out[3] = 0;
        return;
    }

    int exponent = 0;
    (void)std::frexp(largest, &exponent);
    const double scale = std::ldexp(1.0, 8 - exponent);

    for (int c = 0; c < 3; ++c) {
        const double scaled = static_cast<double>(rgb[static_cast<std::size_t>(c)]) * scale;
        out[c] = static_cast<std::uint8_t>(scaled < 0.0 ? 0.0
                                         : (scaled > 255.0 ? 255.0 : scaled));
    }
    out[3] = static_cast<std::uint8_t>(exponent + 128);
}

// Writes flat (uncompressed) scanlines. Widths under 8 make stb_image take the
// non-RLE path unconditionally, which keeps this writer to what it is.
std::vector<std::uint8_t> makeHdr(std::uint32_t width, std::uint32_t height,
                                  const std::vector<std::array<float, 3>>& radiance)
{
    const std::string header = "#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n"
                             + std::string("-Y ") + std::to_string(height)
                             + " +X " + std::to_string(width) + "\n";

    std::vector<std::uint8_t> out(header.begin(), header.end());
    for (const std::array<float, 3>& pixel : radiance) {
        std::uint8_t rgbe[4];
        encodeRgbe(pixel, rgbe);
        out.insert(out.end(), rgbe, rgbe + 4);
    }
    return out;
}

struct TempHdr {
    fs::path root;
    fs::path file;

    TempHdr(std::uint32_t width, std::uint32_t height,
            const std::vector<std::array<float, 3>>& radiance)
    {
        root = fs::temp_directory_path() / ("harpia_hdr_" + AssetId::generate().toString());
        fs::create_directories(root);
        file = root / "environment.hdr";

        const std::vector<std::uint8_t> bytes = makeHdr(width, height, radiance);
        std::ofstream out(file, std::ios::binary);
        out.write(reinterpret_cast<const char*>(bytes.data()),
                  static_cast<std::streamsize>(bytes.size()));
    }

    ~TempHdr()
    {
        std::error_code error;
        fs::remove_all(root, error);
    }

    TempHdr(const TempHdr&)            = delete;
    TempHdr& operator=(const TempHdr&) = delete;
};

struct TempImage {
    fs::path root;
    fs::path file;

    TempImage(std::uint32_t width, std::uint32_t height,
              const std::vector<std::uint8_t>& rgba,
              const std::string& name = "texture.png")
    {
        root = fs::temp_directory_path() / ("harpia_tex_" + AssetId::generate().toString());
        fs::create_directories(root);
        file = root / name;

        const std::vector<std::uint8_t> png = makePng(width, height, rgba);
        std::ofstream out(file, std::ios::binary);
        out.write(reinterpret_cast<const char*>(png.data()),
                  static_cast<std::streamsize>(png.size()));
    }

    ~TempImage()
    {
        std::error_code error;
        fs::remove_all(root, error);
    }
};

std::vector<std::uint8_t> checkerboard(std::uint32_t size)
{
    std::vector<std::uint8_t> rgba(static_cast<std::size_t>(size) * size * 4);
    for (std::uint32_t y = 0; y < size; ++y) {
        for (std::uint32_t x = 0; x < size; ++x) {
            const bool white = ((x / 4) + (y / 4)) % 2 == 0;
            const std::size_t offset = (static_cast<std::size_t>(y) * size + x) * 4;
            rgba[offset + 0] = white ? 255 : 0;
            rgba[offset + 1] = white ? 255 : 0;
            rgba[offset + 2] = white ? 255 : 0;
            rgba[offset + 3] = 255;
        }
    }
    return rgba;
}

} // namespace

TEST_CASE("a PNG decodes to tightly packed RGBA8")
{
    const std::vector<std::uint8_t> source{
        255, 0, 0, 255,   0, 255, 0, 255,
        0, 0, 255, 255,   255, 255, 255, 128,
    };
    const TempImage image(2, 2, source);

    const TextureImportResult result = importTexture(image.file);
    REQUIRE_MESSAGE(result, result.error);

    const TextureAsset& texture = *result.texture;
    CHECK(texture.width == 2);
    CHECK(texture.height == 2);
    CHECK(texture.sizeBytes() == 16);

    // Red, green, blue, then white at half alpha — order and channels intact.
    CHECK(texture.pixels[0] == 255);
    CHECK(texture.pixels[1] == 0);
    CHECK(texture.pixels[5] == 255);
    CHECK(texture.pixels[10] == 255);
    CHECK(texture.pixels[15] == 128);
}

TEST_CASE("mip level count follows the longest edge")
{
    TextureAsset texture;

    texture.width = 1;   texture.height = 1;   CHECK(texture.mipLevels() == 1);
    texture.width = 2;   texture.height = 2;   CHECK(texture.mipLevels() == 2);
    texture.width = 256; texture.height = 256; CHECK(texture.mipLevels() == 9);
    // Non-square: the longer edge decides how far the chain goes.
    texture.width = 256; texture.height = 4;   CHECK(texture.mipLevels() == 9);
}

TEST_CASE("a corrupt or missing image is reported rather than crashed on")
{
    SUBCASE("not an image")
    {
        const fs::path root = fs::temp_directory_path()
                            / ("harpia_bad_" + AssetId::generate().toString());
        fs::create_directories(root);
        const fs::path file = root / "broken.png";
        std::ofstream(file) << "definitely not a png";

        const TextureImportResult result = importTexture(file);
        CHECK_FALSE(result);
        CHECK_FALSE(result.error.empty());

        std::error_code error;
        fs::remove_all(root, error);
    }

    SUBCASE("missing file")
    {
        CHECK_FALSE(importTexture("/nonexistent/image.png"));
    }

    SUBCASE("empty memory")
    {
        CHECK_FALSE(importTextureFromMemory({}));
    }
}

TEST_CASE("textures load through the asset manager by GUID")
{
    const TempImage image(8, 8, checkerboard(8), "albedo.png");

    AssetDatabase database;
    REQUIRE(database.open(image.root));
    database.scan();

    AssetManager manager;
    manager.attach(&database);
    registerTextureLoader(manager);

    const AssetId id = database.idOf(image.file);
    REQUIRE(id.valid());

    const std::shared_ptr<TextureAsset> texture = manager.load<TextureAsset>(id);
    REQUIRE(texture != nullptr);
    CHECK(texture->width == 8);
    CHECK(texture->type() == AssetType::Texture);
    CHECK(manager.load<TextureAsset>(id) == texture); // cached
}

TEST_CASE("a radiance .hdr decodes to float with its range intact")
{
    // Values chosen to straddle 1.0: the whole reason an environment map is not
    // an 8-bit texture is that a sky is many times brighter than white.
    const std::vector<std::array<float, 3>> radiance{
        {0.25f, 0.50f, 0.75f},
        {12.0f, 12.0f, 12.0f},
        {0.00f, 0.00f, 0.00f},
        {40.0f, 10.0f,  2.5f},
        {1.00f, 1.00f, 1.00f},
        {0.10f, 0.20f, 0.30f},
        {3.50f, 0.05f, 0.00f},
        {0.01f, 0.01f, 0.01f},
    };

    const TempHdr image(4, 2, radiance);

    const HdrImportResult imported = importHdrImage(image.file);
    REQUIRE(imported);
    REQUIRE(imported.image != nullptr);

    const HdrImageAsset& hdr = *imported.image;
    CHECK(hdr.width == 4);
    CHECK(hdr.height == 2);
    CHECK(hdr.pixels.size() == 4 * 2 * 4);

    SUBCASE("values above one survive the round trip")
    {
        // RGBE carries an 8-bit mantissa, so the tolerance is relative, not
        // absolute — 40.0 cannot come back to more precision than ~0.4%.
        for (std::size_t i = 0; i < radiance.size(); ++i) {
            const float* texel = &hdr.pixels[i * 4];
            for (int c = 0; c < 3; ++c) {
                const float expected = radiance[i][static_cast<std::size_t>(c)];
                if (expected > 0.0f) {
                    CHECK(texel[c] == doctest::Approx(expected).epsilon(0.01));
                } else {
                    CHECK(texel[c] == doctest::Approx(0.0f).epsilon(0.001));
                }
            }
        }
    }

    SUBCASE("the brightest sample stays far above white")
    {
        // The property that matters downstream: clamping to 1.0 anywhere in the
        // path would make prefiltering average away the sun.
        const float* sun = &hdr.pixels[3 * 4];
        CHECK(sun[0] > 30.0f);
    }

    SUBCASE("alpha is padding, not data")
    {
        for (std::size_t i = 0; i < radiance.size(); ++i) {
            CHECK(hdr.pixels[i * 4 + 3] == doctest::Approx(1.0f));
        }
    }

    SUBCASE("the clamped fetch never reads outside the image")
    {
        const float* inside  = hdr.texel(3, 1);
        const float* clamped = hdr.texel(99, 99);
        CHECK(clamped == inside); // same address: clamped to the last texel
    }
}

TEST_CASE("a file that is not radiance is reported rather than crashed on")
{
    const fs::path root = fs::temp_directory_path()
                        / ("harpia_hdr_bad_" + AssetId::generate().toString());
    fs::create_directories(root);
    const fs::path file = root / "not_really.hdr";
    {
        std::ofstream out(file, std::ios::binary);
        out << "this is not a radiance file";
    }

    const HdrImportResult imported = importHdrImage(file);
    CHECK_FALSE(imported);
    CHECK_FALSE(imported.error.empty());

    std::error_code error;
    fs::remove_all(root, error);
}

TEST_SUITE_BEGIN("gpu");

TEST_CASE("a texture becomes resident with a full mip chain")
{
    rhi::VulkanDevice::resetValidationErrorCount();

    rhi::DeviceDesc desc;
    desc.enableValidation = true;
    desc.window           = nullptr;

    rhi::VulkanDevice device;
    if (!device.create(desc)) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    rhi::VulkanRenderer renderer;
    rhi::GpuUploader    uploader;
    REQUIRE(renderer.createOffscreen(device, 16, 16));
    REQUIRE(uploader.create(device));

    const TempImage image(64, 64, checkerboard(64));
    const TextureImportResult imported = importTexture(image.file);
    REQUIRE_MESSAGE(imported, imported.error);

    SUBCASE("sRGB for colour, linear for data")
    {
        rhi::GpuTexture colour;
        REQUIRE(colour.create(device, uploader, renderer.bindless(), *imported.texture,
                              rhi::TextureColorSpace::Srgb, true, "BaseColour"));
        // The colour space is a format choice, and it has to reach the image.
        CHECK(colour.format() == VK_FORMAT_R8G8B8A8_SRGB);
        CHECK(colour.mipLevels() == 7); // 64 -> 1
        CHECK(colour.bindlessIndex() != rhi::VulkanBindless::kInvalidIndex);

        rhi::GpuTexture data;
        REQUIRE(data.create(device, uploader, renderer.bindless(), *imported.texture,
                            rhi::TextureColorSpace::Linear, true, "NormalMap"));
        CHECK(data.format() == VK_FORMAT_R8G8B8A8_UNORM);
        // Two textures, two distinct slots.
        CHECK(data.bindlessIndex() != colour.bindlessIndex());

        colour.destroy();
        data.destroy();
    }

    SUBCASE("mips can be declined")
    {
        rhi::GpuTexture texture;
        REQUIRE(texture.create(device, uploader, renderer.bindless(), *imported.texture,
                               rhi::TextureColorSpace::Srgb, false, "NoMips"));
        CHECK(texture.mipLevels() == 1);
        texture.destroy();
    }

    SUBCASE("a solid 1x1 stands in for a missing map")
    {
        rhi::GpuTexture white;
        REQUIRE(white.createSolid(device, uploader, renderer.bindless(),
                                  0xFFFFFFFFu, rhi::TextureColorSpace::Srgb, "White"));
        CHECK(white.width() == 1);
        CHECK(white.mipLevels() == 1);
        CHECK(white.bindlessIndex() != rhi::VulkanBindless::kInvalidIndex);
        white.destroy();
    }

    SUBCASE("the bindless slot comes back on destroy")
    {
        const std::uint32_t before = renderer.bindless().usage().sampledImages;
        {
            rhi::GpuTexture texture;
            REQUIRE(texture.create(device, uploader, renderer.bindless(),
                                   *imported.texture, rhi::TextureColorSpace::Srgb,
                                   true, "Scoped"));
            CHECK(renderer.bindless().usage().sampledImages == before + 1);
        }
        CHECK(renderer.bindless().usage().sampledImages == before);
    }

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);

    uploader.destroy();
    renderer.destroy();
    device.destroy();
}

TEST_SUITE_END();
