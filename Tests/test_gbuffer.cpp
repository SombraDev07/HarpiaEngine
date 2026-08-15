// F2 step 2. Renders a quad into the GBuffer and reads every channel back, so
// each one is checked against a number rather than eyeballed: albedo against
// the material factor, normal against the surface it was given, roughness and
// metallic against their factors, and motion against a camera that moved a
// known amount.

#include <doctest/doctest.h>

#include "Core/Math/Math.h"
#include "RHI/GBuffer.h"
#include "RHI/GpuMesh.h"
#include "RHI/RenderTypes.h"
#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanPipeline.h"
#include "RHI/Vulkan/VulkanRenderer.h"

#include <cmath>
#include <cstring>
#include <string>
#include <vector>

using namespace harpia;

namespace {

constexpr std::uint32_t kWidth  = 128;
constexpr std::uint32_t kHeight = 128;

// A quad on the XY plane facing +Z, big enough to cover the whole viewport at
// the camera distance below.
MeshAsset makeQuad()
{
    MeshAsset mesh;
    mesh.vertices = {
        MeshVertex{Vec3(-4, -4, 0), Vec3(0, 0, 1), Vec4(1, 0, 0, 1), Vec2(0, 0)},
        MeshVertex{Vec3( 4, -4, 0), Vec3(0, 0, 1), Vec4(1, 0, 0, 1), Vec2(1, 0)},
        MeshVertex{Vec3( 4,  4, 0), Vec3(0, 0, 1), Vec4(1, 0, 0, 1), Vec2(1, 1)},
        MeshVertex{Vec3(-4,  4, 0), Vec3(0, 0, 1), Vec4(1, 0, 0, 1), Vec2(0, 1)},
    };
    mesh.indices = {0, 1, 2, 0, 2, 3};

    SubMesh sub;
    sub.indexCount = 6;
    sub.material   = 0;
    mesh.subMeshes.push_back(sub);
    mesh.materials.emplace_back();
    mesh.recomputeBounds();
    return mesh;
}

struct GBufferFixture {
    rhi::VulkanDevice   device;
    rhi::VulkanRenderer renderer;
    rhi::GpuUploader    uploader;
    rhi::RenderGraph    graph;
    rhi::GBufferFormats formats;
    rhi::VulkanPipeline pipeline;
    rhi::GpuMesh        mesh;

    rhi::VulkanBuffer frameBuffer;
    rhi::VulkanBuffer objectBuffer;
    rhi::VulkanBuffer materialBuffer;
    rhi::GBufferPushConstants push;

    bool ready = false;

    GBufferFixture()
    {
        rhi::DeviceDesc desc;
        desc.applicationName  = "HarpiaGBufferTest";
        desc.enableValidation = true;
        desc.window           = nullptr;

        if (!device.create(desc) || !renderer.createOffscreen(device, kWidth, kHeight)
            || !uploader.create(device) || !graph.create(device)) {
            return;
        }

        formats = rhi::GBufferFormats::select(device);

        rhi::GraphicsPipelineDesc pipelineDesc;
        pipelineDesc.vertexSpirvPath   = std::string(HARPIA_SHADER_DIR) + "/GBuffer.vert.spv";
        pipelineDesc.fragmentSpirvPath = std::string(HARPIA_SHADER_DIR) + "/GBuffer.frag.spv";
        const auto colorFormats = formats.colorFormats();
        pipelineDesc.colorFormats = {colorFormats.begin(), colorFormats.end()};
        pipelineDesc.depthFormat  = formats.depth;
        pipelineDesc.depthTest    = true;
        pipelineDesc.depthWrite   = true;
        pipelineDesc.descriptorSetLayout = renderer.bindless().layout();
        pipelineDesc.pushConstantBytes   = sizeof(rhi::GBufferPushConstants);
        pipelineDesc.debugName           = "Pipeline_GBuffer";

        if (!pipeline.create(device, pipelineDesc)) {
            return;
        }

        const MeshAsset asset = makeQuad();
        if (!mesh.create(device, uploader, renderer.bindless(), asset, "GBufferQuad")) {
            return;
        }
        ready = true;
    }

