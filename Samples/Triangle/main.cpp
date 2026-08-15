// Harpia Engine — F1 deliverable
//
// A triangle drawn by a pass declared in the render graph, not by hand-rolled
// commands. The graph derives every barrier and drives dynamic rendering; the
// pass body only binds a pipeline and draws.

#include "Core/Threading/JobSystem.h"
#include "Platform/Window.h"
#include "RHI/RenderGraph/RenderGraph.h"
#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanPipeline.h"
#include "RHI/Vulkan/VulkanRenderer.h"

#include <cstdio>
#include <cstring>
#include <string>

namespace {

struct Options {
    bool          headless = false;
    std::uint32_t frames   = 0;
    std::uint32_t width    = 1280;
    std::uint32_t height   = 720;
    std::string   output;
};

Options parseArgs(int argc, char** argv)
{
    Options options;
    for (int i = 1; i < argc; ++i) {
        const char* arg = argv[i];
        const auto next = [&](std::uint32_t fallback) -> std::uint32_t {
            return (i + 1 < argc) ? static_cast<std::uint32_t>(std::atoi(argv[++i])) : fallback;
        };
        if (std::strcmp(arg, "--headless") == 0)      { options.headless = true; }
        else if (std::strcmp(arg, "--frames") == 0)   { options.frames = next(1); }
        else if (std::strcmp(arg, "--width") == 0)    { options.width = next(options.width); }
        else if (std::strcmp(arg, "--height") == 0)   { options.height = next(options.height); }
        else if (std::strcmp(arg, "--output") == 0 && i + 1 < argc) {
            options.output   = argv[++i];
            options.headless = true;
        }
    }
    return options;
}

} // namespace

int main(int argc, char** argv)
{
    using namespace harpia;

    const Options options = parseArgs(argc, argv);

    JobSystem::get().init();

    Window window;
    if (!options.headless && Window::platformAvailable()) {
        WindowDesc desc;
        desc.width  = options.width;
        desc.height = options.height;
        desc.title  = "Harpia — Triangle";
        (void)window.create(desc);
    }
    const bool windowed = window.nativeHandle() != nullptr;

    rhi::DeviceDesc deviceDesc;
    deviceDesc.applicationName  = "Harpia Triangle";
    deviceDesc.enableValidation = true;
    deviceDesc.window           = windowed ? window.nativeHandle() : nullptr;

    rhi::VulkanDevice device;
    if (!device.create(deviceDesc)) {
        std::fprintf(stderr, "[app] device creation failed\n");
        JobSystem::get().shutdown();
        return 1;
    }
    std::printf("[app] gpu: %s\n", device.deviceName().c_str());
    std::printf("[app] mode: %s\n", windowed ? "windowed" : "offscreen");

    rhi::VulkanRenderer renderer;
    const bool rendererOk = windowed
        ? renderer.create(device, window.width(), window.height())
        : renderer.createOffscreen(device, options.width, options.height);
    if (!rendererOk) {
        std::fprintf(stderr, "[app] renderer creation failed\n");
        JobSystem::get().shutdown();
        return 1;
    }

    // The target format decides the pipeline's colour attachment format; the
    // offscreen path and the swapchain do not necessarily agree.
    const VkFormat targetFormat = windowed ? VK_FORMAT_B8G8R8A8_UNORM
                                           : VK_FORMAT_R8G8B8A8_UNORM;

    rhi::GraphicsPipelineDesc pipelineDesc;
    pipelineDesc.vertexSpirvPath   = std::string(HARPIA_SHADER_DIR) + "/Triangle.vert.spv";
    pipelineDesc.fragmentSpirvPath = std::string(HARPIA_SHADER_DIR) + "/Triangle.frag.spv";
    pipelineDesc.colorFormats      = {targetFormat};
    pipelineDesc.descriptorSetLayout = renderer.bindless().layout();
    pipelineDesc.debugName         = "Pipeline_Triangle";

    rhi::VulkanPipeline pipeline;
    if (!pipeline.create(device, pipelineDesc)) {
        std::fprintf(stderr, "[app] pipeline creation failed\n");
        JobSystem::get().shutdown();
        return 1;
    }

    rhi::RenderGraph graph;
    if (!graph.create(device)) {
        std::fprintf(stderr, "[app] render graph creation failed\n");
        JobSystem::get().shutdown();
        return 1;
    }

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
        if (renderer.beginFrame(frame)) {
            graph.beginFrame();

            // beginFrame already moved the target into COLOR_ATTACHMENT_OPTIMAL,
            // so that is the layout the graph inherits.
            const rhi::RgHandle target = graph.importTexture(
                "SceneColor", frame.targetImage, frame.targetView,
                targetFormat, frame.extent,
                VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL);

            VkClearValue clear{};
            clear.color = {{0.02f, 0.02f, 0.04f, 1.0f}};

            graph.addPass("TrianglePass",
                [&](rhi::RgBuilder& builder) {
                    builder.writeColor(target, VK_ATTACHMENT_LOAD_OP_CLEAR, clear);
                },
                [&](rhi::RgContext& context) {
                    pipeline.bind(context.cmd());
                    pipeline.setViewportAndScissor(context.cmd(), frame.extent);
                    vkCmdDraw(context.cmd(), 3, 1, 0, 0);
                });

            graph.compile();
            graph.execute(frame.cmd);

            if (!reported) {
                const rhi::RenderGraph::Stats& stats = graph.stats();
                std::printf("[graph] passes=%u culled=%u barriers=%u transients=%u\n",
                            stats.passes, stats.culledPasses, stats.barriers,
                            stats.transients);
                reported = true;
            }

            renderer.endFrame();
            ++rendered;
        }

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

    graph.destroy();
    pipeline.destroy();
    renderer.destroy();
    device.destroy();
    window.destroy();
    JobSystem::get().shutdown();

    const std::uint64_t validationErrors = rhi::VulkanDevice::validationErrorCount();
    std::printf("[app] frames: %u, validation errors: %llu\n",
                rendered, static_cast<unsigned long long>(validationErrors));

    return (validationErrors != 0 || !captureOk) ? 1 : 0;
}
