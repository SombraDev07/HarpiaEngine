// F2 step 3. GBuffer pass feeding a deferred lighting pass, checked against a
// CPU mirror of the same BRDF. Comparing the shader to an independent
// implementation is what turns "the image looks plausible" into a number.

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

constexpr std::uint32_t kWidth  = 64;
constexpr std::uint32_t kHeight = 64;

// --- CPU mirror of Shaders/Brdf.hlsli ---------------------------------------
// Written from the same formulae, not shared with the shader: two independent
// implementations agreeing is evidence, one implementation agreeing with itself
// is not.

float distributionGGX(float NoH, float alpha)
{
    const float a2 = alpha * alpha;
    const float d  = (NoH * a2 - NoH) * NoH + 1.0f;
    return a2 / std::max(kPi * d * d, 1e-7f);
}

float visibilitySmith(float NoV, float NoL, float alpha)
{
    const float a2 = alpha * alpha;
    const float lambdaV = NoL * std::sqrt((NoV - a2 * NoV) * NoV + a2);
    const float lambdaL = NoV * std::sqrt((NoL - a2 * NoL) * NoL + a2);
    return 0.5f / std::max(lambdaV + lambdaL, 1e-7f);
}

Vec3 fresnelSchlick(float VoH, Vec3 f0)
{
    const float f = std::pow(1.0f - VoH, 5.0f);
    return f0 + (Vec3(1.0f) - f0) * f;
}

float diffuseBurley(float NoV, float NoL, float LoH, float roughness)
{
    const float f90 = 0.5f + 2.0f * roughness * LoH * LoH;
    const float lightScatter = 1.0f + (f90 - 1.0f) * std::pow(1.0f - NoL, 5.0f);
    const float viewScatter  = 1.0f + (f90 - 1.0f) * std::pow(1.0f - NoV, 5.0f);
    return lightScatter * viewScatter * (1.0f / kPi);
}

Vec3 shadeDirect(Vec3 albedo, float metallic, float roughness,
                 Vec3 normal, Vec3 view, Vec3 lightDir, Vec3 lightColor)
{
    const float alpha = roughness * roughness;
    const Vec3  half  = normalize(view + lightDir);

    const float NoV = std::fabs(dot(normal, view)) + 1e-5f;
    const float NoL = std::max(dot(normal, lightDir), 0.0f);
    const float NoH = std::max(dot(normal, half), 0.0f);
    const float LoH = std::max(dot(lightDir, half), 0.0f);

    if (NoL <= 0.0f) {
        return Vec3(0.0f);
    }

    const Vec3 f0 = mix(Vec3(0.04f), albedo, metallic);
    const Vec3 diffuseColor = albedo * (1.0f - metallic);

    const float D = distributionGGX(NoH, alpha);
    const float V = visibilitySmith(NoV, NoL, alpha);
    const Vec3  F = fresnelSchlick(LoH, f0);

    const Vec3 specular = F * (D * V);
    const Vec3 diffuse  = diffuseColor * diffuseBurley(NoV, NoL, LoH, roughness)
                        * (Vec3(1.0f) - F);

    return (diffuse + specular) * lightColor * NoL;
}

// --- half float -------------------------------------------------------------

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

// Runs GBuffer + lighting and reads the HDR result back.
struct DeferredFixture {
    rhi::VulkanDevice   device;
    rhi::VulkanRenderer renderer;
    rhi::GpuUploader    uploader;
    rhi::RenderGraph    graph;
    rhi::GBufferFormats formats;
    rhi::VulkanPipeline gbufferPipeline;
    rhi::VulkanPipeline lightingPipeline;
    rhi::GpuMesh        mesh;

    rhi::VulkanBuffer frameBuffer;
    rhi::VulkanBuffer objectBuffer;
    rhi::VulkanBuffer materialBuffer;
    rhi::VulkanBuffer lightBuffer;

    rhi::GBufferPushConstants  gbufferPush;
    rhi::LightingPushConstants lightingPush;

    bool ready = false;

