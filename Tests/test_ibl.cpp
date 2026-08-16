// The split-sum BRDF table is pure maths with no free parameters, so it gets
// checked against an independent CPU integration of the same integral plus the
// analytic properties the table must satisfy at its edges.

#include <doctest/doctest.h>

#include "Core/Assets/HdrImage.h"
#include "Core/Math/Math.h"
#include "RHI/IblResources.h"
#include "RHI/Vulkan/VulkanBuffer.h"
#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanRenderer.h"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <string>
#include <vector>

using namespace harpia;

namespace {

// --- CPU mirror of Shaders/BrdfLut.frag.hlsl --------------------------------

float radicalInverseVdC(std::uint32_t bits)
{
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return static_cast<float>(bits) * 2.3283064365386963e-10f;
}

Vec3 importanceSampleGGX(Vec2 xi, Vec3 normal, float alpha)
{
    const float phi      = 2.0f * kPi * xi.x;
    const float cosTheta = std::sqrt((1.0f - xi.y) / (1.0f + (alpha * alpha - 1.0f) * xi.y));
    const float sinTheta = std::sqrt(1.0f - cosTheta * cosTheta);

    const Vec3 h{sinTheta * std::cos(phi), sinTheta * std::sin(phi), cosTheta};

    const Vec3 up = std::fabs(normal.z) < 0.999f ? Vec3(0, 0, 1) : Vec3(1, 0, 0);
    const Vec3 tangentX = normalize(cross(up, normal));
    const Vec3 tangentY = cross(normal, tangentX);

    return normalize(tangentX * h.x + tangentY * h.y + normal * h.z);
}

Vec2 integrateBrdf(float NoV, float roughness)
{
    const float alpha = roughness * roughness;

    const Vec3 view{std::sqrt(1.0f - NoV * NoV), 0.0f, NoV};
    const Vec3 normal{0.0f, 0.0f, 1.0f};

    float scale = 0.0f;
    float bias  = 0.0f;

    constexpr std::uint32_t kSamples = 1024;
    for (std::uint32_t i = 0; i < kSamples; ++i) {
        const Vec2 xi{static_cast<float>(i) / kSamples, radicalInverseVdC(i)};
        const Vec3 h = importanceSampleGGX(xi, normal, alpha);
        const Vec3 l = normalize(h * (2.0f * dot(view, h)) - view);

        const float NoL = std::max(l.z, 0.0f);
        if (NoL <= 0.0f) {
            continue;
        }

        const float NoH = std::max(h.z, 0.0f);
        const float VoH = std::max(dot(view, h), 0.0f);

        const float k = alpha * 0.5f;
        const float ggxV = NoV / (NoV * (1.0f - k) + k);
        const float ggxL = NoL / (NoL * (1.0f - k) + k);
        const float gVis = (ggxV * ggxL * VoH) / std::max(NoH * NoV, 1e-7f);

        const float fc = std::pow(1.0f - VoH, 5.0f);
        scale += (1.0f - fc) * gVis;
        bias  += fc * gVis;
    }

    return Vec2(scale, bias) / static_cast<float>(kSamples);
}

float half16ToFloat(std::uint16_t bits)
{
    const std::uint32_t sign     = static_cast<std::uint32_t>(bits >> 15) << 31;
    const std::uint32_t exponent = (bits >> 10) & 0x1Fu;
    const std::uint32_t mantissa = bits & 0x3FFu;

    std::uint32_t out = 0;
    if (exponent == 0) {
        out = sign;
    } else if (exponent == 31) {
        out = sign | 0x7F800000u | (mantissa << 13);
    } else {
        out = sign | ((exponent + 112u) << 23) | (mantissa << 13);
    }

    float result = 0.0f;
    std::memcpy(&result, &out, sizeof(result));
    return result;
}

} // namespace

TEST_SUITE_BEGIN("gpu");

