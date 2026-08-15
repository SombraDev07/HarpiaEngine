// Render graph behaviour: culling, transient aliasing, barrier derivation, and
// the F1 deliverable itself — a triangle drawn through a declared pass.

#include <doctest/doctest.h>

#include "ImageCompare.h"

#include "RHI/RenderGraph/RenderGraph.h"
#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanPipeline.h"
#include "RHI/Vulkan/VulkanRenderer.h"

#include <string>
#include <vector>

using namespace harpia;

namespace {

constexpr std::uint32_t kWidth  = 128;
constexpr std::uint32_t kHeight = 128;

// Owns a headless device plus renderer so each test gets a clean graph.
struct GpuFixture {
    rhi::VulkanDevice   device;
    rhi::VulkanRenderer renderer;
    rhi::RenderGraph    graph;
    bool                ready = false;

    GpuFixture()
    {
        rhi::DeviceDesc desc;
        desc.applicationName  = "HarpiaGraphTest";
        desc.enableValidation = true;
        desc.window           = nullptr;

        if (!device.create(desc)) {
            return;
        }
        if (!renderer.createOffscreen(device, kWidth, kHeight)) {
            return;
        }
        if (!graph.create(device)) {
            return;
        }
        ready = true;
    }

    ~GpuFixture()
    {
        if (ready) {
            graph.destroy();
            renderer.destroy();
            device.destroy();
        }
    }
};

rhi::RgTextureDesc colorDesc(std::uint32_t width = kWidth, std::uint32_t height = kHeight)
{
    rhi::RgTextureDesc desc;
    desc.width  = width;
    desc.height = height;
    desc.format = VK_FORMAT_R8G8B8A8_UNORM;
    return desc;
}

} // namespace

TEST_SUITE_BEGIN("gpu");

TEST_CASE("a pass whose output nobody consumes is culled")
{
    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    fixture.graph.beginFrame();

    bool deadPassRan = false;
    fixture.graph.addPass("DeadPass",
        [&](rhi::RgBuilder& builder) {
            // Writes a transient that no later pass reads and nothing imports.
            const rhi::RgHandle unused = builder.createTexture("Unused", colorDesc());
            builder.writeColor(unused, VK_ATTACHMENT_LOAD_OP_CLEAR);
        },
        [&](rhi::RgContext&) { deadPassRan = true; });

    fixture.graph.compile();

    CHECK(fixture.graph.stats().culledPasses == 1);
    CHECK(fixture.graph.stats().passes == 0);
    CHECK_FALSE(deadPassRan);
}

TEST_CASE("neverCull pins a pass the graph would otherwise drop")
{
    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    fixture.graph.beginFrame();

    fixture.graph.addPass("SideEffectPass",
        [&](rhi::RgBuilder& builder) {
            const rhi::RgHandle unused = builder.createTexture("Unused", colorDesc());
            builder.writeColor(unused, VK_ATTACHMENT_LOAD_OP_CLEAR);
            builder.neverCull();
        },
        [](rhi::RgContext&) {});

    fixture.graph.compile();

    CHECK(fixture.graph.stats().culledPasses == 0);
    CHECK(fixture.graph.stats().passes == 1);
}

TEST_CASE("a chain feeding an imported target survives culling")
{
    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    fixture.graph.beginFrame();

    const rhi::RgHandle target = fixture.graph.importTexture(
        "Target", VK_NULL_HANDLE, VK_NULL_HANDLE, VK_FORMAT_R8G8B8A8_UNORM,
        VkExtent2D{kWidth, kHeight}, VK_IMAGE_LAYOUT_UNDEFINED);

    rhi::RgHandle intermediate = rhi::kRgInvalid;

    fixture.graph.addPass("Produce",
        [&](rhi::RgBuilder& builder) {
            intermediate = builder.createTexture("Intermediate", colorDesc());
            builder.writeColor(intermediate, VK_ATTACHMENT_LOAD_OP_CLEAR);
        },
        [](rhi::RgContext&) {});

    fixture.graph.addPass("Consume",
        [&](rhi::RgBuilder& builder) {
            builder.read(intermediate, rhi::RgUsage::SampledRead);
            builder.writeColor(target, VK_ATTACHMENT_LOAD_OP_CLEAR);
        },
        [](rhi::RgContext&) {});

    fixture.graph.compile();

    // Consume writes an imported resource, so it survives; Produce survives
    // because Consume reads what it wrote.
    CHECK(fixture.graph.stats().passes == 2);
    CHECK(fixture.graph.stats().culledPasses == 0);
    CHECK(fixture.graph.stats().transients == 1);
}

