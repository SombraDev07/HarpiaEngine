//! Campo de altura em tiles, com pirâmide min/max (roadmap §15 fase 6, §7.3).
//!
//! **Porque existe.** Hoje a altura é uma função analítica escrita em três sítios
//! — aqui em Rust, no `terrain.vs.glsl` e no `terrain_bounds.cs.glsl` — e paga-se
//! duas vezes: cinco FBM por vértice no VS (um para a altura, quatro para a
//! normal) e 81 por patch só para saber a caixa. A D48 mediu esse segundo custo a
//! comer o que o culling poupava, e a solução que lá ficou foi recalcular menos
//! vezes, não calcular mais barato.
//!
//! Um campo cozido resolve os dois: a caixa de um patch passa a ser uma leitura
//! de mip, e o VS passa a ler uma textura. Este ficheiro é a metade em CPU — o
//! campo, a pirâmide e as garantias. A GPU vem a seguir, e o A/B da imagem é que
//! decide se entra.
//!
//! **Porquê tiles.** Porque o mundo há-de ser maior do que a memória. O tile é a
//! unidade que se coze, se carrega e se deita fora, e é sobre ele que o item de
//! streaming da fase 6 assenta. Um tile são 256 amostras de lado à célula do
//! nível 0 — 128 unidades de mundo, 256 KB de amostras e ~175 KB de pirâmide.
//!
//! **O que a pirâmide garante.** Os vértices que o clipmap emite caem *exactamente*
//! nas amostras do nível 0: o snap de um nível é `célula × 2^L`, sempre múltiplo
//! da célula base, portanto a grelha de qualquer nível é um subconjunto desta. A
//! caixa que sai daqui contém a malha por construção, e é conservadora porque um
//! bloco de mip pode cobrir amostras fora do patch — o que sobra é folga, e a
//! folga está medida em `patch_bounds_from_the_field_contain_the_mesh`.

use std::collections::HashMap;

use harpia_math::{Vec2, Vec3};

use crate::terrain::{
    clipmap_patch_grid, clipmap_patches_per_level, terrain_height, CLIPMAP_CELL, CLIPMAP_N,
    CLIPMAP_PATCH,
};

/// Amostras por lado de um tile.
pub const TILE_N: u32 = 256;
/// Espaçamento das amostras: a célula do nível 0 do clipmap, e não outra coisa —
/// é o que faz os vértices de todos os níveis caírem em cima de amostras.
pub const TILE_SPACING: f32 = CLIPMAP_CELL;
/// Lado de um tile, em unidades de mundo.
pub const TILE_SIZE: f32 = TILE_N as f32 * TILE_SPACING;
/// Níveis da pirâmide, contando as amostras como nível 0. Com 256 de lado, o
/// último nível é um único par (min, max) do tile inteiro.
pub const TILE_MIPS: u32 = 9;

/// Em que tile cai uma coordenada de mundo.
pub fn tile_of(x: f32, z: f32) -> (i32, i32) {
    (
        (x / TILE_SIZE).floor() as i32,
        (z / TILE_SIZE).floor() as i32,
    )
}

/// A caixa de um patch, ou a razão por que não há caixa.
///
/// `Missing` existe para que um tile por cozer **não** possa passar por terreno
/// plano: um buraco silencioso no mundo é o pior resultado possível, e foi por
/// isso que esta função não devolve `Option`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PatchBounds {
    /// Cabe todo no buraco do anel: o nível de dentro já o cobre.
    Hole,
    /// Falta um tile por cozer. Não é uma caixa vazia, é falta de dados.
    Missing,
    /// Mínimo e máximo em mundo.
    Box(Vec3, Vec3),
}

/// Um tile cozido: as amostras e a pirâmide de (min, max).
pub struct HeightTile {
    coord: (i32, i32),
    /// `TILE_N × TILE_N`, em ordem row-major por z.
    samples: Vec<f32>,
    /// `mips[k]` tem blocos de `2^(k+1)` amostras de lado.
    mips: Vec<Vec<(f32, f32)>>,
}

