// Harpia Engine — a real file, drawn
//
// The sphere grid next door validates the BRDF against something a person can
// judge by eye. This one validates the part no synthetic scene can: that an
// asset authored elsewhere survives import, that its materials find their
// textures, that its scale and winding and tangent space agree with ours.
//
// Those are the failures that hide until they meet each other. A model at the
// wrong scale looks like a broken camera; a flipped tangent looks like a
// broken normal map; a material whose texture never resolved looks like an
// artist chose flat grey. Each is cheap alone and expensive together, which is
// why this sample exists before Sponza rather than after.
//
//   HarpiaGltfViewer --scene path/to/model.gltf [--hdr path/to/env.hdr]
//
// Full frame: GBuffer -> lighting -> tonemap, every pass declared in the render
// graph, every barrier derived from those declarations.

#include "Core/Math/Math.h"
#include "Core/Threading/JobSystem.h"
#include "Platform/Window.h"
#include "RHI/GBuffer.h"
#include "Core/Assets/AssetDatabase.h"
#include "Core/Assets/AssetManager.h"
#include "Core/Assets/GltfLoader.h"
#include "Core/Assets/TextureLoader.h"
#include "RHI/GpuScene.h"
#include "RHI/IblResources.h"
#include "RHI/RenderTypes.h"
#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanPipeline.h"
#include "RHI/Vulkan/VulkanRenderer.h"

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <string>
#include <vector>

using namespace harpia;

namespace {

struct Options {
    bool          headless = false;
    std::uint32_t frames   = 0;
    std::uint32_t width    = 1280;
    std::uint32_t height   = 720;
    std::string   output;
    std::string   scene;
    std::string   hdr;
};

Options parseArgs(int argc, char** argv)
{
    Options options;
    for (int i = 1; i < argc; ++i) {
        const char* arg = argv[i];
        const auto next = [&](std::uint32_t fallback) -> std::uint32_t {
            return (i + 1 < argc) ? static_cast<std::uint32_t>(std::atoi(argv[++i])) : fallback;
        };
        if (std::strcmp(arg, "--headless") == 0)    { options.headless = true; }
        else if (std::strcmp(arg, "--frames") == 0) { options.frames = next(1); }
        else if (std::strcmp(arg, "--width") == 0)  { options.width = next(options.width); }
        else if (std::strcmp(arg, "--height") == 0) { options.height = next(options.height); }
        else if (std::strcmp(arg, "--scene") == 0 && i + 1 < argc) {
            options.scene = argv[++i];
        }
        else if (std::strcmp(arg, "--hdr") == 0 && i + 1 < argc) {
            options.hdr = argv[++i];
        }
        else if (std::strcmp(arg, "--output") == 0 && i + 1 < argc) {
            options.output   = argv[++i];
            options.headless = true;
        }
    }
    // Rendering to a file and never stopping is not a mode anyone wants. One
    // frame is the honest default for a capture.
    if (!options.output.empty() && options.frames == 0) {
        options.frames = 1;
    }
    return options;
}

struct TonemapPush {
    std::uint32_t frameBuffer = 0;
    std::uint32_t hdrTexture  = 0;
    float         exposure    = 1.0f;
    std::uint32_t padding     = 0;
};

} // namespace

