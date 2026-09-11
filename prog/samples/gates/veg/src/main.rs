//! Fase 6, gate `veg`: vegetação cortada por frustum **e por oclusão**.
//!
//! O gate `instances` provou o culling por frustum em compute. Este acrescenta a
//! parte que falta e que é a diferença entre desenhar tudo o que está no ecrã e
//! desenhar só o que se vê: **culling por oclusão**, contra uma pirâmide de
//! profundidade construída no mesmo frame.
//!
//! ## Como a Dagor faz, e porque é que aqui não é assim
//!
//! A erva deles (`grass_generate.dshl`) corta por **feedback do pixel shader**:
//! desenha-se tudo, o PS marca num bitvector qual a instância que passou o teste
//! de profundidade, e um compute compacta esse bitvector numa lista densa. O PS
//! deles tem **três caminhos de código** com intrínsecas de wave só para reduzir o
//! tráfego de atómicos, e mesmo assim o shader de compactação abre com
//! `if (!hardware.dx12 || hardware.xbox || hardware.scarlett) dont_render;` — ou
//! seja, está desligado fora do DX12 de desktop.
//!
//! Aqui o teste é feito **antes** de rasterizar. Um prepass desenha os oclusores,
//! constrói-se uma pirâmide de máximos, e o compute projecta a caixa de cada
//! planta e compara. Três consequências:
//!
//! * **um atómico por instância que sobrevive**, em vez de um por fragmento;
//! * **nenhuma intrínseca de wave**, portanto nada desligado por plataforma;
//! * a pirâmide é do **mesmo frame**, portanto não há o frame de atraso que um
//!   Hi-Z do frame anterior traz — nem o pop-in que vem com ele.
//!
//! ## E a prova, que é a parte que eles não publicam
//!
//! Culling por oclusão que corta de mais não dá erro nenhum: dá geometria que
//! desaparece, e num campo de vegetação ninguém dá por isso. Por isso o gate
//! corre as duas versões e compara: **com oclusão e sem oclusão têm de dar a mesma
//! imagem**. Se o teste cortar uma planta que se vê, há pixels diferentes.

use anyhow::{Context, Result};
use harpia_app::{AppConfig, Sample, run};
use harpia_math::{Mat4, Vec3, Vec4, perspective_vk};
use harpia_render::{
    Access, GBUFFER_DEPTH_FORMAT, Load, Pass, PassPlan, RenderGraph, color_desc, depth_desc,
};
use harpia_rhi::{
    Buffer, ComputePipeline, ComputePipelineDesc, Device, Extent2D, Format, FrameInfo, Gpu,
    GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets, Texture, TextureDesc, TextureDim,
};
use harpia_scene::Frustum;

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

/// Plantas. Muitas, e a maior parte atrás dos muros — senão não há oclusão para
/// medir e o gate não prova nada.
const PLANTS: u32 = 60_000;
/// Seis vértices: um quad virado à câmara.
const PLANT_VERTS: u32 = 6;
/// 36 vértices por caixa.
const BOX_VERTS: u32 = 36;
const HDR: Format = Format::Rgba16Float;
const HDR_FORMATS: [Format; 1] = [HDR];
const FOV_Y: f32 = 60.0;
const NEAR: f32 = 0.5;
const FAR: f32 = 400.0;

const SLOT_PLANTS: u32 = 0;
const SLOT_VISIBLE: u32 = 1;
const SLOT_ARGS: u32 = 2;
const SLOT_BOXES: u32 = 3;
/// O segundo par, para a referência sem oclusão.
const SLOT_VISIBLE_REF: u32 = 4;
const SLOT_ARGS_REF: u32 = 5;