impl HeightTile {
    /// Coze um tile a partir da função analítica.
    ///
    /// As amostras são `terrain_height` nos pontos exactos da grelha — não uma
    /// aproximação dela. É isso que permite ao teste exigir igualdade *exacta*
    /// nos centros e medir só o erro da interpolação entre eles.
    pub fn bake(tx: i32, tz: i32) -> Self {
        let n = TILE_N as usize;
        let mut samples = Vec::with_capacity(n * n);
        for iz in 0..TILE_N {
            for ix in 0..TILE_N {
                let (x, z) = sample_world(tx, tz, ix, iz);
                samples.push(terrain_height(x, z));
            }
        }

        // Nível 1 sai das amostras; os seguintes saem do anterior. Reduzir sempre
        // do nível de cima e nunca das amostras é o que mantém isto O(n²).
        let mut mips: Vec<Vec<(f32, f32)>> = Vec::new();
        let mut prev_side = n;
        for k in 0..(TILE_MIPS - 1) as usize {
            let side = prev_side / 2;
            let mut level = Vec::with_capacity(side * side);
            for bz in 0..side {
                for bx in 0..side {
                    let mut lo = f32::MAX;
                    let mut hi = f32::MIN;
                    for dz in 0..2 {
                        for dx in 0..2 {
                            let (x, z) = (bx * 2 + dx, bz * 2 + dz);
                            let (a, b) = if k == 0 {
                                let h = samples[z * prev_side + x];
                                (h, h)
                            } else {
                                mips[k - 1][z * prev_side + x]
                            };
                            lo = lo.min(a);
                            hi = hi.max(b);
                        }
                    }
                    level.push((lo, hi));
                }
            }
            mips.push(level);
            prev_side = side;
        }

        Self {
            coord: (tx, tz),
            samples,
            mips,
        }
    }

    pub fn coord(&self) -> (i32, i32) {
        self.coord
    }

    /// Uma amostra crua, por índice dentro do tile.
    pub fn sample(&self, ix: u32, iz: u32) -> f32 {
        self.samples[iz as usize * TILE_N as usize + ix as usize]
    }

    /// Mínimo e máximo num rectângulo de amostras, inclusivo nas duas pontas.
    ///
    /// Escolhe o maior mip cujo bloco cabe no intervalo e junta os blocos que o
    /// tocam — no máximo três por eixo. O resultado é **conservador**: um bloco
    /// pode cobrir amostras fora do rectângulo, nunca deixar de cobrir as de
    /// dentro.
    pub fn min_max_cells(&self, x0: u32, z0: u32, x1: u32, z1: u32) -> (f32, f32) {
        let (x0, x1) = (x0.min(TILE_N - 1), x1.min(TILE_N - 1));
        let (z0, z1) = (z0.min(TILE_N - 1), z1.min(TILE_N - 1));
        let span = (x1 - x0 + 1).max(z1 - z0 + 1);
        let k = (u32::BITS - 1 - span.leading_zeros()).min(TILE_MIPS - 1);

        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        if k == 0 {
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let h = self.sample(x, z);
                    lo = lo.min(h);
                    hi = hi.max(h);
                }
            }
            return (lo, hi);
        }

        let side = (TILE_N >> k) as usize;
        let level = &self.mips[k as usize - 1];
        for bz in (z0 >> k)..=(z1 >> k) {
            for bx in (x0 >> k)..=(x1 >> k) {
                let (a, b) = level[bz as usize * side + bx as usize];
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
        (lo, hi)
    }
}

/// Os tiles cozidos, e as perguntas que se lhes fazem.
#[derive(Default)]
pub struct HeightmapField {
    tiles: HashMap<(i32, i32), HeightTile>,
}

impl HeightmapField {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn tile(&self, tx: i32, tz: i32) -> Option<&HeightTile> {
        self.tiles.get(&(tx, tz))
    }

    /// Coze o tile se ainda não estiver. Devolve `true` se teve trabalho.
    pub fn ensure(&mut self, tx: i32, tz: i32) -> bool {
        if self.tiles.contains_key(&(tx, tz)) {
            return false;
        }
        self.tiles.insert((tx, tz), HeightTile::bake(tx, tz));
        true
    }

