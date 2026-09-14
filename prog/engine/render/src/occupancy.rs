//! Occupancy VoxelGI 32³. Sampled in lighting in the same PR that creates it.
//!
//! CPU AABB stamp → RGBA8 volume (R = occupied). The lighting shader cone-traces
//! along the normal; an unused volume is the 4/10 the bar refuses.

use harpia_math::Vec3;
use harpia_rhi::{Format, TextureDesc, TextureDim};

pub const OCCUPANCY_SIZE: u32 = 32;
/// Volume SRV slot (set 5). 0 is fog, 1 is often noise; 2 is free in Sponza.
pub const OCCUPANCY_SLOT: u32 = 2;

#[derive(Clone, Debug)]
pub struct OccupancyVolume {
    pub origin: Vec3,
    pub extent: Vec3,
    /// Packed RGBA8, `SIZE³` texels, R = occupied (0 or 255).
    pub voxels: Vec<u8>,
}

impl OccupancyVolume {
    pub fn new(origin: Vec3, extent: Vec3) -> Self {
        let n = (OCCUPANCY_SIZE * OCCUPANCY_SIZE * OCCUPANCY_SIZE) as usize;
        Self {
            origin,
            extent: extent.max(Vec3::splat(1e-4)),
            voxels: vec![0u8; n * 4],
        }
    }

    pub fn desc() -> TextureDesc {
        TextureDesc {
            width: OCCUPANCY_SIZE,
            height: OCCUPANCY_SIZE,
            depth_slices: OCCUPANCY_SIZE,
            dim: TextureDim::D3,
            mip_levels: 1,
            format: Format::Rgba8Unorm,
            sampled: true,
            storage: false,
            color_attachment: false,
            depth: false,
        }
    }

    pub fn voxel_size(&self) -> Vec3 {
        self.extent / OCCUPANCY_SIZE as f32
    }

    /// Mark every voxel whose cell overlaps `[min, max]` (world).
    pub fn stamp_aabb(&mut self, min: Vec3, max: Vec3) {
        let size = OCCUPANCY_SIZE as f32;
        let to_uv = |p: Vec3| (p - self.origin) / self.extent;
        let u0 = to_uv(min);
        let u1 = to_uv(max);
        let i0 = [
            ((u0.x * size).floor() as i32).clamp(0, OCCUPANCY_SIZE as i32 - 1),
            ((u0.y * size).floor() as i32).clamp(0, OCCUPANCY_SIZE as i32 - 1),
            ((u0.z * size).floor() as i32).clamp(0, OCCUPANCY_SIZE as i32 - 1),
        ];
        let i1 = [
            ((u1.x * size).ceil() as i32).clamp(0, OCCUPANCY_SIZE as i32),
            ((u1.y * size).ceil() as i32).clamp(0, OCCUPANCY_SIZE as i32),
            ((u1.z * size).ceil() as i32).clamp(0, OCCUPANCY_SIZE as i32),
        ];
        for z in i0[2]..i1[2] {
            for y in i0[1]..i1[1] {
                for x in i0[0]..i1[0] {
                    let i = (((z as u32 * OCCUPANCY_SIZE + y as u32) * OCCUPANCY_SIZE + x as u32)
                        * 4) as usize;
                    self.voxels[i] = 255;
                    self.voxels[i + 3] = 255;
                }
            }
        }
    }

    pub fn occupied_count(&self) -> u32 {
        self.voxels.chunks_exact(4).filter(|c| c[0] > 0).count() as u32
    }
}

/// Hemisphere taps around N (not only the normal ray — a floor next to a wall
/// would otherwise miss the occupied voxels sitting to the side).
pub fn cone_ao(vol: &OccupancyVolume, world: Vec3, normal: Vec3) -> f32 {
    let n = normal.normalize_or_zero();
    let vs = vol.voxel_size().x.min(vol.voxel_size().y).min(vol.voxel_size().z);
    let up = if n.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
    let t = n.cross(up).normalize_or_zero();
    let b = n.cross(t);
    let dirs = [n, (n + t * 0.7).normalize(), (n - t * 0.7).normalize(), (n + b * 0.7).normalize(), (n - b * 0.7).normalize()];
    let mut occ = 0.0f32;
    for dir in dirs {
        let mut p = world + dir * vs * 1.5;
        for _ in 0..4 {
            p += dir * vs;
            occ += sample_occ(vol, p);
        }
    }
    1.0 / (1.0 + occ * 1.1)
}

fn sample_occ(vol: &OccupancyVolume, p: Vec3) -> f32 {
    let uv = (p - vol.origin) / vol.extent;
    if uv.min_element() < 0.0 || uv.max_element() > 1.0 {
        return 0.0;
    }
    let size = OCCUPANCY_SIZE as f32;
    let x = (uv.x * size).floor().clamp(0.0, size - 1.0) as u32;
    let y = (uv.y * size).floor().clamp(0.0, size - 1.0) as u32;
    let z = (uv.z * size).floor().clamp(0.0, size - 1.0) as u32;
    let i = (((z * OCCUPANCY_SIZE + y) * OCCUPANCY_SIZE + x) * 4) as usize;
    vol.voxels[i] as f32 / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_darkens_the_floor_beside_it() {
        let mut vol = OccupancyVolume::new(Vec3::new(-4.0, 0.0, -4.0), Vec3::splat(8.0));
        vol.stamp_aabb(Vec3::new(-0.6, 0.0, -0.6), Vec3::new(0.6, 2.4, 0.6));
        assert!(vol.occupied_count() > 10, "stamp wrote nothing");
        let open = cone_ao(&vol, Vec3::new(-3.0, 0.05, -3.0), Vec3::Y);
        let next_to_box = cone_ao(&vol, Vec3::new(0.0, 0.05, 1.2), Vec3::Y);
        assert!(
            next_to_box < open - 0.05,
            "occupancy next to the box is {next_to_box:.3}, open floor is {open:.3}: \
             the volume is not reaching the lighting"
        );
    }

    #[test]
    fn empty_volume_is_a_no_op() {
        let vol = OccupancyVolume::new(Vec3::new(-4.0, 0.0, -4.0), Vec3::splat(8.0));
        let ao = cone_ao(&vol, Vec3::ZERO, Vec3::Y);
        assert!((ao - 1.0).abs() < 1e-4, "empty volume must not darken, got {ao}");
    }
}
