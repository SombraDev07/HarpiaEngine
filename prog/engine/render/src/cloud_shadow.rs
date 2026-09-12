//! Sombra das nuvens: profundidade óptica na direcção do sol, em CPU.
//!
//! **Porque existe, e porquê primeiro em CPU.** O fog em froxels já atenua o sol
//! pela CSM; falta-lhe a extinção das nuvens, que é o que o `INDEX.md` põe na
//! fase 6. A transmitância que temos hoje é de **ecrã** — `(scattered, transmittance)`
//! a meia resolução — e responde «quanto do céu atrás deste pixel está tapado».
//! O fog precisa de outra grandeza: «quanto sol chega a este ponto do ar». Não são
//! a mesma coisa, e usar uma pela outra sombreia o ar pelas nuvens que estão atrás
//! da câmara.
//!
//! A grandeza certa exige marchar a densidade das nuvens na direcção do sol. Essa
//! densidade existe hoje **uma vez**, dentro de 524 linhas de assembly no
//! `clouds.ps.spvasm`. Escrevê-la uma segunda vez num shader novo repetiria o erro
//! que esta árvore já pagou duas vezes: a altura do terreno em três sítios, e os
//! 77 m de divergência entre CPU e GPU da D41. Por isso a segunda cópia nasce
//! aqui, onde se pode testar — e o teste `the_constants_match_the_shader` lê o
//! `.spvasm` e falha se uma das duas mudar sozinha.
//!
//! **O que este módulo não tenta fazer.** A densidade desenhada depende da vista:
//! a cobertura cresce com a distância ao longo do raio, e a erosão e a densidade
//! desvanecem com ela. Um mapa ancorado no mundo não tem raio de vista. Marcha-se
//! por isso só a parte independente da vista — forma × gradiente × cobertura —,
//! que é **exactamente** o que a marcha para o sol do shader já faz: sem erosão,
//! sem desvanecimentos, com a cobertura do início do raio.

use harpia_math::Vec3;

use crate::cloud_noise::NoiseVolume;

/// Passos da marcha para o sol. `OpConstant %uint 6` no `clouds.ps.spvasm`.
pub const LIGHT_STEPS: u32 = 6;
/// Fracção da espessura em que a base abre. `%fgbot`.
pub const GRAD_BOTTOM: f32 = 0.15;
/// Fracção em que o topo fecha (a bigorna é mais suave). `%fgtop`.
pub const GRAD_TOP: f32 = 0.45;
/// Cobertura no fim do desvanecimento por distância. `%fcovfar`.
pub const COVERAGE_FAR: f32 = 0.66;

/// O que a camada é, em km, e como o ruído a esculpe.
///
/// Os campos são os mesmos números que o `CloudCb` leva à GPU — não uma segunda
/// parametrização. Se um deles mudar lá, muda aqui.
#[derive(Clone, Copy, Debug)]
pub struct CloudParams {
    /// Base e topo da camada, em km acima do solo.
    pub bottom_km: f32,
    pub top_km: f32,
    /// Cobertura em `[0,1]` (`layer.z`).
    pub coverage: f32,
    /// Escala da densidade (`layer.w`).
    pub density: f32,
    /// Escalas do ruído (`shape.x`, `shape.y`) e força da erosão (`shape.z`).
    pub base_scale: f32,
    pub detail_scale: f32,
    pub detail_strength: f32,
    /// Deslocamento do vento, km.
    pub wind: Vec3,
    /// Raio do planeta até ao solo, km (`ambient.w`).
    pub ground_radius_km: f32,
    /// Comprimento da marcha para o sol, km (`steps.z`).
    pub light_len_km: f32,
    /// Extinção, km⁻¹ (`steps.w`).
    pub extinction: f32,
}

/// A camada de nuvens avaliada em CPU.
pub struct CloudField<'a> {
    base: &'a NoiseVolume,
    detail: &'a NoiseVolume,
    p: CloudParams,
}

impl<'a> CloudField<'a> {
    pub fn new(base: &'a NoiseVolume, detail: &'a NoiseVolume, p: CloudParams) -> Self {
        Self { base, detail, p }
    }

    pub fn params(&self) -> CloudParams {
        self.p
    }

    /// Fracção de altura dentro da camada, `[0,1]`, a partir do centro do planeta.
    fn height_fraction(&self, pos_km: Vec3) -> f32 {
        let r_in = self.p.ground_radius_km + self.p.bottom_km;
        let thick = (self.p.top_km - self.p.bottom_km).max(1e-6);
        ((pos_km.length() - r_in) / thick).clamp(0.0, 1.0)
    }

