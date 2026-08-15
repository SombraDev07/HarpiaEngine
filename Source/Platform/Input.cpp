#include "Platform/Input.h"

#include <algorithm>
#include <cmath>

namespace harpia {
namespace {

template <typename E>
[[nodiscard]] constexpr std::size_t indexOf(E value) noexcept
{
    return static_cast<std::size_t>(value);
}

} // namespace

Input::Input()
{
    // Gameplay is live by default; the editor opts the others in.
    activeContexts_.set(indexOf(InputContext::Gameplay));
}

ActionId Input::action(std::string_view name)
{
    const ActionId existing = find(name);
    if (existing != kInvalidAction) {
        return existing;
    }

    actions_.emplace_back(name);
    states_.emplace_back();
    return static_cast<ActionId>(actions_.size() - 1);
}

ActionId Input::find(std::string_view name) const noexcept
{
    for (std::size_t i = 0; i < actions_.size(); ++i) {
        if (actions_[i] == name) {
            return static_cast<ActionId>(i);
        }
    }
    return kInvalidAction;
}

std::string_view Input::nameOf(ActionId action) const noexcept
{
    return action < actions_.size() ? std::string_view{actions_[action]} : std::string_view{};
}

void Input::bindKey(ActionId action, Key key, InputContext context)
{
    bindings_.push_back(Binding{action, SourceKind::Key, context,
                                static_cast<std::uint16_t>(key), 0, 1.0f});
}

void Input::bindMouseButton(ActionId action, MouseButton button, InputContext context)
{
    bindings_.push_back(Binding{action, SourceKind::MouseButton, context,
                                static_cast<std::uint16_t>(button), 0, 1.0f});
}

void Input::bindGamepadButton(ActionId action, GamepadButton button, InputContext context)
{
    bindings_.push_back(Binding{action, SourceKind::GamepadButton, context,
                                static_cast<std::uint16_t>(button), 0, 1.0f});
}

void Input::bindGamepadAxis(ActionId action, GamepadAxis axis, float scale, InputContext context)
{
    bindings_.push_back(Binding{action, SourceKind::GamepadAxis, context,
                                static_cast<std::uint16_t>(axis), 0, scale});
}

void Input::bindMouseAxis(ActionId action, MouseAxis axis, float scale, InputContext context)
{
    bindings_.push_back(Binding{action, SourceKind::MouseAxis, context,
                                static_cast<std::uint16_t>(axis), 0, scale});
}

void Input::bindKeyAxis(ActionId action, Key negative, Key positive, InputContext context)
{
    bindings_.push_back(Binding{action, SourceKind::KeyAxis, context,
                                static_cast<std::uint16_t>(negative),
                                static_cast<std::uint16_t>(positive), 1.0f});
}

void Input::clearBindings(ActionId action)
{
    bindings_.erase(std::remove_if(bindings_.begin(), bindings_.end(),
                                   [action](const Binding& b) { return b.action == action; }),
                    bindings_.end());
}

std::size_t Input::bindingCount(ActionId action) const noexcept
{
    return static_cast<std::size_t>(
        std::count_if(bindings_.begin(), bindings_.end(),
                      [action](const Binding& b) { return b.action == action; }));
}

void Input::setContextActive(InputContext context, bool active)
{
    activeContexts_.set(indexOf(context), active);
}

bool Input::contextActive(InputContext context) const noexcept
{
    return activeContexts_.test(indexOf(context));
}

void Input::setStickDeadZone(float deadZone) noexcept
{
    stickDeadZone_ = std::clamp(deadZone, 0.0f, 0.99f);
}

void Input::setTriggerDeadZone(float deadZone) noexcept
{
    triggerDeadZone_ = std::clamp(deadZone, 0.0f, 0.99f);
}

float Input::applyDeadZone(float value, float deadZone) const noexcept
{
    const float magnitude = std::fabs(value);
    if (magnitude <= deadZone) {
        return 0.0f;
    }
    // Rescale so the usable range still reaches 1.0 — otherwise the dead zone
    // silently costs the player part of their range.
    const float scaled = (magnitude - deadZone) / (1.0f - deadZone);
    return value < 0.0f ? -scaled : scaled;
}

void Input::applyStickDeadZones(RawInputState& raw) const noexcept
{
    // Radial, per stick: treating X and Y separately turns a circular stick
    // into a square one and makes diagonals feel wrong.
    const auto processStick = [this](float& x, float& y) {
        const float magnitude = std::sqrt(x * x + y * y);
        if (magnitude <= stickDeadZone_) {
            x = 0.0f;
            y = 0.0f;
            return;
        }
        const float scaled = (magnitude - stickDeadZone_) / (1.0f - stickDeadZone_);
        const float factor = std::min(scaled, 1.0f) / magnitude;
        x *= factor;
        y *= factor;
    };

    processStick(raw.gamepadAxes[indexOf(GamepadAxis::LeftX)],
                 raw.gamepadAxes[indexOf(GamepadAxis::LeftY)]);
    processStick(raw.gamepadAxes[indexOf(GamepadAxis::RightX)],
                 raw.gamepadAxes[indexOf(GamepadAxis::RightY)]);

    for (const GamepadAxis trigger : {GamepadAxis::LeftTrigger, GamepadAxis::RightTrigger}) {
        float& value = raw.gamepadAxes[indexOf(trigger)];
        value = applyDeadZone(value, triggerDeadZone_);
    }
}

void Input::update(const RawInputState& rawInput)
{
    RawInputState raw = rawInput;
    applyStickDeadZones(raw);

    for (ActionState& state : states_) {
        state.wasDown = state.down;
        state.down    = false;
        state.axis    = 0.0f;
    }

    for (const Binding& binding : bindings_) {
        if (!activeContexts_.test(indexOf(binding.context))) {
            continue;
        }
        if (binding.action >= states_.size()) {
            continue;
        }
        ActionState& state = states_[binding.action];

        switch (binding.kind) {
            case SourceKind::Key: {
                if (raw.keys.test(binding.primary)) {
                    state.down = true;
                    state.axis = 1.0f;
                }
                break;
            }
            case SourceKind::MouseButton: {
                if (raw.mouseButtons.test(binding.primary)) {
                    state.down = true;
                    state.axis = 1.0f;
                }
                break;
            }
            case SourceKind::GamepadButton: {
                if (raw.gamepadConnected && raw.gamepadButtons.test(binding.primary)) {
                    state.down = true;
                    state.axis = 1.0f;
                }
                break;
            }
            case SourceKind::GamepadAxis: {
                if (!raw.gamepadConnected) {
                    break;
                }
                const float value = raw.gamepadAxes[binding.primary] * binding.scale;
                // Several bindings can feed one action; the strongest wins so a
                // stick and a key pair can coexist without cancelling out.
                if (std::fabs(value) > std::fabs(state.axis)) {
                    state.axis = value;
                }
                if (std::fabs(value) > 0.5f) {
                    state.down = true;
                }
                break;
            }
            case SourceKind::MouseAxis: {
                const float value = raw.mouseAxes[binding.primary] * binding.scale;
                if (std::fabs(value) > std::fabs(state.axis)) {
                    state.axis = value;
                }
                break;
            }
            case SourceKind::KeyAxis: {
                float value = 0.0f;
                if (raw.keys.test(binding.primary)) {
                    value -= 1.0f;
                }
                if (raw.keys.test(binding.secondary)) {
                    value += 1.0f;
                }
                value *= binding.scale;
                if (std::fabs(value) > std::fabs(state.axis)) {
                    state.axis = value;
                }
                if (value != 0.0f) {
                    state.down = true;
                }
                break;
            }
        }
    }
}

bool Input::down(ActionId action) const noexcept
{
    return action < states_.size() && states_[action].down;
}

bool Input::pressed(ActionId action) const noexcept
{
    return action < states_.size() && states_[action].down && !states_[action].wasDown;
}

bool Input::released(ActionId action) const noexcept
{
    return action < states_.size() && !states_[action].down && states_[action].wasDown;
}

float Input::axis(ActionId action) const noexcept
{
    return action < states_.size() ? states_[action].axis : 0.0f;
}

bool Input::down(std::string_view name) const noexcept
{
    return down(find(name));
}

bool Input::pressed(std::string_view name) const noexcept
{
    return pressed(find(name));
}

float Input::axis(std::string_view name) const noexcept
{
    return axis(find(name));
}

} // namespace harpia
