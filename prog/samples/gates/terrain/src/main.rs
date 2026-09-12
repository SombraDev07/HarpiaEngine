//! Fase 6, gate do terreno: clipmap por `SV_VertexID`.
//!
//! Sem vertex buffer e sem index buffer — o roadmap pede assim, e a razão é que a
//! malha de um clipmap é sempre a mesma grelha, e o VS calcula a posição a partir
//! do índice do vértice.
//!
//! Cada nível está dividido em 64 patches, e o culling é em compute: a CPU não
//! percorre patches nenhuns, despacha uma vez e submete **um** `drawIndirect`.
//! É o ponto 7.3 da comparação com a Dagor, que faz este culling em CPU.
//!
//! A câmara voa (WASD) e mantém-se acima do chão usando a **mesma** função de
//! altura que o VS desenha. Se as duas discordarem vê-se logo: a câmara atravessa
//! o terreno ou flutua. É o ensaio do gate `heightquery`, que vem a seguir.

use anyhow::{Context, Result};
use harpia_app::{AppConfig, Sample, run};
use harpia_math::{Mat4, Vec2, Vec3, Vec4};
use harpia_render::{
    Access, CLIPMAP_LEVELS, CLIPMAP_N, CLIPMAP_PATCH, FIELD_STREAM_SIDE, FlyCamera,
    GBUFFER_DEPTH_FORMAT, Load, Pass, PassPlan, RenderGraph, TILE_N, TerrainCb, TileUpload,
    ToroidalField, clipmap_patch_bounds, clipmap_patch_count, clipmap_patch_vertex_count,
    clipmap_patches_per_level, clipmap_range, clipmap_vertex_count, color_desc, depth_desc,
    terrain_height,
};
use harpia_rhi::{
    Buffer, ComputePipeline, ComputePipelineDesc, Device, Extent2D, Format, FrameInfo, Gpu,
    GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets, Texture, TextureDesc, TextureDim,
};
use harpia_scene::{Bounds, Frustum};

const SLOT_VISIBLE: u32 = 0;
const SLOT_ARGS: u32 = 1;
const SLOT_BOUNDS: u32 = 2;
/// Amostras dos tiles a caminho da textura, e para onde vai cada um.
const SLOT_STAGED: u32 = 3;
const SLOT_SLOTS: u32 = 4;
/// Slot do campo no heap de storage images (set 4). Este gate não usa mais nenhum.
const FIELD_UAV_SLOT: u32 = 0;
/// Tiles por dispatch. 64 × 256 KiB = 16 MiB de staging, e a janela inteira (289)
/// enche em 5 frames — durante os quais o VS continua no FBM, que é o que evita
/// desenhar buracos enquanto o campo não está todo lá.
const UPLOAD_TILES: usize = 64;

/// O que o `field_upload.cs` lê.
#[repr(C)]
#[derive(Clone, Copy)]
struct UploadCb {
    /// x = slot do UAV, y = tiles neste dispatch, z = lado do tile
    params: Vec4,
}

/// O que o `terrain_bounds.cs` lê: que níveis refazer, e onde eles estão.
#[repr(C)]
#[derive(Clone, Copy)]
struct BoundsCb {
    camera_pos: Vec4,
    /// x = célula base, y = N, z = lado do patch, w = nº de patches
    params: Vec4,
    /// x = patches por nível, y = níveis a refazer
    counts: Vec4,
    /// Quais, um por componente.
    levels: [Vec4; 2],
}

/// O que o `terrain_cull.cs` lê. Separado do `TerrainCb` porque o compute não
/// precisa de sol nem de céu, e os planos não cabiam lá dentro sem o refazer.
#[repr(C)]
#[derive(Clone, Copy)]
struct CullCb {
    planes: [Vec4; 6],
    camera_pos: Vec4,
    /// x = célula base, y = N, z = lado do patch, w = nº de patches
    params: Vec4,
    /// x = vértices por patch, y = patches por nível
    counts: Vec4,
}

