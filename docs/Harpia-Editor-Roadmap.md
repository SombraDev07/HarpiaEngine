# Harpia Editor — roadmap (checks)

2026-09-14. O editor **constrói o mundo** e o converte para o runtime. Não é um
visualizador de meshes. A Swarm descreve o pipeline; este ficheiro é a ordem
de implementação na Harpia.

**Produto:** Harpia (`harpia-*`). Tucano é referência de pixel, não o nome
destas pastas. Barra: `docs/Rust-Rewrite-Quality-Bar.md`. Geometria GPU:
`docs/Swarm-Reference-Roadmap.md`. Fases oficiais: `docs/Rust-Rewrite-Roadmap.md`
§15.

**Exit oficial da fase 8 = só a wave E0** (`--frames 8` docking + viewport 3D,
validation 0, resize sem crash). E1–E6 são o editor a sério; não saltam a E0
e **não** são Unreal no dia 1.

Animação, física, áudio e input **não se escrevem**. Entram como crates na
wave que os pede (tabela §8).

Marca `[x]` no mesmo PR que fecha o item. Sem gate, o check não conta.

---

## 0. Onde estamos

| Wave | Nome | Exit | Estado |
|---|---|---|---|
| **P** | Pré-requisitos | crates / dados de autoria, não um binário novo | **feito** (D74) |
| **E0** | Shell | `harpia-editor --frames 8` docking + viewport | [ ] |
| **E1** | Level editor | pick + transform + outliner + gravar/abrir cena | [ ] |
| **E2** | Prefabs | instanciar + override + nested 1 nível | [ ] |
| **E3** | Inspector data-driven | reflection; componente novo aparece sozinho | [ ] |
| **E4** | Assets + material | browser + params PBR; **sem** Shader Studio | [ ] |
| **E5** | Placement / scatter | máscaras + brush; GPU instances | [ ] |
| **E6** | Cook / export | validar → export runtime → path GPU/CPU | [ ] |

Cena de olho: **Sponza** no viewport (não um quad magenta). Overlay do
`storm -- --interactive` (D67) **não** conta.

---

## 1. Pipeline (o que o editor *é*)

```
Authoring (flexível, paths, hierarquia, overrides)
    → Validation
    → Export / Cooking
    → Runtime (Actor-CPU | ECS | GPU instance)
```

O artista trabalha no lado esquerdo. O renderer só vê o lado direito. Um
`Mesh { vb, ib }` no `harpia-scene` **é runtime**. Não é o ficheiro da cena.

| Termo | Significado aqui |
|---|---|
| Authoring | criar/editar conteúdo *antes* do cook |
| Scene authoring | nível: objectos, transforms, hierarquia, luz, materiais, entidades, áreas procedurais |
| Prefab | asset reutilizável (casa, árvore, ruína) |
| Prefab instance | colocação na cena; não duplica os dados do prefab |
| Per-instance override | cor, escala, material, desligar um filho — sem sujar o prefab |
| Nested prefab | prefab dentro de prefab (vila → casa → porta) |
| Placement | posicionar: drag-drop, gizmo, brush, regras, código |
| Procedural placement | densidade, slope, bioma, máscaras, probabilidade |
| Scattering | muitos objectos pequenos (pedras, detritos, erva) |
| Scatter brush | pincel: raio, densidade, tipos, escala/rotação, máscara, apagar |
| Density / placement mask | onde e com que densidade |
| Terrain mask painting | pintar máscaras no clipmap; **não** sculpt raise/lower no MVP |
| GPU instance | path Swarm S1/S3; LOD per instance quando o packed existir |
| Runtime representation | campo no authoring: `Cpu` (selecção/gameplay) ou `Gpu` (estático/scatter) |
| Editor reflection | Inspector a partir dos componentes |
| Scene / prefab serialization | gravar o lado *authoring* |
| Cooking | authoring → buffers, LODs, packed instances |
| Asset dependencies | casa → meshes → materiais → texturas → collider |
| Validation | refs partidas, escala 0, componente obrigatório em falta |
| Runtime preview | o viewport usa o mesmo renderer da Sponza |

---

## 2. Layout alvo (não no E0 inteiro)

