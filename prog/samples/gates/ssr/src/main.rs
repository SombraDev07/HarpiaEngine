//! Fase 7, gate `ssr`: FidelityFX SSSR hierarchical march over a mirror floor.
//!
//! Hi-Z is SPD min on the depth buffer. `-- --no-ssr` skips the march.
//! DNSR is not in this slice.

use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_ffx_spd::{self as spd, Dispatch};
use harpia_ffx_sssr::{self as sssr, DEPTH_THICKNESS, MAX_TRAVERSAL_INTERSECTIONS};
use harpia_math::{Mat4, Vec3, Vec4, perspective_vk};
use harpia_render::{
    color_desc, depth_desc, MaterialGpu, PushConstants, SphereInstance, SphereMesh,
    GBUFFER_DEPTH_FORMAT, INSTANCE_STRIDE, VERTEX_STRIDE,
};
use harpia_render::{Access, Pass, RenderGraph};
use harpia_rhi::{
    Buffer, ComputePipeline, ComputePipelineDesc, Device, Extent2D, Format, FrameInfo, Gpu,
    GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets, Texture, TextureDesc, TextureDim,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const HDR: Format = Format::Rgba16Float;
const SCENE_FORMATS: [Format; 3] = [HDR, Format::Rgba8Unorm, Format::R32Float];
const FOV_Y: f32 = 50.0;
const NEAR: f32 = 0.5;
const FAR: f32 = 80.0;
const ATOMIC_SLOT: u32 = 9;
/// Hi-Z occupies UAV slots `0..mips`. The reflection image must not reuse them:
/// bindless storage updates are consumed at **execute**, so the last CPU bind
/// of a slot wins for every dispatch in the command buffer.
const SLOT_SSR: u32 = 15;

#[repr(C)]
#[derive(Clone, Copy)]
struct SceneCb {
    camera_pos: Vec4,
    sun_dir: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CopyCb {
    params: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SssrCb {
    inv_view_proj: Mat4,
    inv_proj: Mat4,
    view: Mat4,
    proj: Mat4,
    screen: Vec4,
    depth: u32,
    normal: u32,
    color: u32,
    max_steps: u32,
    thickness: f32,
    _pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ApplyCb {
    inv_extent: [f32; 2],
    color: u32,
    ssr: u32,
    enable: u32,
    _pad: [u32; 3],
}

struct Targets {
    color: Texture,
    normal: Texture,
    depth_color: Texture,
    depth: Texture,
    hiz: Texture,
    ssr: Texture,
    dispatch: Dispatch,
}

struct SsrGate {
    scene_pso: Option<GraphicsPipeline>,
    apply_pso: Option<GraphicsPipeline>,
    copy_pso: Option<ComputePipeline>,
    spd_pso: Option<ComputePipeline>,
    intersect_pso: Option<ComputePipeline>,
    floor_vb: Option<Buffer>,
    floor_ib: Option<Buffer>,
    floor_inst: Option<Buffer>,
    floor_idx: u32,
    sph_vb: Option<Buffer>,
    sph_ib: Option<Buffer>,
    sph_inst: Option<Buffer>,
    sph_idx: u32,
    sph_count: u32,
    atomic: Option<Buffer>,
    rt: Option<Targets>,
    extent: Extent2D,
    ssr: bool,
}

impl Default for SsrGate {
    fn default() -> Self {
        Self {
            scene_pso: None,
            apply_pso: None,
            copy_pso: None,
            spd_pso: None,
            intersect_pso: None,
            floor_vb: None,
            floor_ib: None,
            floor_inst: None,
            floor_idx: 0,
            sph_vb: None,
            sph_ib: None,
            sph_inst: None,
            sph_idx: 0,
            sph_count: 0,
            atomic: None,
            rt: None,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
            ssr: true,
        }
    }
}

impl SsrGate {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        let dispatch = spd::setup(w, h, None);
        let mips = dispatch.texture_mips().min(SLOT_SSR);
        self.rt = Some(Targets {
            color: gpu.create_texture(&color_desc(w, h, HDR))?,
            normal: gpu.create_texture(&color_desc(w, h, Format::Rgba8Unorm))?,
            depth_color: gpu.create_texture(&color_desc(w, h, Format::R32Float))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
            hiz: gpu.create_texture(&TextureDesc {
                width: w,
                height: h,
                depth_slices: 1,
                dim: TextureDim::D2,
                mip_levels: mips,
                format: Format::R32Float,
                sampled: true,
                storage: true,
                color_attachment: false,
                depth: false,
            })?,
            ssr: gpu.create_texture(&TextureDesc {
                width: w,
                height: h,
                depth_slices: 1,
                dim: TextureDim::D2,
                mip_levels: 1,
                format: HDR,
                sampled: true,
                storage: true,
                color_attachment: false,
                depth: false,
            })?,
            dispatch,
        });
        self.extent = Extent2D {
            width: w,
            height: h,
        };
        Ok(())
    }
}

fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((v as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}

fn instance_bytes(instances: &[SphereInstance]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            instances.as_ptr().cast::<u8>(),
            std::mem::size_of_val(instances),
        )
    }
}

fn camera() -> (Mat4, Vec3) {
    let eye = Vec3::new(0.0, 3.2, 7.5);
    let view = Mat4::look_at_rh(eye, Vec3::new(0.0, 0.4, 0.0), Vec3::Y);
    (view, eye)
}

impl Sample for SsrGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.scene_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/scene.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/scene.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &SCENE_FORMATS,
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE,
                    instance_stride: INSTANCE_STRIDE,
                    depth_test: true,
                    cull_back: true,
                    ..Default::default()
                },
            })
            .context("ssr scene PSO")?,
        );
        self.apply_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: sssr::apply_spirv(),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets::default(),
            })
            .context("ssr apply PSO")?,
        );
        self.copy_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/copy_depth.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("ssr copy CS")?,
        );
        self.spd_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: spd::min_spirv(),
                cs_entry: "CSMain",
            })
            .context("spd min CS")?,
        );
        self.intersect_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: sssr::intersect_spirv(),
                cs_entry: "CSMain",
            })
            .context("sssr intersect CS")?,
        );

        let floor = SphereMesh::plane_xz();
        self.floor_idx = floor.indices.len() as u32;
        self.floor_vb = Some(gpu.create_vertex_buffer(floor.vertex_bytes())?);
        self.floor_ib = Some(gpu.create_index_buffer(floor.index_bytes())?);
        let floor_mat = MaterialGpu {
            base_color: [0.04, 0.045, 0.05],
            metallic: 1.0,
            roughness: 0.0,
            ..MaterialGpu::default()
        };
        let floor_inst = [SphereInstance::from_material([0.0, 0.0, 0.0], 10.0, &floor_mat)];
        self.floor_inst = Some(gpu.create_vertex_buffer(instance_bytes(&floor_inst))?);

        let sph = SphereMesh::uv(20, 14);
        self.sph_idx = sph.indices.len() as u32;
        self.sph_vb = Some(gpu.create_vertex_buffer(sph.vertex_bytes())?);
        self.sph_ib = Some(gpu.create_index_buffer(sph.index_bytes())?);
        let spheres = [
            SphereInstance::from_material(
                [-2.2, 0.9, 0.2],
                0.85,
                &MaterialGpu {
                    base_color: [0.8, 0.15, 0.1],
                    emissive: [4.0, 0.3, 0.1],
                    roughness: 0.8,
                    ..MaterialGpu::default()
                },
            ),
            SphereInstance::from_material(
                [2.0, 0.7, -0.4],
                0.7,
                &MaterialGpu {
                    base_color: [0.1, 0.25, 0.9],
                    emissive: [0.2, 0.5, 5.0],
                    roughness: 0.8,
                    ..MaterialGpu::default()
                },
            ),
            SphereInstance::from_material(
                [0.1, 1.1, -1.8],
                0.95,
                &MaterialGpu {
                    base_color: [0.2, 0.85, 0.25],
                    emissive: [0.4, 5.0, 0.5],
                    roughness: 0.8,
                    ..MaterialGpu::default()
                },
            ),
        ];
        self.sph_count = spheres.len() as u32;
        self.sph_inst = Some(gpu.create_vertex_buffer(instance_bytes(&spheres))?);
        self.atomic = Some(gpu.create_storage_buffer(ATOMIC_SLOT, &spd::atomic_zeros())?);
        Ok(())
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if self.extent != info.extent {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("ssr rt")?;
        let w = info.extent.width.max(1);
        let h = info.extent.height.max(1);
        let aspect = w as f32 / h as f32;
        let (view, eye) = camera();
        let proj = perspective_vk(FOV_Y.to_radians(), aspect, NEAR, FAR);
        let view_proj = proj * view;
        let inv_view_proj = view_proj.inverse();
        let inv_proj = proj.inverse();
        let color_idx = gpu.bindless_index(rt.color)?;
        let normal_idx = gpu.bindless_index(rt.normal)?;
        let depth_idx = gpu.bindless_index(rt.depth_color)?;
        let hiz_idx = gpu.bindless_index(rt.hiz)?;
        let ssr_idx = gpu.bindless_index(rt.ssr)?;

        let mut g = RenderGraph::new();
        let color = g.texture("color", rt.color);
        let normal = g.texture("normal", rt.normal);
        let depth_color = g.texture("depth-color", rt.depth_color);
        let depth = g.texture("depth", rt.depth);
        let hiz = g.texture("hiz", rt.hiz);
        let ssr_tex = g.texture("ssr", rt.ssr);
        g.pass(
            Pass::new("scene")
                .uses(color, Access::ColorWrite)
                .uses(normal, Access::ColorWrite)
                .uses(depth_color, Access::ColorWrite)
                .uses(depth, Access::DepthWrite),
        );
        if self.ssr {
            g.pass(
                Pass::new("copy")
                    .uses(depth_color, Access::Sampled)
                    .uses(hiz, Access::StorageWrite),
            );
            g.pass(
                Pass::new("spd")
                    .uses(hiz, Access::StorageWrite)
                    .uses(hiz, Access::StorageRead),
            );
            g.pass(
                Pass::new("intersect")
                    .uses(hiz, Access::Sampled)
                    .uses(color, Access::Sampled)
                    .uses(normal, Access::Sampled)
                    .uses(ssr_tex, Access::StorageWrite),
            );
            g.pass(
                Pass::new("apply")
                    .uses(color, Access::Sampled)
                    .uses(ssr_tex, Access::Sampled),
            );
        } else {
            g.pass(Pass::new("apply").uses(color, Access::Sampled));
        }
        if let Err(errors) = g.validate() {
            anyhow::bail!(
                "render graph inválido: {}",
                errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
        let mut plans = g.compile().into_iter();
        let mut next = || plans.next().map(|p| p.barriers).unwrap_or_default();

        gpu.barriers(&next())?;
        gpu.write_frame_bytes(as_bytes(&SceneCb {
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
            sun_dir: Vec4::new(0.35, -0.8, 0.4, 0.0),
        }))?;
        gpu.begin_color_pass(
            &[rt.color, rt.normal, rt.depth_color],
            Some(rt.depth),
            &[
                [0.02, 0.03, 0.06, 1.0],
                [0.5, 0.5, 1.0, 1.0],
                [1.0, 0.0, 0.0, 1.0],
            ],
            Some(1.0),
        )?;
        gpu.set_pipeline(self.scene_pso.as_ref().context("scene pso")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.set_push_constants(PushConstants::new(view_proj).as_bytes())?;
        gpu.bind_vertex_buffer(self.floor_vb.context("floor vb")?, 0)?;
        gpu.bind_vertex_buffer(self.floor_inst.context("floor inst")?, 1)?;
        gpu.bind_index_buffer(self.floor_ib.context("floor ib")?)?;
        gpu.draw_indexed(self.floor_idx, 1, 0, 0, 0)?;
        gpu.bind_vertex_buffer(self.sph_vb.context("sph vb")?, 0)?;
        gpu.bind_vertex_buffer(self.sph_inst.context("sph inst")?, 1)?;
        gpu.bind_index_buffer(self.sph_ib.context("sph ib")?)?;
        gpu.draw_indexed(self.sph_idx, self.sph_count, 0, 0, 0)?;
        gpu.end_color_pass()?;
        gpu.mark("scene");

        if self.ssr {
            gpu.barriers(&next())?;
            let mips = rt.dispatch.texture_mips().min(SLOT_SSR);
            for mip in 0..mips {
                gpu.bind_storage_image(rt.hiz, mip, mip)?;
            }
            gpu.bind_storage_image(rt.ssr, 0, SLOT_SSR)?;
            gpu.write_frame_bytes(as_bytes(&CopyCb {
                params: Vec4::new(depth_idx as f32, w as f32, h as f32, 0.0),
            }))?;
            gpu.bind_compute_bindless()?;
            gpu.set_compute_pipeline(self.copy_pso.as_ref().context("copy pso")?)?;
            gpu.dispatch((w + 7) / 8, (h + 7) / 8, 1)?;
            gpu.mark("hiz-copy");

            gpu.barriers(&next())?;
            gpu.write_frame_bytes(&rt.dispatch.constants_bytes())?;
            gpu.bind_compute_bindless()?;
            gpu.set_compute_pipeline(self.spd_pso.as_ref().context("spd pso")?)?;
            gpu.dispatch(rt.dispatch.groups_x, rt.dispatch.groups_y, 1)?;
            gpu.mark("spd");

            gpu.barriers(&next())?;
            gpu.write_frame_bytes(as_bytes(&SssrCb {
                inv_view_proj,
                inv_proj,
                view,
                proj,
                screen: Vec4::new(w as f32, h as f32, 1.0 / w as f32, 1.0 / h as f32),
                depth: hiz_idx,
                normal: normal_idx,
                color: color_idx,
                max_steps: MAX_TRAVERSAL_INTERSECTIONS,
                thickness: DEPTH_THICKNESS,
                _pad: [0; 3],
            }))?;
            gpu.bind_compute_bindless()?;
            gpu.set_compute_pipeline(self.intersect_pso.as_ref().context("intersect pso")?)?;
            gpu.dispatch((w + 7) / 8, (h + 7) / 8, 1)?;
            gpu.mark("sssr");
        }

        gpu.barriers(&next())?;
        gpu.write_frame_bytes(as_bytes(&ApplyCb {
            inv_extent: [1.0 / w as f32, 1.0 / h as f32],
            color: color_idx,
            ssr: ssr_idx,
            enable: u32::from(self.ssr),
            _pad: [0; 3],
        }))?;
        gpu.begin_swapchain_pass([0.02, 0.03, 0.06, 1.0])?;
        gpu.set_pipeline(self.apply_pso.as_ref().context("apply pso")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.rt
            .as_ref()
            .map(|rt| {
                vec![
                    ("color", rt.color),
                    ("ssr", rt.ssr),
                    ("hiz", rt.hiz),
                    ("depth", rt.depth_color),
                ]
            })
            .unwrap_or_default()
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — ssr (FFX SSSR)".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    let ssr = !config.extra.iter().any(|a| a == "--no-ssr");
    run(
        config,
        SsrGate {
            ssr,
            ..SsrGate::default()
        },
    )
}
