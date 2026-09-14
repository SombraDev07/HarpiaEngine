//! Sky and atmosphere, Hillaire 2020 (`A Scalable and Production Ready Sky and
//! Atmosphere Rendering Technique`) instead of the roadmap's Bruneton bake.
//!
//! Same physics, different bookkeeping: three small LUTs rebuilt every frame
//! rather than one large 4D table baked at init, so the time of day is free to
//! move. See `memory/DECISIONS.md` D19.
//!
//! The LUTs are rendered by fullscreen passes into `RGBA16F` colour targets, not
//! by compute: they are 2D, one output per texel, no group sharing. That keeps
//! them on the proven heap path. Only the aerial-perspective froxel volume needs
//! a 3D UAV, and that reuses the fog plumbing.
//!
//! **Parameterisations are the contract** between the shader that writes a LUT
//! and the shader that reads it. They live here so both ends quote one source:
//!
//! * transmittance `(u, v)` → `(mu, r)` by Bruneton's mapping:
//!   `H = sqrt(top² - bottom²)`, `rho = H·v`, `r = sqrt(rho² + bottom²)`,
//!   `d = (top - r) + u·(rho + H - (top - r))`,
//!   `mu = (H² - rho² - d²) / (2·r·d)`.
//! * multiscattering `(u, v)` → `(cos sun zenith = 2u - 1, r = bottom + v·(top - bottom))`.
//! * sky-view `(u, v)` → azimuth `2π·u`, and a zenith mapping that spends half
//!   the texels below the horizon: `v < 0.5` is sky, `v >= 0.5` is ground, each
//!   squared so the horizon gets the resolution.

use harpia_math::{Mat4, Vec2, Vec4};
use harpia_rhi::{Format, TextureDesc, TextureDim};

/// Earth, in kilometres. Everything in this module is km and km⁻¹.
pub const BOTTOM_RADIUS_KM: f32 = 6360.0;
pub const TOP_RADIUS_KM: f32 = 6460.0;

/// Rayleigh scattering at sea level, km⁻¹, and its scale height in km.
pub const RAYLEIGH_SCATTER: [f32; 3] = [0.005802, 0.013558, 0.033100];
pub const RAYLEIGH_SCALE_KM: f32 = 8.0;

/// Mie scattering / absorption at sea level, km⁻¹, scale height, phase `g`.
pub const MIE_SCATTER: f32 = 0.003996;
pub const MIE_ABSORB: f32 = 0.004440;
pub const MIE_SCALE_KM: f32 = 1.2;
pub const MIE_G: f32 = 0.8;

/// Ozone absorbs but does not scatter. Tent function, centre and half width km.
pub const OZONE_ABSORB: [f32; 3] = [0.000650, 0.001881, 0.000085];
pub const OZONE_CENTRE_KM: f32 = 25.0;
pub const OZONE_WIDTH_KM: f32 = 15.0;

pub const LUT_FORMAT: Format = Format::Rgba16Float;
pub const TRANSMITTANCE_W: u32 = 256;
pub const TRANSMITTANCE_H: u32 = 64;
pub const MULTISCATTER_SIZE: u32 = 32;
pub const SKYVIEW_W: u32 = 192;
pub const SKYVIEW_H: u32 = 108;

/// Aerial perspective froxels, same shape as Hillaire's 32³.
pub const AERIAL_SIZE: u32 = 32;
pub const AERIAL_DEPTH_KM: f32 = 32.0;

/// Raymarch budgets. The multiscattering pass walks `MS_DIRS` directions spread
/// by the golden-angle spiral — one loop instead of Hillaire's 8×8 grid, and
/// better distributed for the same count.
pub const TRANSMITTANCE_STEPS: f32 = 40.0;
pub const MS_DIRS: f32 = 64.0;
pub const MS_STEPS: f32 = 20.0;
pub const SKYVIEW_STEPS: f32 = 32.0;

