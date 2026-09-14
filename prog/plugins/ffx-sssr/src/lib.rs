//! FidelityFX Stochastic Screen-Space Reflections (hierarchical march).
//!
//! GPU algorithm: `prog/3rdPartyLibs/ffx-sssr`. This crate exports SPIR-V.
//! It never calls Vulkan. DNSR is not in this slice.

use std::ffi::CStr;
use std::os::raw::c_char;

use harpia_plugin::{Plugin, PLUGIN_ABI_VERSION};

pub const NAME: &str = "ffx-sssr";
pub const MAX_TRAVERSAL_INTERSECTIONS: u32 = 128;
pub const DEPTH_THICKNESS: f32 = 0.015;

pub struct SssrPlugin;

impl Plugin for SssrPlugin {
    fn name(&self) -> &'static str {
        NAME
    }
}

pub fn intersect_spirv() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/sssr_intersect.cs.spv"))
}

pub fn apply_spirv() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/sssr_apply.ps.spv"))
}

#[unsafe(no_mangle)]
pub extern "C" fn harpia_ffx_sssr_abi_version() -> u32 {
    PLUGIN_ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn harpia_ffx_sssr_name() -> *const c_char {
    CStr::from_bytes_with_nul(b"ffx-sssr\0")
        .expect("static name")
        .as_ptr()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_trait_is_sssr() {
        assert_eq!(SssrPlugin.name(), "ffx-sssr");
        assert_eq!(SssrPlugin.abi_version(), PLUGIN_ABI_VERSION);
    }
}
