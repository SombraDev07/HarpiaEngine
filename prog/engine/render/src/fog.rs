//! Volumetric fog froxels (roadmap §9). Compute only; no `vk::*`.
//!
//! Two volumes, both `RGBA16F` at [`FROXEL_W`]×[`FROXEL_H`]×[`FROXEL_D`]:
//! *scatter* holds `(in-scattered rgb, extinction)` per froxel, *integrated*
//! holds `(accumulated in-scattering, transmittance)` marched along Z.
//!
//! Z is distributed exponentially between [`FogCb::near`] and `far`, so slice
//! `s` sits at `near * (far/near)^((s + 0.5 + jitter) / D)`. The apply pass
//! inverts that with a log.

use harpia_math::{Mat4, Vec2, Vec4};
use harpia_rhi::{Format, TextureDesc, TextureDim};

pub const FROXEL_W: u32 = 160;
pub const FROXEL_H: u32 = 90;
pub const FROXEL_D: u32 = 64;
pub const FROXEL_FORMAT: Format = Format::Rgba16Float;

/// Inject is `[8,8,4]`, integrate `[8,8,1]`. 90 is not a multiple of 8, so the
/// shaders bound-check Y — the dispatch covers 96 rows.
pub const INJECT_GROUP: (u32, u32, u32) = (8, 8, 4);
pub const INTEGRATE_GROUP: (u32, u32, u32) = (8, 8, 1);

pub fn inject_dispatch() -> (u32, u32, u32) {
    (
        FROXEL_W.div_ceil(INJECT_GROUP.0),
        FROXEL_H.div_ceil(INJECT_GROUP.1),
        FROXEL_D.div_ceil(INJECT_GROUP.2),
    )
}

pub fn integrate_dispatch() -> (u32, u32, u32) {
    (
        FROXEL_W.div_ceil(INTEGRATE_GROUP.0),
        FROXEL_H.div_ceil(INTEGRATE_GROUP.1),
        1,
    )
}

pub fn froxel_desc() -> TextureDesc {
    TextureDesc {
        width: FROXEL_W,
        height: FROXEL_H,
        depth_slices: FROXEL_D,
        dim: TextureDim::D3,
        format: FROXEL_FORMAT,
        sampled: true,
        storage: true,
        ..Default::default()
    }
}

/// Halton(2) — the jitter that keeps froxel slices from banding frame to frame.
pub fn halton2(index: u32) -> f32 {
    let mut f = 0.5;
    let mut r = 0.0;
    let mut i = index + 1;
    while i > 0 {
        r += f * (i % 2) as f32;
        i /= 2;
        f *= 0.5;
    }
    r - 0.5
}

/// Per-frame fog constants. One CBV chunk; matches `fog.hlsl` / the `.spvasm`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FogCb {
    /// Camera → world. Froxels are built in view space and pushed out by this.
    pub inv_view: Mat4,
    pub camera_pos: Vec4,
    pub sun_dir: Vec4,
    pub sun_color: Vec4,
    /// density, height falloff, phase anisotropy `g`, base height.
    pub fog: Vec4,
    /// near, far, `tan(fovY/2)`, aspect.
    pub froxel: Vec4,
    /// jitter, slice count, exposure, unused.
    pub misc: Vec4,
    pub scene_color: u32,
    pub scene_depth: u32,
    /// 1/width, 1/height of the scene RT — the apply pass turns FragCoord into uv.
    pub inv_extent: Vec2,
}

impl Default for FogCb {
    fn default() -> Self {
        Self {
            inv_view: Mat4::IDENTITY,
            camera_pos: Vec4::W,
            sun_dir: Vec4::Y,
            sun_color: Vec4::ONE,
            fog: Vec4::new(0.04, 0.25, 0.6, 0.0),
            froxel: Vec4::new(0.5, 60.0, 1.0, 1.777),
            misc: Vec4::new(0.0, FROXEL_D as f32, 1.0, 0.0),
            scene_color: 0,
            scene_depth: 0,
            inv_extent: Vec2::ONE,
        }
    }
}

impl FogCb {
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
        assert_eq!(std::mem::size_of::<FogCb>(), 176);
        assert_eq!(std::mem::offset_of!(FogCb, camera_pos), 64);
        assert_eq!(std::mem::offset_of!(FogCb, sun_dir), 80);
        assert_eq!(std::mem::offset_of!(FogCb, sun_color), 96);
        assert_eq!(std::mem::offset_of!(FogCb, fog), 112);
        assert_eq!(std::mem::offset_of!(FogCb, froxel), 128);
        assert_eq!(std::mem::offset_of!(FogCb, misc), 144);
        assert_eq!(std::mem::offset_of!(FogCb, scene_color), 160);
        assert_eq!(std::mem::offset_of!(FogCb, scene_depth), 164);
        assert_eq!(std::mem::offset_of!(FogCb, inv_extent), 168);
        assert!(std::mem::size_of::<FogCb>() <= harpia_rhi::FRAME_UBO_SIZE as usize);
    }

    #[test]
    fn dispatch_covers_every_froxel() {
        let (x, y, z) = inject_dispatch();
        assert_eq!((x, y, z), (20, 12, 16));
        assert!(x * INJECT_GROUP.0 >= FROXEL_W);
        assert!(y * INJECT_GROUP.1 >= FROXEL_H);
        assert_eq!(z * INJECT_GROUP.2, FROXEL_D);
        assert_eq!(integrate_dispatch(), (20, 12, 1));
    }

    #[test]
    fn halton_is_centred() {
        for i in 0..16 {
            let j = halton2(i);
            assert!((-0.5..0.5).contains(&j), "halton2({i}) = {j}");
        }
    }
}
