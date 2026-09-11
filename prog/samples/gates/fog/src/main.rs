use anyhow::{Context, Result};
use harpia_app::{AppConfig, Sample, run};
use harpia_math::{Vec2, Vec3, Vec4};
use harpia_render::{
    Access, Camera, DEFAULT_ATLAS_SIZE, FogCb, GBUFFER_DEPTH_FORMAT, INSTANCE_STRIDE, Load,
    MaterialGpu, Pass, PassPlan, PushConstants, RenderGraph, SphereInstance, SphereMesh,
    VERTEX_STRIDE, color_desc, compute_csm, depth_desc, froxel_desc, halton2, inject_dispatch,
    integrate_dispatch, shadow_atlas_desc,
};
use harpia_rhi::{
    Buffer, ComputePipeline, ComputePipelineDesc, Device, Extent2D, Format, FrameInfo, Gpu,
    GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const HDR: Format = Format::Rgba16Float;
/// RT1 carries the positive view depth the fog apply pass turns into a slice.
const VIEW_DEPTH: Format = Format::R32Float;
const FORWARD_FORMATS: [Format; 2] = [HDR, VIEW_DEPTH];
/// apply.ps writes display-referred values here, then blit copies to the swapchain.
const COMPOSITE: Format = Format::Rgba8Unorm;
const COMPOSITE_FORMATS: [Format; 1] = [COMPOSITE];

const FOG_NEAR: f32 = 0.5;
const FOG_FAR: f32 = 60.0;
const FOV_Y: f32 = 50.0;

/// UAV slots in set 4 binding 1, SRV slot in set 5 binding 0.
const SLOT_SCATTER: u32 = 0;
const SLOT_INTEGRATED: u32 = 1;

struct Targets {
    color: Texture,
    view_depth: Texture,
    depth: Texture,
    /// apply.ps lands here instead of straight on the swapchain, so `--capture`
    /// can look at the composite the gate is actually judged on.
    composite: Texture,
}

struct FogGate {
    fwd_pso: Option<GraphicsPipeline>,
    shadow_pso: Option<GraphicsPipeline>,
    atlas: Option<Texture>,
    apply_pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    inject_pso: Option<ComputePipeline>,
    integrate_pso: Option<ComputePipeline>,
    plane_vb: Option<Buffer>,
    plane_ib: Option<Buffer>,
    plane_inst: Option<Buffer>,
    plane_idx: u32,
    sph_vb: Option<Buffer>,
    sph_ib: Option<Buffer>,
    sph_inst: Option<Buffer>,
    sph_idx: u32,
    sph_count: u32,
    scatter: Option<Texture>,
    integrated: Option<Texture>,
    rt: Option<Targets>,
    extent: Extent2D,
}

impl Default for FogGate {
    fn default() -> Self {
        Self {
            fwd_pso: None,
            shadow_pso: None,
            atlas: None,
            apply_pso: None,
            blit_pso: None,
            inject_pso: None,
            integrate_pso: None,
            plane_vb: None,
            plane_ib: None,
            plane_inst: None,
            plane_idx: 0,
            sph_vb: None,
            sph_ib: None,
            sph_inst: None,
            sph_idx: 0,
            sph_count: 0,
            scatter: None,
            integrated: None,
            rt: None,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
        }
    }
}

impl FogGate {
    /// O frame declarado como grafo.
    ///
    /// O interessante aqui são os dois volumes do fog: o `inject` escreve o
    /// `scatter`, o `integrate` lê-o e escreve o `integrated`, e o `apply`
    /// amostra esse. Três acessos encadeados a recursos de storage, que era o
    /// sítio com mais barreiras escritas à mão fora da Sponza.
    fn build_graph(&self) -> Result<Vec<PassPlan>> {
        let mut g = RenderGraph::new();
        let rt = self.rt.as_ref().context("rt")?;
        let atlas = g.texture("csm-atlas", self.atlas.context("atlas")?);
        let color = g.texture("color", rt.color);
        let view_depth = g.texture("view-depth", rt.view_depth);
        let depth = g.texture("depth", rt.depth);
        let composite = g.texture("composite", rt.composite);
        let scatter = g.texture("fog-scatter", self.scatter.context("scatter")?);
        let integrated = g.texture("fog-integrated", self.integrated.context("integrated")?);

        g.pass(
            Pass::new("cascades")
                .uses(atlas, Access::DepthWrite)
                .load(atlas, Load::Clear([1.0, 0.0, 0.0, 0.0])),
        );
        g.pass(
            Pass::new("scene")
                .uses(atlas, Access::Sampled)
                .uses(color, Access::ColorWrite)
                .uses(view_depth, Access::ColorWrite)
                .uses(depth, Access::DepthWrite)
                .load(color, Load::Clear([0.40, 0.52, 0.66, 1.0]))
                .load(view_depth, Load::Clear([FOG_FAR, 0.0, 0.0, 0.0]))
                .load(depth, Load::Clear([1.0, 0.0, 0.0, 0.0])),
        );
        g.pass(
            Pass::new("fog inject")
                .uses(atlas, Access::Sampled)
                .uses(scatter, Access::StorageWrite),
        );
        g.pass(
            Pass::new("fog integrate")
                .uses(scatter, Access::StorageRead)
                .uses(integrated, Access::StorageWrite),
        );
        g.pass(
            Pass::new("fog apply")
                .uses(color, Access::Sampled)
                .uses(view_depth, Access::Sampled)
                .uses(integrated, Access::Sampled)
                .uses(composite, Access::ColorWrite)
                .load(composite, Load::Clear([0.0, 0.0, 0.0, 1.0])),
        );
        g.pass(Pass::new("present").uses(composite, Access::Sampled));

        if let Err(errors) = g.validate() {
            anyhow::bail!(
                "render graph inválido: {}",
                errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
        Ok(g.compile())
    }

    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.rt = Some(Targets {
            color: gpu.create_texture(&color_desc(w, h, HDR))?,
            view_depth: gpu.create_texture(&color_desc(w, h, VIEW_DEPTH))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
            composite: gpu.create_texture(&color_desc(w, h, COMPOSITE))?,
        });
        self.extent = Extent2D {
            width: w,
            height: h,
        };
        Ok(())
    }

    fn draw_casters(&self, gpu: &mut Gpu, view_proj: harpia_math::Mat4) -> Result<()> {
        let pso = self.shadow_pso.as_ref().context("shadow pso")?;
        gpu.set_pipeline(pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.set_push_constants(PushConstants::new(view_proj).as_bytes())?;
        // The ground plane is not a caster: it would only shadow itself.
        gpu.bind_vertex_buffer(self.sph_vb.context("sph vb")?, 0)?;
        gpu.bind_vertex_buffer(self.sph_inst.context("sph inst")?, 1)?;
        gpu.bind_index_buffer(self.sph_ib.context("sph ib")?)?;
        gpu.draw_indexed(self.sph_idx, self.sph_count, 0, 0, 0)?;
        Ok(())
    }

    fn draw_scene(&self, gpu: &mut Gpu, view_proj: harpia_math::Mat4) -> Result<()> {
        let pso = self.fwd_pso.as_ref().context("forward pso")?;
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

impl Sample for FogGate {
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
                    cull_back: false,
                    depth_only: true,
                    depth_bias: true,
                },
            })
            .context("shadow PSO")?,
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
            .context("apply PSO")?,
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

        let plane = SphereMesh::plane_xz();
        self.plane_idx = plane.indices.len() as u32;
        self.plane_vb = Some(gpu.create_vertex_buffer(plane.vertex_bytes())?);
        self.plane_ib = Some(gpu.create_index_buffer(plane.index_bytes())?);
        let mut ground = MaterialGpu::default();
        ground.base_color = [0.58, 0.56, 0.52];
        let plane_i = [SphereInstance::from_material(
            [0.0, 0.0, 0.0],
            40.0,
            &ground,
        )];
        self.plane_inst = Some(gpu.create_vertex_buffer(instance_bytes(&plane_i))?);

        let sph = SphereMesh::uv(20, 14);
        self.sph_idx = sph.indices.len() as u32;
        self.sph_vb = Some(gpu.create_vertex_buffer(sph.vertex_bytes())?);
        self.sph_ib = Some(gpu.create_index_buffer(sph.index_bytes())?);
        let spheres = pillars();
        self.sph_count = spheres.len() as u32;
        self.sph_inst = Some(gpu.create_vertex_buffer(instance_bytes(&spheres))?);

        // Froxels live in GENERAL for their whole life: written as UAVs, read as
        // an SRV. Binding them once at init keeps the frame free of descriptor writes.
        let scatter = gpu.create_texture(&froxel_desc())?;
        let integrated = gpu.create_texture(&froxel_desc())?;
        gpu.bind_volume_uav(SLOT_SCATTER, scatter)?;
        gpu.bind_volume_uav(SLOT_INTEGRATED, integrated)?;
        gpu.bind_volume_srv(0, integrated)?;
        self.scatter = Some(scatter);
        self.integrated = Some(integrated);

        self.recreate(gpu, gpu.extent())?;
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        let mut out = Vec::new();
        if let Some(rt) = self.rt.as_ref() {
            out.push(("composite", rt.composite));
            out.push(("scene", rt.color));
        }
        if let Some(t) = self.atlas {
            out.push(("shadow-atlas", t));
        }
        if let Some(t) = self.scatter {
            out.push(("froxel-scatter", t));
        }
        if let Some(t) = self.integrated {
            out.push(("froxel-integrated", t));
        }
        out
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let apply = self.apply_pso.as_ref().context("apply pso")?;
        let blit = self.blit_pso.as_ref().context("blit pso")?;
        let inject = self.inject_pso.as_ref().context("inject pso")?;
        let integrate = self.integrate_pso.as_ref().context("integrate pso")?;
        let atlas = self.atlas.context("atlas")?;
        // Os volumes do fog são declarados no `build_graph`; aqui só se confirma
        // que existem.
        self.scatter.context("scatter")?;
        self.integrated.context("integrated")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let camera = Camera {
            eye: Vec3::new(0.0, 2.6, 15.0),
            target: Vec3::new(0.0, 1.6, -6.0),
            up: Vec3::Y,
            fov_y: FOV_Y.to_radians(),
            aspect: w / h,
            near: 0.4,
            far: 80.0,
        };
        let sun = Vec3::new(0.30, 0.34, -0.89).normalize();
        let view_proj = camera.view_proj();

        let csm = compute_csm(&camera, sun, DEFAULT_ATLAS_SIZE);

        let cb = FogCb {
            inv_view: camera.view().inverse(),
            camera_pos: Vec4::new(camera.eye.x, camera.eye.y, camera.eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            sun_color: Vec4::new(3.6, 3.3, 2.9, 1.0),
            // density, height falloff, phase g (forward scattering), base height
            fog: Vec4::new(0.058, 0.11, 0.66, 0.0),
            froxel: Vec4::new(
                FOG_NEAR,
                FOG_FAR,
                (FOV_Y.to_radians() * 0.5).tan(),
                camera.aspect,
            ),
            misc: Vec4::new(
                halton2(info.frame_index as u32),
                harpia_render::FROXEL_D as f32,
                // Looking into the sun through a forward phase is bright by
                // construction; pull the exposure down or the shafts clip away.
                0.62,
                0.0,
            ),
            scene_color: gpu.bindless_index(rt.color)?,
            scene_depth: gpu.bindless_index(rt.view_depth)?,
            inv_extent: Vec2::new(1.0 / w, 1.0 / h),
            shadow_idx: gpu.bindless_index(atlas)?,
            cascade_count: 4,
            atlas_size: DEFAULT_ATLAS_SIZE as f32,
            // Not 1.0: a shadowed froxel still gets sky and bounce, and a fully
            // black shaft boundary reads as a hard edge in mid-air.
            shadow_strength: 0.85,
            splits: csm.splits,
            cascades: csm.view_proj,
            ..Default::default()
        };
        gpu.write_frame_bytes(cb.as_bytes())?;

        // 0. cascades. The froxel inject samples this, which is what makes the
        //    fog show shafts instead of a uniform haze.
        let mut plans = self.build_graph()?.into_iter();
        let mut next_barriers = move || plans.next().map(|p| p.barriers).unwrap_or_default();

        gpu.barriers(&next_barriers())?;
        gpu.begin_color_pass(&[], Some(atlas), &[], Some(1.0))?;
        for i in 0..4 {
            let (x, y, tw, th) = csm.tile_viewport(i);
            gpu.set_viewport(x, y, tw, th)?;
            self.draw_casters(gpu, csm.view_proj[i])?;
        }
        gpu.end_color_pass()?;

        // 1. scene into HDR + view depth. Depth clears to the fog far plane so
        //    the background gets a full froxel march instead of zero fog.
        gpu.barriers(&next_barriers())?;
        gpu.begin_color_pass(
            &[rt.color, rt.view_depth],
            Some(rt.depth),
            &[[0.40, 0.52, 0.66, 1.0], [FOG_FAR, 0.0, 0.0, 0.0]],
            Some(1.0),
        )?;
        self.draw_scene(gpu, view_proj)?;
        gpu.end_color_pass()?;

        // 2. froxels: inject, then march Z. Both dispatches run outside a pass.
        gpu.barriers(&next_barriers())?;
        let (ix, iy, iz) = inject_dispatch();
        gpu.set_compute_pipeline(inject)?;
        gpu.bind_compute_bindless()?;
        gpu.dispatch(ix, iy, iz)?;
        gpu.barriers(&next_barriers())?;

        let (gx, gy, gz) = integrate_dispatch();
        gpu.set_compute_pipeline(integrate)?;
        gpu.bind_compute_bindless()?;
        gpu.dispatch(gx, gy, gz)?;

        // 3. composite scene * transmittance + in-scattering, then tonemap.
        gpu.barriers(&next_barriers())?;
        gpu.begin_color_pass(&[rt.composite], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(apply)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        // 4. present. A fresh CBV chunk re-points scene_color at the composite.
        let mut blit_cb = cb;
        blit_cb.scene_color = gpu.bindless_index(rt.composite)?;
        gpu.write_frame_bytes(blit_cb.as_bytes())?;
        gpu.barriers(&next_barriers())?;
        gpu.begin_swapchain_pass([0.02, 0.03, 0.05, 1.0])?;
        gpu.set_pipeline(blit)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }
}

/// A receding row of spheres: the point of the gate is seeing fog build with depth.
fn pillars() -> Vec<SphereInstance> {
    let mut out = Vec::new();
    let mut pale = MaterialGpu::default();
    pale.base_color = [0.88, 0.86, 0.82];
    let mut warm = MaterialGpu::default();
    warm.base_color = [0.80, 0.35, 0.22];
    for i in 0..7 {
        let z = -2.0 - i as f32 * 5.0;
        let r = 1.1;
        out.push(SphereInstance::from_material([-3.4, r, z], r, &pale));
        out.push(SphereInstance::from_material([3.4, r, z], r, &warm));
    }
    // A lattice across the sun, Sponza's arcade reduced to what this gate has.
    // The gaps are the whole point: the froxels show the light that gets through,
    // and the shadow volumes between them are the shafts.
    for row in 0..3 {
        for col in 0..7 {
            let x = -12.0 + col as f32 * 4.0;
            let y = 1.6 + row as f32 * 3.6;
            out.push(SphereInstance::from_material([x, y, -6.0], 1.55, &pale));
        }
    }
    out
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
    config.title = "Harpia — fog".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, FogGate::default())
}
