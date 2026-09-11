//! Deferred PBR helpers. No `vk::*`.

mod graph;
mod shadow_atlas;
mod atmosphere;
#[cfg(test)]
mod spvasm_layout;
mod cloud_noise;
mod csm;
mod fly_camera;
mod fog;
mod gltf_scene;
mod ibl;
mod lighting;
mod lights;
mod material;
mod mesh;
mod packing;
mod rain;
mod taa;
mod terrain;
mod water;

pub use atmosphere::{
    aerial_desc, distance_to_top, multiscatter_desc, skyview_desc, transmittance_desc,
    AtmosphereCb, AERIAL_DEPTH_KM, AERIAL_SIZE, MULTISCATTER_SIZE, SKYVIEW_H, SKYVIEW_W, TRANSMITTANCE_H,
    TRANSMITTANCE_W,
};
pub use cloud_noise::{
    base as cloud_noise_base, detail as cloud_noise_detail, volume_desc as cloud_volume_desc,
    CloudCb, CloudSliceCb, NoiseError, NoiseVolume, BASE_SIZE, DETAIL_SIZE,
};
pub use water::{wave_omega, WaterCb, WATER_GRID, WATER_HALF};
pub use fly_camera::FlyCamera;
pub use rain::{rain_map_view_proj, RainCb, RAIN_MAP_SIZE};
pub use lights::{
    assign as assign_lights, assign_counted as assign_lights_counted, depth_slice, slice_depth,
    ClusterAssignment, ClusterRange, ConeStats, Light, PointLight,
    CLUSTER_COUNT, CLUSTER_X, CLUSTER_Y, CLUSTER_Z, MAX_LIGHT_INDICES,
};
pub use terrain::{CLIPMAP_PATCH, clipmap_patch_grid, clipmap_patches_per_level, clipmap_patch_count, clipmap_patch_vertex_count, clipmap_patch_bounds, 
    clipmap_range, clipmap_vertex_count, terrain_height, terrain_normal, TerrainCb, CLIPMAP_CELL,
    CLIPMAP_LEVELS, CLIPMAP_N, TERRAIN_OCTAVES,
};
pub use csm::{compute as compute_csm, Camera, Csm, CASCADE_COUNT, DEFAULT_ATLAS_SIZE};
pub use fog::{
    froxel_desc, halton2, inject_dispatch, integrate_dispatch, FogCb, FROXEL_D, FROXEL_FORMAT,
    FROXEL_H, FROXEL_W, INJECT_GROUP, INTEGRATE_GROUP,
};
pub use gltf_scene::{load_path as load_gltf, CpuImage, CpuPrimitive, CpuScene, GltfError};
pub use graph::{
    Access, GraphError, Handle, Layout, Load, Pass, PassId, PassPlan, RenderGraph, Resource,
    ResourceId,
};
pub use shadow_atlas::{
    priority as shadow_priority, wanted_size as shadow_wanted_size, AtlasPlan, ShadowAtlas,
    ShadowRequest, ShadowSlot, TILE_SIZES,
};
pub use ibl::{
    generate as generate_ibl, generate_env, sample_latlong, sample_lod, sky_radiance, IblCpu,
    RgbaImage,
};
pub use lighting::{LightingCb, PushConstants, PCSS_LIGHT_SIZE};
pub use material::{MaterialGpu, SphereInstance};
pub use mesh::{MeshVertex, SphereMesh, Vertex};
pub use packing::{
    color_desc, depth_desc, dielectric_f0, rt3_rgb, sampled_desc, shadow_atlas_desc,
    GBUFFER_COLOR_FORMATS, GBUFFER_DEPTH_FORMAT, INSTANCE_STRIDE, VERTEX_STRIDE, VERTEX_STRIDE_UV,
};
pub use taa::{TaaCb, TAA_BLEND};
