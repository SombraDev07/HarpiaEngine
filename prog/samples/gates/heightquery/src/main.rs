//! Fase 6, gate `heightquery`: a altura do terreno em CPU contra GPU.
//!
//! O terreno é desenhado pela GPU e a colisão é resolvida pela CPU. Se as duas
//! não concordarem, o jogador atravessa o chão que vê, ou anda no ar — e parece
//! um bug de física quando é um bug de aritmética.
//!
//! Este gate mede a discordância em vez de assumir que não existe: a mesma grelha
//! avaliada dos dois lados, lida de volta, e comparada ponto a ponto.

use anyhow::{Context, Result};
use harpia_app::{AppConfig, Sample, run};
use harpia_math::Vec4;
use harpia_render::terrain_height;
use harpia_render::{Access, Pass, RenderGraph};
use harpia_rhi::{
    ComputePipeline, ComputePipelineDesc, Device, Format, FrameInfo, Gpu, Texture, TextureDesc,
    TextureDim,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

/// Lado da grelha de amostras. 256² = 65 536 pontos, que chega para apanhar o
/// pior caso e ainda cabe num readback.
const N: u32 = 256;
/// Passo entre amostras, em unidades de mundo. Primo-ish de propósito: uma grelha
/// alinhada com a do ruído só testaria os vértices, que são o caso fácil.
const STEP: f32 = 3.7;
/// Canto do mundo onde a grelha começa.
const ORIGIN: (f32, f32) = (-473.0, 291.0);

/// Acima disto a divergência deixa de ser ruído do último bit e passa a ser um
/// buraco onde o jogador cai. Metros.
const TOLERANCE: f32 = 0.05;

#[derive(Default)]
struct HeightQuery {
    cs: Option<ComputePipeline>,
    result: Option<Texture>,
    reported: bool,
}

impl Sample for HeightQuery {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.cs = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/heightquery.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("heightquery CS")?,
        );
        self.result = Some(
            gpu.create_texture(&TextureDesc {
                width: N,
                height: N,
                depth_slices: 1,
                dim: TextureDim::D2,
                mip_levels: 1,
                format: Format::R32Float,
                sampled: true,
                storage: true,
                color_attachment: false,
                depth: false,
            })
            .context("result")?,
        );
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.result
            .map_or_else(Vec::new, |t| vec![("gpu-heights", t)])
    }

    fn frame(&mut self, gpu: &mut Gpu, _info: FrameInfo) -> Result<()> {
        let cs = self.cs.as_ref().context("cs")?;
        let result = self.result.context("result")?;

        gpu.write_frame_bytes(
            Vec4::new(ORIGIN.0, ORIGIN.1, STEP, N as f32)
                .to_array()
                .iter()
                .flat_map(|f| f.to_ne_bytes())
                .collect::<Vec<u8>>()
                .as_slice(),
        )?;
        gpu.set_compute_pipeline(cs)?;
        gpu.bind_compute_bindless()?;
        // O frame declarado como grafo: um compute escreve, e depois lê-se.
        // A barreira que estava aqui escrita à mão sai daqui agora.
        let mut plans = {
            let mut g = RenderGraph::new();
            let r = g.texture("result", result);
            g.pass(Pass::new("heightquery").uses(r, Access::StorageWrite));
            g.pass(Pass::new("read").uses(r, Access::TransferRead));
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
            g.compile().into_iter()
        };
        let mut next_barriers = move || plans.next().map(|p| p.barriers).unwrap_or_default();
        gpu.barriers(&next_barriers())?;
        gpu.dispatch(N / 8, N / 8, 1)?;
        gpu.barriers(&next_barriers())?;
        gpu.mark("heightquery");

        // A janela fica preta de propósito: isto é uma medição, não uma imagem.
        gpu.begin_swapchain_pass([0.03, 0.03, 0.05, 1.0])?;
        gpu.end_swapchain_pass()?;

        Ok(())
    }

    /// Fora do frame: `read_texture` espera pelo device.
    fn finish(&mut self, gpu: &mut Gpu) -> Result<()> {
        if self.reported {
            return Ok(());
        }
        self.reported = true;
        self.compare(gpu)
    }
}

impl HeightQuery {
    /// Lê o resultado da GPU e compara com a mesma grelha feita em CPU.
    fn compare(&self, gpu: &mut Gpu) -> Result<()> {
        let data = gpu.read_texture(self.result.context("result")?)?;
        anyhow::ensure!(
            data.format == Format::R32Float,
            "esperava R32Float, veio {:?}",
            data.format
        );
        let gpu_heights: Vec<f32> = data
            .bytes
            .chunks_exact(4)
            .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        anyhow::ensure!(
            gpu_heights.len() >= (N * N) as usize,
            "readback com {} valores, esperava {}",
            gpu_heights.len(),
            N * N
        );

        let mut max_diff = 0.0f32;
        let mut sum_diff = 0.0f64;
        let mut over = 0u32;
        let mut worst = (0u32, 0u32, 0.0f32, 0.0f32);
        for y in 0..N {
            for x in 0..N {
                let wx = ORIGIN.0 + x as f32 * STEP;
                let wz = ORIGIN.1 + y as f32 * STEP;
                let cpu = terrain_height(wx, wz);
                let g = gpu_heights[(y * N + x) as usize];
                let d = (cpu - g).abs();
                sum_diff += d as f64;
                if d > max_diff {
                    max_diff = d;
                    worst = (x, y, cpu, g);
                }
                if d > TOLERANCE {
                    over += 1;
                }
            }
        }
        let total = (N * N) as f64;
        tracing::info!(
            pontos = N * N,
            diff_media = format!("{:.6}", sum_diff / total),
            diff_max = format!("{max_diff:.6}"),
            acima_da_tolerancia = over,
            percent = format!("{:.2}", 100.0 * over as f64 / total),
            "heightquery cpu vs gpu"
        );
        if max_diff > TOLERANCE {
            let (x, y, cpu, g) = worst;
            tracing::warn!(
                x,
                y,
                mundo_x = ORIGIN.0 + x as f32 * STEP,
                mundo_z = ORIGIN.1 + y as f32 * STEP,
                cpu = format!("{cpu:.4}"),
                gpu = format!("{g:.4}"),
                "pior ponto"
            );
        }
        anyhow::ensure!(
            max_diff <= TOLERANCE,
            "CPU e GPU discordam em até {max_diff:.4} m (tolerância {TOLERANCE}). \
             A colisão e a geometria não são a mesma superfície."
        );
        Ok(())
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — heightquery".into();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(4);
    }
    run(config, HeightQuery::default())
}
