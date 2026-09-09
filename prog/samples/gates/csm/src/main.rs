use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{Vec2, Vec3, Vec4};
use harpia_render::{
    color_desc, compute_csm, depth_desc, shadow_atlas_desc, Camera, LightingCb, MaterialGpu,
    PushConstants, SphereInstance, SphereMesh, DEFAULT_ATLAS_SIZE, GBUFFER_DEPTH_FORMAT,
    INSTANCE_STRIDE, VERTEX_STRIDE,
};
use harpia_rhi::{
    Buffer, Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const SCENE_FORMAT: Format = Format::Rgba8Unorm;

struct SceneRt {
    color: Texture,
    depth: Texture,
}

struct CsmGate {
    shadow_pso: Option<GraphicsPipeline>,
    color_pso: Option<GraphicsPipeline>,
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
    atlas: Option<Texture>,
    scene: Option<SceneRt>,
    scene_extent: Extent2D,
}

impl Default for CsmGate {
    fn default() -> Self {
        Self {
            shadow_pso: None,
            color_pso: None,
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
            atlas: None,
            scene: None,
            scene_extent: Extent2D {
                width: 0,
                height: 0,
            },
        }
    }
}

impl CsmGate {
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

    fn draw_casters(
        &self,
        gpu: &mut Gpu,
        pso: &GraphicsPipeline,
        view_proj: harpia_math::Mat4,
    ) -> Result<()> {
        gpu.set_pipeline(pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.set_push_constants(PushConstants::new(view_proj).as_bytes())?;
        gpu.bind_vertex_buffer(self.plane_vb.context("plane vb")?, 0)?;
        gpu.bind_vertex_buffer(self.plane_inst.context("plane inst")?, 1)?;
        gpu.bind_index_buffer(self.plane_ib.context("plane ib")?)?;
        gpu.draw_indexed(self.plane_idx, 1, 0, 0, 0)?;
        gpu.bind_vertex_buffer(self.sph_vb.context("sph vb")?, 0)?;
        gpu.bind_vertex_buffer(self.sph_inst.context("sph inst")?, 1)?;
        gpu.bind_index_buffer(self.sph_ib.context("sph ib")?)?;
        gpu.draw_indexed(self.sph_idx, self.sph_count, 0, 0, 0)?;
        Ok(())
    }
}

impl Sample for CsmGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
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
                    vertex_stride: VERTEX_STRIDE,
                    instance_stride: INSTANCE_STRIDE,
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
        self.color_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/color.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/color.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &[SCENE_FORMAT],
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE,
                    instance_stride: INSTANCE_STRIDE,
                    depth_test: true,
                    cull_back: true,
                    ..Default::default()
                },
            })
            .context("color PSO")?,
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

        let plane = SphereMesh::plane_xz();
        self.plane_idx = plane.indices.len() as u32;
        self.plane_vb = Some(gpu.create_vertex_buffer(plane.vertex_bytes())?);
        self.plane_ib = Some(gpu.create_index_buffer(plane.index_bytes())?);
        let mut ground = MaterialGpu::default();
        ground.base_color = [0.62, 0.60, 0.56];
        let plane_i = [SphereInstance::from_material([0.0, 0.0, 0.0], 16.0, &ground)];
        self.plane_inst = Some(gpu.create_vertex_buffer(instance_bytes(&plane_i))?);

        let sph = SphereMesh::uv(20, 14);
        self.sph_idx = sph.indices.len() as u32;
        self.sph_vb = Some(gpu.create_vertex_buffer(sph.vertex_bytes())?);
        self.sph_ib = Some(gpu.create_index_buffer(sph.index_bytes())?);
        let spheres = sphere_instances();
        self.sph_count = spheres.len() as u32;
        self.sph_inst = Some(gpu.create_vertex_buffer(instance_bytes(&spheres))?);

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
        let blit_pso = self.blit_pso.as_ref().context("blit pso")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let camera = Camera {
            eye: Vec3::new(0.0, 6.2, 14.5),
            target: Vec3::new(0.0, 1.0, 0.0),
            up: Vec3::Y,
            fov_y: 50.0_f32.to_radians(),
            aspect: w / h,
            near: 0.5,
            far: 70.0,
        };
        let sun = Vec3::new(0.42, 0.82, 0.32).normalize();
        let csm = compute_csm(&camera, sun, DEFAULT_ATLAS_SIZE);
        let view = camera.view();
        let view_proj = camera.view_proj();

        let mut cb = LightingCb {
            inv_view_proj: view_proj.inverse(),
            camera_pos: Vec4::new(camera.eye.x, camera.eye.y, camera.eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            sun_color: Vec4::new(3.6, 3.2, 2.7, 1.0),
            gbuf0: gpu.bindless_index(scene.color)?,
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
            self.draw_casters(gpu, shadow_pso, csm.view_proj[i])?;
        }
        gpu.end_color_pass()?;

        gpu.begin_color_pass(
            &[scene.color],
            Some(scene.depth),
            &[[0.42, 0.55, 0.72, 1.0]],
            Some(1.0),
        )?;
        self.draw_casters(gpu, color_pso, view_proj)?;
        gpu.end_color_pass()?;

        gpu.begin_swapchain_pass([0.02, 0.03, 0.05, 1.0])?;
        gpu.set_pipeline(blit_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }
}

fn sphere_instances() -> Vec<SphereInstance> {
    let mut red = MaterialGpu::default();
    red.base_color = [0.82, 0.18, 0.14];
    let mut white = MaterialGpu::default();
    white.base_color = [0.92, 0.92, 0.90];
    let mut teal = MaterialGpu::default();
    teal.base_color = [0.12, 0.55, 0.52];
    vec![
        SphereInstance::from_material([0.0, 1.35, 0.0], 1.35, &white),
        SphereInstance::from_material([3.2, 0.85, -1.1], 0.85, &red),
        SphereInstance::from_material([-2.8, 0.75, 1.6], 0.75, &teal),
    ]
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
    config.title = "Harpia — csm".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, CsmGate::default())
}
