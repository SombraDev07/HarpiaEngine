//! Cloud noise bake (roadmap §9): Perlin-Worley 128³ + Worley 32³, on the CPU,
//! cached to `.raw`.
//!
//! Layout follows Schneider/Nubis, which is what the raymarch expects:
//!
//! * base 128³ RGBA8 — R is Perlin-Worley (the cloud shape), G/B/A are Worley
//!   FBM at rising frequency (the erosion the shape is remapped against).
//! * detail 32³ RGBA8 — R/G/B are Worley FBM, A unused. This is the
//!   high-frequency wisp added at the cloud edges.
//!
//! Everything is **tileable**: cell and lattice indices wrap, so the volume can
//! repeat across the sky without a seam. That is not decoration — a seam in a
//! 128³ volume is a visible line across the whole sky.
//!
//! The bake is slow enough to matter (2.1M voxels × several octaves), so it is
//! written once to disk and read back after that. Threads come from `std`; the
//! rule in roadmap §14.1 is that `rayon` waits for phase 6.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use harpia_math::{Mat4, Vec2, Vec4};
use harpia_rhi::{Format, TextureDesc, TextureDim};

pub const BASE_SIZE: u32 = 128;
pub const DETAIL_SIZE: u32 = 32;
/// Bump when the generator changes, or a stale cache will be trusted forever.
pub const CACHE_VERSION: &str = "v1";

#[derive(Debug)]
pub struct NoiseError(pub String);

impl std::fmt::Display for NoiseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NoiseError {}

impl From<std::io::Error> for NoiseError {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

/// The GPU side of a baked volume: 3D, `RGBA8_UNORM`, sampled through set 5.
pub fn volume_desc(size: u32) -> TextureDesc {
    TextureDesc {
        width: size,
        height: size,
        depth_slices: size,
        dim: TextureDim::D3,
        format: Format::Rgba8Unorm,
        sampled: true,
        storage: false,
        ..Default::default()
    }
}

/// Constants for the slice viewer that proves bake → upload → sample. The
/// raymarch will grow its own, larger, CB.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CloudSliceCb {
    pub inv_extent: Vec2,
    /// Which Z slice of the volume to show, in `[0,1]`.
    pub slice: f32,
    /// 0 = shape (R), 1 = low Worley (G), 2 = mid (B), 3 = high (A).
    pub channel: f32,
}

impl CloudSliceCb {
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                std::mem::size_of::<Self>(),
            )
        }
    }
}

/// Per-frame cloud constants. Matches `clouds.ps.spvasm`.
///
/// The layer is a shell between `layer.x` and `layer.y` km above the ground,
/// centred on the planet, so it curves away and meets the horizon instead of
/// ending at a flat plane.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CloudCb {
    pub inv_view_proj: Mat4,
    /// World camera; `w` is the altitude in km.
    pub camera_pos: Vec4,
    pub sun_dir: Vec4,
    pub sun_color: Vec4,
    /// bottom km, top km, coverage `[0,1]`, density scale.
    pub layer: Vec4,
    /// base noise scale, detail noise scale, detail strength, phase `g`.
    pub shape: Vec4,
    /// wind offset xyz (km), unused.
    pub wind: Vec4,
    /// march steps, light steps, light march length km, extinction km⁻¹.
    pub steps: Vec4,
    /// ambient rgb, planet bottom radius km.
    pub ambient: Vec4,
    pub inv_extent: Vec2,
    /// Bindless index of the half-res cloud target, for the composite pass.
    pub cloud_rt: u32,
    /// Bindless index of last frame's resolved clouds, for the reprojection.
    pub history_rt: u32,
    /// Last frame's view-projection. The reprojection needs it to find where
    /// this pixel's cloud sat on screen a frame ago.
    pub prev_view_proj: Mat4,
}