    /// Coze tudo o que toca o quadrado à volta de um ponto, e diz quantos cozeu.
    ///
    /// É a forma mais simples de streaming que ainda é honesta: o número que
    /// devolve é o trabalho do frame, e é ele que há-de mostrar se o orçamento
    /// chega quando a câmara anda depressa.
    pub fn ensure_around(&mut self, x: f32, z: f32, radius: f32) -> usize {
        let (tx0, tz0) = tile_of(x - radius, z - radius);
        let (tx1, tz1) = tile_of(x + radius, z + radius);
        let mut baked = 0;
        for tz in tz0..=tz1 {
            for tx in tx0..=tx1 {
                if self.ensure(tx, tz) {
                    baked += 1;
                }
            }
        }
        baked
    }

    /// Uma amostra pelo índice global, ou `None` se o tile dela não estiver cozido.
    pub fn sample_global(&self, gx: i64, gz: i64) -> Option<f32> {
        let n = TILE_N as i64;
        let tx = gx.div_euclid(n) as i32;
        let tz = gz.div_euclid(n) as i32;
        let ix = gx.rem_euclid(n) as u32;
        let iz = gz.rem_euclid(n) as u32;
        self.tiles.get(&(tx, tz)).map(|t| t.sample(ix, iz))
    }

    /// Altura interpolada. `None` se faltar algum dos quatro vizinhos.
    ///
    /// Num centro de amostra o resultado é a amostra, **exactamente**: o peso é
    /// zero e `h + (b - h) * 0.0` não perde bits. É isso que deixa comparar esta
    /// função com a analítica sem tolerância nenhuma nos pontos da grelha.
    pub fn height(&self, x: f32, z: f32) -> Option<f32> {
        let gx = x / TILE_SPACING;
        let gz = z / TILE_SPACING;
        let (fx, fz) = (gx - gx.floor(), gz - gz.floor());
        let (gx0, gz0) = (gx.floor() as i64, gz.floor() as i64);

        let h00 = self.sample_global(gx0, gz0)?;
        let h10 = self.sample_global(gx0 + 1, gz0)?;
        let h01 = self.sample_global(gx0, gz0 + 1)?;
        let h11 = self.sample_global(gx0 + 1, gz0 + 1)?;

        let top = h00 + (h10 - h00) * fx;
        let bottom = h01 + (h11 - h01) * fx;
        Some(top + (bottom - top) * fz)
    }

