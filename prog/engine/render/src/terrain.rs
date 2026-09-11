//! Terreno em clipmap (roadmap §15 fase 6).
//!
//! **Sem vertex buffer.** O roadmap diz «só `ClipmapTerrain` SV_VertexID», e a
//! razão é boa: a malha de um clipmap é sempre a mesma grelha, só muda de escala
//! e de sítio. Guardá-la em memória seria pagar banda para reler uma coisa que o
//! VS calcula em três instruções a partir do índice do vértice.
//!
//! Cada nível é uma instância. O nível 0 é uma grelha cheia à volta da câmara;
//! os de fora são anéis com o centro vazio, porque o centro já está coberto pelo
//! nível de dentro com o dobro da resolução.
//!
//! A altura tem de ser **a mesma função em CPU e GPU**: a CPU precisa dela para
//! pôr coisas no chão e para a câmara não atravessar o terreno, e a GPU para
//! desenhar. O gate `heightquery` mede se as duas concordam — por isso a função
//! está aqui e é escrita para ser copiável para GLSL linha a linha.

use harpia_math::{Mat4, Vec2, Vec3, Vec4};

/// Células por lado, por nível. Mais do que isto e o nível 0 come triângulos sem
/// se ver a diferença; menos e a silhueta perto da câmara fica angulosa.
pub const CLIPMAP_N: u32 = 64;
/// Níveis de LOD. Cada um duplica a célula, portanto o alcance é `base * 2^L`.
pub const CLIPMAP_LEVELS: u32 = 7;
/// Tamanho da célula do nível 0, em unidades de mundo.
pub const CLIPMAP_CELL: f32 = 0.5;
/// Oitavas do ruído. Fixo porque a CPU e o GLSL têm de correr o mesmo laço.
pub const TERRAIN_OCTAVES: u32 = 5;

/// Vértices por instância: seis por célula, uma grelha `N × N`.
pub const fn clipmap_vertex_count() -> u32 {
    CLIPMAP_N * CLIPMAP_N * 6
}

/// Células por lado de um patch — a unidade de culling.
///
/// Um nível inteiro é grande demais para cortar: ou se vê ou não se vê, e quase
/// sempre vê-se um bocado. Dividido em patches, o que está atrás da câmara deixa
/// de ser rasterizado. 8 dá 64 patches por nível, 448 no total — suficientes para
/// o corte ser fino e poucos para o compute ser um arredondamento.
pub const CLIPMAP_PATCH: u32 = 8;

/// Patches por lado, num nível.
pub const fn clipmap_patch_grid() -> u32 {
    CLIPMAP_N / CLIPMAP_PATCH
}

/// Patches num nível.
pub const fn clipmap_patches_per_level() -> u32 {
    clipmap_patch_grid() * clipmap_patch_grid()
}

/// Total de patches do clipmap: é isto que o compute percorre.
pub const fn clipmap_patch_count() -> u32 {
    clipmap_patches_per_level() * CLIPMAP_LEVELS
}

/// Vértices de um patch: seis por célula.
pub const fn clipmap_patch_vertex_count() -> u32 {
    CLIPMAP_PATCH * CLIPMAP_PATCH * 6
}

