//! Gate `furnace`: o nosso BRDF especular conserva energia?
//!
//! Numa fornalha branca a radiância incidente é 1.0 em todas as direcções, por
//! isso um BRDF que não absorve tem de devolver exactamente 1.0. Não é uma
//! questão de gosto nem de screenshot: ou dá 1.0 ou perdeu energia.
//!
//! Mede três modelos lado a lado sobre uma grelha de (rugosidade, N·V):
//!
//! * o **Smith de Schlick** que a Harpia usa hoje,
//! * o **Smith height-correlated**, que é a forma correcta,
//! * o correlacionado **com compensação de multiscatter**, que tem de dar 1.0.
//!
//! Procurei um equivalente disto na Dagor e não encontrei — é um dos poucos
//! sítios onde podemos estar à frente, e custa um gate.

use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::Vec4;
use harpia_rhi::{
    ComputePipeline, ComputePipelineDesc, Device, Format, FrameInfo, Gpu, Texture, TextureDesc,
    TextureDim,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

/// Lado da grelha: 64 valores de N·V × 64 de rugosidade.
const N: u32 = 64;
/// Amostras de Monte Carlo por célula.
const SAMPLES: u32 = 1024;

/// Quanto a versão compensada pode afastar-se de 1.0 antes de ser um bug.
///
/// Não é zero: o integral é Monte Carlo com 1024 amostras e o alvo é meio-float,
/// portanto há ruído legítimo. É apertado o suficiente para apanhar uma
/// compensação mal ligada, que erra por dezenas de por cento e não por milésimos.
const TOLERANCE: f32 = 0.02;

#[derive(Default)]
struct Furnace {
    cs: Option<ComputePipeline>,
    result: Option<Texture>,
    done: bool,
}

impl Sample for Furnace {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.cs = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/furnace.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("furnace CS")?,
        );
        self.result = Some(
            gpu.create_texture(&TextureDesc {
                width: N,
                height: N,
                depth_slices: 1,
                dim: TextureDim::D2,
                mip_levels: 1,
                format: Format::Rgba16Float,
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
        self.result.map_or_else(Vec::new, |t| vec![("albedo-direccional", t)])
    }

    fn frame(&mut self, gpu: &mut Gpu, _info: FrameInfo) -> Result<()> {
        let cs = self.cs.as_ref().context("cs")?;
        let result = self.result.context("result")?;
        let params = Vec4::new(N as f32, SAMPLES as f32, 0.0, 0.0);
        gpu.write_frame_bytes(
            params
                .to_array()
                .iter()
                .flat_map(|f| f.to_ne_bytes())
                .collect::<Vec<u8>>()
                .as_slice(),
        )?;
        gpu.set_compute_pipeline(cs)?;
        gpu.bind_compute_bindless()?;
        gpu.dispatch(N / 8, N / 8, 1)?;
        gpu.storage_barrier(result)?;
        gpu.mark("furnace");
        gpu.begin_swapchain_pass([0.02, 0.02, 0.03, 1.0])?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }

    fn finish(&mut self, gpu: &mut Gpu) -> Result<()> {
        if self.done {
            return Ok(());
        }
        self.done = true;
        self.report(gpu)
    }
}

/// Metade de um `f16` para `f32`. O alvo é `Rgba16Float` e o readback vem cru.
fn half_to_f32(h: u16) -> f32 {
    let sign = ((h >> 15) & 1) as u32;
    let exp = ((h >> 10) & 0x1f) as u32;
    let frac = (h & 0x3ff) as u32;
    let bits = match exp {
        0 if frac == 0 => sign << 31,
        0 => {
            // Subnormal: normaliza deslocando até o bit implícito aparecer.
            let mut e = -1i32;
            let mut f = frac;
            while f & 0x400 == 0 {
                f <<= 1;
                e -= 1;
            }
            let exp32 = (127 - 15 + e + 1) as u32;
            (sign << 31) | (exp32 << 23) | ((f & 0x3ff) << 13)
        }
        0x1f => (sign << 31) | (0xff << 23) | (frac << 13),
        _ => (sign << 31) | ((exp + 127 - 15) << 23) | (frac << 13),
    };
    f32::from_bits(bits)
}

impl Furnace {
    fn report(&self, gpu: &mut Gpu) -> Result<()> {
        let data = gpu.read_texture(self.result.context("result")?)?;
        let halves: Vec<u16> = data
            .bytes
            .chunks_exact(2)
            .map(|b| u16::from_ne_bytes([b[0], b[1]]))
            .collect();
        anyhow::ensure!(
            halves.len() >= (N * N * 4) as usize,
            "readback com {} valores, esperava {}",
            halves.len(),
            N * N * 4
        );

        // Perda de energia por rugosidade, que é onde a diferença aparece.
        println!("\n  rugosidade   Schlick-k   correlated   compensado");
        let mut worst_comp = 0.0f32;
        let mut max_gap = 0.0f32;
        let mut gap_at = 0.0f32;
        for row in [4, 12, 20, 28, 36, 44, 52, 60] {
            let roughness = (row as f32 + 0.5) / N as f32;
            // Média sobre N·V, que é o número que diz quanta energia se perde.
            let (mut s, mut c, mut m) = (0.0f64, 0.0f64, 0.0f64);
            for x in 0..N {
                let i = ((row * N + x) * 4) as usize;
                s += half_to_f32(halves[i]) as f64;
                c += half_to_f32(halves[i + 1]) as f64;
                m += half_to_f32(halves[i + 2]) as f64;
            }
            let (s, c, m) = (
                (s / N as f64) as f32,
                (c / N as f64) as f32,
                (m / N as f64) as f32,
            );
            println!("  {roughness:>9.3}   {s:>9.4}   {c:>10.4}   {m:>10.4}");
            if (c - s).abs() > max_gap {
                max_gap = (c - s).abs();
                gap_at = roughness;
            }
            if (m - 1.0).abs() > worst_comp {
                worst_comp = (m - 1.0).abs();
            }
        }

        // Varrimento completo, não só as linhas impressas.
        let (mut loss_s, mut loss_c, mut worst_all) = (0.0f64, 0.0f64, 0.0f32);
        for i in (0..(N * N) as usize).map(|i| i * 4) {
            loss_s += (1.0 - half_to_f32(halves[i])) as f64;
            loss_c += (1.0 - half_to_f32(halves[i + 1])) as f64;
            worst_all = worst_all.max((half_to_f32(halves[i + 2]) - 1.0).abs());
        }
        let cells = (N * N) as f64;
        tracing::info!(
            celulas = N * N,
            amostras = SAMPLES,
            perda_media_schlick = format!("{:.4}", loss_s / cells),
            perda_media_correlated = format!("{:.4}", loss_c / cells),
            maior_diferenca = format!("{max_gap:.4}"),
            na_rugosidade = format!("{gap_at:.3}"),
            desvio_max_compensado = format!("{worst_all:.4}"),
            "white furnace"
        );
        anyhow::ensure!(
            worst_all <= TOLERANCE,
            "a compensação de multiscatter falha a fornalha: desvio máximo de \
             {worst_all:.4} contra uma tolerância de {TOLERANCE}. O BRDF não \
             conserva energia."
        );
        Ok(())
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — furnace".into();
    let user_set =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set {
        config.max_frames = std::num::NonZeroU32::new(4);
    }
    run(config, Furnace::default())
}
