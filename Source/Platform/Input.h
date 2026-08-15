// Harpia Engine — action-mapped input
//
// Two halves on purpose:
//   RawInputState  — what the devices report this frame. The platform fills it.
//   Input          — actions, bindings, contexts, edge detection. Pure logic.
//
// The split is what makes input testable without a window, and what lets the
// editor replay recorded input later.
#pragma once

#include "Platform/InputTypes.h"

#include <array>
#include <bitset>
#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

namespace harpia {

// One frame of device truth. Everything above reads actions, not this.
struct RawInputState {
    std::bitset<static_cast<std::size_t>(Key::Count)>            keys;
    std::bitset<static_cast<std::size_t>(MouseButton::Count)>    mouseButtons;
    std::bitset<static_cast<std::size_t>(GamepadButton::Count)>  gamepadButtons;

    std::array<float, static_cast<std::size_t>(GamepadAxis::Count)> gamepadAxes{};
    std::array<float, static_cast<std::size_t>(MouseAxis::Count)>   mouseAxes{};

    bool gamepadConnected = false;

    void clear()
    {
        keys.reset();
        mouseButtons.reset();
        gamepadButtons.reset();
        gamepadAxes.fill(0.0f);
        mouseAxes.fill(0.0f);
    }
};

using ActionId = std::uint32_t;
inline constexpr ActionId kInvalidAction = 0xFFFFFFFFu;

class Input {
public:
    Input();

    // --- binding -----------------------------------------------------------

    // Creates the action on first use and returns the same id afterwards.
    [[nodiscard]] ActionId action(std::string_view name);
    [[nodiscard]] ActionId find(std::string_view name) const noexcept;
    [[nodiscard]] std::string_view nameOf(ActionId action) const noexcept;
    [[nodiscard]] std::size_t actionCount() const noexcept { return actions_.size(); }

    void bindKey(ActionId action, Key key, InputContext context = InputContext::Gameplay);
    void bindMouseButton(ActionId action, MouseButton button,
                         InputContext context = InputContext::Gameplay);
    void bindGamepadButton(ActionId action, GamepadButton button,
                           InputContext context = InputContext::Gameplay);

    // Analogue source, optionally inverted or scaled.
    void bindGamepadAxis(ActionId action, GamepadAxis axis, float scale = 1.0f,
                         InputContext context = InputContext::Gameplay);
    void bindMouseAxis(ActionId action, MouseAxis axis, float scale = 1.0f,
                       InputContext context = InputContext::Gameplay);

    // Two keys forming an axis, e.g. A/D for strafing.
    void bindKeyAxis(ActionId action, Key negative, Key positive,
                     InputContext context = InputContext::Gameplay);

    // Rebinding: drop what an action listens to and bind again.
    void clearBindings(ActionId action);
    [[nodiscard]] std::size_t bindingCount(ActionId action) const noexcept;

    // --- contexts ----------------------------------------------------------

    void setContextActive(InputContext context, bool active);
    [[nodiscard]] bool contextActive(InputContext context) const noexcept;

    // --- per-frame ---------------------------------------------------------

    // Rolls current state to previous and evaluates every action against `raw`.
    void update(const RawInputState& raw);

    [[nodiscard]] bool  down(ActionId action) const noexcept;
    [[nodiscard]] bool  pressed(ActionId action) const noexcept;   // went down this frame
    [[nodiscard]] bool  released(ActionId action) const noexcept;  // went up this frame
    [[nodiscard]] float axis(ActionId action) const noexcept;

    // Convenience for one-off lookups; prefer caching the ActionId.
    [[nodiscard]] bool  down(std::string_view name) const noexcept;
    [[nodiscard]] bool  pressed(std::string_view name) const noexcept;
    [[nodiscard]] float axis(std::string_view name) const noexcept;

    // --- tuning ------------------------------------------------------------

    // Stick values below this collapse to zero. Applied radially per stick, not
    // per component, so a diagonal push is not clipped into a square.
    void  setStickDeadZone(float deadZone) noexcept;
    void  setTriggerDeadZone(float deadZone) noexcept;
    [[nodiscard]] float stickDeadZone() const noexcept { return stickDeadZone_; }

private:
    enum class SourceKind : std::uint8_t {
        Key, MouseButton, GamepadButton, GamepadAxis, MouseAxis, KeyAxis
    };

    struct Binding {
        ActionId     action  = kInvalidAction;
        SourceKind   kind    = SourceKind::Key;
        InputContext context = InputContext::Gameplay;
        std::uint16_t primary   = 0;  // key / button / axis index
        std::uint16_t secondary = 0;  // second key for KeyAxis
        float         scale     = 1.0f;
    };

    struct ActionState {
        bool  down     = false;
        bool  wasDown  = false;
        float axis     = 0.0f;
    };

    [[nodiscard]] float applyDeadZone(float value, float deadZone) const noexcept;
    void applyStickDeadZones(RawInputState& raw) const noexcept;

    std::vector<std::string> actions_;
    std::vector<Binding>     bindings_;
    std::vector<ActionState> states_;

    std::bitset<static_cast<std::size_t>(InputContext::Count)> activeContexts_;

    float stickDeadZone_   = 0.15f;
    float triggerDeadZone_ = 0.05f;
};

} // namespace harpia
