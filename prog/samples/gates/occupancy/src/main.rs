//! Fase 7, gate `occupancy`: 32³ volume sampled **in the lighting PS**.
//! Dual RT: the pixel has to change. Same PR that creates the volume.

use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{perspective_vk, Mat4, Vec3, Vec4};
use harpia_render::{
    color_desc, depth_desc, OccupancyVolume, PushConstants, SphereMesh, GBUFFER_DEPTH_FORMAT,
    OCCUPANCY_SLOT, VERTEX_STRIDE,
};
use harpia_rhi::{
    Buffer, Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const HDR: Format = Format::Rgba16Float;
const HDR_ONLY: [Format; 1] = [HDR];
const FOV_Y: f32 = 50.0;
const NEAR: f32 = 0.3;
const FAR: f32 = 40.0;

#[repr(C)]
#[derive(Clone, Copy)]
struct LitCb {
    camera_pos: Vec4,
    sun_dir: Vec4,
    albedo: Vec4,
    origin_extent: Vec4,
    enable: u32,
    _pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BlitCb {
    inv_extent: [f32; 2],
    src: u32,
    _pad: u32,
}

struct Targets {
    on: Texture,
    off: Texture,
    depth: Texture,
}

struct OccupancyGate {
    lit_pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    plane_vb: Option<Buffer>,
    plane_ib: Option<Buffer>,
    plane_idx: u32,
    volume: Option<Texture>,
    origin_extent: Vec4,
    rt: Option<Targets>,
    extent: Extent2D,
    compared: bool,
}

impl Default for OccupancyGate {
    fn default() -> Self {
        Self {
            lit_pso: None,
            blit_pso: None,
            plane_vb: None,
            plane_ib: None,
            plane_idx: 0,
            volume: None,
            origin_extent: Vec4::ZERO,
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

impl OccupancyGate {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.rt = Some(Targets {
            on: gpu.create_texture(&color_desc(w, h, HDR))?,
            off: gpu.create_texture(&color_desc(w, h, HDR))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
        });
        self.extent = Extent2D {
            width: w,
            height: h,
        };
        Ok(())
    }
}

impl Sample for OccupancyGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        let mesh = SphereMesh::plane_xz();
        self.plane_idx = mesh.indices.len() as u32;
        self.plane_vb = Some(gpu.create_vertex_buffer(mesh.vertex_bytes())?);
        self.plane_ib = Some(gpu.create_index_buffer(mesh.index_bytes())?);

        let origin = Vec3::new(-4.0, 0.0, -4.0);
        let extent = 8.0f32;
        let mut vol = OccupancyVolume::new(origin, Vec3::splat(extent));
        vol.stamp_aabb(Vec3::new(-0.7, 0.0, -0.7), Vec3::new(0.7, 2.5, 0.7));
        let tex = gpu.create_texture(&OccupancyVolume::desc())?;
        gpu.upload_texture_mip(tex, 0, &vol.voxels)?;
        gpu.bind_volume_srv(OCCUPANCY_SLOT, tex)?;
        self.volume = Some(tex);
        self.origin_extent = Vec4::new(origin.x, origin.y, origin.z, extent);

        self.lit_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/room.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/lit.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &HDR_ONLY,
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE,
                    depth_test: true,
                    cull_back: false,
                    ..Default::default()
                },
            })
            .context("occupancy lit PSO")?,
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
            .context("blit PSO")?,
        );
        Ok(())
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if self.extent != info.extent {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let eye = Vec3::new(0.0, 3.4, 7.2);
        let view = Mat4::look_at_rh(eye, Vec3::new(0.0, 0.4, 0.0), Vec3::Y);
        let proj = perspective_vk(FOV_Y.to_radians(), w / h, NEAR, FAR);
        let view_proj = proj * view;

        let draw = |gpu: &mut Gpu,
                    pso: &GraphicsPipeline,
                    view_proj: Mat4,
                    vb: Buffer,
                    ib: Buffer,
                    idx: u32|
         -> Result<()> {
            gpu.set_pipeline(pso)?;
            gpu.bind_graphics_bindless()?;
            gpu.bind_vertex_buffer(vb, 0)?;
            gpu.bind_index_buffer(ib)?;
            let parts = [
                plane_world(Vec3::new(0.0, 0.0, 0.0), Vec3::Y, Vec3::X, 4.0, 4.0),
                plane_world(Vec3::new(0.0, 1.25, 0.0), Vec3::Y, Vec3::X, 0.7, 0.7),
                plane_world(Vec3::new(0.7, 1.25, 0.0), Vec3::X, Vec3::Y, 1.25, 0.7),
                plane_world(Vec3::new(-0.7, 1.25, 0.0), -Vec3::X, Vec3::Y, 1.25, 0.7),
                plane_world(Vec3::new(0.0, 1.25, 0.7), Vec3::Z, Vec3::Y, 1.25, 0.7),
                plane_world(Vec3::new(0.0, 1.25, -0.7), -Vec3::Z, Vec3::Y, 1.25, 0.7),
            ];
            for world in parts {
                gpu.set_push_constants(PushConstants::with_world(view_proj, world).as_bytes())?;
                gpu.draw_indexed(idx, 1, 0, 0, 0)?;
            }
            Ok(())
        };

        let mut cb = LitCb {
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
            sun_dir: Vec4::new(0.4, -0.85, 0.3, 0.0),
            albedo: Vec4::new(0.78, 0.74, 0.68, 1.0),
            origin_extent: self.origin_extent,
            enable: 1,
            _pad: [0; 3],
        };
        gpu.write_frame_bytes(as_bytes(&cb))?;
        gpu.begin_color_pass(&[rt.on], Some(rt.depth), &[[0.05, 0.06, 0.08, 1.0]], Some(1.0))?;
        draw(
            gpu,
            self.lit_pso.as_ref().context("lit")?,
            view_proj,
            self.plane_vb.context("vb")?,
            self.plane_ib.context("ib")?,
            self.plane_idx,
        )?;
        gpu.end_color_pass()?;

        cb.enable = 0;
        gpu.write_frame_bytes(as_bytes(&cb))?;
        gpu.begin_color_pass(&[rt.off], Some(rt.depth), &[[0.05, 0.06, 0.08, 1.0]], Some(1.0))?;
        draw(
            gpu,
            self.lit_pso.as_ref().context("lit")?,
            view_proj,
            self.plane_vb.context("vb")?,
            self.plane_ib.context("ib")?,
            self.plane_idx,
        )?;
        gpu.end_color_pass()?;
        gpu.mark("occupancy");

        gpu.write_frame_bytes(as_bytes(&BlitCb {
            inv_extent: [1.0 / w, 1.0 / h],
            src: gpu.bindless_index(rt.on)?,
            _pad: 0,
        }))?;
        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0])?;
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
            "occupancy no lighting contra volume ignorado"
        );
        anyhow::ensure!(
            pct > 1.5,
            "occupancy mudou só {pct:.3}% dos canais: o volume não está no lighting"
        );
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.rt
            .as_ref()
            .map(|rt| vec![("on", rt.on), ("off", rt.off)])
            .unwrap_or_default()
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — occupancy".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, OccupancyGate::default())
}
