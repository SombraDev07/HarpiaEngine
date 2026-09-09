use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{Mat4, Vec2, Vec3};
use harpia_render::{
    color_desc, depth_desc, Camera, MaterialGpu, PushConstants, SphereInstance, SphereMesh, TaaCb,
    GBUFFER_DEPTH_FORMAT, INSTANCE_STRIDE, TAA_BLEND, VERTEX_STRIDE,
};
use harpia_rhi::{
    Buffer, Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const HDR: Format = Format::Rgba16Float;
const MOTION: Format = Format::Rg16Float;
const FORWARD_FORMATS: [Format; 2] = [HDR, MOTION];

struct Targets {
    color: Texture,
    motion: Texture,
    depth: Texture,
    hist_a: Texture,
    hist_b: Texture,
}

struct TaaGate {
    fwd_pso: Option<GraphicsPipeline>,
    taa_pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    plane_vb: Option<Buffer>,
    plane_ib: Option<Buffer>,
    plane_inst: Option<Buffer>,
    plane_idx: u32,
    sph_vb: Option<Buffer>,
    sph_ib: Option<Buffer>,
    sph_inst: Option<Buffer>,
    sph_idx: u32,
    sph_count: u32,
    rt: Option<Targets>,
    extent: Extent2D,
    prev_view_proj: Mat4,
    prev_object_x: f32,
    hist_odd: bool,
    history_valid: bool,
}

impl Default for TaaGate {
    fn default() -> Self {
        Self {
            fwd_pso: None,
            taa_pso: None,
            blit_pso: None,
            plane_vb: None,
            plane_ib: None,
            plane_inst: None,
            plane_idx: 0,
            sph_vb: None,
            sph_ib: None,
            sph_inst: None,
            sph_idx: 0,
            sph_count: 0,
            rt: None,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
            prev_view_proj: Mat4::IDENTITY,
            prev_object_x: 0.0,
            hist_odd: false,
            history_valid: false,
        }
    }
}

impl TaaGate {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.rt = Some(Targets {
            color: gpu.create_texture(&color_desc(w, h, HDR))?,
            motion: gpu.create_texture(&color_desc(w, h, MOTION))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
            hist_a: gpu.create_texture(&color_desc(w, h, HDR))?,
            hist_b: gpu.create_texture(&color_desc(w, h, HDR))?,
        });
        self.extent = Extent2D { width: w, height: h };
        self.history_valid = false;
        Ok(())
    }
}