TEST_CASE("transients with disjoint lifetimes share one image")
{
    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    fixture.graph.beginFrame();

    const rhi::RgHandle target = fixture.graph.importTexture(
        "Target", VK_NULL_HANDLE, VK_NULL_HANDLE, VK_FORMAT_R8G8B8A8_UNORM,
        VkExtent2D{kWidth, kHeight}, VK_IMAGE_LAYOUT_UNDEFINED);

    rhi::RgHandle first  = rhi::kRgInvalid;
    rhi::RgHandle second = rhi::kRgInvalid;

    // First lives across A and B only. Second is not created until C, so the
    // two never coexist and the image can be handed over.
    fixture.graph.addPass("A", [&](rhi::RgBuilder& builder) {
        first = builder.createTexture("First", colorDesc());
        builder.writeColor(first, VK_ATTACHMENT_LOAD_OP_CLEAR);
    }, [](rhi::RgContext&) {});

    fixture.graph.addPass("B", [&](rhi::RgBuilder& builder) {
        builder.read(first, rhi::RgUsage::SampledRead);
        builder.writeColor(target, VK_ATTACHMENT_LOAD_OP_CLEAR);
    }, [](rhi::RgContext&) {});

    fixture.graph.addPass("C", [&](rhi::RgBuilder& builder) {
        second = builder.createTexture("Second", colorDesc());
        builder.writeColor(second, VK_ATTACHMENT_LOAD_OP_CLEAR);
    }, [](rhi::RgContext&) {});

    fixture.graph.addPass("D", [&](rhi::RgBuilder& builder) {
        builder.read(second, rhi::RgUsage::SampledRead);
        builder.writeColor(target, VK_ATTACHMENT_LOAD_OP_LOAD);
    }, [](rhi::RgContext&) {});

    fixture.graph.compile();

    CHECK(fixture.graph.stats().passes == 4);
    CHECK(fixture.graph.stats().transients == 2);
    // Two transients, one physical image: that is the aliasing working.
    CHECK(fixture.graph.stats().physicalImages == 1);
    CHECK(fixture.graph.stats().aliasedImages == 1);
}

TEST_CASE("overlapping lifetimes get separate images")
{
    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    fixture.graph.beginFrame();

    const rhi::RgHandle target = fixture.graph.importTexture(
        "Target", VK_NULL_HANDLE, VK_NULL_HANDLE, VK_FORMAT_R8G8B8A8_UNORM,
        VkExtent2D{kWidth, kHeight}, VK_IMAGE_LAYOUT_UNDEFINED);

    rhi::RgHandle first  = rhi::kRgInvalid;
    rhi::RgHandle second = rhi::kRgInvalid;

    fixture.graph.addPass("A", [&](rhi::RgBuilder& builder) {
        first = builder.createTexture("First", colorDesc());
        builder.writeColor(first, VK_ATTACHMENT_LOAD_OP_CLEAR);
    }, [](rhi::RgContext&) {});

    fixture.graph.addPass("B", [&](rhi::RgBuilder& builder) {
        second = builder.createTexture("Second", colorDesc());
        builder.writeColor(second, VK_ATTACHMENT_LOAD_OP_CLEAR);
    }, [](rhi::RgContext&) {});

    // Both alive at once here, so they cannot share memory.
    fixture.graph.addPass("C", [&](rhi::RgBuilder& builder) {
        builder.read(first, rhi::RgUsage::SampledRead);
        builder.read(second, rhi::RgUsage::SampledRead);
        builder.writeColor(target, VK_ATTACHMENT_LOAD_OP_CLEAR);
    }, [](rhi::RgContext&) {});

    fixture.graph.compile();

    CHECK(fixture.graph.stats().transients == 2);
    CHECK(fixture.graph.stats().physicalImages == 2);
    CHECK(fixture.graph.stats().aliasedImages == 0);
}

