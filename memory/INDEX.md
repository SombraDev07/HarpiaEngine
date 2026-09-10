# Memory index

Persistência entre sessões da IA. **Não** é o allocator (`prog/engine/memory`).

## Onde estás (lê isto primeiro)

**Fase 5 (clima) em curso: fog (com sombras volumétricas), céu, clouds e a
primeira fatia de water feitos; falta SSR na água e rain.** Não comecees mesh shaders, RT, FSR, editor. Cena de referência: **Sponza** (`docs/Rust-Rewrite-Roadmap.md` §0.1).

Quadro oficial com checks: `docs/Rust-Rewrite-Roadmap.md` §15. Cópia viva abaixo em `PROGRESS.md`. Sem gate verde, a fase não está feita.

| Fase | Gate | Estado |
|---|---|---|
| 0 contrato | Null + spec bindless | feito |
| 1 triângulo | `hello-triangle --frames 90` | feito |
| 2 bindless | `gate-bindless` (16 frames) | feito |
| 3 PBR | `gate-pbr-grid` 90 frames | feito |
| **4 CSM+TAA** | **`csm` + `taa` 16; Sponza 90** | **feito** |
| **5 clima** | fog → clouds → water → rain | **agora** (fog, céu, clouds, water base) |
| 6…9 | ver roadmap §15 | não |

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
- Validation: JSON em `/usr/share/vulkan/explicit_layer.d` neste host. `harpia-rhi` ainda procura fallbacks (ver LANDMINES).
- Doutrina canónica em `docs/`. Cópias `Rust-Rewrite-*.md` na raiz estão no `.gitignore`.

## O que não vive aqui

- Código
- Doutrina (`docs/Rust-Rewrite-*.md`)
- Layout bindless (`docs/Bindless-Descriptor-Layout.md`) — contrato; o heap já existe na fase 2

## Fim de sessão

Actualizar `PROGRESS.md`. Se uma fase fechou: checks no roadmap §15 **e** nesta tabela. Acrescentar decisões e minas novas. Sem isto a sessão não fechou.
