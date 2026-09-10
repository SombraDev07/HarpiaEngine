use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{Vec2, Vec3, Vec4};
use harpia_render::{
    color_desc, compute_csm, depth_desc, froxel_desc, halton2, inject_dispatch,
    integrate_dispatch, load_gltf, sampled_desc, shadow_atlas_desc, CpuScene, FlyCamera, FogCb,
    LightingCb, PushConstants, DEFAULT_ATLAS_SIZE, GBUFFER_DEPTH_FORMAT, VERTEX_STRIDE_UV,
};
use harpia_rhi::{
    Buffer, ComputePipeline, ComputePipelineDesc, Device, Extent2D, Format, FrameInfo, Gpu,
    GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

/// Linear HDR: the fog has to be composited in light, not on an encoded image,
/// so the tonemap moved out of `color.ps` and into the fog apply.
const SCENE_FORMAT: Format = Format::Rgba16Float;
const VIEW_DEPTH: Format = Format::R32Float;
const COMPOSITE: Format = Format::Rgba8Unorm;
const COMPOSITE_FORMATS: [Format; 1] = [COMPOSITE];
/// The atrium is about 30 units across; there is no point marching past it.
const FOG_NEAR: f32 = 0.3;
const FOG_FAR: f32 = 45.0;
const SLOT_SCATTER: u32 = 0;
const SLOT_INTEGRATED: u32 = 1;

struct GpuPrim {
    vb: Buffer,
    ib: Buffer,
    index_count: u32,
    albedo: Texture,
    /// glTF `MASK`. Reaches the PS through `LightingCb::alpha_cutoff`.
    alpha_cutoff: f32,
    world: harpia_math::Mat4,
}

struct SceneRt {
    color: Texture,
    view_depth: Texture,
    depth: Texture,
    composite: Texture,
}

struct Sponza {
    shadow_pso: Option<GraphicsPipeline>,
    color_pso: Option<GraphicsPipeline>,
    /// glTF `doubleSided` (the cutout foliage): same shaders, cull off.
    color_pso_two_sided: Option<GraphicsPipeline>,
    /// Same depth-only pass, plus a PS that kills on the MASK cutoff.
    shadow_cutout_pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    apply_pso: Option<GraphicsPipeline>,
    inject_pso: Option<ComputePipeline>,
    integrate_pso: Option<ComputePipeline>,
    scatter: Option<Texture>,
    integrated: Option<Texture>,
    cam: FlyCamera,
    /// Sorted: plain opaque first, then everything that needs the cutout /
    /// no-cull path (glTF `MASK` or `doubleSided` — in Sponza the same three
    /// materials). One partition serves both the shadow and the colour pass.
    prims: Vec<GpuPrim>,
    two_sided_from: usize,
    atlas: Option<Texture>,
    scene: Option<SceneRt>,
    scene_extent: Extent2D,
}

impl Default for Sponza {
    fn default() -> Self {
        Self {
            shadow_pso: None,
            color_pso: None,
            color_pso_two_sided: None,
            shadow_cutout_pso: None,
            blit_pso: None,
            apply_pso: None,
            inject_pso: None,
            integrate_pso: None,
            scatter: None,
            integrated: None,
            // The shot phase 4 validated, now flyable. Slow, because the atrium
            // is about thirty units end to end.
            cam: FlyCamera {
                speed: 3.5,
                fov_y: 55.0_f32.to_radians(),
                near: 0.2,
                far: 80.0,
                ..FlyCamera::looking_at(Vec3::new(-9.5, 1.8, 0.0), Vec3::new(0.0, 1.6, 0.0))
            },
            prims: Vec::new(),
            two_sided_from: 0,
            atlas: None,
            scene: None,
            scene_extent: Extent2D {
                width: 0,
                height: 0,
            },
        }
    }
}

impl Sponza {
    fn recreate_scene(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.scene = Some(SceneRt {
            color: gpu.create_texture(&color_desc(w, h, SCENE_FORMAT))?,
            view_depth: gpu.create_texture(&color_desc(w, h, VIEW_DEPTH))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
            composite: gpu.create_texture(&color_desc(w, h, COMPOSITE))?,
        });
        self.scene_extent = Extent2D { width: w, height: h };
        Ok(())
    }

    /// One CBV chunk per primitive: the RHI ring makes each draw read its own
    /// albedo index. A single write per frame would give every draw the last one.
    fn draw_prims(
        &self,
        gpu: &mut Gpu,
        pso: &GraphicsPipeline,
        range: std::ops::Range<usize>,
        view_proj: harpia_math::Mat4,
        write_material: bool,
        cb: &mut LightingCb,
    ) -> Result<()> {
        gpu.set_pipeline(pso)?;
        gpu.bind_graphics_bindless()?;
        for p in &self.prims[range] {
            if write_material {
                cb.gbuf0 = gpu.bindless_index(p.albedo)?;
                cb.alpha_cutoff = p.alpha_cutoff;
                gpu.write_frame_bytes(cb.as_bytes())?;
            }
            gpu.set_push_constants(PushConstants::with_world(view_proj, p.world).as_bytes())?;
            gpu.bind_vertex_buffer(p.vb, 0)?;
            gpu.bind_index_buffer(p.ib)?;
            gpu.draw_indexed(p.index_count, 1, 0, 0, 0)?;
        }
        Ok(())
    }
}

fn sponza_gltf() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.pop();
    p.push("assets");
    p.push("sponza");
    p.push("glTF");
    p.push("Sponza.gltf");
    p
}

