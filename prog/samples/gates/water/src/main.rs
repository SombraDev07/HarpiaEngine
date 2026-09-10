//! Fase 5, water. Gerstner waves on a grid, Fresnel against the sky, and a body
//! colour from Beer-Lambert down to the seabed.
//!
//! One color pass draws the sky (fullscreen, no depth) and then the surface over
//! it (depth tested). Both write linear radiance into an HDR target; a fullscreen
//! pass tonemaps once at the end. Splitting the sky into its own pass would need
//! the RHI to load an attachment instead of clearing it, and there is no reason
//! to pay for that here.

use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{Vec2, Vec3, Vec4};
use harpia_render::{
    color_desc, depth_desc, FlyCamera, PushConstants, SphereMesh, WaterCb, GBUFFER_DEPTH_FORMAT,
    VERTEX_STRIDE, WATER_GRID, WATER_HALF,
};
use harpia_rhi::{
    Buffer, Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const HDR: Format = Format::Rgba16Float;
const HDR_FORMATS: [Format; 1] = [HDR];
const FOV_Y: f32 = 55.0;

struct Targets {
    hdr: Texture,
    depth: Texture,
}

struct WaterGate {
    sky_pso: Option<GraphicsPipeline>,
    water_pso: Option<GraphicsPipeline>,
    tonemap_pso: Option<GraphicsPipeline>,
    vb: Option<Buffer>,
    ib: Option<Buffer>,
    index_count: u32,
    rt: Option<Targets>,
    extent: Extent2D,
    cam: FlyCamera,
}

impl Default for WaterGate {
    fn default() -> Self {
        Self {
            sky_pso: None,
            water_pso: None,
            tonemap_pso: None,
            vb: None,
            ib: None,
            index_count: 0,
            rt: None,
            extent: Extent2D { width: 0, height: 0 },
            // Low over the surface: at a grazing angle Fresnel is near 1 and the
            // reflection carries the image, which is the case worth looking at.
            cam: FlyCamera {
                speed: 12.0,
                fov_y: FOV_Y.to_radians(),
                near: 0.2,
                far: 400.0,
                ..FlyCamera::looking_at(
                    Vec3::new(0.0, 2.4, 26.0),
                    Vec3::new(0.0, 2.3, 25.0),
                )
            },
        }
    }
}

impl WaterGate {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.rt = Some(Targets {
            hdr: gpu.create_texture(&color_desc(w, h, HDR))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
        });
        self.extent = Extent2D { width: w, height: h };
        Ok(())
    }
}

fn fullscreen(
    gpu: &mut Gpu,
    fs: &'static [u8],
    formats: &'static [Format],
    depth: bool,
) -> Result<GraphicsPipeline> {
    gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
        vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
        fs_spirv: fs,
        vs_entry: "VSMain",
        fs_entry: "PSMain",
        bindless: true,
        targets: PipelineTargets {
            color_formats: formats,
            // The sky shares the pass with the surface, so it needs the same
            // depth attachment declared -- it just never tests against it.
            depth_format: depth.then_some(GBUFFER_DEPTH_FORMAT),
            ..Default::default()
        },
    })
    .map_err(Into::into)
}

impl Sample for WaterGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        let grid = SphereMesh::grid_xz(WATER_GRID, WATER_HALF);
        self.index_count = grid.indices.len() as u32;
        self.vb = Some(gpu.create_vertex_buffer(grid.vertex_bytes())?);
        self.ib = Some(gpu.create_index_buffer(grid.index_bytes())?);
        tracing::info!(
            verts = grid.vertices.len(),
            tris = self.index_count / 3,
            "water grid"
        );

        self.sky_pso = Some(
            fullscreen(
                gpu,
                include_bytes!(concat!(env!("OUT_DIR"), "/sky.ps.spv")),
                &HDR_FORMATS,
                true,
            )
            .context("sky PSO")?,
        );
        self.tonemap_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/tonemap.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets::default(),
            })
            .context("tonemap PSO")?,
        );
        self.water_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/water.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/water.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &HDR_FORMATS,
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE,
                    depth_test: true,
                    // Gerstner can fold a crest past vertical; culling would punch
                    // holes in the surface exactly where the wave is steepest.
                    cull_back: false,
                    ..Default::default()
                },
            })
            .context("water PSO")?,
        );
        self.recreate(gpu, gpu.extent())?;
        Ok(())
    }

    fn update(&mut self, input: &harpia_app::SampleInput, dt: f32) {
        self.cam.update(input, dt);
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.rt.as_ref().map_or_else(Vec::new, |rt| vec![("hdr", rt.hdr)])
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let sky_pso = self.sky_pso.as_ref().context("sky pso")?;
        let water_pso = self.water_pso.as_ref().context("water pso")?;
        let tonemap_pso = self.tonemap_pso.as_ref().context("tonemap pso")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let camera = self.cam.camera(w / h);
        let eye = camera.eye;
        let view_proj = camera.view_proj();
        let sun = Vec3::new(0.16, 0.20, -0.97).normalize();
        let t = info.frame_index as f32 * 0.05;

        let mut cb = WaterCb {
            inv_view_proj: view_proj.inverse(),
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            inv_extent: Vec2::new(1.0 / w, 1.0 / h),
            ..Default::default()
        };
        cb.misc.x = t;
        gpu.write_frame_bytes(cb.as_bytes())?;

        // 1. sky, then the surface over it. Same pass, so the sky does not have
        //    to survive an attachment clear.
        gpu.begin_color_pass(
            &[rt.hdr],
            Some(rt.depth),
            &[[0.0, 0.0, 0.0, 1.0]],
            Some(1.0),
        )?;
        gpu.set_pipeline(sky_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;

        gpu.set_pipeline(water_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.set_push_constants(PushConstants::new(view_proj).as_bytes())?;
        gpu.bind_vertex_buffer(self.vb.context("vb")?, 0)?;
        gpu.bind_index_buffer(self.ib.context("ib")?)?;
        gpu.draw_indexed(self.index_count, 1, 0, 0, 0)?;
        gpu.end_color_pass()?;

        // 2. one tonemap for both.
        let mut tm = cb;
        tm.hdr = gpu.bindless_index(rt.hdr)?;
        gpu.write_frame_bytes(tm.as_bytes())?;
        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0])?;
        gpu.set_pipeline(tonemap_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — water".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, WaterGate::default())
}
