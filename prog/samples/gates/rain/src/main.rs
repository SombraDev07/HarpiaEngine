//! Fase 5, rain — the last weather pass, and the one the roadmap calls the most
//! dangerous.
//!
//! Two passes. The scene renders a wet material into linear HDR plus view depth;
//! a fullscreen pass lays streaks over it and tonemaps once. The streaks need the
//! depth so a drop in front of a surface a metre away does not draw inside it.

use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{Mat4, Quat, Vec2, Vec3, Vec4};
use harpia_render::{
    color_desc, depth_desc, FlyCamera, PushConstants, RainCb, SphereMesh, GBUFFER_DEPTH_FORMAT,
    VERTEX_STRIDE,
};
use harpia_rhi::{
    Buffer, Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const HDR: Format = Format::Rgba16Float;
const VIEW_DEPTH: Format = Format::R32Float;
const SCENE_FORMATS: [Format; 2] = [HDR, VIEW_DEPTH];
/// Linear: rain.ps writes radiance, the blit encodes.
const COMPOSITE: Format = Format::Rgba16Float;
const COMPOSITE_FORMATS: [Format; 1] = [COMPOSITE];
const FOV_Y: f32 = 55.0;
const GROUND_HALF: f32 = 60.0;

/// Blocks standing on the wet ground: something for the ripples to sit beside and
/// for the streaks to fall in front of.
const BLOCKS: [(f32, f32, f32); 7] = [
    (-6.0, -9.0, 1.5),
    (4.5, -13.0, 2.1),
    (-2.0, -20.0, 1.2),
    (9.0, -24.0, 2.6),
    (-11.0, -28.0, 2.0),
    (1.0, -34.0, 3.0),
    (-19.0, -40.0, 2.4),
];

struct Targets {
    hdr: Texture,
    view_depth: Texture,
    depth: Texture,
    /// The streaks land here rather than straight on the swapchain: `--capture`
    /// cannot read the swapchain, and a capture without the streaks cannot judge
    /// this gate.
    composite: Texture,
}

struct RainGate {
    scene_pso: Option<GraphicsPipeline>,
    rain_pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    ground_vb: Option<Buffer>,
    ground_ib: Option<Buffer>,
    ground_idx: u32,
    block_vb: Option<Buffer>,
    block_ib: Option<Buffer>,
    block_idx: u32,
    rt: Option<Targets>,
    extent: Extent2D,
    cam: FlyCamera,
}

impl Default for RainGate {
    fn default() -> Self {
        Self {
            scene_pso: None,
            rain_pso: None,
            blit_pso: None,
            ground_vb: None,
            ground_ib: None,
            ground_idx: 0,
            block_vb: None,
            block_ib: None,
            block_idx: 0,
            rt: None,
            extent: Extent2D { width: 0, height: 0 },
            cam: FlyCamera {
                speed: 8.0,
                fov_y: FOV_Y.to_radians(),
                near: 0.2,
                far: 300.0,
                ..FlyCamera::looking_at(Vec3::new(0.0, 3.4, 8.0), Vec3::new(0.0, 3.15, 7.0))
            },
        }
    }
}

impl RainGate {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.rt = Some(Targets {
            hdr: gpu.create_texture(&color_desc(w, h, HDR))?,
            view_depth: gpu.create_texture(&color_desc(w, h, VIEW_DEPTH))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
            composite: gpu.create_texture(&color_desc(w, h, COMPOSITE))?,
        });
        self.extent = Extent2D { width: w, height: h };
        Ok(())
    }
}