impl Default for CloudCb {
    fn default() -> Self {
        Self {
            inv_view_proj: Mat4::IDENTITY,
            camera_pos: Vec4::new(0.0, 0.0, 0.0, 0.5),
            sun_dir: Vec4::Y,
            sun_color: Vec4::new(3.4, 3.25, 3.0, 1.0),
            layer: Vec4::new(1.5, 4.0, 0.36, 0.8),
            shape: Vec4::new(0.11, 1.1, 0.32, 0.72),
            wind: Vec4::ZERO,
            steps: Vec4::new(64.0, 6.0, 1.6, 6.0),
            ambient: Vec4::new(0.16, 0.21, 0.32, 6360.0),
            inv_extent: Vec2::ONE,
            cloud_rt: 0,
            history_rt: 0,
            prev_view_proj: Mat4::IDENTITY,
        }
    }
}

impl CloudCb {
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                std::mem::size_of::<Self>(),
            )
        }
    }
}

/// One baked volume, RGBA8, `size³`.
pub struct NoiseVolume {
    pub size: u32,
    pub rgba: Vec<u8>,
}

fn hash3(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    let mut h = seed
        .wrapping_add((x as u32).wrapping_mul(0x8DA6_B343))
        .wrapping_add((y as u32).wrapping_mul(0xD8A6_3C79))
        .wrapping_add((z as u32).wrapping_mul(0xCB1A_B31F));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x2975_95AD);
    h ^= h >> 15;
    h
}

fn unit(h: u32) -> f32 {
    (h & 0x00FF_FFFF) as f32 / 16_777_215.0
}

/// Feature point of the cell, in cell-local space.
fn cell_point(cx: i32, cy: i32, cz: i32, cells: i32, seed: u32) -> [f32; 3] {
    let wx = cx.rem_euclid(cells);
    let wy = cy.rem_euclid(cells);
    let wz = cz.rem_euclid(cells);
    let h = hash3(wx, wy, wz, seed);
    [
        unit(h),
        unit(h.wrapping_mul(0x68E3_1DA4)),
        unit(h.wrapping_mul(0xB519_6C2B)),
    ]
}

/// Tileable Worley. Returns 1 - distance so clouds are bright where cells meet.
pub fn worley(p: [f32; 3], cells: i32, seed: u32) -> f32 {
    let fc = cells as f32;
    let g = [p[0] * fc, p[1] * fc, p[2] * fc];
    let base = [
        g[0].floor() as i32,
        g[1].floor() as i32,
        g[2].floor() as i32,
    ];
    let mut best = f32::MAX;
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let c = [base[0] + dx, base[1] + dy, base[2] + dz];
                let fp = cell_point(c[0], c[1], c[2], cells, seed);
                let q = [
                    c[0] as f32 + fp[0] - g[0],
                    c[1] as f32 + fp[1] - g[1],
                    c[2] as f32 + fp[2] - g[2],
                ];
                let d2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2];
                if d2 < best {
                    best = d2;
                }
            }
        }
    }
    (1.0 - best.sqrt()).clamp(0.0, 1.0)
}

fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn grad(h: u32, x: f32, y: f32, z: f32) -> f32 {
    // 12 edge gradients, the classic Perlin set.
    match h % 12 {
        0 => x + y,
        1 => -x + y,
        2 => x - y,
        3 => -x - y,
        4 => x + z,
        5 => -x + z,
        6 => x - z,
        7 => -x - z,
        8 => y + z,
        9 => -y + z,
        10 => y - z,
        _ => -y - z,
    }
}

/// Tileable Perlin in `[0,1]`. `cells` is the lattice period.
pub fn perlin(p: [f32; 3], cells: i32, seed: u32) -> f32 {
    let fc = cells as f32;
    let g = [p[0] * fc, p[1] * fc, p[2] * fc];
    let i = [
        g[0].floor() as i32,
        g[1].floor() as i32,
        g[2].floor() as i32,
    ];
    let f = [g[0] - i[0] as f32, g[1] - i[1] as f32, g[2] - i[2] as f32];
    let u = [fade(f[0]), fade(f[1]), fade(f[2])];
    let mut acc = 0.0;
    for dz in 0..2 {
        for dy in 0..2 {
            for dx in 0..2 {
                let h = hash3(
                    (i[0] + dx).rem_euclid(cells),
                    (i[1] + dy).rem_euclid(cells),
                    (i[2] + dz).rem_euclid(cells),
                    seed,
                );
                let d = grad(
                    h,
                    f[0] - dx as f32,
                    f[1] - dy as f32,
                    f[2] - dz as f32,
                );
                let wx = if dx == 0 { 1.0 - u[0] } else { u[0] };
                let wy = if dy == 0 { 1.0 - u[1] } else { u[1] };
                let wz = if dz == 0 { 1.0 - u[2] } else { u[2] };
                acc += d * wx * wy * wz;
            }
        }
    }
    (acc * 0.5 + 0.5).clamp(0.0, 1.0)
}

