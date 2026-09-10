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
use harpia_math::{Mat4, Vec2, Vec3, Vec4};
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
const VIEW_DEPTH: Format = Format::R32Float;
/// The opaque pass writes colour and the positive view depth; the SSR marches
/// against that depth and samples that colour.
const OPAQUE_FORMATS: [Format; 2] = [HDR, VIEW_DEPTH];
const FOV_Y: f32 = 55.0;

struct Targets {
    /// Sky and rocks, linear HDR.
    opaque: Texture,
    view_depth: Texture,
    depth: Texture,
    /// The opaque image with the water composited over it. Separate, because the
    /// water samples what it is reflecting and cannot also be writing to it.
    hdr: Texture,
}

struct WaterGate {
    sky_pso: Option<GraphicsPipeline>,
    water_pso: Option<GraphicsPipeline>,
    tonemap_pso: Option<GraphicsPipeline>,
    copy_pso: Option<GraphicsPipeline>,
    opaque_pso: Option<GraphicsPipeline>,
    vb: Option<Buffer>,
    ib: Option<Buffer>,
    index_count: u32,
    rock_vb: Option<Buffer>,
    rock_ib: Option<Buffer>,
    rock_idx: u32,
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
            copy_pso: None,
            opaque_pso: None,
            vb: None,
            ib: None,
            index_count: 0,
            rock_vb: None,
            rock_ib: None,
            rock_idx: 0,
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
            opaque: gpu.create_texture(&color_desc(w, h, HDR))?,
            view_depth: gpu.create_texture(&color_desc(w, h, VIEW_DEPTH))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
            hdr: gpu.create_texture(&color_desc(w, h, HDR))?,
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

        let rock = SphereMesh::uv(24, 16);
        self.rock_idx = rock.indices.len() as u32;
        self.rock_vb = Some(gpu.create_vertex_buffer(rock.vertex_bytes())?);
        self.rock_ib = Some(gpu.create_index_buffer(rock.index_bytes())?);

        self.sky_pso = Some(
            fullscreen(
                gpu,
                include_bytes!(concat!(env!("OUT_DIR"), "/sky.ps.spv")),
                &OPAQUE_FORMATS,
                true,
            )
            .context("sky PSO")?,
        );
        self.copy_pso = Some(
            fullscreen(
                gpu,
                include_bytes!(concat!(env!("OUT_DIR"), "/copy.ps.spv")),
                &HDR_FORMATS,
                true,
            )
            .context("copy PSO")?,
        );
        self.opaque_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/opaque.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/opaque.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &OPAQUE_FORMATS,
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE,
                    depth_test: true,
                    cull_back: true,
                    ..Default::default()
                },
            })
            .context("opaque PSO")?,
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
        self.rt.as_ref().map_or_else(Vec::new, |rt| vec![("hdr", rt.hdr), ("opaque", rt.opaque)])
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let sky_pso = self.sky_pso.as_ref().context("sky pso")?;
        let water_pso = self.water_pso.as_ref().context("water pso")?;
        let tonemap_pso = self.tonemap_pso.as_ref().context("tonemap pso")?;
        let copy_pso = self.copy_pso.as_ref().context("copy pso")?;
        let opaque_pso = self.opaque_pso.as_ref().context("opaque pso")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let camera = self.cam.camera(w / h);
        let eye = camera.eye;
        let view_proj = camera.view_proj();
        let sun = Vec3::new(0.16, 0.20, -0.97).normalize();
        let t = info.frame_index as f32 * 0.05;

        let mut cb = WaterCb {
            inv_view_proj: view_proj.inverse(),
            view_proj,
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            inv_extent: Vec2::new(1.0 / w, 1.0 / h),
            ..Default::default()
        };
        cb.misc.x = t;
        gpu.write_frame_bytes(cb.as_bytes())?;

        // 1. opaque: sky (no depth) then the rocks over it. The view-depth target
        //    clears past the far plane so the sky never reads as a near surface
        //    the SSR could hit.
        gpu.begin_color_pass(
            &[rt.opaque, rt.view_depth],
            Some(rt.depth),
            &[[0.0, 0.0, 0.0, 1.0], [1.0e9, 0.0, 0.0, 0.0]],
            Some(1.0),
        )?;
        gpu.set_pipeline(sky_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.set_pipeline(opaque_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.bind_vertex_buffer(self.rock_vb.context("rock vb")?, 0)?;
        gpu.bind_index_buffer(self.rock_ib.context("rock ib")?)?;
        for (x, z, r) in ROCKS {
            let world = Mat4::from_scale_rotation_translation(
                Vec3::splat(r),
                harpia_math::Quat::IDENTITY,
                Vec3::new(x, 0.0, z),
            );
            gpu.set_push_constants(PushConstants::with_world(view_proj, world).as_bytes())?;
            gpu.draw_indexed(self.rock_idx, 1, 0, 0, 0)?;
        }
        gpu.end_color_pass()?;

        // 2. water: copy the opaque image in, then draw the surface over it.
        //    The SSR samples that copy, which is why it cannot be one pass.
        let mut wcb = cb;
        wcb.scene_color = gpu.bindless_index(rt.opaque)?;
        wcb.scene_depth = gpu.bindless_index(rt.view_depth)?;
        gpu.write_frame_bytes(wcb.as_bytes())?;
        gpu.begin_color_pass(&[rt.hdr], Some(rt.depth), &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(copy_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.set_pipeline(water_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.set_push_constants(PushConstants::new(view_proj).as_bytes())?;
        gpu.bind_vertex_buffer(self.vb.context("vb")?, 0)?;
        gpu.bind_index_buffer(self.ib.context("ib")?)?;
        gpu.draw_indexed(self.index_count, 1, 0, 0, 0)?;
        gpu.end_color_pass()?;

        // 3. one tonemap for the lot.
        let mut tm = cb;
        tm.scene_color = gpu.bindless_index(rt.hdr)?;
        gpu.write_frame_bytes(tm.as_bytes())?;
        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0])?;
        gpu.set_pipeline(tonemap_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }
}

/// Rocks standing in the water: x, z, radius. Spread across the view so the
/// reflection has something to find at several distances.
const ROCKS: [(f32, f32, f32); 6] = [
    (-14.0, -6.0, 3.2),
    (9.0, -14.0, 4.4),
    (-4.0, -26.0, 5.0),
    (18.0, -30.0, 6.2),
    (-22.0, -38.0, 5.6),
    (2.0, -48.0, 7.0),
];

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
