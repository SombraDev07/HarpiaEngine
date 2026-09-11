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

/// Uma luz pontual ou um projector. `radius` é o alcance a partir do qual não
/// contribui — sem ele, uma luz tocaria todos os clusters e o clustering não
/// servia de nada.
///
/// Omni e spot são **a mesma estrutura**, e não duas. Um spot é uma luz pontual
/// com um cone; uma omni é um spot cujo cone é a esfera inteira, que é o que
/// `cos_outer = -1` diz. Assim há uma lista de luzes, uma lista de índices, e o
/// shader percorre-as sem saber de que tipo são até ler o `w`. Duas listas
/// separadas obrigariam a dois percursos por pixel e a dois caminhos de código
/// que divergem com o tempo.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Light {
    /// xyz posição no mundo, w raio.
    pub position_radius: Vec4,
    /// rgb cor × intensidade, w cosseno do ângulo **interior** do cone.
    pub color: Vec4,
    /// xyz direcção do cone (normalizada, a apontar para fora da luz),
    /// w cosseno do ângulo **exterior**. `-1` = omni, e nesse caso o xyz não é lido.
    pub dir_cos_outer: Vec4,
}

impl Light {
    /// Uma luz omni: o cone é a esfera toda.
    pub fn point(position: Vec3, radius: f32, color: Vec3, intensity: f32) -> Self {
        Self {
            position_radius: Vec4::new(position.x, position.y, position.z, radius),
            color: Vec4::new(
                color.x * intensity,
                color.y * intensity,
                color.z * intensity,
                1.0,
            ),
            dir_cos_outer: Vec4::new(0.0, -1.0, 0.0, -1.0),
        }
    }

    /// Um projector. Os ângulos são **meio-ângulos** do cone, em radianos, e
    /// `inner <= outer` — a queda faz-se entre os dois.
    pub fn spot(
        position: Vec3,
        direction: Vec3,
        radius: f32,
        inner: f32,
        outer: f32,
        color: Vec3,
        intensity: f32,
    ) -> Self {
        let d = direction.normalize_or_zero();
        // O exterior nunca chega a -1: isso seria uma omni disfarçada, e o teste
        // de cone deixaria de cortar o que quer que fosse.
        let outer = outer.clamp(1e-3, std::f32::consts::FRAC_PI_2 * 0.999);
        let inner = inner.clamp(0.0, outer);
        Self {
            position_radius: Vec4::new(position.x, position.y, position.z, radius),
            color: Vec4::new(
                color.x * intensity,
                color.y * intensity,
                color.z * intensity,
                inner.cos(),
            ),
            dir_cos_outer: Vec4::new(d.x, d.y, d.z, outer.cos()),
        }
    }

    pub fn position(&self) -> Vec3 {
        self.position_radius.truncate()
    }

    pub fn radius(&self) -> f32 {
        self.position_radius.w
    }

    pub fn is_spot(&self) -> bool {
        self.dir_cos_outer.w > -1.0
    }

    pub fn direction(&self) -> Vec3 {
        self.dir_cos_outer.truncate()
    }

    pub fn cos_outer(&self) -> f32 {
        self.dir_cos_outer.w
    }
}

/// Nome antigo, de quando só havia omni.
pub type PointLight = Light;

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
pub fn assign(
    lights: &[Light],
    view: Mat4,
    near: f32,
    far: f32,
    tan_half_fov: f32,
    aspect: f32,
) -> ClusterAssignment {
    assign_counted(lights, view, near, far, tan_half_fov, aspect).0
}

