# Prompt de arranque — engine Rust do zero

> **Harpia (este repo):** o produto e os crates são `harpia-*`. Árvore de código: `prog/` (ver `AGENTS.md` e `memory/DECISIONS.md` D7). Os nomes `tucano-*` / `crates/` abaixo são da referência C++ — não portes `TucanoEngine/`.

Cola o bloco **Prompt** numa conversa nova, num **repositório vazio**.  
Copia também `docs/rust-rewrite-skeleton/` (AGENTS.md + `memory/`) para a raiz do repo novo.

**Produto:** engine **AAA em disciplina** (pastas, libs maduras, gates, plugins, memória persistente) — **não** Unreal no dia 1. A barra de qualidade continua: clona o 6, recusa o 4.

**Decisões fixas:**

- Vulkan puro (`ash` + VMA). **Proibido wgpu / DX12 / GL.**
- Linux + Windows. Surface X11/Wayland e Win32.
- Pastas **rígidas** (tabela abaixo). Não misturar FFX no render, nem `vk::*` fora do RHI.
- **Não inventar** se existir crate/SDK maduro.
- `thirdparty/` + **plugins**. FidelityFX nunca dentro de `tucano-render`.
- `memory/` para a IA gravar estado entre chats. Allocator da engine = crate `tucano-memory` (wrapper, não malloc caseiro).
- Começar do zero. Não portes o C++.

---

## Pastas (rígidas)

```
tucano-rs/
  crates/
    foundation/
      tucano-core/        # Result, ids, tracing
      tucano-memory/      # mimalloc/jemalloc + frame bump + stats. NÃO escrever malloc.
      tucano-math/        # re-export glam. Zero vec3 caseiro.
    rhi/
      tucano-rhi/         # trait + ash. Único sítio com vk::*
    render/
      tucano-render/      # frame graph, GBuffer, lighting, shadows, post
      tucano-shaders/     # HLSL → SPIR-V (DXC)
      tucano-weather/
      tucano-terrain/     # um clipmap
      tucano-gi/
    scene/
      tucano-scene/       # Camera, Mesh, Material, Light — sem GPU
      tucano-world/
      tucano-vegetation/
    runtime/
      tucano-app/         # winit, loop, --frames
      tucano-plugin/      # ABI + libloading
      tucano-assets/      # gltf crate, image crate
    editor/               # fase 8+; pasta existe vazia no dia 0
      tucano-editor/
  thirdparty/             # FidelityFX, C/C++ vendor, LICENSE por pasta
  plugins/                # cdylib .so/.dll (ffx-spd, depois fsr)
  samples/                # hello-triangle, pbr-grid, gates/*  — NÃO crates/
  tools/                  # cook shaders/assets
  assets/                 # defaults da engine (IBL, meshes de teste)
  docs/                   # os 3 markdowns de doutrina
  memory/                 # persistência da IA (INDEX, PROGRESS, DECISIONS, LANDMINES)
  AGENTS.md
```

Nada de `src/` único com 40 módulos. Um crate = uma responsabilidade. Samples não vivem ao lado de `tucano-rhi`.

---

## Não inventar (tabela)

| Precisas de | Usas | Proibido inventar |
|---|---|---|
| Vectores / matrizes | `glam` (`tucano-math` só re-exporta) | `struct Vec3` |
| Janela / input | `winit` + `ash-window` | Win32/X11 cru no render |
| Vulkan | `ash` + `gpu-allocator` ou `vk-mem` | Loader/VMA à mão |
| CPU alloc | `mimalloc` ou `tikv-jemallocator` atrás de `tucano-memory` | malloc próprio |
| Frame bump | `bumpalo` (ou similar) via `tucano-memory` | arena caseira sem motivo |
| Imagens | `image` | stb copiado para crates/ |
| glTF | `gltf` | parser JSON à mão |
| Serialize | `serde` + `toml`/`json` | formato próprio no dia 0 |
| Log | `tracing` | `println!` como arquitectura |
| Erros | `thiserror` nos libs, `anyhow` nos bins | `unwrap` em GPU |
| Plugins | `libloading` | `dlopen` cru |
| Meshopt | crate `meshopt` ou `thirdparty/meshoptimizer` | weld próprio |
| Física | `rapier3d` ou `jolt-rs` (fase tardia) | solver de contactos |
| UI editor | `egui` ou `imgui-rs` (fase 8) | toolkit de widgets |
| Upscale / SPD | **FidelityFX** em `thirdparty/` | reescrever SPD/FSR |
| Hash / jobs | `rustc-hash`, `rayon` se precisares | thread pool de hobby |