fn one<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((v as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}

fn as_bytes<T>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const FOV_Y: f32 = 60.0;
/// Quanto a câmara se mantém acima do chão quando bate nele.
const EYE_CLEARANCE: f32 = 2.0;
/// Linear: o blit é que faz o tonemap, como no resto da árvore.
const HDR: Format = Format::Rgba16Float;
const HDR_FORMATS: [Format; 1] = [HDR];

struct TerrainGate {
    pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    cull_pso: Option<ComputePipeline>,
    bounds_pso: Option<ComputePipeline>,
    visible_buf: Option<Buffer>,
    args_buf: Option<Buffer>,
    bounds_buf: Option<Buffer>,
    /// O centro a que cada nível fez snap da última vez. Um patch só muda de
    /// região do mundo quando este valor muda, e é por isso que as caixas não
    /// precisam de ser refeitas todos os frames.
    last_centre: [Option<Vec2>; CLIPMAP_LEVELS as usize],
    /// Quantos níveis se refizeram, somados — para o gate dizer o que poupou.
    levels_rebuilt: u64,
    frames_counted: u64,
    color: Option<Texture>,
    depth: Option<Texture>,
    extent: Extent2D,
    cam: FlyCamera,
    checked: bool,
    /// `-- --no-cull`: desenha os 448 patches. O controlo que prova que o culling
    /// não muda a imagem — se mudasse, estaria a cortar chão que se vê.
    no_cull: bool,
    /// `-- --static`: a câmara não se mexe.
    ///
    /// O gate faz a câmara voar de propósito — um clipmap parado não prova que o
    /// snap funciona. Mas isso torna **cada frame um mundo diferente**, e medir a
    /// mediana de uma mistura não responde a «ler uma textura custa mais que
    /// avaliar FBM»: a dispersão dentro de uma corrida (0.03 a 0.61 ms) era muito
    /// maior que a diferença entre os dois caminhos. Com a câmara quieta, os 112
    /// frames desenham o mesmo, e o que sobra é ruído de medição.
    still: bool,
    /// `-- --field`: a altura vem do campo cozido em vez do FBM avaliado no VS.
    ///
    /// Opt-in enquanto não estiver medido. O caminho analítico fica a ser o
    /// controlo do A/B: as duas imagens têm de ser idênticas ao pixel, porque as
    /// amostras do campo **são** `terrain_height` nos mesmos pontos.
    field: bool,
    field_tex: Option<Texture>,
    /// A janela que segue a câmara. Decide o que entra; não desenha nada.
    field_win: ToroidalField,
    /// Tiles à espera de subir. A primeira volta são 289 e sobem 64 por frame.
    field_pending: std::collections::VecDeque<TileUpload>,
    staged_buf: Option<Buffer>,
    slots_buf: Option<Buffer>,
    upload_pso: Option<ComputePipeline>,
    /// Só depois de a janela estar toda lá é que o VS lê o campo. Antes disso
    /// lê o FBM: um campo meio subido daria buracos, não um erro.
    field_ready: bool,
    field_uploads: u64,
    /// Do último frame, para a CPU refazer o mesmo culling e comparar.
    last_view_proj: Mat4,
    last_camera_xz: Vec2,
}

impl Default for TerrainGate {
    fn default() -> Self {
        Self {
            pso: None,
            blit_pso: None,
            cull_pso: None,
            bounds_pso: None,
            visible_buf: None,
            args_buf: None,
            bounds_buf: None,
            last_centre: [None; CLIPMAP_LEVELS as usize],
            levels_rebuilt: 0,
            frames_counted: 0,
            checked: false,
            no_cull: false,
            still: false,
            field: false,
            field_tex: None,
            field_win: ToroidalField::new(),
            field_pending: std::collections::VecDeque::new(),
            staged_buf: None,
            slots_buf: None,
            upload_pso: None,
            field_ready: false,
            field_uploads: 0,
            last_view_proj: Mat4::IDENTITY,
            last_camera_xz: Vec2::ZERO,
            color: None,
            depth: None,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
            cam: FlyCamera {
                speed: 40.0,
                boost: 8.0,
                fov_y: FOV_Y.to_radians(),
                near: 0.5,
                // O far tem de cobrir o último nível, senão o clipmap é cortado
                // pelo frustum antes de acabar e vê-se a borda.
                far: clipmap_range() * 1.5,
                ..FlyCamera::looking_at(Vec3::new(0.0, 90.0, 0.0), Vec3::new(120.0, 60.0, -120.0))
            },
        }
    }
}

impl TerrainGate {
    /// Declara o frame como um grafo, e devolve as barreiras já derivadas.
    ///
    /// Antes disto havia dois `storage_barrier_buffer` escritos à mão, e um deles
    /// teve de ser **alargado** quando o `terrain_bounds.cs` passou a escrever o
    /// que o `terrain_cull.cs` lê — um compute a alimentar outro compute. O modo
    /// de falha sem essa barreira é intermitente, que é o pior de todos.
    ///
    /// Aqui as passes declaram o que tocam e as barreiras saem disso. Se amanhã
    /// alguém puser outra pass pelo meio, elas mudam sozinhas.
    ///
    /// `bounds_stale` diz se o `terrain_bounds.cs` corre neste frame. Quando não
    /// corre, a barreira bounds→cull também não faz falta — e é essa a diferença
    /// entre um grafo e uma lista fixa de barreiras.
    fn build_graph(
        &self,
        bounds_stale: bool,
        field_upload: bool,
    ) -> Result<(RenderGraph, Vec<PassPlan>)> {
        let mut g = RenderGraph::new();
        // As caixas sobrevivem entre frames: é a cache que faz o culling valer a
        // pena. Marcá-las persistentes é o que diz ao grafo que podem ser lidas
        // num frame em que ninguém as escreveu.
        let bounds = g.persistent_buffer("bounds", self.bounds_buf.context("bounds")?);
        let visible = g.buffer("visible", self.visible_buf.context("visible")?);
        let args = g.buffer("args", self.args_buf.context("args")?);
        let color = g.texture("color", self.color.context("color")?);
        let depth = g.texture("depth", self.depth.context("depth")?);

        // O campo sobrevive entre frames e na maioria deles ninguém lhe toca:
        // persistente, senão o grafo recusa-o lido sem escritor neste frame.
        let field = self.field_tex.map(|t| g.persistent_texture("field", t));

        if field_upload {
            if let Some(f) = field {
                g.pass(Pass::new("field").uses(f, Access::StorageWrite));
            }
        }
        if bounds_stale {
            g.pass(Pass::new("bounds").uses(bounds, Access::StorageWrite));
        }
        g.pass(
            Pass::new("cull")
                .uses(bounds, Access::StorageRead)
                .uses(visible, Access::StorageWrite)
                .uses(args, Access::StorageWrite),
        );
        // O campo é lido **no vertex shader**; o `StorageRead` dá o layout GENERAL
        // e uma barreira cujo destino inclui o estágio de vértices.
        let mut terrain = Pass::new("terrain")
            .uses(args, Access::Indirect)
            .uses(visible, Access::StorageRead)
            .uses(color, Access::ColorWrite)
            .uses(depth, Access::DepthWrite)
            .load(color, Load::Clear([0.62, 0.72, 0.86, 1.0]))
            .load(depth, Load::Clear([1.0, 0.0, 0.0, 0.0]));
        if let Some(f) = field {
            // `Sampled` e não `StorageRead`: muda o layout de GENERAL para
            // SHADER_READ_ONLY antes desta passe, e de volta antes do próximo
            // upload. Entrou como sonda para explicar um custo que afinal era
            // ruído de medição, e **fica** porque é a configuração com que os
            // 0.127 ms foram medidos. A variante `StorageRead` nunca foi medida
            // com a instrumentação nova — está por comparar, não por preferir
            // (D62).
            terrain = terrain.uses(f, Access::Sampled);
        }
        g.pass(terrain);
        g.pass(Pass::new("blit").uses(color, Access::Sampled));

        if let Err(errors) = g.validate() {
            let msg = errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            anyhow::bail!("render graph inválido: {msg}");
        }
        let plans = g.compile();
        Ok((g, plans))
    }

    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.color = Some(gpu.create_texture(&color_desc(w, h, HDR))?);
        self.depth = Some(gpu.create_texture(&depth_desc(w, h))?);
        self.extent = Extent2D {
            width: w,
            height: h,
        };
        Ok(())
    }
}