    DeferredFixture()
    {
        rhi::DeviceDesc desc;
        desc.applicationName  = "HarpiaLightingTest";
        desc.enableValidation = true;
        desc.window           = nullptr;

        if (!device.create(desc) || !renderer.createOffscreen(device, kWidth, kHeight)
            || !uploader.create(device) || !graph.create(device)) {
            return;
        }

        formats = rhi::GBufferFormats::select(device);

        rhi::GraphicsPipelineDesc gbufferDesc;
        gbufferDesc.vertexSpirvPath   = std::string(HARPIA_SHADER_DIR) + "/GBuffer.vert.spv";
        gbufferDesc.fragmentSpirvPath = std::string(HARPIA_SHADER_DIR) + "/GBuffer.frag.spv";
        const auto colorFormats = formats.colorFormats();
        gbufferDesc.colorFormats = {colorFormats.begin(), colorFormats.end()};
        gbufferDesc.depthFormat  = formats.depth;
        gbufferDesc.depthTest    = true;
        gbufferDesc.depthWrite   = true;
        gbufferDesc.descriptorSetLayout = renderer.bindless().layout();
        gbufferDesc.pushConstantBytes   = sizeof(rhi::GBufferPushConstants);
        gbufferDesc.debugName           = "Pipeline_GBuffer";

        rhi::GraphicsPipelineDesc lightingDesc;
        lightingDesc.vertexSpirvPath   = std::string(HARPIA_SHADER_DIR) + "/Fullscreen.vert.spv";
        lightingDesc.fragmentSpirvPath = std::string(HARPIA_SHADER_DIR) + "/Lighting.frag.spv";
        lightingDesc.colorFormats      = {VK_FORMAT_R16G16B16A16_SFLOAT};
        lightingDesc.descriptorSetLayout = renderer.bindless().layout();
        lightingDesc.pushConstantBytes   = sizeof(rhi::LightingPushConstants);
        lightingDesc.debugName           = "Pipeline_Lighting";

        if (!gbufferPipeline.create(device, gbufferDesc)
            || !lightingPipeline.create(device, lightingDesc)) {
            return;
        }

        const MeshAsset asset = makeQuad();
        if (!mesh.create(device, uploader, renderer.bindless(), asset, "LightingQuad")) {
            return;
        }
        ready = true;
    }

    ~DeferredFixture()
    {
        if (ready) {
            mesh.destroy();
            gbufferPipeline.destroy();
            lightingPipeline.destroy();
            frameBuffer.destroy();
            objectBuffer.destroy();
            materialBuffer.destroy();
            lightBuffer.destroy();
            graph.destroy();
            uploader.destroy();
            renderer.destroy();
            device.destroy();
        }
    }

    bool prepare(const rhi::GpuFrameData&         frame,
                 const rhi::GpuObjectData&        object,
                 const rhi::GpuMaterialData&      material,
                 const rhi::GpuDirectionalLight&  light)
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

        if (!make(frameBuffer, &frame, sizeof(frame), "Frame")
            || !make(objectBuffer, &object, sizeof(object), "Objects")
            || !make(materialBuffer, &material, sizeof(material), "Materials")
            || !make(lightBuffer, &light, sizeof(light), "Lights")) {
            return false;
        }

        rhi::VulkanBindless& bindless = renderer.bindless();
        gbufferPush.frameBuffer =
            bindless.registerStorageBuffer(frameBuffer.handle(), 0, sizeof(frame));
        gbufferPush.objectBuffer =
            bindless.registerStorageBuffer(objectBuffer.handle(), 0, sizeof(object));
        gbufferPush.materialBuffer =
            bindless.registerStorageBuffer(materialBuffer.handle(), 0, sizeof(material));
        gbufferPush.vertexBuffer = mesh.vertexBufferIndex();

        lightingPush.frameBuffer = gbufferPush.frameBuffer;
        lightingPush.lightBuffer =
            bindless.registerStorageBuffer(lightBuffer.handle(), 0, sizeof(light));

