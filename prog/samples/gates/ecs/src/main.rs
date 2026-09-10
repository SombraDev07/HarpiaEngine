//! Fase 6, gate do ECS: 1e6 instâncias em `bevy_ecs` contra `hecs`.
//!
//! D20 diz que a escolha do ECS se faz com números, não com opinião — e desde que
//! o `ecs_bench_suite` do rust-gamedev foi arquivado em Nov 2022 não há benchmark
//! cross-ECS mantido em Rust, portanto medir aqui é a única saída honesta.
//!
//! O que se mede é o que a engine vai mesmo fazer:
//!
//! * **spawn** — construir o mundo. Acontece uma vez, mas um mundo de streaming
//!   fá-lo constantemente em pedaços.
//! * **iterate** — percorrer todas as entidades e escrever numa componente. É o
//!   caso quente: a simulação por frame.
//! * **fragmented** — a mesma volta, mas só sobre as entidades que têm uma
//!   componente extra. É o que distingue um ECS por arquétipos de um por sparse
//!   sets, e é o padrão real de «desenha só o que é visível».
//! * **spawn/despawn** — o custo de churn, que é o que um mundo vivo faz.
//!
//! Nada aqui toca na GPU, portanto o gate corre em qualquer máquina.

use std::time::Instant;

use harpia_math::Vec3;

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const ENTITIES: usize = 1_000_000;
/// Fracção com a componente extra, para o teste fragmentado.
const TAGGED: usize = 8;
const ITERATIONS: usize = 30;
const DT: f32 = 1.0 / 60.0;

#[derive(Clone, Copy)]
struct Position(Vec3);
#[derive(Clone, Copy)]
struct Velocity(Vec3);
/// A componente rara. Uma em cada [`TAGGED`] entidades tem-na.
#[derive(Clone, Copy)]
struct Visible;

/// Deterministic and cheap: the point is to compare two ECSs, not to benchmark a
/// random number generator.
fn seed(i: usize) -> (Vec3, Vec3) {
    let f = i as f32;
    (
        Vec3::new(f * 0.001, f * 0.002, f * 0.003),
        Vec3::new((f * 0.01).sin(), 0.5, (f * 0.01).cos()),
    )
}

struct Timings {
    spawn_ms: f32,
    iterate_ms: f32,
    fragmented_ms: f32,
    churn_ms: f32,
    checksum: f32,
}

fn ms(start: Instant) -> f32 {
    start.elapsed().as_secs_f32() * 1000.0
}

mod bevy_run {
    use super::*;
    use bevy_ecs::prelude::*;

    #[derive(Component, Clone, Copy)]
    struct Pos(Vec3);
    #[derive(Component, Clone, Copy)]
    struct Vel(Vec3);
    #[derive(Component, Clone, Copy)]
    struct Vis;

    pub fn run() -> Timings {
        let t = Instant::now();
        let mut world = World::new();
        world.spawn_batch((0..ENTITIES).map(|i| {
            let (p, v) = seed(i);
            (Pos(p), Vel(v))
        }));
        // The extra component on a fraction of them, which is what forces an
        // archetype split and makes the fragmented query mean something.
        let tagged: Vec<Entity> = world
            .query_filtered::<Entity, With<Pos>>()
            .iter(&world)
            .step_by(TAGGED)
            .collect();
        for e in tagged {
            world.entity_mut(e).insert(Vis);
        }
        let spawn_ms = ms(t);

        let mut all = world.query::<(&mut Pos, &Vel)>();
        let t = Instant::now();
        for _ in 0..ITERATIONS {
            for (mut p, v) in all.iter_mut(&mut world) {
                p.0 += v.0 * DT;
            }
        }
        let iterate_ms = ms(t) / ITERATIONS as f32;

        let mut some = world.query_filtered::<(&mut Pos, &Vel), With<Vis>>();
        let t = Instant::now();
        for _ in 0..ITERATIONS {
            for (mut p, v) in some.iter_mut(&mut world) {
                p.0 += v.0 * DT;
            }
        }
        let fragmented_ms = ms(t) / ITERATIONS as f32;

        let t = Instant::now();
        let doomed: Vec<Entity> = world
            .query_filtered::<Entity, With<Vis>>()
            .iter(&world)
            .collect();
        for e in &doomed {
            world.despawn(*e);
        }
        world.spawn_batch((0..doomed.len()).map(|i| {
            let (p, v) = seed(i);
            (Pos(p), Vel(v))
        }));
        let churn_ms = ms(t);

        let mut sum = 0.0f32;
        for (p, _) in world.query::<(&Pos, &Vel)>().iter(&world) {
            sum += p.0.x;
        }
        Timings {
            spawn_ms,
            iterate_ms,
            fragmented_ms,
            churn_ms,
            checksum: sum,
        }
    }
}

