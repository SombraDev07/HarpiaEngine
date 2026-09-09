use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{Vec2, Vec3, Vec4};
use harpia_render::{
    color_desc, compute_csm, depth_desc, load_gltf, sampled_desc, shadow_atlas_desc, Camera,
    CpuScene, LightingCb, PushConstants, DEFAULT_ATLAS_SIZE, GBUFFER_DEPTH_FORMAT,
    VERTEX_STRIDE_UV,
};
use harpia_rhi::{
    Buffer, Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const SCENE_FORMAT: Format = Format::Rgba8Unorm;

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
    depth: Texture,
}

struct Sponza {
    shadow_pso: Option<GraphicsPipeline>,
    color_pso: Option<GraphicsPipeline>,
    /// glTF `doubleSided` (the cutout foliage): same shaders, cull off.
    color_pso_two_sided: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    /// Sorted: single-sided first, then the `doubleSided` primitives.
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
            blit_pso: None,
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
            depth: gpu.create_texture(&depth_desc(w, h))?,
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
    const FORMATS: [Format; 1] = [SCENE_FORMAT];
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
        tracing::info!(
            primitives = cpu.prims.len(),
            images = cpu.images.len(),
            path = %path.display(),
            "loaded sponza"
        );

        let mut order: Vec<usize> = (0..cpu.prims.len()).collect();
        order.sort_by_key(|&i| cpu.prims[i].double_sided);
        self.two_sided_from = order.partition_point(|&i| !cpu.prims[i].double_sided);
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
        self.recreate_scene(gpu, gpu.extent())?;
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        let mut out = Vec::new();
        if let Some(t) = self.scene.as_ref().map(|s| s.color) {
            out.push(("scene", t));
        }
        if let Some(t) = self.atlas {
            out.push(("shadow-atlas", t));
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
        let color_pso = self.color_pso.as_ref().context("color pso")?;
        let two_sided_pso = self
            .color_pso_two_sided
            .as_ref()
            .context("two-sided color pso")?;
        let blit_pso = self.blit_pso.as_ref().context("blit pso")?;
        let all = 0..self.prims.len();
        let one_sided = 0..self.two_sided_from;
        let two_sided = self.two_sided_from..self.prims.len();

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let camera = Camera {
            eye: Vec3::new(-9.5, 1.8, 0.0),
            target: Vec3::new(0.0, 1.6, 0.0),
            up: Vec3::Y,
            fov_y: 55.0_f32.to_radians(),
            aspect: w / h,
            near: 0.2,
            far: 80.0,
        };
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
            self.draw_prims(gpu, shadow_pso, all.clone(), csm.view_proj[i], false, &mut cb)?;
        }
        gpu.end_color_pass()?;

        gpu.begin_color_pass(
            &[scene.color],
            Some(scene.depth),
            &[[0.38, 0.52, 0.70, 1.0]],
            Some(1.0),
        )?;
        self.draw_prims(gpu, color_pso, one_sided, view_proj, true, &mut cb)?;
        self.draw_prims(gpu, two_sided_pso, two_sided, view_proj, true, &mut cb)?;
        gpu.end_color_pass()?;

        cb.gbuf0 = gpu.bindless_index(scene.color)?;
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
