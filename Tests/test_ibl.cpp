// The split-sum BRDF table is pure maths with no free parameters, so it gets
// checked against an independent CPU integration of the same integral plus the
// analytic properties the table must satisfy at its edges.

#include <doctest/doctest.h>

#include "Core/Math/Math.h"
#include "RHI/IblResources.h"
#include "RHI/Vulkan/VulkanBuffer.h"
#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanRenderer.h"

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

TEST_SUITE_END();
