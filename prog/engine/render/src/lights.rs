//! Luzes pontuais em clusters.
//!
//! Até aqui a engine tinha **uma** luz direccional. Isso é a maior distância que
//! medimos contra a Dagor (`docs/Harpia-vs-Dagor.md` §2): por melhor que fosse o
//! BRDF, cada melhoria ficava confinada ao lóbulo especular de um sol.
//!
//! O frustum é dividido numa grelha 3D de clusters. Cada cluster guarda um
//! intervalo numa lista plana de índices de luz; um pixel descobre o seu cluster
//! a partir da posição no ecrã e da profundidade, e só percorre as luzes que
//! tocam nesse volume. Sem isto, cada pixel percorria todas as luzes.
//!
//! Z é **exponencial**, como nos froxels do fog: clusters lineares em Z dariam
//! fatias absurdamente finas ao pé da câmara e grosseiras ao longe.

use harpia_math::{Mat4, Vec3, Vec4};

pub const CLUSTER_X: u32 = 16;
pub const CLUSTER_Y: u32 = 9;
pub const CLUSTER_Z: u32 = 24;
pub const CLUSTER_COUNT: u32 = CLUSTER_X * CLUSTER_Y * CLUSTER_Z;
/// Tecto de índices na lista plana. Estourá-lo descarta luzes, o que se vê, por
/// isso é contado e reportado em vez de silencioso.
pub const MAX_LIGHT_INDICES: usize = 1 << 18;

/// Uma luz pontual. `radius` é o alcance a partir do qual não contribui — sem
/// ele, uma luz tocaria todos os clusters e o clustering não servia de nada.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PointLight {
    /// xyz posição no mundo, w raio.
    pub position_radius: Vec4,
    /// rgb cor × intensidade, w unused.
    pub color: Vec4,
}

impl PointLight {
    pub fn new(position: Vec3, radius: f32, color: Vec3, intensity: f32) -> Self {
        Self {
            position_radius: Vec4::new(position.x, position.y, position.z, radius),
            color: Vec4::new(
                color.x * intensity,
                color.y * intensity,
                color.z * intensity,
                0.0,
            ),
        }
    }

    pub fn position(&self) -> Vec3 {
        self.position_radius.truncate()
    }

    pub fn radius(&self) -> f32 {
        self.position_radius.w
    }
}

/// `(offset, count)` na lista plana de índices, por cluster.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ClusterRange {
    pub offset: u32,
    pub count: u32,
}

/// O resultado de atribuir luzes a clusters, pronto a subir para a GPU.
#[derive(Clone, Debug, Default)]
pub struct ClusterAssignment {
    pub ranges: Vec<ClusterRange>,
    pub indices: Vec<u32>,
    /// Luzes que não couberam em [`MAX_LIGHT_INDICES`]. Deve ser sempre 0.
    pub dropped: u32,
}

/// Profundidade de vista no limite de uma fatia. Exponencial entre `near` e `far`.
pub fn slice_depth(slice: u32, near: f32, far: f32) -> f32 {
    near * (far / near).powf(slice as f32 / CLUSTER_Z as f32)
}

/// A fatia onde uma profundidade de vista cai. Inverso de [`slice_depth`].
pub fn depth_slice(view_z: f32, near: f32, far: f32) -> u32 {
    if view_z <= near {
        return 0;
    }
    let t = (view_z / near).ln() / (far / near).ln();
    ((t * CLUSTER_Z as f32) as i32).clamp(0, CLUSTER_Z as i32 - 1) as u32
}

