//! `--capture`: dump render targets to PNG after the last frame.
//!
//! Screenshots of the window race the compositor and the window manager; this
//! reads the texture back from the GPU, so a gate can look at real pixels.

use std::path::Path;

use anyhow::{Context, Result};
use harpia_rhi::{Format, TextureData};

/// Writes `<prefix>.<name>.png` and logs the value range (handy in a terminal).
/// A volume comes out as a grid of its slices, so fog froxels are one glance.
pub fn write_png(prefix: &Path, name: &str, data: &TextureData) -> Result<()> {
    let mut path = prefix.as_os_str().to_os_string();
    path.push(format!(".{name}.png"));
    let path = std::path::PathBuf::from(path);
    let (w, h) = if data.depth_slices > 1 {
        let cols = (data.depth_slices as f32).sqrt().ceil() as u32;
        let rows = data.depth_slices.div_ceil(cols);
        tracing::info!(
            target = name,
            slices = data.depth_slices,
            grid = format!("{cols}x{rows}"),
            "volume capture"
        );
        (data.width * cols, data.height * rows)
    } else {
        (data.width, data.height)
    };
    let data = &tile_slices(data);

    match data.format {
        Format::Rgba8Unorm | Format::Rgba8Srgb => {
            image::save_buffer(&path, &data.bytes, w, h, image::ColorType::Rgba8)?;
        }
        Format::Bgra8Unorm | Format::Bgra8Srgb => {
            let mut rgba = data.bytes.clone();
            for px in rgba.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
            image::save_buffer(&path, &rgba, w, h, image::ColorType::Rgba8)?;
        }
        Format::D32Float | Format::R32Float => {
            let floats: Vec<f32> = data
                .bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
            for v in &floats {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
            // Depth lives in [0,1]; stretch what is actually there so a shadow
            // map is readable instead of a flat white field.
            let span = (hi - lo).max(1e-6);
            let grey: Vec<u8> = floats
                .iter()
                .map(|v| (((v - lo) / span).clamp(0.0, 1.0) * 255.0) as u8)
                .collect();
            tracing::info!(target = name, min = lo, max = hi, "captured depth");
            image::save_buffer(&path, &grey, w, h, image::ColorType::L8)?;
        }
        Format::Rgba16Float => {
            let px: Vec<f32> = data.bytes.chunks_exact(2).map(half_to_f32).collect();
            let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
            for v in &px {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
            tracing::info!(target = name, min = lo, max = hi, "captured HDR");
            // Debug view: clamp + gamma. Not a tonemapper, just readable.
            let rgba: Vec<u8> = px
                .iter()
                .map(|v| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8)
                .collect();
            image::save_buffer(&path, &rgba, w, h, image::ColorType::Rgba8)?;
        }
        Format::Rg16Float => {
            let px: Vec<f32> = data.bytes.chunks_exact(2).map(half_to_f32).collect();
            let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
            for v in &px {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
            tracing::info!(target = name, min = lo, max = hi, "captured motion");
            // Motion vectors: x → red, y → green, around a grey zero.
            let mut rgb = Vec::with_capacity(px.len() / 2 * 3);
            for v in px.chunks_exact(2) {
                let enc = |x: f32| ((x * 8.0 + 0.5).clamp(0.0, 1.0) * 255.0) as u8;
                rgb.extend_from_slice(&[enc(v[0]), enc(v[1]), 128]);
            }
            image::save_buffer(&path, &rgb, w, h, image::ColorType::Rgb8)?;
        }
        other => anyhow::bail!("capture does not handle {other:?}"),
    }
    tracing::info!(path = %path.display(), width = w, height = h, "captured");
    Ok(())
}

pub fn capture_all(
    gpu: &mut harpia_rhi::Gpu,
    prefix: &Path,
    targets: &[(&'static str, harpia_rhi::Texture)],
) -> Result<()> {
    use harpia_rhi::Device;
    for (name, tex) in targets {
        let data = gpu
            .read_texture(*tex)
            .with_context(|| format!("read back `{name}`"))?;
        write_png(prefix, name, &data)?;
    }
    Ok(())
}

/// IEEE half → f32. Avoids pulling `half` in just for a debug view.
fn half_to_f32(bytes: &[u8]) -> f32 {
    let bits = u16::from_le_bytes([bytes[0], bytes[1]]);
    let sign = (bits >> 15) as u32;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let mant = (bits & 0x3ff) as u32;
    let out = match exp {
        0 if mant == 0 => sign << 31,
        0 => {
            let mut e: i32 = -1;
            let mut m = mant;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            (sign << 31) | (((127 - 15 + 1 + e) as u32) << 23) | ((m & 0x3ff) << 13)
        }
        0x1f => (sign << 31) | 0x7f80_0000 | (mant << 13),
        _ => (sign << 31) | ((exp + 127 - 15) << 23) | (mant << 13),
    };
    f32::from_bits(out)
}

#[cfg(test)]
mod tests {
    use super::half_to_f32;

    #[test]
    fn half_round_trip() {
        let cases = [(0x0000u16, 0.0f32), (0x3c00, 1.0), (0xbc00, -1.0), (0x4000, 2.0), (0x3800, 0.5)];
        for (bits, want) in cases {
            let got = half_to_f32(&bits.to_le_bytes());
            assert!((got - want).abs() < 1e-6, "{bits:#06x} -> {got}, want {want}");
        }
    }
}

/// Lay a volume's slices out in a grid so one PNG shows the whole thing.
fn tile_slices(data: &TextureData) -> TextureData {
    if data.depth_slices <= 1 {
        return data.clone();
    }
    let bpp = data.bytes.len() / (data.width * data.height * data.depth_slices) as usize;
    let cols = (data.depth_slices as f32).sqrt().ceil() as u32;
    let rows = data.depth_slices.div_ceil(cols);
    let (tw, th) = (data.width * cols, data.height * rows);
    let mut out = vec![0u8; tw as usize * th as usize * bpp];
    let src_row = data.width as usize * bpp;
    for z in 0..data.depth_slices {
        let (cx, cy) = (z % cols, z / cols);
        for y in 0..data.height {
            let s = ((z * data.height + y) as usize) * src_row;
            let dx = (cx * data.width) as usize * bpp;
            let dy = (cy * data.height + y) as usize * tw as usize * bpp;
            out[dy + dx..dy + dx + src_row].copy_from_slice(&data.bytes[s..s + src_row]);
        }
    }
    TextureData {
        width: tw,
        height: th,
        depth_slices: 1,
        format: data.format,
        bytes: out,
    }
}