    ~GBufferFixture()
    {
        if (ready) {
            mesh.destroy();
            pipeline.destroy();
            frameBuffer.destroy();
            objectBuffer.destroy();
            materialBuffer.destroy();
            graph.destroy();
            uploader.destroy();
            renderer.destroy();
            device.destroy();
        }
    }

    // Uploads the three shader-visible buffers and registers them bindless.
    bool prepare(const rhi::GpuFrameData&    frame,
                 const rhi::GpuObjectData&   object,
                 const rhi::GpuMaterialData& material)
    {
        const auto make = [&](rhi::VulkanBuffer& buffer, const void* data,
                              VkDeviceSize size, const char* name) {
            rhi::BufferDesc desc;
            desc.size      = size;
            desc.usage     = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;
            desc.memory    = rhi::BufferMemory::DeviceLocal;
            desc.debugName = name;
            return buffer.create(device, desc) && uploader.upload(buffer, data, size);
        };

        if (!make(frameBuffer, &frame, sizeof(frame), "GBuffer_Frame")
            || !make(objectBuffer, &object, sizeof(object), "GBuffer_Objects")
            || !make(materialBuffer, &material, sizeof(material), "GBuffer_Materials")) {
            return false;
        }

        rhi::VulkanBindless& bindless = renderer.bindless();
        push.frameBuffer =
            bindless.registerStorageBuffer(frameBuffer.handle(), 0, sizeof(frame));
        push.objectBuffer =
            bindless.registerStorageBuffer(objectBuffer.handle(), 0, sizeof(object));
        push.materialBuffer =
            bindless.registerStorageBuffer(materialBuffer.handle(), 0, sizeof(material));
        push.vertexBuffer = mesh.vertexBufferIndex();
        push.objectIndex  = 0;

        return push.frameBuffer != rhi::VulkanBindless::kInvalidIndex
            && push.objectBuffer != rhi::VulkanBindless::kInvalidIndex
            && push.materialBuffer != rhi::VulkanBindless::kInvalidIndex;
    }

    // Runs one GBuffer pass and reads every channel back.
    struct Readback {
        std::vector<std::uint8_t> albedo;
        std::vector<std::uint8_t> normal;
        std::vector<std::uint8_t> material;
        std::vector<std::uint8_t> motion;
    };

