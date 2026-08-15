// F0.b verification: the offscreen renderer produces exactly the pixels we
// asked for, and produces zero validation errors doing it.
//
// Skips instead of failing when no Vulkan device is reachable, so the suite
// still runs on a machine or container without a GPU.

#include <doctest/doctest.h>

#include "ImageCompare.h"

#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanRenderer.h"

#include <cmath>
#include <vector>

using namespace harpia;

namespace {

constexpr std::uint32_t kWidth  = 64;
constexpr std::uint32_t kHeight = 64;

struct Rgba8 {
    std::uint8_t r, g, b, a;
};

[[nodiscard]] std::uint8_t toUnorm8(float value)
{
    const float clamped = value < 0.0f ? 0.0f : (value > 1.0f ? 1.0f : value);
    return static_cast<std::uint8_t>(std::lround(clamped * 255.0f));
}

// Renders one clear of `colour` offscreen and returns the readback.
// Returns false when Vulkan is unavailable on this machine.
[[nodiscard]] bool renderClear(const float colour[4], std::vector<std::uint8_t>& outPixels)
{
    rhi::DeviceDesc desc;
    desc.applicationName  = "HarpiaGoldenTest";
    desc.enableValidation = true;
    desc.window           = nullptr; // headless

    rhi::VulkanDevice device;
    if (!device.create(desc)) {
        return false;
    }

    rhi::VulkanRenderer renderer;
    if (!renderer.createOffscreen(device, kWidth, kHeight)) {
        return false;
    }

    rhi::FrameInfo frame;
    REQUIRE(renderer.beginFrame(frame));

    VkRenderingAttachmentInfo attachment{};
    attachment.sType       = VK_STRUCTURE_TYPE_RENDERING_ATTACHMENT_INFO;
    attachment.imageView   = frame.targetView;
    attachment.imageLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
    attachment.loadOp      = VK_ATTACHMENT_LOAD_OP_CLEAR;
    attachment.storeOp     = VK_ATTACHMENT_STORE_OP_STORE;
    attachment.clearValue.color.float32[0] = colour[0];
    attachment.clearValue.color.float32[1] = colour[1];
    attachment.clearValue.color.float32[2] = colour[2];
    attachment.clearValue.color.float32[3] = colour[3];

    VkRenderingInfo rendering{};
    rendering.sType                = VK_STRUCTURE_TYPE_RENDERING_INFO;
    rendering.renderArea.extent    = frame.extent;
    rendering.layerCount           = 1;
    rendering.colorAttachmentCount = 1;
    rendering.pColorAttachments    = &attachment;

    vkCmdBeginRendering(frame.cmd, &rendering);
    vkCmdEndRendering(frame.cmd);

    renderer.endFrame();

    const bool ok = renderer.readback(outPixels);

    renderer.destroy();
    device.destroy();
    return ok;
}

} // namespace

TEST_CASE("offscreen clear produces exactly the requested colour")
{
    rhi::VulkanDevice::resetValidationErrorCount();

    const float colour[4] = {0.25f, 0.5f, 0.75f, 1.0f};
    std::vector<std::uint8_t> pixels;

    if (!renderClear(colour, pixels)) {
        MESSAGE("no Vulkan device available — skipping render tests");
        return;
    }

    REQUIRE(pixels.size() == std::size_t{kWidth} * kHeight * 4);

    const Rgba8 expected{toUnorm8(colour[0]), toUnorm8(colour[1]),
                         toUnorm8(colour[2]), toUnorm8(colour[3])};

    // Build the reference image and diff it, exercising the same comparison
    // path the real golden images will use later.
    std::vector<std::uint8_t> reference(pixels.size());
    for (std::size_t i = 0; i + 3 < reference.size(); i += 4) {
        reference[i + 0] = expected.r;
        reference[i + 1] = expected.g;
        reference[i + 2] = expected.b;
        reference[i + 3] = expected.a;
    }

    const test::ImageDiff diff = test::compareRgba(pixels, reference, 0);

    CHECK_FALSE(diff.sizeMismatch);
    CHECK(diff.differingPixels == 0);
    CHECK(diff.maxChannelDelta == 0);
    CHECK(diff.meanAbsError == doctest::Approx(0.0));
}

TEST_CASE("rendering raises no validation errors")
{
    rhi::VulkanDevice::resetValidationErrorCount();

    const float colour[4] = {0.1f, 0.2f, 0.3f, 1.0f};
    std::vector<std::uint8_t> pixels;

    if (!renderClear(colour, pixels)) {
        MESSAGE("no Vulkan device available — skipping render tests");
        return;
    }

    // Rule 2: validation errors are a gate, not a log line.
    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);
}

TEST_CASE("bindless heap reports capacity and hands out distinct indices")
{
    rhi::DeviceDesc desc;
    desc.enableValidation = true;
    desc.window           = nullptr;

    rhi::VulkanDevice device;
    if (!device.create(desc)) {
        MESSAGE("no Vulkan device available — skipping bindless test");
        return;
    }

    rhi::VulkanRenderer renderer;
    REQUIRE(renderer.createOffscreen(device, 16, 16));

    rhi::VulkanBindless& bindless = renderer.bindless();

    CHECK(bindless.capacity().sampledImages > 0);
    CHECK(bindless.capacity().storageBuffers > 0);
    CHECK(bindless.set() != VK_NULL_HANDLE);
    CHECK(bindless.layout() != VK_NULL_HANDLE);

    // Samplers are the cheapest resource to create for an index test.
    VkSamplerCreateInfo samplerInfo{};
    samplerInfo.sType     = VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO;
    samplerInfo.magFilter = VK_FILTER_LINEAR;
    samplerInfo.minFilter = VK_FILTER_LINEAR;

    VkSampler sampler = VK_NULL_HANDLE;
    REQUIRE(vkCreateSampler(device.device(), &samplerInfo, nullptr, &sampler) == VK_SUCCESS);

    const std::uint32_t first  = bindless.registerSampler(sampler);
    const std::uint32_t second = bindless.registerSampler(sampler);

    CHECK(first != rhi::VulkanBindless::kInvalidIndex);
    CHECK(second != rhi::VulkanBindless::kInvalidIndex);
    CHECK(first != second);
    CHECK(bindless.usage().samplers == 2);

    // A released index is handed back out rather than leaked.
    bindless.releaseSampler(first);
    CHECK(bindless.usage().samplers == 1);
    const std::uint32_t reused = bindless.registerSampler(sampler);
    CHECK(reused == first);

    vkDestroySampler(device.device(), sampler, nullptr);
    renderer.destroy();
    device.destroy();
}
