#include "Platform/Window.h"

#include <GLFW/glfw3.h>

#include <cstdio>

namespace harpia {
namespace {

int  g_glfwRefCount = 0;
bool g_glfwFailed   = false;

bool ensureGlfw()
{
    if (g_glfwFailed) {
        return false;
    }
    if (g_glfwRefCount == 0) {
        glfwSetErrorCallback([](int code, const char* description) {
            std::fprintf(stderr, "[glfw] error %d: %s\n", code, description);
        });
        if (glfwInit() != GLFW_TRUE) {
            g_glfwFailed = true;
            return false;
        }
    }
    ++g_glfwRefCount;
    return true;
}

void releaseGlfw()
{
    if (g_glfwRefCount > 0 && --g_glfwRefCount == 0) {
        glfwTerminate();
    }
}

} // namespace

Window::~Window()
{
    destroy();
}

bool Window::platformAvailable()
{
    if (!ensureGlfw()) {
        return false;
    }
    const bool ok = glfwVulkanSupported() == GLFW_TRUE;
    releaseGlfw();
    return ok;
}

const char** Window::requiredInstanceExtensions(std::uint32_t& outCount)
{
    outCount = 0;
    return glfwGetRequiredInstanceExtensions(&outCount);
}

bool Window::create(const WindowDesc& desc)
{
    if (window_ != nullptr) {
        return true;
    }
    if (!ensureGlfw()) {
        return false;
    }

    glfwWindowHint(GLFW_CLIENT_API, GLFW_NO_API); // Vulkan owns the surface
    glfwWindowHint(GLFW_RESIZABLE, desc.resizable ? GLFW_TRUE : GLFW_FALSE);

    window_ = glfwCreateWindow(static_cast<int>(desc.width),
                               static_cast<int>(desc.height),
                               desc.title.c_str(),
                               nullptr,
                               nullptr);
    if (window_ == nullptr) {
        releaseGlfw();
        return false;
    }

    glfwSetWindowUserPointer(window_, this);
    glfwSetFramebufferSizeCallback(window_, &Window::framebufferSizeCallback);

    int fbWidth  = 0;
    int fbHeight = 0;
    glfwGetFramebufferSize(window_, &fbWidth, &fbHeight);
    width_  = static_cast<std::uint32_t>(fbWidth);
    height_ = static_cast<std::uint32_t>(fbHeight);

    return true;
}

void Window::destroy()
{
    if (window_ != nullptr) {
        glfwDestroyWindow(window_);
        window_ = nullptr;
        releaseGlfw();
    }
    width_  = 0;
    height_ = 0;
}

void Window::framebufferSizeCallback(GLFWwindow* window, int width, int height)
{
    auto* self = static_cast<Window*>(glfwGetWindowUserPointer(window));
    if (self == nullptr) {
        return;
    }
    self->width_   = static_cast<std::uint32_t>(width < 0 ? 0 : width);
    self->height_  = static_cast<std::uint32_t>(height < 0 ? 0 : height);
    self->resized_ = true;
}

void Window::pollEvents()
{
    glfwPollEvents();
}

bool Window::shouldClose() const
{
    return window_ == nullptr || glfwWindowShouldClose(window_) == GLFW_TRUE;
}

void Window::requestClose()
{
    if (window_ != nullptr) {
        glfwSetWindowShouldClose(window_, GLFW_TRUE);
    }
}

void Window::readInput(RawInputState& outState)
{
    outState.clear();
    if (window_ == nullptr) {
        return;
    }

    // Key and mouse-button enum values mirror GLFW's, so this is a cast rather
    // than a table. InputTypes.h is the place that promise is documented.
    for (std::size_t key = 0; key < static_cast<std::size_t>(Key::Count); ++key) {
        if (glfwGetKey(window_, static_cast<int>(key)) == GLFW_PRESS) {
            outState.keys.set(key);
        }
    }
    for (std::size_t button = 0; button < static_cast<std::size_t>(MouseButton::Count); ++button) {
        if (glfwGetMouseButton(window_, static_cast<int>(button)) == GLFW_PRESS) {
            outState.mouseButtons.set(button);
        }
    }

    double mouseX = 0.0;
    double mouseY = 0.0;
    glfwGetCursorPos(window_, &mouseX, &mouseY);

    // The first frame has no previous position; reporting the absolute cursor
    // as a delta would fling the camera.
    if (mousePrimed_) {
        outState.mouseAxes[static_cast<std::size_t>(MouseAxis::X)] =
            static_cast<float>(mouseX - lastMouseX_);
        outState.mouseAxes[static_cast<std::size_t>(MouseAxis::Y)] =
            static_cast<float>(mouseY - lastMouseY_);
    }
    lastMouseX_  = mouseX;
    lastMouseY_  = mouseY;
    mousePrimed_ = true;

    GLFWgamepadstate pad{};
    if (glfwGetGamepadState(GLFW_JOYSTICK_1, &pad) == GLFW_TRUE) {
        outState.gamepadConnected = true;
        for (std::size_t i = 0; i < static_cast<std::size_t>(GamepadButton::Count); ++i) {
            if (pad.buttons[i] == GLFW_PRESS) {
                outState.gamepadButtons.set(i);
            }
        }
        for (std::size_t i = 0; i < static_cast<std::size_t>(GamepadAxis::Count); ++i) {
            outState.gamepadAxes[i] = pad.axes[i];
        }
        // GLFW reports triggers in [-1,1]; the engine treats them as [0,1].
        for (const GamepadAxis trigger : {GamepadAxis::LeftTrigger, GamepadAxis::RightTrigger}) {
            float& value = outState.gamepadAxes[static_cast<std::size_t>(trigger)];
            value = (value + 1.0f) * 0.5f;
        }
    }
}

bool Window::consumeResized() noexcept
{
    const bool was = resized_;
    resized_ = false;
    return was;
}

} // namespace harpia
