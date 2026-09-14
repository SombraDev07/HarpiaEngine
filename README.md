# Harpia

Renderer **deferred** em Rust sobre **Vulkan 1.3**. Escrito de raiz: o que não
sobrevive a um gate com validation ligada não entra no pixel.

Não é um port. A referência C++ (`TucanoEngine`) define a barra a superar —
PBR, IBL, fog, clima e bindless honestos — não a checklist a copiar.

[![Rust](https://img.shields.io/badge/Rust-1.85+-orange.svg)](https://www.rust-lang.org/)
[![Vulkan](https://img.shields.io/badge/API-Vulkan%201.3-red.svg)](https://www.vulkan.org/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#licença)

## Estado

Fases **0–6 fechadas**. Fase **7** (GI + post extra) é a próxima.
Linux é a plataforma de desenvolvimento; o host de referência é AMD / RADV.

| Fase | Gate | Estado |
|---|---|---|
| 0 contrato | Null + spec bindless | feito |
| 1 triângulo | `hello-triangle` | feito |
| 2 bindless | `gate-bindless` | feito |
| 3 PBR | `gate-pbr-grid` | feito |
| 4 CSM + TAA | `gate-csm`, `gate-taa`, Sponza | feito |
| 5 clima | fog, céu, nuvens, água, chuva | feito |
| 6 mundo | terreno, vegetação, instâncias | em curso |
| 7–9 | GI, editor, opcionais | não |

O quadro oficial, com checks, está em [`docs/Rust-Rewrite-Roadmap.md`](docs/Rust-Rewrite-Roadmap.md) §15.

## O que está no pixel

| Área | Contrato |
|---|---|
| RHI | Vulkan 1.3 (`ash` + `gpu-allocator`). `vk::*` só em `harpia-rhi`. Backend Null para CI. |
| Bindless | Heap de 8192 descritores; o slot 0 é null para sempre. |
| PBR | Deferred, 5 MRT + D32. GGX, IBL split-sum, energy compensation, ACES. |
| Sombras | CSM de 4 cascatas, frustum da câmara, PCF. |
| Antialiasing | TAA com history e clamp 3×3. |
| Clima | Fog em froxels, céu Hillaire, nuvens Nubis, água Gerstner, chuva. |
| Mundo | Clipmap (Losasso/Hoppe), culling em compute, céu e nuvens no terreno. |

**Sponza** (glTF) é a cena de referência de luz e sombra.
**`storm`** é a demo de composição de clima. Um interior não substitui o outro.

## Requisitos

- Rust **1.85+** (edition 2024)
- Driver Vulkan 1.3
- `spirv-as` e `spirv-val` no `PATH` (o build monta e valida os shaders)
- Linux (X11 / Wayland via `winit`)

## Começar

```bash
git clone https://github.com/SombraDev07/HarpiaEngine.git
cd HarpiaEngine

python3 prog/tools/fetch_sponza.py          # Sponza glTF (CC-BY, Khronos / Crytek)

cargo run -p sponza --release -- --frames 90
cargo run -p storm --release -- --frames 16
cargo run -p storm --release -- --interactive   # overlay egui; F1 esconde
cargo run -p gate-terrain --release -- --frames 16
```

Sem GPU (backend Null):

```bash
cargo test --workspace
cargo run -p hello-triangle -- --backend null --frames 8
```

Por omissão os samples correm `--frames N` e fecham. Validation Vulkan está
ligada; o processo sai com código ≠ 0 se houver erros. `--interactive` mantém a
janela aberta até a fechares — em AMD/RADV o default limitado não é negociável
(`GPUVM` não aparece na validation).

```bash
cargo run -p sponza --release -- --interactive
```

## Arquitectura

Disciplina tipo Dagor: um `prog/engine` com módulos irmãos, samples **fora** da
engine, `vk::*` num único crate.

```
prog/
  engine/
    core/ memory/ math/
    drv/          # harpia-rhi — o único sítio com vk::*
    render/       # frame graph, PBR, clima, clipmap
    scene/ app/ plugin/
  samples/        # hello-triangle, sponza, storm, gates/*
  tools/          # shader-build, fetch_sponza
  plugins/        # cdylibs (FidelityFX, quando a fase o pedir)
  1stPartyLibs/ 3rdPartyLibs/
docs/             # doutrina, roadmap, barra de qualidade
memory/           # estado de desenvolvimento entre sessões
assets/sponza/    # cena de referência (fetch, não o repo)
```

Crates `harpia-*`. Um crate por pasta. Shaders novos em **GLSL**; o SPIR-V
legado vive como `.spvasm` e é montado no build.

## Documentação

| Documento | Papel |
|---|---|
| [`docs/Rust-Rewrite-Quality-Bar.md`](docs/Rust-Rewrite-Quality-Bar.md) | Barra de qualidade. Se discordar de qualquer outro texto, **ganha**. |
| [`docs/Rust-Rewrite-Roadmap.md`](docs/Rust-Rewrite-Roadmap.md) | Fases, packing, o que entra quando. |
| [`docs/Bindless-Descriptor-Layout.md`](docs/Bindless-Descriptor-Layout.md) | Contrato do heap bindless. |
| [`docs/AAA-Gap-Analysis.md`](docs/AAA-Gap-Analysis.md) | O que falta, com números medidos. |
| [`AGENTS.md`](AGENTS.md) | Regras duras para quem implementa. |

## Licença

O código da Harpia é **MIT OR Apache-2.0**, como declarado em `Cargo.toml`.
A Sponza em `assets/sponza/` é **CC-BY** (Crytek, via Khronos glTF-Sample-Assets).