**Inventas só o DNA da engine:** trait RHI, frame graph, packing GBuffer, PBR/clima/clipmap, bindless layout, plugin ABI. Se vais escrever 200 linhas que crates.io já tem em produção, paras e usas o crate.

AAA aqui = esta disciplina, não 40 passes no hello-triangle.

---

## Memória da IA

Copia `docs/rust-rewrite-skeleton/memory/` → `memory/` no repo novo.

- **Início de sessão:** INDEX → PROGRESS → DECISIONS (LANDMINES se for GPU).
- **Fim de sessão:** actualizar PROGRESS + decisões/minas novas. Sem isto a sessão não fechou.
- Código não vai para `memory/`. Só estado.

`crates/foundation/tucano-memory` é **outra coisa**: tracking de alocações da engine (CPU), não notas da IA.

---

## O que ler (ordem)

| # | O quê | Quando |
|---|---|---|
| 1 | `docs/Rust-Rewrite-Quality-Bar.md` | **Primeiro.** Doutrina. Ganha se discordar. |
| 2 | `docs/Rust-Rewrite-Roadmap.md` | Fases, packing, minas. |
| 3 | Este ficheiro + `AGENTS.md` do skeleton | Pastas, libs, Vulkan, plugins. |
| 4 | `memory/INDEX.md` | Toda a sessão depois da 0. |
| 5 | Spec Vulkan 1.3 + dynamic rendering + descriptor indexing | Ao fazer `tucano-rhi`. |
| 6 | FidelityFX em `thirdparty/` | Bloom (SPD), FSR depois de TAA. |
| 7 | Spec pack Tucano (opcional) | Só DNA visual. |

**Não leias** ECS/Lua/Jolt/VSM/SSGI-8taps da Tucano.

---

## Prompt (copiar daqui para baixo)