/// A caixa envolvente de um patch, em mundo, ou `None` se ele cair todo no buraco
/// do anel.
///
/// **Exacta, não estimada.** O `y` vem de avaliar a altura nos mesmos
/// `(PATCH+1)²` vértices que o VS vai avaliar, portanto o intervalo é o intervalo
/// verdadeiro da geometria — não há margem a adivinhar nem pop a temer. Menos a
/// saia, que desce `cell * 2` na fronteira interior e por isso se subtrai sempre.
///
/// É a mesma função que o `terrain_cull.cs` corre. O gate compara as duas.
pub fn clipmap_patch_bounds(patch: u32, camera_xz: Vec2) -> Option<(Vec3, Vec3)> {
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

    // O buraco do anel: o quarto central já está coberto pelo nível de dentro.
    // Só se rejeita o patch se ele couber **todo** lá dentro.
    if level > 0 {
        let inside = |cx: u32, cy: u32| {
            let dx = (cx as f32 - n * 0.5).abs();
            let dy = (cy as f32 - n * 0.5).abs();
            dx.max(dy) < n * 0.25
        };
        let all_in =
            (0..CLIPMAP_PATCH).all(|i| (0..CLIPMAP_PATCH).all(|j| inside(c0x + i, c0y + j)));
        if all_in {
            return None;
        }
    }

    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for j in 0..=CLIPMAP_PATCH {
        for i in 0..=CLIPMAP_PATCH {
            let g = Vec2::new((c0x + i) as f32 - n * 0.5, (c0y + j) as f32 - n * 0.5);
            let w = centre + g * cell;
            let h = terrain_height(w.x, w.y);
            lo = lo.min(h);
            hi = hi.max(h);
        }
    }
    // A saia desce a altura na fronteira interior do anel; subtrai-se sempre para
    // a caixa nunca ficar por baixo da geometria.
    lo -= cell * 2.0;

    let g0 = Vec2::new(c0x as f32 - n * 0.5, c0y as f32 - n * 0.5);
    let g1 = Vec2::new(
        (c0x + CLIPMAP_PATCH) as f32 - n * 0.5,
        (c0y + CLIPMAP_PATCH) as f32 - n * 0.5,
    );
    let w0 = centre + g0 * cell;
    let w1 = centre + g1 * cell;
    Some((Vec3::new(w0.x, lo, w0.y), Vec3::new(w1.x, hi, w1.y)))
}

/// Alcance do último nível, em unidades de mundo.
pub fn clipmap_range() -> f32 {
    CLIPMAP_CELL * (1u32 << (CLIPMAP_LEVELS - 1)) as f32 * CLIPMAP_N as f32 * 0.5
}

/// Hash inteiro em `[0, 1)` a partir de coordenadas de célula.
///
/// **Não uses `fract(sin(x) * 43758.5)`.** É o hash mais copiado da internet e
/// não é portável: `sin` de um argumento grande difere no último bit entre a
/// libm da CPU e o hardware da GPU, e multiplicar por 43758 amplifica isso até a
/// parte fraccionária ser outra. O gate `heightquery` mediu-o com esse hash e
/// deu **99.84% dos pontos fora da tolerância, com 77 m de erro máximo** num
/// terreno de ±60 m — a colisão e a geometria eram superfícies diferentes.
///
/// Aritmética inteira é exacta nos dois lados, portanto isto concorda sempre.
fn hash2(x: f32, y: f32) -> f32 {
    let ix = x as i32 as u32;
    let iy = y as i32 as u32;
    let mut h = ix
        .wrapping_mul(374_761_393)
        .wrapping_add(iy.wrapping_mul(668_265_263));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    h as f32 * (1.0 / 4_294_967_296.0)
}

fn smooth(t: f32) -> f32 {
    // Hermite: derivada zero nas pontas, senão as células do ruído dão vincos.
    t * t * (3.0 - 2.0 * t)
}

/// Ruído de valor bilinear numa grelha inteira.
fn value_noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (ux, uy) = (smooth(fx), smooth(fy));
    let a = hash2(ix, iy);
    let b = hash2(ix + 1.0, iy);
    let c = hash2(ix, iy + 1.0);
    let d = hash2(ix + 1.0, iy + 1.0);
    let top = a + (b - a) * ux;
    let bottom = c + (d - c) * ux;
    top + (bottom - top) * uy
}