TEST_CASE("F1: a triangle drawn through a graph pass")
{
    rhi::VulkanDevice::resetValidationErrorCount();

    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    rhi::GraphicsPipelineDesc pipelineDesc;
    pipelineDesc.vertexSpirvPath     = std::string(HARPIA_SHADER_DIR) + "/Triangle.vert.spv";
    pipelineDesc.fragmentSpirvPath   = std::string(HARPIA_SHADER_DIR) + "/Triangle.frag.spv";
    pipelineDesc.colorFormats        = {VK_FORMAT_R8G8B8A8_UNORM};
    pipelineDesc.descriptorSetLayout = fixture.renderer.bindless().layout();
    pipelineDesc.debugName           = "Pipeline_TriangleTest";

    rhi::VulkanPipeline pipeline;
    REQUIRE(pipeline.create(fixture.device, pipelineDesc));

    rhi::FrameInfo frame;
    REQUIRE(fixture.renderer.beginFrame(frame));

    fixture.graph.beginFrame();

    const rhi::RgHandle target = fixture.graph.importTexture(
        "SceneColor", frame.targetImage, frame.targetView,
        VK_FORMAT_R8G8B8A8_UNORM, frame.extent,
        VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL);

    VkClearValue clear{};
    clear.color = {{0.0f, 0.0f, 0.0f, 1.0f}};

    fixture.graph.addPass("TrianglePass",
        [&](rhi::RgBuilder& builder) {
            builder.writeColor(target, VK_ATTACHMENT_LOAD_OP_CLEAR, clear);
        },
        [&](rhi::RgContext& context) {
            pipeline.bind(context.cmd());
            pipeline.setViewportAndScissor(context.cmd(), frame.extent);
            vkCmdDraw(context.cmd(), 3, 1, 0, 0);
        });

    fixture.graph.compile();
    fixture.graph.execute(frame.cmd);
    fixture.renderer.endFrame();

    std::vector<std::uint8_t> pixels;
    REQUIRE(fixture.renderer.readback(pixels));
    REQUIRE(pixels.size() == std::size_t{kWidth} * kHeight * 4);

    const auto pixelAt = [&](std::uint32_t x, std::uint32_t y) {
        const std::size_t offset = (std::size_t{y} * kWidth + x) * 4;
        return std::array<std::uint8_t, 4>{pixels[offset], pixels[offset + 1],
                                           pixels[offset + 2], pixels[offset + 3]};
    };

    // Top-left corner is outside the triangle: it must be the clear colour.
    const auto corner = pixelAt(2, 2);
    CHECK(corner[0] == 0);
    CHECK(corner[1] == 0);
    CHECK(corner[2] == 0);

    // Centre is inside and lit by the interpolated vertex colours.
    const auto centre = pixelAt(kWidth / 2, kHeight / 2);
    const int centreSum = centre[0] + centre[1] + centre[2];
    CHECK(centreSum > 60);

    // Vertex colours are red at the apex, green bottom-right, blue bottom-left.
    // Sampling near each corner of the triangle pins the winding and the
    // interpolation direction, which a flipped viewport would silently break.
    const auto apex = pixelAt(kWidth / 2, kHeight / 4);
    CHECK(apex[0] > apex[1]);
    CHECK(apex[0] > apex[2]);

    const auto bottomRight = pixelAt((kWidth * 3) / 4, (kHeight * 3) / 4);
    CHECK(bottomRight[1] > bottomRight[0]);

    const auto bottomLeft = pixelAt(kWidth / 4, (kHeight * 3) / 4);
    CHECK(bottomLeft[2] > bottomLeft[0]);

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);

    pipeline.destroy();
}

TEST_CASE("the graph derives a barrier only when it is needed")
{
    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    rhi::FrameInfo frame;
    REQUIRE(fixture.renderer.beginFrame(frame));

    fixture.graph.beginFrame();

    const rhi::RgHandle target = fixture.graph.importTexture(
        "SceneColor", frame.targetImage, frame.targetView,
        VK_FORMAT_R8G8B8A8_UNORM, frame.extent,
        VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL);

    // Two consecutive colour writes to a resource already in the right layout.
    // The layout does not change, but both are writes, so a write-after-write
    // barrier is still required — dropping it would be a real race.
    fixture.graph.addPass("First", [&](rhi::RgBuilder& builder) {
        builder.writeColor(target, VK_ATTACHMENT_LOAD_OP_CLEAR);
    }, [](rhi::RgContext&) {});

    fixture.graph.addPass("Second", [&](rhi::RgBuilder& builder) {
        builder.writeColor(target, VK_ATTACHMENT_LOAD_OP_LOAD);
    }, [](rhi::RgContext&) {});

    fixture.graph.compile();
    fixture.graph.execute(frame.cmd);
    fixture.renderer.endFrame();

    CHECK(fixture.graph.stats().barriers == 2);
    CHECK(fixture.graph.finalLayout(target) == VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL);
}

TEST_SUITE_END();