    /// Gradiente de altura: base suave, topo mais suave ainda.
    fn gradient(&self, hf: f32) -> f32 {
        (hf / GRAD_BOTTOM).clamp(0.0, 1.0) * ((1.0 - hf) / GRAD_TOP).clamp(0.0, 1.0)
    }

    /// Forma: Perlin-Worley remapeado contra o seu próprio FBM de Worley.
    fn shape(&self, pos_km: Vec3) -> f32 {
        let p = pos_km * self.p.base_scale + self.p.wind;
        let t = sample_volume(self.base, p);
        let fbm = t[1] * 0.625 + t[2] * 0.25 + t[3] * 0.125;
        let lo = fbm - 1.0;
        ((t[0] - lo) / (1.0 - lo).max(1e-6)).clamp(0.0, 1.0)
    }

    /// Densidade que a marcha para o sol usa: **sem** erosão e **sem** os
    /// desvanecimentos por distância, com a cobertura pedida.
    ///
    /// É a definição que o `clouds.ps` usa no seu laço de luz, e é a única que faz
    /// sentido num mapa ancorado no mundo — ali não há raio de vista.
    pub fn light_density(&self, pos_km: Vec3, coverage: f32) -> f32 {
        let hf = self.height_fraction(pos_km);
        let sg = self.shape(pos_km) * self.gradient(hf);
        let cv = 1.0 - coverage;
        ((sg - cv) / (1.0 - cv).max(1e-6)).clamp(0.0, 1.0) * self.p.density
    }

    /// Densidade desenhada, com erosão. Existe para o teste que fixa a diferença
    /// entre as duas — não para o mapa.
    pub fn view_density(&self, pos_km: Vec3, coverage: f32) -> f32 {
        let hf = self.height_fraction(pos_km);
        let d0 = self.light_density(pos_km, coverage) / self.p.density;
        let dp = pos_km * self.p.detail_scale + self.p.wind * 2.0;
        let t = sample_volume(self.detail, dp);
        let dfbm = t[0] * 0.625 + t[1] * 0.25 + t[2] * 0.125;
        let er = dfbm * self.p.detail_strength * (1.0 - hf);
        ((d0 - er) / (1.0 - er).max(1e-6)).clamp(0.0, 1.0) * self.p.density
    }

    /// Profundidade óptica até ao sol, seis passos, como o shader.
    pub fn optical_depth_to_sun(&self, pos_km: Vec3, sun: Vec3) -> f32 {
        let dt = self.p.light_len_km / LIGHT_STEPS as f32;
        let mut tau = 0.0;
        for j in 0..LIGHT_STEPS {
            let t = (j as f32 + 0.5) * dt;
            tau += self.light_density(pos_km + sun * t, self.p.coverage) * dt;
        }
        tau
    }

    /// Quanto sol chega a este ponto, em `[0,1]`.
    pub fn transmittance_to_sun(&self, pos_km: Vec3, sun: Vec3) -> f32 {
        (-self.p.extinction * self.optical_depth_to_sun(pos_km, sun)).exp()
    }

    /// O mapa: transmitância na base da camada, numa grelha XZ do mundo.
    ///
    /// Um froxel procura aqui pelo XZ onde o seu raio de sol atravessa a base da
    /// camada — o lookup habitual de sombra de nuvens.
    pub fn shadow_map(&self, sun: Vec3, centre_xz: (f32, f32), extent_km: f32, size: u32) -> Vec<f32> {
        let mut out = Vec::with_capacity((size * size) as usize);
        let step = extent_km / size as f32;
        let base_r = self.p.ground_radius_km + self.p.bottom_km;
        for iz in 0..size {
            for ix in 0..size {
                let x = centre_xz.0 - extent_km * 0.5 + (ix as f32 + 0.5) * step;
                let z = centre_xz.1 - extent_km * 0.5 + (iz as f32 + 0.5) * step;
                // O ponto na base da camada, em coordenadas centradas no planeta.
                let pos = Vec3::new(x, base_r, z);
                out.push(self.transmittance_to_sun(pos, sun));
            }
        }
        out
    }
}

