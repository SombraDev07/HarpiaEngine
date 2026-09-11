//! Fase 6, gate `instances`: 2500 cubos, culling em compute, **um** draw.
//!
//! A CPU escreve as caixas uma vez no arranque e nunca mais toca nelas. Todos os
//! frames um compute decide quais sobrevivem ao frustum, escreve a lista e
//! preenche o `instanceCount` do comando de draw. A CPU submete um
//! `drawIndexedIndirect` e nunca chega a saber quantos cubos foram desenhados.
//!
//! É aqui que passamos à frente da Dagor no que o roadmap identificou: eles fazem
//! o culling de patches do terreno **em CPU** e depois montam lotes de draws
//! instanciados com parâmetros em constantes de VS. Isto não tem trabalho por
//! instância do lado da CPU.
//!
//! E, como sempre, há a prova: o mesmo culling feito em CPU, comparado com o
//! contador que a GPU escreveu.

use anyhow::{Context, Result};
use harpia_app::{AppConfig, Sample, run};
use harpia_math::{Mat4, Vec3, Vec4, perspective_vk};
use harpia_render::{
    Access, GBUFFER_DEPTH_FORMAT, Load, Pass, PassPlan, RenderGraph, color_desc, depth_desc,
};
use harpia_rhi::{
    Buffer, ComputePipeline, ComputePipelineDesc, Device, Extent2D, Format, FrameInfo, Gpu,
    GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets, Texture,
};
use harpia_scene::{Bounds, Frustum};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

/// Omissão. `-- --cubes N` varre a escala onde o custo por instância aparece.
const CUBES: u32 = 2500;
/// 36 vértices por cubo: seis faces, dois triângulos cada, gerados do `gl_VertexIndex`.
const CUBE_VERTICES: u32 = 36;
/// Raio da esfera envolvente de um cubo de meia-aresta 1: a meia-diagonal.
const SPHERE_R: f32 = 1.732_050_8;
const HDR: Format = Format::Rgba16Float;
const HDR_FORMATS: [Format; 1] = [HDR];
const FOV_Y: f32 = 55.0;
const NEAR: f32 = 0.5;
const FAR: f32 = 400.0;

