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

/// Porosity of stone / wood: how much of the wet darkening is `albedo²`.
///
/// A film of water traps light in the pores. Squaring the diffuse is the cheap
/// physical stand-in (the same one the Dagor wetness path uses). Doing only a
/// multiply, which [`RainCb::wet_albedo_scale`] did, darkens without filling
/// the pores — that is frost, not rain.
pub const WET_POROSITY: f32 = 0.6;
/// Linear roughness a fully wet dielectric settles to.
pub const WET_ROUGHNESS: f32 = 0.1;
/// F0 of a water film.
pub const WET_F0: f32 = 0.02;

/// `saturate((v - lo) / (hi - lo))`.
pub fn clamp_range(v: f32, lo: f32, hi: f32) -> f32 {
    ((v - lo) / (hi - lo).max(1e-3)).clamp(0.0, 1.0)
}

/// Staged wetness: darken by porosity, drop roughness, kill metalness.
///
/// Returns `(albedo, roughness, metalness)`. Each term has its own range so a
/// light drizzle is not a puddle and a puddle is not chrome. The test pins that
/// all three move, because any one of them alone is a known wrong look.
pub fn apply_wetness(wetness: f32, albedo: Vec3, roughness: f32, metalness: f32) -> (Vec3, f32, f32) {
    let w = wetness.clamp(0.0, 1.0);
    let dark = clamp_range(w, 0.0, 0.35) * WET_POROSITY;
    let albedo = albedo.lerp(albedo * albedo, dark);
    let roughness = roughness + (WET_ROUGHNESS - roughness) * clamp_range(w, 0.2, 1.0);
    let metalness = metalness * (1.0 - clamp_range(w, 0.25, 0.5));
    (albedo, roughness, metalness)
}

/// Puddles accumulate. Instant wetness is a switch; rain that started two
/// seconds ago must not look like rain that has been falling for ten minutes.
pub fn puddle_growth(prev: f32, intensity: f32, dt: f32, rate: f32, limit: f32) -> f32 {
    (prev + intensity.max(0.0) * rate.max(0.0) * dt.max(0.0)).clamp(0.0, limit.max(0.0))
}

/// Extra wetness where the ground meets the water. `border` is the height band
/// in world units; below the waterline it saturates to 1.
pub fn shore_wetness(world_y: f32, water_level: f32, border: f32) -> f32 {
    1.0 - clamp_range(world_y - water_level, -border, border)
}

/// Per-frame constants for the storm demo (water + rain + lights).
///
/// Gate-rain and gate-water keep their own CBs: this one is the composition,
/// and lives here so the layout test can pin it against the GLSL the demo
/// compiles. New fields go on the end.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StormCb {
    pub inv_view_proj: Mat4,
    pub view_proj: Mat4,
    pub view: Mat4,
    pub camera_pos: Vec4,
    pub sun_dir: Vec4,
    pub sun_color: Vec4,
    pub sky_zenith: Vec4,
    /// Horizon rgb; `w` = seabed depth.
    pub sky_horizon: Vec4,
    /// intensity, wetness, ripple strength, time.
    pub rain: Vec4,
    /// streak scale x, y, fall speed, brightness.
    pub streak: Vec4,
    /// ripple cell, frequency, speed, decay.
    pub ripple: Vec4,
    /// xy wind in screen/world, z = puddle growth, w = water level.
    pub wind: Vec4,
    pub shallow: Vec4,
    pub deep: Vec4,
    pub wave0: Vec4,
    pub wave1: Vec4,
    pub wave2: Vec4,
    pub wave3: Vec4,
    /// time, Gerstner Q, specular power, exposure.
    pub misc: Vec4,
    /// SSR steps, thickness, max distance, foam.
    pub ssr: Vec4,
    pub inv_extent: Vec2,
    pub scene_color: u32,
    pub scene_depth: u32,
    pub rain_map: u32,
    pub light_count: u32,
    /// Dry albedo rgb, dry roughness. Rewritten per draw group.
    pub material: Vec4,
    pub rain_map_vp: Mat4,
}