/// One GPU texture per *image*, not per primitive, and every mip uploaded —
/// sampling a mip that was never written is a GPUVM on RADV.
fn upload_images(gpu: &mut Gpu, scene: &CpuScene) -> Result<Vec<Texture>> {
    let mut out = Vec::with_capacity(scene.images.len());
    for img in &scene.images {
        let levels = img.mip_levels();
        let tex = gpu.create_texture(&sampled_desc(
            img.width,
            img.height,
            levels,
            Format::Rgba8Srgb,
        ))?;
        for (mip, data) in img.mip_chain().iter().enumerate() {
            gpu.upload_texture_mip(tex, mip as u32, data)?;
        }
        out.push(tex);
    }
    Ok(out)
}

fn color_targets(cull_back: bool) -> PipelineTargets<'static> {
    const FORMATS: [Format; 2] = [SCENE_FORMAT, VIEW_DEPTH];
    PipelineTargets {
        color_formats: &FORMATS,
        depth_format: Some(GBUFFER_DEPTH_FORMAT),
        vertex_stride: VERTEX_STRIDE_UV,
        instance_stride: 0,
        depth_test: true,
        cull_back,
        ..Default::default()
    }
}

impl Sample for Sponza {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        let path = sponza_gltf();
        let cpu = load_gltf(&path).map_err(|e| anyhow::anyhow!("{e}"))?;
        let textures = upload_images(gpu, &cpu)?;


        let special = |i: usize| cpu.prims[i].alpha_cutoff > 0.0 || cpu.prims[i].double_sided;
        let mut order: Vec<usize> = (0..cpu.prims.len()).collect();
        order.sort_by_key(|&i| special(i));
        self.two_sided_from = order.partition_point(|&i| !special(i));
        tracing::info!(
            primitives = cpu.prims.len(),
            images = cpu.images.len(),
            cutout = cpu.prims.len() - self.two_sided_from,
            path = %path.display(),
            "loaded sponza"
        );
        for i in order {
            let p = &cpu.prims[i];
            self.prims.push(GpuPrim {
                vb: gpu.create_vertex_buffer(p.vertex_bytes())?,
                ib: gpu.create_index_buffer(p.index_bytes())?,
                index_count: p.indices.len() as u32,
                albedo: textures[p.albedo],
                alpha_cutoff: p.alpha_cutoff,
                world: p.world,
            });
        }

