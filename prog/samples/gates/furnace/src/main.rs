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
use harpia_app::{AppConfig, Sample, run};
use harpia_math::Vec4;
use harpia_render::{Access, Pass, RenderGraph};
use harpia_rhi::{
    ComputePipeline, ComputePipelineDesc, Device, Format, FrameInfo, Gpu, Texture, TextureDesc,
    TextureDim,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

/// Lado da grelha: 64 valores de N·V × 64 de rugosidade, por painel.
const N: u32 = 64;
/// Amostras de Monte Carlo por célula.
///
/// 4096 e não 1024 por causa do teste de troca de eixos: com 1024 o resíduo era
/// 0.0332, e um controlo negativo (a amostragem a ignorar `alpha_y`) dava 0.0342.
/// Indistinguíveis. Medido a 1024/4096/16384 o resíduo dá 0.0332/0.0161/0.0073 —
/// parte-se a meio quando as amostras quadruplicam, que é a assinatura de 1/sqrt(N)
/// e a prova de que é ruído e não assimetria do modelo. O sinal a sério não se mexe:
/// a separação média fica em 0.0436 nas três.
const SAMPLES: u32 = 4096;
/// Razão de anisotropia do painel da direita: `alpha_y = alpha_x / RATIO`.
const ANISO_RATIO: f32 = 4.0;
/// Quanto o caminho anisotrópico pode afastar-se do isotrópico com `ax == ay`.
///
/// Não é zero porque os dois integram com sequências de phi diferentes: o
/// isotrópico usa `phi = 2*pi*xi`, o anisotrópico a inversão esticada, que no caso
/// `ax == ay` é a mesma distribuição por outra parametrização. O que se está a
/// medir é se os dois modelos concordam, não se as amostras coincidem.
const ANISO_TOLERANCE: f32 = 0.01;
/// Separação média mínima entre olhar ao longo da tangente e da bitangente.
///
/// Calibrado contra o ruído: com `ANISO_RATIO = 1.0` (material isotrópico) a
/// separação **média** medida é 0.0021 e o **máximo** 0.0332. Um limiar sobre o
/// máximo teria de ser maior que 0.0332 para não ser ruído; sobre a média, 0.02 é
/// dez vezes o ruído e um sexto do valor real.
const MIN_SPLIT: f32 = 0.02;
/// Quanto o albedo pode mudar ao trocar os eixos e rodar a vista 90 graus.
///
/// Zero em teoria; o que sobra é Monte Carlo, porque os dois lados sorteiam
/// sequências diferentes da mesma distribuição. A 4096 amostras o resíduo medido é
/// 0.0161, e um Lambda que ignore uma das rugosidades dá **2.58** — não é um
/// limiar apertado contra o ruído, é uma diferença de duas ordens de grandeza.
const SWAP_TOLERANCE: f32 = 0.03;

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
                width: N * 3,
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
        self.result
            .map_or_else(Vec::new, |t| vec![("albedo-direccional", t)])
    }

    fn frame(&mut self, gpu: &mut Gpu, _info: FrameInfo) -> Result<()> {
        let cs = self.cs.as_ref().context("cs")?;
        let result = self.result.context("result")?;
        let params = Vec4::new(N as f32, SAMPLES as f32, ANISO_RATIO, 0.0);
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
        // O frame declarado como grafo: um compute escreve, e depois lê-se.
        // A barreira que estava aqui escrita à mão sai daqui agora.
        let mut plans = {
            let mut g = RenderGraph::new();
            let r = g.texture("result", result);
            g.pass(Pass::new("furnace").uses(r, Access::StorageWrite));
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
        gpu.dispatch(N * 3 / 8, N / 8, 1)?;
        gpu.barriers(&next_barriers())?;
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
            halves.len() >= (N * 3 * N * 4) as usize,
            "readback com {} valores, esperava {}",
            halves.len(),
            N * 3 * N * 4
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
                let i = ((row * N * 3 + x) * 4) as usize;
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

        // Varrimento completo do painel isotrópico, não só as linhas impressas.
        let at = |row: u32, col: u32, ch: usize| {
            half_to_f32(halves[((row * N * 3 + col) * 4) as usize + ch])
        };
        let (mut loss_s, mut loss_c, mut worst_all) = (0.0f64, 0.0f64, 0.0f32);
        for row in 0..N {
            for col in 0..N {
                loss_s += (1.0 - at(row, col, 0)) as f64;
                loss_c += (1.0 - at(row, col, 1)) as f64;
                worst_all = worst_all.max((at(row, col, 2) - 1.0).abs());
            }
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

        self.report_aniso(&at)
    }

    /// O painel da direita: a GGX anisotrópica responde a três perguntas.
    fn report_aniso(&self, at: &dyn Fn(u32, u32, usize) -> f32) -> Result<()> {
        let mut worst_reduction = 0.0f32;
        let mut reduction_at = (0.0f32, 0.0f32);
        let mut mean_split = 0.0f64;
        let mut worst_swap = 0.0f32;
        let mut swap_at = (0.0f32, 0.0f32);
        let (mut loss_t, mut loss_b) = (0.0f64, 0.0f64);
        let (mut over_t, mut over_b) = (0.0f32, 0.0f32);

        println!("\n  rugosidade   iso(ref)   aniso ax=ay   ao longo de T   ao longo de B");
        for row in 0..N {
            let roughness = (row as f32 + 0.5) / N as f32;
            let (mut r_iso, mut r_red, mut r_t, mut r_b) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for col in 0..N {
                let iso = at(row, col, 1);
                let red = at(row, N + col, 0);
                let t = at(row, N + col, 1);
                let b = at(row, N + col, 2);
                // 1. Com ax == ay o modelo anisotrópico tem de dar o isotrópico.
                let d = (red - iso).abs();
                if d > worst_reduction {
                    worst_reduction = d;
                    reduction_at = (roughness, (col as f32 + 0.5) / N as f32);
                }
                // 2. Anisotropia a sério não pode criar energia.
                over_t = over_t.max(t - 1.0);
                over_b = over_b.max(b - 1.0);
                loss_t += (1.0 - t) as f64;
                loss_b += (1.0 - b) as f64;
                // 3. Tem de fazer alguma coisa: a média de |T - B| sobre a grelha.
                //    Usa-se a média e não o máximo porque o máximo do ruído de
                //    Monte Carlo chega a 0.033 num material isotrópico, e um
                //    limiar abaixo disso não verifica nada (provado: com razão
                //    1.0 a versão anterior deste teste passava).
                mean_split += (t - b).abs() as f64;
                // 4. Trocar os eixos e rodar a vista 90 graus é relabelar: o
                //    mesmo material, o mesmo número. É esta que apanha um Lambda
                //    que ignore uma das rugosidades.
                let sw_t = at(row, 2 * N + col, 0);
                let sw_b = at(row, 2 * N + col, 1);
                for (x, y) in [(t, sw_t), (b, sw_b)] {
                    if (x - y).abs() > worst_swap {
                        worst_swap = (x - y).abs();
                        swap_at = (roughness, (col as f32 + 0.5) / N as f32);
                    }
                }
                r_iso += iso as f64;
                r_red += red as f64;
                r_t += t as f64;
                r_b += b as f64;
            }
            if row % 8 == 4 {
                let n = N as f64;
                println!(
                    "  {roughness:>9.3}   {:>8.4}   {:>11.4}   {:>13.4}   {:>13.4}",
                    r_iso / n,
                    r_red / n,
                    r_t / n,
                    r_b / n
                );
            }
        }
        let cells = (N * N) as f64;
        tracing::info!(
            razao = ANISO_RATIO,
            desvio_max_reducao = format!("{worst_reduction:.4}"),
            na_rugosidade = format!("{:.3}", reduction_at.0),
            no_ndv = format!("{:.3}", reduction_at.1),
            perda_media_tangente = format!("{:.4}", loss_t / cells),
            perda_media_bitangente = format!("{:.4}", loss_b / cells),
            separacao_media_T_B = format!("{:.4}", mean_split / cells),
            desvio_max_troca_de_eixos = format!("{worst_swap:.4}"),
            na_rugosidade_ = format!("{:.3}", swap_at.0),
            no_ndv_ = format!("{:.3}", swap_at.1),
            "GGX anisotrópica"
        );

        anyhow::ensure!(
            worst_reduction <= ANISO_TOLERANCE,
            "com alpha_x == alpha_y a GGX anisotrópica devia dar o isotrópico, e \
             desvia {worst_reduction:.4} (tolerância {ANISO_TOLERANCE}) na \
             rugosidade {:.3}, N·V {:.3}. O modelo anisotrópico está errado.",
            reduction_at.0,
            reduction_at.1
        );
        anyhow::ensure!(
            over_t <= TOLERANCE && over_b <= TOLERANCE,
            "a GGX anisotrópica cria energia: {over_t:.4} ao longo da tangente, \
             {over_b:.4} ao longo da bitangente"
        );
        let mean_split = (mean_split / cells) as f32;
        anyhow::ensure!(
            mean_split > MIN_SPLIT,
            "ao longo da tangente e da bitangente o albedo é o mesmo (separação \
             média {mean_split:.4}, mínimo {MIN_SPLIT}): a anisotropia não está a \
             fazer nada"
        );
        anyhow::ensure!(
            worst_swap <= SWAP_TOLERANCE,
            "trocar alpha_x com alpha_y e rodar a vista 90 graus é o mesmo \
             material, e o albedo muda {worst_swap:.4} (tolerância \
             {SWAP_TOLERANCE}) na rugosidade {:.3}, N·V {:.3}. O modelo \
             anisotrópico não é simétrico nos eixos.",
            swap_at.0,
            swap_at.1
        );
        Ok(())
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — furnace".into();
    let user_set = std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set {
        config.max_frames = std::num::NonZeroU32::new(4);
    }
    run(config, Furnace::default())
}