impl Default for StormCb {
    fn default() -> Self {
        Self {
            inv_view_proj: Mat4::IDENTITY,
            view_proj: Mat4::IDENTITY,
            view: Mat4::IDENTITY,
            camera_pos: Vec4::W,
            sun_dir: Vec4::new(0.22, 0.40, -0.89, 0.0),
            sun_color: Vec4::new(0.55, 0.60, 0.72, 1.0),
            sky_zenith: Vec4::new(0.22, 0.26, 0.34, 0.0),
            sky_horizon: Vec4::new(0.38, 0.42, 0.50, 6.0),
            rain: Vec4::new(0.9, 0.85, 1.0, 0.0),
            streak: Vec4::new(64.0, 0.5, 2.3, 0.34),
            ripple: Vec4::new(1.35, 26.0, 1.5, 3.2),
            wind: Vec4::new(0.55, 0.12, 0.0, 0.0),
            shallow: Vec4::new(0.08, 0.22, 0.26, 0.0),
            deep: Vec4::new(0.006, 0.03, 0.055, 0.16),
            wave0: Vec4::new(1.0, 0.0, 0.42, 34.0),
            wave1: Vec4::new(0.80, 0.60, 0.26, 17.0),
            wave2: Vec4::new(0.20, -0.98, 0.13, 8.5),
            wave3: Vec4::new(-0.55, 0.84, 0.06, 4.2),
            misc: Vec4::new(0.0, 0.62, 220.0, 1.0),
            ssr: Vec4::new(28.0, 0.9, 90.0, 0.55),
            inv_extent: Vec2::ONE,
            scene_color: 0,
            scene_depth: 0,
            rain_map: 0,
            light_count: 0,
            material: Vec4::new(0.30, 0.29, 0.27, 0.55),
            rain_map_vp: Mat4::IDENTITY,
        }
    }
}

