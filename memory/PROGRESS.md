# Progress

**Fase actual:** 1 **verde** (Linux / RADV). **Parar.** Fase 2 (bindless + upload) é outra sessão.

## Verde

- `cargo test --workspace` — Null + unit OK.
- `cargo run -p hello-triangle -- --frames 90` — AMD Radeon RX 6700 (RADV), `B8G8R8A8_UNORM`, resize 30→800×600 e 60→1280×720, **0 erros de validation**, exit 0.
- `--backend null --frames 8` — sem GPU, exit 0.

## Feito

- Árvore achatada: `prog/engine/core` (crate `harpia-core`), `drv`, `app`, … Samples em `prog/samples/`. Sem pasta extra `harpia-*` dentro do módulo.
- Hello-triangle preto: constantes SPIR-V `1` eram bits inteiros, não `1.0`. Corrigido (`.spvasm` + assembler). Clear um pouco mais claro para o miss ser óbvio.
- Workspace Rust **2024**, crates vivos: `harpia-core`, `harpia-memory`, `harpia-math`, `harpia-rhi`, `harpia-plugin`, `harpia-app`.
- `docs/Bindless-Descriptor-Layout.md` escrito **antes** do shader do triângulo.
- Hello-triangle: `SV_VertexID`, sem bindless, sem plugins, `old_swapchain`, surface estável.

## Não feito (propositado)

- Fase 2+: PBR, sombras, clima, FSR, editor, heap bindless, upload.
- Compile/run Windows não exercitado neste host.
- `vulkan-validationlayers` não está no apt do sistema; o gate usou `VK_LAYER_PATH` no `.deps` da referência Tucano (ver LANDMINES). Instalar o pacote distro no host.

## Próximo (fase 2)

Bindless heap 8192, slot 0 = dummy 1×1 (barreira VS+PS+CS), `bindless_index`, upload mips completos, triângulo texturizado, compute UAV simples.