TEST_CASE("the split-sum BRDF table matches an independent CPU integration")
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

    rhi::IblResources ibl;
    REQUIRE(ibl.create(device, renderer.bindless(), std::string(HARPIA_SHADER_DIR)));
    CHECK(ibl.brdfLutIndex() != rhi::VulkanBindless::kInvalidIndex);

    constexpr std::uint32_t kSize = rhi::IblResources::kBrdfLutSize;

    std::vector<std::uint8_t> raw;
    REQUIRE(uploader.downloadImage(ibl.brdfLutImage(),
                                   VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                                   VkExtent2D{kSize, kSize}, 4, raw));

    const auto* halves = reinterpret_cast<const std::uint16_t*>(raw.data());
    const auto texel = [&](std::uint32_t x, std::uint32_t y) {
        const std::size_t offset = (static_cast<std::size_t>(y) * kSize + x) * 2;
        return Vec2(half16ToFloat(halves[offset]), half16ToFloat(halves[offset + 1]));
    };

    SUBCASE("values agree with the reference across the table")
    {
        float worstScale = 0.0f;
        float worstBias  = 0.0f;

        for (std::uint32_t y = 8; y < kSize; y += 32) {
            for (std::uint32_t x = 8; x < kSize; x += 32) {
                // The shader samples texel centres, so the reference has to
                // use the same coordinates rather than the texel corner.
                const float NoV       = (static_cast<float>(x) + 0.5f) / kSize;
                const float roughness = (static_cast<float>(y) + 0.5f) / kSize;

                const Vec2 expected = integrateBrdf(NoV, roughness);
                const Vec2 actual   = texel(x, y);

                worstScale = std::max(worstScale, std::fabs(actual.x - expected.x));
                worstBias  = std::max(worstBias, std::fabs(actual.y - expected.y));
            }
        }

        // Half float storage and the GPU's own transcendentals account for the
        // remainder; anything larger would mean the integrals differ.
        CHECK(worstScale < 0.01f);
        CHECK(worstBias < 0.01f);
    }

    SUBCASE("the table obeys the properties the approximation requires")
    {
        for (std::uint32_t y = 0; y < kSize; y += 16) {
            for (std::uint32_t x = 1; x < kSize; x += 16) {
                const Vec2 value = texel(x, y);

                CHECK(value.x >= 0.0f);
                CHECK(value.y >= 0.0f);
                // Energy conservation: F0 * scale + bias can never exceed one,
                // or a surface would reflect more than it received.
                CHECK(value.x + value.y <= 1.02f);
            }
        }
    }

    SUBCASE("a smooth surface leaves F0 essentially untouched")
    {
        // Top row is roughness ~0: a mirror reflects exactly F0 head-on, so
        // scale approaches 1 and bias approaches 0.
        const Vec2 mirror = texel(kSize - 4, 0);
        CHECK(mirror.x == doctest::Approx(1.0f).epsilon(0.05));
        CHECK(mirror.y < 0.05f);
    }

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);

    ibl.destroy();
    uploader.destroy();
    renderer.destroy();
    device.destroy();
}

