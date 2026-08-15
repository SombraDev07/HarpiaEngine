// Harpia Engine — F0.b deliverable
//
// Animated clear, zero validation errors. Runs windowed when a display is
// reachable and offscreen otherwise, writing a PNG that CI compares against a
// golden image.

#include "Core/Profiling/Profiler.h"
#include "Core/Threading/JobSystem.h"
#include "Platform/Window.h"
#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanRenderer.h"

#include <cmath>
#include <cstdio>
#include <cstring>
#include <string>

namespace {

struct Options {
    bool          headless = false;
    std::uint32_t frames   = 0; // 0 = run until the window closes
    std::uint32_t width    = 1280;
    std::uint32_t height   = 720;
    std::string   output;
    float         fixedTime = -1.0f; // deterministic colour for golden images
};

Options parseArgs(int argc, char** argv)
{
    Options options;
    for (int i = 1; i < argc; ++i) {
        const char* arg = argv[i];
        const auto next = [&](std::uint32_t fallback) -> std::uint32_t {
            return (i + 1 < argc) ? static_cast<std::uint32_t>(std::atoi(argv[++i])) : fallback;
        };

        if (std::strcmp(arg, "--headless") == 0) {
            options.headless = true;
        } else if (std::strcmp(arg, "--frames") == 0) {
            options.frames = next(1);
        } else if (std::strcmp(arg, "--width") == 0) {
            options.width = next(options.width);
        } else if (std::strcmp(arg, "--height") == 0) {
            options.height = next(options.height);
        } else if (std::strcmp(arg, "--output") == 0 && i + 1 < argc) {
            options.output   = argv[++i];
            options.headless = true;
        } else if (std::strcmp(arg, "--fixed-time") == 0 && i + 1 < argc) {
            options.fixedTime = std::strtof(argv[++i], nullptr);
        }
    }
    return options;
}

// A gradient that varies over time so a still frame proves the clear ran and a
// moving one proves the loop is alive.
VkClearColorValue clearColourFor(float seconds)
{
    VkClearColorValue colour{};
    colour.float32[0] = 0.5f + 0.5f * std::sin(seconds * 0.9f);
    colour.float32[1] = 0.5f + 0.5f * std::sin(seconds * 1.3f + 2.0f);
    colour.float32[2] = 0.5f + 0.5f * std::sin(seconds * 1.7f + 4.0f);
    colour.float32[3] = 1.0f;
    return colour;
}

void recordClear(const harpia::rhi::FrameInfo& frame, float seconds)
{
    harpia::rhi::DebugLabel label(frame.cmd, "ClearScreen", 0.2f, 0.7f, 0.5f);

    VkRenderingAttachmentInfo colour{};
    colour.sType       = VK_STRUCTURE_TYPE_RENDERING_ATTACHMENT_INFO;
    colour.imageView   = frame.targetView;
    colour.imageLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
    colour.loadOp      = VK_ATTACHMENT_LOAD_OP_CLEAR;
    colour.storeOp     = VK_ATTACHMENT_STORE_OP_STORE;
    colour.clearValue.color = clearColourFor(seconds);

    VkRenderingInfo rendering{};
    rendering.sType                = VK_STRUCTURE_TYPE_RENDERING_INFO;
    rendering.renderArea.extent    = frame.extent;
    rendering.layerCount           = 1;
    rendering.colorAttachmentCount = 1;
    rendering.pColorAttachments    = &colour;

    // Dynamic rendering: no render pass object, no framebuffer object.
    vkCmdBeginRendering(frame.cmd, &rendering);
    vkCmdEndRendering(frame.cmd);
}

} // namespace

int main(int argc, char** argv)
{
    using namespace harpia;

    const Options options = parseArgs(argc, argv);

    JobSystem::get().init();

    const bool wantWindow = !options.headless && Window::platformAvailable();

    Window window;
    if (wantWindow) {
        WindowDesc desc;
        desc.width  = options.width;
        desc.height = options.height;
        desc.title  = "Harpia — ClearScreen";
        if (!window.create(desc)) {
            std::fprintf(stderr, "[app] window creation failed, falling back to offscreen\n");
        }
    }

    const bool windowed = window.nativeHandle() != nullptr;

    rhi::DeviceDesc deviceDesc;
    deviceDesc.applicationName  = "Harpia ClearScreen";
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

    const rhi::VulkanBindless::Capacity capacity = renderer.bindless().capacity();
    std::printf("[app] bindless: %u sampled images, %u storage buffers, %u samplers\n",
                capacity.sampledImages, capacity.storageBuffers, capacity.samplers);

    std::uint32_t rendered = 0;
    float         seconds  = 0.0f;

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

        seconds = options.fixedTime >= 0.0f
                    ? options.fixedTime
                    : static_cast<float>(rendered) / 60.0f;

        rhi::FrameInfo frame;
        if (renderer.beginFrame(frame)) {
            recordClear(frame, seconds);
            renderer.endFrame();
            ++rendered;
        }

        HARPIA_FRAME_MARK();

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

    renderer.destroy();
    device.destroy();
    window.destroy();
    JobSystem::get().shutdown();

    const std::uint64_t validationErrors = rhi::VulkanDevice::validationErrorCount();
    std::printf("[app] frames: %u, validation errors: %llu\n",
                rendered, static_cast<unsigned long long>(validationErrors));

    // Rule 2: validation errors are a build failure, not a log line.
    if (validationErrors != 0 || !captureOk) {
        return 1;
    }
    return 0;
}
