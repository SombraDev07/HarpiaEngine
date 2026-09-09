//! Cascaded shadow maps from the **real** camera frustum (FOV, aspect, near/far, view).
//! Not the C++ 60° / 16:9 lie. Texel snap uses the atlas size, not `1/2048`.

use harpia_math::{perspective_vk, Mat4, Vec3, Vec4};

pub const CASCADE_COUNT: usize = 4;
pub const DEFAULT_ATLAS_SIZE: u32 = 2048;
pub const SPLIT_LAMBDA: f32 = 0.75;

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye, self.target, self.up)
    }

    /// Vulkan clip space (see [`perspective_vk`]): +Y up in the world is up on screen.
    pub fn proj(&self) -> Mat4 {
        perspective_vk(self.fov_y, self.aspect.max(1e-4), self.near.max(1e-3), self.far)
    }

    pub fn view_proj(&self) -> Mat4 {
        self.proj() * self.view()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Csm {
    pub atlas_size: u32,
    pub view_proj: [Mat4; 4],
    /// Positive view-space distances of each cascade far plane (`-view.z` in RH).
    pub splits: Vec4,
}

impl Csm {
    pub fn texel_size(&self) -> f32 {
        1.0 / (self.atlas_size.max(1) as f32)
    }

    pub fn tile_viewport(&self, cascade: usize) -> (f32, f32, f32, f32) {
        let tile = (self.atlas_size / 2).max(1) as f32;
        let i = cascade.min(CASCADE_COUNT - 1);
        let x = (i % 2) as f32 * tile;
        let y = (i / 2) as f32 * tile;
        (x, y, tile, tile)
    }
}

pub fn compute(camera: &Camera, sun_dir: Vec3, atlas_size: u32) -> Csm {
    let atlas_size = atlas_size.max(2);
    let mut split_far = [0.0f32; CASCADE_COUNT];
    for (i, dst) in split_far.iter_mut().enumerate() {
        *dst = practical_split(camera.near, camera.far, i + 1, CASCADE_COUNT, SPLIT_LAMBDA);
    }
    let view_inv = camera.view().inverse();
    let sun = {
        let s = sun_dir.normalize_or_zero();
        if s.length_squared() < 1e-8 {
            Vec3::Y
        } else {
            s
        }
    };
    let mut view_proj = [Mat4::IDENTITY; CASCADE_COUNT];
    for i in 0..CASCADE_COUNT {
        let near_i = if i == 0 { camera.near } else { split_far[i - 1] };
        let far_i = split_far[i];
        let corners_v = frustum_corners(camera.fov_y, camera.aspect, near_i, far_i);
        let corners_w = corners_v.map(|c| view_inv.transform_point3(c));
        let mut center = Vec3::ZERO;
        for c in corners_w {
            center += c;
        }
        center /= 8.0;
        let mut radius = 0.0f32;
        for c in corners_w {
            radius = radius.max((c - center).length());
        }
        radius = (radius * 1.05).max(0.05);

        let tile_res = (atlas_size / 2) as f32;
        let mut units_per_texel = (radius * 2.0) / tile_res;
        if units_per_texel > 1e-6 {
            radius = (radius / units_per_texel).ceil() * units_per_texel;
            units_per_texel = (radius * 2.0) / tile_res;
        }

        let up = if sun.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        // Snap the sphere centre on the light's right/up axes so the grid follows atlas texels.
        let look = -sun;
        let right = up.cross(look).normalize_or_zero();
        let right = if right.length_squared() < 1e-8 {
            Vec3::X
        } else {
            right
        };
        let light_up = look.cross(right).normalize_or_zero();
        let cx = (center.dot(right) / units_per_texel).round() * units_per_texel;
        let cy = (center.dot(light_up) / units_per_texel).round() * units_per_texel;
        let cz = center.dot(look);
        let center = right * cx + light_up * cy + look * cz;
        let dist = radius * 2.0;
        let eye = center + sun * dist;
        let light_view = Mat4::look_at_rh(eye, center, up);
        let z_near = (dist - radius).max(0.05);
        let z_far = dist + radius;
        let ortho = Mat4::orthographic_rh(-radius, radius, -radius, radius, z_near, z_far);
        view_proj[i] = ortho * light_view;
    }
    Csm {
        atlas_size,
        view_proj,
        splits: Vec4::new(split_far[0], split_far[1], split_far[2], split_far[3]),
    }
}

fn practical_split(near: f32, far: f32, i: usize, n: usize, lambda: f32) -> f32 {
    let si = i as f32 / n as f32;
    let log = near * (far / near.max(near + 1e-4)).powf(si);
    let uni = near + (far - near) * si;
    log * lambda + uni * (1.0 - lambda)
}

fn frustum_corners(fov_y: f32, aspect: f32, near: f32, far: f32) -> [Vec3; 8] {
    let tan_half = (fov_y * 0.5).tan();
    let aspect = aspect.max(1e-4);
    let ny = tan_half * near;
    let nx = ny * aspect;
    let fy = tan_half * far;
    let fx = fy * aspect;
    [
        Vec3::new(-nx, -ny, -near),
        Vec3::new(nx, -ny, -near),
        Vec3::new(-nx, ny, -near),
        Vec3::new(nx, ny, -near),
        Vec3::new(-fx, -fy, -far),
        Vec3::new(fx, -fy, -far),
        Vec3::new(-fx, fy, -far),
        Vec3::new(fx, fy, -far),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(fov_y: f32, aspect: f32) -> Camera {
        Camera {
            eye: Vec3::new(0.0, 4.0, 12.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov_y,
            aspect,
            near: 0.5,
            far: 80.0,
        }
    }

    fn sun() -> Vec3 {
        Vec3::new(0.35, 0.8, 0.4).normalize()
    }

    #[test]
    fn aspect_changes_cascade_vp() {
        let a = compute(&cam(60.0_f32.to_radians(), 16.0 / 9.0), sun(), 2048);
        let b = compute(&cam(60.0_f32.to_radians(), 4.0 / 3.0), sun(), 2048);
        assert_ne!(a.view_proj[0], b.view_proj[0]);
        assert_ne!(a.view_proj[3], b.view_proj[3]);
    }

    #[test]
    fn fov_is_not_hardcoded_sixty() {
        let fake = compute(&cam(60.0_f32.to_radians(), 16.0 / 9.0), sun(), 2048);
        let real = compute(&cam(40.0_f32.to_radians(), 16.0 / 9.0), sun(), 2048);
        assert_ne!(fake.view_proj[3], real.view_proj[3]);
    }

    #[test]
    fn texel_comes_from_atlas_size() {
        let a = compute(&cam(50.0_f32.to_radians(), 1.5), sun(), 256);
        let b = compute(&cam(50.0_f32.to_radians(), 1.5), sun(), 2048);
        assert!((a.texel_size() - 1.0 / 256.0).abs() < 1e-8);
        assert!((b.texel_size() - 1.0 / 2048.0).abs() < 1e-8);
        assert_ne!(a.view_proj[0], b.view_proj[0]);
    }

    #[test]
    fn splits_increase() {
        let c = compute(&cam(55.0_f32.to_radians(), 1.777), sun(), 2048);
        assert!(c.splits.x < c.splits.y);
        assert!(c.splits.y < c.splits.z);
        assert!(c.splits.z < c.splits.w);
    }
}
