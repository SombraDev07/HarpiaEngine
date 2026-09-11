//! Gate `ibl`: quanto é que o split-sum nos custa, e em quê?
//!
//! O split-sum (Karis 2013) parte o integral do IBL especular em dois factores
//! pré-calculados: um mapa de ambiente convoluído por rugosidade e uma LUT de
//! dois canais. É rápido, é o que toda a gente usa, e é uma **aproximação** — mas
//! «é uma aproximação» não é um número, e sem número não se sabe se vale a pena
//! melhorar nem o que melhorar primeiro.
//!
//! Este gate integra o mesmo material por Monte Carlo e compara. Corre em CPU e
//! sem GPU nenhuma, como o `gate-ecs`: é matemática, não é desenho.
//!
//! O erro vem decomposto, porque «8% de erro» sozinho não diz o que fazer:
//!
//! * **algorítmico** — o split-sum contra a referência integrada sobre o **mesmo
//!   mapa de 8 bits**. É o custo de partir o integral em dois.
//! * **total** — o split-sum contra a referência integrada sobre o céu
//!   **analítico**. Inclui o que se perde a guardar o ambiente em 8 bits e a
//!   pré-convoluí-lo com poucas amostras.
//!
//! A diferença entre os dois diz se o problema é o método ou os dados.

use anyhow::Result;
use harpia_math::Vec3;
use harpia_render::{IblCpu, RgbaImage, generate_env, generate_ibl, sample_lod, sky_radiance};

const PI: f32 = std::f32::consts::PI;
/// Lado da grelha de (N·V, rugosidade).
const GRID: u32 = 32;
/// Amostras da referência. 4096 é o que o roadmap pediu.
const REF_SAMPLES: u32 = 4096;
/// Resolução do mapa sobre o qual a referência integra, para o erro algorítmico.
const ENV_W: u32 = 128;
const ENV_H: u32 = 64;

/// F0 de um dieléctrico comum e de um metal. O split-sum é linear em F0, por isso
/// medir nas duas pontas cobre tudo o que está pelo meio.
const F0_CASES: [(&str, f32); 2] = [("dieléctrico", 0.04), ("metal", 0.95)];

/// Acima disto o split-sum deixa de ser «uma aproximação» e passa a ser um bug.
///
/// Não é um número escolhido: é onde a literatura põe o split-sum para
/// rugosidades moderadas, e o gate publica o valor medido ao lado. Se subir, algo
/// se partiu — foi assim que se apanhou a LUT a usar um G diferente do da luz
/// directa.
const MAX_REL_ERROR: f32 = 0.25;

fn hammersley(i: u32, n: u32) -> (f32, f32) {
    (
        i as f32 / n as f32,
        (i.reverse_bits() as f32) * 2.328_306_4e-10,
    )
}

/// Meio-vector segundo a GGX, com a normal em +Z.
fn importance_ggx(xi: (f32, f32), alpha: f32) -> Vec3 {
    let a2 = alpha * alpha;
    let phi = 2.0 * PI * xi.0;
    let cos_t = ((1.0 - xi.1) / (1.0 + (a2 - 1.0) * xi.1)).sqrt();
    let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
    Vec3::new(phi.cos() * sin_t, phi.sin() * sin_t, cos_t)
}

/// Smith height-correlated — a mesma forma que o `gate-furnace` provou ser a
/// correcta (D44) e que a luz directa usa. Devolve G, não V.
fn g_correlated(ndv: f32, ndl: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let lv = ndl * (ndv * ndv * (1.0 - a2) + a2).sqrt();
    let ll = ndv * (ndl * ndl * (1.0 - a2) + a2).sqrt();
    let d = lv + ll;
    if d > 0.0 {
        (0.5 / d) * 4.0 * ndv * ndl
    } else {
        0.0
    }
}

fn f_schlick(f0: f32, voh: f32) -> f32 {
    f0 + (1.0 - f0) * (1.0 - voh).powi(5)
}

