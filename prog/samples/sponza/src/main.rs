use anyhow::{Context, Result};
use harpia_app::{AppConfig, Sample, run};
use harpia_math::{Mat4, Vec2, Vec3, Vec4};
use harpia_render::{
    Access, CpuScene, DEFAULT_ATLAS_SIZE, FlyCamera, FogCb, GBUFFER_DEPTH_FORMAT, LightingCb, Load,
    Pass, PassPlan, PushConstants, RAIN_MAP_SIZE, RainCb, RenderGraph, VERTEX_STRIDE_UV,
    color_desc, compute_csm, depth_desc, froxel_desc, halton2, inject_dispatch, integrate_dispatch,
    load_gltf, rain_map_view_proj, sampled_desc, shadow_atlas_desc,
};
use harpia_rhi::{
    ComputePipeline, ComputePipelineDesc, Device, Extent2D, Format, FrameInfo, Gpu,
    GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets, Texture,
};
use harpia_scene::{
    ActiveFrustum, Bounds, CullStats, Frustum, Material, Mesh, TwoSided, Visible, WorldTransform,
    cull_to_frustum,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

/// Linear HDR: the fog has to be composited in light, not on an encoded image,
/// so the tonemap moved out of `color.ps` and into the fog apply.
const SCENE_FORMAT: Format = Format::Rgba16Float;
const VIEW_DEPTH: Format = Format::R32Float;
const COMPOSITE: Format = Format::Rgba8Unorm;
const COMPOSITE_FORMATS: [Format; 1] = [COMPOSITE];
const SCENE_ONLY: [Format; 1] = [SCENE_FORMAT];
/// The atrium is about 30 units across; there is no point marching past it.
const FOG_NEAR: f32 = 0.3;
const FOG_FAR: f32 = 45.0;
/// The atrium is open to the sky but the arcades are not. Half-extent covers the
/// nave with room around it; the height clears the upper gallery.
const RAIN_HALF: f32 = 26.0;
const RAIN_HEIGHT: f32 = 24.0;
const SLOT_SCATTER: u32 = 0;
const SLOT_INTEGRATED: u32 = 1;

struct SceneRt {
    color: Texture,
    view_depth: Texture,
    depth: Texture,
    /// The scene with the streaks added, still linear. The rain pass samples the
    /// scene, so it cannot also be writing to it.
    rained: Texture,
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
    rain_pso: Option<GraphicsPipeline>,
    rain_map: Option<Texture>,
    inject_pso: Option<ComputePipeline>,
    integrate_pso: Option<ComputePipeline>,
    scatter: Option<Texture>,
    integrated: Option<Texture>,
    cam: FlyCamera,
    /// Sorted: plain opaque first, then everything that needs the cutout /
    /// no-cull path (glTF `MASK` or `doubleSided` — in Sponza the same three
    /// materials). One partition serves both the shadow and the colour pass.
    /// A cena como entidades. Substitui a `Vec<GpuPrim>` e o `usize` que marcava
    /// onde começavam as de duas faces: agora é uma componente e uma query.
    world: bevy_ecs::world::World,
    cull: bevy_ecs::schedule::Schedule,
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
            rain_pso: None,
            rain_map: None,
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
            world: bevy_ecs::world::World::new(),
            cull: bevy_ecs::schedule::Schedule::default(),
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
    /// O frame da Sponza declarado como grafo: seis passes encadeadas.
    ///
    /// É a cena com mais passes da árvore, e a que tinha mais barreiras a serem
    /// raciocinadas à mão — duas, mais as transições de layout implícitas entre
    /// cada alvo e a pass que o lê a seguir. Aqui isso tudo sai da declaração.
    ///
    /// Repare-se no `scene.depth`: é escrito como profundidade na pass da cena e
    /// **amostrado** na chuva e no fog. É exactamente o par leitura↔escrita com
    /// mudança de layout que ninguém verificava.
    fn build_graph(&self) -> Result<Vec<PassPlan>> {
        let mut g = RenderGraph::new();
        let rt = self.scene.as_ref().context("scene rt")?;
        let atlas = g.texture("csm-atlas", self.atlas.context("atlas")?);
        let rain_map = g.texture("rain-map", self.rain_map.context("rain map")?);
        let color = g.texture("scene-color", rt.color);
        let view_depth = g.texture("view-depth", rt.view_depth);
        let depth = g.texture("depth", rt.depth);
        let rained = g.texture("rained", rt.rained);
        let composite = g.texture("composite", rt.composite);
        let scatter = g.texture("fog-scatter", self.scatter.context("scatter")?);
        let integrated = g.texture("fog-integrated", self.integrated.context("integrated")?);

        g.pass(
            Pass::new("cascades")
                .uses(atlas, Access::DepthWrite)
                .load(atlas, Load::Clear([1.0, 0.0, 0.0, 0.0])),
        );
        g.pass(
            Pass::new("rain map")
                .uses(rain_map, Access::DepthWrite)
                .load(rain_map, Load::Clear([1.0, 0.0, 0.0, 0.0])),
        );
        g.pass(
            Pass::new("scene")
                .uses(atlas, Access::Sampled)
                .uses(color, Access::ColorWrite)
                .uses(view_depth, Access::ColorWrite)
                .uses(depth, Access::DepthWrite)
                .load(color, Load::Clear([0.0; 4]))
                .load(view_depth, Load::Clear([0.0; 4]))
                .load(depth, Load::Clear([1.0, 0.0, 0.0, 0.0])),
        );
        g.pass(
            Pass::new("rain")
                .uses(color, Access::Sampled)
                .uses(view_depth, Access::Sampled)
                .uses(rain_map, Access::Sampled)
                .uses(rained, Access::ColorWrite)
                .load(rained, Load::Clear([0.0, 0.0, 0.0, 1.0])),
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
                .uses(rained, Access::Sampled)
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

    fn recreate_scene(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.scene = Some(SceneRt {
            color: gpu.create_texture(&color_desc(w, h, SCENE_FORMAT))?,
            view_depth: gpu.create_texture(&color_desc(w, h, VIEW_DEPTH))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
            rained: gpu.create_texture(&color_desc(w, h, SCENE_FORMAT))?,
            composite: gpu.create_texture(&color_desc(w, h, COMPOSITE))?,
        });
        self.scene_extent = Extent2D {
            width: w,
            height: h,
        };
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

/// Desenha as entidades que a query escolhe.
///
/// Livre e não método: os handles do RHI são `Copy`, portanto passá-los por valor
/// evita ter `self` emprestado de duas maneiras ao mesmo tempo.
///
/// Um chunk de CBV por primitiva: o anel do RHI faz cada draw ler o seu próprio
/// índice de albedo. Uma escrita por frame dava a todos o último.
///
/// `culled` diz se se respeita a marca `Visible`. O pass de sombra e o mapa de
/// chuva **não** a respeitam: um caster fora do ecrã continua a projectar sombra
/// para dentro dele, e cortá-lo faz a sombra desaparecer.
#[allow(clippy::too_many_arguments)]
fn draw_set(
    world: &mut bevy_ecs::world::World,
    gpu: &mut Gpu,
    pso: GraphicsPipeline,
    two_sided: bool,
    culled: bool,
    view_proj: Mat4,
    write_material: bool,
    cb: &mut LightingCb,
) -> Result<()> {
    gpu.set_pipeline(&pso)?;
    gpu.bind_graphics_bindless()?;
    let mut q = world.query::<(
        &Mesh,
        &Material,
        &WorldTransform,
        Option<&TwoSided>,
        Option<&Visible>,
    )>();
    let batch: Vec<(Mesh, Material, Mat4)> = q
        .iter(world)
        .filter(|(_, _, _, ts, vis)| ts.is_some() == two_sided && (!culled || vis.is_some()))
        .map(|(m, mat, x, _, _)| (*m, *mat, x.0))
        .collect();
    for (mesh, mat, world_m) in batch {
        if write_material {
            cb.gbuf0 = gpu.bindless_index(mat.albedo)?;
            cb.alpha_cutoff = mat.alpha_cutoff;
            gpu.write_frame_bytes(cb.as_bytes())?;
        }
        gpu.set_push_constants(PushConstants::with_world(view_proj, world_m).as_bytes())?;
        gpu.bind_vertex_buffer(mesh.vb, 0)?;
        gpu.bind_index_buffer(mesh.ib)?;
        gpu.draw_indexed(mesh.index_count, 1, 0, 0, 0)?;
    }
    Ok(())
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

        let mut two_sided = 0;
        for p in &cpu.prims {
            let mesh = Mesh {
                vb: gpu.create_vertex_buffer(p.vertex_bytes())?,
                ib: gpu.create_index_buffer(p.index_bytes())?,
                index_count: p.indices.len() as u32,
            };
            // A caixa é calculada aqui porque é o último sítio onde os vértices
            // ainda existem em CPU -- depois disto só há um handle de buffer.
            let bounds = Bounds::from_points(p.vertices.iter().map(|v| {
                let local = Vec3::new(v.pos[0], v.pos[1], v.pos[2]);
                p.world.transform_point3(local)
            }))
            .unwrap_or(Bounds {
                center: Vec3::ZERO,
                extents: Vec3::ZERO,
            });
            let mut e = self.world.spawn((
                mesh,
                Material {
                    albedo: textures[p.albedo],
                    alpha_cutoff: p.alpha_cutoff,
                },
                WorldTransform(p.world),
                bounds,
            ));
            // Cutout e doubleSided partilham o caminho sem culling, tal como antes.
            if p.alpha_cutoff > 0.0 || p.double_sided {
                e.insert(TwoSided);
                two_sided += 1;
            }
        }
        self.world.insert_resource(CullStats::default());
        self.world
            .insert_resource(ActiveFrustum(Frustum::from_view_proj(Mat4::IDENTITY)));
        self.cull.add_systems(cull_to_frustum);
        tracing::info!(
            primitives = cpu.prims.len(),
            images = cpu.images.len(),
            cutout = two_sided,
            path = %path.display(),
            "loaded sponza"
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
        self.rain_map = Some(gpu.create_texture(&shadow_atlas_desc(RAIN_MAP_SIZE))?);
        self.rain_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/rain.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &SCENE_ONLY,
                    ..Default::default()
                },
            })
            .context("rain PSO")?,
        );
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
        let rain_map = self.rain_map.context("rain map")?;
        let rain_pso = self.rain_pso.as_ref().context("rain pso")?;
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
        // Os dois volumes do fog já não são nomeados aqui: quem os declara é o
        // `build_graph`, e as barreiras deles vêm de lá.
        self.scatter.context("scatter")?;
        self.integrated.context("integrated")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let camera = self.cam.camera(w / h);
        let sun = Vec3::new(0.35, 0.85, 0.28).normalize();
        let csm = compute_csm(&camera, sun, DEFAULT_ATLAS_SIZE);
        let view = camera.view();
        let view_proj = camera.view_proj();

        // Culling antes de qualquer pass: o frustum deste frame decide quem leva
        // a marca `Visible`, e só o pass de cor a respeita.
        self.world
            .insert_resource(ActiveFrustum(Frustum::from_view_proj(view_proj)));
        self.cull.run(&mut self.world);
        if info.frame_index == 0 {
            let c = *self.world.resource::<CullStats>();
            tracing::info!(visible = c.visible, total = c.total, "frustum cull");
        }

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

        let mut plans = self.build_graph()?.into_iter();
        let mut next_barriers = move || plans.next().map(|p| p.barriers).unwrap_or_default();

        gpu.barriers(&next_barriers())?;
        gpu.begin_color_pass(&[], Some(atlas), &[], Some(1.0))?;
        for i in 0..4 {
            let (x, y, tw, th) = csm.tile_viewport(i);
            gpu.set_viewport(x, y, tw, th)?;
            draw_set(
                &mut self.world,
                gpu,
                *shadow_pso,
                false,
                false,
                csm.view_proj[i],
                false,
                &mut cb,
            )?;
            // Cutout casters need the albedo alpha, so they carry material.
            draw_set(
                &mut self.world,
                gpu,
                *shadow_cutout_pso,
                true,
                false,
                csm.view_proj[i],
                true,
                &mut cb,
            )?;
        }
        gpu.end_color_pass()?;
        gpu.mark("cascades");

        // Rain map: the same casters seen from straight up. Screen-space streaks
        // have no idea the arcade has a roof, and this is what stops it raining
        // indoors.
        let rain_vp = rain_map_view_proj(
            Vec3::new(camera.eye.x, 0.0, camera.eye.z),
            RAIN_HALF,
            RAIN_HEIGHT,
        );
        gpu.barriers(&next_barriers())?;
        gpu.begin_color_pass(&[], Some(rain_map), &[], Some(1.0))?;
        draw_set(
            &mut self.world,
            gpu,
            *shadow_pso,
            false,
            false,
            rain_vp,
            false,
            &mut cb,
        )?;
        draw_set(
            &mut self.world,
            gpu,
            *shadow_cutout_pso,
            true,
            false,
            rain_vp,
            true,
            &mut cb,
        )?;
        gpu.end_color_pass()?;
        gpu.mark("rain map");

        // The clear is sky radiance, not a colour: the scene target is linear HDR
        // now and the tonemap happens in the fog apply. Depth clears to the fog
        // far plane so the sky gets a full froxel march instead of zero fog.
        gpu.barriers(&next_barriers())?;
        gpu.begin_color_pass(
            &[scene.color, scene.view_depth],
            Some(scene.depth),
            &[[0.62, 0.86, 1.20, 1.0], [FOG_FAR, 0.0, 0.0, 0.0]],
            Some(1.0),
        )?;
        draw_set(
            &mut self.world,
            gpu,
            *color_pso,
            false,
            true,
            view_proj,
            true,
            &mut cb,
        )?;
        draw_set(
            &mut self.world,
            gpu,
            *two_sided_pso,
            true,
            true,
            view_proj,
            true,
            &mut cb,
        )?;
        gpu.end_color_pass()?;
        gpu.mark("scene");

        // Rain, default-on: streaks over the scene, masked by the rain map so
        // they fall in the nave and not through the arcade roof. Linear in and
        // linear out -- the fog composite still owns the tonemap.
        let rain_cb = RainCb {
            inv_view_proj: view_proj.inverse(),
            view_proj,
            camera_pos: Vec4::new(camera.eye.x, camera.eye.y, camera.eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            sky_zenith: Vec4::new(1.10, 1.20, 1.45, 0.0),
            sky_horizon: Vec4::new(0.95, 1.02, 1.20, 0.0),
            inv_extent: Vec2::new(1.0 / w, 1.0 / h),
            scene_color: gpu.bindless_index(scene.color)?,
            scene_depth: gpu.bindless_index(scene.view_depth)?,
            rain_map: gpu.bindless_index(rain_map)?,
            rain_map_vp: rain_vp,
            ..Default::default()
        };
        gpu.write_frame_bytes(rain_cb.as_bytes())?;
        gpu.barriers(&next_barriers())?;
        gpu.begin_color_pass(&[scene.rained], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(rain_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;
        gpu.mark("rain");

        // Fog, default-on (roadmap fase 5): the inject reads the same cascades the
        // scene did, so the shafts through the arcade cost no extra pass.
        let fog_cb = FogCb {
            inv_view: view.inverse(),
            camera_pos: Vec4::new(camera.eye.x, camera.eye.y, camera.eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            sun_color: Vec4::new(4.0, 3.6, 3.1, 1.0),
            fog: Vec4::new(0.020, 0.09, 0.60, 0.0),
            froxel: Vec4::new(FOG_NEAR, FOG_FAR, (camera.fov_y * 0.5).tan(), camera.aspect),
            misc: Vec4::new(
                halton2(info.frame_index as u32),
                harpia_render::FROXEL_D as f32,
                1.0,
                0.0,
            ),
            scene_color: gpu.bindless_index(scene.rained)?,
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
        // As barreiras vêm **antes** da pass que protegem. Na primeira versão deste
        // porte ficaram onde estavam os `storage_barrier` antigos — a seguir aos
        // dispatches — e a que protege o `scatter` saía depois de já ter sido lido.
        gpu.barriers(&next_barriers())?;
        let (ix, iy, iz) = inject_dispatch();
        gpu.set_compute_pipeline(inject_pso)?;
        gpu.bind_compute_bindless()?;
        gpu.dispatch(ix, iy, iz)?;
        gpu.barriers(&next_barriers())?;
        let (gx, gy, gz) = integrate_dispatch();
        gpu.set_compute_pipeline(integrate_pso)?;
        gpu.bind_compute_bindless()?;
        gpu.dispatch(gx, gy, gz)?;
        gpu.mark("fog froxels");

        gpu.barriers(&next_barriers())?;
        gpu.begin_color_pass(&[scene.composite], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(apply_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;
        gpu.mark("fog apply");

        cb.gbuf0 = gpu.bindless_index(scene.composite)?;
        cb.alpha_cutoff = 0.0;
        gpu.write_frame_bytes(cb.as_bytes())?;
        gpu.barriers(&next_barriers())?;
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
