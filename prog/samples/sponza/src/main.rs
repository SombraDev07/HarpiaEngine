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
    Buffer, ComputePipeline, ComputePipelineDesc, Device, Extent2D, Format, FrameInfo, Gpu,
    GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets, Texture,
};
use harpia_scene::{
    ActiveFrustum, Bounds, CullStats, Frustum, Material, Mesh, MeshRange, TwoSided, WorldTransform,
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
    vb: Option<Buffer>,
    ib: Option<Buffer>,
    /// A cena toda como dados: uma entrada por primitiva, e uma lista fixa de
    /// comandos indirectos cujo `instanceCount` o culling escreve.
    prim_buf: Option<Buffer>,
    args_buf: Option<Buffer>,
    args_all_buf: Option<Buffer>,
    args_p2_buf: Option<Buffer>,
    seen_buf: Option<Buffer>,
    hiz_pso: Option<ComputePipeline>,
    hiz: Option<Texture>,
    hiz_mips: u32,
    /// `-- --occlusion` **liga** a segunda fase.
    ///
    /// Desligada por omissão, e medida: nesta cena custa mais do que poupa.
    /// 103 primitivas com ~15 000 triângulos cada é a granularidade errada para
    /// culling por oclusão — corta 3 de 78 e a pirâmide custa 0.033 ms, o que dá
    /// um frame 6% mais lento. O caminho está escrito e verificado; o que falta é
    /// dividir as primitivas em meshlets (D57).
    occlusion: bool,
    cull_pso: Option<ComputePipeline>,
    /// Quantas primitivas, e onde acabam as de uma face só.
    prim_count: u32,
    one_sided_count: u32,
    last_cpu_visible: u32,
    /// As caixas dos meshlets, em CPU. São a referência do culling em compute —
    /// o `Visible` do ECS é por primitiva e já não serve para comparar.
    meshlet_bounds: Vec<(Vec3, Vec3)>,
    /// Triângulos por meshlet. `usize::MAX` = um meshlet por primitiva, que é o
    /// que a medição diz ser o melhor com draws indirectos clássicos (D58).
    meshlet_tris: usize,
    /// `-- --mesh` desenha a cena com mesh shaders em vez de draws indirectos.
    mesh_path: bool,
    /// Tecto de vértices únicos por meshlet: 64 com mesh shaders, sem limite sem eles.
    max_verts: usize,
    mesh_pso: Option<GraphicsPipeline>,
    mesh_pso_two_sided: Option<GraphicsPipeline>,
    mvert_buf: Option<Buffer>,
    mtri_buf: Option<Buffer>,
    range_buf: Option<Buffer>,
    vert_storage: Option<Buffer>,
    checked: bool,
    scene_extent: Extent2D,
}