/// O integral a sério: `∫ L(l) · f(v,l) · (n·l) dl`, por importance sampling da
/// GGX. Com `pdf = D·NoH/(4·VoH)` o `D` cancela e sobra `G·F·VoH/(NoV·NoH)`.
fn reference(ndv: f32, alpha: f32, f0: f32, radiance: &dyn Fn(Vec3) -> Vec3) -> Vec3 {
    let v = Vec3::new((1.0 - ndv * ndv).max(0.0).sqrt(), 0.0, ndv);
    let mut acc = Vec3::ZERO;
    for i in 0..REF_SAMPLES {
        let h = importance_ggx(hammersley(i, REF_SAMPLES), alpha);
        let l = h * 2.0 * v.dot(h) - v;
        let ndl = l.z;
        if ndl <= 0.0 {
            continue;
        }
        let ndh = h.z.max(1e-6);
        let voh = v.dot(h).max(1e-6);
        let w = g_correlated(ndv, ndl, alpha) * f_schlick(f0, voh) * voh / (ndv * ndh);
        // O mundo do gate tem Y para cima e a normal em +Z; roda-se para o céu.
        acc += radiance(Vec3::new(l.x, l.z, l.y)) * w;
    }
    acc / REF_SAMPLES as f32
}

/// O que o shader faz: uma amostra do mapa pré-filtrado e dois números da LUT.
fn split_sum(ibl: &IblCpu, ndv: f32, roughness: f32, f0: f32) -> Vec3 {
    // O mapa pré-filtrado amostra-se ao longo de **R**, não de V: a hipótese do
    // split-sum é N = V = R para a *convolução*, mas a direcção de consulta em
    // tempo de execução é a reflexão. Com N = +Z, R = 2(N·V)N − V.
    //
    // Neste céu isto não muda nada — só depende da latitude, e V e R têm a mesma
    // — mas escrito como estava mentia com qualquer outro ambiente.
    let v = Vec3::new((1.0 - ndv * ndv).max(0.0).sqrt(), 0.0, ndv);
    let r_tangent = Vec3::new(-v.x, 0.0, ndv);
    let r = Vec3::new(r_tangent.x, r_tangent.z, r_tangent.y);
    let max_lod = (ibl.prefiltered.mip_levels - 1) as f32;
    let pre = sample_lod(&ibl.prefiltered, r, roughness * max_lod) * ibl.ibl_scale;
    let (scale, bias) = lut(&ibl.brdf_lut, ndv, roughness);
    pre * (f0 * scale + bias)
}

/// A LUT, amostrada como a GPU: bilinear com clamp nas bordas.
fn lut(img: &RgbaImage, ndv: f32, roughness: f32) -> (f32, f32) {
    let (w, h) = (img.width, img.height);
    let fx = ndv * w as f32 - 0.5;
    let fy = roughness * h as f32 - 0.5;
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let px = &img.mips[0];
    let fetch = |xi: i32, yi: i32| {
        let x = xi.clamp(0, w as i32 - 1) as u32;
        let y = yi.clamp(0, h as i32 - 1) as u32;
        let i = ((y * w + x) * 4) as usize;
        (px[i] as f32 / 255.0, px[i + 1] as f32 / 255.0)
    };
    let (xi, yi) = (x0 as i32, y0 as i32);
    let lerp =
        |a: (f32, f32), b: (f32, f32), t: f32| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    let top = lerp(fetch(xi, yi), fetch(xi + 1, yi), tx);
    let bottom = lerp(fetch(xi, yi + 1), fetch(xi + 1, yi + 1), tx);
    lerp(top, bottom, ty)
}

/// Erro relativo em luminância. Relativo porque o absoluto favorece as células
/// escuras, que são justamente as que ninguém vê.
fn rel_error(ours: Vec3, want: Vec3) -> f32 {
    let l = |c: Vec3| c.x * 0.2126 + c.y * 0.7152 + c.z * 0.0722;
    let (a, b) = (l(ours), l(want));
    if b > 1e-4 { (a - b).abs() / b } else { 0.0 }
}

struct Stats {
    max: f32,
    at: (f32, f32),
    mean: f32,
}

