// Input is testable without a window because RawInputState is a plain struct
// the tests fill directly. That separation is the whole point of the design.

#include <doctest/doctest.h>

#include "Platform/Input.h"

#include <cmath>

using namespace harpia;

namespace {

RawInputState withKey(Key key)
{
    RawInputState raw;
    raw.keys.set(static_cast<std::size_t>(key));
    return raw;
}

} // namespace

TEST_CASE("actions are created once and found by name")
{
    Input input;

    const ActionId jump = input.action("Jump");
    CHECK(input.action("Jump") == jump);
    CHECK(input.find("Jump") == jump);
    CHECK(input.find("Crouch") == kInvalidAction);
    CHECK(input.nameOf(jump) == "Jump");
    CHECK(input.actionCount() == 1);
}

TEST_CASE("a bound key drives the action, not the other way round")
{
    Input input;
    const ActionId jump = input.action("Jump");
    input.bindKey(jump, Key::Space);

    input.update(RawInputState{});
    CHECK_FALSE(input.down(jump));

    input.update(withKey(Key::Space));
    CHECK(input.down(jump));
    CHECK(input.axis(jump) == doctest::Approx(1.0f));
}

TEST_CASE("pressed and released are single-frame edges")
{
    Input input;
    const ActionId fire = input.action("Fire");
    input.bindKey(fire, Key::F);

    const RawInputState held = withKey(Key::F);

    input.update(held);
    CHECK(input.pressed(fire));
    CHECK(input.down(fire));
    CHECK_FALSE(input.released(fire));

    // Still held: down stays true, pressed does not fire again.
    input.update(held);
    CHECK_FALSE(input.pressed(fire));
    CHECK(input.down(fire));

    input.update(RawInputState{});
    CHECK(input.released(fire));
    CHECK_FALSE(input.down(fire));

    input.update(RawInputState{});
    CHECK_FALSE(input.released(fire));
}

TEST_CASE("several bindings feed one action")
{
    Input input;
    const ActionId jump = input.action("Jump");
    input.bindKey(jump, Key::Space);
    input.bindGamepadButton(jump, GamepadButton::South);

    CHECK(input.bindingCount(jump) == 2);

    input.update(withKey(Key::Space));
    CHECK(input.down(jump));

    RawInputState pad;
    pad.gamepadConnected = true;
    pad.gamepadButtons.set(static_cast<std::size_t>(GamepadButton::South));
    input.update(pad);
    CHECK(input.down(jump));

    // A gamepad button with no gamepad attached must not fire.
    RawInputState ghost;
    ghost.gamepadButtons.set(static_cast<std::size_t>(GamepadButton::South));
    ghost.gamepadConnected = false;
    input.update(ghost);
    CHECK_FALSE(input.down(jump));
}

TEST_CASE("a key pair forms an axis")
{
    Input input;
    const ActionId strafe = input.action("Strafe");
    input.bindKeyAxis(strafe, Key::A, Key::D);

    input.update(withKey(Key::A));
    CHECK(input.axis(strafe) == doctest::Approx(-1.0f));

    input.update(withKey(Key::D));
    CHECK(input.axis(strafe) == doctest::Approx(1.0f));

    // Both down cancel out.
    RawInputState both;
    both.keys.set(static_cast<std::size_t>(Key::A));
    both.keys.set(static_cast<std::size_t>(Key::D));
    input.update(both);
    CHECK(input.axis(strafe) == doctest::Approx(0.0f));
}

TEST_CASE("rebinding replaces what an action listens to")
{
    Input input;
    const ActionId jump = input.action("Jump");
    input.bindKey(jump, Key::Space);

    input.update(withKey(Key::Space));
    REQUIRE(input.down(jump));

    input.clearBindings(jump);
    CHECK(input.bindingCount(jump) == 0);
    input.bindKey(jump, Key::W);

    input.update(withKey(Key::Space));
    CHECK_FALSE(input.down(jump));

    input.update(withKey(Key::W));
    CHECK(input.down(jump));
}

