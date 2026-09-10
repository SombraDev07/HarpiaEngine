# AGENTS.md — Harpia

**Produto: Harpia.** `TucanoEngine/` é a engine C++ de *referência* — a barra a superar, não um port e não o nome desta codebase. Crates: `harpia-*`. Não clones a checklist Tucano; clona o **6**, recusa o **4**.

Engine 3D em Rust. **AAA = disciplina de produção**, não Unreal no dia 1.

Leitura obrigatória (ordem):

1. `docs/Rust-Rewrite-Quality-Bar.md` — doutrina. Se discordar de qualquer outro texto, **a barra ganha**.
2. `docs/Rust-Rewrite-Roadmap.md` — fases, packing, minas.
3. `docs/Rust-Rewrite-Starter-Prompt.md` — pastas, libs, Vulkan, plugins.
4. Este ficheiro.
5. `memory/INDEX.md` → `memory/PROGRESS.md` → `memory/DECISIONS.md` (e `LANDMINES.md` se for GPU).

Não clones a checklist do Tucano C++. Clona o **6**: PBR + IBL + fog + clima + bindless. Recusa o **4**: VSM falso, occupancy órfão, segundo clipmap, CSM 60°/16:9, SSGI de 8 taps.

## Regras duras

- Fase N só depois do exit da N−1. Não comeces por mesh shaders, RT, FSR, editor.
- Gates: `prog/samples/gates/*`, `--frames N` (16 nos gates; hello-triangle 90). **Nunca unbounded em AMD.**
- `MESA_VK_ABORT_ON_DEVICE_LOSS=1`. Validation ON. Exit ≠ 0 se houver erros de validation.
- `vk::*` **só** em `prog/engine/drv`. Render nunca chama `vkCmd*`.
- `cfg` de plataforma só em `harpia-rhi` (módulo window/surface).
- Não inventes libs. O que está dentro e **o que entra em que fase**: `docs/Rust-Rewrite-Roadmap.md` §14.1. Nada entra fora da fase que o pede. `wgpu` está fora (D0).
- Inventas só: trait RHI, frame graph, PBR/clima/clipmap, bindless, plugin ABI.
- Swapchain: não destruir a surface; passar `oldSwapchain`; `presentFormat` real (BGRA no X11).
- Sem `unwrap` em GPU. Sem `waitIdle` no frame quente.
- Occupancy entra no lighting no mesmo PR em que o volume nasce, ou não nasce.
- Um clipmap. Nenhum sistema chamado VSM até ser variance ou virtual-paged com o nome certo.
- Hello-triangle corre **sem nenhum plugin**. SPD só na fase bloom. FSR só depois de TAA verde.
- **Sponza** (glTF) é a cena de referência para PBR / luz / sombra (`docs/Rust-Rewrite-Roadmap.md` §0.1). `pbr-grid` é o gate de material; não a substitui.
- FidelityFX vive em `prog/3rdPartyLibs/<nome>/` + LICENSE. Wrapper em `prog/plugins/` (cdylib). Plugin não chama `vkCmd*`.
- Código em `prog/` (ver árvore abaixo). Notas da IA em `memory/`. Doutrina em `docs/`.
- Início de sessão: lê `memory/` (INDEX primeiro — tem o quadro de fases). Fim: actualiza `PROGRESS.md` / `DECISIONS.md` / `LANDMINES.md` **e** os checks em `docs/Rust-Rewrite-Roadmap.md` §15 se uma fase fechou. Sem isto a sessão não fechou. Código não vai para `memory/`.

## Árvore (`prog/`)

Disciplina tipo Dagor: um `prog/engine` com módulos irmãos, 1st/3rd party ao lado, samples **fora** da engine. Sem prefixos `da*`.

```
prog/engine/{core,memory,math,drv,render,scene,app,plugin,editor,…}
prog/{1stPartyLibs,3rdPartyLibs,plugins,samples,tools}
```

`drv` = RHI (Vulkan). Um crate por pasta (`engine/core`, não `engine/core/harpia-core`). Novos domínios só quando a fase os exigir.

## Minas (pagas no C++ Vulkan)

1. RADV GPUVM **não aparece na validation**. `--frames N`, `MESA_VK_ABORT_ON_DEVICE_LOSS=1`.
2. Upload de textura **por mip**. Nunca amostrar mips UNDEFINED.
3. Barreiras: `VK_REMAINING_MIP_LEVELS` / `ARRAY_LAYERS`. UAV view = mip 0 se o recurso tem mips.
4. Dummy 1×1 sampled: destino VS+PS+CS. Dummy 3D UAV em GENERAL.
5. Rain: VS precisa de SRV+VERTEX no occluder. Cones não lêem HDR.
6. Hot-reload de PSO: **antes** de gravar o frame; **não** recriar o pipeline layout.
7. ImGui: pool separado do bindless 8192. Viewport RT usa `present_format()`, não RGBA hardcoded.
8. `begin_frame` espera a fence do slot — writes em dynamic CB **depois** de begin_frame.
9. Row pitch 256 em `copy_buffer_to_texture`.
10. Validation callback pode correr noutro thread → allocator com TLS (mimalloc).
11. Em set 2, `t0 space1` e `u0 space1` são o **mesmo binding 0**. UAVs de buffer começam em `u1`. Arrays HLSL `RWStructuredBuffer[N]` viram **um** binding — SPIR-V precisa de buffers soltos.
12. Não destroias um `PipelineLayout` depois de criar o PSO.

Bindless: heap 8192, **slot 0 = null para sempre**. Spec: `docs/Bindless-Descriptor-Layout.md` (escrita antes do primeiro shader).