/// Como [`assign`], mas devolve também **quantos** slots o teste de cone poupou.
///
/// O número só faz sentido comparado com alguma coisa, e a coisa é a esfera
/// envolvente: um spot de raio R ocupa, se o tratarmos como omni, exactamente os
/// mesmos clusters que uma omni de raio R. O que o cone poupa é a diferença — e é
/// isso que justifica o custo do teste.
pub fn assign_counted(
    lights: &[Light],
    view: Mat4,
    near: f32,
    far: f32,
    tan_half_fov: f32,
    aspect: f32,
) -> (ClusterAssignment, ConeStats) {
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); CLUSTER_COUNT as usize];
    let mut stats = ConeStats::default();

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

        // Direcção do cone em espaço de vista. A `view` tem translação, por isso
        // uma direcção transforma-se como vector e não como ponto — passar por
        // `transform_point3` daria uma direcção deslocada pela posição da câmara,
        // e o cone apontaria para onde lhe desse na gana.
        let spot = light.is_spot();
        let dir_v = if spot {
            view.transform_vector3(light.direction())
                .normalize_or_zero()
        } else {
            Vec3::ZERO
        };
        let sin_outer = if spot {
            (1.0 - light.cos_outer() * light.cos_outer())
                .max(0.0)
                .sqrt()
        } else {
            0.0
        };

        for z in z0..=z1 {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let c = (z * CLUSTER_Y + y) * CLUSTER_X + x;
                    stats.sphere_slots += 1;
                    if spot {
                        let (centre, radius) =
                            cluster_bounds(x, y, z, near, far, tan_half_fov, aspect);
                        if !cone_touches_sphere(
                            p,
                            dir_v,
                            r,
                            light.cos_outer(),
                            sin_outer,
                            centre,
                            radius,
                        ) {
                            continue;
                        }
                    }
                    stats.cone_slots += 1;
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
    (
        ClusterAssignment {
            ranges,
            indices,
            dropped,
        },
        stats,
    )
}

/// Quantos slots de cluster a esfera envolvente pediria, e quantos ficaram
/// depois do teste de cone.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConeStats {
    pub sphere_slots: u64,
    pub cone_slots: u64,
}

impl ConeStats {
    /// Fracção cortada pelo cone, em `[0, 1]`.
    pub fn saved(&self) -> f32 {
        if self.sphere_slots == 0 {
            return 0.0;
        }
        1.0 - self.cone_slots as f32 / self.sphere_slots as f32
    }
}

/// Centro e raio de uma esfera que contém o cluster, em espaço de vista.
///
/// O cluster é um tronco de pirâmide, não uma caixa; envolvê-lo numa esfera é
/// conservador — pode aceitar um cone que na verdade passa ao lado, nunca
/// rejeitar um que toque. Para o culling é esse o lado seguro do erro.
fn cluster_bounds(
    x: u32,
    y: u32,
    z: u32,
    near: f32,
    far: f32,
    tan_half_fov: f32,
    aspect: f32,
) -> (Vec3, f32) {
    let d0 = slice_depth(z, near, far);
    let d1 = slice_depth(z + 1, near, far);
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for d in [d0, d1] {
        let half_h = d * tan_half_fov;
        let half_w = half_h * aspect;
        for (ix, iy) in [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)] {
            let sx = (ix as f32 / CLUSTER_X as f32 * 2.0 - 1.0) * half_w;
            // A grelha conta de cima para baixo; +Y em espaço de vista é para cima.
            let sy = -(iy as f32 / CLUSTER_Y as f32 * 2.0 - 1.0) * half_h;
            let c = Vec3::new(sx, sy, -d);
            lo = lo.min(c);
            hi = hi.max(c);
        }
    }
    let centre = (lo + hi) * 0.5;
    (centre, (hi - centre).length())
}

