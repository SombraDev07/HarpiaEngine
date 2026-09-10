//! Rain (roadmap §5 / fase 5, last).
//!
//! Two halves, which is what the roadmap means by "GBuffer wet + post":
//!
//! * **Wet material.** Water soaking a surface darkens its albedo and smooths it,
//!   so the same rock reads darker *and* shinier. Up-facing surfaces additionally
//!   collect standing water, which flattens their normal towards straight up and
//!   carries ripples.
//! * **Streaks**, as a screen-space post pass. Simulating drops as geometry costs
//!   far more than it shows at these speeds — a streak crosses the frame in a few
//!   frames and is never seen still.

use harpia_math::{Mat4, Vec2, Vec3, Vec4};

/// Side of the top-down rain map, in texels.
pub const RAIN_MAP_SIZE: u32 = 1024;

/// Orthographic view-projection looking straight down over `centre`.
///
/// Screen-space streaks fall through a roof, because a screen-space pass has no
/// idea there is one. This is the standard answer: render depth from above once,
/// and a pixel whose surface is not the topmost thing at its position is under
/// cover — no streaks, and no wetness either.
///
/// Looking straight down makes `up` degenerate, so the basis uses `-Z`.
pub fn rain_map_view_proj(centre: Vec3, half_extent: f32, height: f32) -> Mat4 {
    let eye = Vec3::new(centre.x, centre.y + height, centre.z);
    let view = Mat4::look_at_rh(eye, centre, Vec3::NEG_Z);
    let half = half_extent.max(0.001);
    // Reverse-less ortho into Vulkan's [0,1] depth, matching `perspective_vk`.
    let proj = Mat4::orthographic_rh(-half, half, -half, half, 0.0, height * 2.0);
    proj * view
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RainCb {
    pub inv_view_proj: Mat4,
    pub view_proj: Mat4,
    pub camera_pos: Vec4,
    pub sun_dir: Vec4,
    pub sun_color: Vec4,
    /// Sky radiance at the zenith, rgb.
    pub sky_zenith: Vec4,
    /// Sky radiance at the horizon rgb; `w` = ground height in world units.
    pub sky_horizon: Vec4,
    /// intensity `[0,1]`, wetness `[0,1]`, ripple strength, time in seconds.
    pub rain: Vec4,
    /// streak scale x, streak scale y, fall speed, streak brightness.
    pub streak: Vec4,
    /// ripple cell size (world units), ring frequency, ring speed, decay.
    pub ripple: Vec4,
    pub inv_extent: Vec2,
    pub scene_color: u32,
    pub scene_depth: u32,
    /// exposure, rain-map texel size, bias in world units, unused.
    pub misc: Vec4,
    /// Top-down depth of the scene. 0 = everything is exposed.
    pub rain_map: u32,
    pub _pad: [u32; 3],
    pub rain_map_vp: Mat4,
}

impl Default for RainCb {
    fn default() -> Self {
        Self {
            inv_view_proj: Mat4::IDENTITY,
            view_proj: Mat4::IDENTITY,
            camera_pos: Vec4::W,
            sun_dir: Vec4::Y,
            // Overcast: the sun is behind cloud, so it is dim and the sky does
            // most of the lighting. A sunny rainstorm reads as a bug.
            sun_color: Vec4::new(2.6, 2.6, 2.7, 1.0),
            sky_zenith: Vec4::new(1.55, 1.65, 1.85, 0.0),
            sky_horizon: Vec4::new(1.30, 1.36, 1.48, 0.0),
            rain: Vec4::new(0.85, 0.9, 1.0, 0.0),
            streak: Vec4::new(64.0, 0.5, 2.3, 0.34),
            ripple: Vec4::new(1.35, 26.0, 1.5, 3.2),
            inv_extent: Vec2::ONE,
            scene_color: 0,
            scene_depth: 0,
            misc: Vec4::new(1.0, 1.0 / RAIN_MAP_SIZE as f32, 0.15, 0.0),
            rain_map: 0,
            _pad: [0; 3],
            rain_map_vp: Mat4::IDENTITY,
        }
    }
}

impl RainCb {
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                std::mem::size_of::<Self>(),
            )
        }
    }

    /// Albedo scale for a fully wet surface.
    ///
    /// Water fills the surface roughness and traps light in it, so a wet
    /// material reflects less diffusely — about half, which is why tarmac goes
    /// almost black in rain. Kept here rather than in the shader so the test can
    /// pin the direction.
    pub fn wet_albedo_scale(wetness: f32) -> f32 {
        1.0 - 0.55 * wetness.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cb_layout_matches_the_spvasm() {
        use std::mem::offset_of;
        assert_eq!(std::mem::size_of::<RainCb>(), 368);
        assert!(std::mem::size_of::<RainCb>() <= harpia_rhi::FRAME_UBO_SIZE as usize);

        let expected = [
            (0, 0),
            (1, offset_of!(RainCb, view_proj) as u32),
            (2, offset_of!(RainCb, camera_pos) as u32),
            (3, offset_of!(RainCb, sun_dir) as u32),
            (4, offset_of!(RainCb, sun_color) as u32),
            (5, offset_of!(RainCb, sky_zenith) as u32),
            (6, offset_of!(RainCb, sky_horizon) as u32),
            (7, offset_of!(RainCb, rain) as u32),
            (8, offset_of!(RainCb, streak) as u32),
            (9, offset_of!(RainCb, ripple) as u32),
            (10, offset_of!(RainCb, inv_extent) as u32),
            (11, offset_of!(RainCb, scene_color) as u32),
            (12, offset_of!(RainCb, scene_depth) as u32),
            (13, offset_of!(RainCb, misc) as u32),
            (14, offset_of!(RainCb, rain_map) as u32),
            (15, offset_of!(RainCb, rain_map_vp) as u32),
        ];
        for shader in [
            "prog/samples/gates/rain/shaders/scene.ps.spvasm",
            "prog/samples/gates/rain/shaders/rain.ps.spvasm",
        ] {
            crate::spvasm_layout::assert_prefix_matches(shader, "Rain", &expected);
        }
        // blit.ps is GLSL now: it names byte offsets, not member indices.
        crate::spvasm_layout::assert_glsl_offsets(
            "prog/samples/gates/rain/shaders/blit.ps.glsl",
            &expected,
        );
    }

    /// Looking straight down must still give a usable basis.
    #[test]
    fn rain_map_projects_below_the_eye_into_the_middle() {
        let vp = rain_map_view_proj(Vec3::new(3.0, 0.0, -4.0), 20.0, 30.0);
        let under = vp * harpia_math::Vec4::new(3.0, 0.0, -4.0, 1.0);
        assert!(under.w.abs() > 0.0, "ortho keeps w");
        assert!(under.x.abs() < 1e-4 && under.y.abs() < 1e-4, "{under:?}");
        // a point 10 units to the +X side must land to one side, not off the map
        let side = vp * harpia_math::Vec4::new(13.0, 0.0, -4.0, 1.0);
        assert!(side.x.abs() > 0.4 && side.x.abs() < 1.0, "{side:?}");
        // higher is nearer the ortho eye, so its depth must be smaller
        let low = vp * harpia_math::Vec4::new(3.0, 0.0, -4.0, 1.0);
        let high = vp * harpia_math::Vec4::new(3.0, 8.0, -4.0, 1.0);
        assert!(high.z < low.z, "high {} vs low {}", high.z, low.z);
    }

    /// Wet darkens. Getting this backwards makes rain look like frost.
    #[test]
    fn wetness_darkens_monotonically() {
        assert_eq!(RainCb::wet_albedo_scale(0.0), 1.0);
        assert!(RainCb::wet_albedo_scale(1.0) < RainCb::wet_albedo_scale(0.5));
        assert!(RainCb::wet_albedo_scale(0.5) < RainCb::wet_albedo_scale(0.0));
        assert!(RainCb::wet_albedo_scale(1.0) > 0.0, "never fully black");
        // out of range must not invert it
        assert_eq!(RainCb::wet_albedo_scale(-3.0), 1.0);
        assert_eq!(RainCb::wet_albedo_scale(9.0), RainCb::wet_albedo_scale(1.0));
    }
}