pub fn lut_desc(width: u32, height: u32) -> TextureDesc {
    TextureDesc {
        width,
        height,
        format: LUT_FORMAT,
        sampled: true,
        color_attachment: true,
        ..Default::default()
    }
}

pub fn transmittance_desc() -> TextureDesc {
    lut_desc(TRANSMITTANCE_W, TRANSMITTANCE_H)
}

pub fn multiscatter_desc() -> TextureDesc {
    lut_desc(MULTISCATTER_SIZE, MULTISCATTER_SIZE)
}

pub fn skyview_desc() -> TextureDesc {
    lut_desc(SKYVIEW_W, SKYVIEW_H)
}

pub fn aerial_desc() -> TextureDesc {
    TextureDesc {
        width: AERIAL_SIZE,
        height: AERIAL_SIZE,
        depth_slices: AERIAL_SIZE,
        dim: TextureDim::D3,
        format: LUT_FORMAT,
        sampled: true,
        storage: true,
        ..Default::default()
    }
}

/// Workgroup for the terrain aerial compute shader: 8×8×4 covers the 32³ volume
/// in one dispatch.
pub const AERIAL_GROUP_X: u32 = 8;
pub const AERIAL_GROUP_Y: u32 = 8;
pub const AERIAL_GROUP_Z: u32 = 4;

pub fn aerial_dispatch() -> (u32, u32, u32) {
    (
        AERIAL_SIZE / AERIAL_GROUP_X,
        AERIAL_SIZE / AERIAL_GROUP_Y,
        AERIAL_SIZE / AERIAL_GROUP_Z,
    )
}

/// Clipmap-range aerial far plane, in km.
///
/// Hillaire's 32 km is for mountains. The terrain clipmap ends at ~1 km, so a
/// 32 km volume would spend 31 slices on empty air and the haze on the hills
/// would be invisible. 2 km puts the far edge of the clipmap near the last
/// slice. Densities are still the Hillaire coefficients; `mie.w` scales them
/// so 1 km of air reads as atmosphere instead of a fade-to-sky mix.
pub const TERRAIN_AERIAL_FAR_KM: f32 = 2.0;
/// Multiply Rayleigh/Mie so a kilometre of clipmap has a visible optical depth.
pub const TERRAIN_AERIAL_DENSITY_SCALE: f32 = 12.0;
pub const AERIAL_MARCH_STEPS: f32 = 8.0;

/// Fills the aerial-perspective 32³. Same physics numbers as [`AtmosphereCb`],
/// shorter far plane, no LUT indices.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AerialCb {
    pub inv_view_proj: Mat4,
    /// World camera in metres, `w` = altitude km.
    pub camera_pos: Vec4,
    pub sun_dir: Vec4,
    pub sun_illuminance: Vec4,
    /// bottom km, top km, mie `g`, far km.
    pub radii: Vec4,
    /// Rayleigh scattering km⁻¹, `w` = scale height km.
    pub rayleigh: Vec4,
    /// mie scatter, mie absorption, mie scale height, density scale.
    pub mie: Vec4,
    /// march steps, unused.
    pub steps: Vec4,
}

impl Default for AerialCb {
    fn default() -> Self {
        Self {
            inv_view_proj: Mat4::IDENTITY,
            camera_pos: Vec4::new(0.0, 0.0, 0.0, 0.2),
            sun_dir: Vec4::Y,
            sun_illuminance: Vec4::new(3.4, 3.2, 2.9, 1.0),
            radii: Vec4::new(
                BOTTOM_RADIUS_KM,
                TOP_RADIUS_KM,
                MIE_G,
                TERRAIN_AERIAL_FAR_KM,
            ),
            rayleigh: Vec4::new(
                RAYLEIGH_SCATTER[0],
                RAYLEIGH_SCATTER[1],
                RAYLEIGH_SCATTER[2],
                RAYLEIGH_SCALE_KM,
            ),
            mie: Vec4::new(
                MIE_SCATTER,
                MIE_ABSORB,
                MIE_SCALE_KM,
                TERRAIN_AERIAL_DENSITY_SCALE,
            ),
            steps: Vec4::new(AERIAL_MARCH_STEPS, 0.0, 0.0, 0.0),
        }
    }
}

