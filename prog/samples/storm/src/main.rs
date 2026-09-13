//! Storm: water, rain and lights in one scene.
//!
//! Sponza stays a lighting map — this is the place that composes weather.
//! Staged wetness, puddles that grow, wind on the streaks, lanterns on wet
//! wood and on the water, and a depth rain map so the pier roof actually
//! keeps the deck dry. Screen-space streaks stay (they cover the frame);
//! particles would not.

use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{Mat4, Quat, Vec2, Vec3, Vec4};
use harpia_render::{
    color_desc, depth_desc, puddle_growth, rain_map_view_proj, shadow_atlas_desc, FlyCamera,
    Light, PushConstants, SphereMesh, StormCb, GBUFFER_DEPTH_FORMAT, RAIN_MAP_SIZE, VERTEX_STRIDE,
    WATER_GRID,
};
use harpia_rhi::{
    Buffer, Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const HDR: Format = Format::Rgba16Float;
const VIEW_DEPTH: Format = Format::R32Float;
const OPAQUE_FORMATS: [Format; 2] = [HDR, VIEW_DEPTH];
const HDR_FORMATS: [Format; 1] = [HDR];
const COMPOSITE: Format = Format::Rgba16Float;
const COMPOSITE_FORMATS: [Format; 1] = [COMPOSITE];
const FOV_Y: f32 = 55.0;
const WATER_HALF: f32 = 70.0;
const SLOT_LIGHTS: u32 = 0;
const MAX_LIGHTS: usize = 32;

const GROUND_MAT: Vec4 = Vec4::new(0.18, 0.17, 0.16, 0.72);
const WOOD_MAT: Vec4 = Vec4::new(0.42, 0.28, 0.16, 0.55);
const ROCK_MAT: Vec4 = Vec4::new(0.36, 0.34, 0.31, 0.82);
const ROOF_MAT: Vec4 = Vec4::new(0.11, 0.10, 0.10, 0.62);

/// Flattened boulders in the water, not billiard balls. (x, z, sx, sy, sz)
const ROCKS: [(f32, f32, f32, f32, f32); 5] = [
    (-16.0, -5.0, 3.4, 1.45, 2.7),
    (15.0, -9.0, 4.1, 1.65, 3.1),
    (-11.0, -16.0, 2.7, 1.15, 2.3),
    (19.0, -21.0, 3.3, 1.35, 2.6),
    (-20.0, -27.0, 3.9, 1.55, 3.0),
];

const POSTS: [(f32, f32); 8] = [
    (-5.2, 0.5),
    (5.2, 0.5),
    (-5.2, 5.5),
    (5.2, 5.5),
    (-5.2, 10.5),
    (5.2, 10.5),
    (-5.2, 13.8),
    (5.2, 13.8),
];

struct Targets {
    opaque: Texture,
    view_depth: Texture,
    depth: Texture,
    hdr: Texture,
    composite: Texture,
}

struct StormDemo {
    sky_pso: Option<GraphicsPipeline>,
    opaque_pso: Option<GraphicsPipeline>,
    water_pso: Option<GraphicsPipeline>,
    copy_pso: Option<GraphicsPipeline>,
    rain_pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    rain_map_pso: Option<GraphicsPipeline>,
    water_vb: Option<Buffer>,
    water_ib: Option<Buffer>,
    water_idx: u32,
    ground_vb: Option<Buffer>,
    ground_ib: Option<Buffer>,
    ground_idx: u32,
    plane_vb: Option<Buffer>,
    plane_ib: Option<Buffer>,
    plane_idx: u32,
    sphere_vb: Option<Buffer>,
    sphere_ib: Option<Buffer>,
    sphere_idx: u32,
    lights: Vec<Light>,
    light_buf: Option<Buffer>,
    rain_map: Option<Texture>,
    rt: Option<Targets>,
    extent: Extent2D,
    cam: FlyCamera,
    puddle: f32,
}

impl Default for StormDemo {
    fn default() -> Self {
        Self {
            sky_pso: None,
            opaque_pso: None,
            water_pso: None,
            copy_pso: None,
            rain_pso: None,
            blit_pso: None,
            rain_map_pso: None,
            water_vb: None,
            water_ib: None,
            water_idx: 0,
            ground_vb: None,
            ground_ib: None,
            ground_idx: 0,
            plane_vb: None,
            plane_ib: None,
            plane_idx: 0,
            sphere_vb: None,
            sphere_ib: None,
            sphere_idx: 0,
            lights: Vec::new(),
            light_buf: None,
            rain_map: None,
            rt: None,
            extent: Extent2D { width: 0, height: 0 },
            cam: FlyCamera {
                speed: 10.0,
                fov_y: FOV_Y.to_radians(),
                near: 0.2,
                far: 280.0,
                ..FlyCamera::looking_at(Vec3::new(12.0, 3.85, 20.5), Vec3::new(-1.5, 1.05, 3.0))
            },
            puddle: 0.22,
        }
    }
}

fn lanterns() -> Vec<Light> {
    let warm = Vec3::new(1.00, 0.68, 0.38);
    let mut out = Vec::new();
    for (i, z) in [1.0, 4.5, 8.0, 11.5].iter().copied().enumerate() {
        let side = if i % 2 == 0 { -4.6 } else { 4.6 };
        out.push(Light::point(Vec3::new(side, 3.15, z), 14.0, warm, 22.0));
        out.push(Light::point(Vec3::new(-side, 3.15, z + 1.6), 12.0, warm, 18.0));
    }
    // Spots off the deck, onto the water — so the spec on the waves is a light,
    // not just the sun.
    out.push(Light::spot(
        Vec3::new(-2.0, 3.6, 2.0),
        Vec3::new(-0.15, -0.55, -0.82),
        28.0,
        0.20,
        0.38,
        Vec3::new(1.0, 0.78, 0.50),
        40.0,
    ));
    out.push(Light::spot(
        Vec3::new(2.2, 3.6, 2.0),
        Vec3::new(0.18, -0.50, -0.84),
        28.0,
        0.20,
        0.38,
        Vec3::new(0.95, 0.82, 0.62),
        36.0,
    ));
    debug_assert!(out.len() <= MAX_LIGHTS);
    out
}

fn light_bytes(lights: &[Light]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            lights.as_ptr().cast::<u8>(),
            std::mem::size_of_val(lights),
        )
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
            depth_format: depth.then_some(GBUFFER_DEPTH_FORMAT),
            ..Default::default()
        },
    })
    .map_err(Into::into)
}

