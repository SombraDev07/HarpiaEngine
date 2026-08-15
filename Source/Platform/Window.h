// Harpia Engine — window
//
// Deliberately knows nothing about Vulkan. The RHI asks for the native handle
// and creates the surface itself, which keeps rule 5 intact: no vk* call lives
// outside Source/RHI.
#pragma once

#include <cstdint>
#include <string>

struct GLFWwindow;

namespace harpia {

struct WindowDesc {
    std::uint32_t width  = 1280;
    std::uint32_t height = 720;
    std::string   title  = "Harpia";
    bool          resizable = true;
};

class Window {
public:
    Window() = default;
    ~Window();

    Window(const Window&)            = delete;
    Window& operator=(const Window&) = delete;

    [[nodiscard]] bool create(const WindowDesc& desc);
    void destroy();

    void pollEvents();
    [[nodiscard]] bool shouldClose() const;
    void requestClose();

    [[nodiscard]] std::uint32_t width() const noexcept  { return width_; }
    [[nodiscard]] std::uint32_t height() const noexcept { return height_; }

    // True once between the resize and the next call. The renderer uses this
    // to know it must rebuild the swapchain.
    [[nodiscard]] bool consumeResized() noexcept;

    // Minimised windows have a zero-sized framebuffer; presenting to one is
    // invalid, so callers skip the frame.
    [[nodiscard]] bool minimised() const noexcept { return width_ == 0 || height_ == 0; }

    [[nodiscard]] GLFWwindow* nativeHandle() const noexcept { return window_; }

    // True when a window system is reachable. Headless machines (CI) get false
    // and are expected to run the offscreen path instead.
    [[nodiscard]] static bool platformAvailable();

    // Instance extensions the window system requires. Returned as a pointer to
    // GLFW-owned storage; the RHI copies them.
    [[nodiscard]] static const char** requiredInstanceExtensions(std::uint32_t& outCount);

private:
    static void framebufferSizeCallback(GLFWwindow* window, int width, int height);

    GLFWwindow*   window_  = nullptr;
    std::uint32_t width_   = 0;
    std::uint32_t height_  = 0;
    bool          resized_ = false;
};

} // namespace harpia