/// Three Worley octaves, weighted 0.625 / 0.25 / 0.125.
pub fn worley_fbm(p: [f32; 3], cells: i32, seed: u32) -> f32 {
    worley(p, cells, seed) * 0.625
        + worley(p, cells * 2, seed.wrapping_add(1)) * 0.25
        + worley(p, cells * 4, seed.wrapping_add(2)) * 0.125
}

fn remap(x: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    let t = (x - a) / (b - a).max(1e-5);
    (c + t * (d - c)).clamp(0.0, 1.0)
}

fn bake(size: u32, detail: bool) -> Vec<u8> {
    let n = size as usize;
    let mut out = vec![0u8; n * n * n * 4];
    let slice_bytes = n * n * 4;
    let threads = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4)
        .min(n);
    let chunk = n.div_ceil(threads);
    std::thread::scope(|scope| {
        for (t, part) in out.chunks_mut(slice_bytes * chunk).enumerate() {
            let z0 = t * chunk;
            scope.spawn(move || {
                for (zi, plane) in part.chunks_mut(slice_bytes).enumerate() {
                    let z = (z0 + zi) as f32 / size as f32;
                    for y in 0..n {
                        for x in 0..n {
                            let p = [x as f32 / size as f32, y as f32 / size as f32, z];
                            let px = &mut plane[(y * n + x) * 4..][..4];
                            if detail {
                                px[0] = (worley_fbm(p, 4, 70) * 255.0) as u8;
                                px[1] = (worley_fbm(p, 8, 71) * 255.0) as u8;
                                px[2] = (worley_fbm(p, 16, 72) * 255.0) as u8;
                                px[3] = 255;
                            } else {
                                let per = perlin(p, 8, 10);
                                let low = worley_fbm(p, 4, 20);
                                // Schneider's shape channel: Perlin remapped
                                // against the low-frequency Worley.
                                px[0] = (remap(per, low, 1.0, 0.0, 1.0) * 255.0) as u8;
                                px[1] = (low * 255.0) as u8;
                                px[2] = (worley_fbm(p, 8, 21) * 255.0) as u8;
                                px[3] = (worley_fbm(p, 16, 22) * 255.0) as u8;
                            }
                        }
                    }
                }
            });
        }
    });
    out
}

fn cache_path(dir: &Path, name: &str, size: u32) -> PathBuf {
    dir.join(format!("cloud_noise_{name}_{size}_{CACHE_VERSION}.raw"))
}