    /// Mínimo e máximo num rectângulo de mundo, atravessando tiles se for preciso.
    pub fn min_max(&self, x0: f32, z0: f32, x1: f32, z1: f32) -> Option<(f32, f32)> {
        let n = TILE_N as i64;
        let gx0 = (x0 / TILE_SPACING).floor() as i64;
        let gz0 = (z0 / TILE_SPACING).floor() as i64;
        let gx1 = (x1 / TILE_SPACING).ceil() as i64;
        let gz1 = (z1 / TILE_SPACING).ceil() as i64;

        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for tz in gz0.div_euclid(n)..=gz1.div_euclid(n) {
            for tx in gx0.div_euclid(n)..=gx1.div_euclid(n) {
                let tile = self.tiles.get(&(tx as i32, tz as i32))?;
                // O rectângulo, recortado a este tile.
                let cx0 = (gx0 - tx * n).clamp(0, n - 1) as u32;
                let cz0 = (gz0 - tz * n).clamp(0, n - 1) as u32;
                let cx1 = (gx1 - tx * n).clamp(0, n - 1) as u32;
                let cz1 = (gz1 - tz * n).clamp(0, n - 1) as u32;
                let (a, b) = tile.min_max_cells(cx0, cz0, cx1, cz1);
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
        Some((lo, hi))
    }

    /// A caixa de um patch do clipmap, tirada da pirâmide.
    ///
    /// Mesma geometria que `clipmap_patch_bounds`: o mesmo buraco de anel e a
    /// mesma saia subtraída em baixo. O que muda é de onde vem o `y` — daqui, em
    /// três leituras, em vez de 81 avaliações de FBM.
    pub fn patch_bounds(&self, patch: u32, camera_xz: Vec2) -> PatchBounds {
        let per_level = clipmap_patches_per_level();
        let level = patch / per_level;
        let p = patch % per_level;
        let grid = clipmap_patch_grid();
        let (px, py) = (p % grid, p / grid);

        let cell = CLIPMAP_CELL * (1u32 << level) as f32;
        let n = CLIPMAP_N as f32;
        let snap = cell * 2.0;
        let centre = Vec2::new(
            (camera_xz.x / snap).floor() * snap,
            (camera_xz.y / snap).floor() * snap,
        );
        let (c0x, c0y) = (px * CLIPMAP_PATCH, py * CLIPMAP_PATCH);

        if level > 0 {
            let inside = |cx: u32, cy: u32| {
                let dx = (cx as f32 - n * 0.5).abs();
                let dy = (cy as f32 - n * 0.5).abs();
                dx.max(dy) < n * 0.25
            };
            let all_in =
                (0..CLIPMAP_PATCH).all(|i| (0..CLIPMAP_PATCH).all(|j| inside(c0x + i, c0y + j)));
            if all_in {
                return PatchBounds::Hole;
            }
        }

        let g0 = Vec2::new(c0x as f32 - n * 0.5, c0y as f32 - n * 0.5);
        let g1 = Vec2::new(
            (c0x + CLIPMAP_PATCH) as f32 - n * 0.5,
            (c0y + CLIPMAP_PATCH) as f32 - n * 0.5,
        );
        let w0 = centre + g0 * cell;
        let w1 = centre + g1 * cell;

        let Some((lo, hi)) = self.min_max(w0.x, w0.y, w1.x, w1.y) else {
            return PatchBounds::Missing;
        };
        // A saia desce a altura na fronteira interior do anel; a caixa tem de a
        // conter, tal como na versão analítica.
        PatchBounds::Box(
            Vec3::new(w0.x, lo - cell * 2.0, w0.y),
            Vec3::new(w1.x, hi, w1.y),
        )
    }
}

/// Mundo de uma amostra. Uma só definição, para a CPU e (depois) o GLSL.
fn sample_world(tx: i32, tz: i32, ix: u32, iz: u32) -> (f32, f32) {
    let gx = tx as i64 * TILE_N as i64 + ix as i64;
    let gz = tz as i64 * TILE_N as i64 + iz as i64;
    (gx as f32 * TILE_SPACING, gz as f32 * TILE_SPACING)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{clipmap_patch_bounds, CLIPMAP_LEVELS};

    /// Nos pontos da grelha o campo é a função, sem tolerância.
    ///
    /// Se isto falhar, o campo cozido não é o mesmo terreno — e tudo o que vier a
    /// seguir (colisão, colocação de vegetação, caixas) passa a discordar da
    /// geometria, que é a classe de bug que o D41 apanhou com 77 m de erro.
    #[test]
    fn the_bake_is_the_analytic_field_at_sample_points() {
        let tile = HeightTile::bake(3, -2);
        for &(ix, iz) in &[(0u32, 0u32), (1, 0), (255, 255), (128, 37), (7, 200)] {
            let (x, z) = sample_world(3, -2, ix, iz);
            assert_eq!(
                tile.sample(ix, iz),
                terrain_height(x, z),
                "amostra ({ix}, {iz}) do tile (3, -2) diverge da função"
            );
        }
    }

    /// Entre amostras há erro, e ele é **medido**, não assumido.
    ///
    /// O campo interpola linearmente o que a função faz com Hermite: a diferença
    /// existe e tem de caber na folga do terreno. Um teste que só exigisse
    /// «pequeno» passaria com o campo errado; este imprime o número.
    #[test]
    fn interpolation_error_between_samples_is_bounded() {
        let mut field = HeightmapField::new();
        field.ensure(0, 0);

        let mut max_err = 0.0f32;
        for i in 0..64 {
            for j in 0..64 {
                // Pontos fora da grelha de propósito: 1/3 de célula.
                let x = (i as f32 + 1.0 / 3.0) * TILE_SPACING;
                let z = (j as f32 + 2.0 / 3.0) * TILE_SPACING;
                let got = field.height(x, z).expect("tile 0,0 está cozido");
                max_err = max_err.max((got - terrain_height(x, z)).abs());
            }
        }
        // O terreno tem ±60 m; meio metro de erro de reamostragem entre amostras
        // de 0.5 u é a ordem de grandeza certa. Acima disto o espaçamento está
        // errado, não a interpolação.
        assert!(
            max_err < 0.5,
            "erro máximo de interpolação {max_err:.4} m — grande de mais"
        );
        // E não é zero: se fosse, este teste não estaria a medir nada.
        assert!(max_err > 0.0, "erro exactamente zero: o teste não mede nada");
    }

    /// A pirâmide contém todas as amostras debaixo dela.
    ///
    /// É a única propriedade que o culling precisa. Um mip que corte um metro
    /// rejeita um patch cujo chão se vê, e abre-se um buraco na paisagem.
    #[test]
    fn the_pyramid_contains_every_sample_under_it() {
        let tile = HeightTile::bake(-1, 4);
        let rects = [
            (0u32, 0u32, 7u32, 7u32),
            (13, 200, 44, 255),
            (100, 100, 131, 131),
            (0, 0, 255, 255),
            (250, 3, 255, 9),
        ];
        for &(x0, z0, x1, z1) in &rects {
            let (lo, hi) = tile.min_max_cells(x0, z0, x1, z1);
            let mut true_lo = f32::MAX;
            let mut true_hi = f32::MIN;
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let h = tile.sample(x, z);
                    true_lo = true_lo.min(h);
                    true_hi = true_hi.max(h);
                }
            }
            assert!(
                lo <= true_lo && hi >= true_hi,
                "mip [{lo}, {hi}] não contém [{true_lo}, {true_hi}] em ({x0},{z0})-({x1},{z1})"
            );
        }
    }