impl StormDemo {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.rt = Some(Targets {
            opaque: gpu.create_texture(&color_desc(w, h, HDR))?,
            view_depth: gpu.create_texture(&color_desc(w, h, VIEW_DEPTH))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
            hdr: gpu.create_texture(&color_desc(w, h, HDR))?,
            composite: gpu.create_texture(&color_desc(w, h, COMPOSITE))?,
        });
        self.extent = Extent2D { width: w, height: h };
        Ok(())
    }

    fn draw_mesh(
        gpu: &mut Gpu,
        pso: &GraphicsPipeline,
        view_proj: Mat4,
        world: Mat4,
        vb: Buffer,
        ib: Buffer,
        idx: u32,
    ) -> Result<()> {
        gpu.set_pipeline(pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.bind_vertex_buffer(vb, 0)?;
        gpu.bind_index_buffer(ib)?;
        gpu.set_push_constants(PushConstants::with_world(view_proj, world).as_bytes())?;
        gpu.draw_indexed(idx, 1, 0, 0, 0)?;
        Ok(())
    }
}

impl Sample for StormDemo {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        let water = SphereMesh::grid_xz(WATER_GRID, WATER_HALF);
        self.water_idx = water.indices.len() as u32;
        self.water_vb = Some(gpu.create_vertex_buffer(water.vertex_bytes())?);
        self.water_ib = Some(gpu.create_index_buffer(water.index_bytes())?);

        // Shore is a slab that *meets* the water, not a plane that sits in it.
        let ground = SphereMesh::plane_xz();
        self.ground_idx = ground.indices.len() as u32;
        self.ground_vb = Some(gpu.create_vertex_buffer(ground.vertex_bytes())?);
        self.ground_ib = Some(gpu.create_index_buffer(ground.index_bytes())?);

        let plane = SphereMesh::plane_xz();
        self.plane_idx = plane.indices.len() as u32;
        self.plane_vb = Some(gpu.create_vertex_buffer(plane.vertex_bytes())?);
        self.plane_ib = Some(gpu.create_index_buffer(plane.index_bytes())?);

        let sphere = SphereMesh::uv(20, 14);
        self.sphere_idx = sphere.indices.len() as u32;
        self.sphere_vb = Some(gpu.create_vertex_buffer(sphere.vertex_bytes())?);
        self.sphere_ib = Some(gpu.create_index_buffer(sphere.index_bytes())?);

        self.lights = lanterns();
        let mut packed = self.lights.clone();
        packed.resize(MAX_LIGHTS, Light::point(Vec3::ZERO, 0.0, Vec3::ZERO, 0.0));
        self.light_buf = Some(gpu.create_storage_buffer(SLOT_LIGHTS, light_bytes(&packed))?);