impl Default for Sponza {
    fn default() -> Self {
        Self {
            prim_buf: None,
            args_buf: None,
            args_all_buf: None,
            args_p2_buf: None,
            seen_buf: None,
            hiz_pso: None,
            hiz: None,
            hiz_mips: 0,
            occlusion: false,
            cull_pso: None,
            prim_count: 0,
            one_sided_count: 0,
            last_cpu_visible: 0,
            meshlet_bounds: Vec::new(),
            meshlet_tris: usize::MAX,
            mesh_path: false,
            max_verts: usize::MAX,
            mesh_pso: None,
            mesh_pso_two_sided: None,
            mvert_buf: None,
            mtri_buf: None,
            range_buf: None,
            vert_storage: None,
            checked: false,
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
            vb: None,
            ib: None,
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

        let prims = g.persistent_buffer("prims", self.prim_buf.context("prims")?);
        let args = g.persistent_buffer("args", self.args_buf.context("args")?);
        let hiz_r = g.texture("hi-z", self.hiz.context("hiz")?);
        let args_p2 = g.persistent_buffer("args-p2", self.args_p2_buf.context("args p2")?);
        // Persistente: é o que o frame anterior deixou, e é isso que a fase 1 usa.
        let seen = g.persistent_buffer("seen", self.seen_buf.context("seen")?);
        g.pass(
            Pass::new("cull")
                .uses(prims, Access::StorageRead)
                .uses(seen, Access::StorageRead)
                .uses(args, Access::StorageWrite),
        );
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
                .uses(args, Access::Indirect)
                .uses(atlas, Access::Sampled)
                .uses(color, Access::ColorWrite)
                .uses(view_depth, Access::ColorWrite)
                .uses(depth, Access::DepthWrite)
                .load(color, Load::Clear([0.0; 4]))
                .load(view_depth, Load::Clear([0.0; 4]))
                .load(depth, Load::Clear([1.0, 0.0, 0.0, 0.0])),
        );
        if self.occlusion {
            for level in 0..self.hiz_mips {
                let mut p = Pass::new("hi-z").uses(hiz_r, Access::StorageWrite);
                p = if level == 0 {
                    p.uses(depth, Access::Sampled)
                } else {
                    p.uses(hiz_r, Access::StorageRead)
                };
                g.pass(p);
            }
            g.pass(
                Pass::new("cull 2")
                    .uses(prims, Access::StorageRead)
                    .uses(hiz_r, Access::Sampled)
                    .uses(seen, Access::StorageWrite)
                    .uses(args_p2, Access::StorageWrite),
            );
            g.pass(
                Pass::new("scene 2")
                    .uses(args_p2, Access::Indirect)
                    .uses(color, Access::ColorWrite)
                    .uses(view_depth, Access::ColorWrite)
                    .uses(depth, Access::DepthWrite)
                    .load(color, Load::Keep)
                    .load(view_depth, Load::Keep)
                    .load(depth, Load::Keep),
            );
        }
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
            // `sampled: true` porque a pirâmide lê o depth buffer; o
            // `depth_desc` normal não o permite.
            depth: gpu.create_texture(&harpia_rhi::TextureDesc {
                sampled: true,
                ..depth_desc(w, h)
            })?,
            rained: gpu.create_texture(&color_desc(w, h, SCENE_FORMAT))?,
            composite: gpu.create_texture(&color_desc(w, h, COMPOSITE))?,
        });
        let mips = 32 - w.max(h).leading_zeros();
        self.hiz = Some(gpu.create_texture(&harpia_rhi::TextureDesc {
            width: w,
            height: h,
            depth_slices: 1,
            dim: harpia_rhi::TextureDim::D2,
            mip_levels: mips,
            format: Format::R32Float,
            sampled: true,
            storage: true,
            color_attachment: false,
            depth: false,
        })?);
        self.hiz_mips = mips;
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
/// Desenha um intervalo da lista de comandos com **um** draw indirecto.
///
/// Era um `draw_indexed` por primitiva, com uma escrita de CBV por cada uma para
/// o pixel shader saber o seu albedo — 596 draws por frame. Agora a matriz e o
/// material vêm de uma tabela que o VS indexa pelo `firstInstance` do comando, e
/// cada pass é um draw.
///
/// As primitivas estão ordenadas com as de uma face primeiro, porque são dois
/// PSOs e cada um quer um intervalo contíguo.
#[allow(clippy::too_many_arguments)]
fn draw_indirect_set(
    gpu: &mut Gpu,
    pso: &GraphicsPipeline,
    two_sided: bool,
    view_proj: Mat4,
    vb: Buffer,
    ib: Buffer,
    args: Buffer,
    one_sided: u32,
    total: u32,
) -> Result<()> {
    let (start, count) = if two_sided {
        (one_sided, total - one_sided)
    } else {
        (0, one_sided)
    };
    if count == 0 {
        return Ok(());
    }
    gpu.set_pipeline(pso)?;
    gpu.bind_graphics_bindless()?;
    gpu.set_push_constants(PushConstants::new(view_proj).as_bytes())?;
    gpu.bind_vertex_buffer(vb, 0)?;
    gpu.bind_index_buffer(ib)?;
    // Cinco u32 por comando: o `stride` do lado do RHI já é esse.
    gpu.draw_indexed_indirect(args, start as u64 * 20, count)?;
    Ok(())
}