/// Um cone de ângulo `acos(cos_half)`, ápice em `apex`, alcance `range`, toca a
/// esfera?
///
/// `closest` é a distância do centro da esfera à superfície do cone. As três
/// rejeições são: fora do ângulo, para lá da ponta, e atrás do ápice.
///
/// A terceira é **redundante** nesta formulação, e sei-o porque a tirei e nenhum
/// teste falhou: atrás do ápice o `along` é negativo, portanto `-along·sin_half`
/// é positivo e cresce, e o teste de ângulo já rejeita o cone espelhado sozinho.
/// Fica escrita à mesma — é um `<` barato que corta cedo o caso comum de uma luz
/// virada ao contrário da câmara — mas não é ela que garante a correcção, e dizer
/// que era seria mentira que alguém acreditaria.
fn cone_touches_sphere(
    apex: Vec3,
    dir: Vec3,
    range: f32,
    cos_half: f32,
    sin_half: f32,
    centre: Vec3,
    radius: f32,
) -> bool {
    let v = centre - apex;
    let along = v.dot(dir);
    let perp = (v.length_squared() - along * along).max(0.0).sqrt();
    let closest = cos_half * perp - along * sin_half;
    !(closest > radius || along > radius + range || along < -radius)
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
        let l = Light::point(Vec3::new(0.0, 0.0, -10.0), 3.0, Vec3::ONE, 1.0);
        let a = assign(&[l], view_at(Vec3::ZERO), NEAR, FAR, TAN, ASPECT);
        let total: u32 = a.ranges.iter().map(|r| r.count).sum();
        assert!(total > 0, "a luz não foi atribuída a cluster nenhum");
        assert_eq!(a.dropped, 0);
    }

    /// Uma luz atrás da câmara ou para lá do far não pode ocupar clusters.
    #[test]
    fn lights_outside_the_frustum_are_skipped() {
        for pos in [Vec3::new(0.0, 0.0, 50.0), Vec3::new(0.0, 0.0, -900.0)] {
            let l = Light::point(pos, 2.0, Vec3::ONE, 1.0);
            let a = assign(&[l], view_at(Vec3::ZERO), NEAR, FAR, TAN, ASPECT);
            let total: u32 = a.ranges.iter().map(|r| r.count).sum();
            assert_eq!(total, 0, "luz em {pos:?} entrou em {total} clusters");
        }
    }

    /// O ponto todo do clustering: uma luz pequena não pode tocar o mundo todo.
    #[test]
    fn a_small_light_touches_few_clusters() {
        let l = Light::point(Vec3::new(0.0, 0.0, -20.0), 1.0, Vec3::ONE, 1.0);
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
            let l = Light::point(Vec3::new(0.0, 0.0, -25.0), r, Vec3::ONE, 1.0);
            let a = assign(&[l], view_at(Vec3::ZERO), NEAR, FAR, TAN, ASPECT);
            let touched = a.ranges.iter().filter(|c| c.count > 0).count();
            assert!(
                touched >= previous,
                "raio {r} tocou {touched}, antes {previous}"
            );
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
        for (px, depth, r) in [
            (12.0f32, 30.0f32, 6.0f32),
            (-20.0, 45.0, 9.0),
            (4.0, 8.0, 3.0),
        ] {
            let l = Light::point(Vec3::new(px, 0.0, -depth), r, Vec3::ONE, 1.0);
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
                Light::point(
                    Vec3::new(f.sin() * 20.0, 0.0, -5.0 - f),
                    4.0,
                    Vec3::ONE,
                    1.0,
                )
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

#[cfg(test)]
mod spot_tests {
    use super::*;

    /// A câmara **não** está na origem, de propósito.
    ///
    /// Com ela na origem a `view` não tem translação, e aí `transform_point3` e
    /// `transform_vector3` dão o mesmo — o teste deixava passar o erro de
    /// transformar a direcção do cone como se fosse um ponto. Descobri-o a
    /// quebrar o código de propósito e a ver o teste passar.
    fn camera() -> (Mat4, f32, f32, f32, f32) {
        let eye = Vec3::new(-7.0, 3.5, 11.0);
        let view = Mat4::look_at_rh(eye, eye + Vec3::new(0.3, -0.2, -1.0), Vec3::Y);
        (
            view,
            0.1,
            100.0,
            (60.0_f32.to_radians() * 0.5).tan(),
            16.0 / 9.0,
        )
    }

    /// O `camera()` tem de ter translação, senão os testes abaixo não distinguem
    /// uma direcção transformada como vector de uma transformada como ponto.
    #[test]
    fn the_test_camera_is_not_at_the_origin() {
        let (view, ..) = camera();
        let d = Vec3::new(0.0, 0.0, -1.0);
        assert!(
            (view.transform_vector3(d) - view.transform_point3(d)).length() > 1.0,
            "a câmara de teste está na origem e os testes de direcção ficam cegos"
        );
    }

    #[test]
    fn a_cone_pointing_at_a_sphere_touches_it() {
        let cos_half = 30.0_f32.to_radians().cos();
        let sin_half = 30.0_f32.to_radians().sin();
        let apex = Vec3::ZERO;
        let dir = Vec3::new(0.0, 0.0, -1.0);
        assert!(cone_touches_sphere(
            apex,
            dir,
            10.0,
            cos_half,
            sin_half,
            Vec3::new(0.0, 0.0, -5.0),
            0.5
        ));
    }

    #[test]
    fn a_cone_rejects_what_is_behind_beside_and_beyond() {
        let cos_half = 15.0_f32.to_radians().cos();
        let sin_half = 15.0_f32.to_radians().sin();
        let apex = Vec3::ZERO;
        let dir = Vec3::new(0.0, 0.0, -1.0);
        let touch =
            |c: Vec3, r: f32| cone_touches_sphere(apex, dir, 10.0, cos_half, sin_half, c, r);
        assert!(!touch(Vec3::new(0.0, 0.0, 5.0), 0.5), "atrás do ápice");
        assert!(
            !touch(Vec3::new(0.0, 0.0, -30.0), 0.5),
            "para lá do alcance"
        );
        assert!(!touch(Vec3::new(20.0, 0.0, -5.0), 0.5), "muito ao lado");
        // E uma esfera grande o suficiente engole o ápice: tem de tocar.
        assert!(
            touch(Vec3::new(0.0, 0.0, 5.0), 8.0),
            "esfera que contém o ápice"
        );
    }

    /// A propriedade que interessa: **nenhum ponto iluminado fica num cluster que
    /// descartou a luz.** Um falso positivo custa um teste por pixel; um falso
    /// negativo é um buraco negro no sítio onde devia estar luz.
    ///
    /// Amostra-se o interior do cone e verifica-se que o cluster de cada ponto
    /// recebeu o índice. É o teste que uma fórmula de cone errada não sobrevive.
    #[test]
    fn every_lit_point_lands_in_a_cluster_that_kept_the_light() {
        let (view, near, far, tan_half_fov, aspect) = camera();
        let apex = Vec3::new(1.5, 0.8, -6.0);
        let dir = Vec3::new(0.2, -0.4, -1.0).normalize();
        let radius = 9.0;
        // Estreito de propósito. Com 28 graus o teste passava mesmo com a
        // direcção do cone transformada como ponto em vez de vector — um erro que
        // a desvia 19.6 graus, e a 28 sobra sobreposição que chega. A 11 não sobra.
        let outer = 11.0_f32.to_radians();
        let light = Light::spot(apex, dir, radius, outer * 0.6, outer, Vec3::ONE, 5.0);
        let (a, stats) = assign_counted(&[light], view, near, far, tan_half_fov, aspect);

        // Base ortonormal do cone, para varrer o seu interior.
        let up = if dir.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
        let t = up.cross(dir).normalize();
        let b = dir.cross(t);

        let mut checked = 0;
        for di in 1..=12 {
            let d = radius * di as f32 / 12.0;
            for ai in 0..16 {
                let ang = std::f32::consts::TAU * ai as f32 / 16.0;
                for ri in 0..=4 {
                    // `outer` é o meio-ângulo, portanto o raio a esta distância.
                    let rr = d * outer.tan() * ri as f32 / 4.0;
                    let p = apex + dir * d + (t * ang.cos() + b * ang.sin()) * rr;
                    let pv = view.transform_point3(p);
                    let depth = -pv.z;
                    if depth <= near || depth >= far {
                        continue;
                    }
                    let z = depth_slice(depth, near, far);
                    let half_h = depth * tan_half_fov;
                    let half_w = half_h * aspect;
                    let fx = (pv.x / half_w * 0.5 + 0.5) * CLUSTER_X as f32;
                    let fy = (-pv.y / half_h * 0.5 + 0.5) * CLUSTER_Y as f32;
                    if !(0.0..CLUSTER_X as f32).contains(&fx)
                        || !(0.0..CLUSTER_Y as f32).contains(&fy)
                    {
                        continue;
                    }
                    let c = ((z * CLUSTER_Y + fy as u32) * CLUSTER_X + fx as u32) as usize;
                    let range = a.ranges[c];
                    let found = a.indices
                        [range.offset as usize..(range.offset + range.count) as usize]
                        .contains(&0);
                    assert!(
                        found,
                        "o ponto {p:?} está dentro do cone e o cluster {c} não tem a luz"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 300, "só {checked} pontos testados");
        assert!(
            stats.cone_slots < stats.sphere_slots,
            "o cone não cortou nada"
        );
    }

    /// Mover a câmara e a luz **juntas** não pode mudar o resultado.
    ///
    /// É o invariante que ataca directamente a classe de erro «direcção
    /// transformada como ponto»: nessa forma a direcção do cone em espaço de
    /// vista passa a depender de **onde a câmara está**, e não só de para onde
    /// olha. Com a câmara na origem o erro é invisível; a 13 unidades da origem
    /// desvia o cone 19.6 graus.
    #[test]
    fn translating_camera_and_light_together_changes_nothing() {
        let (view, near, far, tan_half_fov, aspect) = camera();
        let pos = Vec3::new(0.5, 1.0, -9.0);
        let dir = Vec3::new(0.1, -0.3, -1.0).normalize();
        let a = assign_counted(
            &[Light::spot(pos, dir, 9.0, 0.12, 0.18, Vec3::ONE, 5.0)],
            view,
            near,
            far,
            tan_half_fov,
            aspect,
        );

        // A mesma cena, deslocada. Em espaço de vista nada mudou.
        let shift = Vec3::new(40.0, -15.0, 23.0);
        let moved_view = view * Mat4::from_translation(-shift);
        let b = assign_counted(
            &[Light::spot(
                pos + shift,
                dir,
                9.0,
                0.12,
                0.18,
                Vec3::ONE,
                5.0,
            )],
            moved_view,
            near,
            far,
            tan_half_fov,
            aspect,
        );
        assert_eq!(
            a.1.cone_slots, b.1.cone_slots,
            "deslocar câmara e luz juntas mudou {} slots para {}",
            a.1.cone_slots, b.1.cone_slots
        );
        assert_eq!(a.0.indices, b.0.indices, "a lista de índices mudou");
    }

    /// Uma omni não pode ser tocada pelo teste de cone: se fosse, a correcção que
    /// o `gate-lights` prova para omni deixava de valer.
    #[test]
    fn omni_lights_are_untouched_by_the_cone_test() {
        let (view, near, far, tan_half_fov, aspect) = camera();
        let omni = Light::point(Vec3::new(0.0, 0.0, -5.0), 6.0, Vec3::ONE, 4.0);
        let (a, stats) = assign_counted(&[omni], view, near, far, tan_half_fov, aspect);
        assert_eq!(
            stats.sphere_slots, stats.cone_slots,
            "o cone mexeu numa omni"
        );
        assert!(a.indices.len() > 1);
    }

    /// Um cone estreito tem de custar bem menos clusters do que uma omni do mesmo
    /// raio — senão o teste está escrito mas não está a fazer nada.
    #[test]
    fn a_narrow_cone_costs_far_less_than_a_sphere() {
        let (view, near, far, tan_half_fov, aspect) = camera();
        let pos = Vec3::new(0.0, 4.0, -8.0);
        let dir = Vec3::new(0.0, -1.0, 0.0);
        let spot = Light::spot(pos, dir, 10.0, 0.15, 0.20, Vec3::ONE, 5.0);
        let (_, stats) = assign_counted(&[spot], view, near, far, tan_half_fov, aspect);
        assert!(
            stats.saved() > 0.5,
            "um cone de 11 graus só cortou {:.1}% dos clusters da esfera",
            stats.saved() * 100.0
        );
    }
}