/// Amostra trilinear com repetição, como o sampler de wrap do set 2 binding 0.
///
/// Os volumes são `u8` e a GPU lê-os normalizados; aqui divide-se por 255 pelo
/// mesmo motivo. A repetição não é detalhe: as coordenadas são posição de mundo
/// vezes uma escala, portanto saem de `[0,1)` quase sempre.
fn sample_volume(v: &NoiseVolume, p: Vec3) -> [f32; 4] {
    let n = v.size as i32;
    let wrap = |i: i32| i.rem_euclid(n);
    let texel = |x: i32, y: i32, z: i32, c: usize| -> f32 {
        let idx = ((wrap(z) * n + wrap(y)) * n + wrap(x)) as usize * 4 + c;
        v.rgba[idx] as f32 * (1.0 / 255.0)
    };

    let fx = p.x * v.size as f32 - 0.5;
    let fy = p.y * v.size as f32 - 0.5;
    let fz = p.z * v.size as f32 - 0.5;
    let (x0, y0, z0) = (fx.floor(), fy.floor(), fz.floor());
    let (tx, ty, tz) = (fx - x0, fy - y0, fz - z0);
    let (x0, y0, z0) = (x0 as i32, y0 as i32, z0 as i32);

    let mut out = [0.0f32; 4];
    for (c, o) in out.iter_mut().enumerate() {
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let c00 = lerp(texel(x0, y0, z0, c), texel(x0 + 1, y0, z0, c), tx);
        let c10 = lerp(texel(x0, y0 + 1, z0, c), texel(x0 + 1, y0 + 1, z0, c), tx);
        let c01 = lerp(texel(x0, y0, z0 + 1, c), texel(x0 + 1, y0, z0 + 1, c), tx);
        let c11 = lerp(texel(x0, y0 + 1, z0 + 1, c), texel(x0 + 1, y0 + 1, z0 + 1, c), tx);
        *o = lerp(lerp(c00, c10, ty), lerp(c01, c11, ty), tz);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um volume pequeno e determinista, para os testes não dependerem do bake.
    fn volume(size: u32, f: impl Fn(u32, u32, u32) -> [u8; 4]) -> NoiseVolume {
        let mut rgba = Vec::with_capacity((size * size * size) as usize * 4);
        for z in 0..size {
            for y in 0..size {
                for x in 0..size {
                    rgba.extend_from_slice(&f(x, y, z));
                }
            }
        }
        NoiseVolume { size, rgba }
    }

    fn params() -> CloudParams {
        CloudParams {
            bottom_km: 1.5,
            top_km: 4.0,
            coverage: 0.5,
            density: 1.0,
            base_scale: 0.05,
            detail_scale: 0.4,
            detail_strength: 0.3,
            wind: Vec3::ZERO,
            ground_radius_km: 6360.0,
            light_len_km: 1.2,
            extinction: 1.0,
        }
    }

    /// As constantes daqui **são** as do shader. Se uma mudar sozinha, a sombra
    /// deixa de corresponder às nuvens e ninguém repara — é a mesma classe de bug
    /// que pôs a altura do terreno em três sítios.
    #[test]
    fn the_constants_match_the_shader() {
        let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for _ in 0..3 {
            root.pop();
        }
        let src = std::fs::read_to_string(
            root.join("prog/samples/gates/clouds/shaders/clouds.ps.spvasm"),
        )
        .expect("o shader das nuvens tem de estar onde sempre esteve");

        let konst = |name: &str| -> f32 {
            let needle = format!("%{name} = OpConstant %float ");
            let line = src
                .lines()
                .find(|l| l.trim_start().starts_with(&needle))
                .unwrap_or_else(|| panic!("{name} não está no shader"));
            line.rsplit(' ').next().unwrap().parse().unwrap()
        };
        assert_eq!(konst("fgbot"), GRAD_BOTTOM);
        assert_eq!(konst("fgtop"), GRAD_TOP);
        assert_eq!(konst("fcovfar"), COVERAGE_FAR);

        let steps = src
            .lines()
            .find(|l| l.trim_start().starts_with("%ulight = OpConstant %uint "))
            .expect("os passos da marcha de luz não estão no shader");
        assert_eq!(
            steps.rsplit(' ').next().unwrap().parse::<u32>().unwrap(),
            LIGHT_STEPS
        );
    }

    /// Repetição e interpolação: é assim que a GPU lê, e tem de ser assim aqui.
    #[test]
    fn sampling_wraps_and_interpolates() {
        let v = volume(4, |x, _, _| [(x * 60) as u8, 0, 0, 0]);
        // Fora de [0,1) volta ao mesmo sítio.
        let a = sample_volume(&v, Vec3::new(0.3, 0.2, 0.1));
        let b = sample_volume(&v, Vec3::new(1.3, 1.2, -0.9));
        assert!((a[0] - b[0]).abs() < 1e-5, "{a:?} vs {b:?} — não repete");
        // No centro entre dois texels dá a média deles.
        let mid = sample_volume(&v, Vec3::new(0.25, 0.125, 0.125));
        let expect = (0.0 + 60.0 / 255.0) * 0.5;
        assert!(
            (mid[0] - expect).abs() < 1e-3,
            "interpolação deu {} e não {expect}",
            mid[0]
        );
    }

    /// Fora da camada não há nuvem: o gradiente fecha nas duas pontas.
    #[test]
    fn there_is_nothing_outside_the_layer() {
        let base = volume(8, |_, _, _| [255, 0, 0, 0]);
        let det = volume(4, |_, _, _| [0, 0, 0, 255]);
        let p = params();
        let f = CloudField::new(&base, &det, p);
        let r = p.ground_radius_km;
        for km in [p.bottom_km - 0.5, p.top_km + 0.5] {
            let d = f.light_density(Vec3::new(0.0, r + km, 0.0), p.coverage);
            assert_eq!(d, 0.0, "densidade a {km} km, fora da camada");
        }
        // E há alguma coisa lá dentro, senão o teste acima não prova nada.
        let mid = (p.bottom_km + p.top_km) * 0.5;
        assert!(f.light_density(Vec3::new(0.0, r + mid, 0.0), 1.0) > 0.0);
    }

    /// A densidade da marcha de luz é a da vista **sem** erosão.
    ///
    /// Fixa a simplificação que o shader faz. Se alguém "melhorar" uma das duas,
    /// a sombra deixa de corresponder às nuvens desenhadas.
    #[test]
    fn the_light_march_ignores_the_detail_erosion() {
        let base = volume(8, |x, y, z| [200, 90, 60, 30 + ((x + y + z) % 7) as u8 * 10]);
        let det = volume(4, |x, y, z| [((x * 40 + y * 20 + z * 10) % 255) as u8, 120, 80, 255]);
        let p = params();
        let f = CloudField::new(&base, &det, p);
        let pos = Vec3::new(3.0, p.ground_radius_km + 2.5, -2.0);

        let light = f.light_density(pos, p.coverage);
        let view = f.view_density(pos, p.coverage);
        assert!(light > 0.0, "o caso de teste não tem nuvem nenhuma");
        assert!(
            view < light,
            "a erosão só pode tirar densidade: vista {view}, luz {light}"
        );
        // E com erosão desligada as duas coincidem.
        let mut p0 = p;
        p0.detail_strength = 0.0;
        let f0 = CloudField::new(&base, &det, p0);
        assert!((f0.view_density(pos, p.coverage) - f0.light_density(pos, p.coverage)).abs() < 1e-6);
    }

    /// Mais cobertura, menos sol a passar. E o mapa tem de ter estrutura.
    #[test]
    fn more_coverage_casts_a_darker_map() {
        let base = volume(16, |x, y, z| {
            let v = ((x * 37 + y * 17 + z * 7) % 256) as u8;
            [v, 90, 60, 30]
        });
        let det = volume(4, |_, _, _| [0, 0, 0, 255]);
        let sun = Vec3::new(0.3, 0.9, 0.2).normalize();

        let mean = |cov: f32| {
            let mut p = params();
            p.coverage = cov;
            let f = CloudField::new(&base, &det, p);
            let m = f.shadow_map(sun, (0.0, 0.0), 20.0, 16);
            (m.iter().sum::<f32>() / m.len() as f32, m)
        };
        let (light, map) = mean(0.35);
        let (heavy, _) = mean(0.95);
        assert!(
            heavy < light,
            "mais cobertura devia escurecer: {heavy} contra {light}"
        );
        // Um mapa constante passaria os testes acima e não seria sombra nenhuma.
        let lo = map.iter().cloned().fold(f32::MAX, f32::min);
        let hi = map.iter().cloned().fold(f32::MIN, f32::max);
        assert!(hi - lo > 0.01, "o mapa é liso: {lo}..{hi}");
        assert!(map.iter().all(|t| (0.0..=1.0).contains(t)));
    }

    /// O sol a incidir de outro lado dá outro mapa — senão não é sombra do sol.
    #[test]
    fn the_map_follows_the_sun() {
        let base = volume(16, |x, y, z| [((x * 29 + y * 11 + z * 5) % 256) as u8, 90, 60, 30]);
        let det = volume(4, |_, _, _| [0, 0, 0, 255]);
        let f = CloudField::new(&base, &det, params());
        let a = f.shadow_map(Vec3::new(0.0, 1.0, 0.0), (0.0, 0.0), 20.0, 12);
        let b = f.shadow_map(Vec3::new(0.7, 0.5, 0.5).normalize(), (0.0, 0.0), 20.0, 12);
        let diff: f32 = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum();
        assert!(diff > 0.05, "o mapa não mexeu com o sol: soma {diff}");
    }
}
