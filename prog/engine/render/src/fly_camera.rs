//! A fly camera: WASD to move, right-drag or the arrow keys to look.
//!
//! Until now every sample hard-coded its camera, so validating anything meant
//! editing a literal and rebuilding. The terrain phase cannot work that way —
//! there is no way to judge a clipmap you cannot fly over.
//!
//! Angles are stored, not a matrix: yaw and pitch survive a resize and cannot
//! drift out of orthonormal the way an accumulated rotation matrix does.

use crate::Camera;
use harpia_core::{Input, Key, MouseButton};
use harpia_math::Vec3;

/// Just short of straight up. Exactly ±90° makes `forward` parallel to `up` and
/// the view matrix degenerate, so the clamp leaves a degree of room.
const PITCH_LIMIT: f32 = 1.5533; // 89 degrees

#[derive(Clone, Copy, Debug)]
pub struct FlyCamera {
    pub position: Vec3,
    /// Radians about +Y. 0 looks down −Z, matching `Mat4::look_at_rh`.
    pub yaw: f32,
    /// Radians, positive looks up. Clamped to ±89°.
    pub pitch: f32,
    /// World units per second.
    pub speed: f32,
    /// Multiplier while Shift is held.
    pub boost: f32,
    /// Radians per pixel of mouse motion.
    pub sensitivity: f32,
    /// Radians per second for the arrow keys.
    pub key_look_speed: f32,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for FlyCamera {
    fn default() -> Self {
        Self {
            position: Vec3::new(0.0, 2.0, 8.0),
            yaw: 0.0,
            pitch: 0.0,
            speed: 6.0,
            boost: 5.0,
            sensitivity: 0.0022,
            key_look_speed: 1.6,
            fov_y: 55.0_f32.to_radians(),
            near: 0.2,
            far: 400.0,
        }
    }
}

impl FlyCamera {
    /// Seed from the `eye`/`target` pair a sample already has, so switching a
    /// hard-coded camera over to this one does not move the shot.
    pub fn looking_at(eye: Vec3, target: Vec3) -> Self {
        let d = (target - eye).normalize_or_zero();
        // forward = (-sin(yaw)cos(pitch), sin(pitch), -cos(yaw)cos(pitch))
        let pitch = d.y.clamp(-1.0, 1.0).asin();
        let yaw = (-d.x).atan2(-d.z);
        Self {
            position: eye,
            yaw,
            pitch: pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT),
            ..Self::default()
        }
    }

