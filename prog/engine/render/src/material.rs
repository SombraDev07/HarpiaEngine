//! CPU material. Packed into instance attrs for `pbr-grid` (no glTF file yet).

use crate::packing::dielectric_f0;

#[derive(Clone, Copy, Debug)]
pub struct MaterialGpu {
    pub base_color: [f32; 3],
    pub metallic: f32,
    pub roughness: f32,
    pub ao: f32,
    pub reflectance: f32,
    pub clearcoat: f32,
    pub clearcoat_roughness: f32,
    pub emissive: [f32; 3],
    pub fuzz: f32,
    pub fuzz_color: [f32; 3],
}

impl Default for MaterialGpu {
    fn default() -> Self {
        Self {
            base_color: [0.8, 0.8, 0.8],
            metallic: 0.0,
            roughness: 0.5,
            ao: 1.0,
            reflectance: 0.5,
            clearcoat: 0.0,
            clearcoat_roughness: 0.1,
            emissive: [0.0, 0.0, 0.0],
            fuzz: 0.0,
            fuzz_color: [1.0, 1.0, 1.0],
        }
    }
}

impl MaterialGpu {
    pub fn dielectric_f0(&self) -> f32 {
        dielectric_f0(self.reflectance)
    }
}

/// One instance: pos_scale + 4 material vec4s = 80 bytes (locations 2–6).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SphereInstance {
    pub pos_scale: [f32; 4],
    pub base_color_coat_rough: [f32; 4],
    pub orm_f0: [f32; 4],
    pub emissive_coat: [f32; 4],
    pub fuzz_fuzz_color: [f32; 4],
}

impl SphereInstance {
    pub fn from_material(center: [f32; 3], radius: f32, mat: &MaterialGpu) -> Self {
        Self {
            pos_scale: [center[0], center[1], center[2], radius],
            base_color_coat_rough: [
                mat.base_color[0],
                mat.base_color[1],
                mat.base_color[2],
                mat.clearcoat_roughness,
            ],
            orm_f0: [mat.ao, mat.roughness, mat.metallic, mat.dielectric_f0()],
            emissive_coat: [mat.emissive[0], mat.emissive[1], mat.emissive[2], mat.clearcoat],
            fuzz_fuzz_color: [
                mat.fuzz,
                mat.fuzz_color[0],
                mat.fuzz_color[1],
                mat.fuzz_color[2],
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_is_eighty_bytes() {
        assert_eq!(std::mem::size_of::<SphereInstance>(), 80);
    }
}
