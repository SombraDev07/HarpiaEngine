# Swarm (Saber) — o que a Harpia pega, quando, e como

2026-09-14. Referência de **infraestrutura de geometria e shaders**, não de GI
nem de “Unreal no dia 1”. Fonte: Max Bukhalov & Egor Orachev, *Geometry
Rendering and Shaders Infrastructure in Warhammer 40,000: Space Marine 2*,
[REAC 2025](https://enginearchitecture.org/downloads/REAC_2025_Saber.pdf).
Swarm Engine, Saber Interactive.

**Não é um port.** Não há código Swarm neste host. Não entram crates `saber-*`
nem um terceiro runtime de actores. A barra (`docs/Rust-Rewrite-Quality-Bar.md`)
ganha se discordar. A ordem oficial das fases (`docs/Rust-Rewrite-Roadmap.md`
§15) **não muda**: a fase 8 (editor) é o próximo exit. Este documento diz o que
da Swarm cabe *dentro* dessa ordem.

**Parentesco.** A Dagor é a referência de terreno / PBR / clima
(`docs/Harpia-vs-Dagor.md`, `docs/Dagor-Fase6.md`). A Tucano é a referência de
pixel (o 6). A Swarm é a referência de **como a geometria chega ao GPU e como
os shaders não explodem**. As três não competem.

---

## 1. O que eles resolveram (e o que nós já temos)

Space Marine 2: ~60 M triângulos no ecrã, 300–400 k instâncias estáticas,
culling GPU ~0.5 ms, ~40 MB para o path de cull, 5–6 câmaras activas de um
máximo de 64. Renderer **híbrido de propósito**: gameplay na CPU, estático na
GPU. **Sem switch automático.**

Shaders: HLSL uber + permutações. ~200 k pipelines possíveis, ~2 k usados.
Precache de PSO — esqueceram os *engine passes* e tiveram stutter meses antes
do ship.

| Peça Swarm | Harpia hoje | Veredicto |
|---|---|---|
| GPU cull + `drawIndirect` | `gate-terrain`, `gate-veg`, `gate-instances` | **Já feito.** Não reconstruir. |
| Hi-Z / oclusão | veg: 86.2% cortados, imagem bit-idêntica (D55) | **Já feito.** Occluders manuais: recusar. |
| Bindless | heap 8192, slot 0 = null | **Já feito.** |
| Occupancy no lighting | D71 | **Já feito.** Não é o tema Swarm. |
| Dois caminhos (CPU editável / GPU estático) | samples misturam tudo | **Pegar na fase 8.** |
| Instance buffer packed (célula + quat + flags) | `Instance { pos_scale, color }` em float4 | **Pegar depois do editor**, no mundo. |
| Draw requests por bucket de PSO | samples gravam draws à mão | **Pegar quando o Outliner tiver N objectos.** |
| Pass de shader como unidade | 5 `blit.ps.glsl` copiados | **Pegar já como higiene**; infra depois. |
| Precache de PSO | create no `init` do sample | **Pegar no editor** (primeiro sítio que stutters). |
| Uber 256 defines / 5 h de cook | GLSL por gate, `harpia-shader-build` | **Recusar a explosão.** Manter o cook incremental. |
| ECS DSL + 40 systems de render | `bevy_ecs` (D38) | **Recusar.** |
| Mesh shaders / GPU path a 100% | medido negativo (D60) | **Recusar** até todas as passes mudarem. |

---

## 2. O que desejamos de facto (quatro fatias)

Não a engine inteira. Estas quatro, e só estas:

1. **Híbrido explícito.** O tipo do objecto escolhe o path no editor / no
   glTF / no ECS. Estático, scatter, vegetação → GPU. Selecção, gizmos,
   material override, cinemática → CPU. O renderer **não** decide sozinho.
   (Swarm: “in 90% of cases static is best”; pior caso = tudo em Actor.)
2. **Um sítio para dados de instância.** Buffer GPU global: partição **estática**
   (orçamento fixo, packed, load-once) + **dinâmica** (cresce, archetypes).
   Customização por índice de paleta, **sem** desinstanciar. Sem god-class
   `AnimInstance`.
3. **Draw list = gather → map → reduce por chave.** A chave é
   `(PSO, mesh, pass)`. Um `drawIndirect` (ou DIP) por bucket, payloads por
   câmara. Não um `vkCmdDraw` por entidade no Outliner.
4. **Shaders como passes, não como cópias.** Um blit, um VS fullscreen, PSO
   cacheado por `(shader, present_format, vertex layout)`. Permutações
   **geradas da cena**, não enumeradas. Branching dinâmico > `#ifdef`.
   **Zero** `vkCreateGraphicsPipelines` no frame quente.

Tudo o resto da palestra (Actor vs ECS vs static como três frontends, 80
passes com 2 k condições, cache de 700 MB, 2 frames de latência, occluders
manuais, 64 câmaras) **não entra**.

---

## 3. Quando — encaixe nas fases oficiais

A fase 8 **não ganha um exit extra**. Docking + viewport `--frames 8` continua
a ser o verde. As fatias Swarm são restrições *dentro* dos PRs, ou trabalho
*depois* desse verde.

```
agora ──────────────────────────────────────────────►
fase 8 editor          8+ polish         mundo/veg          9 opcional
S1 higiene             S2 draw list      S3 instance mgr    S5 placement
   path híbrido           buckets           packed           (máscaras)
   PSO no load         S4 pass unit      S3 gate 100k
   blit único             cook cena
```

| Slice | Fase | Bloqueia o exit da fase 8? |
|---|---|---|
| **S1** higiene híbrido + PSO + blit | 8 (dentro dos PRs do editor) | Não. É como se faz o viewport. |
| **S2** draw list por bucket | 8+, depois do `--frames 8` verde | Não. |
| **S3** instance manager packed | mundo / veg (dívida da fase 6: VT ainda aberta) | Não. |
| **S4** pass unit + combos da cena | quando o editor tiver >1 permutação de material | Não. |
| **S5** placement GPU por máscara | depois de haver brush no editor | Não. Precisa de ferramenta. |
| Mesh shaders / GPU 100% / uber / ECS DSL | **nunca neste roadmap** | — |

---

## 4. Como — PRs, crates, gates

### S1 — Higiene da fase 8 (já no primeiro PR do editor)

**O quê.** Viewport 3D da Sponza pelo path GPU (o que já corre em
`prog/samples/sponza`). O objecto seleccionado (e gizmos, mais tarde) pelo
path CPU: transform/material escritos depois de `begin_frame`, um draw
directo, **não** um segundo cull compute só para o cubo seleccionado.

**Onde.** `prog/engine/editor` + sample `prog/samples/editor` (não inflar o
HUD do `storm`, D67). Pool ImGui/egui **≠** bindless 8192. Viewport RT =
`present_format()`, free de descriptors atrasado (mina 7).

**PSO.** Todos os pipelines do editor (blit, gizmo, viewport compose) criados
no load. Hot-reload **antes** de gravar o frame; **não** recriar o
`PipelineLayout` (mina 6). Se um material novo aparecer, o hitch é no load
do asset, não no clique do Outliner.

**Blit.** Um GLSL partilhado (`prog/engine/render/shaders/` ou
`prog/1stPartyLibs/fullscreen`). Os cinco `blit.ps.glsl` dos gates deixam de
ser copiados em PRs novos. Não é um rewrite dos gates velhos no mesmo PR.

**Exit (continua o da fase 8).** `cargo run -p harpia-editor -- --frames 8`
(nome exacto do binário quando nascer): docking + viewport Sponza,
validation 0, resize sem crash. Null `--frames 8`. Sem isto, S2 não começa.

**Proibido neste slice.** Instance buffer packed, scatter compute, codegen de
passes, segundo ECS.

---

### S2 — Draw list por bucket (8+)

**Quando.** O Outliner tem dezenas de meshes com PSOs diferentes e o record
do command buffer começa a cheirar a um draw por entidade.

**O quê.** Pedido de draw = `{ instance_id, mesh, pso, camera_mask }`.
Workers (ou, no dia 1, um thread) juntam por chave; um DIP / `drawIndirect`
por bucket. Payloads escritos para staging mapeado — Swarm faz isto em
~1 MB. Nós: o mesmo, à escala do editor, não 10 k pedidos.

**Onde.** `harpia-render`, não no crate do editor. O editor emite pedidos; o
render agrupa. `vkCmd*` continua só em `drv`.

**Gate.** Estender `gate-instances` (ou um `gate-drawlist`): ≥3 meshes, ≥3
PSOs, N instâncias, **número de draws gravados = número de buckets**, não N.
`--frames 16`, validation 0. CPU time do record não escala com N (o cull
já prova isto em D47; agora é o *sort*).

**Latência.** Swarm aceita 2 frames (readback). Nós **não**: o editor precisa
de pick no mesmo frame. Readback só em gates. Cull GPU → indirect, como já
é.

---

### S3 — Instance manager packed (mundo, depois do editor verde)

**Quando.** O editor (ou o mundo) precisa de colocar milhares de props, não
2 500 cubos de teste. A dívida VT/feedback da fase 6 pode andar no mesmo
comboio se o PR for honesto; não misturar os dois sem gate próprio.

**Layout (contrato, não C++).** Partição estática: quat comprimido, escala,
posição **relativa à célula** da grelha (a janela toroidal de 17 tiles, D62,
já é a grelha). Flags: hidden, shadow caster, LOD. Índice de paleta 0–127
para tint/material sem partir o instance draw. Partição dinâmica: só o que
muda (transform da selecção, dissolve, mais tarde skin). Upload uma vez no
load da cena; updates dinâmicos depois de `begin_frame`.

**Onde.** `harpia-render` (buffer + alloc). `gate-instances` passa a escrever
este layout. Veg (`gate-veg`) adopta o mesmo buffer quando o packed existir
— um manager, não dois.

**Gate.** `gate-instances` a **100 k** packed, 1 `drawIndirect`, CPU de submit
plana (já 0.17 ms a 1 M no layout gordo, D47 — o packed é para **memória e
bandwidth**, não para ganhar esse número outra vez). Orçamento GPU do buffer
estático declarado no gate (Swarm: ~5 MB / 300 k). Validation 0.
`--frames 16`. A Sponza **não** precisa disto para o atrium; o gate é a
prova.

**Recusar.** God-object de animação. Desinstanciar para mudar uma cor.

---

### S4 — Pass unit + combos da cena (quando o material permutar)

**Quando.** O editor deixar de ter “um PS de compose” e passar a ter
variantes (skinned / unlit / gizmo / distorção). **Não** no primeiro PR do
viewport.

**O quê.** Um pass = shaders + vertex format + render state + `present_format`
+ flags (material vs engine, precache sim/não). `harpia-shader-build` já é o
sítio (D35). Acrescentar: lista de passes, compile incremental por ficheiro,
PSO blob por pass. Combos **lidos da cena** (glTF da Sponza + materiais do
editor), não o produto cartesiano dos defines.

**Regra Swarm que copiamos à letra:** *keep desc parameters and conditions
low; split a pass in two sometimes; dynamic branching is ok; don’t forget to
compress; engine-pass permutations are easy to lose control of.*

**Gate.** Cook da Sponza + editor: tempo e nº de SPIR-V únicos impressos.
Nenhum pass de engine com “todas as combinações”. Primeiro uso de um
material no viewport = 0 creates de pipeline (precache no load). Se o
precache falhar, o frame **não** cria o PSO — falha o load com erro, como
validation ≠ 0.

**Recusar.** 256 defines por pass. Cache de 5 h multi-plataforma. Enumerar
200 k pipelines “porque um dia alguém liga a flag”.

---

### S5 — Placement GPU (só com ferramenta)

**Quando.** O editor tiver brush / máscara no terreno. Sem UI de pintura isto
é um compute órfão — a barra recusa.

**O quê.** Compute amortizado (<0.5 ms async, Swarm) gera instâncias a partir
de máscaras (até 16 tipos). Cache perto da câmara. LOD com redução de
densidade. Isto **sobre** o manager S3, não em paralelo.

**Referência irmã.** Dagor `gpuObjects` (`docs/Dagor-Fase6.md`) — mesma
ideia, outro código. Implementar a técnica, não transplantar.

**Gate.** `gate-placement`: máscara 1k² → instâncias visíveis mudam com a
câmara, A/B `-- --no-placement`, occupancy/lighting **não** entram neste
binário. Validation 0. `--frames 16`.

---

## 5. Recusar (lista explícita)

Não entram neste roadmap, mesmo “porque a Saber faz”:

- Três frontends (Actor / ECS próprio / static) como runtimes separados.
  Temos `bevy_ecs` + sample directo. O híbrido é **dois paths de submit**,
  não três engines.
- DSL `.ecs` + codegen C++. Reflection da fase 8 = `bevy_reflect` ou macros
  nossas, no crate `editor`.
- Uber-shader HLSL com macro soup. GLSL + `harpia-shader-build` continua.
- Switch automático CPU ↔ GPU. O asset marca o path.
- Arquitectura a 2 frames de latência no editor / no pick.
- Occluders low-poly manuais como solução (eles admitem que falham; chunks
  grandes não saem). Hi-Z da geometria real, como a veg.
- Mesh shaders por omissão, GPU path a 100%, FSR, RT — fase 9, e só com
  medida (D60 já disse não aos mesh shaders).
- DGI / DDGI / WorldSDF — continuam fora (D71). A Swarm nem os discute.

---

## 6. Teste de honestidade (antes de um PR “Swarm”)

1. Isto é uma das **quatro fatias** da §2? Senão, não é Swarm, é scope.
2. A fase 8 já está verde, **ou** o PR é S1 (higiene do viewport)?
3. Há gate `--frames N` em RADV, validation 0? Sem pixel/contagem, não existe.
4. `vkCmd*` só em `drv`? Plugin/editor não grava draws.
5. Hello-triangle continua **sem** plugins e sem instance manager.
6. Occupancy / GI não vêm “de boleia” neste PR.
7. O nome bate com o que o código faz? “GPU-driven” que ainda percorre N
   entidades na CPU para gravar N draws é mentira no submit — a mesma regra
   da barra, no command buffer.

---

## 7. Ordem da próxima sessão (fase 8)

O próximo código **não** é S3–S5. É o editor:

1. crate `harpia-editor` + sample, pool ≠ bindless, viewport `present_format`.
2. Sponza no viewport (path GPU). Selecção mínima = path CPU (S1).
3. PSO no load. Blit partilhado nos ficheiros **novos**.
4. `--frames 8`, resize, Null 8, validation 0.
5. Só então Outliner/Inspector, file dialog, gizmos — e S2 quando o número
   de draws o pedir.

Detalhe do exit da fase 8: `docs/Rust-Rewrite-Roadmap.md` §15.
Authoring (prefabs, brush, cook, Inspector): `docs/Harpia-Editor-Roadmap.md`.