impl AerialCb {
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                std::mem::size_of::<Self>(),
            )
        }
    }
}

/// Per-frame atmosphere constants. Matches the `.spvasm` LUT shaders.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AtmosphereCb {
    pub inv_view_proj: Mat4,
    /// World camera, `w` = altitude above the ground in km.
    pub camera_pos: Vec4,
    /// Direction **towards** the sun.
    pub sun_dir: Vec4,
    pub sun_illuminance: Vec4,
    /// bottom km, top km, mie `g`, multiscatter strength.
    pub radii: Vec4,
    /// Rayleigh scattering km⁻¹, `w` = scale height km.
    pub rayleigh: Vec4,
    /// mie scatter, mie absorption, mie scale height, unused.
    pub mie: Vec4,
    /// Ozone absorption km⁻¹, `w` unused.
    pub ozone: Vec4,
    /// ozone centre km, ozone half width km, unused, unused.
    pub ozone_tent: Vec4,
    /// transmittance steps, multiscatter directions, multiscatter steps, sky-view steps.
    pub steps: Vec4,
    pub transmittance_lut: u32,
    pub multiscatter_lut: u32,
    pub skyview_lut: u32,
    pub _pad0: u32,
    pub inv_extent: Vec2,
    pub _pad1: Vec2,
}

impl Default for AtmosphereCb {
    fn default() -> Self {
        Self {
            inv_view_proj: Mat4::IDENTITY,
            camera_pos: Vec4::new(0.0, 0.0, 0.0, 0.2),
            sun_dir: Vec4::Y,
            sun_illuminance: Vec4::new(10.0, 10.0, 10.0, 1.0),
            radii: Vec4::new(BOTTOM_RADIUS_KM, TOP_RADIUS_KM, MIE_G, 1.0),
            rayleigh: Vec4::new(
                RAYLEIGH_SCATTER[0],
                RAYLEIGH_SCATTER[1],
                RAYLEIGH_SCATTER[2],
                RAYLEIGH_SCALE_KM,
            ),
            mie: Vec4::new(MIE_SCATTER, MIE_ABSORB, MIE_SCALE_KM, 0.0),
            ozone: Vec4::new(OZONE_ABSORB[0], OZONE_ABSORB[1], OZONE_ABSORB[2], 0.0),
            ozone_tent: Vec4::new(OZONE_CENTRE_KM, OZONE_WIDTH_KM, 0.0, 0.0),
            steps: Vec4::new(TRANSMITTANCE_STEPS, MS_DIRS, MS_STEPS, SKYVIEW_STEPS),
            transmittance_lut: 0,
            multiscatter_lut: 0,
            skyview_lut: 0,
            _pad0: 0,
            inv_extent: Vec2::ONE,
            _pad1: Vec2::ZERO,
        }
    }
}

impl AtmosphereCb {
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                std::mem::size_of::<Self>(),
            )
        }
    }
}