impl Sample for TerrainGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/terrain.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/terrain.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &HDR_FORMATS,
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    depth_test: true,
                    cull_back: true,
                    ..Default::default()
                },
            })
            .context("terrain PSO")?,
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
        self.cull_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/terrain_cull.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("terrain cull CS")?,
        );
        self.bounds_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/terrain_bounds.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("terrain bounds CS")?,
        );
        self.visible_buf = Some(gpu.create_storage_buffer(
            SLOT_VISIBLE,
            as_bytes(&vec![0u32; clipmap_patch_count() as usize]),
        )?);
        // (lo, hi) por patch.
        self.bounds_buf = Some(gpu.create_storage_buffer(
            SLOT_BOUNDS,
            as_bytes(&vec![0f32; clipmap_patch_count() as usize * 2]),
        )?);
        // `VkDrawIndirectCommand`: quatro u32.
        self.args_buf = Some(gpu.create_storage_buffer(SLOT_ARGS, as_bytes(&[0u32; 4]))?);

        if self.field {
            // Storage **e** amostrada: o compute escreve por `imageStore` e o VS
            // lê por `texelFetch`. Fica em GENERAL nos dois papéis (mina 4).
            let tex = gpu.create_texture(&TextureDesc {
                width: FIELD_STREAM_SIDE,
                height: FIELD_STREAM_SIDE,
                depth_slices: 1,
                dim: TextureDim::D2,
                mip_levels: 1,
                format: Format::R32Float,
                sampled: true,
                storage: true,
                color_attachment: false,
                depth: false,
            })?;
            gpu.bind_storage_image(tex, 0, FIELD_UAV_SLOT)?;
            self.field_tex = Some(tex);

            let tile_floats = (TILE_N * TILE_N) as usize;
            self.staged_buf = Some(gpu.create_storage_buffer(
                SLOT_STAGED,
                as_bytes(&vec![0f32; tile_floats * UPLOAD_TILES]),
            )?);
            self.slots_buf = Some(gpu.create_storage_buffer(
                SLOT_SLOTS,
                as_bytes(&vec![0u32; 4 * UPLOAD_TILES]),
            )?);
            self.upload_pso = Some(
                gpu.create_compute_pipeline(&ComputePipelineDesc {
                    cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/field_upload.cs.spv")),
                    cs_entry: "CSMain",
                })
                .context("field upload CS")?,
            );

            // A primeira volta: coze a janela toda à volta de onde a câmara começa.
            let t0 = std::time::Instant::now();
            let first = self.field_win.advance(self.cam.position.x, self.cam.position.z);
            let bake_ms = t0.elapsed().as_secs_f32() * 1000.0;
            tracing::info!(
                tiles = first.len(),
                lado = FIELD_STREAM_SIDE,
                mib = FIELD_STREAM_SIDE as usize * FIELD_STREAM_SIDE as usize * 4 / (1024 * 1024),
                bake_ms = format!("{bake_ms:.0}"),
                "campo de altura cozido (janela toroidal)"
            );
            self.field_pending.extend(first);
        }

        self.recreate(gpu, gpu.extent())?;
        tracing::info!(
            levels = CLIPMAP_LEVELS,
            patches = clipmap_patch_count(),
            verts_por_patch = clipmap_patch_vertex_count(),
            verts_por_nivel = clipmap_vertex_count(),
            alcance = clipmap_range(),
            "clipmap"
        );
        Ok(())
    }

    fn update(&mut self, input: &harpia_app::SampleInput, dt: f32) {
        self.cam.update(input, dt);
        // A mesma função que o VS desenha. Se divergirem isto passa a ver-se.
        let ground = terrain_height(self.cam.position.x, self.cam.position.z);
        self.cam.position.y = self.cam.position.y.max(ground + EYE_CLEARANCE);
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.color.map_or_else(Vec::new, |c| vec![("terrain", c)])
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let pso = self.pso.as_ref().context("pso")?;
        let blit_pso = self.blit_pso.as_ref().context("blit pso")?;
        let color = self.color.context("color")?;
        let depth = self.depth.context("depth")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        // Sem input (todo o modo gate) a câmara está parada, portanto anda-se de
        // propósito: um clipmap parado não prova que o snap funciona.
        if info.frame_index > 0 && !self.still {
            let t = info.frame_index as f32 * 0.9;
            self.cam.position.x = t;
            self.cam.position.z = -t * 0.6;
            let ground = terrain_height(self.cam.position.x, self.cam.position.z);
            self.cam.position.y = (ground + 45.0).max(70.0);
        }
        let camera = self.cam.camera(w / h);
        let sun = Vec3::new(0.42, 0.70, -0.58).normalize();

        // A janela segue a câmara. Decidir o que entra é barato; cozer é o que
        // custa, e só acontece quando a câmara atravessa um tile. Tudo o que mexe
        // em `self` fica aqui, antes dos empréstimos que o resto do frame faz.
        let mut upload: Vec<TileUpload> = Vec::new();
        if self.field {
            let entered = self.field_win.advance(camera.eye.x, camera.eye.z);
            self.field_pending.extend(entered);
            let take = UPLOAD_TILES.min(self.field_pending.len());
            for _ in 0..take {
                if let Some(t) = self.field_pending.pop_front() {
                    upload.push(t);
                }
            }
            if !upload.is_empty() && self.field_pending.is_empty() {
                self.field_ready = true;
            }
        }
        self.field_uploads += upload.len() as u64;

        let mut cb = TerrainCb {
            view_proj: camera.view_proj(),
            camera_pos: Vec4::new(camera.eye.x, camera.eye.y, camera.eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            inv_extent: Vec2::new(1.0 / w, 1.0 / h),
            sky_horizon: Vec4::new(0.62, 0.72, 0.86, clipmap_range() * 0.85),
            // 0 quando não há campo: o slot 0 é o dummy e o VS lê isso como
            // «avalia o FBM». E **só** depois de a janela estar toda residente:
            // um campo meio subido daria buracos, que é pior que ser mais lento.
            field: match (self.field_ready, self.field_tex) {
                (true, Some(t)) => gpu.bindless_index(t)?,
                _ => 0,
            },
            ..Default::default()
        };

        let args = self.args_buf.context("args")?;
        let cull_pso = self.cull_pso.as_ref().context("cull pso")?;
        self.last_view_proj = camera.view_proj();
        self.last_camera_xz = Vec2::new(camera.eye.x, camera.eye.z);

        // O contador volta a zero antes do dispatch: o compute só o incrementa.
        gpu.write_storage_buffer(args, as_bytes(&[clipmap_patch_vertex_count(), 0u32, 0, 0]))?;

        let frustum = Frustum::from_view_proj(self.last_view_proj);
        let cull = CullCb {
            planes: frustum.planes(),
            camera_pos: cb.camera_pos,
            params: Vec4::new(
                harpia_render::CLIPMAP_CELL,
                CLIPMAP_N as f32,
                CLIPMAP_PATCH as f32,
                clipmap_patch_count() as f32,
            ),
            counts: Vec4::new(
                clipmap_patch_vertex_count() as f32,
                clipmap_patches_per_level() as f32,
                0.0,
                0.0,
            ),
        };
        // Que níveis mudaram de sítio? Só esses precisam de caixas novas.
        let mut stale: Vec<u32> = Vec::new();
        for level in 0..CLIPMAP_LEVELS {
            let snap = harpia_render::CLIPMAP_CELL * (1u32 << level) as f32 * 2.0;
            let centre = Vec2::new(
                (self.last_camera_xz.x / snap).floor() * snap,
                (self.last_camera_xz.y / snap).floor() * snap,
            );
            if self.last_centre[level as usize] != Some(centre) {
                self.last_centre[level as usize] = Some(centre);
                stale.push(level);
            }
        }
        self.levels_rebuilt += stale.len() as u64;
        self.frames_counted += 1;

        // O frame declarado como grafo. As barreiras que se seguem vêm daqui, e
        // não de raciocínio caso a caso.
        let (_graph, plans) = self.build_graph(!stale.is_empty(), !upload.is_empty())?;
        let mut plan = plans.into_iter();
        let mut next_barriers = move || plan.next().map(|p| p.barriers).unwrap_or_default();

        // Os tiles que entraram vão para a textura por compute: um workgroup de
        // 16×16 por bloco, uma camada do dispatch por tile.
        if !upload.is_empty() {
            let upload_pso = self.upload_pso.as_ref().context("upload pso")?;
            let staged = self.staged_buf.context("staged")?;
            let slots_buf = self.slots_buf.context("slots")?;
            let mut floats: Vec<u8> =
                Vec::with_capacity(upload.len() * (TILE_N * TILE_N) as usize * 4);
            let mut slots: Vec<u32> = Vec::with_capacity(upload.len() * 4);
            for t in &upload {
                floats.extend_from_slice(&t.bytes);
                slots.extend_from_slice(&[t.slot.0, t.slot.1, 0, 0]);
            }
            gpu.write_storage_buffer(staged, &floats)?;
            gpu.write_storage_buffer(slots_buf, as_bytes(&slots))?;
            let ucb = UploadCb {
                params: Vec4::new(
                    FIELD_UAV_SLOT as f32,
                    upload.len() as f32,
                    TILE_N as f32,
                    0.0,
                ),
            };
            gpu.write_frame_bytes(one(&ucb))?;
            gpu.set_compute_pipeline(upload_pso)?;
            gpu.bind_compute_bindless()?;
            gpu.barriers(&next_barriers())?;
            gpu.dispatch(TILE_N / 16, TILE_N / 16, upload.len() as u32)?;
        }
        gpu.mark("field");

        if !stale.is_empty() {
            let bounds_pso = self.bounds_pso.as_ref().context("bounds pso")?;
            let mut levels = [Vec4::ZERO; 2];
            for (i, &l) in stale.iter().enumerate() {
                levels[i / 4][i % 4] = l as f32;
            }
            let upd = BoundsCb {
                camera_pos: cb.camera_pos,
                params: Vec4::new(
                    harpia_render::CLIPMAP_CELL,
                    CLIPMAP_N as f32,
                    CLIPMAP_PATCH as f32,
                    clipmap_patch_count() as f32,
                ),
                counts: Vec4::new(
                    clipmap_patches_per_level() as f32,
                    stale.len() as f32,
                    0.0,
                    0.0,
                ),
                levels,
            };
            gpu.write_frame_bytes(one(&upd))?;
            gpu.set_compute_pipeline(bounds_pso)?;
            gpu.bind_compute_bindless()?;
            gpu.barriers(&next_barriers())?;
            gpu.dispatch(stale.len() as u32 * clipmap_patches_per_level(), 1, 1)?;
        }
        gpu.mark("bounds");

        if self.no_cull {
            let all: Vec<u32> = (0..clipmap_patch_count()).collect();
            gpu.write_storage_buffer(self.visible_buf.context("visible")?, as_bytes(&all))?;
            gpu.write_storage_buffer(
                args,
                as_bytes(&[clipmap_patch_vertex_count(), clipmap_patch_count(), 0, 0]),
            )?;
        } else {
            gpu.write_frame_bytes(one(&cull))?;
            gpu.barriers(&next_barriers())?;
            gpu.set_compute_pipeline(cull_pso)?;
            gpu.bind_compute_bindless()?;
            // Um workgroup por patch: as 81 amostras de altura repartem-se pelas lanes.
            gpu.dispatch(clipmap_patch_count(), 1, 1)?;
        }
        gpu.mark("cull");

        gpu.write_frame_bytes(cb.as_bytes())?;

        // O clear é radiância do céu, não uma cor: o alvo é linear.
        gpu.barriers(&next_barriers())?;
        gpu.begin_color_pass(&[color], Some(depth), &[[0.62, 0.72, 0.86, 1.0]], Some(1.0))?;
        gpu.set_pipeline(pso)?;
        gpu.bind_graphics_bindless()?;
        // **Um** draw para o clipmap inteiro, e a CPU não sabe quantos patches
        // saem dele: quem escolheu foi o compute.
        gpu.draw_indirect(args, 0, 1)?;
        gpu.end_color_pass()?;
        gpu.mark("terrain");

        gpu.barriers(&next_barriers())?;
        cb.scene = gpu.bindless_index(color)?;
        gpu.write_frame_bytes(cb.as_bytes())?;
        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0])?;
        gpu.set_pipeline(blit_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }

    /// O compute escolheu bem? A CPU refaz o mesmo culling e compara.
    fn finish(&mut self, gpu: &mut Gpu) -> Result<()> {
        if self.checked {
            return Ok(());
        }
        self.checked = true;

        let args = gpu.read_buffer(self.args_buf.context("args")?, 16)?;
        let word = |b: &[u8], i: usize| {
            u32::from_ne_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]])
        };
        let gpu_count = word(&args, 1);
        let total = clipmap_patch_count();

        // A **mesma** caixa que o compute calcula: altura exacta nos mesmos
        // (PATCH+1)² vértices. Comparar contra uma caixa diferente deixaria
        // qualquer erro esconder-se na folga entre os dois testes.
        let frustum = Frustum::from_view_proj(self.last_view_proj);
        let keep = |id: u32| {
            clipmap_patch_bounds(id, self.last_camera_xz).is_some_and(|(lo, hi)| {
                frustum.intersects(&Bounds {
                    center: (lo + hi) * 0.5,
                    extents: (hi - lo) * 0.5,
                })
            })
        };
        let cpu_count = (0..total).filter(|&id| keep(id)).count() as u32;

        let drawn = gpu_count * clipmap_patch_vertex_count();
        let all = clipmap_vertex_count() * CLIPMAP_LEVELS;
        tracing::info!(
            patches = total,
            visiveis_gpu = gpu_count,
            visiveis_cpu = cpu_count,
            cortados = total - gpu_count,
            percent_cortado = format!("{:.1}", 100.0 * (total - gpu_count) as f32 / total as f32),
            verts_desenhados = drawn,
            verts_sem_culling = all,
            "culling de patches em compute"
        );

        anyhow::ensure!(
            word(&args, 0) == clipmap_patch_vertex_count()
                && word(&args, 2) == 0
                && word(&args, 3) == 0,
            "campos fixos do comando errados: {:?}",
            (word(&args, 0), word(&args, 2), word(&args, 3))
        );
        anyhow::ensure!(gpu_count > 0, "o culling cortou o terreno todo");
        if self.no_cull {
            anyhow::ensure!(gpu_count == total, "o controlo tem de desenhar os {total}");
            tracing::info!("controlo `--no-cull`: os {total} patches desenhados");
            return Ok(());
        }
        anyhow::ensure!(
            gpu_count < total,
            "o culling não cortou patch nenhum — não está a fazer nada"
        );
        anyhow::ensure!(
            gpu_count == cpu_count,
            "a GPU guardou {gpu_count} patches e a CPU {cpu_count} com o mesmo teste"
        );

        // O contador pode estar certo e a lista ser lixo.
        let list = gpu.read_buffer(self.visible_buf.context("visible")?, gpu_count as usize * 4)?;
        let mut seen = vec![false; total as usize];
        for k in 0..gpu_count as usize {
            let id = word(&list, k);
            anyhow::ensure!(id < total, "patch {id} fora do clipmap, na posição {k}");
            anyhow::ensure!(!seen[id as usize], "patch {id} repetido na lista");
            seen[id as usize] = true;
            anyhow::ensure!(keep(id), "a GPU guardou o patch {id} e o frustum rejeita-o");
        }
        tracing::info!(
            patches = gpu_count,
            "lista: todos únicos e dentro do frustum"
        );
        // O que a cache das caixas poupa: um nível só se refaz quando muda de snap.
        tracing::info!(
            niveis = CLIPMAP_LEVELS,
            refeitos_por_frame = format!(
                "{:.2}",
                self.levels_rebuilt as f64 / self.frames_counted.max(1) as f64
            ),
            percent = format!(
                "{:.0}",
                100.0 * self.levels_rebuilt as f64
                    / (self.frames_counted.max(1) * CLIPMAP_LEVELS as u64) as f64
            ),
            "caixas recalculadas"
        );
        Ok(())
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — terrain".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    let no_cull = config.extra.iter().any(|a| a == "--no-cull");
    let field = config.extra.iter().any(|a| a == "--field");
    let still = config.extra.iter().any(|a| a == "--static");
    run(
        config,
        TerrainGate {
            no_cull,
            field,
            still,
            ..TerrainGate::default()
        },
    )
}