#[repr(C)]
#[derive(Clone, Copy)]
struct Plant {
    pos_scale: Vec4,
    color: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Box {
    centre_pad: Vec4,
    half_extent: Vec4,
    color: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SceneCb {
    view_proj: Mat4,
    camera_pos: Vec4,
    sun_dir: Vec4,
    params: Vec4,
    hiz: Vec4,
    slots: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CullCb {
    planes: [Vec4; 6],
    view_proj: Mat4,
    params: Vec4,
    hiz: Vec4,
    slots: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HizCb {
    params: Vec4,
    src_size: Vec4,
}

fn as_bytes<T>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

fn one<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((v as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}

/// O chão e uma fila de muros.
///
/// Os muros existem para haver oclusão a sério: sem alguma coisa à frente, o
/// culling por oclusão não corta nada e a comparação com/sem não mede nada.
fn boxes() -> Vec<Box> {
    let mut out = vec![Box {
        centre_pad: Vec4::new(0.0, -1.0, -60.0, 0.0),
        half_extent: Vec4::new(140.0, 1.0, 140.0, 0.0),
        color: Vec4::new(0.30, 0.31, 0.28, 1.0),
    }];
    for i in 0..7 {
        let f = i as f32;
        out.push(Box {
            centre_pad: Vec4::new(-48.0 + f * 16.0, 7.0, -26.0 - (f * 1.7).sin() * 5.0, 0.0),
            half_extent: Vec4::new(6.5, 8.0, 1.2, 0.0),
            color: Vec4::new(0.52, 0.47, 0.42, 1.0),
        });
    }
    out
}

/// Plantas espalhadas sobre o chão, a maior parte para lá dos muros.
fn plants() -> Vec<Plant> {
    (0..PLANTS)
        .map(|i| {
            let f = i as f32;
            let x = (f * 0.7311).sin() * 120.0;
            let z = -12.0 - (f * 0.3137).fract() * 150.0;
            Plant {
                pos_scale: Vec4::new(x, 0.0, z, 0.5 + (f * 1.93).sin().abs() * 0.5),
                color: Vec4::new(
                    0.18 + 0.22 * (f * 2.1).sin().abs(),
                    0.42 + 0.30 * (f * 1.3).sin().abs(),
                    0.14 + 0.14 * (f * 3.7).sin().abs(),
                    1.0,
                ),
            }
        })
        .collect()
}

struct Targets {
    /// Com oclusão e sem oclusão, lado a lado: o gate compara-os todos os frames.
    color: Texture,
    color_ref: Texture,
    depth: Texture,
    /// Profundidade copiada para R32F, porque o depth buffer não é de storage.
    hiz: Texture,
    hiz_mips: u32,
    width: u32,
    height: u32,
}

#[derive(Default)]
struct Veg {
    box_pso: Option<GraphicsPipeline>,
    veg_pso: Option<GraphicsPipeline>,
    hiz_pso: Option<ComputePipeline>,
    cull_pso: Option<ComputePipeline>,
    plant_buf: Option<Buffer>,
    visible_buf: Option<Buffer>,
    visible_ref_buf: Option<Buffer>,
    args_buf: Option<Buffer>,
    args_ref_buf: Option<Buffer>,
    box_buf: Option<Buffer>,
    plants: Vec<Plant>,
    boxes: Vec<Box>,
    rt: Option<Targets>,
    extent: Extent2D,
    checked: bool,
    last_view_proj: Mat4,
    /// `-- --no-occlusion` desliga o teste Hi-Z, para o A/B.
    occlusion: bool,
}

impl Veg {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        // Níveis até 1×1. O culling escolhe o que cobre o rectângulo projectado,
        // e sem os níveis de cima uma planta grande obrigaria a centenas de
        // leituras.
        let mips = 32 - w.max(h).leading_zeros();
        self.rt = Some(Targets {
            color: gpu.create_texture(&color_desc(w, h, HDR))?,
            color_ref: gpu.create_texture(&color_desc(w, h, HDR))?,
            // `depth_desc` dá `sampled: false`, e a pirâmide precisa de ler o
            // depth buffer. Sem isto a transição para SHADER_READ_ONLY é ilegal:
            // a imagem não tem o uso SAMPLED.
            depth: gpu.create_texture(&TextureDesc {
                sampled: true,
                ..depth_desc(w, h)
            })?,
            hiz: gpu.create_texture(&TextureDesc {
                width: w,
                height: h,
                depth_slices: 1,
                dim: TextureDim::D2,
                mip_levels: mips,
                format: Format::R32Float,
                sampled: true,
                storage: true,
                color_attachment: false,
                depth: false,
            })?,
            hiz_mips: mips,
            width: w,
            height: h,
        });
        self.extent = Extent2D {
            width: w,
            height: h,
        };
        Ok(())
    }

    /// O frame declarado como grafo.
    fn build_graph(&self) -> Result<Vec<PassPlan>> {
        let mut g = RenderGraph::new();
        let rt = self.rt.as_ref().context("rt")?;
        let boxes = g.persistent_buffer("boxes", self.box_buf.context("boxes")?);
        let plants = g.persistent_buffer("plants", self.plant_buf.context("plants")?);
        let visible = g.buffer("visible", self.visible_buf.context("visible")?);
        let args = g.buffer("args", self.args_buf.context("args")?);
        let color = g.texture("color", rt.color);
        let depth = g.texture("depth", rt.depth);
        let hiz = g.texture("hi-z", rt.hiz);

        // 1. Oclusores: cor e profundidade de uma vez. A pirâmide sai daqui, por
        //    isso não há prepass separado — o Hi-Z é do **mesmo** frame.
        g.pass(
            Pass::new("occluders")
                .uses(boxes, Access::StorageRead)
                .uses(color, Access::ColorWrite)
                .uses(depth, Access::DepthWrite)
                .load(color, Load::Clear([0.42, 0.55, 0.70, 1.0]))
                .load(depth, Load::Clear([1.0, 0.0, 0.0, 0.0])),
        );
        // 2. A pirâmide, nível a nível.
        for level in 0..rt.hiz_mips {
            let mut p = Pass::new("hi-z").uses(hiz, Access::StorageWrite);
            if level == 0 {
                p = p.uses(depth, Access::Sampled);
            } else {
                p = p.uses(hiz, Access::StorageRead);
            }
            g.pass(p);
        }
        // 3. O culling corre **duas vezes**: com oclusão e sem. A segunda é a
        //    referência contra a qual a primeira é julgada.
        let visible_ref = g.buffer("visible-ref", self.visible_ref_buf.context("vis ref")?);
        let args_ref = g.buffer("args-ref", self.args_ref_buf.context("args ref")?);
        let color_ref = g.texture("color-ref", rt.color_ref);
        for (name, v, a) in [("cull", visible, args), ("cull-ref", visible_ref, args_ref)] {
            g.pass(
                Pass::new(name)
                    .uses(plants, Access::StorageRead)
                    .uses(hiz, Access::Sampled)
                    .uses(v, Access::StorageWrite)
                    .uses(a, Access::StorageWrite),
            );
        }
        // 4. E a vegetação entra por cima dos oclusores, nos dois alvos.
        //    O alvo de referência leva primeiro uma cópia dos oclusores, senão a
        //    comparação misturava «sem vegetação» com «sem oclusores».
        g.pass(
            Pass::new("veg")
                .uses(plants, Access::StorageRead)
                .uses(visible, Access::StorageRead)
                .uses(args, Access::Indirect)
                .uses(color, Access::ColorWrite)
                .uses(depth, Access::DepthWrite)
                .load(color, Load::Keep)
                .load(depth, Load::Keep),
        );
        // O alvo de referência refaz o ciclo todo — oclusores com limpeza de
        // profundidade, depois a vegetação — para a comparação ser entre duas
        // imagens completas e não entre uma completa e meia.
        g.pass(
            Pass::new("occluders-ref")
                .uses(boxes, Access::StorageRead)
                .uses(color_ref, Access::ColorWrite)
                .uses(depth, Access::DepthWrite)
                .load(color_ref, Load::Clear([0.42, 0.55, 0.70, 1.0]))
                .load(depth, Load::Clear([1.0, 0.0, 0.0, 0.0])),
        );
        g.pass(
            Pass::new("veg-ref")
                .uses(plants, Access::StorageRead)
                .uses(visible_ref, Access::StorageRead)
                .uses(args_ref, Access::Indirect)
                .uses(color_ref, Access::ColorWrite)
                .uses(depth, Access::DepthWrite)
                .load(color_ref, Load::Keep)
                .load(depth, Load::Keep),
        );
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
}

impl Sample for Veg {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        let box_targets = PipelineTargets {
            color_formats: &HDR_FORMATS,
            depth_format: Some(GBUFFER_DEPTH_FORMAT),
            depth_test: true,
            cull_back: true,
            ..Default::default()
        };
        self.box_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/box.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/box.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: box_targets,
            })
            .context("box PSO")?,
        );
        self.veg_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/veg.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/veg.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &HDR_FORMATS,
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    depth_test: true,
                    // Um quad virado à câmara é de uma face só: cortar as de trás
                    // faria desaparecer metade conforme o ângulo.
                    cull_back: false,
                    ..Default::default()
                },
            })
            .context("veg PSO")?,
        );
        self.hiz_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/hiz.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("hiz CS")?,
        );
        self.cull_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/cull.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("cull CS")?,
        );

        self.boxes = boxes();
        self.plants = plants();
        self.box_buf = Some(gpu.create_storage_buffer(SLOT_BOXES, as_bytes(&self.boxes))?);
        self.plant_buf = Some(gpu.create_storage_buffer(SLOT_PLANTS, as_bytes(&self.plants))?);
        self.visible_buf =
            Some(gpu.create_storage_buffer(SLOT_VISIBLE, as_bytes(&vec![0u32; PLANTS as usize]))?);
        self.args_buf = Some(gpu.create_storage_buffer(SLOT_ARGS, as_bytes(&[0u32; 4]))?);
        self.visible_ref_buf = Some(
            gpu.create_storage_buffer(SLOT_VISIBLE_REF, as_bytes(&vec![0u32; PLANTS as usize]))?,
        );
        self.args_ref_buf = Some(gpu.create_storage_buffer(SLOT_ARGS_REF, as_bytes(&[0u32; 4]))?);

        self.recreate(gpu, gpu.extent())?;
        tracing::info!(plantas = PLANTS, oclusores = self.boxes.len(), "veg");
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.rt.as_ref().map_or_else(Vec::new, |rt| {
            vec![("veg", rt.color), ("veg-sem-oclusao", rt.color_ref)]
        })
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let (color, depth, hiz, mips, hw, hh) =
            (rt.color, rt.depth, rt.hiz, rt.hiz_mips, rt.width, rt.height);
        let rt_color_ref = rt.color_ref;
        let box_pso = self.box_pso.as_ref().context("box pso")?.clone();
        let veg_pso = self.veg_pso.as_ref().context("veg pso")?.clone();
        let hiz_pso = self.hiz_pso.as_ref().context("hiz pso")?.clone();
        let cull_pso = self.cull_pso.as_ref().context("cull pso")?.clone();
        let args = self.args_buf.context("args")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        // A câmara anda para a frente: o que estava tapado pelos muros vai
        // aparecendo, que é onde um culling por oclusão demasiado agressivo se
        // denuncia.
        let t = info.frame_index as f32 * 0.12;
        let eye = Vec3::new(t.sin() * 6.0, 4.0, 26.0 - t * 0.8);
        let view = Mat4::look_at_rh(eye, eye + Vec3::new(0.0, -0.12, -1.0), Vec3::Y);
        let view_proj = perspective_vk(FOV_Y.to_radians(), w / h, NEAR, FAR) * view;
        self.last_view_proj = view_proj;
        let sun = Vec3::new(0.35, 0.82, 0.45).normalize();

        let mut plans = self.build_graph()?.into_iter();
        let mut next = move || plans.next().map(|p| p.barriers).unwrap_or_default();

        let hiz_index = gpu.bindless_index(hiz)?;
        let scene_cb = SceneCb {
            view_proj,
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            params: Vec4::new(self.boxes.len() as f32, BOX_VERTS as f32, 0.0, 0.0),
            hiz: Vec4::new(hiz_index as f32, hw as f32, hh as f32, mips as f32),
            slots: Vec4::ZERO,
        };

        // 1. Oclusores.
        gpu.write_frame_bytes(one(&scene_cb))?;
        gpu.barriers(&next())?;
        gpu.begin_color_pass(&[color], Some(depth), &[[0.42, 0.55, 0.70, 1.0]], Some(1.0))?;
        gpu.set_pipeline(&box_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(BOX_VERTS, self.boxes.len() as u32, 0, 0)?;
        gpu.end_color_pass()?;
        gpu.mark("occluders");

        // 2. A pirâmide.
        let depth_index = gpu.bindless_index(depth)?;
        for level in 0..mips {
            let dw = (hw >> level).max(1);
            let dh = (hh >> level).max(1);
            let (sw, sh) = if level == 0 {
                (hw, hh)
            } else {
                ((hw >> (level - 1)).max(1), (hh >> (level - 1)).max(1))
            };
            let cb = HizCb {
                params: Vec4::new(
                    if level == 0 { depth_index } else { hiz_index } as f32,
                    dw as f32,
                    dh as f32,
                    if level == 0 { 1.0 } else { 0.0 },
                ),
                // z = o slot onde este nível está ligado; é o próprio nível.
                src_size: Vec4::new(
                    sw as f32,
                    sh as f32,
                    level as f32,
                    // O mip de onde se lê: o anterior. No nível 0 lê-se o depth
                    // buffer, que só tem um.
                    level.saturating_sub(1) as f32,
                ),
            };
            gpu.write_frame_bytes(one(&cb))?;
            gpu.barriers(&next())?;
            gpu.bind_storage_image(hiz, level, level)?;
            gpu.set_compute_pipeline(&hiz_pso)?;
            gpu.bind_compute_bindless()?;
            gpu.dispatch(dw.div_ceil(8), dh.div_ceil(8), 1)?;
        }
        gpu.mark("hi-z");

        // 3. O culling.
        gpu.write_storage_buffer(args, as_bytes(&[PLANT_VERTS, 0u32, 0, 0]))?;
        let frustum = Frustum::from_view_proj(view_proj);
        let cull = CullCb {
            planes: frustum.planes(),
            view_proj,
            params: Vec4::new(
                PLANTS as f32,
                PLANT_VERTS as f32,
                if self.occlusion { 1.0 } else { 0.0 },
                0.0,
            ),
            hiz: Vec4::new(hiz_index as f32, hw as f32, hh as f32, mips as f32),
            slots: Vec4::ZERO,
        };
        let args_ref = self.args_ref_buf.context("args ref")?;
        gpu.write_storage_buffer(args_ref, as_bytes(&[PLANT_VERTS, 0u32, 0, 0]))?;
        for (occ, vslot, aslot) in [
            (self.occlusion, SLOT_VISIBLE, SLOT_ARGS),
            (false, SLOT_VISIBLE_REF, SLOT_ARGS_REF),
        ] {
            let mut c = cull;
            c.params.z = if occ { 1.0 } else { 0.0 };
            c.slots = Vec4::new(vslot as f32, aslot as f32, 0.0, 0.0);
            gpu.write_frame_bytes(one(&c))?;
            gpu.barriers(&next())?;
            gpu.set_compute_pipeline(&cull_pso)?;
            gpu.bind_compute_bindless()?;
            gpu.dispatch(PLANTS.div_ceil(64), 1, 1)?;
        }
        gpu.mark("cull");

        // 4. A vegetação, por cima, com o depth dos oclusores.
        let mut veg_cb = scene_cb;
        veg_cb.slots = Vec4::new(SLOT_VISIBLE as f32, 0.0, 0.0, 0.0);
        gpu.write_frame_bytes(one(&veg_cb))?;
        gpu.barriers(&next())?;
        gpu.begin_color_pass(&[color], Some(depth), &[], None)?;
        gpu.set_pipeline(&veg_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw_indirect(args, 0, 1)?;
        gpu.end_color_pass()?;
        gpu.mark("veg");

        // 5. E a referência: o mesmo ciclo, sem oclusão.
        gpu.write_frame_bytes(one(&scene_cb))?;
        gpu.barriers(&next())?;
        gpu.begin_color_pass(
            &[rt_color_ref],
            Some(depth),
            &[[0.42, 0.55, 0.70, 1.0]],
            Some(1.0),
        )?;
        gpu.set_pipeline(&box_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(BOX_VERTS, self.boxes.len() as u32, 0, 0)?;
        gpu.end_color_pass()?;

        veg_cb.slots = Vec4::new(SLOT_VISIBLE_REF as f32, 0.0, 0.0, 0.0);
        gpu.write_frame_bytes(one(&veg_cb))?;
        gpu.barriers(&next())?;
        gpu.begin_color_pass(&[rt_color_ref], Some(depth), &[], None)?;
        gpu.set_pipeline(&veg_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw_indirect(args_ref, 0, 1)?;
        gpu.end_color_pass()?;
        gpu.mark("veg-ref");

        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0])?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }

    /// Quantas plantas sobreviveram, e a CPU concorda no frustum?
    fn finish(&mut self, gpu: &mut Gpu) -> Result<()> {
        if self.checked {
            return Ok(());
        }
        self.checked = true;

        let data = gpu.read_buffer(self.args_buf.context("args")?, 16)?;
        let gpu_count = u32::from_ne_bytes([data[4], data[5], data[6], data[7]]);

        // O mesmo teste de frustum, na CPU. Não inclui a oclusão — essa não se
        // pode refazer em CPU sem rasterizar, e é por isso que a prova dela é a
        // comparação de imagens e não um número.
        let planes = Frustum::from_view_proj(self.last_view_proj).planes();
        let frustum_count = self
            .plants
            .iter()
            .filter(|p| {
                let s = p.pos_scale.w;
                let lo = p.pos_scale.truncate() + Vec3::new(-s * 0.5, 0.0, -s * 0.5);
                let hi = p.pos_scale.truncate() + Vec3::new(s * 0.5, s * 2.0, s * 0.5);
                let c = (lo + hi) * 0.5;
                let e = (hi - lo) * 0.5;
                planes.iter().all(|pl| {
                    let n = Vec3::new(pl.x, pl.y, pl.z);
                    let r = e.x * n.x.abs() + e.y * n.y.abs() + e.z * n.z.abs();
                    n.dot(c) + pl.w + r >= 0.0
                })
            })
            .count() as u32;

        tracing::info!(
            plantas = PLANTS,
            oclusao = self.occlusion,
            visiveis = gpu_count,
            so_frustum_cpu = frustum_count,
            cortadas_pela_oclusao = frustum_count.saturating_sub(gpu_count),
            percent_oclusao = format!(
                "{:.1}",
                100.0 * frustum_count.saturating_sub(gpu_count) as f32
                    / frustum_count.max(1) as f32
            ),
            "culling de vegetação"
        );

        // A prova que interessa: cortar por oclusão **não pode mudar a imagem**.
        //
        // O contador concordar não chega — um teste demasiado agressivo tira erva
        // que se vê, e num campo de vegetação ninguém dá por isso a olho. As duas
        // imagens são renderizadas no mesmo frame, com o mesmo depth e a mesma
        // geometria; a única diferença é o teste Hi-Z.
        let rt = self.rt.as_ref().context("rt")?;
        let (with, without) = (rt.color, rt.color_ref);
        let a = gpu.read_texture(with)?;
        let b = gpu.read_texture(without)?;
        let n = a.bytes.len().min(b.bytes.len());
        let mut differing = 0usize;
        let mut worst = 0u16;
        for i in (0..n).step_by(2) {
            let x = u16::from_ne_bytes([a.bytes[i], a.bytes[i + 1]]);
            let y = u16::from_ne_bytes([b.bytes[i], b.bytes[i + 1]]);
            if x != y {
                differing += 1;
                worst = worst.max(x.abs_diff(y));
            }
        }
        let channels = n / 2;
        let pct = 100.0 * differing as f64 / channels.max(1) as f64;
        tracing::info!(
            canais = channels,
            diferentes = differing,
            percent = format!("{pct:.4}"),
            maior_diferenca = worst,
            "com oclusão contra sem oclusão"
        );
        // O limiar é **contado**, não escolhido.
        //
        // O código certo dá zero quase sempre: cinco corridas deram 0, 0, 0, 0, 3.
        // Os três canais são a lista de sobreviventes mudar de ordem e duas folhas
        // à mesma profundidade trocarem de vencedor — não é erva a desaparecer.
        //
        // Do outro lado: a pirâmide a ler sempre o mip 0 (o bug que este gate
        // apanhou) dá **176**, e a guardar o mínimo em vez do máximo dá **803**.
        // 32 fica dez vezes acima do ruído e cinco vezes abaixo do erro mais
        // pequeno que se quer apanhar. Um limiar sobre a percentagem, que foi o que
        // escrevi primeiro, deixava passar os 176.
        const MAX_DIFFERING: usize = 32;
        anyhow::ensure!(
            differing <= MAX_DIFFERING,
            "a oclusão mudou {differing} canais ({pct:.4}%, máximo {MAX_DIFFERING}): \
             está a cortar vegetação que se vê"
        );

        anyhow::ensure!(
            gpu_count > 0,
            "cortou tudo: não se desenhou vegetação nenhuma"
        );
        anyhow::ensure!(
            gpu_count < PLANTS,
            "não cortou nada: o culling não está a fazer o seu trabalho"
        );
        if self.occlusion {
            anyhow::ensure!(
                gpu_count <= frustum_count,
                "a oclusão deixou passar {gpu_count} e o frustum sozinho só {frustum_count}"
            );
            anyhow::ensure!(
                frustum_count - gpu_count > 0,
                "a oclusão não cortou uma única planta além do frustum: ou não está \
                 ligada, ou o teste Hi-Z nunca dá verdadeiro"
            );
        } else {
            anyhow::ensure!(
                gpu_count == frustum_count,
                "sem oclusão a GPU devia dar o mesmo que o frustum em CPU: \
                 {gpu_count} contra {frustum_count}"
            );
        }
        Ok(())
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — veg".into();
    let user_set = std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set {
        config.max_frames = std::num::NonZeroU32::new(24);
    }
    let occlusion = !config.extra.iter().any(|a| a == "--no-occlusion");
    run(
        config,
        Veg {
            occlusion,
            ..Default::default()
        },
    )
}