/// Atribui cada luz aos clusters que a sua esfera toca.
///
/// Faz-se em espaço de vista: a esfera projecta-se num intervalo de fatias em Z e
/// num rectângulo em XY, e marca-se só esse bloco. Testar todos os clusters
/// contra todas as luzes seria `CLUSTER_COUNT × luzes` — 3.4 milhões de testes
/// para mil luzes, por frame.
pub fn assign(lights: &[PointLight], view: Mat4, near: f32, far: f32, tan_half_fov: f32, aspect: f32) -> ClusterAssignment {
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); CLUSTER_COUNT as usize];

    for (i, light) in lights.iter().enumerate() {
        let p = view.transform_point3(light.position());
        // RH: a câmara olha para −Z, portanto a profundidade positiva é −z.
        let depth = -p.z;
        let r = light.radius();
        if depth + r <= near || depth - r >= far {
            continue;
        }

        let z0 = depth_slice((depth - r).max(near), near, far);
        let z1 = depth_slice((depth + r).min(far), near, far);

        // Extensão em XY calculada nas **duas** profundidades da esfera, e unida.
        //
        // Usar só a mais próxima parece conservador — é lá que o frustum é mais
        // estreito, logo a mesma distância no mundo cobre mais ecrã — mas também
        // empurra o *centro* para fora. Para uma luz descentrada isso desloca o
        // intervalo inteiro e perde as células do lado de dentro. O gate de
        // correcção apanhou exactamente isso: 1.68% dos canais diferentes da
        // força-bruta.
        let near_d = (depth - r).max(near);
        let far_d = (depth + r).min(far).max(near_d);
        let mut x0 = CLUSTER_X - 1;
        let mut x1 = 0;
        let mut y0 = CLUSTER_Y - 1;
        let mut y1 = 0;
        for d in [near_d, far_d] {
            let half_h = d * tan_half_fov;
            let half_w = half_h * aspect;
            let (a, b) = span(p.x - r, p.x + r, half_w, CLUSTER_X);
            x0 = x0.min(a);
            x1 = x1.max(b);
            // +Y no mundo é para cima; a grelha conta de cima para baixo.
            let (c, d2) = span(-p.y - r, -p.y + r, half_h, CLUSTER_Y);
            y0 = y0.min(c);
            y1 = y1.max(d2);
        }

        for z in z0..=z1 {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let c = (z * CLUSTER_Y + y) * CLUSTER_X + x;
                    buckets[c as usize].push(i as u32);
                }
            }
        }
    }

    let mut ranges = Vec::with_capacity(CLUSTER_COUNT as usize);
    let mut indices: Vec<u32> = Vec::new();
    let mut dropped = 0;
    for bucket in &buckets {
        let offset = indices.len() as u32;
        let room = MAX_LIGHT_INDICES.saturating_sub(indices.len());
        let take = bucket.len().min(room);
        dropped += (bucket.len() - take) as u32;
        indices.extend_from_slice(&bucket[..take]);
        ranges.push(ClusterRange {
            offset,
            count: take as u32,
        });
    }
    // Um buffer vazio não pode ser ligado, e um cluster sem luzes lê zero delas.
    if indices.is_empty() {
        indices.push(0);
    }
    ClusterAssignment {
        ranges,
        indices,
        dropped,
    }
}

