//! A cena como entidades, e o culling que isso torna possível.
//!
//! Antes disto cada sample construía o mundo em código: a Sponza tinha uma
//! `Vec<GpuPrim>` e um `usize` a marcar onde começavam as primitivas de duas
//! faces. Funciona para 103 primitivas e mais nada — não havia onde pendurar uma
//! bounding box, portanto não havia como não desenhar o que não se vê.
//!
//! O ECS é `bevy_ecs` (D38, decidido a medir com o `gate-ecs`). Este crate não
//! sabe nada de Vulkan: guarda os handles opacos que o RHI devolve.

use bevy_ecs::prelude::*;
use harpia_math::{Mat4, Vec3, Vec4};
use harpia_rhi::{Buffer, Texture};

/// A malha que uma entidade desenha.
#[derive(Component, Clone, Copy, Debug)]
pub struct Mesh {
    pub vb: Buffer,
    pub ib: Buffer,
    pub index_count: u32,
}

/// Onde esta entidade vive dentro dos buffers partilhados.
///
/// Toda a cena está num par de buffers, e cada primitiva é um intervalo neles.
/// É isso que permite um draw indirecto múltiplo: um `vkCmdDrawIndexedIndirect`
/// com N comandos precisa de **um** VB e **um** IB ligados, e cada comando leva o
/// seu `firstIndex` e `vertexOffset`.
///
/// `prim` é o índice na tabela por primitiva que o shader lê — e vai no
/// `firstInstance` do comando, para o VS o ler como `gl_InstanceIndex` sem
/// precisar de `gl_DrawID` nem da extensão que ele obriga.
#[derive(Component, Clone, Copy, Debug)]
pub struct MeshRange {
    pub first_index: u32,
    pub vertex_offset: i32,
    pub prim: u32,
}

/// O material. `alpha_cutoff` 0 = opaco (glTF `OPAQUE`).
#[derive(Component, Clone, Copy, Debug)]
pub struct Material {
    pub albedo: Texture,
    pub alpha_cutoff: f32,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct WorldTransform(pub Mat4);

/// glTF `doubleSided`: a folhagem com cutout não pode levar back-face culling.
#[derive(Component, Clone, Copy, Debug)]
pub struct TwoSided;

/// Caixa alinhada aos eixos, **em espaço do mundo**.
///
/// Guardada como centro e meia-extensão porque é essa a forma que o teste do
/// frustum quer: um produto escalar com o valor absoluto do normal dá logo o
/// raio projectado da caixa.
#[derive(Component, Clone, Copy, Debug)]
pub struct Bounds {
    pub center: Vec3,
    pub extents: Vec3,
}

impl Bounds {
    /// Envolve pontos já transformados para o mundo.
    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Option<Self> {
        let mut it = points.into_iter();
        let first = it.next()?;
        let (mut lo, mut hi) = (first, first);
        for p in it {
            lo = lo.min(p);
            hi = hi.max(p);
        }
        Some(Self {
            center: (lo + hi) * 0.5,
            extents: (hi - lo) * 0.5,
        })
    }
}

/// Marca que sobrevive ao culling deste frame.
#[derive(Component, Clone, Copy, Debug)]
pub struct Visible;

/// Os seis planos do frustum, cada um `(a, b, c, d)` com `a·x + b·y + c·z + d`
/// positivo do lado de dentro.
///
/// Gribb-Hartmann a partir da view-projection, o que quer dizer que funciona
/// com a nossa `perspective_vk` sem casos especiais: o flip de Y e o depth em
/// `[0, 1]` já estão dentro da matriz.
#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    pub fn from_view_proj(vp: Mat4) -> Self {
        // glam guarda por colunas; as linhas é que interessam aqui.
        let r0 = Vec4::new(vp.x_axis.x, vp.y_axis.x, vp.z_axis.x, vp.w_axis.x);
        let r1 = Vec4::new(vp.x_axis.y, vp.y_axis.y, vp.z_axis.y, vp.w_axis.y);
        let r2 = Vec4::new(vp.x_axis.z, vp.y_axis.z, vp.z_axis.z, vp.w_axis.z);
        let r3 = Vec4::new(vp.x_axis.w, vp.y_axis.w, vp.z_axis.w, vp.w_axis.w);
        // Near é só `r2` porque o depth do Vulkan vai de 0 a 1, não de -1 a 1.
        let planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2];
        Self {
            planes: planes.map(normalize_plane),
        }
    }

    /// Os seis planos, para um shader os receber tal como estão.
    pub fn planes(&self) -> [Vec4; 6] {
        self.planes
    }

    /// Conservador: verdadeiro se a caixa **puder** estar visível.
    ///
    /// Um falso positivo custa um draw; um falso negativo faz desaparecer
    /// geometria, que é muito pior. Por isso o teste é «está inteiramente fora
    /// de algum plano?» e não «está dentro de todos».
    pub fn intersects(&self, b: &Bounds) -> bool {
        for plane in &self.planes {
            let n = Vec3::new(plane.x, plane.y, plane.z);
            // Raio da caixa projectado no normal do plano.
            let radius =
                b.extents.x * n.x.abs() + b.extents.y * n.y.abs() + b.extents.z * n.z.abs();
            if n.dot(b.center) + plane.w + radius < 0.0 {
                return false;
            }
        }
        true
    }
}