/// Distance from a point at radius `r` with view cosine `mu` to the top of the
/// atmosphere. Used to check the LUT mapping on the CPU side.
pub fn distance_to_top(r: f32, mu: f32, top: f32) -> f32 {
    let disc = r * r * (mu * mu - 1.0) + top * top;
    (-r * mu + disc.max(0.0).sqrt()).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cb_layout_matches_the_spvasm() {
        use std::mem::offset_of;
        assert_eq!(std::mem::size_of::<AtmosphereCb>(), 240);
        assert_eq!(std::mem::offset_of!(AtmosphereCb, camera_pos), 64);
        assert_eq!(std::mem::offset_of!(AtmosphereCb, sun_dir), 80);
        assert_eq!(std::mem::offset_of!(AtmosphereCb, sun_illuminance), 96);
        assert_eq!(std::mem::offset_of!(AtmosphereCb, radii), 112);
        assert_eq!(std::mem::offset_of!(AtmosphereCb, rayleigh), 128);
        assert_eq!(std::mem::offset_of!(AtmosphereCb, mie), 144);
        assert_eq!(std::mem::offset_of!(AtmosphereCb, ozone), 160);
        assert_eq!(std::mem::offset_of!(AtmosphereCb, ozone_tent), 176);
        assert_eq!(std::mem::offset_of!(AtmosphereCb, steps), 192);
        assert_eq!(std::mem::offset_of!(AtmosphereCb, transmittance_lut), 208);
        assert_eq!(std::mem::offset_of!(AtmosphereCb, inv_extent), 224);
        assert!(std::mem::size_of::<AtmosphereCb>() <= harpia_rhi::FRAME_UBO_SIZE as usize);

        // Every shader that binds this block has to agree with the struct.
        let base = offset_of!(AtmosphereCb, transmittance_lut) as u32;
        let expected = [
            (0, 0),
            (1, 64),
            (2, 80),
            (3, 96),
            (4, 112),
            (5, 128),
            (6, 144),
            (7, 160),
            (8, 176),
            (9, 192),
            (10, base),
            (11, base + 4),
            (12, base + 8),
            (13, base + 12),
            (14, offset_of!(AtmosphereCb, inv_extent) as u32),
        ];
        for shader in [
            "prog/samples/gates/sky/shaders/transmittance.ps.spvasm",
            "prog/samples/gates/sky/shaders/multiscatter.ps.spvasm",
            "prog/samples/gates/sky/shaders/skyview.ps.spvasm",
            "prog/samples/gates/sky/shaders/sky.ps.spvasm",
            "prog/samples/gates/sky/shaders/blit.ps.spvasm",
        ] {
            crate::spvasm_layout::assert_prefix_matches(shader, "Atmos", &expected);
        }
        crate::spvasm_layout::assert_glsl_offsets(
            "prog/samples/gates/sky/shaders/sky_hdr.ps.glsl",
            &expected,
        );
    }

    /// Straight up from the ground is exactly the atmosphere thickness; straight
    /// down grazes and comes back out. Pins the sign convention the LUT uses.
    #[test]
    fn distance_to_top_is_sane() {
        let (b, t) = (BOTTOM_RADIUS_KM, TOP_RADIUS_KM);
        let up = distance_to_top(b, 1.0, t);
        assert!((up - (t - b)).abs() < 1e-2, "straight up = {up}");
        let horizon = distance_to_top(b, 0.0, t);
        assert!(horizon > up * 5.0, "the horizon path must be far longer");
        assert!(distance_to_top(t, -1.0, t) > 0.0);
    }

    #[test]
    fn aerial_cb_layout_matches_the_glsl() {
        use std::mem::offset_of;
        assert_eq!(std::mem::size_of::<AerialCb>(), 176);
        assert_eq!(offset_of!(AerialCb, camera_pos), 64);
        assert_eq!(offset_of!(AerialCb, sun_dir), 80);
        assert_eq!(offset_of!(AerialCb, sun_illuminance), 96);
        assert_eq!(offset_of!(AerialCb, radii), 112);
        assert_eq!(offset_of!(AerialCb, rayleigh), 128);
        assert_eq!(offset_of!(AerialCb, mie), 144);
        assert_eq!(offset_of!(AerialCb, steps), 160);
        assert!(std::mem::size_of::<AerialCb>() <= harpia_rhi::FRAME_UBO_SIZE as usize);
        assert_eq!(aerial_dispatch(), (4, 4, 8));
        crate::spvasm_layout::assert_glsl_offsets(
            "prog/samples/gates/terrain/shaders/aerial.cs.glsl",
            &[
                (0, 0),
                (1, 64),
                (2, 80),
                (3, 96),
                (4, 112),
                (5, 128),
                (6, 144),
                (7, 160),
            ],
        );
    }
}
