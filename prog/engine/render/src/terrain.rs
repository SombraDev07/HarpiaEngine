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

/// Alcance do último nível, em unidades de mundo.
pub fn clipmap_range() -> f32 {
    CLIPMAP_CELL * (1u32 << (CLIPMAP_LEVELS - 1)) as f32 * CLIPMAP_N as f32 * 0.5
}

/// Hash de valor em `[0, 1)`. **Tem de bater com o GLSL exactamente**, por isso é
/// escrito com as mesmas operações e as mesmas constantes.
fn hash2(x: f32, y: f32) -> f32 {
    let d = x * 127.1 + y * 311.7;
    let s = d.sin() * 43758.545;
    s - s.floor()
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
    pub _pad: u32,
}

impl Default for TerrainCb {
    fn default() -> Self {
        Self {
            view_proj: Mat4::IDENTITY,
            camera_pos: Vec4::W,
            sun_dir: Vec4::Y,
            sun_color: Vec4::new(3.4, 3.2, 2.9, 1.0),
            params: Vec4::new(
                CLIPMAP_CELL,
                60.0,
                CLIPMAP_N as f32,
                CLIPMAP_LEVELS as f32,
            ),
            sky_zenith: Vec4::new(0.22, 0.38, 0.72, 0.0),
            sky_horizon: Vec4::new(0.62, 0.72, 0.86, 0.0),
            inv_extent: Vec2::ONE,
            scene: 0,
            _pad: 0,
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
        ];
        for shader in [
            "prog/samples/gates/terrain/shaders/terrain.vs.glsl",
            "prog/samples/gates/terrain/shaders/terrain.ps.glsl",
            "prog/samples/gates/terrain/shaders/blit.ps.glsl",
        ] {
            crate::spvasm_layout::assert_glsl_offsets(shader, &expected);
        }
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

    /// O alcance tem de cobrir bem mais do que o nível 0, senão os níveis extra
    /// não estão a fazer nada.
    #[test]
    fn levels_extend_the_range() {
        let level0 = CLIPMAP_CELL * CLIPMAP_N as f32 * 0.5;
        assert!(clipmap_range() > level0 * 30.0, "{} vs {level0}", clipmap_range());
        assert_eq!(clipmap_vertex_count(), 64 * 64 * 6);
    }
}
