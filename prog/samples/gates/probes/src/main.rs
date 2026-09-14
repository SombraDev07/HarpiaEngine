//! Fase 7, gate `probes`: CPU-seeded lat-long sampled on an SSSR miss.
//! Dual RT: `--no-probes` is the off image in the same run.

use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_ffx_spd::{self as spd, Dispatch};
use harpia_ffx_sssr::{self as sssr, DEPTH_THICKNESS, MAX_TRAVERSAL_INTERSECTIONS};
use harpia_math::{perspective_vk, Mat4, Vec3, Vec4};
use harpia_render::{
    box_radiance, color_desc, depth_desc, seed_probe, MaterialGpu, PushConstants, SphereInstance,
    SphereMesh, GBUFFER_DEPTH_FORMAT, INSTANCE_STRIDE, PROBE_MIPS, VERTEX_STRIDE,
};
use harpia_rhi::{
    Buffer, ComputePipeline, ComputePipelineDesc, Device, Extent2D, Format, FrameInfo, Gpu,
    GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets, Texture, TextureDesc, TextureDim,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const HDR: Format = Format::Rgba16Float;
const SCENE_FORMATS: [Format; 3] = [HDR, Format::Rgba8Unorm, Format::R32Float];
const LDR: [Format; 1] = [Format::Rgba8Unorm];
const FOV_Y: f32 = 50.0;
const NEAR: f32 = 0.5;
const FAR: f32 = 80.0;
const ATOMIC_SLOT: u32 = 9;
const SLOT_SSR: u32 = 15;

#[repr(C)]
#[derive(Clone, Copy)]
struct SceneCb {
    camera_pos: Vec4,
    sun_dir: Vec4,
    albedo: Vec4,
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
    inv_view_proj: Mat4,
    camera_pos: Vec4,
    inv_extent: [f32; 2],
    color: u32,
    depth: u32,
    normal: u32,
    ssr: u32,
    probe: u32,
    enable_ssr: u32,
    enable_probe: u32,
    probe_mips: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BlitCb {
    inv_extent: [f32; 2],
    src: u32,
    _pad: u32,
}

struct Targets {
    color: Texture,
    normal: Texture,
    depth_color: Texture,
    depth: Texture,
    hiz: Texture,
    ssr: Texture,
    on: Texture,
    off: Texture,
    dispatch: Dispatch,
}

struct ProbesGate {
    scene_pso: Option<GraphicsPipeline>,
    room_pso: Option<GraphicsPipeline>,
    apply_pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
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
    plane_vb: Option<Buffer>,
    plane_ib: Option<Buffer>,
    plane_idx: u32,
    atomic: Option<Buffer>,
    probe: Option<Texture>,
    rt: Option<Targets>,
    extent: Extent2D,
    compared: bool,
}

impl Default for ProbesGate {
    fn default() -> Self {
        Self {
            scene_pso: None,
            room_pso: None,
            apply_pso: None,
            blit_pso: None,
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
            plane_vb: None,
            plane_ib: None,
            plane_idx: 0,
            atomic: None,
            probe: None,
            rt: None,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
            compared: false,
        }
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

fn plane_world(origin: Vec3, normal: Vec3, tangent: Vec3, half_u: f32, half_v: f32) -> Mat4 {
    let n = normal.normalize();
    let t = tangent.normalize();
    let b = n.cross(t).normalize();
    Mat4::from_cols(
        Vec4::new(t.x * half_u, t.y * half_u, t.z * half_u, 0.0),
        Vec4::new(n.x, n.y, n.z, 0.0),
        Vec4::new(b.x * half_v, b.y * half_v, b.z * half_v, 0.0),
        Vec4::new(origin.x, origin.y, origin.z, 1.0),
    )
}

impl ProbesGate {
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
            on: gpu.create_texture(&color_desc(w, h, Format::Rgba8Unorm))?,
            off: gpu.create_texture(&color_desc(w, h, Format::Rgba8Unorm))?,
            dispatch,
        });
        self.extent = Extent2D {
            width: w,
            height: h,
        };
        Ok(())
    }
}

impl Sample for ProbesGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        let seeded = seed_probe(Vec3::new(0.0, 1.2, 0.0), 12.0, box_radiance);
        let probe = gpu.create_texture(&seeded.latlong.desc())?;
        for (mip, data) in seeded.latlong.mips.iter().enumerate() {
            gpu.upload_texture_mip(probe, mip as u32, data)?;
        }
        self.probe = Some(probe);

        let scene_targets = PipelineTargets {
            color_formats: &SCENE_FORMATS,
            depth_format: Some(GBUFFER_DEPTH_FORMAT),
            vertex_stride: VERTEX_STRIDE,
            instance_stride: INSTANCE_STRIDE,
            depth_test: true,
            cull_back: true,
            ..Default::default()
        };
        self.scene_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/scene.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/scene.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: scene_targets,
            })
            .context("probes scene")?,
        );
        self.room_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/room.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/room.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &SCENE_FORMATS,
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE,
                    depth_test: true,
                    cull_back: false,
                    ..Default::default()
                },
            })
            .context("probes room")?,
        );
        self.apply_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/apply.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &LDR,
                    ..Default::default()
                },
            })
            .context("probes apply")?,
        );
        self.blit_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/blit.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets::default(),
            })
            .context("blit")?,
        );
        self.copy_pso = Some(gpu.create_compute_pipeline(&ComputePipelineDesc {
            cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/copy_depth.cs.spv")),
            cs_entry: "CSMain",
        })?);
        self.spd_pso = Some(gpu.create_compute_pipeline(&ComputePipelineDesc {
            cs_spirv: spd::min_spirv(),
            cs_entry: "CSMain",
        })?);
        self.intersect_pso = Some(gpu.create_compute_pipeline(&ComputePipelineDesc {
            cs_spirv: sssr::intersect_spirv(),
            cs_entry: "CSMain",
        })?);

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
        self.floor_inst = Some(gpu.create_vertex_buffer(instance_bytes(&[
            SphereInstance::from_material([0.0, 0.0, 0.0], 8.0, &floor_mat),
        ]))?);

        let sph = SphereMesh::uv(20, 14);
        self.sph_idx = sph.indices.len() as u32;
        self.sph_vb = Some(gpu.create_vertex_buffer(sph.vertex_bytes())?);
        self.sph_ib = Some(gpu.create_index_buffer(sph.index_bytes())?);
        let chrome = MaterialGpu {
            base_color: [0.95, 0.95, 0.95],
            metallic: 1.0,
            roughness: 0.05,
            ..MaterialGpu::default()
        };
        self.sph_count = 1;
        self.sph_inst = Some(gpu.create_vertex_buffer(instance_bytes(&[
            SphereInstance::from_material([0.0, 1.1, 0.0], 1.0, &chrome),
        ]))?);

        let plane = SphereMesh::plane_xz();
        self.plane_idx = plane.indices.len() as u32;
        self.plane_vb = Some(gpu.create_vertex_buffer(plane.vertex_bytes())?);
        self.plane_ib = Some(gpu.create_index_buffer(plane.index_bytes())?);
        self.atomic = Some(gpu.create_storage_buffer(ATOMIC_SLOT, &spd::atomic_zeros())?);
        Ok(())
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if self.extent != info.extent {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let w = info.extent.width.max(1);
        let h = info.extent.height.max(1);
        let eye = Vec3::new(0.0, 2.4, 7.0);
        let view = Mat4::look_at_rh(eye, Vec3::new(0.0, 0.8, 0.0), Vec3::Y);
        let proj = perspective_vk(FOV_Y.to_radians(), w as f32 / h as f32, NEAR, FAR);
        let view_proj = proj * view;
        let inv_view_proj = view_proj.inverse();

        gpu.write_frame_bytes(as_bytes(&SceneCb {
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
            sun_dir: Vec4::new(0.25, -0.8, 0.4, 0.0),
            albedo: Vec4::ONE,
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
        gpu.set_pipeline(self.scene_pso.as_ref().context("scene")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.set_push_constants(PushConstants::new(view_proj).as_bytes())?;
        gpu.bind_vertex_buffer(self.floor_vb.context("fvb")?, 0)?;
        gpu.bind_vertex_buffer(self.floor_inst.context("fi")?, 1)?;
        gpu.bind_index_buffer(self.floor_ib.context("fib")?)?;
        gpu.draw_indexed(self.floor_idx, 1, 0, 0, 0)?;
        gpu.bind_vertex_buffer(self.sph_vb.context("svb")?, 0)?;
        gpu.bind_vertex_buffer(self.sph_inst.context("si")?, 1)?;
        gpu.bind_index_buffer(self.sph_ib.context("sib")?)?;
        gpu.draw_indexed(self.sph_idx, self.sph_count, 0, 0, 0)?;

        gpu.set_pipeline(self.room_pso.as_ref().context("room")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.bind_vertex_buffer(self.plane_vb.context("pvb")?, 0)?;
        gpu.bind_index_buffer(self.plane_ib.context("pib")?)?;
        let walls = [
            (
                plane_world(Vec3::new(8.0, 2.5, 0.0), -Vec3::X, Vec3::Z, 2.5, 8.0),
                Vec4::new(0.85, 0.12, 0.10, 1.0),
            ),
            (
                plane_world(Vec3::new(-8.0, 2.5, 0.0), Vec3::X, Vec3::Z, 2.5, 8.0),
                Vec4::new(0.10, 0.72, 0.75, 1.0),
            ),
            (
                plane_world(Vec3::new(0.0, 2.5, -8.0), Vec3::Z, Vec3::X, 8.0, 2.5),
                Vec4::new(0.12, 0.22, 0.85, 1.0),
            ),
            (
                plane_world(Vec3::new(0.0, 2.5, 8.0), -Vec3::Z, Vec3::X, 8.0, 2.5),
                Vec4::new(0.88, 0.78, 0.12, 1.0),
            ),
        ];
        for (world, albedo) in walls {
            gpu.write_frame_bytes(as_bytes(&SceneCb {
                camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
                sun_dir: Vec4::new(0.25, -0.8, 0.4, 0.0),
                albedo,
            }))?;
            gpu.set_push_constants(PushConstants::with_world(view_proj, world).as_bytes())?;
            gpu.draw_indexed(self.plane_idx, 1, 0, 0, 0)?;
        }
        gpu.end_color_pass()?;
        gpu.mark("scene");

        let mips = rt.dispatch.texture_mips().min(SLOT_SSR);
        for mip in 0..mips {
            gpu.bind_storage_image(rt.hiz, mip, mip)?;
        }
        gpu.bind_storage_image(rt.ssr, 0, SLOT_SSR)?;
        gpu.write_frame_bytes(as_bytes(&CopyCb {
            params: Vec4::new(
                gpu.bindless_index(rt.depth_color)? as f32,
                w as f32,
                h as f32,
                0.0,
            ),
        }))?;
        gpu.bind_compute_bindless()?;
        gpu.set_compute_pipeline(self.copy_pso.as_ref().context("copy")?)?;
        gpu.dispatch(w.div_ceil(8), h.div_ceil(8), 1)?;

        gpu.storage_barrier(rt.hiz)?;
        gpu.write_frame_bytes(&rt.dispatch.constants_bytes())?;
        gpu.bind_compute_bindless()?;
        gpu.set_compute_pipeline(self.spd_pso.as_ref().context("spd")?)?;
        gpu.dispatch(rt.dispatch.groups_x, rt.dispatch.groups_y, 1)?;

        gpu.storage_barrier(rt.hiz)?;
        gpu.write_frame_bytes(as_bytes(&SssrCb {
            inv_view_proj,
            inv_proj: proj.inverse(),
            view,
            proj,
            screen: Vec4::new(w as f32, h as f32, 1.0 / w as f32, 1.0 / h as f32),
            depth: gpu.bindless_index(rt.hiz)?,
            normal: gpu.bindless_index(rt.normal)?,
            color: gpu.bindless_index(rt.color)?,
            max_steps: MAX_TRAVERSAL_INTERSECTIONS,
            thickness: DEPTH_THICKNESS,
            _pad: [0; 3],
        }))?;
        gpu.bind_compute_bindless()?;
        gpu.set_compute_pipeline(self.intersect_pso.as_ref().context("sssr")?)?;
        gpu.dispatch(w.div_ceil(8), h.div_ceil(8), 1)?;
        gpu.storage_barrier(rt.ssr)?;
        gpu.mark("sssr");

        let apply = ApplyCb {
            inv_view_proj,
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
            inv_extent: [1.0 / w as f32, 1.0 / h as f32],
            color: gpu.bindless_index(rt.color)?,
            depth: gpu.bindless_index(rt.depth_color)?,
            normal: gpu.bindless_index(rt.normal)?,
            ssr: gpu.bindless_index(rt.ssr)?,
            probe: gpu.bindless_index(self.probe.context("probe")?)?,
            enable_ssr: 1,
            enable_probe: 1,
            probe_mips: PROBE_MIPS,
        };
        gpu.write_frame_bytes(as_bytes(&apply))?;
        gpu.begin_color_pass(&[rt.on], None, &[[0.0; 4]], None)?;
        gpu.set_pipeline(self.apply_pso.as_ref().context("apply")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        let mut off = apply;
        off.enable_probe = 0;
        gpu.write_frame_bytes(as_bytes(&off))?;
        gpu.begin_color_pass(&[rt.off], None, &[[0.0; 4]], None)?;
        gpu.set_pipeline(self.apply_pso.as_ref().context("apply")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        gpu.write_frame_bytes(as_bytes(&BlitCb {
            inv_extent: [1.0 / w as f32, 1.0 / h as f32],
            src: gpu.bindless_index(rt.on)?,
            _pad: 0,
        }))?;
        gpu.begin_swapchain_pass([0.0; 4])?;
        gpu.set_pipeline(self.blit_pso.as_ref().context("blit")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }

    fn finish(&mut self, gpu: &mut Gpu) -> Result<()> {
        if self.compared {
            return Ok(());
        }
        self.compared = true;
        let rt = self.rt.as_ref().context("rt")?;
        let on = gpu.read_texture(rt.on)?;
        let off = gpu.read_texture(rt.off)?;
        anyhow::ensure!(on.bytes.len() == off.bytes.len());
        let mut differing = 0u64;
        for (a, b) in on.bytes.iter().zip(off.bytes.iter()) {
            if a != b {
                differing += 1;
            }
        }
        let pct = 100.0 * differing as f64 / on.bytes.len().max(1) as f64;
        tracing::info!(
            canais_diferentes = differing,
            percent = format!("{pct:.2}"),
            "probe no miss do SSR contra miss preto"
        );
        anyhow::ensure!(
            pct > 1.0,
            "probes mudaram só {pct:.3}%: o miss do SSR não está a ler o lat-long"
        );
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.rt
            .as_ref()
            .map(|rt| vec![("on", rt.on), ("off", rt.off), ("ssr", rt.ssr)])
            .unwrap_or_default()
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — probes".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, ProbesGate::default())
}
