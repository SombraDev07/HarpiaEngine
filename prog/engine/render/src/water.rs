//! Water surface (roadmap §5 / fase 5).
//!
//! Gerstner waves on a tessellated grid, Fresnel against a sky reflection, and a
//! depth-based body colour. No SSR yet — that needs the scene depth and is the
//! next slice — so the reflection is the analytic sky, which is what a Fresnel
//! term is mostly showing at grazing angles anyway.

use harpia_math::{Mat4, Vec2, Vec4};

/// Quads a side on the water grid. The surface can only be as detailed as this:
/// Gerstner displaces vertices, it does not tessellate.
pub const WATER_GRID: u32 = 192;
/// Half-extent of the water plane, world units.
pub const WATER_HALF: f32 = 90.0;

/// Deep-water dispersion: a wave of length `l` travels at `sqrt(g / k)`.
/// Returned as the angular frequency, which is what the VS needs.
pub fn wave_omega(wavelength: f32) -> f32 {
    let k = std::f32::consts::TAU / wavelength.max(0.01);
    (9.81 * k).sqrt()
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WaterCb {
    pub inv_view_proj: Mat4,
    pub camera_pos: Vec4,
    pub sun_dir: Vec4,
    pub sun_color: Vec4,
    /// Shallow water tint, rgb. `w` unused.
    pub shallow: Vec4,
    /// Deep water tint rgb, `w` = how fast the body colour saturates with depth.
    pub deep: Vec4,
    /// Per wave: direction xz (unit), amplitude, wavelength.
    pub wave0: Vec4,
    pub wave1: Vec4,
    pub wave2: Vec4,
    pub wave3: Vec4,
    /// time, Gerstner steepness `Q`, specular power, exposure.
    pub misc: Vec4,
    /// Sky radiance at the zenith, rgb. `w` unused.
    pub sky_zenith: Vec4,
    /// Sky radiance at the horizon, rgb. `w` = seabed depth in world units.
    pub sky_horizon: Vec4,
    pub inv_extent: Vec2,
    /// Bindless index of the HDR target, for the tonemap pass.
    pub hdr: u32,
    pub _pad: u32,
}

impl Default for WaterCb {
    fn default() -> Self {
        Self {
            inv_view_proj: Mat4::IDENTITY,
            camera_pos: Vec4::W,
            sun_dir: Vec4::Y,
            sun_color: Vec4::new(5.2, 4.8, 4.2, 1.0),
            shallow: Vec4::new(0.10, 0.32, 0.36, 0.0),
            deep: Vec4::new(0.008, 0.045, 0.075, 0.16),
            // Four wavelengths an octave apart, fanned around +X so the surface
            // never looks like a single travelling ripple.
            wave0: Vec4::new(1.0, 0.0, 0.42, 34.0),
            wave1: Vec4::new(0.80, 0.60, 0.26, 17.0),
            wave2: Vec4::new(0.20, -0.98, 0.13, 8.5),
            wave3: Vec4::new(-0.55, 0.84, 0.06, 4.2),
            misc: Vec4::new(0.0, 0.62, 220.0, 1.0),
            sky_zenith: Vec4::new(0.10, 0.20, 0.42, 0.0),
            sky_horizon: Vec4::new(0.52, 0.62, 0.78, 6.0),
            inv_extent: Vec2::ONE,
            hdr: 0,
            _pad: 0,
        }
    }
}

impl WaterCb {
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
        assert_eq!(std::mem::size_of::<WaterCb>(), 272);
        assert!(std::mem::size_of::<WaterCb>() <= harpia_rhi::FRAME_UBO_SIZE as usize);

        let expected = [
            (0, 0),
            (1, offset_of!(WaterCb, camera_pos) as u32),
            (2, offset_of!(WaterCb, sun_dir) as u32),
            (3, offset_of!(WaterCb, sun_color) as u32),
            (4, offset_of!(WaterCb, shallow) as u32),
            (5, offset_of!(WaterCb, deep) as u32),
            (6, offset_of!(WaterCb, wave0) as u32),
            (7, offset_of!(WaterCb, wave1) as u32),
            (8, offset_of!(WaterCb, wave2) as u32),
            (9, offset_of!(WaterCb, wave3) as u32),
            (10, offset_of!(WaterCb, misc) as u32),
            (11, offset_of!(WaterCb, sky_zenith) as u32),
            (12, offset_of!(WaterCb, sky_horizon) as u32),
            (13, offset_of!(WaterCb, inv_extent) as u32),
            (14, offset_of!(WaterCb, hdr) as u32),
        ];
        for shader in [
            "prog/samples/gates/water/shaders/water.vs.spvasm",
            "prog/samples/gates/water/shaders/water.ps.spvasm",
            "prog/samples/gates/water/shaders/sky.ps.spvasm",
            "prog/samples/gates/water/shaders/tonemap.ps.spvasm",
        ] {
            crate::spvasm_layout::assert_prefix_matches(shader, "Water", &expected);
        }
    }

    /// Shorter waves move slower, which is what keeps the surface from looking
    /// like one rigid sheet sliding past.
    #[test]
    fn dispersion_is_deep_water() {
        assert!(wave_omega(34.0) < wave_omega(8.5));
        // omega = sqrt(g * 2pi / l): 34 m swell is about 1.34 rad/s
        assert!((wave_omega(34.0) - 1.345).abs() < 0.01, "{}", wave_omega(34.0));
    }
}
