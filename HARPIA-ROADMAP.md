# HarpiaEngine — Roadmap Único

> **Documento canônico.** O plano.
> Estado de execução: [PROGRESS.md](PROGRESS.md) — **F0.a e F0.b concluídas e verificadas**.
> Engine nova, do zero. Dagor Engine é **referência de algoritmo**, nunca dependência.
> Alvo: Linux + Vulkan 1.3, C++20, CMake.
>
> Base: auditoria de `/home/bruno/DagorEngine` (BSD 3-Clause) + escada de capacidades,
> estendida com as linhas que a escada não tem.

---

# ÍNDICE

- [Premissa](#premissa)
- [Decisões travadas](#decisões-travadas-não-reabrir)
- [O que a escada da imagem não cobre](#o-que-a-escada-da-imagem-não-cobre)
- [Onde superamos, igualamos e não superamos](#onde-superamos-igualamos-e-não-superamos)
- [PARTE I — Fundação](#parte-i--fundação)
- [PARTE II — Render](#parte-ii--render)
- [PARTE III — Mundo](#parte-iii--mundo-terreno-água-vegetação)
- [PARTE IV — Simulação & gameplay](#parte-iv--simulação--gameplay)
- [PARTE V — Sequência de execução](#parte-v--sequência-de-execução)
- [PARTE VI — Regras invioláveis](#parte-vi--regras-invioláveis)

---

# Premissa

## Por que não dá pra copiar o C++ do Dagor

Todo `.cpp` de render do Dagor depende de três coisas ao mesmo tempo:

1. `d3d::` — RHI com semântica **D3D11** (`set_texture`, `set_render_target`)
2. `prog/engine/shaders/` — **27k linhas** de runtime: shadervars globais, blocos, *stcode*
3. **DSHL** — linguagem de shader própria (**1010** arquivos `.dshl`) com compilador próprio

Arrastar um arrasta os três. Portanto: **lê-se o Dagor, não se linka o Dagor.**
A unidade de reuso é o **algoritmo e a matemática do shader** — nunca o arquivo `.cpp`.

## O que realmente vale copiar: a lista de compras

O Dagor levou 20 anos escolhendo dependências. Todas são projetos independentes, usáveis sem ele:

| Precisa de | Dagor usa | Usamos |
|---|---|---|
| Compressão de animação | `acl` | **ACL** — mesma |
| Navmesh | `recastnavigation` | **Recast/Detour** — mesma |
| Áudio | `miniaudio` (FMOD opcional) | **miniaudio** — mesma |
| Física | Bullet + **Jolt** | **Jolt** — só o novo |
| Rede | `enet` | **enet** — mesma |
| Upscaling | `fsr2`, `fsr3.1`, `ffx` | **FSR 3.1** — mesma |
| Gizmos / node editor | `ImGuizmo`, `imgui-node-editor` | **as mesmas** |
| Compressão de malha | `draco` | **draco** / meshoptimizer |
| Shader compiler | `dxc` | **DXC** — mesma |
| Crash reporting | `breakpad`/`crashpad` | **crashpad** |
| Scripting | **daScript** + Quirrel | **daScript** — repo separado, BSD |

**daScript é um projeto autônomo** (`GaijinEntertainment/daScript`, BSD), não um include do Dagor.
Podemos usá-lo sem violar a regra. É uma das melhores peças que a Gaijin produziu.

## Licença

BSD 3-Clause permite copiar trecho, exigindo retenção do copyright e do disclaimer.
Arquivo com trecho derivado leva cabeçalho de atribuição; `NOTICE.md` da raiz consolida.
Uso comercial liberado.

---

# Decisões travadas (não reabrir)

| Decisão | Escolha | Por quê |
|---|---|---|
| Shader language | **HLSL → SPIR-V via DXC** | Linguagem própria é o maior peso morto do Dagor: 1010 arquivos presos a um compilador que eles mantêm pra sempre. |
| Variantes de shader | defines + **SPIRV-Reflect** | Substitui shadervars + stcode sem inventar linguagem. |
| Backend gráfico | **Vulkan 1.3 só** | Dagor mantém 7 backends atrás de uma API estilo D3D11. Um feito bem bate sete medianos. |
| Dynamic rendering | `VK_KHR_dynamic_rendering` | Sem render pass / framebuffer objects. Metade do boilerplate some. |
| Descritores | **Bindless dia 1** | `descriptor_indexing` + `UPDATE_AFTER_BIND` + `PARTIALLY_BOUND`. |
| Sincronização | `synchronization2` + render graph | Barreiras derivadas do grafo, nunca à mão. |
| Alocação GPU | **VMA** | Não é onde está a diferenciação. |
| Build | **CMake** | `jam` é parte do que torna o Dagor impenetrável de fora. |
| Testes de imagem | **Golden image por pass, no CI** | A linha que falta na escada e a que mais protege o projeto. |

---

# O que a escada da imagem não cobre

A imagem tem 31 linhas. Falta o seguinte, e o Dagor tem tudo:

| Ausente na escada | Dagor | Por que importa |
|---|---|---|
| **Terreno / landscape** | `landMesh` 14.7k, `heightmap` 4.5k, `terraform` 1.6k | Sem isso não existe mundo aberto |
| **Água / oceano** | `fftWater` **13.5k** | Um dos sistemas mais caros que existem |
| **Vegetação** | `rendInst` **36.8k** (o maior módulo deles) | Define o custo de qualquer cena externa |
| **Decals** | `decals`, `burningDecals`, `clipmapDecals` | Sem decal, tudo parece limpo demais |
| **Vento** | `wind` | Amarra vegetação, tecido, partículas e água |
| **Input** | `daInput` 4.6k | É o que o jogador toca primeiro |
| **Testes** | `doctest`, `catch2` | Sem golden image, otimização de render é aposta |
| **Save / versionamento** | DataBlock `.blk` | O dia em que os saves quebram chega sempre |
| **Crash reporting** | `breakpad`, `crashpad` | — |
| **Localização** | — | Aparece só dentro de "UI" na escada |

---

# Onde superamos, igualamos e não superamos

Honestidade sobre o alvo. 20 anos e centenas de engenheiros não se batem em volume.

### 🟢 Superamos — por arquitetura, não por esforço

| Eixo | Dagor | Harpia |
|---|---|---|
| API de render | `d3d::`, semântica D3D11 de 2010 | bindless + render graph nativos |
| Linguagem de shader | DSHL própria + compilador próprio | HLSL + DXC, ferramental padrão |
| Backends | 7, manutenção alta | 1 moderno |
| Build | `jam`, opaco | CMake, onboarding em minutos |
| Testes de render | sem golden image sistemático | golden image por pass no CI |
| **Editor** | `daEditorX` **sem Play mode** | Play mode desde cedo |
| Reflexão/serialização | DataBlock, sem meta unificada | TypeRegistry único, inspector auto-gerado |

### 🟡 Igualamos — com menos código, lendo o deles

Sombras, atmosfera, GI, post-processing, água, terreno, AA/upscaling.
Aqui o Dagor é referência boa. Copiamos a matemática e entregamos o mesmo resultado
com menos superfície, porque não carregamos 20 anos de compatibilidade.

### 🔴 Não superamos — e tudo bem

`rendInst` (36.8k linhas de instanciação com LOD/impostor/culling afinados por uma década),
`daRg` (37.5k de framework de UI reativo), a matriz de plataformas (6 plataformas, consoles),
e a validação de ter rodado em milhões de máquinas.

**O vão real do Dagor não é técnico — é de produto.** Ele é engine de estúdio.
É aí que a Harpia tem espaço, e é por isso que Editor, Reflection e Asset sobem ao topo.

---

# PARTE I — Fundação

> *"Cheap to climb early, brutal to retrofit — these constrain every layer above."*

## Alvos de coluna

| Linha | Alvo | Topo | Decisão |
|---|---|---|---|
| Threading | Job system + deps (3) | Fibers | Para na 3 — fibers quebram Tracy e gdb |
| Memory | Pools + tracking (3+) | TLSF | Para na 3, tracking sobe junto |
| **Reflection / types** | **Full meta (4)** | Full meta | **Topo. Gate de tudo.** |
| Backend portability | Raw VK (2) | RHI | **Para na 2 de propósito** |
| Binding model | Bindless (3) | Bindless | Topo, dia 1 |
| **Asset handling** | **DB + GUID + hot reload (4)** | idem | Topo, em etapas |
| **Tooling / editor** | **Full editor (4)** | idem | Topo — é o diferencial |
| Profiling | GPU capture (3) | Telemetria | Para na 3 |
| Build / ship | Release + CI (2) | DLC/patching | Cert não se aplica |

## Cadeia de dependência

```
Memory ─┐
        ├─→ Reflection/types ─→ Asset handling ─→ Tooling/editor
Threading┘         │                  │
                   └──────────────────┴─→ serialização, undo, prefabs, hot reload

Binding model ──→ render (paralelo)
Profiling ──────→ dia 1 (paralelo)
Build/CI ───────→ dia 1 (paralelo)
```

**Reflection é o gargalo do projeto inteiro.**

---

### 1.1 Threading — col 3

**Construir:** job system com grafo de dependências. Pool de N-1 threads, fila por thread com
work-stealing, `Job` com dependentes e contador atômico. `parallel_for` com particionamento.

**Não construir:** fibers. Stack switching quebra Tracy, quebra gdb, e transforma crash em
investigação de horas.

**Regra:** nenhum sistema cria thread própria. Tudo pelo scheduler.

**Dagor:** `prog/engine/osApiWrappers/` — wrappers de thread por plataforma.

**Retrofit:** total. Paralelizar depois = auditar todo estado compartilhado.

---

### 1.2 Memory — col 3 + tracking

**Construir nesta ordem:**
1. **Frame arena** — bump allocator, reset por frame. Tudo transiente aqui.
2. **Pools tipados** — vida longa, tamanho fixo.
3. **Tracking desde já** — tag de subsistema por alocação, overlay ImGui com MB por sistema,
   allocs por frame, high-water mark.

**Não construir:** TLSF. Alocador de console, resolve fragmentação em memória fixa.

**Regra:** zero `new`/`malloc` no caminho de render por frame.

**Retrofit:** o *tracking* é o caro — tag em 200 call sites depois é trabalho mecânico puro.

---

### 1.3 Reflection / types — col 4 · **a linha mais importante**

**Decisão:** separar **registry** (estável) de **mecanismo de registro** (trocável).

```cpp
const TypeInfo* type = TypeRegistry::get<Transform>();
for (const FieldInfo& f : type->fields) { ... }
```

**Começar com macro, não codegen:**

```cpp
HARPIA_REFLECT(Transform,
    HARPIA_FIELD(position),
    HARPIA_FIELD(rotation),
    HARPIA_FIELD(scale))
```

Codegen via libclang (caminho do TucanoEngine, com `src/Generated`) adiciona libclang ao build,
um passo de geração e um parser pra manter. Macro custa uma linha por tipo e funciona em qualquer
compilador. C++26 static reflection (P2996) resolveria nativamente, mas o suporte em GCC/Clang
ainda é experimental — não apostar o projeto nisso.

**O que o registry entrega, e o que cada item destrava:**

| Capacidade | Destrava |
|---|---|
| Enumerar campos + tipo + offset | Inspector do editor |
| Ler/escrever campo por nome | Serialização, undo |
| **Versão do tipo + migração** | Save que não quebra ao editar componente |
| Construir tipo por nome | Prefabs, carregar cena |
| Metadados (range, tooltip, hidden) | Inspector que não parece debug |

**Versionamento não é opcional.** Todo tipo serializável carrega `version`.

**Dagor:** `prog/engine/ioSys/dataBlock/` — formato **DataBlock (.blk)**, config e serialização
de tudo, com `prog/tools/blkEditor`. `prog/engine/daECS/core/componentType.cpp` — registry de
tipos de componente em runtime.

**Superamos:** Dagor tem DataBlock (dados) e registro de componente ECS como sistemas separados.
Unificamos num `TypeRegistry` só, que serve serialização, inspector, undo e prefab da mesma fonte.

**Retrofit:** o maior do documento. Reflection tardia = reescrever serialização + editor + asset juntos.

---

### 1.4 Backend portability — col 2, **parada deliberada**

**Construir:** Vulkan 1.3 direto. Sem RHI, sem device virtual.

**Não construir:** abstração sobre backends. Única linha onde ficar à esquerda é vitória.

**Disciplina que substitui a abstração:** nenhuma chamada `vk*` fora de `Source/RHI/`. O resto do
engine fala `TextureHandle`, `BufferHandle`, `PassBuilder`. Porta aberta pra DX12 depois, sem pagar hoje.

**Dagor:** `prog/engine/drv/drv3d_vulkan/` — backend maduro, ler organização de device/filas/sync.

---

### 1.5 Binding model — col 3, dia 1

**Construir:** bindless no primeiro triângulo. Um descriptor set global, arrays grandes de
sampled image / storage buffer / sampler. Textura = `uint32`.

**Retrofit:** meses. A escada engana ao pintar bindless como "col 3 moderno", sugerindo destino.
É ponto de partida. Descriptor caching (col 2) é trabalho que se joga fora.

---

### 1.6 Input — *(ausente na escada)*

**Construir:** camada de ação abstrata (`"Jump"`, não `KEY_SPACE`), rebinding, gamepad, dead zones,
múltiplos devices, contextos (gameplay/UI/editor).

**Dagor:** `prog/gameLibs/daInput/` (4.6k).

**Retrofit:** médio. Ler tecla direto espalhado pelo código vira caçada depois.

---

### 1.7 Asset handling — col 4, em etapas

**Uma decisão da coluna 4 tem que estar na coluna 1:**

> **GUID desde o primeiro asset.** Identidade estável, independente de caminho no disco.
> Renomear ou mover arquivo não pode quebrar referência.

| Etapa | Entrega | Quando |
|---|---|---|
| 1. Load direto + GUID | glTF/PNG do disco, mapeados por GUID em `assets.db` | F1.b |
| 2. Import + cache | import roda uma vez, cache binário | boot > 5s |
| 3. Cook offline + packs | CLI gera pacotes, runtime só lê formato final | conteúdo real |
| 4. Streaming + hot reload | carga assíncrona pelo job system; salvar no Blender atualiza no editor | com o editor |

**Hot reload é produtividade, não runtime.** Decide se você itera em 2s ou 40s. Maior retorno
do documento para dev solo.

**Dagor:** `assetMgr` (8k, standalone, sem acoplamento), `prog/tools/AssetViewer/assetBuildCache.cpp`,
`vromfsPacker` (formato de pacote), `prog/tools/dag4blend` (**addon de Blender** — a ponte com DCC).

**Superamos:** o pipeline do Dagor é dirigido por `jam` e opaco de fora. CLI clara + watch mode.

---

### 1.8 Tooling / editor — col 4 · **o diferencial**

Sobe ao topo, mas **depois** de reflection e asset. Editor antes de reflection = inspector
hardcoded que se joga fora.

| Etapa | Entrega |
|---|---|
| 1. Inspector + gizmos | Painéis **auto-gerados** pelo `TypeRegistry`. ImGuizmo. Zero UI por tipo escrita à mão |
| 2. Cena + hierarquia | Árvore, seleção, parenting, save/load via reflection |
| 3. Undo/redo | Command stack sobre "set field by path" — reflection entrega quase de graça |
| 4. Prefabs | Instância + override. Depende de GUID + reflection |
| 5. **Play mode** | Rodar o jogo no viewport; Stop restaura estado |
| 6. Sequencer | Só quando houver conteúdo pra animar |

**Play mode é o marco que define "editor comercial"** — e é exatamente o que falta no `daEditorX`.
Maior abertura competitiva do projeto. A tentativa anterior (`IHarpiaPreview`, backends grid e dng)
já produziu conhecimento sobre isso: reler antes de reprojetar.

**Dagor:** `propPanel` (21.9k, já ImGui), `prog/tools/libTools/`, `blkEditor`.
Vendoriza `ImGuizmo` e `imgui-node-editor` — usar as mesmas.

---

### 1.9 Profiling & observability — col 3, dia 1

- **Tracy** na fase 0 — zonas CPU e GPU, allocs marcadas, frame markers
- **RenderDoc como prática:** todo pass nomeado com `VK_EXT_debug_utils`, todo recurso com nome
  legível. Capture com `Buffer_0x7f3a...` custa uma hora; com `CSM_Cascade2_Depth` custa um minuto
- **Frame budget explícito** — orçamento em ms por sistema, assert em debug quando estoura

**Não construir:** telemetria ao vivo. Infra de estúdio com jogadores reais.

---

### 1.10 Build / ship — col 2 + CI

- **CMake**, presets `linux-debug` / `linux-release` / `linux-relwithdebinfo`
- **CI desde a fase 0**, mesmo só compilando. CI no mês 18 acha 200 problemas de uma vez;
  no dia 1 acha um por vez
- CI roda os **golden image tests** — é o que dá dente à regra
- Packaging: binário + assets num tarball rodável em máquina limpa
- **crashpad** quando houver usuário externo

**Não construir:** cert/TRC (console), DLC, patching diferencial.

---

# PARTE II — Render

## F1 · Render graph + primeiro triângulo

O grafo **antes** do conteúdo.

- Declaração de passes, recursos transientes, **aliasing de memória**, barreiras automáticas,
  hints de async compute
- Compilação por frame com cache; pipeline cache em disco
- `HelloTriangle` rodando **através do grafo**

**Dagor:** `prog/gameLibs/render/daFrameGraph/`, `prog/gameLibs/render/resourceSlot/`
— o resource slot resolve "quem escreve em qual recurso quando passes são opcionais",
problema real que aparece na F5.

**Entregável:** triângulo desenhado por pass declarado, barreiras geradas.
Screenshot vira **golden image #1**.

> A partir daqui, todo pass entra com golden image. Sem exceção.

---

## F2 · Deferred PBR — *Materials col 3, Lighting col 3*

- GBuffer: albedo, normal (octahedral), roughness/metallic, **motion vectors**, depth
- Import glTF 2.0 (`cgltf`)
- BRDF GGX + Smith + Fresnel; difuso Burley
- IBL split-sum: BRDF LUT pré-integrada + prefiltered environment
- Luzes direcional + pontuais/spot com **clustered culling**
- Tonemap ACES + exposição

> **Motion vectors nascem aqui, não na F6.** Só são *usados* no TAA, mas se não nascerem com o
> GBuffer você reabre todo shader de geometria, skinning e vento depois. Dívida mais cara do roadmap.

**Dagor:** `render/deferredRenderer.cpp`, `deferredRT.cpp` (layout de GBuffer),
`preIntegratedGF.cpp` (LUT split-sum), `debugGbuffer.cpp` (visualização de canais — construir junto).

**Entregável:** Sponza com PBR completo. Golden image.

---

## F2.5 · Transparência — *col 2 → 3*

1. **Sorted alpha** — ordenação por profundidade, um pass forward após o deferred
2. **Weighted Blended OIT** — order-independent, barato, sem lista encadeada

Sem isso não existe vidro, água, folhagem nem partícula. Vem logo após o GBuffer.

**Dagor:** `render/semi_trans_render/` (em daNetGameLibs), `transparent_partition`.

---

## F3 · Sombras — *col 4 → 5*

1. **CSM 4 cascatas** — split log/uniforme misto, **estabilização por snap de texel**
   (sem shimmer ao mover câmera), depth bias + normal offset
2. **PCF → PCSS** — penumbra por distância do blocker
3. **Sombras estáticas toroidais** — cache de geometria estática em atlas toroidal que só
   atualiza a borda quando a câmera anda. Ganho enorme em mundo aberto

Luzes locais: atlas octaédrico (pontuais), tile de atlas (spots).

**Dagor — área mais forte dele, ler tudo:** `render/cascadeShadows.cpp` (CSM de produção,
estabilização inclusa), `xcsm.cpp`, `toroidalStaticShadows/`, `esmShadows.cpp`, `variance.cpp`,
`heightmapShadows/` (sombra de terreno por raymarch), `voxelShadows.cpp`, `bigLightsShadows.cpp`.

**Entregável:** CSM estabilizada, penumbra suave, zero shimmer. Golden image em 3 câmeras × 3 sóis.

---

## F4 · Céu e atmosfera — *col 3–4*

- **Bruneton**: LUTs de transmittance, multi-scattering, irradiance
- Sol dirigindo cor e intensidade da direcional, time-of-day
- Fog volumétrico froxel com god rays
- Nuvens volumétricas raymarch (meia-res + reprojeção temporal) — **opcional**, é caro

**Dagor:** `prog/gameLibs/daSkies2/` (8.3k, Bruneton completo e maduro),
`render/clouds.cpp`, `render/volumetricLights/`.

**Entregável:** ciclo dia/noite contínuo, sem pop, plausível ao nascer do sol.

---

## F5 · Iluminação global — *col 3 → 4*

Cada etapa entrega valor sozinha:

1. **SSAO** — barato, ganho imediato
2. **SSR** — reflexo de tela com fallback pra probe
3. **Probes de reflexão** — paraláxe corrigida, preenche o que o SSR perde
4. **DDGI** — grade de probes de irradiância por compute
5. **World SDF** — jump flooding na GPU; destrava occlusion, GI de longo alcance, soft shadows de área

**Dagor:** `prog/gameLibs/daGI2/` (5.9k, voxel + probes), `prog/gameLibs/daSDF/` (2.9k, jump flooding),
`render/ssao.cpp`, `screenSpaceReflections.cpp`, `esmAo.cpp`.

**Entregável:** sala fechada iluminada só pela janela, com bounce visível. Comparar com referência offline.

---

## F6 · AA, upscaling e post — *col 4–5*

- **TAA** com histórico, clamp de vizinhança, rejeição por motion vector
- **FSR 3.1** — prioridade sobre DLSS pelo alvo Linux/multi-vendor
- Resolução dinâmica atrelada a budget de frame
- Post: bloom, DOF, motion blur, lens flare, film grain, LUT de grade

**Dagor:** `render/temporalAA.cpp`, `smaa.cpp`, `fxaa.cpp`, `antialiasing/`,
`daNetGame/render/FSR.cpp`, `temporalSuperResolution.cpp`, `render/dynamicResolution.cpp`,
`render/upscale/`, `render/dof/`, `bloomCore/`, `objectMotionBlur/`, `genericLUT/`.

**Entregável:** câmera rápida sem shimmer, objeto animado sem ghosting.
**Teste em vídeo, não screenshot** — único marco que golden image não cobre.

---

## F6.5 · Partículas & VFX — *col 2 → 3*

1. Billboards em GPU, simulação em compute
2. Colisão contra depth buffer, ordenação
3. Lit particles integradas ao clustered
4. Editor de VFX por node graph — **`imgui-node-editor`, já vendorizado pelo Dagor**

**Dagor:** `prog/gameLibs/daFx/` (9.5k) — sistema de FX em GPU deles.

---

## F7 · Escala — *Draw submission col 4, Geometry col 3*

- **GPU-driven**: culling frustum + **Hi-Z occlusion** em compute, `DrawIndirect`
- **Meshlets** (`meshoptimizer`) com cluster culling
- **Texture streaming** com residência por mip e budget de VRAM
- Virtual texturing (terreno)

**Dagor:** `prog/gameLibs/rendInst/` (**36.8k**, o maior módulo de render — estudar antes de projetar),
`render/occlusion/`.

**Não fazer:** virtual geometry estilo Nanite. Custo altíssimo, e quebra com deformação.

---

## F8 · Ray tracing — *condicional*

Só depois de tudo estável. RADV suporta RT no Linux.

- BVH build + refit; RT shadows, RT AO, RT reflections; denoiser temporal + espacial

**Dagor:** `prog/gameLibs/bvh/`, `render/rtsm/`, `rtao/`, `rtr/`, `ptgi/`, `denoiser/`,
`omm/` (opacity micromaps), `daSWRT/` (RT por software).

---

# PARTE III — Mundo (terreno, água, vegetação)

> Nada disto está na escada da imagem. Tudo isto define se existe mundo aberto.

## F7.1 · Terreno

- **Heightmap comprimido** + clipmap de LOD contínuo (sem seams, sem pop)
- Virtual texturing com atlas de material e splat
- Query de altura na CPU para física e gameplay
- Erosão e ferramenta de escultura (no editor)

**Dagor:** `prog/gameLibs/landMesh/` (**14.7k**), `heightmap/` (4.5k), `terraform/` (1.6k),
`prog/engine/heightMapLand/compressedHeightmap.cpp`, `render/toroidalHeightmap.cpp`.

---

## F7.2 · Água

- Oceano por **FFT** (espectro Phillips/JONSWAP), cascatas de resolução
- Shore blending por profundidade, foam por jacobiano
- Reflexão: SSR + probe planar; refração por depth
- Ripples e wakes por interação de objeto

**Dagor:** `prog/gameLibs/fftWater/` (**13.5k**), `render/waterObjects.cpp`, `waterProjFx.cpp`,
`waterRipples/`, `waterFoamTrail.cpp`, `foam/`, `wakePs/`.

**Realidade:** 13.5k linhas só de água. É dos sistemas mais caros que existem. Planejar como fase própria.

---

## F7.3 · Vegetação

- Instanciação GPU com frustum + Hi-Z culling
- LOD → **impostor** (billboard octaédrico) na distância
- **Vento** amarrado a um campo global, compartilhado com tecido e partículas
- Scatter procedural por máscara de terreno
- Grama por clipmap, gerada na GPU

**Dagor:** `prog/gameLibs/rendInst/` (36.8k), `render/fast_grass.cpp`, `randomGrass.cpp`,
`fastGrassBaker/`, `render/wind/`, `treesAbove/`.

**Não superamos** — 36.8k linhas afinadas por uma década. Igualamos com meshlets, que é
mais moderno e menos testado.

---

## F7.4 · Decals

Projeção em GBuffer (deferred decals), atlas, fade por ângulo, decals de dano e queimadura.

**Dagor:** `render/decals/`, `burningDecals.cpp`, `clipmapDecals.cpp`, `decalMatrices/`, `tireTracks.cpp`.

---

# PARTE IV — Simulação & gameplay

## 4.1 World model (ECS) — col 4 · **pertence à FASE 0**

> A escada coloca ECS quase no fim. **Errado.** É decisão de dia 1 —
> migrar OOP tree → ECS é reescrever o jogo.

- **Archetype ECS**, SoA, chunks de 16KB
- Queries com máscara de bloom filter, queries multithread
- Eventos, templates de entidade em dados

**Dagor:** `prog/engine/daECS/` (**20.8k**) — `archetypes.cpp`, `componentType.cpp`,
`ecsQueryManager.h`. Desacoplado do framework de jogo, reaproveitável isolado.

**Superamos:** integrar o ECS ao `TypeRegistry` desde o início — no Dagor, registro de componente
e DataBlock são sistemas separados.

---

## 4.2 World streaming — col 3–4 (F7)

Células com pipeline de 3 estágios, grid shardeado, predição de movimento, **HLOD**, persistência de célula.

**Dagor:** sistema de células do `daNetGame` + `rendInst` HLOD.

---

## 4.3 Physics — col 2–3

**Não escrever solver.** O Dagor não escreveu: usa Bullet e **Jolt** atrás de `physCommon`,
e a camada `daPhys` tem só **813 linhas**.

**Usar Jolt direto**, sem abstração no início. Mais novo, mais rápido, multithread por design,
e é o caminho novo da própria Gaijin. Abstração só quando houver segundo backend real.

Escopo: rigid body, character controller, raycast, collision mesh. Ragdoll com animação.

**Dagor:** `prog/engine/phys/{physBullet,physJolt,physCommon,fastPhys}` (9.6k), `gameLibs/daPhys/` (813).

---

## 4.4 Animation — col 3–4

- Skeletal + blend trees, state machine
- **Compressão com ACL** — a mesma que o Dagor usa
- IK grounding (two-bone + FABRIK)
- Root motion; blend por camada e máscara de osso

**Não fazer:** motion matching (col 5). Precisa de volume enorme de dados de captura.

**Dagor:** `prog/engine/anim/` (11.9k), `animChar/` (1.8k), `gameLibs/anim/` (4.4k),
`animHelpers/`, `3rdPartyLibs/acl`.

---

## 4.5 AI — col 3

- **Navmesh com Recast/Detour** — a mesma do Dagor, não escrever geração de navmesh
- Behavior trees; avoidance local (RVO)
- LOD de percepção (agentes distantes pensam menos)

**Dagor:** `prog/gameLibs/pathFinder/` (7.9k, wrapper de `recastnavigation`).

---

## 4.6 Networking — col 1, **decisão de escopo**

Se o alvo for single-player, **fica na coluna 1** e isso não é dívida.
Se houver multiplayer: replicação cliente-servidor com autoridade no servidor, sobre **enet**.

**Não fazer:** rollback + lag compensation (col 4). É a coisa mais difícil da escada inteira.

**Dagor:** `prog/engine/daNet/` (4.7k), `3rdPartyLibs/enet`.

---

## 4.7 Audio — col 2–3

- **miniaudio** (MIT) — a mesma do Dagor. FMOD/Wwise só se houver necessidade real
- Mixer com buses, espacialização 3D, occlusion por raycast
- Reverb zones

**Dagor:** `prog/gameLibs/soundSystem/` (12.7k), `soundOcclusionGPU/`, `3rdPartyLibs/miniaudio`.

---

## 4.8 UI — col 2–3

**Duas UIs distintas, não confundir:**

| | Tecnologia | Uso |
|---|---|---|
| **Editor** | ImGui + ImGuizmo + imgui-node-editor | ferramentas, inspector, gizmos |
| **Jogo** | retained tree, data-bound, com layout e localização | HUD, menus |

**Dagor:** `prog/gameLibs/daRg/` (**37.5k** — framework reativo sobre Quirrel),
`prog/engine/imgui/`, `gameLibs/imguiInput/`.

**Não superamos** o daRg em capacidade. Ganhamos em simplicidade — 37.5k linhas de UI reativa
é escopo de time, não de solo.

---

## 4.9 Scripting & iteration — col 3

- **daScript** — repo autônomo BSD (`GaijinEntertainment/daScript`), não é include do Dagor.
  Rápido, tipado, feito pra gameplay. Melhor escolha disponível
- Alternativa mais simples: Lua/LuaJIT
- Hot reload de script desde o início — é metade do valor

**Não fazer:** visual scripting (col 4) antes de existir alguém não-programador no projeto.

**Dagor:** `prog/gameLibs/das/`, `dasModules/`, `quirrel/` (19.5k).

---

# PARTE V — Sequência de execução

| Bloco | Conteúdo | Marco verificável |
|---|---|---|
| **F0.a** | Memory (arena+pools+tracking), Threading (job system), CMake, **CI**, Tracy | testes passam no CI |
| **F0.b** | Vulkan device, **bindless heap**, VMA, validation em zero | tela limpa animada |
| **F0.c** | **Reflection: TypeRegistry + macro + serialização versionada** | struct salva, edita, carrega |
| **F0.d** | **ECS archetype** integrado ao TypeRegistry + Input | 10k entidades, query MT |
| **F1** | Render graph + triângulo | **golden image #1** |
| **F1.b** | Asset DB com GUID + load direto | glTF carregado por GUID |
| **F2** | Deferred PBR + motion vectors | Sponza |
| **F2.b** | Inspector auto-gerado + gizmos | editar material em runtime |
| **F2.5** | Transparência (sorted → WBOIT) | vidro e folhagem |
| **F3** | Sombras: CSM → PCSS → toroidal | zero shimmer |
| **F3.b** | Cena, hierarquia, **undo**, prefabs | editor utilizável |
| **F4** | Atmosfera Bruneton + fog volumétrico | ciclo dia/noite |
| **F4.b** | Física (Jolt) + Animação (ACL) | personagem andando |
| **F5** | GI: SSAO → SSR → probes → DDGI → SDF | bounce visível |
| **F5.b** | **Play mode** + hot reload de asset | **marco de "editor comercial"** |
| **F6** | TAA + FSR 3.1 + post stack | movimento estável (vídeo) |
| **F6.5** | Partículas GPU + node graph de VFX | |
| **F7** | GPU-driven, meshlets, texture streaming, world streaming | mundo grande a 60fps |
| **F7.1–7.4** | Terreno, água, vegetação, decals | cena externa completa |
| **F8** | Ray tracing (condicional) | |
| **—** | Audio, AI, UI de jogo, scripting | entram conforme o jogo-demo exigir |

## Calibração de tempo

F0 → F7 em tempo integral: **18–26 meses**. Solo em tempo parcial: **3–5 anos**.
Não é argumento contra — é para não medir progresso contra expectativa errada.

---

# PARTE VI — Regras invioláveis

1. **Toda fase termina em imagem verificável.** Golden image commitada, comparada por SSIM no CI.
2. **Validation layers em zero.** Não é meta, é condição de merge.
3. **Nada de `#include` do Dagor.** Referência se lê, não se linka.
4. **Trecho copiado leva cabeçalho de atribuição BSD.** Sem exceção.
5. **Nenhuma chamada `vk*` fora de `Source/RHI/`.**
6. **Bindless, motion vectors, GUID de asset e ECS não são features — são fundação.** Nascem em F0–F2.
7. **Não escrever o que já existe:** física, navmesh, áudio, compressão de animação,
   scripting, gizmos, upscaling. A lista de compras do Dagor é a melhor parte dele.
8. **Duas semanas sem imagem nova é o sinal de alerta** — não o volume de código escrito.
9. **Não pular pra frente.** Mesh shader antes de CSM estável é como o Dagor virou
   20 anos de legado: features empilhadas sobre base não verificada.