        return lightingPush.lightBuffer != rhi::VulkanBindless::kInvalidIndex;
    }

    // Returns the HDR lighting result as floats.
    std::vector<float> render()
    {
        rhi::FrameInfo frame;
        REQUIRE(renderer.beginFrame(frame));

        graph.beginFrame();

        rhi::GBufferHandles gbuffer;
        rhi::RgHandle       hdr = rhi::kRgInvalid;

        graph.addPass("GBufferPass",
            [&](rhi::RgBuilder& builder) {
                gbuffer = rhi::declareGBuffer(builder, formats, kWidth, kHeight);
            },
            [&](rhi::RgContext& context) {
                gbufferPipeline.bind(context.cmd());
                gbufferPipeline.setViewportAndScissor(context.cmd(),
                                                      VkExtent2D{kWidth, kHeight});
                VkDescriptorSet set = renderer.bindless().set();
                vkCmdBindDescriptorSets(context.cmd(), VK_PIPELINE_BIND_POINT_GRAPHICS,
                                        gbufferPipeline.layout(), 0, 1, &set, 0, nullptr);
                vkCmdPushConstants(context.cmd(), gbufferPipeline.layout(),
                                   VK_SHADER_STAGE_ALL, 0, sizeof(gbufferPush),
                                   &gbufferPush);
                mesh.bindIndices(context.cmd());
                mesh.drawSubMesh(context.cmd(), 0);
            });

        graph.addPass("LightingPass",
            [&](rhi::RgBuilder& builder) {
                for (const rhi::RgHandle handle : gbuffer.color) {
                    builder.read(handle, rhi::RgUsage::SampledRead);
                }
                builder.read(gbuffer.depth, rhi::RgUsage::SampledRead);

                rhi::RgTextureDesc desc;
                desc.width  = kWidth;
                desc.height = kHeight;
                desc.format = VK_FORMAT_R16G16B16A16_SFLOAT;
                hdr = builder.createTexture("SceneHdr", desc);
                builder.writeColor(hdr, VK_ATTACHMENT_LOAD_OP_CLEAR);

                // Only the final consumer needs pinning: the GBuffer pass
                // survives because this one reads it. In the real engine the
                // final target is imported from the swapchain and nothing needs
                // pinning at all.
                builder.neverCull();
            },
            [&](rhi::RgContext& context) {
                lightingPipeline.bind(context.cmd());
                lightingPipeline.setViewportAndScissor(context.cmd(),
                                                       VkExtent2D{kWidth, kHeight});
                VkDescriptorSet set = renderer.bindless().set();
                vkCmdBindDescriptorSets(context.cmd(), VK_PIPELINE_BIND_POINT_GRAPHICS,
                                        lightingPipeline.layout(), 0, 1, &set, 0, nullptr);
                vkCmdPushConstants(context.cmd(), lightingPipeline.layout(),
                                   VK_SHADER_STAGE_ALL, 0, sizeof(lightingPush),
                                   &lightingPush);
                vkCmdDraw(context.cmd(), 3, 1, 0, 0); // fullscreen triangle
            });

        graph.compile();

        // The GBuffer targets have physical images once compile() has run, so
        // this is the earliest the lighting pass can be told where to find them.
        rhi::VulkanBindless& bindless = renderer.bindless();
        const std::uint32_t albedoSlot =
            bindless.registerSampledImage(graph.viewOf(gbuffer.color[0]),
                                          VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        const std::uint32_t normalSlot =
            bindless.registerSampledImage(graph.viewOf(gbuffer.color[1]),
                                          VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        const std::uint32_t materialSlot =
            bindless.registerSampledImage(graph.viewOf(gbuffer.color[2]),
                                          VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        const std::uint32_t depthSlot =
            bindless.registerSampledImage(graph.viewOf(gbuffer.depth),
                                          VK_IMAGE_LAYOUT_DEPTH_READ_ONLY_OPTIMAL);

        lightingPush.albedoTexture   = albedoSlot;
        lightingPush.normalTexture   = normalSlot;
        lightingPush.materialTexture = materialSlot;
        lightingPush.depthTexture    = depthSlot;

        graph.execute(frame.cmd);
        renderer.endFrame();

        std::vector<std::uint8_t> raw;
        REQUIRE(uploader.downloadImage(graph.imageOf(hdr),
                                       VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
                                       VkExtent2D{kWidth, kHeight}, 8, raw));

        bindless.releaseSampledImage(albedoSlot);
        bindless.releaseSampledImage(normalSlot);
        bindless.releaseSampledImage(materialSlot);
        bindless.releaseSampledImage(depthSlot);

        std::vector<float> pixels(raw.size() / 2);
        const auto* halves = reinterpret_cast<const std::uint16_t*>(raw.data());
        for (std::size_t i = 0; i < pixels.size(); ++i) {
            pixels[i] = half16ToFloat(halves[i]);
        }
        return pixels;
    }
};

std::size_t centreTexel()
{
    return (static_cast<std::size_t>(kHeight / 2) * kWidth + kWidth / 2) * 4;
}

} // namespace

TEST_SUITE_BEGIN("gpu");

TEST_CASE("deferred lighting matches an independent CPU evaluation of the BRDF")
{
    rhi::VulkanDevice::resetValidationErrorCount();

    DeferredFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    const Vec3 eye(0.0f, 0.0f, 5.0f);
    const Mat4 view = glm::lookAt(eye, Vec3(0.0f), Vec3(0, 1, 0));
    const Mat4 projection = perspectiveReverseZ(radians(60.0f), 1.0f, 0.1f);

    rhi::GpuFrameData frame;
    frame.viewProjection     = projection * view;
    frame.prevViewProjection = frame.viewProjection;
    frame.invViewProjection  = inverse(frame.viewProjection);
    frame.cameraPosition     = Vec4(eye, 1.0f);
    frame.renderSize         = Vec2(kWidth, kHeight);
    frame.invRenderSize      = Vec2(1.0f / kWidth, 1.0f / kHeight);

    rhi::GpuObjectData object;
    object.normalMatrix = Mat4(normalMatrix(object.model));

    rhi::GpuMaterialData material;
    material.baseColorFactor = Vec4(0.8f, 0.2f, 0.2f, 1.0f);
    material.roughnessFactor = 0.4f;
    material.metallicFactor  = 0.0f;

    rhi::GpuDirectionalLight light;
    // Shining from up-and-towards-the-camera onto a quad that faces +Z.
    light.direction      = Vec4(normalize(Vec3(-0.3f, -0.4f, -1.0f)), 0.0f);
    light.colorIntensity = Vec4(1.0f, 1.0f, 1.0f, 3.0f);
    light.ambient        = Vec4(0.0f); // isolate the direct term

    REQUIRE(fixture.prepare(frame, object, material, light));

    const std::vector<float> pixels = fixture.render();
    const std::size_t texel = centreTexel();

    // The centre pixel is the quad's centre, which sits at the world origin.
    const Vec3 surface(0.0f);
    const Vec3 normal(0.0f, 0.0f, 1.0f);
    const Vec3 viewDirection = normalize(eye - surface);
    const Vec3 lightDirection = normalize(-Vec3(light.direction));

    // Roughness and metallic survive an 8-bit quantisation in the GBuffer, so
    // the reference has to use what actually got stored.
    const float storedRoughness = std::round(material.roughnessFactor * 255.0f) / 255.0f;
    const float storedMetallic  = std::round(material.metallicFactor * 255.0f) / 255.0f;
    const Vec3  storedAlbedo(std::round(material.baseColorFactor.x * 255.0f) / 255.0f,
                             std::round(material.baseColorFactor.y * 255.0f) / 255.0f,
                             std::round(material.baseColorFactor.z * 255.0f) / 255.0f);

    const Vec3 expected = shadeDirect(storedAlbedo, storedMetallic, storedRoughness,
                                      normal, viewDirection, lightDirection,
                                      Vec3(light.colorIntensity) * light.colorIntensity.w);

    CHECK(pixels[texel + 0] == doctest::Approx(expected.x).epsilon(0.02));
    CHECK(pixels[texel + 1] == doctest::Approx(expected.y).epsilon(0.02));
    CHECK(pixels[texel + 2] == doctest::Approx(expected.z).epsilon(0.02));

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);
}