    /// Unit vector the camera is looking along.
    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        // yaw 0, pitch 0 must be (0, 0, -1)
        Vec3::new(-sy * cp, sp, -cy * cp)
    }

    /// Unit vector to the camera's right, level with the horizon.
    pub fn right(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        Vec3::new(cy, 0.0, -sy)
    }

    /// Advance by `dt` seconds.
    ///
    /// Looking needs the right mouse button held — grabbing the cursor
    /// unasked makes a window impossible to leave, and a gate window that
    /// swallows the pointer is worse than one that ignores it.
    pub fn update(&mut self, input: &Input, dt: f32) {
        if input.button_held(MouseButton::Right) {
            let (dx, dy) = input.mouse_delta();
            self.yaw -= dx * self.sensitivity;
            self.pitch -= dy * self.sensitivity;
        }
        let look_x = input.axis(Key::Left, Key::Right);
        let look_y = input.axis(Key::Down, Key::Up);
        self.yaw -= look_x * self.key_look_speed * dt;
        self.pitch += look_y * self.key_look_speed * dt;
        self.pitch = self.pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);

        let fwd = input.axis(Key::S, Key::W);
        let strafe = input.axis(Key::A, Key::D);
        let lift = input.axis(Key::Q, Key::E);
        let mut step = self.forward() * fwd + self.right() * strafe + Vec3::Y * lift;
        // Diagonals must not be faster than a straight line.
        if step.length_squared() > 1.0 {
            step = step.normalize();
        }
        let speed = if input.held(Key::Shift) {
            self.speed * self.boost
        } else {
            self.speed
        };
        self.position += step * (speed * dt);

        // Scroll trims the speed rather than the FOV: a zoomed gate capture is
        // a different measurement, but a faster camera is the same one.
        let scroll = input.scroll();
        if scroll != 0.0 {
            self.speed = (self.speed * (1.0 + scroll * 0.1)).clamp(0.05, 500.0);
        }
    }

    pub fn camera(&self, aspect: f32) -> Camera {
        Camera {
            eye: self.position,
            target: self.position + self.forward(),
            up: Vec3::Y,
            fov_y: self.fov_y,
            aspect,
            near: self.near,
            far: self.far,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    /// yaw 0 / pitch 0 has to agree with `look_at_rh`, which looks down −Z.
    #[test]
    fn default_looks_down_minus_z() {
        let c = FlyCamera::default();
        let f = c.forward();
        assert!(approx(f.x, 0.0) && approx(f.y, 0.0) && approx(f.z, -1.0), "{f:?}");
        let r = c.right();
        assert!(approx(r.x, 1.0) && approx(r.y, 0.0) && approx(r.z, 0.0), "{r:?}");
    }

    /// The whole point of `looking_at`: the shot must not move.
    #[test]
    fn looking_at_round_trips_the_direction() {
        for (eye, target) in [
            (Vec3::new(-9.5, 1.8, 0.0), Vec3::new(0.0, 1.6, 0.0)),
            (Vec3::new(0.0, 2.4, 26.0), Vec3::new(0.0, 2.3, 25.0)),
            (Vec3::new(3.0, 9.0, -4.0), Vec3::new(-1.0, 0.0, 2.0)),
        ] {
            let want = (target - eye).normalize();
            let got = FlyCamera::looking_at(eye, target).forward();
            assert!((want - got).length() < 1e-5, "{want:?} vs {got:?}");
        }
    }

    #[test]
    fn pitch_cannot_flip_over() {
        let mut c = FlyCamera::default();
        let mut i = Input::new();
        i.set_key(Key::Up, true);
        for _ in 0..600 {
            c.update(&i, 1.0 / 60.0);
        }
        assert!(c.pitch <= PITCH_LIMIT, "{}", c.pitch);
        // still a usable basis: forward never becomes parallel to up
        assert!(c.forward().cross(Vec3::Y).length() > 0.01);
    }

    #[test]
    fn w_moves_along_forward_at_speed() {
        let mut c = FlyCamera::default();
        let start = c.position;
        let mut i = Input::new();
        i.set_key(Key::W, true);
        c.update(&i, 0.5);
        let moved = c.position - start;
        assert!(approx(moved.z, -c.speed * 0.5), "{moved:?}");
    }

    /// Holding W and D must not travel faster than W alone.
    #[test]
    fn diagonals_are_not_faster() {
        let mut straight = FlyCamera::default();
        let mut diagonal = FlyCamera::default();
        let mut i = Input::new();
        i.set_key(Key::W, true);
        straight.update(&i, 0.5);
        i.set_key(Key::D, true);
        diagonal.update(&i, 0.5);
        let a = (straight.position - FlyCamera::default().position).length();
        let b = (diagonal.position - FlyCamera::default().position).length();
        assert!((a - b).abs() < 1e-4, "straight {a} vs diagonal {b}");
    }

    /// The property every gate depends on: no input, no movement.
    #[test]
    fn idle_input_never_moves_the_camera() {
        let mut c = FlyCamera::default();
        let before = (c.position, c.yaw, c.pitch);
        let i = Input::new();
        for _ in 0..1000 {
            c.update(&i, 1.0 / 60.0);
        }
        assert_eq!((c.position, c.yaw, c.pitch), before);
    }

    #[test]
    fn look_needs_the_right_button() {
        let mut c = FlyCamera::default();
        let mut i = Input::new();
        i.add_mouse_delta(120.0, 40.0);
        c.update(&i, 1.0 / 60.0);
        assert_eq!((c.yaw, c.pitch), (0.0, 0.0), "mouse alone must not turn it");
        i.set_button(MouseButton::Right, true);
        c.update(&i, 1.0 / 60.0);
        assert!(c.yaw != 0.0 && c.pitch != 0.0);
    }
}