const SLOT_INSTANCES: u32 = 0;
const SLOT_VISIBLE: u32 = 1;
const SLOT_ARGS: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct Instance {
    pos_scale: Vec4,
    color: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CullCb {
    planes: [Vec4; 6],
    /// x = nº de instâncias, y = vértices por instância
    params: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DrawCb {
    view_proj: Mat4,
    sun_dir: Vec4,
}

fn as_bytes<T>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

fn one<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((v as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}

/// Cubos numa nuvem larga, muito para lá do que a câmara vê. Se coubessem todos
/// no ecrã, o culling não tinha nada para cortar e o gate não media nada.
fn cubes(n: u32) -> Vec<Instance> {
    (0..n)
        .map(|i| {
            let f = i as f32;
            Instance {
                pos_scale: Vec4::new(
                    (f * 0.937).sin() * 160.0,
                    (f * 1.31).sin() * 22.0,
                    (f * 0.521).cos() * 160.0,
                    0.8 + (f * 2.7).sin().abs() * 1.4,
                ),
                color: Vec4::new(
                    0.35 + 0.45 * (f * 1.7).sin().abs(),
                    0.35 + 0.45 * (f * 2.9).sin().abs(),
                    0.35 + 0.45 * (f * 4.1).sin().abs(),
                    1.0,
                ),
            }
        })
        .collect()
}

struct Targets {
    color: Texture,
    depth: Texture,
}

#[derive(Default)]
struct Instances {
    cull_pso: Option<ComputePipeline>,
    draw_pso: Option<GraphicsPipeline>,
    inst_buf: Option<Buffer>,
    visible_buf: Option<Buffer>,
    args_buf: Option<Buffer>,
    instances: Vec<Instance>,
    rt: Option<Targets>,
    extent: Extent2D,
    checked: bool,
    /// O `view_proj` do último frame, para a CPU repetir o mesmo culling.
    last_view_proj: Mat4,
    /// `-- --cpu-cull`: o mesmo ecrã, com o culling feito onde a Dagor o faz.
    cpu_cull: bool,
    /// Quantos cubos, deste arranque.
    count: u32,
    /// Reutilizado entre frames: alocar 2500 u32 por frame mediria o alocador.
    scratch: Vec<u32>,
}

impl Instances {
    /// O frame declarado como grafo.
    ///
    /// Em modo `--cpu-cull` a pass de culling não existe: a CPU escreve a lista e
    /// os argumentos, e o draw é normal. As barreiras seguem a declaração — que é
    /// o ponto de haver um grafo em vez de uma lista fixa.
    fn build_graph(&self, compute_cull: bool) -> Result<Vec<PassPlan>> {
        let mut g = RenderGraph::new();
        let inst = g.persistent_buffer("instances", self.inst_buf.context("inst")?);
        let visible = g.buffer("visible", self.visible_buf.context("visible")?);
        let args = g.buffer("args", self.args_buf.context("args")?);
        let rt = self.rt.as_ref().context("rt")?;
        let color = g.texture("color", rt.color);
        let depth = g.texture("depth", rt.depth);

        if compute_cull {
            g.pass(
                Pass::new("cull")
                    .uses(inst, Access::StorageRead)
                    .uses(visible, Access::StorageWrite)
                    .uses(args, Access::StorageWrite),
            );
        } else {
            // A CPU escreveu os dois por staging; para o grafo isso é uma escrita.
            g.pass(
                Pass::new("upload")
                    .uses(visible, Access::HostWrite)
                    .uses(args, Access::HostWrite),
            );
        }
        g.pass(
            Pass::new("cubes")
                .uses(inst, Access::StorageRead)
                .uses(visible, Access::StorageRead)
                .uses(args, Access::Indirect)
                .uses(color, Access::ColorWrite)
                .uses(depth, Access::DepthWrite)
                .load(color, Load::Clear([0.02, 0.025, 0.04, 1.0]))
                .load(depth, Load::Clear([1.0, 0.0, 0.0, 0.0])),
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

    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.rt = Some(Targets {
            color: gpu.create_texture(&color_desc(w, h, HDR))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
        });
        self.extent = Extent2D {
            width: w,
            height: h,
        };
        Ok(())
    }
}

impl Sample for Instances {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.cull_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/cull.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("cull CS")?,
        );
        self.draw_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/cube.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/cube.ps.spv")),
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
            .context("cube PSO")?,
        );

        self.instances = cubes(self.count);
        self.inst_buf = Some(gpu.create_storage_buffer(SLOT_INSTANCES, as_bytes(&self.instances))?);
        self.visible_buf = Some(
            gpu.create_storage_buffer(SLOT_VISIBLE, as_bytes(&vec![0u32; self.count as usize]))?,
        );
        // `VkDrawIndirectCommand`: quatro u32. Não é a variante indexada porque
        // os cubos saem do `gl_VertexIndex` e não há index buffer para ligar.
        self.args_buf = Some(gpu.create_storage_buffer(SLOT_ARGS, as_bytes(&[0u32; 4]))?);

        self.recreate(gpu, gpu.extent())?;
        tracing::info!(cubos = self.count, "instances");
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.rt
            .as_ref()
            .map_or_else(Vec::new, |rt| vec![("cubes", rt.color)])
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let cull_pso = self.cull_pso.as_ref().context("cull pso")?;
        let draw_pso = self.draw_pso.as_ref().context("draw pso")?;
        let args = self.args_buf.context("args")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        // A câmara roda: um frustum parado cullava sempre o mesmo e não provava
        // que a lista se refaz.
        let a = info.frame_index as f32 * 0.03;
        let eye = Vec3::new(a.sin() * 30.0, 18.0, a.cos() * 30.0);
        let view = Mat4::look_at_rh(eye, Vec3::new(0.0, 0.0, 0.0), Vec3::Y);
        let view_proj = perspective_vk(FOV_Y.to_radians(), w / h, NEAR, FAR) * view;
        self.last_view_proj = view_proj;
        let sun = Vec3::new(0.4, 0.8, 0.45).normalize();

        // O contador tem de voltar a zero antes do dispatch: o compute só o
        // incrementa. Sem isto crescia para sempre e o draw lia lixo.
        gpu.write_storage_buffer(args, as_bytes(&[CUBE_VERTICES, 0u32, 0, 0]))?;

        let mut plans = self.build_graph(!self.cpu_cull)?.into_iter();
        let mut next_barriers = move || plans.next().map(|p| p.barriers).unwrap_or_default();

        let frustum = Frustum::from_view_proj(view_proj);
        // O lado da comparação que a Dagor faz: a CPU percorre as instâncias,
        // testa cada uma, monta a lista e só então submete. É o trabalho por
        // instância que o caminho de cima não tem.
        let cpu_visible = if self.cpu_cull {
            let planes = frustum.planes();
            self.scratch.clear();
            for (i, inst) in self.instances.iter().enumerate() {
                let c = inst.pos_scale.truncate();
                let r = inst.pos_scale.w * SPHERE_R;
                if planes
                    .iter()
                    .all(|p| Vec3::new(p.x, p.y, p.z).dot(c) + p.w + r >= 0.0)
                {
                    self.scratch.push(i as u32);
                }
            }
            let n = self.scratch.len() as u32;
            gpu.write_storage_buffer(
                self.visible_buf.context("visible")?,
                as_bytes(&self.scratch),
            )?;
            // Os argumentos ficam certos para o `finish` comparar os dois modos.
            gpu.write_storage_buffer(args, as_bytes(&[CUBE_VERTICES, n, 0, 0]))?;
            n
        } else {
            let cull = CullCb {
                planes: frustum.planes(),
                params: Vec4::new(self.count as f32, CUBE_VERTICES as f32, 0.0, 0.0),
            };
            gpu.write_frame_bytes(one(&cull))?;
            gpu.set_compute_pipeline(cull_pso)?;
            gpu.bind_compute_bindless()?;
            gpu.barriers(&next_barriers())?;
            gpu.dispatch(self.count.div_ceil(64), 1, 1)?;
            0
        };
        if self.cpu_cull {
            gpu.barriers(&next_barriers())?;
        }
        gpu.mark("cull");

        let draw = DrawCb {
            view_proj,
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
        };
        gpu.write_frame_bytes(one(&draw))?;
        gpu.barriers(&next_barriers())?;
        gpu.begin_color_pass(
            &[rt.color],
            Some(rt.depth),
            &[[0.02, 0.025, 0.04, 1.0]],
            Some(1.0),
        )?;
        gpu.set_pipeline(draw_pso)?;
        gpu.bind_graphics_bindless()?;
        if self.cpu_cull {
            gpu.draw(CUBE_VERTICES, cpu_visible, 0, 0)?;
        } else {
            // Um draw. A CPU não sabe quantas instâncias saem daqui.
            gpu.draw_indirect(args, 0, 1)?;
        }
        gpu.end_color_pass()?;
        gpu.mark("cubes");

        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0])?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }

    /// A GPU contou bem? A CPU refaz o mesmo culling e compara.
    fn finish(&mut self, gpu: &mut Gpu) -> Result<()> {
        if self.checked {
            return Ok(());
        }
        self.checked = true;

        let args = gpu.read_buffer(self.args_buf.context("args")?, 16)?;
        let word = |i: usize| {
            u32::from_ne_bytes([
                args[i * 4],
                args[i * 4 + 1],
                args[i * 4 + 2],
                args[i * 4 + 3],
            ])
        };
        let gpu_count = word(1);

        let frustum = Frustum::from_view_proj(self.last_view_proj);
        let planes = frustum.planes();
        // O **mesmo** teste que o compute faz, linha por linha. Comparar contra um
        // teste diferente deixa qualquer erro esconder-se na folga entre os dois:
        // era o que acontecia com uma caixa contra uma esfera, que dava 506 contra
        // 496 e aceitava tudo o que estivesse entre as duas.
        let visible_on_cpu = |i: &Instance| {
            let c = i.pos_scale.truncate();
            let r = i.pos_scale.w * SPHERE_R;
            planes
                .iter()
                .all(|p| Vec3::new(p.x, p.y, p.z).dot(c) + p.w + r >= 0.0)
        };
        let cpu_count = self.instances.iter().filter(|i| visible_on_cpu(i)).count() as u32;

        // E a caixa envolvente, que contém a esfera: um majorante independente.
        let box_count = self
            .instances
            .iter()
            .filter(|i| {
                frustum.intersects(&Bounds {
                    center: i.pos_scale.truncate(),
                    extents: Vec3::splat(i.pos_scale.w * SPHERE_R),
                })
            })
            .count() as u32;

        tracing::info!(
            modo = if self.cpu_cull { "cpu" } else { "compute" },
            cubos = self.count,
            visiveis_gpu = gpu_count,
            visiveis_cpu = cpu_count,
            caixa_cpu = box_count,
            cortados = self.count - gpu_count,
            percent_cortado = format!(
                "{:.1}",
                100.0 * (self.count - gpu_count) as f32 / self.count as f32
            ),
            "culling em compute"
        );

        anyhow::ensure!(
            word(0) == CUBE_VERTICES && word(2) == 0 && word(3) == 0,
            "o compute escreveu mal os campos fixos do comando: {:?}",
            (word(0), word(2), word(3))
        );
        anyhow::ensure!(
            gpu_count > 0,
            "o culling cortou tudo: o draw indirecto não desenhou nada"
        );
        anyhow::ensure!(
            gpu_count < self.count,
            "o culling não cortou nada — o frustum não está a apertar"
        );
        anyhow::ensure!(
            gpu_count == cpu_count,
            "a GPU contou {gpu_count} e a CPU {cpu_count} com o mesmo teste"
        );
        anyhow::ensure!(
            gpu_count <= box_count,
            "a esfera aceitou {gpu_count} e a caixa que a contém só {box_count}"
        );

        // O contador pode estar certo e a lista ser lixo. Cada id tem de existir,
        // de passar o teste, e de aparecer uma só vez -- ids repetidos seriam um
        // `atomicAdd` a devolver o mesmo slot a duas threads.
        let list = gpu.read_buffer(self.visible_buf.context("visible")?, gpu_count as usize * 4)?;
        let mut seen = vec![false; self.count as usize];
        for k in 0..gpu_count as usize {
            let id = u32::from_ne_bytes([
                list[k * 4],
                list[k * 4 + 1],
                list[k * 4 + 2],
                list[k * 4 + 3],
            ]);
            anyhow::ensure!(id < self.count, "id {id} fora do array na posição {k}");
            anyhow::ensure!(!seen[id as usize], "id {id} repetido na lista de visíveis");
            seen[id as usize] = true;
            anyhow::ensure!(
                visible_on_cpu(&self.instances[id as usize]),
                "a GPU pôs o cubo {id} na lista e o frustum rejeita-o"
            );
        }
        tracing::info!(
            ids = gpu_count,
            "lista de visíveis: todos únicos e dentro do frustum"
        );
        Ok(())
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — instances".into();
    let user_set = std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    let cpu_cull = config.extra.iter().any(|a| a == "--cpu-cull");
    let count = config
        .extra
        .iter()
        .position(|a| a == "--cubes")
        .and_then(|i| config.extra.get(i + 1))
        .map(|v| v.parse::<u32>())
        .transpose()
        .context("`--cubes` quer um número")?
        .unwrap_or(CUBES)
        .max(1);
    run(
        config,
        Instances {
            cpu_cull,
            count,
            scratch: Vec::with_capacity(count as usize),
            ..Default::default()
        },
    )
}
