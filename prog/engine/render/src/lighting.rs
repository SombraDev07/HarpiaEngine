//! Per-frame lighting CBV. Written after `begin_frame`. Push constants hold viewProj+world.
//! Shadow fields are appended: `cascade_count == 0` skips CSM (pbr-grid).

use harpia_math::{Mat4, Vec2, Vec4};

use crate::csm::Csm;

/// Roadmap §6 default. Read as `tan` of the sun's angular radius: the real sun
/// is ~0.0047, this is exaggerated so the penumbra is visible.
pub const PCSS_LIGHT_SIZE: f32 = 0.035;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LightingCb {
    pub inv_view_proj: Mat4,
    pub camera_pos: Vec4,
    pub sun_dir: Vec4,
    pub sun_color: Vec4,
    pub gbuf0: u32,
    pub gbuf1: u32,
    pub gbuf2: u32,
    pub gbuf3: u32,
    pub gbuf4: u32,
    pub irradiance: u32,
    pub prefiltered: u32,
    pub brdf_lut: u32,
    pub ibl_scale: f32,
    pub ibl_max_mip: f32,
    pub exposure: f32,
    /// glTF `MASK` cutoff for the primitive being drawn. 0 = opaque.
    pub alpha_cutoff: f32,
    pub inv_extent: Vec2,
    pub _pad1: Vec2,
    pub view: Mat4,
    pub shadow_idx: u32,
    pub atlas_size: f32,
    pub cascade_count: u32,
    pub _pad2: u32,
    pub splits: Vec4,
    pub cascades: [Mat4; 4],
    /// PCSS: light size (tan of the source's angular radius, exaggerated for
    /// looks), blocker-search radius in texels, min and max PCF radius in texels.
    pub pcss: Vec4,
    /// [`crate::Csm::radius`] per cascade.
    pub cascade_radius: Vec4,
}

impl Default for LightingCb {
    fn default() -> Self {
        Self {
            inv_view_proj: Mat4::IDENTITY,
            camera_pos: Vec4::ZERO,
            sun_dir: Vec4::Y,
            sun_color: Vec4::ONE,
            gbuf0: 0,
            gbuf1: 0,
            gbuf2: 0,
            gbuf3: 0,
            gbuf4: 0,
            irradiance: 0,
            prefiltered: 0,
            brdf_lut: 0,
            ibl_scale: 1.0,
            ibl_max_mip: 0.0,
            exposure: 1.0,
            alpha_cutoff: 0.0,
            inv_extent: Vec2::ONE,
            _pad1: Vec2::ZERO,
            view: Mat4::IDENTITY,
            shadow_idx: 0,
            atlas_size: 1.0,
            cascade_count: 0,
            _pad2: 0,
            splits: Vec4::ZERO,
            cascades: [Mat4::IDENTITY; 4],
            pcss: Vec4::new(PCSS_LIGHT_SIZE, 3.0, 1.0, 4.5),
            cascade_radius: Vec4::ONE,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PushConstants {
    pub view_proj: Mat4,
    pub world: Mat4,
}

impl PushConstants {
    pub fn new(view_proj: Mat4) -> Self {
        Self {
            view_proj,
            world: Mat4::IDENTITY,
        }
    }

    pub fn with_world(view_proj: Mat4, world: Mat4) -> Self {
        Self { view_proj, world }
    }

    pub fn as_bytes(&self) -> &[u8] {
        as_bytes(self)
    }
}

impl LightingCb {
    pub fn as_bytes(&self) -> &[u8] {
        as_bytes(self)
    }

    pub fn apply_csm(&mut self, csm: &Csm, shadow_idx: u32) {
        self.shadow_idx = shadow_idx;
        self.atlas_size = csm.atlas_size as f32;
        self.cascade_count = 4;
        self.splits = csm.splits;
        self.cascades = csm.view_proj;
        self.cascade_radius = csm.radius;
    }
}

fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((v as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_is_128_and_cb_fits_ubo() {
        use std::mem::offset_of;
        assert_eq!(std::mem::size_of::<PushConstants>(), 128);
        assert!(std::mem::size_of::<LightingCb>() <= harpia_rhi::FRAME_UBO_SIZE as usize);
        assert_eq!(std::mem::size_of::<LightingCb>(), 560);
        assert_eq!(std::mem::offset_of!(LightingCb, pcss), 528);
        assert_eq!(std::mem::offset_of!(LightingCb, cascade_radius), 544);
        assert_eq!(std::mem::offset_of!(LightingCb, exposure), 152);
        assert_eq!(std::mem::offset_of!(LightingCb, alpha_cutoff), 156);
        assert_eq!(std::mem::offset_of!(LightingCb, view), 176);
        assert_eq!(std::mem::offset_of!(LightingCb, shadow_idx), 240);
        assert_eq!(std::mem::offset_of!(LightingCb, splits), 256);
        assert_eq!(std::mem::offset_of!(LightingCb, cascades), 272);

        // The widest block in the tree, read by five shaders. A member that
        // drifts here does not fail validation -- the block is still the right
        // size -- it just makes the GPU read the wrong bytes.
        let g = offset_of!(LightingCb, gbuf0) as u32;
        let casc = offset_of!(LightingCb, cascades) as u32;
        let expected = [
            (0, 0),
            (1, 64),
            (2, 80),
            (3, 96),
            (4, g),
            (5, g + 4),
            (6, g + 8),
            (7, g + 12),
            (8, g + 16),
            (9, offset_of!(LightingCb, irradiance) as u32),
            (10, offset_of!(LightingCb, prefiltered) as u32),
            (11, offset_of!(LightingCb, brdf_lut) as u32),
            (12, offset_of!(LightingCb, ibl_scale) as u32),
            (13, offset_of!(LightingCb, ibl_max_mip) as u32),
            (14, offset_of!(LightingCb, exposure) as u32),
            (15, offset_of!(LightingCb, alpha_cutoff) as u32),
            (16, offset_of!(LightingCb, inv_extent) as u32),
            (17, offset_of!(LightingCb, _pad1) as u32),
            (18, offset_of!(LightingCb, view) as u32),
            (19, offset_of!(LightingCb, shadow_idx) as u32),
            (20, offset_of!(LightingCb, atlas_size) as u32),
            (21, offset_of!(LightingCb, cascade_count) as u32),
            (22, offset_of!(LightingCb, _pad2) as u32),
            (23, offset_of!(LightingCb, splits) as u32),
            (24, casc),
            (25, casc + 64),
            (26, casc + 128),
            (27, casc + 192),
            (28, offset_of!(LightingCb, pcss) as u32),
            (29, offset_of!(LightingCb, cascade_radius) as u32),
        ];
        for shader in [
            "prog/samples/gates/csm/shaders/color.ps.spvasm",
            "prog/samples/gates/csm/shaders/blit.ps.spvasm",
            "prog/samples/sponza/shaders/color.ps.spvasm",
            "prog/samples/sponza/shaders/shadow.ps.spvasm",
            "prog/samples/sponza/shaders/blit.ps.spvasm",
        ] {
            crate::spvasm_layout::assert_prefix_matches(shader, "Lighting", &expected);
        }
    }
}