/// Converte um intervalo `[lo, hi]` em espaço de vista para um intervalo de
/// células, dado o meio-extensão do frustum àquela profundidade.
fn span(lo: f32, hi: f32, half_extent: f32, cells: u32) -> (u32, u32) {
    let to_cell = |v: f32| {
        let t = (v / half_extent.max(1e-4) * 0.5 + 0.5) * cells as f32;
        (t.floor() as i32).clamp(0, cells as i32 - 1) as u32
    };
    let a = to_cell(lo);
    let b = to_cell(hi);
    (a.min(b), a.max(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_at(eye: Vec3) -> Mat4 {
        Mat4::look_at_rh(eye, eye + Vec3::NEG_Z, Vec3::Y)
    }

    const NEAR: f32 = 0.5;
    const FAR: f32 = 200.0;
    const TAN: f32 = 0.5774; // fov 60
    const ASPECT: f32 = 16.0 / 9.0;

    #[test]
    fn slices_are_exponential_and_round_trip() {
        assert!((slice_depth(0, NEAR, FAR) - NEAR).abs() < 1e-4);
        assert!((slice_depth(CLUSTER_Z, NEAR, FAR) - FAR).abs() < 0.1);
        // Fatias perto da câmara são muito mais finas que as de longe.
        let first = slice_depth(1, NEAR, FAR) - slice_depth(0, NEAR, FAR);
        let last = slice_depth(CLUSTER_Z, NEAR, FAR) - slice_depth(CLUSTER_Z - 1, NEAR, FAR);
        assert!(last > first * 20.0, "{last} vs {first}");
        for s in 0..CLUSTER_Z {
            let mid = (slice_depth(s, NEAR, FAR) + slice_depth(s + 1, NEAR, FAR)) * 0.5;
            assert_eq!(depth_slice(mid, NEAR, FAR), s, "fatia {s} não fecha");
        }
    }

    #[test]
    fn a_light_in_front_lands_in_some_cluster() {
        let l = PointLight::new(Vec3::new(0.0, 0.0, -10.0), 3.0, Vec3::ONE, 1.0);
        let a = assign(&[l], view_at(Vec3::ZERO), NEAR, FAR, TAN, ASPECT);
        let total: u32 = a.ranges.iter().map(|r| r.count).sum();
        assert!(total > 0, "a luz não foi atribuída a cluster nenhum");
        assert_eq!(a.dropped, 0);
    }

    /// Uma luz atrás da câmara ou para lá do far não pode ocupar clusters.
    #[test]
    fn lights_outside_the_frustum_are_skipped() {
        for pos in [Vec3::new(0.0, 0.0, 50.0), Vec3::new(0.0, 0.0, -900.0)] {
            let l = PointLight::new(pos, 2.0, Vec3::ONE, 1.0);
            let a = assign(&[l], view_at(Vec3::ZERO), NEAR, FAR, TAN, ASPECT);
            let total: u32 = a.ranges.iter().map(|r| r.count).sum();
            assert_eq!(total, 0, "luz em {pos:?} entrou em {total} clusters");
        }
    }

    /// O ponto todo do clustering: uma luz pequena não pode tocar o mundo todo.
    #[test]
    fn a_small_light_touches_few_clusters() {
        let l = PointLight::new(Vec3::new(0.0, 0.0, -20.0), 1.0, Vec3::ONE, 1.0);
        let a = assign(&[l], view_at(Vec3::ZERO), NEAR, FAR, TAN, ASPECT);
        let touched = a.ranges.iter().filter(|r| r.count > 0).count();
        assert!(touched > 0);
        assert!(
            touched < CLUSTER_COUNT as usize / 20,
            "tocou {touched} de {CLUSTER_COUNT} clusters — o clustering não está a apertar"
        );
    }

    /// Conservador: a luz tem de estar nos clusters que cobre, e um raio maior
    /// nunca pode tocar menos clusters.
    #[test]
    fn bigger_radius_never_shrinks_coverage() {
        let mut previous = 0;
        for r in [1.0f32, 4.0, 12.0, 30.0] {
            let l = PointLight::new(Vec3::new(0.0, 0.0, -25.0), r, Vec3::ONE, 1.0);
            let a = assign(&[l], view_at(Vec3::ZERO), NEAR, FAR, TAN, ASPECT);
            let touched = a.ranges.iter().filter(|c| c.count > 0).count();
            assert!(touched >= previous, "raio {r} tocou {touched}, antes {previous}");
            previous = touched;
        }
    }

    /// Trava o bug que o gate de correcção apanhou.
    ///
    /// Uma luz descentrada e funda: com a caixa calculada só à profundidade mais
    /// próxima, o intervalo de células deslocava-se para fora e perdia as células
    /// do lado de dentro. Aqui compara-se contra a verdade: quais os clusters
    /// cujo centro está mesmo dentro do raio da luz.
    #[test]
    fn coverage_contains_every_cluster_the_sphere_reaches() {
        let view = view_at(Vec3::ZERO);
        for (px, depth, r) in [(12.0f32, 30.0f32, 6.0f32), (-20.0, 45.0, 9.0), (4.0, 8.0, 3.0)] {
            let l = PointLight::new(Vec3::new(px, 0.0, -depth), r, Vec3::ONE, 1.0);
            let a = assign(&[l], view, NEAR, FAR, TAN, ASPECT);
            // Amostra o volume da luz e confirma que cada ponto cai num cluster
            // que a lista marcou.
            for i in 0..12 {
                for j in 0..12 {
                    let t = (i as f32 / 11.0 - 0.5) * 2.0 * r * 0.9;
                    let u = (j as f32 / 11.0 - 0.5) * 2.0 * r * 0.9;
                    let d = depth + u;
                    if d <= NEAR || d >= FAR {
                        continue;
                    }
                    let half_h = d * TAN;
                    let half_w = half_h * ASPECT;
                    let cx = (((px + t) / half_w * 0.5 + 0.5) * CLUSTER_X as f32).floor();
                    if !(0.0..CLUSTER_X as f32).contains(&cx) {
                        continue;
                    }
                    let cy = CLUSTER_Y / 2;
                    let cz = depth_slice(d, NEAR, FAR);
                    let c = ((cz * CLUSTER_Y + cy) * CLUSTER_X + cx as u32) as usize;
                    assert!(
                        a.ranges[c].count > 0,
                        "ponto dentro da luz (px={px}, d={d:.1}) caiu no cluster {c}, \
                         que não tem luz nenhuma"
                    );
                }
            }
        }
    }

    #[test]
    fn ranges_point_inside_the_index_list() {
        let lights: Vec<PointLight> = (0..64)
            .map(|i| {
                let f = i as f32;
                PointLight::new(Vec3::new(f.sin() * 20.0, 0.0, -5.0 - f), 4.0, Vec3::ONE, 1.0)
            })
            .collect();
        let a = assign(&lights, view_at(Vec3::ZERO), NEAR, FAR, TAN, ASPECT);
        assert_eq!(a.ranges.len(), CLUSTER_COUNT as usize);
        for r in &a.ranges {
            assert!(
                (r.offset + r.count) as usize <= a.indices.len(),
                "intervalo {r:?} sai da lista de {} índices",
                a.indices.len()
            );
        }
        assert_eq!(a.dropped, 0);
    }
}
