# Progress

**Fase actual:** 2 **verde** (Linux / RADV). **Parar.** Fase 3 (deferred PBR) é outra sessão.

## Verde

- `cargo test --workspace` — Null + unit OK.
- `cargo run -p hello-triangle -- --frames 90` — 0 validation, resize 30/60.
- `cargo run -p gate-bindless` — 16 frames, resize 6/12, **0 erros de validation**, exit 0.
- `--backend null` nos dois samples.

## Feito

- Árvore achatada: `prog/engine/core` (crate `harpia-core`), `drv`, `app`, … Samples em `prog/samples/`.
- Hello-triangle preto: constantes SPIR-V `1` eram bits inteiros, não `1.0`. Corrigido.
- Fase 2: heap sampled 8192, slot 0 dummy 1×1 (barreira VS+PS+CS), upload por mip (pitch 256), `bindless_index`, compute UAV 64×64 amostrado no pixel.

## Não feito (propositado)

- Fase 3+: deferred PBR, sombras, clima, FSR, editor.
- Compile/run Windows não exercitado neste host.

## Próximo (fase 3)

GBuffer 5 MRTs + packing, lighting fullscreen (sol + IBL split-sum), `pbr-grid` 90 frames. Sem clima, sem sombras.