mod hecs_run {
    use super::*;

    pub fn run() -> Timings {
        let t = Instant::now();
        let mut world = hecs::World::new();
        world.spawn_batch((0..ENTITIES).map(|i| {
            let (p, v) = seed(i);
            (Position(p), Velocity(v))
        }));
        let tagged: Vec<hecs::Entity> = world
            .iter()
            .map(|e| e.entity())
            .step_by(TAGGED)
            .collect();
        for e in tagged {
            let _ = world.insert_one(e, Visible);
        }
        let spawn_ms = ms(t);

        let t = Instant::now();
        for _ in 0..ITERATIONS {
            for (p, v) in world.query_mut::<(&mut Position, &Velocity)>() {
                p.0 += v.0 * DT;
            }
        }
        let iterate_ms = ms(t) / ITERATIONS as f32;

        let t = Instant::now();
        for _ in 0..ITERATIONS {
            for (p, v) in world
                .query_mut::<hecs::With<(&mut Position, &Velocity), &Visible>>()
            {
                p.0 += v.0 * DT;
            }
        }
        let fragmented_ms = ms(t) / ITERATIONS as f32;

        let t = Instant::now();
        let doomed: Vec<hecs::Entity> = world
            .iter()
            .filter(|e| e.has::<Visible>())
            .map(|e| e.entity())
            .collect();
        for e in &doomed {
            let _ = world.despawn(*e);
        }
        world.spawn_batch((0..doomed.len()).map(|i| {
            let (p, v) = seed(i);
            (Position(p), Velocity(v))
        }));
        let churn_ms = ms(t);

        let mut sum = 0.0f32;
        for (p, _) in world.query_mut::<(&Position, &Velocity)>() {
            sum += p.0.x;
        }
        Timings {
            spawn_ms,
            iterate_ms,
            fragmented_ms,
            churn_ms,
            checksum: sum,
        }
    }
}

fn main() -> anyhow::Result<std::process::ExitCode> {
    println!("gate-ecs: {ENTITIES} entidades, {ITERATIONS} iteracoes, 1 em {TAGGED} marcada\n");

    let bevy = bevy_run::run();
    let hecs = hecs_run::run();

    println!(
        "{:<12} {:>12} {:>12} {:>12}",
        "", "bevy_ecs", "hecs", "racio"
    );
    let row = |name: &str, a: f32, b: f32| {
        println!(
            "{name:<12} {a:>10.2}ms {b:>10.2}ms {:>11.2}x",
            if b > 0.0 { a / b } else { 0.0 }
        );
    };
    row("spawn", bevy.spawn_ms, hecs.spawn_ms);
    row("iterate", bevy.iterate_ms, hecs.iterate_ms);
    row("fragmented", bevy.fragmented_ms, hecs.fragmented_ms);
    row("churn", bevy.churn_ms, hecs.churn_ms);

    // Both must have done the same arithmetic, or the comparison is of two
    // different programs and the numbers mean nothing.
    let drift = (bevy.checksum - hecs.checksum).abs() / bevy.checksum.abs().max(1.0);
    println!("\nchecksum bevy={:.3e} hecs={:.3e}", bevy.checksum, hecs.checksum);
    anyhow::ensure!(
        drift < 1e-3,
        "os dois mundos divergiram ({drift:.3e}): o benchmark esta a comparar coisas diferentes"
    );

    println!("\nescolha: iterate e o caso quente; fragmented diz como se portam com");
    println!("queries esparsas; churn e o custo de um mundo vivo.");
    Ok(std::process::ExitCode::SUCCESS)
}
