//! FidelityFX Single Pass Downsampler.
//!
//! GPU headers live in `prog/3rdPartyLibs/ffx-spd`. This crate exports SPIR-V
//! and the CPU `SpdSetup` numbers. It never calls Vulkan.

use std::ffi::CStr;
use std::os::raw::c_char;

use harpia_plugin::{Plugin, PLUGIN_ABI_VERSION};

pub const NAME: &str = "ffx-spd";
/// Six uint counters, one per slice, as the SPD sample uses.
pub const ATOMIC_BYTES: usize = 24;
/// Slot 6 is the coherent mip the header reads between workgroups.
pub const COHERENT_MIP: u32 = 6;
pub const MAX_MIPS: u32 = 12;

/// Dispatch + constants for one `SpdDownsample`.
///
/// Packed as the UBO the shader reads: `mips`, `numWorkGroups`, `ivec2 offset`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dispatch {
    pub groups_x: u32,
    pub groups_y: u32,
    pub mips: u32,
    pub num_work_groups: u32,
    pub work_group_offset_x: i32,
    pub work_group_offset_y: i32,
}

impl Dispatch {
    /// 16-byte UBO body (`std140`: two uints + ivec2).
    pub fn constants_bytes(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[0..4].copy_from_slice(&self.mips.to_le_bytes());
        out[4..8].copy_from_slice(&self.num_work_groups.to_le_bytes());
        out[8..12].copy_from_slice(&self.work_group_offset_x.to_le_bytes());
        out[12..16].copy_from_slice(&self.work_group_offset_y.to_le_bytes());
        out
    }

    /// Levels the destination texture must have: source mip 0 plus generated.
    pub fn texture_mips(&self) -> u32 {
        (self.mips + 1).max(COHERENT_MIP + 1)
    }
}

/// Port of `SpdSetup` in `ffx_spd.h` (`A_CPU`). `mips = None` auto from the rect.
pub fn setup(width: u32, height: u32, mips: Option<u32>) -> Dispatch {
    let width = width.max(1);
    let height = height.max(1);
    let work_group_offset_x = 0i32;
    let work_group_offset_y = 0i32;
    let end_x = width.saturating_sub(1) / 64;
    let end_y = height.saturating_sub(1) / 64;
    let groups_x = end_x + 1;
    let groups_y = end_y + 1;
    let num_work_groups = groups_x * groups_y;
    let mips = match mips {
        Some(m) => m.min(MAX_MIPS),
        None => {
            let res = width.max(height) as f32;
            res.log2().floor().clamp(0.0, MAX_MIPS as f32) as u32
        }
    };
    Dispatch {
        groups_x,
        groups_y,
        mips,
        num_work_groups,
        work_group_offset_x,
        work_group_offset_y,
    }
}

pub fn atomic_zeros() -> [u8; ATOMIC_BYTES] {
    [0u8; ATOMIC_BYTES]
}

pub fn karis_spirv() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/spd_karis.cs.spv"))
}

pub fn min_spirv() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/spd_min.cs.spv"))
}

pub struct SpdPlugin;

impl Plugin for SpdPlugin {
    fn name(&self) -> &'static str {
        NAME
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn harpia_ffx_spd_abi_version() -> u32 {
    PLUGIN_ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn harpia_ffx_spd_name() -> *const c_char {
    CStr::from_bytes_with_nul(b"ffx-spd\0")
        .expect("static name")
        .as_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn harpia_spd_setup(
    width: u32,
    height: u32,
    mips: i32,
    out: *mut Dispatch,
) {
    if out.is_null() {
        return;
    }
    let mips = if mips < 0 {
        None
    } else {
        Some(mips as u32)
    };
    unsafe { *out = setup(width, height, mips) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_matches_amd_rect() {
        let d = setup(1280, 720, None);
        assert_eq!(d.groups_x, 20);
        assert_eq!(d.groups_y, 12);
        assert_eq!(d.num_work_groups, 240);
        assert_eq!(d.mips, 10);
        assert_eq!(d.texture_mips(), 11);
        assert_eq!(d.constants_bytes().len(), 16);
    }

    #[test]
    fn plugin_trait_is_spd() {
        assert_eq!(SpdPlugin.name(), "ffx-spd");
        assert_eq!(SpdPlugin.abi_version(), PLUGIN_ABI_VERSION);
    }
}