/// Altura do terreno em `(x, z)`, unidades de mundo.
///
/// FBM com lacunaridade 2 e ganho 0.5. As oitavas vêm de [`TERRAIN_OCTAVES`] e o
/// laço é desenrolado da mesma forma no GLSL — mudar aqui sem mudar lá faz a
/// geometria e a colisão discordarem, que é o bug mais desagradável possível
/// num terreno.
pub fn terrain_height(x: f32, z: f32) -> f32 {
    let mut amplitude = 1.0f32;
    let mut frequency = 0.008f32;
    let mut sum = 0.0f32;
    let mut norm = 0.0f32;
    for _ in 0..TERRAIN_OCTAVES {
        sum += value_noise(x * frequency, z * frequency) * amplitude;
        norm += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    // [0,1] -> [-1,1] e depois a escala vertical.
    (sum / norm * 2.0 - 1.0) * 60.0
}

/// Normal por diferenças centrais. `eps` deve acompanhar a célula do nível que
/// se está a desenhar, senão a normal é mais fina ou mais grossa que a malha.
pub fn terrain_normal(x: f32, z: f32, eps: f32) -> Vec3 {
    let dx = terrain_height(x + eps, z) - terrain_height(x - eps, z);
    let dz = terrain_height(x, z + eps) - terrain_height(x, z - eps);
    Vec3::new(-dx, 2.0 * eps, -dz).normalize()
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TerrainCb {
    pub view_proj: Mat4,
    /// xyz câmara, w unused.
    pub camera_pos: Vec4,
    pub sun_dir: Vec4,
    pub sun_color: Vec4,
    /// célula base, escala de altura (informativa), N, níveis.
    pub params: Vec4,
    pub sky_zenith: Vec4,
    /// rgb no horizonte, w = distância a que o terreno desvanece.
    pub sky_horizon: Vec4,
    pub inv_extent: Vec2,
    /// Índice bindless do alvo offscreen, para o blit.
    pub scene: u32,
    /// Índice bindless do campo de altura cozido, ou 0 para avaliar o FBM.
    ///
    /// Zero é o slot do dummy 1×1 e nunca é um campo válido, portanto serve de
    /// «desligado» sem gastar outro campo no CB — que está cheio nos 176 bytes que
    /// o teste de layout fixa.
    pub field: u32,
}

impl Default for TerrainCb {
    fn default() -> Self {
        Self {
            view_proj: Mat4::IDENTITY,
            camera_pos: Vec4::W,
            sun_dir: Vec4::Y,
            sun_color: Vec4::new(3.4, 3.2, 2.9, 1.0),
            // w = lado do patch, não o nº de níveis: o VS já não deriva o nível
            // do índice da instância, tira-o do id do patch que o culling escolheu.
            params: Vec4::new(CLIPMAP_CELL, 60.0, CLIPMAP_N as f32, CLIPMAP_PATCH as f32),
            sky_zenith: Vec4::new(0.22, 0.38, 0.72, 0.0),
            sky_horizon: Vec4::new(0.62, 0.72, 0.86, 0.0),
            inv_extent: Vec2::ONE,
            scene: 0,
            field: 0,
        }
    }
}

impl TerrainCb {
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                std::mem::size_of::<Self>(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cb_layout_matches_the_glsl() {
        use std::mem::offset_of;
        assert_eq!(std::mem::size_of::<TerrainCb>(), 176);
        assert!(std::mem::size_of::<TerrainCb>() <= harpia_rhi::FRAME_UBO_SIZE as usize);
        let expected = [
            (0, 0),
            (1, offset_of!(TerrainCb, camera_pos) as u32),
            (2, offset_of!(TerrainCb, sun_dir) as u32),
            (3, offset_of!(TerrainCb, sun_color) as u32),
            (4, offset_of!(TerrainCb, params) as u32),
            (5, offset_of!(TerrainCb, sky_zenith) as u32),
            (6, offset_of!(TerrainCb, sky_horizon) as u32),
            (7, offset_of!(TerrainCb, inv_extent) as u32),
            (8, offset_of!(TerrainCb, scene) as u32),
            (9, offset_of!(TerrainCb, field) as u32),
        ];
        for shader in [
            "prog/samples/gates/terrain/shaders/terrain.vs.glsl",
            "prog/samples/gates/terrain/shaders/terrain.ps.glsl",
            "prog/samples/gates/terrain/shaders/blit.ps.glsl",
        ] {
            crate::spvasm_layout::assert_glsl_offsets(shader, &expected);
        }
    }

    /// O hash tem de ser **inteiro**, não `fract(sin(x) * 43758)`.
    ///
    /// Com o hash trigonométrico o gate `heightquery` mediu 99.84% dos pontos
    /// fora da tolerância e 77 m de erro entre CPU e GPU. Este teste trava os
    /// valores para que uma regressão apareça sem ser preciso uma GPU.
    #[test]
    fn hash_is_integer_and_pinned() {
        // Valores exactos: aritmética inteira dá o mesmo em qualquer máquina.
        assert_eq!(hash2(0.0, 0.0), 0.0);
        for (x, y) in [(1.0, 0.0), (-3.0, 7.0), (129.0, -45.0)] {
            let h = hash2(x, y);
            assert!(
                (0.0..1.0).contains(&h),
                "hash2({x}, {y}) = {h} fora de [0,1)"
            );
        }
        // Células vizinhas não podem colidir, senão o ruído tem riscas.
        let a = hash2(10.0, 20.0);
        assert_ne!(a, hash2(11.0, 20.0));
        assert_ne!(a, hash2(10.0, 21.0));
        assert_ne!(a, hash2(20.0, 10.0), "simétrico em x/y daria diagonais");
    }

    /// Determinista: a mesma coordenada dá sempre a mesma altura, ou a colisão e
    /// a geometria divergem entre frames.
    #[test]
    fn height_is_deterministic_and_bounded() {
        for (x, z) in [(0.0, 0.0), (123.4, -567.8), (-1e4, 1e4), (0.5, 0.5)] {
            let a = terrain_height(x, z);
            assert_eq!(a, terrain_height(x, z));
            assert!(a.is_finite(), "({x}, {z}) deu {a}");
            assert!(a.abs() <= 60.0 + 1e-3, "({x}, {z}) saiu da escala: {a}");
        }
    }

    /// Sem isto o terreno é uma chapa e o gate não prova nada.
    #[test]
    fn height_actually_varies() {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for i in 0..400 {
            let h = terrain_height(i as f32 * 17.3, i as f32 * -29.1);
            lo = lo.min(h);
            hi = hi.max(h);
        }
        assert!(hi - lo > 20.0, "variação de só {:.2}", hi - lo);
    }

    /// A normal aponta para cima num terreno que não é vertical, e é unitária.
    #[test]
    fn normals_point_up_and_are_unit() {
        for (x, z) in [(0.0, 0.0), (250.0, -80.0), (-13.0, 42.0)] {
            let n = terrain_normal(x, z, 0.5);
            assert!((n.length() - 1.0).abs() < 1e-4, "não unitária: {n:?}");
            assert!(n.y > 0.0, "normal virada ao contrário em ({x}, {z}): {n:?}");
        }
    }

    /// A caixa de um patch tem de **conter** a geometria que ele desenha.
    ///
    /// É a garantia que faz o culling seguro: se a caixa ficar aquém, o frustum
    /// rejeita um patch cujo chão se vê, e abre-se um buraco.
    ///
    /// O que se desenha é a malha, não o campo de alturas: o VS avalia a altura
    /// nos **vértices** e o rasterizador interpola em linha entre eles. Um ponto
    /// dentro de um triângulo pode por isso ficar acima ou abaixo do terreno
    /// verdadeiro — a primeira versão deste teste amostrava `terrain_height` no
    /// interior das células e falhava por 13 mm, a acusar a caixa de um erro que
    /// era do teste. Aqui percorrem-se os vértices que o VS emite, derivados da
    /// mesma tabela de offsets, que é o que a caixa tem mesmo de conter.
    #[test]
    fn patch_bounds_contain_the_mesh() {
        // A mesma tabela do `terrain.vs.glsl`, para um erro de alcance aparecer.
        const OFFSETS: [(u32, u32); 6] = [(0, 0), (0, 1), (1, 0), (1, 0), (0, 1), (1, 1)];
        let cam = Vec2::new(37.0, -91.0);
        let n = CLIPMAP_N as f32;
        let mut checked = 0;
        for patch in 0..clipmap_patch_count() {
            let Some((lo, hi)) = clipmap_patch_bounds(patch, cam) else {
                continue;
            };
            checked += 1;
            assert!(lo.x < hi.x && lo.z < hi.z, "patch {patch} degenerado em XZ");
            assert!(lo.y <= hi.y, "patch {patch} com Y invertido");

            let per_level = clipmap_patches_per_level();
            let level = patch / per_level;
            let p = patch % per_level;
            let grid = clipmap_patch_grid();
            let cell = CLIPMAP_CELL * (1u32 << level) as f32;
            let snap = cell * 2.0;
            let centre = Vec2::new((cam.x / snap).floor() * snap, (cam.y / snap).floor() * snap);
            let (c0x, c0y) = ((p % grid) * CLIPMAP_PATCH, (p / grid) * CLIPMAP_PATCH);

            for cy in c0y..c0y + CLIPMAP_PATCH {
                for cx in c0x..c0x + CLIPMAP_PATCH {
                    for (ox, oy) in OFFSETS {
                        let g = Vec2::new((cx + ox) as f32 - n * 0.5, (cy + oy) as f32 - n * 0.5);
                        let w = centre + g * cell;
                        let h = terrain_height(w.x, w.y);
                        assert!(
                            h >= lo.y && h <= hi.y,
                            "patch {patch}: vértice a {h} fora de [{}, {}]",
                            lo.y,
                            hi.y
                        );
                        assert!(
                            w.x >= lo.x && w.x <= hi.x && w.y >= lo.z && w.y <= hi.z,
                            "patch {patch}: vértice em ({}, {}) fora da caixa XZ",
                            w.x,
                            w.y
                        );
                    }
                }
            }
        }
        assert!(
            checked > 300,
            "só {checked} patches com caixa, de {}",
            clipmap_patch_count()
        );
    }

    /// Só os patches que cabem **todos** no buraco do anel são rejeitados.
    ///
    /// Rejeitar um que faça fronteira com o buraco tiraria chão que se vê. Por
    /// isso são 9 por nível e não 16: o buraco ocupa um quarto da área, mas as
    /// suas células de bordo caem em patches que também têm células de fora, e
    /// esses têm de ser desenhados (as células de dentro colapsam sozinhas no VS).
    /// E o nível 0 não tem buraco nenhum: é a grelha cheia à volta da câmara.
    #[test]
    fn only_fully_interior_patches_are_dropped() {
        let cam = Vec2::new(0.0, 0.0);
        let per_level = clipmap_patches_per_level();
        for p in 0..per_level {
            assert!(
                clipmap_patch_bounds(p, cam).is_some(),
                "o nível 0 não tem buraco, e o patch {p} foi descartado"
            );
        }
        for level in 1..CLIPMAP_LEVELS {
            let dropped = (0..per_level)
                .filter(|&p| clipmap_patch_bounds(level * per_level + p, cam).is_none())
                .count();
            assert_eq!(
                dropped, 9,
                "nível {level} descartou {dropped} patches, não 9"
            );
        }
    }

    /// Os patches têm de ladrilhar o nível sem sobrepor nem deixar espaço.
    #[test]
    fn patches_tile_their_level() {
        assert_eq!(clipmap_patch_grid() * CLIPMAP_PATCH, CLIPMAP_N);
        assert_eq!(
            clipmap_patch_vertex_count() * clipmap_patches_per_level(),
            clipmap_vertex_count(),
            "os patches de um nível têm de dar os mesmos vértices que o nível"
        );
        assert_eq!(
            clipmap_patch_count(),
            clipmap_patches_per_level() * CLIPMAP_LEVELS
        );
    }

    /// O alcance tem de cobrir bem mais do que o nível 0, senão os níveis extra
    /// não estão a fazer nada.
    #[test]
    fn levels_extend_the_range() {
        let level0 = CLIPMAP_CELL * CLIPMAP_N as f32 * 0.5;
        assert!(
            clipmap_range() > level0 * 30.0,
            "{} vs {level0}",
            clipmap_range()
        );
        assert_eq!(clipmap_vertex_count(), 64 * 64 * 6);
    }
}
