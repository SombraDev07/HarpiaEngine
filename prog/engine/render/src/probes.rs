//! Reflection probes (≤4). CPU seed of a lat-long; SSR miss samples it.
//!
//! Capture is the upload of that lat-long (and its GGX mips). The host C++ also
//! rendered cubemap faces; we seed from a radiance function of the room so the
//! miss is a colour the scene actually owns, not a second IBL.

use harpia_math::Vec3;

use crate::ibl::{latlong_from_fn, prefilter, RgbaImage};

pub const PROBE_MAX: u32 = 4;
pub const PROBE_LAT_W: u32 = 128;
pub const PROBE_LAT_H: u32 = 64;
pub const PROBE_MIPS: u32 = 5;

#[derive(Clone, Debug)]
pub struct ReflectionProbe {
    pub position: Vec3,
    pub radius: f32,
    pub latlong: RgbaImage,
}

/// Six-sided room: +X red, −X cyan, +Y green, −Y dark, +Z blue, −Z yellow.
/// A miss that samples this cannot be confused with the procedural sky.
pub fn box_radiance(dir: Vec3) -> Vec3 {
    let n = dir.normalize_or_zero();
    let a = n.abs();
    if a.x >= a.y && a.x >= a.z {
        if n.x >= 0.0 {
            Vec3::new(0.85, 0.12, 0.10)
        } else {
            Vec3::new(0.10, 0.72, 0.75)
        }
    } else if a.y >= a.z {
        if n.y >= 0.0 {
            Vec3::new(0.18, 0.78, 0.22)
        } else {
            Vec3::new(0.08, 0.07, 0.06)
        }
    } else if n.z >= 0.0 {
        Vec3::new(0.12, 0.22, 0.85)
    } else {
        Vec3::new(0.88, 0.78, 0.12)
    }
}

/// Warm atrium tint for Sponza: stone/wood, not the outdoor IBL.
pub fn atrium_radiance(dir: Vec3) -> Vec3 {
    let n = dir.normalize_or_zero();
    let t = (n.y * 0.5 + 0.5).clamp(0.0, 1.0);
    let floor = Vec3::new(0.22, 0.16, 0.12);
    let wall = Vec3::new(0.42, 0.32, 0.24);
    let skyhole = Vec3::new(0.55, 0.62, 0.78);
    if n.y > 0.35 {
        wall.lerp(skyhole, ((n.y - 0.35) / 0.65).clamp(0.0, 1.0))
    } else {
        floor.lerp(wall, t)
    }
}

pub fn seed(position: Vec3, radius: f32, radiance: impl Fn(Vec3) -> Vec3) -> ReflectionProbe {
    let env = latlong_from_fn(PROBE_LAT_W, PROBE_LAT_H, radiance);
    let latlong = prefilter(&env, PROBE_MIPS);
    ReflectionProbe {
        position,
        radius,
        latlong,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibl::sample_latlong;

    #[test]
    fn box_seed_is_not_the_sky() {
        let p = seed(Vec3::new(0.0, 1.0, 0.0), 8.0, box_radiance);
        assert_eq!(p.latlong.mip_levels, PROBE_MIPS);
        assert_eq!(p.latlong.mips.len(), PROBE_MIPS as usize);
        let pos_x = sample_latlong(&p.latlong, Vec3::X);
        let pos_z = sample_latlong(&p.latlong, Vec3::Z);
        assert!(
            pos_x.x > pos_x.y && pos_x.x > pos_x.z,
            "+X must be red, got {pos_x:?}"
        );
        assert!(
            pos_z.z > pos_z.x && pos_z.z > pos_z.y,
            "+Z must be blue, got {pos_z:?}"
        );
    }
}