fn normalize_plane(p: Vec4) -> Vec4 {
    let len = Vec3::new(p.x, p.y, p.z).length();
    if len > 0.0 { p / len } else { p }
}

/// O frustum deste frame, para os sistemas de culling lerem.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ActiveFrustum(pub Frustum);

/// Quantas entidades sobreviveram ao último culling, e de quantas.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct CullStats {
    pub visible: u32,
    pub total: u32,
}

/// Marca `Visible` no que o frustum toca e tira a quem não toca.
///
/// Escrito com dois passes de `Commands` em vez de um `bool` por entidade porque
/// é assim que uma query fica barata: percorrer só o arquétipo `Visible` é o que
/// torna o desenho independente do tamanho do mundo.
pub fn cull_to_frustum(
    mut commands: Commands,
    frustum: Res<ActiveFrustum>,
    mut stats: ResMut<CullStats>,
    q: Query<(Entity, &Bounds, Option<&Visible>)>,
) {
    let mut visible = 0;
    let mut total = 0;
    for (entity, bounds, was_visible) in &q {
        total += 1;
        let now = frustum.0.intersects(bounds);
        if now {
            visible += 1;
        }
        match (now, was_visible.is_some()) {
            (true, false) => {
                commands.entity(entity).insert(Visible);
            }
            (false, true) => {
                commands.entity(entity).remove::<Visible>();
            }
            _ => {}
        }
    }
    stats.visible = visible;
    stats.total = total;
}

#[cfg(test)]
mod tests {
    use super::*;
    use harpia_math::perspective_vk;

    fn camera() -> Mat4 {
        // A olhar de (0,0,10) para a origem, que é −Z.
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, Vec3::Y);
        perspective_vk(60.0_f32.to_radians(), 16.0 / 9.0, 0.1, 100.0) * view
    }

    fn point(p: Vec3) -> Bounds {
        Bounds {
            center: p,
            extents: Vec3::ZERO,
        }
    }

    #[test]
    fn bounds_wrap_their_points() {
        let b =
            Bounds::from_points([Vec3::new(-1.0, 0.0, 2.0), Vec3::new(3.0, 4.0, -2.0)]).unwrap();
        assert_eq!(b.center, Vec3::new(1.0, 2.0, 0.0));
        assert_eq!(b.extents, Vec3::new(2.0, 2.0, 2.0));
        assert!(Bounds::from_points([]).is_none());
    }

    #[test]
    fn what_the_camera_looks_at_is_inside() {
        let f = Frustum::from_view_proj(camera());
        assert!(f.intersects(&point(Vec3::ZERO)));
        assert!(f.intersects(&point(Vec3::new(0.0, 0.0, 5.0))));
    }

    /// O erro que faz geometria desaparecer: um plano com o sinal trocado.
    #[test]
    fn behind_and_beyond_are_outside() {
        let f = Frustum::from_view_proj(camera());
        assert!(
            !f.intersects(&point(Vec3::new(0.0, 0.0, 20.0))),
            "atrás da câmara"
        );
        assert!(
            !f.intersects(&point(Vec3::new(0.0, 0.0, -200.0))),
            "para lá do far"
        );
        assert!(
            !f.intersects(&point(Vec3::new(500.0, 0.0, 0.0))),
            "muito à direita"
        );
        assert!(
            !f.intersects(&point(Vec3::new(0.0, 500.0, 0.0))),
            "muito acima"
        );
    }

    /// Uma caixa grande a meio de um plano tem de continuar visível: cortá-la
    /// seria pior do que desenhar a mais.
    #[test]
    fn a_box_straddling_a_plane_survives() {
        let f = Frustum::from_view_proj(camera());
        let straddling = Bounds {
            center: Vec3::new(0.0, 0.0, 20.0),
            extents: Vec3::splat(30.0),
        };
        assert!(f.intersects(&straddling));
    }

    #[test]
    fn culling_marks_and_unmarks() {
        let mut world = World::new();
        world.insert_resource(ActiveFrustum(Frustum::from_view_proj(camera())));
        world.insert_resource(CullStats::default());
        let inside = world.spawn(point(Vec3::ZERO)).id();
        let outside = world.spawn(point(Vec3::new(0.0, 0.0, 400.0))).id();

        let mut schedule = Schedule::default();
        schedule.add_systems(cull_to_frustum);
        schedule.run(&mut world);

        assert!(world.get::<Visible>(inside).is_some());
        assert!(world.get::<Visible>(outside).is_none());
        assert_eq!(world.resource::<CullStats>().visible, 1);
        assert_eq!(world.resource::<CullStats>().total, 2);

        // Vira a câmara ao contrário: quem estava dentro sai, quem estava fora entra.
        let back = Mat4::look_at_rh(
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(0.0, 0.0, 400.0),
            Vec3::Y,
        );
        let vp = perspective_vk(60.0_f32.to_radians(), 16.0 / 9.0, 0.1, 1000.0) * back;
        world.insert_resource(ActiveFrustum(Frustum::from_view_proj(vp)));
        schedule.run(&mut world);
        assert!(
            world.get::<Visible>(inside).is_none(),
            "Visible tem de ser retirado"
        );
        assert!(world.get::<Visible>(outside).is_some());
    }
}