impl Sponza {
    /// O culling em compute concorda com o mesmo teste em CPU?
    ///
    /// O ECS continua a marcar `Visible` com a mesma caixa e os mesmos planos, e
    /// isso deixou de escolher o que se desenha — passou a ser a referência. Se os
    /// dois discordarem, um deles tem a caixa ou o plano errado, e o que se vê é
    /// geometria a desaparecer num sítio qualquer da cena.
    fn check_cull(&mut self, gpu: &mut Gpu) -> Result<()> {
        if self.checked {
            return Ok(());
        }
        self.checked = true;
        // Duas listas: a fase 1 (o que se via no frame anterior) e a fase 2 (o que
        // a oclusão deixou passar e ainda não tinha sido desenhado). Desenha-se a
        // **união** das duas.
        let prim_count = self.prim_count;
        let count = |gpu: &mut Gpu, buf: Buffer| -> Result<u32> {
            let bytes = gpu.read_buffer(buf, prim_count as usize * 20)?;
            let mut n = 0;
            for i in 0..prim_count as usize {
                let o = i * 20 + 4;
                let v = u32::from_ne_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
                anyhow::ensure!(
                    v <= 1,
                    "instanceCount {v} no comando {i}: o culling escreveu lixo"
                );
                n += v;
            }
            Ok(n)
        };
        let p1 = count(gpu, self.args_buf.context("args")?)?;
        let p2 = if self.occlusion {
            count(gpu, self.args_p2_buf.context("args p2")?)?
        } else {
            0
        };
        let gpu_visible = p1 + p2;
        tracing::info!(
            oclusao = self.occlusion,
            fase1 = p1,
            fase2 = p2,
            desenhadas = gpu_visible,
            so_frustum_cpu = self.last_cpu_visible,
            meshlets = self.prim_count,
            poupadas = self.last_cpu_visible.saturating_sub(gpu_visible),
            "culling de primitivas"
        );
        if self.occlusion {
            // O frustum é um majorante: a oclusão só pode tirar. Não é igualdade
            // porque a fase 1 desenha o que se via no frame anterior mesmo que
            // esteja tapado agora — é conservador de propósito e corrige-se no
            // frame seguinte.
            anyhow::ensure!(
                gpu_visible <= self.last_cpu_visible,
                "desenharam-se {gpu_visible} primitivas e o frustum só deixa passar {}",
                self.last_cpu_visible
            );
        } else {
            anyhow::ensure!(
                gpu_visible == self.last_cpu_visible,
                "sem oclusão o culling em compute devia dar o mesmo que a CPU: \
                 {gpu_visible} contra {}",
                self.last_cpu_visible
            );
        }
        anyhow::ensure!(
            gpu_visible > 0 && gpu_visible < self.prim_count,
            "o culling não cortou nada ou cortou tudo: {gpu_visible} de {}",
            self.prim_count
        );
        Ok(())
    }
}