fn load_or_bake(dir: &Path, name: &str, size: u32, detail: bool) -> Result<NoiseVolume, NoiseError> {
    let path = cache_path(dir, name, size);
    let want = (size as usize).pow(3) * 4;
    if let Ok(mut f) = std::fs::File::open(&path) {
        let mut rgba = Vec::with_capacity(want);
        f.read_to_end(&mut rgba)?;
        if rgba.len() == want {
            tracing::info!(path = %path.display(), "cloud noise from cache");
            return Ok(NoiseVolume { size, rgba });
        }
        tracing::warn!(path = %path.display(), "cloud noise cache is the wrong size, rebaking");
    }
    let started = std::time::Instant::now();
    let rgba = bake(size, detail);
    tracing::info!(
        size,
        seconds = started.elapsed().as_secs_f32(),
        "cloud noise baked"
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::File::create(&path)?.write_all(&rgba)?;
    Ok(NoiseVolume { size, rgba })
}

/// 128³ shape volume. Baked once, then read from `dir`.
pub fn base(dir: &Path) -> Result<NoiseVolume, NoiseError> {
    load_or_bake(dir, "base", BASE_SIZE, false)
}

/// 32³ detail volume.
pub fn detail(dir: &Path) -> Result<NoiseVolume, NoiseError> {
    load_or_bake(dir, "detail", DETAIL_SIZE, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A seam in the volume is a line across the whole sky, so tileability is
    /// the property worth a test.
    #[test]
    fn noise_tiles() {
        for (y, z) in [(0.3, 0.7), (0.11, 0.92)] {
            let a = worley([0.0, y, z], 4, 20);
            let b = worley([1.0, y, z], 4, 20);
            assert!((a - b).abs() < 1e-4, "worley seam: {a} vs {b}");
            let a = perlin([0.0, y, z], 8, 10);
            let b = perlin([1.0, y, z], 8, 10);
            assert!((a - b).abs() < 1e-4, "perlin seam: {a} vs {b}");
        }
    }

    #[test]
    fn noise_is_in_range_and_not_flat() {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for i in 0..500 {
            let t = i as f32 / 500.0;
            let v = worley_fbm([t, t * 0.37, t * 0.71], 4, 20);
            assert!((0.0..=1.0).contains(&v), "worley_fbm out of range: {v}");
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(hi - lo > 0.2, "worley_fbm is flat: {lo}..{hi}");
    }

    #[test]
    fn cloud_cb_layout_matches_the_spvasm() {
        use std::mem::offset_of;
        assert_eq!(std::mem::size_of::<CloudCb>(), 272);
        assert_eq!(std::mem::offset_of!(CloudCb, camera_pos), 64);
        assert_eq!(std::mem::offset_of!(CloudCb, sun_dir), 80);
        assert_eq!(std::mem::offset_of!(CloudCb, sun_color), 96);
        assert_eq!(std::mem::offset_of!(CloudCb, layer), 112);
        assert_eq!(std::mem::offset_of!(CloudCb, shape), 128);
        assert_eq!(std::mem::offset_of!(CloudCb, wind), 144);
        assert_eq!(std::mem::offset_of!(CloudCb, steps), 160);
        assert_eq!(std::mem::offset_of!(CloudCb, ambient), 176);
        assert_eq!(std::mem::offset_of!(CloudCb, inv_extent), 192);
        assert_eq!(std::mem::offset_of!(CloudCb, cloud_rt), 200);
        assert_eq!(std::mem::offset_of!(CloudCb, history_rt), 204);
        assert_eq!(std::mem::offset_of!(CloudCb, prev_view_proj), 208);
        assert!(std::mem::size_of::<CloudCb>() <= harpia_rhi::FRAME_UBO_SIZE as usize);

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
            (9, offset_of!(CloudCb, inv_extent) as u32),
            (10, offset_of!(CloudCb, cloud_rt) as u32),
            (11, offset_of!(CloudCb, history_rt) as u32),
            (12, offset_of!(CloudCb, prev_view_proj) as u32),
        ];
        for shader in [
            "prog/samples/gates/clouds/shaders/clouds.ps.spvasm",
            "prog/samples/gates/clouds/shaders/reproject.ps.spvasm",
            "prog/samples/gates/clouds/shaders/composite.ps.spvasm",
            "prog/samples/gates/clouds/shaders/blit.ps.spvasm",
        ] {
            crate::spvasm_layout::assert_prefix_matches(shader, "Cloud", &expected);
        }
    }

    #[test]
    fn slice_cb_is_one_chunk() {
        assert_eq!(std::mem::size_of::<CloudSliceCb>(), 16);
        assert_eq!(std::mem::offset_of!(CloudSliceCb, slice), 8);
        assert_eq!(std::mem::offset_of!(CloudSliceCb, channel), 12);
    }

    #[test]
    fn bake_is_deterministic() {
        let a = bake(8, false);
        let b = bake(8, false);
        assert_eq!(a, b);
        assert_eq!(a.len(), 8 * 8 * 8 * 4);
    }
}