    /// Conservadora, mas não inútil: um rectângulo pequeno não pode devolver o
    /// intervalo do tile inteiro.
    #[test]
    fn the_pyramid_is_tighter_than_the_whole_tile() {
        let tile = HeightTile::bake(0, 0);
        let (all_lo, all_hi) = tile.min_max_cells(0, 0, TILE_N - 1, TILE_N - 1);
        let (lo, hi) = tile.min_max_cells(8, 8, 15, 15);
        assert!(
            hi - lo < (all_hi - all_lo) * 0.75,
            "bloco de 8 deu {:.2} m de amplitude contra {:.2} do tile",
            hi - lo,
            all_hi - all_lo
        );
    }

    /// A caixa que sai da pirâmide contém a malha que o VS emite, e a folga
    /// contra a caixa exacta está medida.
    #[test]
    fn patch_bounds_from_the_field_contain_the_mesh() {
        const OFFSETS: [(u32, u32); 6] = [(0, 0), (0, 1), (1, 0), (1, 0), (0, 1), (1, 1)];
        let cam = Vec2::new(37.0, -91.0);

        let mut field = HeightmapField::new();
        // Só os níveis baixos: cozer o alcance todo são 256 tiles e este teste não
        // precisa disso para provar a propriedade.
        let levels = 3.min(CLIPMAP_LEVELS);
        let range = CLIPMAP_CELL * (1u32 << (levels - 1)) as f32 * CLIPMAP_N as f32;
        field.ensure_around(cam.x, cam.y, range);

        let per_level = clipmap_patches_per_level();
        let n = CLIPMAP_N as f32;
        let mut checked = 0;
        let mut worst_slack = 0.0f32;

        for patch in 0..per_level * levels {
            let exact = clipmap_patch_bounds(patch, cam);
            match (field.patch_bounds(patch, cam), exact) {
                (PatchBounds::Hole, None) => continue,
                (PatchBounds::Box(lo, hi), Some((elo, ehi))) => {
                    checked += 1;
                    // Contém a caixa exacta: é o que garante que contém a malha.
                    assert!(
                        lo.y <= elo.y && hi.y >= ehi.y,
                        "patch {patch}: [{}, {}] não contém a exacta [{}, {}]",
                        lo.y,
                        hi.y,
                        elo.y,
                        ehi.y
                    );
                    // A folga é só o que a caixa da pirâmide tem **a mais**: ela
                    // contém a exacta, portanto a diferença nunca é negativa.
                    worst_slack = worst_slack.max((hi.y - lo.y) - (ehi.y - elo.y));

                    // E contém mesmo cada vértice que o VS emite.
                    let level = patch / per_level;
                    let p = patch % per_level;
                    let grid = clipmap_patch_grid();
                    let cell = CLIPMAP_CELL * (1u32 << level) as f32;
                    let snap = cell * 2.0;
                    let centre =
                        Vec2::new((cam.x / snap).floor() * snap, (cam.y / snap).floor() * snap);
                    let (c0x, c0y) = ((p % grid) * CLIPMAP_PATCH, (p / grid) * CLIPMAP_PATCH);
                    for cy in c0y..c0y + CLIPMAP_PATCH {
                        for cx in c0x..c0x + CLIPMAP_PATCH {
                            for (ox, oy) in OFFSETS {
                                let g = Vec2::new(
                                    (cx + ox) as f32 - n * 0.5,
                                    (cy + oy) as f32 - n * 0.5,
                                );
                                let w = centre + g * cell;
                                let h = terrain_height(w.x, w.y);
                                assert!(
                                    h >= lo.y && h <= hi.y,
                                    "patch {patch}: vértice a {h} fora de [{}, {}]",
                                    lo.y,
                                    hi.y
                                );
                            }
                        }
                    }
                }
                (got, want) => panic!("patch {patch}: campo deu {got:?}, exacta deu {want:?}"),
            }
        }
        assert!(checked > 100, "só {checked} patches verificados");
        // A folga é o preço de ler blocos em vez de avaliar vértices. Fica aqui
        // um número: se subir muito, o culling deixa de cortar e nota-se.
        assert!(
            worst_slack < 40.0,
            "folga de {worst_slack:.2} m contra a caixa exacta — grande de mais"
        );
    }