fn compare(ibl: &IblCpu, f0: f32, radiance: &dyn Fn(Vec3) -> Vec3) -> Stats {
    let mut max = 0.0f32;
    let mut at = (0.0f32, 0.0f32);
    let mut sum = 0.0f64;
    for yi in 0..GRID {
        let roughness = (yi as f32 + 0.5) / GRID as f32;
        let alpha = (roughness * roughness).max(0.001);
        for xi in 0..GRID {
            let ndv = ((xi as f32 + 0.5) / GRID as f32).max(0.02);
            let e = rel_error(
                split_sum(ibl, ndv, roughness, f0),
                reference(ndv, alpha, f0, radiance),
            );
            sum += e as f64;
            if e > max {
                max = e;
                at = (roughness, ndv);
            }
            if std::env::var("HARPIA_IBL_DUMP").is_ok() && (yi == 0 || yi == GRID / 2) && xi < 3 {
                let ours = split_sum(ibl, ndv, roughness, f0);
                let want = reference(ndv, alpha, f0, radiance);
                let (sc, bi) = lut(&ibl.brdf_lut, ndv, roughness);
                let max_lod = (ibl.prefiltered.mip_levels - 1) as f32;
                let pre = sample_lod(
                    &ibl.prefiltered,
                    Vec3::new(-((1.0f32 - ndv * ndv).max(0.0).sqrt()), ndv, 0.0),
                    roughness * max_lod,
                );
                eprintln!(
                    "    r={roughness:.3} ndv={ndv:.3}  pre={:.4} scale={sc:.4} bias={bi:.4} \
                     -> nosso={:.4} ref={:.4} erro={:.1}%",
                    pre.y,
                    ours.y,
                    want.y,
                    e * 100.0
                );
            }
        }
    }
    Stats {
        max,
        at,
        mean: (sum / (GRID * GRID) as f64) as f32,
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let ibl = generate_ibl();
    let env = generate_env(ENV_W, ENV_H);
    // A referência «algorítmica» integra sobre o mesmo mapa que o pré-filtro viu,
    // com o mesmo vizinho-mais-próximo: assim a única diferença é o método.
    let over_env = |d: Vec3| harpia_render::sample_latlong(&env, d);
    let over_sky = sky_radiance;

    println!(
        "\n  {:<12} {:>12} {:>12} {:>12} {:>12}",
        "F0", "alg. médio", "alg. máximo", "total médio", "total máx."
    );
    let mut worst = 0.0f32;
    let mut worst_label = "";
    let mut worst_at = (0.0f32, 0.0f32);
    for (name, f0) in F0_CASES {
        let alg = compare(&ibl, f0, &over_env);
        let tot = compare(&ibl, f0, &over_sky);
        println!(
            "  {name:<12} {:>11.1}% {:>11.1}% {:>11.1}% {:>11.1}%",
            alg.mean * 100.0,
            alg.max * 100.0,
            tot.mean * 100.0,
            tot.max * 100.0
        );
        if alg.max > worst {
            worst = alg.max;
            worst_label = name;
            worst_at = alg.at;
        }
        tracing::info!(
            f0 = name,
            erro_algoritmico_medio = format!("{:.4}", alg.mean),
            erro_algoritmico_max = format!("{:.4}", alg.max),
            na_rugosidade = format!("{:.3}", alg.at.0),
            no_ndv = format!("{:.3}", alg.at.1),
            erro_total_medio = format!("{:.4}", tot.mean),
            erro_total_max = format!("{:.4}", tot.max),
            "split-sum contra Monte Carlo"
        );
    }

    println!(
        "\n  grelha {GRID}x{GRID}, {REF_SAMPLES} amostras de referência, mapa {ENV_W}x{ENV_H}\n"
    );
    anyhow::ensure!(
        worst <= MAX_REL_ERROR,
        "o split-sum desvia {:.1}% da referência ({worst_label}, rugosidade {:.3}, \
         N·V {:.3}), acima do limite de {:.0}%. Isto não é a aproximação a \
         trabalhar — é alguma coisa partida.",
        worst * 100.0,
        worst_at.0,
        worst_at.1,
        MAX_REL_ERROR * 100.0
    );
    Ok(())
}
