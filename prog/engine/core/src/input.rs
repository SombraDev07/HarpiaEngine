//! Keyboard and mouse state, with no windowing dependency.
//!
//! This lives in `core` so that `app` (which owns winit) can fill it in and
//! `render` (which owns the camera) can read it, without either depending on the
//! other. Nothing here knows what a `KeyCode` is — `app` translates.
//!
//! **Gates never see input.** A sample running under `--frames N` is a
//! reproducible measurement; a stray keypress during a capture would quietly
//! change the pixels it is judged on. `app` keeps this struct empty unless the
//! run is interactive.

/// The keys the engine reacts to. Deliberately small: a key that nothing reads
/// is a key that silently does nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum Key {
    W,
    A,
    S,
    D,
    Q,
    E,
    Space,
    Shift,
    Control,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Tab,
    F1,
    F2,
    F3,
}

impl Key {
    pub const COUNT: usize = 18;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    pub const COUNT: usize = 3;
}

/// Edge- and level-triggered input for one frame.
///
/// `held` is the current state; `pressed` and `released` compare against the
/// previous frame, so they fire exactly once. Call [`Input::end_frame`] after
/// the frame is done or the edges never clear.
#[derive(Clone, Debug)]
pub struct Input {
    keys: [bool; Key::COUNT],
    keys_prev: [bool; Key::COUNT],
    buttons: [bool; MouseButton::COUNT],
    buttons_prev: [bool; MouseButton::COUNT],
    mouse_delta: (f32, f32),
    scroll: f32,
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

impl Input {
    pub const fn new() -> Self {
        Self {
            keys: [false; Key::COUNT],
            keys_prev: [false; Key::COUNT],
            buttons: [false; MouseButton::COUNT],
            buttons_prev: [false; MouseButton::COUNT],
            mouse_delta: (0.0, 0.0),
            scroll: 0.0,
        }
    }

    pub fn set_key(&mut self, key: Key, down: bool) {
        self.keys[key as usize] = down;
    }

    pub fn set_button(&mut self, button: MouseButton, down: bool) {
        self.buttons[button as usize] = down;
    }

    /// Raw motion, accumulated over the frame. Pixels, not normalised.
    pub fn add_mouse_delta(&mut self, dx: f32, dy: f32) {
        self.mouse_delta.0 += dx;
        self.mouse_delta.1 += dy;
    }

    pub fn add_scroll(&mut self, lines: f32) {
        self.scroll += lines;
    }

    pub fn held(&self, key: Key) -> bool {
        self.keys[key as usize]
    }

    pub fn pressed(&self, key: Key) -> bool {
        self.keys[key as usize] && !self.keys_prev[key as usize]
    }

    pub fn released(&self, key: Key) -> bool {
        !self.keys[key as usize] && self.keys_prev[key as usize]
    }

    pub fn button_held(&self, button: MouseButton) -> bool {
        self.buttons[button as usize]
    }

    pub fn button_pressed(&self, button: MouseButton) -> bool {
        self.buttons[button as usize] && !self.buttons_prev[button as usize]
    }

    pub fn mouse_delta(&self) -> (f32, f32) {
        self.mouse_delta
    }

    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// `+1` while `pos` is held, `-1` while `neg` is, `0` for neither or both.
    pub fn axis(&self, neg: Key, pos: Key) -> f32 {
        f32::from(self.held(pos)) - f32::from(self.held(neg))
    }

    /// Roll the edge state forward and drop the per-frame deltas.
    pub fn end_frame(&mut self) {
        self.keys_prev = self.keys;
        self.buttons_prev = self.buttons;
        self.mouse_delta = (0.0, 0.0);
        self.scroll = 0.0;
    }

    /// Drop everything, including what is currently held.
    ///
    /// The window losing focus is the case that matters: a key released while
    /// another window has focus never reports the release, so without this the
    /// camera keeps flying after the user alt-tabs away.
    pub fn clear(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_fire_once() {
        let mut i = Input::new();
        i.set_key(Key::W, true);
        assert!(i.pressed(Key::W) && i.held(Key::W));
        i.end_frame();
        assert!(!i.pressed(Key::W), "pressed must not repeat while held");
        assert!(i.held(Key::W));
        i.set_key(Key::W, false);
        assert!(i.released(Key::W));
        i.end_frame();
        assert!(!i.released(Key::W));
    }

    #[test]
    fn axis_cancels_when_both_are_held() {
        let mut i = Input::new();
        assert_eq!(i.axis(Key::A, Key::D), 0.0);
        i.set_key(Key::D, true);
        assert_eq!(i.axis(Key::A, Key::D), 1.0);
        i.set_key(Key::A, true);
        assert_eq!(i.axis(Key::A, Key::D), 0.0, "both held is neutral, not jitter");
        i.set_key(Key::D, false);
        assert_eq!(i.axis(Key::A, Key::D), -1.0);
    }

    #[test]
    fn deltas_do_not_survive_the_frame() {
        let mut i = Input::new();
        i.add_mouse_delta(4.0, -2.0);
        i.add_mouse_delta(1.0, 1.0);
        assert_eq!(i.mouse_delta(), (5.0, -1.0));
        i.end_frame();
        assert_eq!(i.mouse_delta(), (0.0, 0.0));
    }

    /// Losing focus has to drop held keys, or the camera flies away on its own.
    #[test]
    fn clear_drops_held_state() {
        let mut i = Input::new();
        i.set_key(Key::W, true);
        i.set_button(MouseButton::Right, true);
        i.clear();
        assert!(!i.held(Key::W));
        assert!(!i.button_held(MouseButton::Right));
        assert!(!i.released(Key::W), "clear is not a release edge");
    }
}