/// Desenha um intervalo de meshlets com **um** dispatch de mesh tasks.
///
/// Sem lista de comandos: um workgroup por meshlet, e o próprio shader lê o
/// `instanceCount` que o culling escreveu para saber se emite alguma coisa. Era
/// este o custo que D58 mediu a bater o ganho — ~0.1 ms por cada mil comandos de
/// draw — e aqui não há nenhum.
fn draw_mesh_set(
    gpu: &mut Gpu,
    pso: &GraphicsPipeline,
    two_sided: bool,
    view_proj: Mat4,
    one_sided: u32,
    total: u32,
) -> Result<()> {
    let (base, count) = if two_sided {
        (one_sided, total - one_sided)
    } else {
        (0, one_sided)
    };
    if count == 0 {
        return Ok(());
    }
    gpu.set_pipeline(pso)?;
    gpu.bind_graphics_bindless()?;
    // A segunda matriz leva o meshlet base: o dispatch não tem offset de workgroup.
    let base_m = Mat4::from_cols(
        Vec4::new(base as f32, 0.0, 0.0, 0.0),
        Vec4::ZERO,
        Vec4::ZERO,
        Vec4::ZERO,
    );
    gpu.set_push_constants(PushConstants::with_world(view_proj, base_m).as_bytes())?;
    gpu.draw_mesh_tasks(count, 1, 1)?;
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

const SLOT_PRIMS: u32 = 0;
const SLOT_ARGS: u32 = 1;
const SLOT_ARGS_ALL: u32 = 2;
const SLOT_ARGS_P2: u32 = 3;
const SLOT_SEEN: u32 = 4;
// Os quatro buffers do formato canónico, para o mesh shader.
const SLOT_VERTS: u32 = 5;
const SLOT_MVERTS: u32 = 6;
const SLOT_MTRIS: u32 = 7;
const SLOT_RANGES: u32 = 8;

/// Uma primitiva, como a GPU a vê.
#[repr(C)]
#[derive(Clone, Copy)]
struct PrimGpu {
    world: Mat4,
    /// x = índice bindless do albedo, y = corte de alfa
    material: Vec4,
    centre: Vec4,
    extents: Vec4,
}

fn as_bytes<T>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

impl Sample for Sponza {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        let path = sponza_gltf();
        let cpu = load_gltf(&path).map_err(|e| anyhow::anyhow!("{e}"))?;
        let textures = upload_images(gpu, &cpu)?;

        // **Um** vertex buffer e **um** index buffer para a cena toda.
        //
        // Eram 103 primitivas com um par de buffers cada, e um draw por primitiva
        // com o VB e o IB a serem religados entre cada um: 596 draws por frame,
        // 0.36 ms de CPU. Um draw indirecto múltiplo precisa de um só VB e um só
        // IB — os índices não são rebaseados, cada comando leva o seu
        // `vertexOffset`, que é exactamente para isto que ele existe.
        // E partida em **meshlets**: grupos de 128 triângulos com a sua própria
        // caixa. 103 primitivas com ~15 000 triângulos cada não dão ao culling por
        // oclusão nada para cortar — uma primitiva desse tamanho quase nunca está
        // inteiramente tapada, e medido dava 3 de 78 (D57). Com meshlets a unidade
        // de teste fica da ordem de grandeza das plantas do `gate-veg`, onde a
        // mesma máquina corta 86%.
        let mut all_verts: Vec<harpia_render::MeshVertex> = Vec::new();
        let mut all_idx: Vec<u32> = Vec::new();
        let mut ranges: Vec<(u32, u32, i32)> = Vec::new(); // first_index, count, vertex_offset
        let mut meshlets: Vec<harpia_render::Meshlet> = Vec::new();
        for (i, p) in cpu.prims.iter().enumerate() {
            let vertex_offset = all_verts.len() as i32;
            let base = all_idx.len() as u32;
            let (mut ml, idx) =
                harpia_render::build_meshlets(p, i as u32, self.meshlet_tris, self.max_verts);
            for m in &mut ml {
                m.first_index += base;
                m.vertex_offset = vertex_offset;
            }
            ranges.push((base, idx.len() as u32, vertex_offset));
            meshlets.extend_from_slice(&ml);
            all_verts.extend_from_slice(&p.vertices);
            all_idx.extend_from_slice(&idx);
        }
        // Storage **e** vertex buffer: o mesh shader lê os vértices como storage e
        // o caminho clássico liga o mesmo buffer como vertex. Duas cópias dos 6 MB
        // seria o preço de não o fazer.
        let vb = gpu.create_storage_buffer(SLOT_VERTS, unsafe {
            std::slice::from_raw_parts(
                all_verts.as_ptr().cast::<u8>(),
                std::mem::size_of_val(all_verts.as_slice()),
            )
        })?;
        self.vert_storage = Some(vb);
        let ib_handle = gpu.create_index_buffer(unsafe {
            std::slice::from_raw_parts(
                all_idx.as_ptr().cast::<u8>(),
                std::mem::size_of_val(all_idx.as_slice()),
            )
        })?;
        self.vb = Some(vb);
        self.ib = Some(ib_handle);
        let ib = ib_handle;
        tracing::info!(
            primitivas = cpu.prims.len(),
            meshlets = meshlets.len(),
            triangulos = all_idx.len() / 3,
            vertices = all_verts.len(),
            "geometria fundida e partida em meshlets"
        );

        let mut two_sided = 0;
        for (i, p) in cpu.prims.iter().enumerate() {
            let (first_index, index_count, vertex_offset) = ranges[i];
            let mesh = Mesh {
                vb,
                ib,
                index_count,
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
                MeshRange {
                    first_index,
                    vertex_offset,
                    prim: i as u32,
                },
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
        // A tabela por primitiva e a lista de comandos.
        //
        // As de uma face vêm primeiro e as de duas a seguir, porque são dois PSOs
        // diferentes: assim cada um desenha um intervalo contíguo da mesma lista,
        // com dois `drawIndexedIndirect` em vez de 103 `drawIndexed`.
        // Um comando por **meshlet**, com os de uma face primeiro: são dois PSOs e
        // cada um quer um intervalo contíguo. A tabela que o shader lê tem a mesma
        // forma de antes — a matriz e o material do meshlet são os da sua
        // primitiva — por isso **nenhum shader mudou** para isto.
        let two_sided_of = |m: &harpia_render::Meshlet| {
            let p = &cpu.prims[m.prim as usize];
            p.alpha_cutoff > 0.0 || p.double_sided
        };
        let mut order: Vec<usize> = (0..meshlets.len()).collect();
        order.sort_by_key(|&i| two_sided_of(&meshlets[i]) as u8);
        self.one_sided_count = order
            .iter()
            .filter(|&&i| !two_sided_of(&meshlets[i]))
            .count() as u32;
        self.prim_count = order.len() as u32;

        let mut prims: Vec<PrimGpu> = Vec::with_capacity(order.len());
        let mut args: Vec<u32> = Vec::with_capacity(order.len() * 5);
        for (slot, &i) in order.iter().enumerate() {
            let m = &meshlets[i];
            let p = &cpu.prims[m.prim as usize];
            prims.push(PrimGpu {
                world: p.world,
                material: Vec4::new(
                    gpu.bindless_index(textures[p.albedo])? as f32,
                    p.alpha_cutoff,
                    0.0,
                    0.0,
                ),
                centre: Vec4::new(m.centre[0], m.centre[1], m.centre[2], 0.0),
                extents: Vec4::new(m.extents[0], m.extents[1], m.extents[2], 0.0),
            });
            self.meshlet_bounds.push((
                Vec3::new(m.centre[0], m.centre[1], m.centre[2]),
                Vec3::new(m.extents[0], m.extents[1], m.extents[2]),
            ));
            args.extend_from_slice(&[
                m.index_count,
                1,
                m.first_index,
                m.vertex_offset as u32,
                // `firstInstance` = o índice na tabela. É por aqui que o VS sabe
                // qual é a sua matriz, sem `gl_DrawID`.
                slot as u32,
            ]);
        }
        // Só o mesh shader lê o formato canónico, e só com ele se constrói. Sem
        // `--mesh` há um meshlet por primitiva e nenhum tecto de vértices: o
        // empacotamento partia-as para caberem nos 64 vértices de saída, o `ensure!`
        // abaixo recusava arrancar, e a Sponza por omissão ficou partida em todos os
        // backends desde que o mesh shader entrou.
        if self.mesh_path {
            // O formato canónico, pela **mesma ordem** dos comandos: o mesh shader
            // usa `gl_WorkGroupID` como índice, portanto as duas tabelas têm de
            // estar alinhadas ou cada meshlet desenha a geometria de outro.
            let ordered: Vec<harpia_render::Meshlet> = order.iter().map(|&i| meshlets[i]).collect();
            let packed = harpia_render::pack_meshlets(&ordered, &all_idx);
            anyhow::ensure!(
                packed.meshlets.len() == ordered.len(),
                "o empacotamento partiu {} meshlets em {}: as tabelas deixariam de estar \
                 alinhadas com os comandos",
                ordered.len(),
                packed.meshlets.len()
            );
            // E verificado **antes** de subir: um `SetMeshOutputsEXT` com valores
            // acima do que o shader declarou pendura a GPU, e um GPU hang não diz
            // qual foi a linha. Melhor falhar aqui com o número.
            for (i, r) in packed.meshlets.iter().enumerate() {
                anyhow::ensure!(
                    r.vertex_count as usize <= harpia_render::MESHLET_VERTS
                        && r.triangle_count as usize <= harpia_render::MESHLET_PRIMS,
                    "meshlet {i} com {} vértices e {} triângulos: acima do que o mesh \
                     shader declara ({} e {})",
                    r.vertex_count,
                    r.triangle_count,
                    harpia_render::MESHLET_VERTS,
                    harpia_render::MESHLET_PRIMS
                );
                anyhow::ensure!(
                    r.vertex_offset as usize + r.vertex_count as usize <= packed.vertices.len()
                        && r.triangle_offset as usize + r.triangle_count as usize
                            <= packed.triangles.len(),
                    "meshlet {i} aponta para fora dos buffers"
                );
            }

            self.mvert_buf =
                Some(gpu.create_storage_buffer(SLOT_MVERTS, as_bytes(&packed.vertices))?);
            self.mtri_buf =
                Some(gpu.create_storage_buffer(SLOT_MTRIS, as_bytes(&packed.triangles))?);
            self.range_buf =
                Some(gpu.create_storage_buffer(SLOT_RANGES, as_bytes(&packed.meshlets))?);
        }

        self.prim_buf = Some(gpu.create_storage_buffer(SLOT_PRIMS, as_bytes(&prims))?);
        self.args_buf = Some(gpu.create_storage_buffer(SLOT_ARGS, as_bytes(&args))?);
        // Uma segunda lista, com `instanceCount` sempre a 1: as sombras e o mapa
        // de chuva **não** respeitam o culling da câmara, porque um caster fora do
        // ecrã continua a projectar sombra dentro dele.
        self.args_all_buf = Some(gpu.create_storage_buffer(SLOT_ARGS_ALL, as_bytes(&args))?);
        self.args_p2_buf = Some(gpu.create_storage_buffer(SLOT_ARGS_P2, as_bytes(&args))?);
        // Todos vistos no arranque: o primeiro frame desenha tudo na fase 1, e a
        // pirâmide dele já serve o frame seguinte.
        self.seen_buf =
            Some(gpu.create_storage_buffer(SLOT_SEEN, as_bytes(&vec![1u32; order.len()]))?);
        self.hiz_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/hiz.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("hiz CS")?,
        );
        if self.mesh_path {
            anyhow::ensure!(
                gpu.mesh_shaders(),
                "`-- --mesh` pedido e este device não tem VK_EXT_mesh_shader"
            );
            for (slot, cull_back) in [(0usize, true), (1, false)] {
                let pso = gpu
                    .create_mesh_pipeline(&harpia_rhi::MeshPipelineDesc {
                        ms_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/scene.mesh.spv")),
                        fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/color.ps.spv")),
                        ms_entry: "MSMain",
                        fs_entry: "PSMain",
                        targets: color_targets(cull_back),
                    })
                    .context("mesh PSO")?;
                if slot == 0 {
                    self.mesh_pso = Some(pso);
                } else {
                    self.mesh_pso_two_sided = Some(pso);
                }
            }
        }

        self.cull_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/cull.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("cull CS")?,
        );

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

    fn finish(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.check_cull(gpu)
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
        // A marca `Visible` já não escolhe o que se desenha — isso é do compute —
        // mas continua a correr porque é a **referência** contra a qual o culling
        // na GPU é verificado no `finish`.
        // A referência é o **mesmo** teste sobre as **mesmas** caixas, em CPU.
        let planes = Frustum::from_view_proj(view_proj).planes();
        self.last_cpu_visible = self
            .meshlet_bounds
            .iter()
            .filter(|(c, e)| {
                planes.iter().all(|pl| {
                    let n = Vec3::new(pl.x, pl.y, pl.z);
                    let r = e.x * n.x.abs() + e.y * n.y.abs() + e.z * n.z.abs();
                    n.dot(*c) + pl.w + r >= 0.0
                })
            })
            .count() as u32;

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

        // O que os draws indirectos precisam. Copiado para locais porque as
        // chamadas a seguir levam `&mut gpu` e não podem ter `self` emprestado.
        let (vb, ib) = (self.vb.context("vb")?, self.ib.context("ib")?);
        let args_culled = self.args_buf.context("args")?;
        let args_all = self.args_all_buf.context("args all")?;
        let one_sided = self.one_sided_count;
        let total = self.prim_count;

        let mut plans = self.build_graph()?.into_iter();
        let mut next_barriers = move || plans.next().map(|p| p.barriers).unwrap_or_default();

        // O culling da câmara, em compute: escreve `instanceCount` em cada comando.
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct CullCb {
            planes: [Vec4; 6],
            /// x = nº de primitivas, y = fase, z = slot dos comandos, w = slot do `seen`
            params: Vec4,
            view_proj: Mat4,
            hiz: Vec4,
        }
        let hiz = self.hiz.context("hiz")?;
        let hiz_index = gpu.bindless_index(hiz)? as f32;
        let (hw, hh, mips) = (
            self.scene_extent.width,
            self.scene_extent.height,
            self.hiz_mips,
        );
        let cull_base = CullCb {
            planes: Frustum::from_view_proj(view_proj).planes(),
            params: Vec4::new(total as f32, 1.0, SLOT_ARGS as f32, SLOT_SEEN as f32),
            view_proj,
            hiz: Vec4::new(hiz_index, hw as f32, hh as f32, mips as f32),
        };
        let write_cull = |gpu: &mut Gpu, c: &CullCb| -> Result<()> {
            gpu.write_frame_bytes(unsafe {
                std::slice::from_raw_parts(
                    (c as *const CullCb).cast::<u8>(),
                    std::mem::size_of::<CullCb>(),
                )
            })?;
            Ok(())
        };

        // Fase 1 do culling: frustum, e só o que se via no frame anterior.
        write_cull(gpu, &cull_base)?;
        gpu.barriers(&next_barriers())?;
        gpu.set_compute_pipeline(self.cull_pso.as_ref().context("cull pso")?)?;
        gpu.bind_compute_bindless()?;
        gpu.dispatch(total.div_ceil(64), 1, 1)?;
        gpu.mark("cull");

        gpu.barriers(&next_barriers())?;
        gpu.begin_color_pass(&[], Some(atlas), &[], Some(1.0))?;
        for i in 0..4 {
            let (x, y, tw, th) = csm.tile_viewport(i);
            gpu.set_viewport(x, y, tw, th)?;
            draw_indirect_set(
                gpu,
                shadow_pso,
                false,
                csm.view_proj[i],
                vb,
                ib,
                args_all,
                one_sided,
                total,
            )?;
            // Cutout casters need the albedo alpha, so they carry material.
            draw_indirect_set(
                gpu,
                shadow_cutout_pso,
                true,
                csm.view_proj[i],
                vb,
                ib,
                args_all,
                one_sided,
                total,
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
        draw_indirect_set(
            gpu, shadow_pso, false, rain_vp, vb, ib, args_all, one_sided, total,
        )?;
        draw_indirect_set(
            gpu,
            shadow_cutout_pso,
            true,
            rain_vp,
            vb,
            ib,
            args_all,
            one_sided,
            total,
        )?;
        gpu.end_color_pass()?;
        gpu.mark("rain map");

        // The clear is sky radiance, not a colour: the scene target is linear HDR
        // now and the tonemap happens in the fog apply. Depth clears to the fog
        // far plane so the sky gets a full froxel march instead of zero fog.
        // O CBV da iluminação, **uma** vez por pass.
        //
        // Era escrito por primitiva, porque o índice do albedo vivia lá dentro e
        // cada draw precisava do seu. Agora o albedo vem da tabela por primitiva —
        // mas o CBV continua a trazer o sol, as cascatas e o IBL, e sem esta
        // escrita a pass lia o que ficou da anterior. O sintoma foi a cena inteira
        // com um banho vermelho, com as texturas certas por baixo.
        gpu.write_frame_bytes(cb.as_bytes())?;
        gpu.barriers(&next_barriers())?;
        gpu.begin_color_pass(
            &[scene.color, scene.view_depth],
            Some(scene.depth),
            &[[0.62, 0.86, 1.20, 1.0], [FOG_FAR, 0.0, 0.0, 0.0]],
            Some(1.0),
        )?;
        // Fase 1: o que se via no frame anterior. É isto que enche o depth buffer
        // de que a pirâmide sai.
        if self.mesh_path {
            draw_mesh_set(
                gpu,
                self.mesh_pso.as_ref().context("mesh pso")?,
                false,
                view_proj,
                one_sided,
                total,
            )?;
            draw_mesh_set(
                gpu,
                self.mesh_pso_two_sided.as_ref().context("mesh pso 2")?,
                true,
                view_proj,
                one_sided,
                total,
            )?;
        } else {
            draw_indirect_set(
                gpu,
                color_pso,
                false,
                view_proj,
                vb,
                ib,
                args_culled,
                one_sided,
                total,
            )?;
            draw_indirect_set(
                gpu,
                two_sided_pso,
                true,
                view_proj,
                vb,
                ib,
                args_culled,
                one_sided,
                total,
            )?;
        }

        gpu.end_color_pass()?;
        gpu.mark("scene");

        if self.occlusion {
            #[repr(C)]
            #[derive(Clone, Copy)]
            struct HizCb {
                params: Vec4,
                src_size: Vec4,
            }
            let depth_index = gpu.bindless_index(scene.depth)? as f32;
            for level in 0..mips {
                let dw = (hw >> level).max(1);
                let dh = (hh >> level).max(1);
                let (sw, sh) = if level == 0 {
                    (hw, hh)
                } else {
                    ((hw >> (level - 1)).max(1), (hh >> (level - 1)).max(1))
                };
                let c = HizCb {
                    params: Vec4::new(
                        if level == 0 { depth_index } else { hiz_index },
                        dw as f32,
                        dh as f32,
                        if level == 0 { 1.0 } else { 0.0 },
                    ),
                    src_size: Vec4::new(
                        sw as f32,
                        sh as f32,
                        level as f32,
                        level.saturating_sub(1) as f32,
                    ),
                };
                gpu.write_frame_bytes(unsafe {
                    std::slice::from_raw_parts(
                        (&c as *const HizCb).cast::<u8>(),
                        std::mem::size_of::<HizCb>(),
                    )
                })?;
                gpu.barriers(&next_barriers())?;
                gpu.bind_storage_image(hiz, level, level)?;
                gpu.set_compute_pipeline(self.hiz_pso.as_ref().context("hiz pso")?)?;
                gpu.bind_compute_bindless()?;
                gpu.dispatch(dw.div_ceil(8), dh.div_ceil(8), 1)?;
            }
            gpu.mark("hi-z");

            let mut c2 = cull_base;
            c2.params.y = 2.0;
            c2.params.z = SLOT_ARGS_P2 as f32;
            write_cull(gpu, &c2)?;
            gpu.barriers(&next_barriers())?;
            gpu.set_compute_pipeline(self.cull_pso.as_ref().context("cull pso")?)?;
            gpu.bind_compute_bindless()?;
            gpu.dispatch(total.div_ceil(64), 1, 1)?;
            gpu.mark("cull 2");

            let args_p2 = self.args_p2_buf.context("args p2")?;
            gpu.write_frame_bytes(cb.as_bytes())?;
            gpu.barriers(&next_barriers())?;
            // Sem valores de limpeza: a fase 2 **acrescenta** ao que a 1 desenhou.
            gpu.begin_color_pass(
                &[scene.color, scene.view_depth],
                Some(scene.depth),
                &[],
                None,
            )?;
            draw_indirect_set(
                gpu, color_pso, false, view_proj, vb, ib, args_p2, one_sided, total,
            )?;
            draw_indirect_set(
                gpu,
                two_sided_pso,
                true,
                view_proj,
                vb,
                ib,
                args_p2,
                one_sided,
                total,
            )?;
            gpu.end_color_pass()?;
            gpu.mark("scene 2");
        }

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
    let occlusion = config.extra.iter().any(|a| a == "--occlusion");
    let mesh_path = config.extra.iter().any(|a| a == "--mesh");
    let meshlet_tris = config
        .extra
        .iter()
        .position(|a| a == "--meshlets")
        .and_then(|i| config.extra.get(i + 1))
        .map(|v| v.parse::<usize>())
        .transpose()
        .context("`--meshlets` quer um número de triângulos")?
        // Com mesh shaders o valor por omissão é o limite canónico: um meshlet por
        // primitiva não caberia nos 64 vértices de saída.
        .unwrap_or(if mesh_path {
            harpia_render::MESHLET_PRIMS
        } else {
            usize::MAX
        });
    run(
        config,
        Sponza {
            occlusion,
            mesh_path,
            // O tecto de vértices é do hardware quando há mesh shaders, e não
            // existe sem eles.
            max_verts: if mesh_path {
                harpia_render::MESHLET_VERTS
            } else {
                usize::MAX
            },
            meshlet_tris,
            ..Default::default()
        },
    )
}
