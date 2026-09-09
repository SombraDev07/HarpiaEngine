# Decisions

Fechadas. Não reabrir sem motivo escrito aqui.

## D-1 — Produto

- Esta engine chama-se **Harpia**. Crates `harpia-*`, `ENGINE_NAME = "harpia"`.
- **Tucano** (`TucanoEngine/`) é referência C++ para *superar* (PBR/IBL/fog/clima/bindless honestos). Não se copia linha a linha, não se reusa o nome nos crates, não se aninha um `tucano-rs`.

## D7 — Pastas (disciplina Dagor, nomes Harpia)

- Código em `prog/`. Doutrina em `docs/`. Memória da IA em `memory/`.
- `prog/engine/` — um crate por pasta, irmãos: `core`, `memory`, `math`, `drv` (RHI), `render`, `scene`, `app`, `plugin`, `editor`. Sem `core/harpia-core`.
- `prog/1stPartyLibs/` — libs nossas que não são a engine. `prog/3rdPartyLibs/` — vendor (FFX). `prog/plugins/` — cdylib.
- `prog/samples/` e `prog/tools/` **não** vivem dentro de `engine/`.
- Inspirado na divisão `prog/engine` + 1st/3rd da Dagor. **Não** copiar `daECS` / `daNet` / a checklist de pastas da Gaijin. Anim/ECS/net só quando a fase os pedir.

## D0 — Stack gráfico

- Vulkan 1.3 via `ash` + `gpu-allocator`. **Proibido** wgpu / DX12 / GL / Metal.
- Um backend GPU (Vulkan) + Null para testes CPU.
- Linux: X11 + Wayland. Windows: Win32. Surface via `winit` + `ash-window`. `cfg` só em `harpia-rhi` (`window.rs`).

## D1 — Edição Rust

- Pedido: 2024 se possível, senão 2021.
- Workspace em **2024** (toolchain local: rustc 1.98).

## D2 — Bindless

- Spec: `docs/Bindless-Descriptor-Layout.md`.
- Heap sampled 8192, slot 0 = null (dummy 1×1) criado no init do device Vulkan.
- Índice inteiro no CBV (set 0), escrito **depois** de `begin_frame`.
- Hello-triangle continua com pipeline layout vazio. `gate-bindless` usa o layout completo (sets 0–4).

## D3 — Hello-triangle

- Vértices no VS via `SV_VertexID` / `VertexIndex`. Sem vertex buffer.
- Dynamic rendering (VK 1.3). Sem `VkRenderPass`.
- Default `--frames 90` (nunca unbounded por omissão). Resize programático nos frames 30 e 60.
- `--interactive` / `-i`: janela até o utilizador fechar. Sem resize automático. Aviso AMD. Não é o default.
- FIFO present. `old_swapchain` no resize. Surface vive até ao drop do device.
- `present_format` escolhido da lista da surface; preferir `B8G8R8A8_UNORM` (X11/Mesa).

## D4 — Allocator

- `harpia-memory` wraps `mimalloc` + atomics de stats. Não é um malloc novo.
- `FrameBump` wraps `bumpalo`; API existe, hello-triangle não a usa.
- Global allocator instalado nos bins, não nas libs.

## D5 — Plugins

- `harpia-plugin` tem trait + loader vazio (`libloading` no Cargo).
- Hello-triangle **não** carrega plugins.

## D6 — Shaders

- Fonte de verdade: HLSL (`prog/samples/hello-triangle/shaders/triangle.hlsl`).
- Cook: DXC `-spirv -fspv-target-env=vulkan1.3`. Fallback de assembleia: `spirv-as` sobre `.spvasm` equivalente, só até haver DXC no host.
- `harpia-shaders` como crate de cook: fase posterior. SPD/FSR: FidelityFX em `prog/3rdPartyLibs/`, não reescritos.