int main(int argc, char** argv)
{
    const Options options = parseArgs(argc, argv);

    JobSystem::get().init();

    Window window;
    if (!options.headless && Window::platformAvailable()) {
        WindowDesc desc;
        desc.width  = options.width;
        desc.height = options.height;
        desc.title  = "Harpia — Deferred PBR";
        (void)window.create(desc);
    }
    const bool windowed = window.nativeHandle() != nullptr;

    rhi::DeviceDesc deviceDesc;
    deviceDesc.applicationName  = "Harpia Deferred";
    deviceDesc.enableValidation = true;
    deviceDesc.window           = windowed ? window.nativeHandle() : nullptr;

    rhi::VulkanDevice device;
    if (!device.create(deviceDesc)) {
        JobSystem::get().shutdown();
        return 1;
    }
    std::printf("[app] gpu: %s\n", device.deviceName().c_str());

    rhi::VulkanRenderer renderer;
    const bool rendererOk = windowed
        ? renderer.create(device, window.width(), window.height())
        : renderer.createOffscreen(device, options.width, options.height);
    if (!rendererOk) {
        JobSystem::get().shutdown();
        return 1;
    }

    rhi::GpuUploader uploader;
    rhi::RenderGraph graph;
    if (!uploader.create(device) || !graph.create(device)) {
        JobSystem::get().shutdown();
        return 1;
    }

    const rhi::GBufferFormats formats = rhi::GBufferFormats::select(device);
    const VkFormat swapFormat = windowed ? VK_FORMAT_B8G8R8A8_UNORM : VK_FORMAT_R8G8B8A8_UNORM;

    // --- pipelines ----------------------------------------------------------
    const std::string shaderDir = HARPIA_SHADER_DIR;

    rhi::GraphicsPipelineDesc gbufferDesc;
    gbufferDesc.vertexSpirvPath   = shaderDir + "/GBuffer.vert.spv";
    gbufferDesc.fragmentSpirvPath = shaderDir + "/GBuffer.frag.spv";
    const auto colorFormats = formats.colorFormats();
    gbufferDesc.colorFormats = {colorFormats.begin(), colorFormats.end()};
    gbufferDesc.depthFormat  = formats.depth;
    gbufferDesc.depthTest    = true;
    gbufferDesc.depthWrite   = true;
    gbufferDesc.cullMode     = VK_CULL_MODE_BACK_BIT;
    gbufferDesc.descriptorSetLayout = renderer.bindless().layout();
    gbufferDesc.pushConstantBytes   = sizeof(rhi::GBufferPushConstants);
    gbufferDesc.debugName           = "Pipeline_GBuffer";

    rhi::GraphicsPipelineDesc lightingDesc;
    lightingDesc.vertexSpirvPath   = shaderDir + "/Fullscreen.vert.spv";
    lightingDesc.fragmentSpirvPath = shaderDir + "/Lighting.frag.spv";
    lightingDesc.colorFormats      = {VK_FORMAT_R16G16B16A16_SFLOAT};
    lightingDesc.descriptorSetLayout = renderer.bindless().layout();
    lightingDesc.pushConstantBytes   = sizeof(rhi::LightingPushConstants);
    lightingDesc.debugName           = "Pipeline_Lighting";

    rhi::GraphicsPipelineDesc tonemapDesc;
    tonemapDesc.vertexSpirvPath   = shaderDir + "/Fullscreen.vert.spv";
    tonemapDesc.fragmentSpirvPath = shaderDir + "/Tonemap.frag.spv";
    tonemapDesc.colorFormats      = {swapFormat};
    tonemapDesc.descriptorSetLayout = renderer.bindless().layout();
    tonemapDesc.pushConstantBytes   = sizeof(TonemapPush);
    tonemapDesc.debugName           = "Pipeline_Tonemap";

    rhi::VulkanPipeline gbufferPipeline;
    rhi::VulkanPipeline lightingPipeline;
    rhi::VulkanPipeline tonemapPipeline;
    if (!gbufferPipeline.create(device, gbufferDesc)
        || !lightingPipeline.create(device, lightingDesc)
        || !tonemapPipeline.create(device, tonemapDesc)) {
        std::fprintf(stderr, "[app] pipeline creation failed\n");
        JobSystem::get().shutdown();
        return 1;
    }

    // --- scene --------------------------------------------------------------
    if (options.scene.empty()) {
        std::fprintf(stderr, "usage: HarpiaGltfViewer --scene <file.gltf> [--hdr <env.hdr>]\n");
        JobSystem::get().shutdown();
        return 1;
    }

    const std::filesystem::path scenePath(options.scene);

    // The database is what turns a texture URI inside the glTF into a GUID the
    // material can hold. Scanning the model's own directory is the convention
    // every DCC export already follows.
    AssetDatabase database;
    AssetManager  manager;
    const bool haveDatabase = database.open(scenePath.parent_path());
    if (haveDatabase) {
        database.scan();
        manager.attach(&database);
        registerTextureLoader(manager);
    } else {
        std::fprintf(stderr, "[app] no asset database beside the scene; "
                             "materials will fall back to their factors\n");
    }

    const GltfImportResult imported =
        importGltf(scenePath, haveDatabase ? &database : nullptr);
    if (!imported) {
        std::fprintf(stderr, "[app] %s\n", imported.error.c_str());
        JobSystem::get().shutdown();
        return 1;
    }

    rhi::GpuScene scene;
    if (!scene.create(device, uploader, renderer.bindless(), *imported.mesh,
                      haveDatabase ? &manager : nullptr, "Scene")) {
        std::fprintf(stderr, "[app] the scene could not be made resident\n");
        JobSystem::get().shutdown();
        return 1;
    }

    const std::vector<rhi::GpuSubMesh>& subMeshes = scene.mesh().subMeshes();

    // One draw per submesh, each pointing at the material its primitive named.
    // A submesh with no material falls to slot 0, which GpuScene guarantees
    // exists even for a mesh that declared none.
    std::vector<rhi::GpuObjectData> objects(subMeshes.size());
    for (std::size_t i = 0; i < subMeshes.size(); ++i) {
        objects[i].model         = Mat4(1.0f);
        objects[i].prevModel     = objects[i].model;
        objects[i].normalMatrix  = Mat4(normalMatrix(objects[i].model));
        objects[i].materialIndex = subMeshes[i].material >= 0
            ? static_cast<std::uint32_t>(subMeshes[i].material) : 0u;
    }

    // Frame the model from its own bounds rather than from a constant. A viewer
    // that assumes a scale renders every asset but the one it was tuned on as
    // either a speck or a wall.
    const AABB&  bounds = scene.mesh().bounds();
    const Vec3   centre = (bounds.min + bounds.max) * 0.5f;
    const Vec3   extent = bounds.max - bounds.min;
    const float  radius = std::max(std::max(extent.x, extent.y), extent.z) * 0.5f;
    const float  distance = std::max(radius * 2.5f, 0.1f);

    std::printf("[app] %s: %zu submeshes, %zu materials, %zu textures\n",
                scenePath.filename().string().c_str(),
                subMeshes.size(), scene.materialCount(), scene.textureCount());

    const auto makeBuffer = [&](rhi::VulkanBuffer& buffer, const void* data,
                                VkDeviceSize size, const char* name) {
        rhi::BufferDesc desc;
        desc.size      = size;
        desc.usage     = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;
        desc.memory    = rhi::BufferMemory::DeviceLocal;
        desc.debugName = name;
        return buffer.create(device, desc) && uploader.upload(buffer, data, size);
    };

    rhi::VulkanBuffer frameBuffer;
    rhi::VulkanBuffer objectBuffer;
    rhi::VulkanBuffer lightBuffer;
    rhi::VulkanBuffer environmentBuffer;

    rhi::GpuFrameData        frameData;
    rhi::GpuDirectionalLight light;
    light.direction      = Vec4(normalize(Vec3(-0.4f, -0.5f, -1.0f)), 0.0f);
    light.colorIntensity = Vec4(1.0f, 0.96f, 0.9f, 4.0f);
    light.ambient        = Vec4(0.03f, 0.035f, 0.045f, 0.0f);

    // The BRDF table is generated once and read by every material forever.
    rhi::IblResources ibl;
    if (!ibl.create(device, renderer.bindless(), shaderDir)) {
        std::fprintf(stderr, "[app] IBL resource creation failed\n");
        JobSystem::get().shutdown();
        return 1;
    }

    rhi::GpuEnvironment environment;
    environment.skyZenith   = Vec4(0.16f, 0.28f, 0.55f, 1.0f);
    environment.skyHorizon  = Vec4(0.52f, 0.58f, 0.66f, 0.0f);
    environment.groundColor = Vec4(0.10f, 0.09f, 0.08f, 0.0f);
    environment.brdfLut     = ibl.brdfLutIndex();

    // An .hdr replaces the analytic sky. Without one the fallback still lights
    // the scene, which is the difference between "no environment" and "black".
    if (!options.hdr.empty()) {
        const HdrImportResult radiance = importHdrImage(options.hdr);
        if (!radiance) {
            std::fprintf(stderr, "[app] %s\n", radiance.error.c_str());
        } else if (!ibl.loadEnvironment(device, renderer.bindless(), *radiance.image,
                                        shaderDir)) {
            std::fprintf(stderr, "[app] the environment could not be prefiltered\n");
        } else {
            environment.environmentCube = ibl.environmentIndex();
            environment.irradianceCube  = ibl.irradianceIndex();
            std::printf("[app] environment: %s\n", options.hdr.c_str());
        }
    }

    if (!makeBuffer(environmentBuffer, &environment, sizeof(environment), "Environment")
        || !makeBuffer(frameBuffer, &frameData, sizeof(frameData), "Frame")
        || !makeBuffer(objectBuffer, objects.data(),
                       sizeof(rhi::GpuObjectData) * objects.size(), "Objects")
        || !makeBuffer(lightBuffer, &light, sizeof(light), "Lights")) {
        JobSystem::get().shutdown();
        return 1;
    }

    rhi::VulkanBindless& bindless = renderer.bindless();

    rhi::GBufferPushConstants gbufferPush;
    gbufferPush.frameBuffer =
        bindless.registerStorageBuffer(frameBuffer.handle(), 0, sizeof(frameData));
    gbufferPush.objectBuffer = bindless.registerStorageBuffer(
        objectBuffer.handle(), 0, sizeof(rhi::GpuObjectData) * objects.size());
    // The scene owns its material buffer; it was built while resolving textures.
    gbufferPush.materialBuffer = scene.materialBufferIndex();
    gbufferPush.vertexBuffer   = scene.mesh().vertexBufferIndex();

    rhi::LightingPushConstants lightingPush;
    lightingPush.frameBuffer = gbufferPush.frameBuffer;
    lightingPush.lightBuffer =
        bindless.registerStorageBuffer(lightBuffer.handle(), 0, sizeof(light));
    lightingPush.environmentBuffer =
        bindless.registerStorageBuffer(environmentBuffer.handle(), 0, sizeof(environment));


    // --- frame loop ---------------------------------------------------------
    std::uint32_t rendered = 0;
    bool          reported = false;

    while (true) {
        if (windowed) {
            window.pollEvents();
            if (window.shouldClose()) {
                break;
            }
            if (window.consumeResized()) {
                renderer.onResize(window.width(), window.height());
            }
            if (window.minimised()) {
                continue;
            }
        }

        rhi::FrameInfo frame;
        if (!renderer.beginFrame(frame)) {
            continue;
        }

        const float aspect = static_cast<float>(frame.extent.width)
                           / static_cast<float>(frame.extent.height);
        // Framed from the model's own bounds, so an asset authored in metres
        // and one authored in centimetres both fill the view.
        const Vec3 eye = centre + Vec3(0.0f, radius * 0.4f, distance);

        const Mat4 view = glm::lookAt(eye, centre, Vec3(0, 1, 0));
        const Mat4 projection = perspectiveReverseZ(radians(45.0f), aspect, 0.1f);

        frameData.prevViewProjection = frameData.viewProjection;
        frameData.viewProjection     = projection * view;
        frameData.invViewProjection  = inverse(frameData.viewProjection);
        frameData.cameraPosition     = Vec4(eye, 1.0f);
        frameData.renderSize    = Vec2(frame.extent.width, frame.extent.height);
        frameData.invRenderSize = Vec2(1.0f / frameData.renderSize.x,
                                       1.0f / frameData.renderSize.y);
        (void)uploader.upload(frameBuffer, &frameData, sizeof(frameData));

        graph.beginFrame();

        const rhi::RgHandle target = graph.importTexture(
            "SwapchainColor", frame.targetImage, frame.targetView,
            swapFormat, frame.extent, VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL);

        rhi::GBufferHandles gbuffer;
        rhi::RgHandle       hdr = rhi::kRgInvalid;

        graph.addPass("GBuffer",
            [&](rhi::RgBuilder& builder) {
                gbuffer = rhi::declareGBuffer(builder, formats,
                                              frame.extent.width, frame.extent.height);
            },
            [&](rhi::RgContext& context) {
                gbufferPipeline.bind(context.cmd());
                gbufferPipeline.setViewportAndScissor(context.cmd(), frame.extent);

                VkDescriptorSet set = bindless.set();
                vkCmdBindDescriptorSets(context.cmd(), VK_PIPELINE_BIND_POINT_GRAPHICS,
                                        gbufferPipeline.layout(), 0, 1, &set, 0, nullptr);
                scene.mesh().bindIndices(context.cmd());

                // One draw per submesh. GPU-driven indirect replaces this loop
                // in F7; the data layout it needs is already in place.
                for (std::size_t i = 0; i < subMeshes.size(); ++i) {
                    gbufferPush.objectIndex = static_cast<std::uint32_t>(i);
                    vkCmdPushConstants(context.cmd(), gbufferPipeline.layout(),
                                       VK_SHADER_STAGE_ALL, 0, sizeof(gbufferPush),
                                       &gbufferPush);
                    scene.mesh().drawSubMesh(context.cmd(), i);
                }
            });

        graph.addPass("Lighting",
            [&](rhi::RgBuilder& builder) {
                for (const rhi::RgHandle handle : gbuffer.color) {
                    builder.read(handle, rhi::RgUsage::SampledRead);
                }
                builder.read(gbuffer.depth, rhi::RgUsage::SampledRead);

                rhi::RgTextureDesc desc;
                desc.width  = frame.extent.width;
                desc.height = frame.extent.height;
                desc.format = VK_FORMAT_R16G16B16A16_SFLOAT;
                hdr = builder.createTexture("SceneHdr", desc);
                builder.writeColor(hdr, VK_ATTACHMENT_LOAD_OP_CLEAR);
            },
            [&](rhi::RgContext& context) {
                lightingPipeline.bind(context.cmd());
                lightingPipeline.setViewportAndScissor(context.cmd(), frame.extent);

                VkDescriptorSet set = bindless.set();
                vkCmdBindDescriptorSets(context.cmd(), VK_PIPELINE_BIND_POINT_GRAPHICS,
                                        lightingPipeline.layout(), 0, 1, &set, 0, nullptr);
                vkCmdPushConstants(context.cmd(), lightingPipeline.layout(),
                                   VK_SHADER_STAGE_ALL, 0, sizeof(lightingPush),
                                   &lightingPush);
                vkCmdDraw(context.cmd(), 3, 1, 0, 0);
            });

        TonemapPush tonemapPush;
        tonemapPush.exposure = 1.0f;

        graph.addPass("Tonemap",
            [&](rhi::RgBuilder& builder) {
                builder.read(hdr, rhi::RgUsage::SampledRead);
                builder.writeColor(target, VK_ATTACHMENT_LOAD_OP_CLEAR);
            },
            [&](rhi::RgContext& context) {
                tonemapPipeline.bind(context.cmd());
                tonemapPipeline.setViewportAndScissor(context.cmd(), frame.extent);

                VkDescriptorSet set = bindless.set();
                vkCmdBindDescriptorSets(context.cmd(), VK_PIPELINE_BIND_POINT_GRAPHICS,
                                        tonemapPipeline.layout(), 0, 1, &set, 0, nullptr);
                vkCmdPushConstants(context.cmd(), tonemapPipeline.layout(),
                                   VK_SHADER_STAGE_ALL, 0, sizeof(tonemapPush),
                                   &tonemapPush);
                vkCmdDraw(context.cmd(), 3, 1, 0, 0);
            });

        graph.compile();

        // Transients only have physical images after compile(), so this is the
        // earliest the sampling passes can be told where to look.
        const std::uint32_t albedoSlot = bindless.registerSampledImage(
            graph.viewOf(gbuffer.color[0]), VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        const std::uint32_t normalSlot = bindless.registerSampledImage(
            graph.viewOf(gbuffer.color[1]), VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        const std::uint32_t materialSlot = bindless.registerSampledImage(
            graph.viewOf(gbuffer.color[2]), VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        const std::uint32_t depthSlot = bindless.registerSampledImage(
            graph.viewOf(gbuffer.depth), VK_IMAGE_LAYOUT_DEPTH_READ_ONLY_OPTIMAL);
        const std::uint32_t hdrSlot = bindless.registerSampledImage(
            graph.viewOf(hdr), VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);

        lightingPush.albedoTexture   = albedoSlot;
        lightingPush.normalTexture   = normalSlot;
        lightingPush.materialTexture = materialSlot;
        lightingPush.depthTexture    = depthSlot;
        tonemapPush.hdrTexture       = hdrSlot;

        graph.execute(frame.cmd);

        if (!reported) {
            const rhi::RenderGraph::Stats& stats = graph.stats();
            std::printf("[graph] passes=%u culled=%u barriers=%u transients=%u "
                        "images=%u aliased=%u\n",
                        stats.passes, stats.culledPasses, stats.barriers,
                        stats.transients, stats.physicalImages, stats.aliasedImages);
            reported = true;
        }

        renderer.endFrame();

        bindless.releaseSampledImage(albedoSlot);
        bindless.releaseSampledImage(normalSlot);
        bindless.releaseSampledImage(materialSlot);
        bindless.releaseSampledImage(depthSlot);
        bindless.releaseSampledImage(hdrSlot);

        ++rendered;
        if (options.frames != 0 && rendered >= options.frames) {
            break;
        }
    }

    bool captureOk = true;
    if (!windowed && !options.output.empty()) {
        captureOk = renderer.captureToPng(options.output);
        if (captureOk) {
            std::printf("[app] wrote %s\n", options.output.c_str());
        }
    }

    scene.destroy();
    frameBuffer.destroy();
    objectBuffer.destroy();
    lightBuffer.destroy();
    environmentBuffer.destroy();
    ibl.destroy();
    graph.destroy();
    uploader.destroy();
    gbufferPipeline.destroy();
    lightingPipeline.destroy();
    tonemapPipeline.destroy();
    renderer.destroy();
    device.destroy();
    window.destroy();
    JobSystem::get().shutdown();

    const std::uint64_t validationErrors = rhi::VulkanDevice::validationErrorCount();
    std::printf("[app] frames: %u, validation errors: %llu\n",
                rendered, static_cast<unsigned long long>(validationErrors));

    return (validationErrors != 0 || !captureOk) ? 1 : 0;
}
