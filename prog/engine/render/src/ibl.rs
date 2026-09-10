//! CPU IBL: procedural sky, irradiance, GGX prefilter mips, BRDF LUT. All mips filled.

use harpia_math::Vec3;
use harpia_rhi::{Format, TextureDesc, TextureDim};

const PI: f32 = std::f32::consts::PI;

#[derive(Clone, Debug)]
pub struct IblCpu {
    pub irradiance: RgbaImage,
    pub prefiltered: RgbaImage,
    pub brdf_lut: RgbaImage,
    pub ibl_scale: f32,
}

#[derive(Clone, Debug)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
    /// Packed mip0, mip1, … tightly (no row pitch).
    pub mips: Vec<Vec<u8>>,
}

impl RgbaImage {
    pub fn desc(&self) -> TextureDesc {
        TextureDesc {
            width: self.width,
            height: self.height,
            depth_slices: 1,
            dim: TextureDim::D2,
            mip_levels: self.mip_levels,
            format: Format::Rgba8Unorm,
            sampled: true,
            storage: false,
            color_attachment: false,
            depth: false,
        }
    }
}

pub fn generate() -> IblCpu {
    let env_w = 128u32;
    let env_h = 64u32;
    let env = generate_sky(env_w, env_h);
    let irradiance = convolve_irradiance(&env, 32, 16);
    let prefiltered = prefilter_ggx(&env, 8);
    let brdf_lut = integrate_brdf(128);
    IblCpu {
        irradiance,
        prefiltered,
        brdf_lut,
        ibl_scale: 1.0,
    }
}

fn generate_sky(w: u32, h: u32) -> RgbaImage {
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let d = uv_to_dir((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            let c = sky(d);
            write_px(&mut px, w, x, y, c);
        }
    }
    RgbaImage {
        width: w,
        height: h,
        mip_levels: 1,
        mips: vec![px],
    }
}

fn sky(dir: Vec3) -> Vec3 {
    let n = dir.normalize_or_zero();
    let t = (n.y * 0.5 + 0.5).clamp(0.0, 1.0);
    let zenith = Vec3::new(0.22, 0.38, 0.72);
    let horizon = Vec3::new(0.72, 0.68, 0.58);
    let ground = Vec3::new(0.12, 0.11, 0.10);
    if n.y >= 0.0 {
        zenith.lerp(horizon, 1.0 - t)
    } else {
        horizon.lerp(ground, (-n.y).clamp(0.0, 1.0))
    }
}

