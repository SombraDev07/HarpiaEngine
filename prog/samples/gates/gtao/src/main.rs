//! Fase 7, gate `gtao`: Jimenez GTAO on a corner. Dual RT so the pixel must change.

use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{perspective_vk, Mat4, Vec3, Vec4};
use harpia_render::{
    color_desc, depth_desc, PushConstants, SphereMesh, GBUFFER_DEPTH_FORMAT, VERTEX_STRIDE,
};
use harpia_rhi::{
    Buffer, Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const HDR: Format = Format::Rgba16Float;
const SCENE: [Format; 3] = [HDR, Format::Rgba8Unorm, Format::R32Float];
const LDR: [Format; 1] = [Format::Rgba8Unorm];
const FOV_Y: f32 = 55.0;
const NEAR: f32 = 0.3;
const FAR: f32 = 40.0;

#[repr(C)]
#[derive(Clone, Copy)]
struct SceneCb {
    camera_pos: Vec4,
    sun_dir: Vec4,
    albedo: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GtaoCb {
    inv_view_proj: Mat4,
    inv_proj: Mat4,
    view: Mat4,
    screen: Vec4,
    color: u32,
    depth: u32,
    enable: u32,
    _pad0: u32,
    radius: Vec4,
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
    on: Texture,
    off: Texture,
}

struct GtaoGate {
    room_pso: Option<GraphicsPipeline>,
    gtao_pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    plane_vb: Option<Buffer>,
    plane_ib: Option<Buffer>,
    plane_idx: u32,
    rt: Option<Targets>,
    extent: Extent2D,
    compared: bool,
}

impl Default for GtaoGate {
    fn default() -> Self {
        Self {
            room_pso: None,
            gtao_pso: None,
            blit_pso: None,
            plane_vb: None,
            plane_ib: None,
            plane_idx: 0,
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

impl GtaoGate {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.rt = Some(Targets {
            color: gpu.create_texture(&color_desc(w, h, HDR))?,
            normal: gpu.create_texture(&color_desc(w, h, Format::Rgba8Unorm))?,
            depth_color: gpu.create_texture(&color_desc(w, h, Format::R32Float))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
            on: gpu.create_texture(&color_desc(w, h, Format::Rgba8Unorm))?,
            off: gpu.create_texture(&color_desc(w, h, Format::Rgba8Unorm))?,
        });
        self.extent = Extent2D {
            width: w,
            height: h,
        };
        Ok(())
    }
}

impl Sample for GtaoGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        let mesh = SphereMesh::plane_xz();
        self.plane_idx = mesh.indices.len() as u32;
        self.plane_vb = Some(gpu.create_vertex_buffer(mesh.vertex_bytes())?);
        self.plane_ib = Some(gpu.create_index_buffer(mesh.index_bytes())?);
        self.room_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/room.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/room.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &SCENE,
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE,
                    depth_test: true,
                    cull_back: false,
                    ..Default::default()
                },
            })
            .context("gtao room PSO")?,
        );
        self.gtao_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/gtao.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &LDR,
                    ..Default::default()
                },
            })
            .context("gtao PSO")?,
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
        let aspect = w / h;
        let eye = Vec3::new(3.2, 1.6, 3.2);
        let view = Mat4::look_at_rh(eye, Vec3::new(0.0, 0.4, 0.0), Vec3::Y);
        let proj = perspective_vk(FOV_Y.to_radians(), aspect, NEAR, FAR);
        let view_proj = proj * view;

        gpu.write_frame_bytes(as_bytes(&SceneCb {
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
            sun_dir: Vec4::new(0.35, -0.75, 0.45, 0.0),
            albedo: Vec4::new(0.72, 0.70, 0.66, 1.0),
        }))?;
        gpu.begin_color_pass(
            &[rt.color, rt.normal, rt.depth_color],
            Some(rt.depth),
            &[
                [0.15, 0.16, 0.18, 1.0],
                [0.5, 0.5, 1.0, 1.0],
                [1.0, 0.0, 0.0, 1.0],
            ],
            Some(1.0),
        )?;
        gpu.set_pipeline(self.room_pso.as_ref().context("room")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.bind_vertex_buffer(self.plane_vb.context("vb")?, 0)?;
        gpu.bind_index_buffer(self.plane_ib.context("ib")?)?;
        let walls = [
            plane_world(Vec3::new(0.0, 0.0, 0.0), Vec3::Y, Vec3::X, 4.0, 4.0),
            plane_world(Vec3::new(-4.0, 2.0, 0.0), Vec3::X, Vec3::Z, 2.0, 4.0),
            plane_world(Vec3::new(0.0, 2.0, -4.0), Vec3::Z, Vec3::X, 4.0, 2.0),
        ];
        for world in walls {
            gpu.set_push_constants(PushConstants::with_world(view_proj, world).as_bytes())?;
            gpu.draw_indexed(self.plane_idx, 1, 0, 0, 0)?;
        }
        gpu.end_color_pass()?;
        gpu.mark("scene");

        let gtao = GtaoCb {
            inv_view_proj: view_proj.inverse(),
            inv_proj: proj.inverse(),
            view,
            screen: Vec4::new(w, h, 1.0 / w, 1.0 / h),
            color: gpu.bindless_index(rt.color)?,
            depth: gpu.bindless_index(rt.depth_color)?,
            enable: 1,
            _pad0: 0,
            radius: Vec4::new(1.6, 0.0, 0.0, 0.0),
        };
        gpu.write_frame_bytes(as_bytes(&gtao))?;
        gpu.begin_color_pass(&[rt.on], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(self.gtao_pso.as_ref().context("gtao")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        let mut off = gtao;
        off.enable = 0;
        gpu.write_frame_bytes(as_bytes(&off))?;
        gpu.begin_color_pass(&[rt.off], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(self.gtao_pso.as_ref().context("gtao")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;
        gpu.mark("gtao");

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
            "gtao on contra off"
        );
        anyhow::ensure!(
            pct > 2.0,
            "GTAO mudou só {pct:.3}% dos canais: o pass não está no pixel"
        );
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.rt
            .as_ref()
            .map(|rt| vec![("on", rt.on), ("off", rt.off), ("color", rt.color)])
            .unwrap_or_default()
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — gtao".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, GtaoGate::default())
}