```
┌──────────┬─────────────────────────┬─────────────┐
│ Outliner │      Viewport 3D        │ Inspector   │
│ World    │   gizmos + pick         │ Transform   │
│ Terrain  │                         │ Rendering   │
│ Env      │                         │ Materials   │
│ Gameplay │                         │ Components  │
├──────────┴─────────────────────────┴─────────────┤
│ Asset browser │ Console │ Profiler (opcional)    │
└──────────────────────────────────────────────────┘
```

E0: docking vazio + viewport. O resto encaixa nestes painéis, não em janelas
soltas por ferramenta.

---

## 3. Wave P — pré-requisitos (antes ou no PR 0 da E0)

Sem isto o editor nasce a gravar handles GPU no `.scene` e parte no reload.

### Já existe — não reconstruir

- [x] RHI Vulkan + Null; `present_format()`; `--frames N`; validation abort
- [x] Heap bindless 8192, slot 0 = null
- [x] Deferred PBR, CSM, TAA, clima, GI (fases 0–7)
- [x] Sponza glTF (`gltf` + `image`)
- [x] Clipmap + veg + `gate-instances` (`drawIndirect`)
- [x] `bevy_ecs` (D38) em `harpia-scene` (Mesh/Material/Bounds/Visible — **runtime**)
- [x] Stack egui (`egui` + `egui-winit` + `egui-ash-renderer`) no overlay D67
- [x] Input de janela via `winit` (rato/teclado). Não escrever um input manager
- [x] `harpia-shader-build` + GLSL

### Já fechado nesta wave

- [x] Crate `prog/engine/editor` (`harpia-editor`) + sample `prog/samples/editor`
      (binário `harpia-editor`). **Não** inflar o HUD do `storm` (D67)
- [x] Pool de descriptors **à parte** do bindless 8192 (mina 7; overlay D67).
      `--frames 8` não instancia egui
- [x] Viewport = RT offscreen em `present_format()` (BGRA neste host)
- [x] PSOs do editor criados no **load** (Swarm S1). Zero create no clique
- [x] Blit / VS fullscreen **partilhados** em `prog/engine/render/shaders/`
- [x] Modelo de dados **authoring** ≠ GPU: paths, UUID, transform local, parent,
      prefab id, `RuntimePath { Cpu, Gpu }`. `harpia-scene::Mesh { vb, ib }` é o
      produto do cook/sync
- [x] Hierarquia: `ChildOf` / `Children` do `bevy_ecs`
- [x] `Name` no authoring (`bevy_ecs::Name`). Round-trip sem `Entity(`
- [x] `serde` + RON (`.scene`). `postcard` só no cook (E6)
- [x] `bevy_reflect` em `RuntimePath` / `MeshRef` / `MaterialRef`
- [x] File dialog: `rfd` 0.17 com `xdg-portal` **sem** feature `wayland`.
      Pick/save **nunca** de `Sample::frame`.

Scatter packed / draw list / height no pincel: **wave E5**, não P.

### Não é pré-requisito do editor

- Mesh shaders, RT, FSR, DDGI, WorldSDF
- Sculpt de terreno (raise / lower / erosion) — o PDF Swarm **não** prova; o
  nosso terreno é clipmap. Máscaras sim (E5)
- Shader Studio / editor de nós
- Rede, save de jogador, Lua

---

## 4. Wave E0 — shell  `[ ]`

Fecha a **fase 8** oficial.

- [x] Sample próprio, `--frames 8` default, `--interactive` opt-in, aviso AMD
- [ ] Docking egui (árvore de painéis, mesmo que vazios)
- [ ] Viewport 3D: Sponza (ou um cubo PBR se o load da Sponza for o PR
      seguinte — **não** magenta). Path GPU para a cena (S1)
- [ ] Câmara do viewport (órbita / fly) com `winit`, não com um crate de input novo
- [ ] Resize  sem crash; surface + `old_swapchain`; validation 0
- [ ] Null `--frames 8`
- [ ] Hello-triangle **continua** sem editor, sem plugins, sem instance manager

**Gate:** `cargo run -p harpia-editor -- --frames 8` (ajustar o nome do
binário). RADV, `MESA_VK_ABORT_ON_DEVICE_LOSS=1`.

**Proibido nesta wave:** Outliner cheio, prefabs, brush, rapier, kira,
gravar `.scene`.

---

## 5. Wave E1 — level editor  `[ ]`

O centro do trabalho: colocar, transformar, organizar, gravar.

- [ ] Selecção no viewport (pick: ray vs AABB no CPU — path CPU da selecção, S1)
- [ ] Gizmos translate / rotate / scale (um crate de gizmo **ou** desenho
      nosso em `editor`; `vkCmd*` só em `drv`)