impl Sample for TaaGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.fwd_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/forward.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/forward.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &FORWARD_FORMATS,
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE,
                    instance_stride: INSTANCE_STRIDE,
                    depth_test: true,
                    cull_back: true,
                    ..Default::default()
                },
            })
            .context("forward PSO")?,
        );
        self.taa_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/taa.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &[HDR],
                    depth_format: None,
                    ..Default::default()
                },
            })
            .context("taa PSO")?,
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

        let plane = SphereMesh::plane_xz();
        self.plane_idx = plane.indices.len() as u32;
        self.plane_vb = Some(gpu.create_vertex_buffer(plane.vertex_bytes())?);
        self.plane_ib = Some(gpu.create_index_buffer(plane.index_bytes())?);
        let mut ground = MaterialGpu::default();
        ground.base_color = [0.55, 0.54, 0.50];
        let plane_i = [SphereInstance::from_material([0.0, 0.0, 0.0], 14.0, &ground)];
        self.plane_inst = Some(gpu.create_vertex_buffer(instance_bytes(&plane_i))?);

        let sph = SphereMesh::uv(20, 14);
        self.sph_idx = sph.indices.len() as u32;
        self.sph_vb = Some(gpu.create_vertex_buffer(sph.vertex_bytes())?);
        self.sph_ib = Some(gpu.create_index_buffer(sph.index_bytes())?);
        let mut red = MaterialGpu::default();
        red.base_color = [0.85, 0.2, 0.15];
        let mut white = MaterialGpu::default();
        white.base_color = [0.9, 0.9, 0.88];
        let spheres = [
            SphereInstance::from_material([0.0, 1.2, 0.0], 1.2, &white),
            SphereInstance::from_material([2.8, 0.7, -1.0], 0.7, &red),
        ];
        self.sph_count = spheres.len() as u32;
        self.sph_inst = Some(gpu.create_vertex_buffer(instance_bytes(&spheres))?);

        self.recreate(gpu, gpu.extent())?;
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        let mut out = Vec::new();
        if let Some(rt) = self.rt.as_ref() {
            out.push(("color", rt.color));
            out.push(("motion", rt.motion));
            // The TAA output of the frame just drawn.
            out.push(("resolved", if self.hist_odd { rt.hist_b } else { rt.hist_a }));
        }
        out
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let fwd = self.fwd_pso.as_ref().context("fwd")?;
        let taa = self.taa_pso.as_ref().context("taa")?;
        let blit = self.blit_pso.as_ref().context("blit")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let t = info.frame_index as f32 * 0.11;
        let camera = Camera {
            eye: Vec3::new(t.sin() * 9.0, 5.5, t.cos() * 13.0),
            target: Vec3::new(0.0, 1.0, 0.0),
            up: Vec3::Y,
            fov_y: 50.0_f32.to_radians(),
            aspect: w / h,
            near: 0.4,
            far: 60.0,
        };
        let view_proj = camera.view_proj();
        let object_x = (info.frame_index as f32 * 0.22).sin() * 2.4;
        let (hist_src, hist_dst) = if self.hist_odd {
            (rt.hist_b, rt.hist_a)
        } else {
            (rt.hist_a, rt.hist_b)
        };

        let mut cb = TaaCb {
            prev_view_proj: self.prev_view_proj,
            inv_extent: Vec2::new(1.0 / w, 1.0 / h),
            object_x,
            prev_object_x: self.prev_object_x,
            color_idx: gpu.bindless_index(rt.color)?,
            history_idx: gpu.bindless_index(hist_src)?,
            motion_idx: gpu.bindless_index(rt.motion)?,
            history_valid: u32::from(self.history_valid),
            blend: TAA_BLEND,
            ..Default::default()
        };
        gpu.write_frame_bytes(cb.as_bytes())?;

        gpu.begin_color_pass(
            &[rt.color, rt.motion],
            Some(rt.depth),
            &[[0.35, 0.48, 0.68, 1.0], [0.0, 0.0, 0.0, 0.0]],
            Some(1.0),
        )?;
        gpu.set_pipeline(fwd)?;
        gpu.bind_graphics_bindless()?;
        gpu.set_push_constants(PushConstants::new(view_proj).as_bytes())?;
        gpu.bind_vertex_buffer(self.plane_vb.context("pvb")?, 0)?;
        gpu.bind_vertex_buffer(self.plane_inst.context("pinst")?, 1)?;
        gpu.bind_index_buffer(self.plane_ib.context("pib")?)?;
        gpu.draw_indexed(self.plane_idx, 1, 0, 0, 0)?;
        gpu.bind_vertex_buffer(self.sph_vb.context("svb")?, 0)?;
        gpu.bind_vertex_buffer(self.sph_inst.context("sinst")?, 1)?;
        gpu.bind_index_buffer(self.sph_ib.context("sib")?)?;
        gpu.draw_indexed(self.sph_idx, self.sph_count, 0, 0, 0)?;
        gpu.end_color_pass()?;

        gpu.begin_color_pass(&[hist_dst], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(taa)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        cb.color_idx = gpu.bindless_index(hist_dst)?;
        gpu.write_frame_bytes(cb.as_bytes())?;
        gpu.begin_swapchain_pass([0.02, 0.03, 0.05, 1.0])?;
        gpu.set_pipeline(blit)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;

        self.prev_view_proj = view_proj;
        self.prev_object_x = object_x;
        self.hist_odd = !self.hist_odd;
        self.history_valid = true;
        Ok(())
    }
}

fn instance_bytes(instances: &[SphereInstance]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            instances.as_ptr().cast::<u8>(),
            std::mem::size_of_val(instances),
        )
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — taa".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, TaaGate::default())
}
