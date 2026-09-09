//! TAA constants. History + neighbourhood clamp. Motion = camera + object.

use harpia_math::{Mat4, Vec2};

pub const TAA_BLEND: f32 = 0.9;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TaaCb {
    pub prev_view_proj: Mat4,
    pub inv_extent: Vec2,
    pub object_x: f32,
    pub prev_object_x: f32,
    pub color_idx: u32,
    pub history_idx: u32,
    pub motion_idx: u32,
    pub history_valid: u32,
    pub blend: f32,
    pub _pad0: f32,
    pub _pad1: f32,
    pub _pad2: f32,
}

impl Default for TaaCb {
    fn default() -> Self {
        Self {
            prev_view_proj: Mat4::IDENTITY,
            inv_extent: Vec2::ONE,
            object_x: 0.0,
            prev_object_x: 0.0,
            color_idx: 0,
            history_idx: 0,
            motion_idx: 0,
            history_valid: 0,
            blend: TAA_BLEND,
            _pad0: 0.0,
            _pad1: 0.0,
            _pad2: 0.0,
        }
    }
}

impl TaaCb {
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
    fn taa_cb_fits_and_offsets() {
        assert!(std::mem::size_of::<TaaCb>() <= harpia_rhi::FRAME_UBO_SIZE as usize);
        assert_eq!(std::mem::offset_of!(TaaCb, inv_extent), 64);
        assert_eq!(std::mem::offset_of!(TaaCb, object_x), 72);
        assert_eq!(std::mem::offset_of!(TaaCb, color_idx), 80);
        assert_eq!(std::mem::offset_of!(TaaCb, blend), 96);
    }
}
