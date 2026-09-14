//! DX10 DDS mip 0 → RGBA8.
//!
//! The rain maps from Tucano are BC1 (rainfall / spatter) and BC5 (ddn / ripples).
//! RHI uploads are uncompressed, so this is the cook: decode once at load, upload
//! mip 0, never sample an UNDEFINED mip.

use std::fmt;

#[derive(Clone, Debug)]
pub struct DdsImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct DdsError(pub String);

impl fmt::Display for DdsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DdsError {}

const DXGI_BC1_UNORM: u32 = 71;
const DXGI_BC1_UNORM_SRGB: u32 = 72;
const DXGI_BC4_UNORM: u32 = 80;
const DXGI_BC5_UNORM: u32 = 83;
const DXGI_BC5_SNORM: u32 = 84;
const DXGI_R8G8B8A8_UNORM: u32 = 28;
const DXGI_B8G8R8A8_UNORM: u32 = 87;
const FOURCC_DX10: u32 = 0x3031_5844; // 'DX10'

pub fn decode(bytes: &[u8]) -> Result<DdsImage, DdsError> {
    if bytes.len() < 148 || bytes.get(0..4) != Some(b"DDS ") {
        return Err(DdsError("not a DDS".into()));
    }
    let height = u32_at(bytes, 12)?;
    let width = u32_at(bytes, 16)?;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err(DdsError(format!("bad DDS size {width}x{height}")));
    }
    let pf_flags = u32_at(bytes, 80)?;
    let fourcc = u32_at(bytes, 84)?;
    if pf_flags & 0x4 == 0 || fourcc != FOURCC_DX10 {
        return Err(DdsError("need DX10 DDS".into()));
    }
    let dxgi = u32_at(bytes, 128)?;
    let data = bytes.get(148..).ok_or_else(|| DdsError("DDS truncated".into()))?;
    match dxgi {
        DXGI_BC1_UNORM | DXGI_BC1_UNORM_SRGB => decode_bc1(width, height, data),
        DXGI_BC4_UNORM => decode_bc4(width, height, data),
        DXGI_BC5_UNORM | DXGI_BC5_SNORM => decode_bc5(width, height, data),
        DXGI_R8G8B8A8_UNORM => decode_rgba(width, height, data, false),
        DXGI_B8G8R8A8_UNORM => decode_rgba(width, height, data, true),
        other => Err(DdsError(format!("unsupported DXGI format {other}"))),
    }
}

fn u32_at(bytes: &[u8], off: usize) -> Result<u32, DdsError> {
    let s = bytes
        .get(off..off + 4)
        .ok_or_else(|| DdsError("DDS header truncated".into()))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn decode_rgba(w: u32, h: u32, data: &[u8], bgra: bool) -> Result<DdsImage, DdsError> {
    let n = (w as usize) * (h as usize) * 4;
    if data.len() < n {
        return Err(DdsError("RGBA DDS truncated".into()));
    }
    let mut rgba = vec![0u8; n];
    if bgra {
        for i in 0..w as usize * h as usize {
            rgba[i * 4] = data[i * 4 + 2];
            rgba[i * 4 + 1] = data[i * 4 + 1];
            rgba[i * 4 + 2] = data[i * 4];
            rgba[i * 4 + 3] = data[i * 4 + 3];
        }
    } else {
        rgba.copy_from_slice(&data[..n]);
    }
    Ok(DdsImage {
        width: w,
        height: h,
        rgba,
    })
}

fn rgb565(c: u16) -> [u8; 3] {
    [
        ((((c >> 11) & 31) as u32 * 255 + 15) / 31) as u8,
        ((((c >> 5) & 63) as u32 * 255 + 31) / 63) as u8,
        (((c & 31) as u32 * 255 + 15) / 31) as u8,
    ]
}

fn lerp_u8(a: u8, b: u8, num: u32, den: u32) -> u8 {
    ((u32::from(a) * (den - num) + u32::from(b) * num) / den) as u8
}

fn decode_bc1(w: u32, h: u32, data: &[u8]) -> Result<DdsImage, DdsError> {
    let bw = (w + 3) / 4;
    let bh = (h + 3) / 4;
    let need = bw as usize * bh as usize * 8;
    if data.len() < need {
        return Err(DdsError("BC1 DDS truncated".into()));
    }
    let mut rgba = vec![255u8; w as usize * h as usize * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let blk = &data[(by * bw + bx) as usize * 8..][..8];
            let c0 = u16::from_le_bytes([blk[0], blk[1]]);
            let c1 = u16::from_le_bytes([blk[2], blk[3]]);
            let bits = u32::from_le_bytes([blk[4], blk[5], blk[6], blk[7]]);
            let a = rgb565(c0);
            let b = rgb565(c1);
            let mut pal = [[0u8; 4]; 4];
            pal[0] = [a[0], a[1], a[2], 255];
            pal[1] = [b[0], b[1], b[2], 255];
            if c0 > c1 {
                pal[2] = [
                    lerp_u8(a[0], b[0], 1, 3),
                    lerp_u8(a[1], b[1], 1, 3),
                    lerp_u8(a[2], b[2], 1, 3),
                    255,
                ];
                pal[3] = [
                    lerp_u8(a[0], b[0], 2, 3),
                    lerp_u8(a[1], b[1], 2, 3),
                    lerp_u8(a[2], b[2], 2, 3),
                    255,
                ];
            } else {
                pal[2] = [
                    lerp_u8(a[0], b[0], 1, 2),
                    lerp_u8(a[1], b[1], 1, 2),
                    lerp_u8(a[2], b[2], 1, 2),
                    255,
                ];
                pal[3] = [0, 0, 0, 0];
            }
            for i in 0..16u32 {
                let idx = ((bits >> (2 * i)) & 3) as usize;
                let px = i & 3;
                let py = i >> 2;
                let x = bx * 4 + px;
                let y = by * 4 + py;
                if x >= w || y >= h {
                    continue;
                }
                let o = ((y * w + x) * 4) as usize;
                rgba[o..o + 4].copy_from_slice(&pal[idx]);
            }
        }
    }
    Ok(DdsImage {
        width: w,
        height: h,
        rgba,
    })
}