    /// Um tile por cozer é `Missing`, nunca uma caixa.
    ///
    /// O controlo negativo desta classe inteira: se faltar dados e a resposta for
    /// «terreno plano a zero», o culling aceita e o mundo abre um buraco.
    #[test]
    fn a_missing_tile_is_not_a_flat_world() {
        let field = HeightmapField::new();
        assert_eq!(field.height(10.0, 10.0), None);
        assert_eq!(field.min_max(0.0, 0.0, 10.0, 10.0), None);
        assert_eq!(
            field.patch_bounds(0, Vec2::ZERO),
            PatchBounds::Missing,
            "sem tiles a caixa não pode existir"
        );
    }

    /// Os tiles ladrilham o mundo: cada coordenada cai num e num só.
    #[test]
    fn tiles_tile_the_world() {
        assert_eq!(TILE_SIZE, TILE_N as f32 * TILE_SPACING);
        for &(x, z) in &[(0.0f32, 0.0f32), (127.9, 0.1), (-0.1, -128.0), (1000.0, -1.0)] {
            let (tx, tz) = tile_of(x, z);
            let (x0, z0) = (tx as f32 * TILE_SIZE, tz as f32 * TILE_SIZE);
            assert!(
                x >= x0 && x < x0 + TILE_SIZE && z >= z0 && z < z0 + TILE_SIZE,
                "({x}, {z}) caiu no tile ({tx}, {tz}), que cobre ({x0}, {z0})+{TILE_SIZE}"
            );
        }
        // E a amostra 0 de um tile é o canto dele.
        let (x, z) = sample_world(2, -3, 0, 0);
        assert_eq!((x, z), (2.0 * TILE_SIZE, -3.0 * TILE_SIZE));
    }

    /// Cozer é idempotente e conta o trabalho: é o número que o streaming vai usar.
    #[test]
    fn ensure_counts_the_work_once() {
        let mut field = HeightmapField::new();
        assert!(field.ensure(0, 0));
        assert!(!field.ensure(0, 0), "cozer duas vezes é trabalho a dobrar");
        assert_eq!(field.tile_count(), 1);
        let baked = field.ensure_around(0.0, 0.0, TILE_SIZE * 0.5);
        assert!(baked >= 3, "à volta da origem faltavam mais tiles: {baked}");
    }
}