        self.rain_map = Some(gpu.create_texture(&shadow_atlas_desc(RAIN_MAP_SIZE))?);

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
                    cull_back: false,
                    ..Default::default()
                },
            })
            .context("water PSO")?,
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
        self.rain_map_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/opaque.vs.spv")),
                fs_spirv: &[],
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &[],
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE,
                    depth_test: true,
                    cull_back: false,
                    depth_only: true,
                    ..Default::default()
                },
            })
            .context("rain map PSO")?,
        );

        self.recreate(gpu, gpu.extent())?;
        tracing::info!(luzes = self.lights.len(), "storm");
        Ok(())
    }

    fn update(&mut self, input: &harpia_app::SampleInput, dt: f32) {
        self.cam.update(input, dt);
        self.puddle = puddle_growth(self.puddle, 0.9, dt, 0.55, 1.0);
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        let mut out = self.rt.as_ref().map_or_else(Vec::new, |rt| {
            vec![
                ("composite", rt.composite),
                ("scene", rt.hdr),
                ("opaque", rt.opaque),
            ]
        });
        if let Some(map) = self.rain_map {
            out.push(("rain-map", map));
        }
        out
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let rain_map = self.rain_map.context("rain map")?;
        let sky_pso = self.sky_pso.as_ref().context("sky")?;
        let opaque_pso = self.opaque_pso.as_ref().context("opaque")?;
        let water_pso = self.water_pso.as_ref().context("water")?;
        let copy_pso = self.copy_pso.as_ref().context("copy")?;
        let rain_pso = self.rain_pso.as_ref().context("rain")?;
        let blit_pso = self.blit_pso.as_ref().context("blit")?;
        let map_pso = self.rain_map_pso.as_ref().context("rain map pso")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let camera = self.cam.camera(w / h);
        let view_proj = camera.view_proj();
        // Same clock the water gate uses: 16 frames have to show motion.
        let t = info.frame_index as f32 * 0.05;
        let rain_vp = rain_map_view_proj(Vec3::new(0.0, 0.0, 6.0), 36.0, 40.0);

        let mut cb = StormCb {
            inv_view_proj: view_proj.inverse(),
            view_proj,
            view: camera.view(),
            camera_pos: Vec4::new(camera.eye.x, camera.eye.y, camera.eye.z, 1.0),
            inv_extent: Vec2::new(1.0 / w, 1.0 / h),
            light_count: self.lights.len() as u32,
            rain_map_vp: rain_vp,
            ..Default::default()
        };
        cb.rain.w = t;
        cb.misc.x = t;
        cb.wind.z = self.puddle;
        cb.material = GROUND_MAT;

        let plane_vb = self.plane_vb.context("plane vb")?;
        let plane_ib = self.plane_ib.context("plane ib")?;
        let ground_vb = self.ground_vb.context("ground vb")?;
        let ground_ib = self.ground_ib.context("ground ib")?;
        let sphere_vb = self.sphere_vb.context("sphere vb")?;
        let sphere_ib = self.sphere_ib.context("sphere ib")?;
        let water_vb = self.water_vb.context("water vb")?;
        let water_ib = self.water_ib.context("water ib")?;

        let deck = Mat4::from_scale_rotation_translation(
            Vec3::new(5.6, 1.0, 9.0),
            Quat::IDENTITY,
            Vec3::new(0.0, 1.85, 6.0),
        );
        let roof = Mat4::from_scale_rotation_translation(
            Vec3::new(6.4, 1.0, 9.4),
            Quat::IDENTITY,
            Vec3::new(0.0, 3.95, 6.0),
        );
        // x ±20, z 14..32 — land behind the camera, water in front of it.
        let ground_world = Mat4::from_scale_rotation_translation(
            Vec3::new(20.0, 1.0, 9.0),
            Quat::IDENTITY,
            Vec3::new(0.0, 0.62, 23.0),
        );

        // 1. rain map — top-down depth. A box ocluder would miss this roof.
        gpu.write_frame_bytes(cb.as_bytes())?;
        gpu.begin_color_pass(&[], Some(rain_map), &[], Some(1.0))?;
        StormDemo::draw_mesh(gpu, map_pso, rain_vp, ground_world, ground_vb, ground_ib, self.ground_idx)?;
        StormDemo::draw_mesh(gpu, map_pso, rain_vp, deck, plane_vb, plane_ib, self.plane_idx)?;
        StormDemo::draw_mesh(gpu, map_pso, rain_vp, roof, plane_vb, plane_ib, self.plane_idx)?;
        for (x, z) in POSTS {
            let world = Mat4::from_scale_rotation_translation(
                Vec3::new(0.14, 1.05, 0.14),
                Quat::IDENTITY,
                Vec3::new(x, 0.9, z),
            );
            StormDemo::draw_mesh(gpu, map_pso, rain_vp, world, sphere_vb, sphere_ib, self.sphere_idx)?;
        }
        for (x, z, sx, sy, sz) in ROCKS {
            let world = Mat4::from_scale_rotation_translation(
                Vec3::new(sx, sy, sz),
                Quat::IDENTITY,
                Vec3::new(x, 0.0, z),
            );
            StormDemo::draw_mesh(gpu, map_pso, rain_vp, world, sphere_vb, sphere_ib, self.sphere_idx)?;
        }
        gpu.end_color_pass()?;

        cb.rain_map = gpu.bindless_index(rain_map)?;

        // 2. opaque: sky, then wet ground / pier / rocks.
        gpu.write_frame_bytes(cb.as_bytes())?;
        gpu.begin_color_pass(
            &[rt.opaque, rt.view_depth],
            Some(rt.depth),
            &[[0.0, 0.0, 0.0, 1.0], [1.0e9, 0.0, 0.0, 0.0]],
            Some(1.0),
        )?;
        gpu.set_pipeline(sky_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;

        cb.material = GROUND_MAT;
        gpu.write_frame_bytes(cb.as_bytes())?;
        StormDemo::draw_mesh(gpu, opaque_pso, view_proj, ground_world, ground_vb, ground_ib, self.ground_idx)?;

        cb.material = WOOD_MAT;
        gpu.write_frame_bytes(cb.as_bytes())?;
        StormDemo::draw_mesh(gpu, opaque_pso, view_proj, deck, plane_vb, plane_ib, self.plane_idx)?;
        for (x, z) in POSTS {
            let world = Mat4::from_scale_rotation_translation(
                Vec3::new(0.14, 1.05, 0.14),
                Quat::IDENTITY,
                Vec3::new(x, 0.9, z),
            );
            StormDemo::draw_mesh(gpu, opaque_pso, view_proj, world, sphere_vb, sphere_ib, self.sphere_idx)?;
        }

        cb.material = ROOF_MAT;
        gpu.write_frame_bytes(cb.as_bytes())?;
        StormDemo::draw_mesh(gpu, opaque_pso, view_proj, roof, plane_vb, plane_ib, self.plane_idx)?;

        cb.material = ROCK_MAT;
        gpu.write_frame_bytes(cb.as_bytes())?;
        for (x, z, sx, sy, sz) in ROCKS {
            let world = Mat4::from_scale_rotation_translation(
                Vec3::new(sx, sy, sz),
                Quat::IDENTITY,
                Vec3::new(x, 0.0, z),
            );
            StormDemo::draw_mesh(gpu, opaque_pso, view_proj, world, sphere_vb, sphere_ib, self.sphere_idx)?;
        }

        // The lanterns have to be in the picture, not just in the lighting
        // integral — otherwise the spec on the water has no source.
        let bulbs: Vec<(Vec3, f32)> = self
            .lights
            .iter()
            .map(|l| (l.position(), if l.is_spot() { 0.11 } else { 0.07 }))
            .collect();
        cb.material = Vec4::new(7.5, 4.0, 1.5, -1.0);
        gpu.write_frame_bytes(cb.as_bytes())?;
        for (p, r) in bulbs {
            let world = Mat4::from_scale_rotation_translation(Vec3::splat(r), Quat::IDENTITY, p);
            StormDemo::draw_mesh(gpu, opaque_pso, view_proj, world, sphere_vb, sphere_ib, self.sphere_idx)?;
        }
        gpu.end_color_pass()?;

        // 3. water over a copy of the opaque image (SSR samples it).
        cb.scene_color = gpu.bindless_index(rt.opaque)?;
        cb.scene_depth = gpu.bindless_index(rt.view_depth)?;
        gpu.write_frame_bytes(cb.as_bytes())?;
        gpu.begin_color_pass(&[rt.hdr], Some(rt.depth), &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(copy_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.set_pipeline(water_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.set_push_constants(PushConstants::new(view_proj).as_bytes())?;
        gpu.bind_vertex_buffer(water_vb, 0)?;
        gpu.bind_index_buffer(water_ib)?;
        gpu.draw_indexed(self.water_idx, 1, 0, 0, 0)?;
        gpu.end_color_pass()?;

        // 4. streaks, masked by the rain map.
        cb.scene_color = gpu.bindless_index(rt.hdr)?;
        cb.scene_depth = gpu.bindless_index(rt.view_depth)?;
        gpu.write_frame_bytes(cb.as_bytes())?;
        gpu.begin_color_pass(&[rt.composite], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(rain_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        // 5. present.
        cb.scene_color = gpu.bindless_index(rt.composite)?;
        gpu.write_frame_bytes(cb.as_bytes())?;
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
    config.title = "Harpia — storm".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, StormDemo::default())
}