TEST_CASE("the GBuffer pass survives because lighting reads it")
{
    DeferredFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    rhi::GpuFrameData frame;
    frame.invViewProjection = inverse(frame.viewProjection);
    rhi::GpuObjectData object;
    rhi::GpuMaterialData material;
    rhi::GpuDirectionalLight light;
    REQUIRE(fixture.prepare(frame, object, material, light));

    (void)fixture.render();

    // Both passes ran and nothing was culled: the dependency the graph found is
    // the GBuffer being sampled, which is what removed the need to pin it.
    CHECK(fixture.graph.stats().passes == 2);
    CHECK(fixture.graph.stats().culledPasses == 0);
    // Five GBuffer targets plus the HDR output.
    CHECK(fixture.graph.stats().transients == 6);
    // The graph derived the attachment-to-sampled transitions on its own.
    CHECK(fixture.graph.stats().barriers >= 5);
}

TEST_CASE("a rougher surface spreads its highlight and dims the peak")
{
    DeferredFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    const Vec3 eye(0.0f, 0.0f, 5.0f);
    const Mat4 view = glm::lookAt(eye, Vec3(0.0f), Vec3(0, 1, 0));
    const Mat4 projection = perspectiveReverseZ(radians(60.0f), 1.0f, 0.1f);

    rhi::GpuFrameData frame;
    frame.viewProjection     = projection * view;
    frame.prevViewProjection = frame.viewProjection;
    frame.invViewProjection  = inverse(frame.viewProjection);
    frame.cameraPosition     = Vec4(eye, 1.0f);

    rhi::GpuObjectData object;
    object.normalMatrix = Mat4(normalMatrix(object.model));

    rhi::GpuDirectionalLight light;
    // Straight down the view axis, so the centre pixel is the specular peak.
    light.direction      = Vec4(0.0f, 0.0f, -1.0f, 0.0f);
    light.colorIntensity = Vec4(1.0f, 1.0f, 1.0f, 1.0f);
    light.ambient        = Vec4(0.0f);

    rhi::GpuMaterialData material;
    material.baseColorFactor = Vec4(0.0f, 0.0f, 0.0f, 1.0f); // isolate specular
    material.metallicFactor  = 0.0f;

    material.roughnessFactor = 0.05f;
    REQUIRE(fixture.prepare(frame, object, material, light));
    const float sharpPeak = fixture.render()[centreTexel()];

    DeferredFixture rough;
    REQUIRE(rough.ready);
    material.roughnessFactor = 0.6f;
    REQUIRE(rough.prepare(frame, object, material, light));
    const float roughPeak = rough.render()[centreTexel()];

    // Same energy spread over a wider lobe means a lower peak. This is the
    // property that makes roughness read as roughness rather than as gloss.
    CHECK(sharpPeak > roughPeak);
    CHECK(roughPeak > 0.0f);
}

TEST_SUITE_END();