- [ ] Drag-and-drop do browser (E4) *ou*, até lá, spawn de um cubo/glTF pelo menu
- [ ] Outliner: árvore World / Environment / (Gameplay vazio). Click → selecciona
- [ ] Reparent no Outliner
- [ ] Undo/redo mínimo (transform + spawn/despawn). Sem isto o artista não trabalha
- [ ] Gravar / abrir `.scene` (authoring). Round-trip: fecha o editor, abre, os
      transforms batem
- [ ] Luz: a directional que já existe, exposta no Inspector (posição/ângulo).
      Não clustered nesta wave

**Gate:** `--frames 8` continua verde **e** teste CPU: `save → load → equal`
num cubo. Pick: clicar o cubo selecciona-o (Null ou RADV).

---

## 6. Wave E2 — prefabs  `[ ]`

- [ ] Prefab serialization (filhos, refs, ids **locais**, dependências)
- [ ] Criar prefab a partir da selecção
- [ ] Instanciar na cena (N instâncias, um asset)
- [ ] Per-instance override: transform sempre; **mais** um de: cor / material /
      filho desligado. Paleta Swarm: override **sem** desinstanciar no runtime GPU
- [ ] Nested prefab, **um** nível no MVP (casa → porta). Vila de vilas fica
      para quando o 1 nível não partir
- [ ] Editar o prefab abre o asset, não “explode” todas as instâncias sem querer
- [ ] Apply / revert overrides

**Gate:** 50 instâncias do mesmo prefab; uma com tint override; reload da cena
mantém o override; o prefab original não mudou.

**Runtime:** instâncias estáticas sem override → path `Gpu` quando S3 existir.
Com override ou seleccionado → `Cpu` até o packed ter paleta (Swarm S3).

---

## 7. Wave E3 — Inspector data-driven  `[ ]`

- [ ] Reflection nos componentes de authoring (`bevy_reflect` + metadados:
      nome, min/max, hidden)
- [ ] Inspector gerado: Transform, Name, RuntimePath, refs de mesh/material
- [ ] Componente novo registado → aparece no Add Component **sem** UI à mão
- [ ] Valores default a partir do tipo
- [ ] Sem codegen C++ / DSL `.ecs` da Saber. Sem segundo ECS

**Gate:** um componente de teste (`Health { hp: u32 }`) só com `#[reflect]`
aparece e serializa. Sem widget custom.

---

## 8. Wave E4 — asset browser + material  `[ ]`

- [ ] Browser: meshes, texturas, materiais, prefabs. Pesquisa por nome
- [ ] Thumbnails: estático (PNG cook) ou captura 1 frame; **não** bloqueia E4
      se a lista textual existir
- [ ] Arrastar mesh/prefab para o viewport = placement
- [ ] Dependências visíveis (casa → parede → albedo)
- [ ] Material editor **simples**: albedo / roughness / metallic / normal /
      emissive / fuzz — os campos que o GBuffer já tem. Sem nós
- [ ] Shader Studio **fora** (Swarm S4 / pass unit é cook, não UI de artista)

**Gate:** abrir Sponza pelo browser no editor; mudar roughness dum material
e ver o pixel no viewport (runtime preview). `--frames 8` não hitcha (PSO no
load ou no import, não no slider a 60 Hz).

---

## 9. Wave E5 — terreno, máscaras, scatter  `[ ]`

Precisa do packed (S3) se forem milhares de pedras. Heightquery já existe.

- [ ] Instance manager packed (Swarm S3) — `gate-instances` a 100 k
- [ ] Draw list por bucket (Swarm S2) — quando N objectos × N PSOs
- [ ] Terrain no Outliner: o clipmap que já existe, **não** um segundo terreno
- [ ] Terrain mask painting: pelo menos 2 canais (ex. grass / rock) no height
      snapshot da câmara
- [ ] Density mask + placement mask (onde pode / não pode)
- [ ] Scatter brush: raio, densidade, mesh types, escala e rotação aleatórias,
      espaçamento, apagar
- [ ] Regras procedurais mínimas: slope + altura (os dois que o clipmap dá)
      + máscara. Bioma/estrada **depois**, quando houver esses dados
- [ ] Resultado = GPU instances (S1/S3), LOD per instance quando o packed
      tiver LOD; até lá um LOD