impl StormCb {
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                std::mem::size_of::<Self>(),
            )
        }
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

    /// Staged wetness moves albedo *and* roughness *and* metalness.
    ///
    /// Darken-only is frost. Roughness-only is varnish. Metal that stays metal
    /// under a water film is a chrome puddle. Each assertion would pass if the
    /// other two terms were missing — together they refuse those three looks.
    #[test]
    fn staged_wetness_moves_all_three() {
        let dry = Vec3::new(0.45, 0.38, 0.30);
        let (wet_a, wet_r, wet_m) = apply_wetness(1.0, dry, 0.7, 0.8);
        let (damp_a, _damp_r, _damp_m) = apply_wetness(0.15, dry, 0.7, 0.8);
        let (_mid_a, mid_r, _mid_m) = apply_wetness(0.4, dry, 0.7, 0.8);
        assert!(wet_a.length() < dry.length(), "must darken");
        assert!(wet_r < 0.7 && wet_r >= WET_ROUGHNESS - 1e-5, "must smooth");
        assert!(wet_m < 0.8 * 0.5, "water film is not metal");
        // Albedo saturates by 0.35 (porosity band); roughness keeps moving after.
        assert!(damp_a.length() > wet_a.length(), "drizzle is lighter than a flood");
        assert!(mid_r > wet_r, "more wet → smoother");
        // Dry is a no-op.
        let (a0, r0, m0) = apply_wetness(0.0, dry, 0.7, 0.8);
        assert!((a0 - dry).length() < 1e-6 && (r0 - 0.7).abs() < 1e-6 && (m0 - 0.8).abs() < 1e-6);
    }

    /// A drizzle that has just started is not a flood. Growth is monotonic and
    /// capped, and intensity 0 does not invent puddles.
    #[test]
    fn puddles_accumulate_and_cap() {
        let mut g = 0.0;
        for _ in 0..30 {
            let next = puddle_growth(g, 0.9, 1.0 / 60.0, 0.8, 1.0);
            assert!(next >= g, "growth went backwards");
            g = next;
        }
        assert!(g > 0.2, "30 frames of rain left the ground dry: {g}");
        assert!(g <= 1.0);
        assert_eq!(puddle_growth(0.4, 0.0, 1.0, 10.0, 1.0), 0.4);
        assert_eq!(puddle_growth(0.95, 1.0, 1.0, 10.0, 0.9), 0.9);
    }

    /// The shore is wetter than the hill. Below the waterline it saturates.
    #[test]
    fn the_shore_is_wetter_than_the_hill() {
        let water = 0.0;
        let shore = shore_wetness(0.1, water, 0.6);
        let hill = shore_wetness(2.0, water, 0.6);
        let submerged = shore_wetness(-0.6, water, 0.6);
        assert!(shore > hill, "shore {shore} vs hill {hill}");
        assert!(submerged > shore);
        assert!((submerged - 1.0).abs() < 1e-5);
    }

    #[test]
    fn storm_cb_layout_matches_the_glsl() {
        use std::mem::offset_of;
        assert_eq!(std::mem::size_of::<StormCb>(), 576);
        assert!(std::mem::size_of::<StormCb>() <= harpia_rhi::FRAME_UBO_SIZE as usize);
        assert_eq!(offset_of!(StormCb, view_proj), 64);
        assert_eq!(offset_of!(StormCb, view), 128);
        assert_eq!(offset_of!(StormCb, camera_pos), 192);
        assert_eq!(offset_of!(StormCb, rain), 272);
        assert_eq!(offset_of!(StormCb, wind), 320);
        assert_eq!(offset_of!(StormCb, wave0), 368);
        assert_eq!(offset_of!(StormCb, misc), 432);
        assert_eq!(offset_of!(StormCb, inv_extent), 464);
        assert_eq!(offset_of!(StormCb, scene_color), 472);
        assert_eq!(offset_of!(StormCb, rain_map), 480);
        assert_eq!(offset_of!(StormCb, light_count), 484);
        assert_eq!(offset_of!(StormCb, material), 496);
        assert_eq!(offset_of!(StormCb, rain_map_vp), 512);

        let expected = [
            (0, 0),
            (1, offset_of!(StormCb, view_proj) as u32),
            (2, offset_of!(StormCb, view) as u32),
            (3, offset_of!(StormCb, camera_pos) as u32),
            (4, offset_of!(StormCb, sun_dir) as u32),
            (5, offset_of!(StormCb, sun_color) as u32),
            (6, offset_of!(StormCb, sky_zenith) as u32),
            (7, offset_of!(StormCb, sky_horizon) as u32),
            (8, offset_of!(StormCb, rain) as u32),
            (9, offset_of!(StormCb, streak) as u32),
            (10, offset_of!(StormCb, ripple) as u32),
            (11, offset_of!(StormCb, wind) as u32),
            (12, offset_of!(StormCb, shallow) as u32),
            (13, offset_of!(StormCb, deep) as u32),
            (14, offset_of!(StormCb, wave0) as u32),
            (15, offset_of!(StormCb, wave1) as u32),
            (16, offset_of!(StormCb, wave2) as u32),
            (17, offset_of!(StormCb, wave3) as u32),
            (18, offset_of!(StormCb, misc) as u32),
            (19, offset_of!(StormCb, ssr) as u32),
            (20, offset_of!(StormCb, inv_extent) as u32),
            (21, offset_of!(StormCb, scene_color) as u32),
            (22, offset_of!(StormCb, scene_depth) as u32),
            (23, offset_of!(StormCb, rain_map) as u32),
            (24, offset_of!(StormCb, light_count) as u32),
            (25, offset_of!(StormCb, material) as u32),
            (26, offset_of!(StormCb, rain_map_vp) as u32),
        ];
        for shader in [
            "prog/samples/storm/shaders/opaque.ps.glsl",
            "prog/samples/storm/shaders/water.ps.glsl",
            "prog/samples/storm/shaders/water.vs.glsl",
            "prog/samples/storm/shaders/rain.ps.glsl",
            "prog/samples/storm/shaders/sky.ps.glsl",
            "prog/samples/storm/shaders/copy.ps.glsl",
            "prog/samples/storm/shaders/blit.ps.glsl",
        ] {
            crate::spvasm_layout::assert_glsl_offsets(shader, &expected);
        }
    }
}