```
És o implementador de uma engine 3D AAA em Rust, do zero, num repositório vazio.
AAA = disciplina de produção (pastas, libs maduras, gates, validation, plugins, memória),
NÃO implementar Unreal no dia 1. A barra de qualidade manda: clona o 6, recusa o 4.

## Leitura obrigatória (antes de código)
1. docs/Rust-Rewrite-Quality-Bar.md     — doutrina. Se discordar, A BARRA GANHA.
2. docs/Rust-Rewrite-Roadmap.md
3. docs/Rust-Rewrite-Starter-Prompt.md  — pastas, libs, Vulkan, plugins, memory.
4. AGENTS.md
5. memory/INDEX.md → memory/PROGRESS.md → memory/DECISIONS.md

Copia docs/rust-rewrite-skeleton/ (AGENTS.md + memory/) para a raiz se ainda não existir.

Não clones a checklist Tucano C++. Clona o 6: PBR + IBL + fog + clima + bindless.
Recusa o 4: VSM falso, occupancy órfão, segundo clipmap, CSM 60°/16:9, SSGI de 8 taps.
Obrigatório cedo: CSM com frustum da câmara REAL + TAA; UM clipmap; occupancy no lighting
no mesmo PR ou o volume não nasce.

Não comeces por mesh shaders, RT, FSR, editor. Fase N só depois do exit da N−1.
Gates: samples/gates/*, --frames N (16), nunca unbounded em AMD.
MESA_VK_ABORT_ON_DEVICE_LOSS=1. Validation ON.

## Stack
- Rust 2024 se possível, senão 2021.
- Vulkan 1.3, crate ash + VMA (gpu-allocator ou vk-mem). PROIBIDO wgpu/DX12/GL/Metal.
- vk::* SÓ em crates/rhi/tucano-rhi. Render nunca chama vkCmd*.
- Linux (X11+Wayland) e Windows (Win32). cfg só em rhi/window.
- winit + ash-window. HLSL→SPIR-V (DXC). Bindless heap 8192, slot 0 = null.
- Swapchain: não destruir surface; oldSwapchain; presentFormat real (BGRA no X11).

## Pastas — cria-as todas no dia 0 (mesmo vazias com .gitkeep)
crates/foundation/{tucano-core,tucano-memory,tucano-math}
crates/rhi/tucano-rhi
crates/render/{tucano-render,tucano-shaders,tucano-weather,tucano-terrain,tucano-gi}
crates/scene/{tucano-scene,tucano-world,tucano-vegetation}
crates/runtime/{tucano-app,tucano-plugin,tucano-assets}
crates/editor/tucano-editor          # vazio até fase 8
thirdparty/  plugins/  samples/  tools/  assets/  docs/  memory/

Não ponhas samples dentro de crates/. Não ponhas FFX em crates/. Não ponhas notas em docs/.

## Não inventes bibliotecas
Usa glam, winit, ash, gpu-allocator/vk-mem, mimalloc ou jemallocator (via tucano-memory),
bumpalo se precisares de frame alloc, image, gltf, serde, tracing, thiserror, libloading,
meshopt, rapier/jolt-rs quando for física, egui/imgui-rs no editor, FidelityFX para SPD/FSR.
Se crates.io ou AMD já têm uma solução madura, USAS. Inventas só: RHI trait, frame graph,
PBR/clima/clipmap, bindless, plugin ABI.
tucano-math = re-export glam. tucano-memory = wrap do allocator + stats, NÃO um malloc novo.

## Plugins + thirdparty
FidelityFX vive em thirdparty/<nome>/ + LICENSE. Wrapper em plugins/ (cdylib).
Plugin não chama vkCmd*; declara pass ou usa o trait RHI.
Hello-triangle corre sem nenhum plugin.
SPD só na fase bloom. FSR só depois de TAA verde.

## Memória (obrigatório)
Início: lê memory/. Fim: actualiza PROGRESS.md, DECISIONS.md, LANDMINES.md se couber.
Sem update de memory/ a sessão não fechou. Não pares código na pasta memory/.

## Primeira entrega (fase 0 → 1)
1. Workspace + TODAS as pastas da árvore (gitkeep onde vazio).
2. Crates vivos: tucano-core, tucano-memory (mimalloc wrap), tucano-math (glam),
   tucano-rhi (trait + Null + Vulkan mínimo), tucano-plugin (trait + loader vazio),
   tucano-app.
3. thirdparty/README.md (ainda sem FFX). memory/ copiado do skeleton, PROGRESS actualizado.
4. Doc do descriptor layout bindless ANTES do primeiro shader (docs/ ou memory/DECISIONS).
5. samples/hello-triangle: Vulkan, --frames 90, validation 0, resize, Linux E Windows.
6. NÃO: PBR, sombras, clima, FSR, editor, segundo backend.

Quando fase 1 verde: actualiza memory/, PARA, espera a fase 2 (bindless + upload).

## Estilo
Rust idiomático. Sem unwrap em GPU. Sem waitIdle no frame quente.
Sem port C++ linha a linha.
```

---

## Como usar na prática

1. Repo **vazio**.
2. Copia para `docs/`: Quality-Bar, Roadmap, este Starter-Prompt.
3. Copia `docs/rust-rewrite-skeleton/AGENTS.md` → raiz.  
   Copia `docs/rust-rewrite-skeleton/memory/` → `memory/`.
4. Chat novo **nesse** repo. Cola o prompt. Não abras TucanoEngine.

---

## O que a IA *não* precisa no dia 0

Código Tucano, Jolt, Lua, ECS, editor, DX12, wgpu, FFX extraído, mesh shaders, DXR.