fn convolve_irradiance(env: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let mut px = vec![0u8; (w * h * 4) as usize];
            let samples = 16u32;
    for y in 0..h {
        for x in 0..w {
            let n = uv_to_dir((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            let up = if n.y.abs() > 0.999 {
                Vec3::X
            } else {
                Vec3::Y
            };
            let tangent = up.cross(n).normalize_or_zero();
            let bitan = n.cross(tangent);
            let mut acc = Vec3::ZERO;
            let mut weight = 0.0f32;
            for i in 0..samples {
                for j in 0..samples {
                    let xi = hammersley(i * samples + j, samples * samples);
                    let phi = 2.0 * PI * xi.0;
                    let cos_t = (1.0 - xi.1).sqrt();
                    let sin_t = xi.1.sqrt();
                    let l = tangent * (phi.cos() * sin_t) + bitan * (phi.sin() * sin_t) + n * cos_t;
                    let c = sample_latlong(env, l.normalize_or_zero());
                    acc += c * cos_t;
                    weight += cos_t;
                }
            }
            write_px(&mut px, w, x, y, acc / weight.max(1e-4));
        }
    }
    RgbaImage {
        width: w,
        height: h,
        mip_levels: 1,
        mips: vec![px],
    }
}

fn prefilter_ggx(env: &RgbaImage, mip_levels: u32) -> RgbaImage {
    let mip_levels = mip_levels.max(1);
    let mut mips = Vec::with_capacity(mip_levels as usize);
    let mut w = env.width;
    let mut h = env.height;
    for mip in 0..mip_levels {
        let roughness = if mip_levels == 1 {
            0.0
        } else {
            mip as f32 / (mip_levels - 1) as f32
        };
        let a = (roughness * roughness).max(0.001);
        let mut px = vec![0u8; (w * h * 4) as usize];
        let samples = if mip == 0 { 8 } else { 24 };
        for y in 0..h {
            for x in 0..w {
                let n = uv_to_dir((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                let v = n;
                let mut acc = Vec3::ZERO;
                let mut weight = 0.0f32;
                for i in 0..samples {
                    let xi = hammersley(i, samples);
                    let h = importance_ggx(xi, n, a);
                    let l = (h * 2.0 * v.dot(h) - v).normalize_or_zero();
                    let ndl = n.dot(l).max(0.0);
                    if ndl > 0.0 {
                        acc += sample_latlong(env, l) * ndl;
                        weight += ndl;
                    }
                }
                write_px(&mut px, w, x, y, acc / weight.max(1e-4));
            }
        }
        mips.push(px);
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    RgbaImage {
        width: env.width,
        height: env.height,
        mip_levels,
        mips,
    }
}

fn integrate_brdf(size: u32) -> RgbaImage {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let samples = 64u32;
    for y in 0..size {
        for x in 0..size {
            let ndv = ((x as f32 + 0.5) / size as f32).max(0.001);
            let roughness = (y as f32 + 0.5) / size as f32;
            let a = (roughness * roughness).max(0.001);
            let v = Vec3::new((1.0 - ndv * ndv).sqrt(), 0.0, ndv);
            let n = Vec3::Z;
            let mut a_acc = 0.0f32;
            let mut b_acc = 0.0f32;
            for i in 0..samples {
                let xi = hammersley(i, samples);
                let h = importance_ggx(xi, n, a);
                let l = (h * 2.0 * v.dot(h) - v).normalize_or_zero();
                let ndl = l.z.max(0.0);
                let ndh = h.z.max(0.0);
                let voh = v.dot(h).max(0.0);
                if ndl > 0.0 {
                    let g = g_smith(ndv, ndl, a);
                    let g_vis = (g * voh) / (ndh * ndv).max(1e-4);
                    let fc = (1.0 - voh).powf(5.0);
                    a_acc += (1.0 - fc) * g_vis;
                    b_acc += fc * g_vis;
                }
            }
            let inv = 1.0 / samples as f32;
            let scale = (a_acc * inv).clamp(0.0, 1.0);
            let bias = (b_acc * inv).clamp(0.0, 1.0);
            let i = ((y * size + x) * 4) as usize;
            px[i] = (scale * 255.0) as u8;
            px[i + 1] = (bias * 255.0) as u8;
            px[i + 2] = 0;
            px[i + 3] = 255;
        }
    }
    RgbaImage {
        width: size,
        height: size,
        mip_levels: 1,
        mips: vec![px],
    }
}

fn g_smith(ndv: f32, ndl: f32, a: f32) -> f32 {
    let k = (a + 1.0).powi(2) / 8.0;
    let g1 = |x: f32| x / (x * (1.0 - k) + k).max(1e-4);
    g1(ndv) * g1(ndl)
}

fn importance_ggx(xi: (f32, f32), n: Vec3, a: f32) -> Vec3 {
    let a2 = a * a;
    let phi = 2.0 * PI * xi.0;
    let cos_t = ((1.0 - xi.1) / (1.0 + (a2 - 1.0) * xi.1)).sqrt();
    let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
    let h = Vec3::new(phi.cos() * sin_t, phi.sin() * sin_t, cos_t);
    let up = if n.z.abs() < 0.999 { Vec3::Z } else { Vec3::X };
    let tangent = up.cross(n).normalize_or_zero();
    let bitan = n.cross(tangent);
    (tangent * h.x + bitan * h.y + n * h.z).normalize_or_zero()
}

fn hammersley(i: u32, n: u32) -> (f32, f32) {
    (i as f32 / n as f32, radical_inverse(i))
}

fn radical_inverse(bits: u32) -> f32 {
    bits.reverse_bits() as f32 * 2.3283064e-10
}

fn uv_to_dir(u: f32, v: f32) -> Vec3 {
    let phi = (u * 2.0 - 1.0) * PI;
    let theta = v * PI;
    Vec3::new(theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin())
}

fn dir_to_uv(d: Vec3) -> (f32, f32) {
    let n = d.normalize_or_zero();
    let u = n.z.atan2(n.x) / (2.0 * PI) + 0.5;
    let v = n.y.clamp(-1.0, 1.0).acos() / PI;
    (u.fract().rem_euclid(1.0), v.clamp(0.0, 1.0))
}

fn sample_latlong(img: &RgbaImage, dir: Vec3) -> Vec3 {
    let (u, v) = dir_to_uv(dir);
    let x = (u * (img.width - 1) as f32).round() as u32;
    let y = (v * (img.height - 1) as f32).round() as u32;
    let i = ((y * img.width + x) * 4) as usize;
    let p = &img.mips[0];
    Vec3::new(p[i] as f32, p[i + 1] as f32, p[i + 2] as f32) / 255.0
}

fn write_px(px: &mut [u8], w: u32, x: u32, y: u32, c: Vec3) {
    let i = ((y * w + x) * 4) as usize;
    px[i] = (c.x.clamp(0.0, 1.0) * 255.0) as u8;
    px[i + 1] = (c.y.clamp(0.0, 1.0) * 255.0) as u8;
    px[i + 2] = (c.z.clamp(0.0, 1.0) * 255.0) as u8;
    px[i + 3] = 255;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ibl_mips_are_complete() {
        let ibl = generate();
        assert_eq!(ibl.prefiltered.mips.len(), ibl.prefiltered.mip_levels as usize);
        assert_eq!(ibl.irradiance.mips.len(), 1);
        assert_eq!(ibl.brdf_lut.mips.len(), 1);
        let last = ibl.prefiltered.mips.last().unwrap();
        assert_eq!(last.len(), 4);
    }
}