/// Unsigned BC4 palette. Tucano's loader treats BC5_SNORM the same way, and
/// `unpackXYNormal` in the rain shader expects 0..1 in RG (`n.xy * 2 - 1`).
fn bc4_palette(r0: u8, r1: u8) -> [u8; 8] {
    let mut pal = [0u8; 8];
    pal[0] = r0;
    pal[1] = r1;
    if r0 > r1 {
        for i in 2..8 {
            pal[i] = (((8 - i) as u32 * u32::from(r0) + (i - 1) as u32 * u32::from(r1)) / 7) as u8;
        }
    } else {
        for i in 2..6 {
            pal[i] = (((6 - i) as u32 * u32::from(r0) + (i - 1) as u32 * u32::from(r1)) / 5) as u8;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    pal
}

fn decode_bc4_block(block: &[u8]) -> [[u8; 4]; 4] {
    let pal = bc4_palette(block[0], block[1]);
    let mut bits = 0u64;
    for i in 0..6 {
        bits |= u64::from(block[2 + i]) << (8 * i);
    }
    let mut out = [[0u8; 4]; 4];
    for i in 0..16 {
        let idx = ((bits >> (3 * i)) & 7) as usize;
        out[i >> 2][i & 3] = pal[idx];
    }
    out
}

fn decode_bc4(w: u32, h: u32, data: &[u8]) -> Result<DdsImage, DdsError> {
    let bw = (w + 3) / 4;
    let bh = (h + 3) / 4;
    if data.len() < bw as usize * bh as usize * 8 {
        return Err(DdsError("BC4 DDS truncated".into()));
    }
    let mut rgba = vec![255u8; w as usize * h as usize * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let blk = &data[(by * bw + bx) as usize * 8..][..8];
            let r = decode_bc4_block(blk);
            for py in 0..4u32 {
                for px in 0..4u32 {
                    let x = bx * 4 + px;
                    let y = by * 4 + py;
                    if x >= w || y >= h {
                        continue;
                    }
                    let o = ((y * w + x) * 4) as usize;
                    let v = r[py as usize][px as usize];
                    rgba[o] = v;
                    rgba[o + 1] = v;
                    rgba[o + 2] = v;
                    rgba[o + 3] = 255;
                }
            }
        }
    }
    Ok(DdsImage {
        width: w,
        height: h,
        rgba,
    })
}

fn decode_bc5(w: u32, h: u32, data: &[u8]) -> Result<DdsImage, DdsError> {
    let bw = (w + 3) / 4;
    let bh = (h + 3) / 4;
    if data.len() < bw as usize * bh as usize * 16 {
        return Err(DdsError("BC5 DDS truncated".into()));
    }
    let mut rgba = vec![255u8; w as usize * h as usize * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let blk = &data[(by * bw + bx) as usize * 16..][..16];
            let r = decode_bc4_block(&blk[..8]);
            let g = decode_bc4_block(&blk[8..]);
            for py in 0..4u32 {
                for px in 0..4u32 {
                    let x = bx * 4 + px;
                    let y = by * 4 + py;
                    if x >= w || y >= h {
                        continue;
                    }
                    let o = ((y * w + x) * 4) as usize;
                    let rv = r[py as usize][px as usize];
                    let gv = g[py as usize][px as usize];
                    let nx = f32::from(rv) / 255.0 * 2.0 - 1.0;
                    let ny = f32::from(gv) / 255.0 * 2.0 - 1.0;
                    let nz = (1.0 - nx * nx - ny * ny).max(0.0).sqrt();
                    rgba[o] = rv;
                    rgba[o + 1] = gv;
                    rgba[o + 2] = (nz * 0.5 + 0.5).clamp(0.0, 1.0).mul_add(255.0, 0.0) as u8;
                    rgba[o + 3] = 255;
                }
            }
        }
    }
    Ok(DdsImage {
        width: w,
        height: h,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn rain_file(name: &str) -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for _ in 0..3 {
            p.pop();
        }
        p.join("assets/rain").join(name)
    }

    #[test]
    fn rainfall_is_bc1_512() {
        let bytes = std::fs::read(rain_file("rainfall.dds")).expect("assets/rain/rainfall.dds");
        let img = decode(&bytes).expect("decode rainfall");
        assert_eq!((img.width, img.height), (512, 512));
        assert_eq!(img.rgba.len(), 512 * 512 * 4);
        let sum: u32 = img.rgba.iter().step_by(4).map(|&c| u32::from(c)).sum();
        let mean = sum as f32 / (512.0 * 512.0);
        assert!(mean > 1.0 && mean < 250.0, "rainfall mean {mean}");
    }

    #[test]
    fn ripple_is_bc5_256() {
        let bytes = std::fs::read(rain_file("Ripple/ripple1_ddn.dds")).expect("ripple1");
        let img = decode(&bytes).expect("decode ripple");
        assert_eq!((img.width, img.height), (256, 256));
        let min_r = *img.rgba.iter().step_by(4).min().unwrap();
        let max_r = *img.rgba.iter().step_by(4).max().unwrap();
        assert!(max_r > min_r, "ripple RG is flat");
    }
}