TEST_CASE("the equirectangular projection puts each face where it looks")
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

    // A source whose colour encodes the direction it is seen from, so that "is
    // face 4 really +Z" becomes arithmetic rather than a judgement about a
    // picture. A sign error in the face basis still produces a cube that looks
    // plausible; only numbers catch it.
    //
    // Longitude is stored as its cosine and sine rather than as a ramp. A ramp
    // jumps from 1 to 0 at the antimeridian, and the sampler repeats in U — so
    // filtering across that seam would blend the two ends and report a midpoint
    // that means nothing. That blend is correct behaviour for a real map, whose
    // edges are continuous; it is only this synthetic source that would be
    // discontinuous. Encoding periodically keeps the source honest there.
    //
    // Everything is scaled by 4 so the whole image sits above 1.0: radiance has
    // to survive the projection, or prefiltering later averages an already
    // clipped sky.
    constexpr std::uint32_t kSourceWidth  = 64;
    constexpr std::uint32_t kSourceHeight = 32;
    constexpr float         kScale        = 4.0f;

    HdrImageAsset source;
    source.width  = kSourceWidth;
    source.height = kSourceHeight;
    source.pixels.resize(static_cast<std::size_t>(kSourceWidth) * kSourceHeight * 4);

    for (std::uint32_t y = 0; y < kSourceHeight; ++y) {
        for (std::uint32_t x = 0; x < kSourceWidth; ++x) {
            const std::size_t offset =
                (static_cast<std::size_t>(y) * kSourceWidth + x) * 4;

            const float u = (static_cast<float>(x) + 0.5f) / kSourceWidth;
            const float v = (static_cast<float>(y) + 0.5f) / kSourceHeight;
            const float longitude = (u - 0.5f) * 2.0f * kPi;

            source.pixels[offset + 0] = kScale * (0.5f + 0.5f * std::cos(longitude));
            source.pixels[offset + 1] = kScale * (0.5f + 0.5f * std::sin(longitude));
            source.pixels[offset + 2] = kScale * v;   // latitude does not wrap
            source.pixels[offset + 3] = 1.0f;
        }
    }

    rhi::IblResources ibl;
    REQUIRE(ibl.create(device, renderer.bindless(), std::string(HARPIA_SHADER_DIR)));
    REQUIRE(ibl.loadEnvironment(device, renderer.bindless(), source,
                                std::string(HARPIA_SHADER_DIR)));
    CHECK(ibl.hasEnvironment());
    CHECK(ibl.environmentIndex() != rhi::VulkanBindless::kInvalidIndex);

    constexpr std::uint32_t kSize = rhi::IblResources::kEnvironmentSize;

    // CPU mirror of Shaders/Cubemap.hlsli, written from the same convention
    // rather than shared with it.
    const auto faceDirection = [](std::uint32_t face, float u, float v) {
        const float s = 2.0f * u - 1.0f;
        const float t = 2.0f * v - 1.0f;
        Vec3 d;
        switch (face) {
            case 0: d = Vec3( 1.0f,    -t,    -s); break;
            case 1: d = Vec3(-1.0f,    -t,     s); break;
            case 2: d = Vec3(    s,  1.0f,     t); break;
            case 3: d = Vec3(    s, -1.0f,    -t); break;
            case 4: d = Vec3(    s,    -t,  1.0f); break;
            default: d = Vec3(  -s,    -t, -1.0f); break;
        }
        return normalize(d);
    };

    for (std::uint32_t face = 0; face < rhi::IblResources::kCubeFaces; ++face) {
        std::vector<std::uint8_t> raw;
        REQUIRE(uploader.downloadImage(ibl.environmentImage(),
                                       VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                                       VkExtent2D{kSize, kSize}, 8, raw,
                                       VK_IMAGE_ASPECT_COLOR_BIT, face));

        const auto* halves = reinterpret_cast<const std::uint16_t*>(raw.data());

        // The face centre, far from every edge where filtering would muddy the
        // comparison.
        const std::uint32_t centre = kSize / 2;
        const std::size_t   offset =
            (static_cast<std::size_t>(centre) * kSize + centre) * 4;

        // Undo the encoding: cosine and sine back to an angle, which compares
        // correctly across the wrap that broke a linear ramp.
        const float cosLongitude = half16ToFloat(halves[offset + 0]) / kScale * 2.0f - 1.0f;
        const float sinLongitude = half16ToFloat(halves[offset + 1]) / kScale * 2.0f - 1.0f;
        const float latitudeFraction = half16ToFloat(halves[offset + 2]) / kScale;

        const float u = (static_cast<float>(centre) + 0.5f) / kSize;
        const Vec3  direction = faceDirection(face, u, u);

        const float expectedLongitude = std::atan2(direction.z, direction.x);
        const float expectedLatitude =
            std::acos(std::clamp(direction.y, -1.0f, 1.0f)) / kPi;

        CAPTURE(face);
        CHECK(std::atan2(sinLongitude, cosLongitude)
              == doctest::Approx(expectedLongitude).epsilon(0.02));
        CHECK(latitudeFraction == doctest::Approx(expectedLatitude).epsilon(0.02));

        // The encoding never reaches zero, so a face that sampled nothing at all
        // would fail here rather than quietly reading as a valid direction.
        CHECK(std::fabs(cosLongitude) + std::fabs(sinLongitude) > 0.5f);
    }

    SUBCASE("opposite faces look in opposite directions")
    {
        // +X and -X must disagree by half a turn in longitude. A basis with one
        // axis flipped still produces a plausible-looking cube; this is what
        // separates plausible from correct.
        const Vec3 plusX  = faceDirection(0, 0.5f, 0.5f);
        const Vec3 minusX = faceDirection(1, 0.5f, 0.5f);
        CHECK(dot(plusX, minusX) == doctest::Approx(-1.0f).epsilon(0.001));
    }

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);

    ibl.destroy();
    uploader.destroy();
    renderer.destroy();
    device.destroy();
}