- [ ] **Não** raise/lower/erosion nesta wave

**Gate:** `gate-placement` ou o editor `--frames 16`: pintar máscara →
instâncias aparecem; `-- --no-placement` / limpar brush as tira. Validation 0.
Occupancy/GI **não** vêm de boleia.

---

## 10. Wave E6 — validation, export, cooking  `[ ]`

- [ ] Validation: mesh em falta, material em falta, prefab partido, escala 0,
      collider em falta *se* o componente Collision estiver presente, path
      Gpu sem mesh
- [ ] Relatório no painel (não `unwrap`)
- [ ] Scene export: authoring `.scene` → runtime (buffers, packed instances,
      lista CPU para o que é editável)
- [ ] Cooking: glTF → mesh intern; mips; `meshopt` se ainda não estiver no
      import (autorizado fase 5, usar quando doer)
- [ ] Asset dependencies: export falha se faltar um filho
- [ ] Runtime preview = o mesmo graph da Sponza (já o viewport). Física /
      áudio no preview só se a lib da §11 já estiver ligada nessa wave

**Gate:** cook duma cena com 1 prefab + 1 scatter; o binário `sponza` **não**
é o teste — um `harpia-editor -- --cook` ou tool `prog/tools/cook-scene`
escreve o runtime e um sample `--frames 16` carrega-o com validation 0.

---

## 11. Bibliotecas (não inventar)

Nada disto entra no `Cargo.toml` **antes** da wave que o usa.

| Domínio | Crate | Wave | Notas |
|---|---|---|---|
| UI | `egui` + `egui-winit` + `egui-ash-renderer` | E0 | Já no overlay. **Não** `egui-wgpu` (D0) |
| ECS | `bevy_ecs` | P | D38. Hierarquia daqui, não um grafo nosso |
| Reflection | `bevy_reflect` | E1/E3 | Inspector. Sem DSL Saber |
| Cena texto | `serde` + `ron` (ou `serde_json`) | E1 | Authoring |
| Cena cozida | `postcard` | E6 | Runtime |
| File dialog | `rfd` | E1 | Não bloquear sem fence |
| Assets 3D | `gltf` + `image` | já | Não `bevy_asset` sem decisão nova |
| Física | **`rapier3d`** | preview de colisão / rigid, **depois** de E1 ter componente `Collider` authoring | D33: Rust puro. Box3D/Jolt só se rede/replay o exigir. **Não** escrever solver |
| Animação | clips do **`gltf`** (sample interpolado) | quando um prefab skinned aparecer no viewport | Sem graph editor, sem “AnimInstance”. Skinning GPU é shader, não um motor de animação |
| Áudio | **`kira`** (+ `cpal`) | quando houver listener + 1 fonte na cena | Fase 9 no §14.1; não no E0 |
| Input | **`winit`** (editor) + **`gilrs`** só no jogo | E0 usa winit+egui | Não escrever action mapping de raiz; `gilrs` não é preciso para o rato do gizmo |
| Profiling | `puffin` ou `tracy-client` | painel inferior, depois de E0 | §14.1 fase 8 |

Collider **authoring** (caixa / cápsula / mesh path) pode existir em E1–E3 como
dados e validation. O `rapier3d` liga-se quando o viewport tiver de **simular**
ou query raycast de pick mais honesto que AABB. Pick AABB chega no E1.

---

## 12. Fora deste roadmap

- Unreal Blueprint, Sequencer, Landscape sculpt, Niagara
- Shader graph / Shader Studio
- Três runtimes Actor / ECS / GPU como engines separadas — só **dois paths de
  submit** (S1)
- `waitIdle` no frame quente; create de PSO no clique
- `vkCmd*` em `editor` / plugins
- Inflar o `storm` até ser o editor
- Animação/física/áudio/input caseiros
- Rede, Lua, FSR, mesh shaders por omissão

---

## 13. Como fechar um check

1. Gate `--frames N` (8 no shell, 16 nas tools) em RADV, validation 0.
2. Null `--frames 8` se tocar em `drv` / loop.
3. Authoring round-trip quando o item for serialização.
4. Pixel ou contagem quando o item for scatter/material (A/B).
5. Marcar `[x]` **neste ficheiro** + `memory/PROGRESS.md` + §15 se for E0.

Próximo código: **P (crate + viewport) → E0**. Não E5, não rapier, não kira.