        self.shadow_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/shadow.vs.spv")),
                fs_spirv: &[],
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &[],
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE_UV,
                    instance_stride: 0,
                    depth_test: true,
                    // Shadow casters are not culled: single-sided geometry
                    // (Sponza's drapes) would otherwise cast nothing. Acne is
                    // handled by depth_bias, not by front-face culling.
                    cull_back: false,
                    depth_only: true,
                    depth_bias: true,
                },
            })
            .context("shadow PSO")?,
        );
        self.shadow_cutout_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/shadow.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/shadow.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &[],
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE_UV,
                    instance_stride: 0,
                    depth_test: true,
                    cull_back: false,
                    depth_only: true,
                    depth_bias: true,
                },
            })
            .context("cutout shadow PSO")?,
        );
        let color = GraphicsPipelineDesc {
            vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/color.vs.spv")),
            fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/color.ps.spv")),
            vs_entry: "VSMain",
            fs_entry: "PSMain",
            bindless: true,
            targets: color_targets(true),
        };
        self.color_pso = Some(gpu.create_graphics_pipeline(&color).context("color PSO")?);
        self.color_pso_two_sided = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                targets: color_targets(false),
                ..color
            })
            .context("two-sided color PSO")?,
        );
        self.blit_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/blit.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/blit.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets::default(),
            })
            .context("blit PSO")?,
        );
        self.atlas = Some(gpu.create_texture(&shadow_atlas_desc(DEFAULT_ATLAS_SIZE))?);
        self.apply_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/apply.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &COMPOSITE_FORMATS,
                    ..Default::default()
                },
            })
            .context("fog apply PSO")?,
        );
        self.inject_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/inject.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("inject PSO")?,
        );
        self.integrate_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/integrate.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("integrate PSO")?,
        );
        let scatter = gpu.create_texture(&froxel_desc())?;
        let integrated = gpu.create_texture(&froxel_desc())?;
        gpu.bind_volume_uav(SLOT_SCATTER, scatter)?;
        gpu.bind_volume_uav(SLOT_INTEGRATED, integrated)?;
        gpu.bind_volume_srv(0, integrated)?;
        self.scatter = Some(scatter);
        self.integrated = Some(integrated);
        self.recreate_scene(gpu, gpu.extent())?;
        Ok(())
    }

    fn update(&mut self, input: &harpia_app::SampleInput, dt: f32) {
        self.cam.update(input, dt);
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        let mut out = Vec::new();
        if let Some(t) = self.scene.as_ref().map(|s| s.color) {
            out.push(("scene", t));
        }
        if let Some(t) = self.atlas {
            out.push(("shadow-atlas", t));
        }
        if let Some(rt) = self.scene.as_ref() {
            out.push(("composite", rt.composite));
        }
        out
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.scene_extent.width
            || info.extent.height != self.scene_extent.height
        {
            self.recreate_scene(gpu, info.extent)?;
        }
        let scene = self.scene.as_ref().context("scene rt")?;
        let atlas = self.atlas.context("atlas")?;
        let shadow_pso = self.shadow_pso.as_ref().context("shadow pso")?;
        let shadow_cutout_pso = self
            .shadow_cutout_pso
            .as_ref()
            .context("cutout shadow pso")?;
        let color_pso = self.color_pso.as_ref().context("color pso")?;
        let two_sided_pso = self
            .color_pso_two_sided
            .as_ref()
            .context("two-sided color pso")?;
        let blit_pso = self.blit_pso.as_ref().context("blit pso")?;
        let apply_pso = self.apply_pso.as_ref().context("apply pso")?;
        let inject_pso = self.inject_pso.as_ref().context("inject pso")?;
        let integrate_pso = self.integrate_pso.as_ref().context("integrate pso")?;
        let scatter = self.scatter.context("scatter")?;
        let integrated = self.integrated.context("integrated")?;
        let one_sided = 0..self.two_sided_from;
        let two_sided = self.two_sided_from..self.prims.len();

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let camera = self.cam.camera(w / h);
        let sun = Vec3::new(0.35, 0.85, 0.28).normalize();
        let csm = compute_csm(&camera, sun, DEFAULT_ATLAS_SIZE);
        let view = camera.view();
        let view_proj = camera.view_proj();

        let mut cb = LightingCb {
            inv_view_proj: view_proj.inverse(),
            camera_pos: Vec4::new(camera.eye.x, camera.eye.y, camera.eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            sun_color: Vec4::new(4.0, 3.6, 3.1, 1.0),
            gbuf0: gpu.bindless_index(scene.color)?,
            exposure: 1.0,
            inv_extent: Vec2::new(1.0 / w, 1.0 / h),
            view,
            ..Default::default()
        };
        cb.apply_csm(&csm, gpu.bindless_index(atlas)?);
        gpu.write_frame_bytes(cb.as_bytes())?;

        gpu.begin_color_pass(&[], Some(atlas), &[], Some(1.0))?;
        for i in 0..4 {
            let (x, y, tw, th) = csm.tile_viewport(i);
            gpu.set_viewport(x, y, tw, th)?;
            self.draw_prims(
                gpu,
                shadow_pso,
                one_sided.clone(),
                csm.view_proj[i],
                false,
                &mut cb,
            )?;
            // Cutout casters need the albedo alpha, so they carry material.
            self.draw_prims(
                gpu,
                shadow_cutout_pso,
                two_sided.clone(),
                csm.view_proj[i],
                true,
                &mut cb,
            )?;
        }
        gpu.end_color_pass()?;

        // The clear is sky radiance, not a colour: the scene target is linear HDR
        // now and the tonemap happens in the fog apply. Depth clears to the fog
        // far plane so the sky gets a full froxel march instead of zero fog.
        gpu.begin_color_pass(
            &[scene.color, scene.view_depth],
            Some(scene.depth),
            &[[0.62, 0.86, 1.20, 1.0], [FOG_FAR, 0.0, 0.0, 0.0]],
            Some(1.0),
        )?;
        self.draw_prims(gpu, color_pso, one_sided, view_proj, true, &mut cb)?;
        self.draw_prims(gpu, two_sided_pso, two_sided, view_proj, true, &mut cb)?;
        gpu.end_color_pass()?;

        // Fog, default-on (roadmap fase 5): the inject reads the same cascades the
        // scene did, so the shafts through the arcade cost no extra pass.
        let fog_cb = FogCb {
            inv_view: view.inverse(),
            camera_pos: Vec4::new(camera.eye.x, camera.eye.y, camera.eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            sun_color: Vec4::new(4.0, 3.6, 3.1, 1.0),
            fog: Vec4::new(0.020, 0.09, 0.60, 0.0),
            froxel: Vec4::new(FOG_NEAR, FOG_FAR, (camera.fov_y * 0.5).tan(), camera.aspect),
            misc: Vec4::new(halton2(info.frame_index as u32), harpia_render::FROXEL_D as f32, 1.0, 0.0),
            scene_color: gpu.bindless_index(scene.color)?,
            scene_depth: gpu.bindless_index(scene.view_depth)?,
            inv_extent: Vec2::new(1.0 / w, 1.0 / h),
            shadow_idx: gpu.bindless_index(atlas)?,
            cascade_count: 4,
            atlas_size: DEFAULT_ATLAS_SIZE as f32,
            shadow_strength: 0.85,
            splits: csm.splits,
            cascades: csm.view_proj,
        };
        gpu.write_frame_bytes(fog_cb.as_bytes())?;
        let (ix, iy, iz) = inject_dispatch();
        gpu.set_compute_pipeline(inject_pso)?;
        gpu.bind_compute_bindless()?;
        gpu.dispatch(ix, iy, iz)?;
        gpu.storage_barrier(scatter)?;
        let (gx, gy, gz) = integrate_dispatch();
        gpu.set_compute_pipeline(integrate_pso)?;
        gpu.bind_compute_bindless()?;
        gpu.dispatch(gx, gy, gz)?;
        gpu.storage_barrier(integrated)?;

        gpu.begin_color_pass(&[scene.composite], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(apply_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        cb.gbuf0 = gpu.bindless_index(scene.composite)?;
        cb.alpha_cutoff = 0.0;
        gpu.write_frame_bytes(cb.as_bytes())?;
        gpu.begin_swapchain_pass([0.02, 0.03, 0.05, 1.0])?;
        gpu.set_pipeline(blit_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — sponza".into();
    run(config, Sponza::default())
}
