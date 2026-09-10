//! GBuffer packing ABI (roadmap §3). `fuzzColor` reaches lighting via RT3.

use harpia_rhi::{Format, TextureDesc, TextureDim};

/// RT0 albedo sRGB, RT1 normal, RT2 ORM, RT3 emissive/fuzzColor, RT4 depthColor.
pub const GBUFFER_COLOR_FORMATS: [Format; 5] = [
    Format::Rgba8Srgb,
    Format::Rgba8Unorm,
    Format::Rgba8Unorm,
    Format::Rgba8Unorm,
    Format::R32Float,
];

pub const GBUFFER_DEPTH_FORMAT: Format = Format::D32Float;

pub const VERTEX_STRIDE: u32 = 24;
pub const VERTEX_STRIDE_UV: u32 = 32;
pub const INSTANCE_STRIDE: u32 = 80;

/// glTF-style reflectance → dielectric F0. 0.5 ⇒ 0.04.
pub fn dielectric_f0(reflectance: f32) -> f32 {
    0.16 * reflectance * reflectance
}

pub fn color_desc(width: u32, height: u32, format: Format) -> TextureDesc {
    TextureDesc {
        width,
        height,
        depth_slices: 1,
        dim: TextureDim::D2,
        mip_levels: 1,
        format,
        sampled: true,
        storage: false,
        color_attachment: true,
        depth: false,
    }
}

/// Plain sampled texture (glTF albedo). Not a render target: no
/// COLOR_ATTACHMENT usage, and a full mip chain the caller must upload.
pub fn sampled_desc(width: u32, height: u32, mip_levels: u32, format: Format) -> TextureDesc {
    TextureDesc {
        width,
        height,
        depth_slices: 1,
        dim: TextureDim::D2,
        mip_levels: mip_levels.max(1),
        format,
        sampled: true,
        storage: false,
        color_attachment: false,
        depth: false,
    }
}

pub fn depth_desc(width: u32, height: u32) -> TextureDesc {
    TextureDesc {
        width,
        height,
        depth_slices: 1,
        dim: TextureDim::D2,
        mip_levels: 1,
        format: GBUFFER_DEPTH_FORMAT,
        sampled: false,
        storage: false,
        color_attachment: false,
        depth: true,
    }
}

/// Sampled D32 atlas (CSM). GBuffer depth stays [`depth_desc`] (`sampled: false`).
pub fn shadow_atlas_desc(size: u32) -> TextureDesc {
    TextureDesc {
        width: size.max(1),
        height: size.max(1),
        depth_slices: 1,
        dim: TextureDim::D2,
        mip_levels: 1,
        format: GBUFFER_DEPTH_FORMAT,
        sampled: true,
        storage: false,
        color_attachment: false,
        depth: true,
    }
}

/// If `fuzz > 0`, RT3 RGB is `fuzzColor` and lighting must not add emissive.
pub fn rt3_rgb(fuzz: f32, fuzz_color: [f32; 3], emissive: [f32; 3]) -> [f32; 3] {
    if fuzz > 0.001 {
        fuzz_color
    } else {
        emissive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflectance_half_is_four_percent() {
        let f0 = dielectric_f0(0.5);
        assert!((f0 - 0.04).abs() < 1e-5);
    }

    #[test]
    fn fuzz_steals_rt3() {
        assert_eq!(rt3_rgb(0.4, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), [1.0, 0.0, 0.0]);
        assert_eq!(rt3_rgb(0.0, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]);
    }
}
