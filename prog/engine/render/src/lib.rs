//! Deferred PBR helpers. No `vk::*`.

mod csm;
mod gltf_scene;
mod ibl;
mod lighting;
mod material;
mod mesh;
mod packing;
mod taa;

pub use csm::{compute as compute_csm, Camera, Csm, CASCADE_COUNT, DEFAULT_ATLAS_SIZE};
pub use gltf_scene::{load_path as load_gltf, CpuImage, CpuPrimitive, CpuScene, GltfError};
pub use ibl::{generate as generate_ibl, IblCpu, RgbaImage};
pub use lighting::{LightingCb, PushConstants};
pub use material::{MaterialGpu, SphereInstance};
pub use mesh::{MeshVertex, SphereMesh, Vertex};
pub use packing::{
    color_desc, depth_desc, dielectric_f0, rt3_rgb, sampled_desc, shadow_atlas_desc,
    GBUFFER_COLOR_FORMATS, GBUFFER_DEPTH_FORMAT, INSTANCE_STRIDE, VERTEX_STRIDE, VERTEX_STRIDE_UV,
};
pub use taa::{TaaCb, TAA_BLEND};
