// Harpia Engine — input device vocabulary
//
// Roadmap gap: the capability ladder has no input row at all, and it is the
// first thing a player touches. Values mirror GLFW so the platform layer is a
// cast, but nothing above Platform/ ever names a key — gameplay asks for
// "Jump", not for Space.
#pragma once

#include <cstdint>

namespace harpia {

enum class Key : std::uint16_t {
    Unknown = 0,

    Space = 32, Apostrophe = 39, Comma = 44, Minus, Period, Slash,
    Num0 = 48, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
    Semicolon = 59, Equal = 61,
    A = 65, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    LeftBracket = 91, Backslash, RightBracket, GraveAccent = 96,

    Escape = 256, Enter, Tab, Backspace, Insert, Delete,
    Right, Left, Down, Up, PageUp, PageDown, Home, End,
    CapsLock = 280, ScrollLock, NumLock, PrintScreen, Pause,
    F1 = 290, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,

    LeftShift = 340, LeftControl, LeftAlt, LeftSuper,
    RightShift, RightControl, RightAlt, RightSuper,

    Count = 512
};

enum class MouseButton : std::uint8_t {
    Left = 0, Right = 1, Middle = 2, Button4 = 3, Button5 = 4,
    Count = 8
};

enum class GamepadButton : std::uint8_t {
    South = 0, East, West, North,          // A/B/X/Y on an Xbox pad
    LeftBumper, RightBumper,
    Back, Start, Guide,
    LeftThumb, RightThumb,
    DpadUp, DpadRight, DpadDown, DpadLeft,
    Count
};

enum class GamepadAxis : std::uint8_t {
    LeftX = 0, LeftY, RightX, RightY, LeftTrigger, RightTrigger,
    Count
};

enum class MouseAxis : std::uint8_t {
    X = 0, Y, ScrollX, ScrollY,
    Count
};

// Contexts gate whole groups of bindings. The editor pushing Ui must not let a
// Gameplay "Fire" binding through, which is the bug every ad-hoc input layer
// eventually grows.
enum class InputContext : std::uint8_t {
    Gameplay = 0,
    Ui,
    Editor,
    Debug,
    Count
};

} // namespace harpia
