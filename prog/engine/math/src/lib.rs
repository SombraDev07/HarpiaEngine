//! Engine math is [`glam`]. This crate exists so call sites pin one version.

pub use glam::*;

/// Right-handed perspective for **Vulkan** clip space.
///
/// `glam::Mat4::perspective_rh` already maps depth to `[0,1]`, but it keeps
/// OpenGL's "+Y is up" NDC. Vulkan's framebuffer origin is top-left, so a raw
/// RH projection renders the whole image mirrored vertically — and, because
/// facing is decided *after* the viewport transform, it also flips the winding,
/// so `FRONT_FACE_COUNTER_CLOCKWISE` + cull-back ends up culling the front
/// faces and showing the inside of everything. Negating the Y row fixes both.
pub fn perspective_vk(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let mut m = Mat4::perspective_rh(fov_y, aspect, near, far);
    m.y_axis.y = -m.y_axis.y;
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vk_projection_flips_y_only() {
        let gl = Mat4::perspective_rh(1.0, 1.6, 0.1, 100.0);
        let vk = perspective_vk(1.0, 1.6, 0.1, 100.0);
        assert_eq!(vk.y_axis.y, -gl.y_axis.y);
        assert_eq!(vk.x_axis, gl.x_axis);
        assert_eq!(vk.z_axis, gl.z_axis);
        assert_eq!(vk.w_axis, gl.w_axis);
    }

    /// World up must land in the *upper* half of the framebuffer.
    #[test]
    fn world_up_is_screen_up() {
        let view = Mat4::look_at_rh(Vec3::ZERO, -Vec3::Z, Vec3::Y);
        let clip = perspective_vk(1.0, 1.0, 0.1, 100.0) * view * Vec4::new(0.0, 1.0, -5.0, 1.0);
        // Vulkan NDC: y = -1 is the top of the framebuffer.
        assert!(clip.y / clip.w < 0.0);
    }
}