    Readback render()
    {
        rhi::FrameInfo frame;
        REQUIRE(renderer.beginFrame(frame));

        graph.beginFrame();

        rhi::GBufferHandles handles;
        VkImage images[4]{};

        graph.addPass("GBufferPass",
            [&](rhi::RgBuilder& builder) {
                handles = rhi::declareGBuffer(builder, formats, kWidth, kHeight);
                // Nothing in the graph reads these targets — the readback below
                // is a side effect it cannot see, so the pass must be pinned or
                // culling removes it. That culling is correct, not a bug.
                builder.neverCull();
            },
            [&](rhi::RgContext& context) {
                pipeline.bind(context.cmd());
                pipeline.setViewportAndScissor(context.cmd(), VkExtent2D{kWidth, kHeight});

                VkDescriptorSet set = renderer.bindless().set();
                vkCmdBindDescriptorSets(context.cmd(), VK_PIPELINE_BIND_POINT_GRAPHICS,
                                        pipeline.layout(), 0, 1, &set, 0, nullptr);
                vkCmdPushConstants(context.cmd(), pipeline.layout(), VK_SHADER_STAGE_ALL,
                                   0, sizeof(push), &push);

                mesh.bindIndices(context.cmd());
                mesh.drawSubMesh(context.cmd(), 0);

                for (std::size_t i = 0; i < 4; ++i) {
                    images[i] = context.image(handles.color[i]);
                }
            });

        graph.compile();
        graph.execute(frame.cmd);
        renderer.endFrame();

        Readback result;
        const VkExtent2D extent{kWidth, kHeight};
        constexpr VkImageLayout kAfterPass = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;

        const auto texelBytes = [](VkFormat format) -> std::uint32_t {
            switch (format) {
                case VK_FORMAT_R8G8_UNORM: return 2;
                case VK_FORMAT_R16G16_SNORM:
                case VK_FORMAT_R16G16_SFLOAT:
                case VK_FORMAT_R8G8B8A8_UNORM:
                case VK_FORMAT_B8G8R8A8_UNORM: return 4;
                default: return 8;
            }
        };

        REQUIRE(uploader.downloadImage(images[0], kAfterPass, extent,
                                       texelBytes(formats.albedo), result.albedo));
        REQUIRE(uploader.downloadImage(images[1], kAfterPass, extent,
                                       texelBytes(formats.normal), result.normal));
        REQUIRE(uploader.downloadImage(images[2], kAfterPass, extent,
                                       texelBytes(formats.material), result.material));
        REQUIRE(uploader.downloadImage(images[3], kAfterPass, extent,
                                       texelBytes(formats.motion), result.motion));
        return result;
    }
};

// The centre texel, which the quad always covers.
std::size_t centreTexel()
{
    return static_cast<std::size_t>(kHeight / 2) * kWidth + kWidth / 2;
}

float snorm16ToFloat(std::int16_t value)
{
    return std::max(static_cast<float>(value) / 32767.0f, -1.0f);
}

float half16ToFloat(std::uint16_t bits)
{
    const std::uint32_t sign     = static_cast<std::uint32_t>(bits >> 15) << 31;
    const std::uint32_t exponent = (bits >> 10) & 0x1Fu;
    const std::uint32_t mantissa = bits & 0x3FFu;

    std::uint32_t out = 0;
    if (exponent == 0) {
        out = sign; // zero or subnormal, near enough for these magnitudes
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

TEST_CASE("GBuffer formats resolve to something the device supports")
{
    rhi::DeviceDesc desc;
    desc.window = nullptr;

    rhi::VulkanDevice device;
    if (!device.create(desc)) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    const rhi::GBufferFormats formats = rhi::GBufferFormats::select(device);

    CHECK(formats.albedo != VK_FORMAT_UNDEFINED);
    CHECK(formats.normal != VK_FORMAT_UNDEFINED);
    CHECK(formats.material != VK_FORMAT_UNDEFINED);
    CHECK(formats.motion != VK_FORMAT_UNDEFINED);
    CHECK(formats.depth != VK_FORMAT_UNDEFINED);
    CHECK(formats.colorFormats().size() == 4);

    device.destroy();
}

TEST_CASE("a quad writes the expected value into every GBuffer channel")
{
    rhi::VulkanDevice::resetValidationErrorCount();

    GBufferFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    // The normal format has to be the signed one for the checks below to mean
    // what they say; if a device ever falls back, the test should say so.
    REQUIRE(fixture.formats.normal == VK_FORMAT_R16G16_SNORM);

    const Mat4 view = glm::lookAt(Vec3(0, 0, 5), Vec3(0, 0, 0), Vec3(0, 1, 0));
    const Mat4 projection = perspectiveReverseZ(radians(60.0f), 1.0f, 0.1f);

    rhi::GpuFrameData frame;
    frame.viewProjection     = projection * view;
    frame.prevViewProjection = frame.viewProjection; // camera did not move
    frame.cameraPosition     = Vec4(0, 0, 5, 1);
    frame.renderSize         = Vec2(kWidth, kHeight);
    frame.invRenderSize      = Vec2(1.0f / kWidth, 1.0f / kHeight);

    rhi::GpuObjectData object;
    object.model         = Mat4(1.0f);
    object.prevModel     = Mat4(1.0f);
    object.normalMatrix  = Mat4(normalMatrix(object.model));
    object.materialIndex = 0;

    rhi::GpuMaterialData material;
    material.baseColorFactor = Vec4(0.25f, 0.5f, 0.75f, 1.0f);
    material.roughnessFactor = 0.75f;
    material.metallicFactor  = 0.25f;

    REQUIRE(fixture.prepare(frame, object, material));

    const GBufferFixture::Readback readback = fixture.render();

    const std::size_t texel = centreTexel();

    SUBCASE("albedo carries the base colour factor")
    {
        const std::uint8_t* pixels = readback.albedo.data() + texel * 4;
        CHECK(pixels[0] == doctest::Approx(0.25f * 255.0f).epsilon(0.02));
        CHECK(pixels[1] == doctest::Approx(0.5f * 255.0f).epsilon(0.02));
        CHECK(pixels[2] == doctest::Approx(0.75f * 255.0f).epsilon(0.02));
    }

    SUBCASE("the normal decodes back to the surface it was given")
    {
        const auto* encoded =
            reinterpret_cast<const std::int16_t*>(readback.normal.data()) + texel * 2;

        const Vec2 octahedral{snorm16ToFloat(encoded[0]), snorm16ToFloat(encoded[1])};
        const Vec3 decoded = decodeOctahedral(octahedral);

        // The quad faces +Z, so that is what must come back out.
        CHECK(decoded.z == doctest::Approx(1.0f).epsilon(0.01));
        CHECK(std::fabs(decoded.x) < 0.02f);
        CHECK(std::fabs(decoded.y) < 0.02f);
    }

    SUBCASE("roughness and metallic land in their channels, in that order")
    {
        const std::uint8_t* pixels = readback.material.data() + texel * 2;
        CHECK(pixels[0] == doctest::Approx(0.75f * 255.0f).epsilon(0.02)); // roughness
        CHECK(pixels[1] == doctest::Approx(0.25f * 255.0f).epsilon(0.02)); // metallic
    }

    SUBCASE("a still camera on a still object produces no motion")
    {
        const auto* motion =
            reinterpret_cast<const std::uint16_t*>(readback.motion.data()) + texel * 2;

        CHECK(std::fabs(half16ToFloat(motion[0])) < 1e-3f);
        CHECK(std::fabs(half16ToFloat(motion[1])) < 1e-3f);
    }

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);
}

TEST_CASE("a camera that moved leaves a motion vector of the right sign and size")
{
    rhi::VulkanDevice::resetValidationErrorCount();

    GBufferFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    const Mat4 projection = perspectiveReverseZ(radians(60.0f), 1.0f, 0.1f);
    const Mat4 view       = glm::lookAt(Vec3(0, 0, 5), Vec3(0, 0, 0), Vec3(0, 1, 0));
    // Last frame the camera sat to the left; this frame it has moved right.
    // A camera moving right sweeps the world left across the screen, so a
    // static surface carries motion in -X.
    const Mat4 prevView   = glm::lookAt(Vec3(-0.5f, 0, 5), Vec3(-0.5f, 0, 0), Vec3(0, 1, 0));

    rhi::GpuFrameData frame;
    frame.viewProjection     = projection * view;
    frame.prevViewProjection = projection * prevView;
    frame.renderSize         = Vec2(kWidth, kHeight);
    frame.invRenderSize      = Vec2(1.0f / kWidth, 1.0f / kHeight);

    rhi::GpuObjectData object;
    object.normalMatrix = Mat4(normalMatrix(object.model));

    rhi::GpuMaterialData material;
    REQUIRE(fixture.prepare(frame, object, material));

    const GBufferFixture::Readback readback = fixture.render();

    const auto* motion =
        reinterpret_cast<const std::uint16_t*>(readback.motion.data()) + centreTexel() * 2;
    const float motionX = half16ToFloat(motion[0]);
    const float motionY = half16ToFloat(motion[1]);

    CHECK(motionX < -0.01f);
    // Nothing moved vertically.
    CHECK(std::fabs(motionY) < 1e-2f);

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);
}

TEST_CASE("the background stays cleared where no geometry covers it")
{
    GBufferFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    const Mat4 projection = perspectiveReverseZ(radians(60.0f), 1.0f, 0.1f);
    const Mat4 view       = glm::lookAt(Vec3(0, 0, 5), Vec3(0, 0, 0), Vec3(0, 1, 0));

    rhi::GpuFrameData frame;
    frame.viewProjection     = projection * view;
    frame.prevViewProjection = frame.viewProjection;

    // Push the quad far off to the side so it misses the viewport entirely.
    rhi::GpuObjectData object;
    object.model        = glm::translate(Mat4(1.0f), Vec3(1000.0f, 0.0f, 0.0f));
    object.prevModel    = object.model;
    object.normalMatrix = Mat4(normalMatrix(object.model));

    rhi::GpuMaterialData material;
    material.baseColorFactor = Vec4(1.0f);
    REQUIRE(fixture.prepare(frame, object, material));

    const GBufferFixture::Readback readback = fixture.render();

    const std::uint8_t* albedo = readback.albedo.data() + centreTexel() * 4;
    CHECK(albedo[0] == 0);
    CHECK(albedo[1] == 0);
    CHECK(albedo[2] == 0);
}

TEST_SUITE_END();
