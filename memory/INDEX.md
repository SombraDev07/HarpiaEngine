# Memory index

Persistência entre sessões da IA. **Não** é o allocator (`prog/engine/memory`).

## Onde estás (lê isto primeiro)

**Fase 6 (mundo) FECHADA. Fase 7 (GI + post) FECHADA** — GTAO, bloom SPD, auto-exposure,
SSR, probes CPU, occupancy 32³ no lighting (gate) e no compose da Sponza (D71).
**Fase 8 (editor):** wave **P feita** (D74). Exit oficial = **E0** (docking +
Sponza no viewport). Mapa: `docs/Harpia-Editor-Roadmap.md`.

**Os mesh shaders (fase 9) foram escritos fora de ordem e medidos a negativo** — 1.099 ms contra 0.738 na Sponza, opt-in atrás de `-- --mesh`, roadmap §15 actualizado (D60). O desvio não se repete: a fase 8 (editor) é o passo seguinte.

Quadro oficial com checks: `docs/Rust-Rewrite-Roadmap.md` §15. Cópia viva abaixo em `PROGRESS.md`. Sem gate verde, a fase não está feita.

| Fase | Gate | Estado |
|---|---|---|
| 0 contrato | Null + spec bindless | feito |
| 1 triângulo | `hello-triangle --frames 90` | feito |
| 2 bindless | `gate-bindless` (16 frames) | feito |
| 3 PBR | `gate-pbr-grid` 90 frames | feito |
| **4 CSM+TAA** | **`csm` + `taa` 16; Sponza 90** | **feito** |
| **5 clima** | fog → clouds → water → rain | **feito** |
| **6 mundo** | terreno + veg + instâncias | **feito** |
| 7…9 | ver roadmap §15 | **7 fechada** |

## Ordem de leitura (início de sessão)

1. Este ficheiro
2. `PROGRESS.md` — verde / falta / próximo passo concreto
3. `DECISIONS.md` — fechadas (não reabrir sem motivo)
4. `LANDMINES.md` — se fores mexer em GPU / swapchain / barreiras / AMD / SPIR-V
5. Roadmap §15 e a barra em `docs/Rust-Rewrite-Quality-Bar.md`

## Host (não está no código)

- GPU de desenvolvimento: **AMD RX 6700 / RADV**. Gates sempre `--frames N`.
- Repo GitHub: `https://github.com/SombraDev07/HarpiaEngine` (`main`). `gh` autenticado; SSH host key neste host falhou — push por HTTPS.
- Sem `git config user.*` no repo: commits com `GIT_AUTHOR_*` / `GIT_COMMITTER_*` da conta GitHub.
- **DXC não está no PATH.** Shaders: HLSL como spec + `.spvasm` + `prog/tools/assemble_spvasm.py` (ou `spirv-as` se existir).
  **Corrigido (D35): o toolchain está instalado desde Maio/Junho.** `spirv-as`
  assembla e valida os 54 shaders, o build usa-o, e `spirv-val` corre no build.
  Shaders novos escrevem-se em **GLSL** (`harpia-shader-build` compila-os); o
  assembler em Python é fallback com aviso.
- Validation: JSON em `/usr/share/vulkan/explicit_layer.d` neste host. `harpia-rhi` ainda procura fallbacks (ver LANDMINES).
- Doutrina canónica em `docs/`. Cópias `Rust-Rewrite-*.md` na raiz estão no `.gitignore`.

## O que não vive aqui

- Código
- Doutrina (`docs/Rust-Rewrite-*.md`)
- Layout bindless (`docs/Bindless-Descriptor-Layout.md`) — contrato; o heap já existe na fase 2

## Editor (Harpia, não Tucano)

`docs/Harpia-Editor-Roadmap.md` — waves P → E0…E6 com checks. E0 fecha a fase 8
oficial. Authoring ≠ GPU. Animação/física/áudio/input = crates (`gltf`,
`rapier3d`, `kira`, `winit`/`gilrs`), não motores nossos.

## Comparação com a Swarm (Saber)

`docs/Swarm-Reference-Roadmap.md` — REAC 2025, geometria + shaders. **Não** é
port, **não** adianta a fase 8. O que se pega: híbrido explícito, instance
buffer packed, draw list por PSO, pass unit + precache. O que se recusa: uber
256 defines, ECS DSL, GPU path a 100%, occluders manuais. S1 entra *dentro*
dos PRs do editor; S2–S5 só depois do `--frames 8` verde.

## Comparação com a Dagor

`docs/Harpia-vs-Dagor.md` — análise técnica em terreno, render e PBR, lida no
código. Resumo: o BRDF aguenta; só temos uma luz; eles fazem culling de terreno em
CPU e é aí que podemos passar à frente. A `DagorEngine/` está no `.gitignore`.

`docs/Dagor-Ceu-Fog.md` — céu, nuvens, fog e perspectiva aérea. O fog deles é
iluminado pela pilha toda e amostra a sombra das nuvens em dois termos. A
perspectiva aérea chama-se `skies_frustum_scattering` (D19 → D69 no terreno).
O céu não conhece o fog: pede-o por uma macro. As três peças estão ligadas no
`gate-terrain`; o fog da Sponza continua a ser outra composição.

`docs/Dagor-Fase6.md` — vegetação (rendInst), grama, colocação por GPU e mundo,
lidos no código. O culling deles é CPU **também** na vegetação (zero
`draw_indirect`/`dispatch` em `rendInst`, `landMesh` e `heightmap`), o que
confirma a abertura do lado do nosso compute. O que nos falta e lá está:
pirâmide min/max de alturas, LODs com impostores, grama gerada em compute e
vento que escreve motion vectors.

## Isto é AAA?

Não, e `docs/AAA-Gap-Analysis.md` diz porquê com números medidos, o que falta por
ordem de alavancagem, e um roadmap revisto (fases 5.5 e 6.5 novas). Leitura
obrigatória antes de planear a fase 7.

## Fim de sessão

Actualizar `PROGRESS.md`. Se uma fase fechou: checks no roadmap §15 **e** nesta tabela. Acrescentar decisões e minas novas. Sem isto a sessão não fechou.