impl Sample for RainGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        // A tessellated ground, not a quad: the ripples are shaded per pixel, but
        // a coarse grid still helps the specular stay stable across it.
        let ground = SphereMesh::grid_xz(48, GROUND_HALF);
        self.ground_idx = ground.indices.len() as u32;
        self.ground_vb = Some(gpu.create_vertex_buffer(ground.vertex_bytes())?);
        self.ground_ib = Some(gpu.create_index_buffer(ground.index_bytes())?);
        let block = SphereMesh::uv(20, 14);
        self.block_idx = block.indices.len() as u32;
        self.block_vb = Some(gpu.create_vertex_buffer(block.vertex_bytes())?);
        self.block_ib = Some(gpu.create_index_buffer(block.index_bytes())?);

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
                    depth_test: true,
                    cull_back: true,
                    ..Default::default()
                },
            })
            .context("scene PSO")?,
        );
        self.rain_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/rain.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &COMPOSITE_FORMATS,
                    ..Default::default()
                },
            })
            .context("rain PSO")?,
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
        self.recreate(gpu, gpu.extent())?;
        Ok(())
    }

    fn update(&mut self, input: &harpia_app::SampleInput, dt: f32) {
        self.cam.update(input, dt);
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.rt
            .as_ref()
            .map_or_else(Vec::new, |rt| vec![("composite", rt.composite), ("scene", rt.hdr)])
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let scene_pso = self.scene_pso.as_ref().context("scene pso")?;
        let rain_pso = self.rain_pso.as_ref().context("rain pso")?;
        let blit_pso = self.blit_pso.as_ref().context("blit pso")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let camera = self.cam.camera(w / h);
        let view_proj = camera.view_proj();
        let sun = Vec3::new(0.30, 0.52, -0.80).normalize();
        let t = info.frame_index as f32 / 60.0;

        let mut cb = RainCb {
            inv_view_proj: view_proj.inverse(),
            view_proj,
            camera_pos: Vec4::new(camera.eye.x, camera.eye.y, camera.eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            inv_extent: Vec2::new(1.0 / w, 1.0 / h),
            ..Default::default()
        };
        cb.rain.w = t;
        gpu.write_frame_bytes(cb.as_bytes())?;

        // 1. wet scene into HDR + view depth. The depth target clears past the far
        //    plane so the sky never reads as a near surface.
        gpu.begin_color_pass(
            &[rt.hdr, rt.view_depth],
            Some(rt.depth),
            &[[0.40, 0.42, 0.46, 1.0], [1.0e9, 0.0, 0.0, 0.0]],
            Some(1.0),
        )?;
        gpu.set_pipeline(scene_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.bind_vertex_buffer(self.ground_vb.context("ground vb")?, 0)?;
        gpu.bind_index_buffer(self.ground_ib.context("ground ib")?)?;
        gpu.set_push_constants(PushConstants::new(view_proj).as_bytes())?;
        gpu.draw_indexed(self.ground_idx, 1, 0, 0, 0)?;

        gpu.bind_vertex_buffer(self.block_vb.context("block vb")?, 0)?;
        gpu.bind_index_buffer(self.block_ib.context("block ib")?)?;
        for (x, z, r) in BLOCKS {
            let world = Mat4::from_scale_rotation_translation(
                Vec3::splat(r),
                Quat::IDENTITY,
                Vec3::new(x, r * 0.65, z),
            );
            gpu.set_push_constants(PushConstants::with_world(view_proj, world).as_bytes())?;
            gpu.draw_indexed(self.block_idx, 1, 0, 0, 0)?;
        }
        gpu.end_color_pass()?;

        // 2. streaks over it, then one tonemap.
        let mut post = cb;
        post.scene_color = gpu.bindless_index(rt.hdr)?;
        post.scene_depth = gpu.bindless_index(rt.view_depth)?;
        gpu.write_frame_bytes(post.as_bytes())?;
        gpu.begin_color_pass(&[rt.composite], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(rain_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        // 3. present.
        let mut blit = cb;
        blit.scene_color = gpu.bindless_index(rt.composite)?;
        gpu.write_frame_bytes(blit.as_bytes())?;
        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0])?;
        gpu.set_pipeline(blit_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — rain".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, RainGate::default())
}