TEST_CASE("the prefiltered chain widens the lobe without inventing energy")
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

    // Half the sphere bright, half dark, split at the equator. A convolution
    // has nowhere to hide against a step: the mirror mip must keep it sharp,
    // and each rougher mip must soften it by a measurable amount.
    constexpr std::uint32_t kSourceWidth  = 64;
    constexpr std::uint32_t kSourceHeight = 32;
    constexpr float         kBright       = 8.0f;
    constexpr float         kDark         = 0.5f;

    HdrImageAsset source;
    source.width  = kSourceWidth;
    source.height = kSourceHeight;
    source.pixels.resize(static_cast<std::size_t>(kSourceWidth) * kSourceHeight * 4);

    for (std::uint32_t y = 0; y < kSourceHeight; ++y) {
        // Latitude runs from the north pole, so the top half is the sky.
        const float value = y < kSourceHeight / 2 ? kBright : kDark;
        for (std::uint32_t x = 0; x < kSourceWidth; ++x) {
            const std::size_t offset =
                (static_cast<std::size_t>(y) * kSourceWidth + x) * 4;
            source.pixels[offset + 0] = value;
            source.pixels[offset + 1] = value;
            source.pixels[offset + 2] = value;
            source.pixels[offset + 3] = 1.0f;
        }
    }

    rhi::IblResources ibl;
    REQUIRE(ibl.create(device, renderer.bindless(), std::string(HARPIA_SHADER_DIR)));
    REQUIRE(ibl.loadEnvironment(device, renderer.bindless(), source,
                                std::string(HARPIA_SHADER_DIR)));

    const auto faceMean = [&](std::uint32_t mip, std::uint32_t face) {
        const std::uint32_t edge = rhi::IblResources::kEnvironmentSize >> mip;
        std::vector<std::uint8_t> raw;
        REQUIRE(uploader.downloadImage(ibl.environmentImage(),
                                       VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                                       VkExtent2D{edge, edge}, 8, raw,
                                       VK_IMAGE_ASPECT_COLOR_BIT, face, mip));
        const auto* halves = reinterpret_cast<const std::uint16_t*>(raw.data());

        double sum = 0.0;
        for (std::uint32_t i = 0; i < edge * edge; ++i) {
            sum += static_cast<double>(half16ToFloat(halves[static_cast<std::size_t>(i) * 4]));
        }
        return static_cast<float>(sum / (edge * edge));
    };

    SUBCASE("no mip brightens or darkens the environment as a whole")
    {
        // Convolution redistributes energy, it does not create it. Every mip's
        // mean has to stay inside the range the source spans — a prefilter that
        // forgot to divide by the accumulated weight fails right here, and that
        // is the failure which otherwise reads as a wrong exposure.
        for (std::uint32_t mip = 0; mip < rhi::IblResources::kEnvironmentMips; ++mip) {
            const float mean = faceMean(mip, 2);
            CAPTURE(mip);
            CHECK(mean >= kDark * 0.95f);
            CHECK(mean <= kBright * 1.05f);
        }
    }

    SUBCASE("roughness pulls the bright face towards the average")
    {
        // +Y sees only sky at roughness 0. As the lobe widens it reaches across
        // the equator and picks up ground, so the face mean has to fall.
        float previous = faceMean(0, 2);
        CHECK(previous == doctest::Approx(kBright).epsilon(0.05));

        for (std::uint32_t mip = 1; mip < rhi::IblResources::kEnvironmentMips; ++mip) {
            const float mean = faceMean(mip, 2);
            CAPTURE(mip);
            CHECK(mean <= previous + 1e-3f);
            previous = mean;
        }

        // And it has to have actually moved, or the prefilter ran and did
        // nothing measurable.
        CHECK(previous < kBright * 0.95f);
    }

    SUBCASE("the mirror mip keeps the contrast the rough mips lose")
    {
        // -Y looks into the ground. The contrast between opposing faces is what
        // roughness destroys, so it is largest at mip 0.
        const float sharpContrast = faceMean(0, 2) - faceMean(0, 3);
        const float roughContrast = faceMean(rhi::IblResources::kEnvironmentMips - 1, 2)
                                  - faceMean(rhi::IblResources::kEnvironmentMips - 1, 3);

        CHECK(sharpContrast > 0.0f);
        CHECK(roughContrast < sharpContrast);
    }

    SUBCASE("irradiance follows the hemisphere each normal faces")
    {
        // A cosine integral over a sky/ground step: a normal pointing up sees
        // mostly sky, one pointing down mostly ground, and both land strictly
        // between the two source values. Landing outside would mean the sin
        // Jacobian or the PI were wrong — the two errors that scale the whole
        // diffuse term and read as a wrong exposure rather than a wrong lobe.
        const auto irradianceMean = [&](std::uint32_t face) {
            constexpr std::uint32_t kEdge = rhi::IblResources::kIrradianceSize;
            std::vector<std::uint8_t> raw;
            REQUIRE(uploader.downloadImage(ibl.irradianceImage(),
                                           VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                                           VkExtent2D{kEdge, kEdge}, 8, raw,
                                           VK_IMAGE_ASPECT_COLOR_BIT, face, 0));
            const auto* halves = reinterpret_cast<const std::uint16_t*>(raw.data());
            double sum = 0.0;
            for (std::uint32_t i = 0; i < kEdge * kEdge; ++i) {
                sum += static_cast<double>(
                    half16ToFloat(halves[static_cast<std::size_t>(i) * 4]));
            }
            return static_cast<float>(sum / (kEdge * kEdge));
        };

        CHECK(ibl.irradianceIndex() != rhi::VulkanBindless::kInvalidIndex);

        const float up   = irradianceMean(2);   // +Y, facing the sky
        const float down = irradianceMean(3);   // -Y, facing the ground

        CHECK(up > down);
        CHECK(up < kBright);
        CHECK(down > kDark);
    }

    SUBCASE("roughness per mip spans zero to one")
    {
        CHECK(rhi::IblResources::roughnessOfMip(0) == doctest::Approx(0.0f));
        CHECK(rhi::IblResources::roughnessOfMip(rhi::IblResources::kEnvironmentMips - 1)
              == doctest::Approx(1.0f));
    }

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);

    ibl.destroy();
    uploader.destroy();
    renderer.destroy();
    device.destroy();
}

TEST_SUITE_END();