TEST_CASE("an inactive context swallows its bindings")
{
    Input input;

    const ActionId fire   = input.action("Fire");
    const ActionId confirm = input.action("Confirm");
    input.bindKey(fire, Key::Enter, InputContext::Gameplay);
    input.bindKey(confirm, Key::Enter, InputContext::Ui);

    // Gameplay is on by default, Ui is not.
    CHECK(input.contextActive(InputContext::Gameplay));
    CHECK_FALSE(input.contextActive(InputContext::Ui));

    input.update(withKey(Key::Enter));
    CHECK(input.down(fire));
    CHECK_FALSE(input.down(confirm));

    // Opening a menu: Ui on, gameplay off. The same key must not do both.
    input.setContextActive(InputContext::Ui, true);
    input.setContextActive(InputContext::Gameplay, false);

    input.update(withKey(Key::Enter));
    CHECK_FALSE(input.down(fire));
    CHECK(input.down(confirm));
}

TEST_CASE("stick dead zone is radial, so diagonals keep their range")
{
    Input input;
    input.setStickDeadZone(0.2f);

    const ActionId moveX = input.action("MoveX");
    const ActionId moveY = input.action("MoveY");
    input.bindGamepadAxis(moveX, GamepadAxis::LeftX);
    input.bindGamepadAxis(moveY, GamepadAxis::LeftY);

    SUBCASE("inside the dead zone collapses to zero")
    {
        RawInputState raw;
        raw.gamepadConnected = true;
        raw.gamepadAxes[static_cast<std::size_t>(GamepadAxis::LeftX)] = 0.1f;
        raw.gamepadAxes[static_cast<std::size_t>(GamepadAxis::LeftY)] = 0.1f;

        input.update(raw);
        CHECK(input.axis(moveX) == doctest::Approx(0.0f));
        CHECK(input.axis(moveY) == doctest::Approx(0.0f));
    }

    SUBCASE("a full diagonal still reaches full magnitude")
    {
        // A stick pushed fully diagonally reports ~0.707 on each component.
        const float component = 0.70710678f;
        RawInputState raw;
        raw.gamepadConnected = true;
        raw.gamepadAxes[static_cast<std::size_t>(GamepadAxis::LeftX)] = component;
        raw.gamepadAxes[static_cast<std::size_t>(GamepadAxis::LeftY)] = component;

        input.update(raw);

        const float x = input.axis(moveX);
        const float y = input.axis(moveY);
        const float magnitude = std::sqrt(x * x + y * y);

        // Per-component dead zones would clip this below 1.0; radial does not.
        CHECK(magnitude == doctest::Approx(1.0f).epsilon(0.01));
    }

    SUBCASE("full deflection on one axis reaches exactly 1")
    {
        RawInputState raw;
        raw.gamepadConnected = true;
        raw.gamepadAxes[static_cast<std::size_t>(GamepadAxis::LeftX)] = 1.0f;

        input.update(raw);
        CHECK(input.axis(moveX) == doctest::Approx(1.0f));
    }
}

TEST_CASE("axis scale inverts and attenuates")
{
    Input input;
    const ActionId look = input.action("LookY");
    input.bindMouseAxis(look, MouseAxis::Y, -2.0f);

    RawInputState raw;
    raw.mouseAxes[static_cast<std::size_t>(MouseAxis::Y)] = 3.0f;

    input.update(raw);
    CHECK(input.axis(look) == doctest::Approx(-6.0f));
}

TEST_CASE("the strongest binding wins when several feed one axis")
{
    Input input;
    const ActionId move = input.action("Move");
    input.bindKeyAxis(move, Key::S, Key::W);
    input.bindGamepadAxis(move, GamepadAxis::LeftY);

    RawInputState raw;
    raw.gamepadConnected = true;
    raw.keys.set(static_cast<std::size_t>(Key::W));                       // +1.0
    raw.gamepadAxes[static_cast<std::size_t>(GamepadAxis::LeftY)] = 0.5f; // weaker

    input.update(raw);
    CHECK(input.axis(move) == doctest::Approx(1.0f));
}

TEST_CASE("unknown actions answer safely")
{
    Input input;
    CHECK_FALSE(input.down(kInvalidAction));
    CHECK_FALSE(input.pressed(kInvalidAction));
    CHECK(input.axis(kInvalidAction) == doctest::Approx(0.0f));
    CHECK_FALSE(input.down("never registered"));
    CHECK(input.nameOf(kInvalidAction).empty());
}
