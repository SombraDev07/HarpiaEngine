# Progresso — HarpiaEngine

Estado de execução do [HARPIA-ROADMAP.md](HARPIA-ROADMAP.md).
Atualizado ao fim de cada bloco.

## Concluído

### F0.a — Fundação: memória, threading, build, CI ✅

| Entrega | Onde |
|---|---|
| `MemoryTracker` — contadores por `MemTag`, thread-safe, high-water mark | `Source/Core/Memory/` |
| `Arena` — bump allocator de capacidade fixa, `Scope` RAII aninhável | `Source/Core/Memory/` |
| `Pool<T>` — pool tipado com handles geracionais | `Source/Core/Memory/` |
| `JobSystem` — grafo de dependências + work stealing, sem fibers | `Source/Core/Threading/` |
| `Profiler.h` — macros Tracy, no-op quando desligado | `Source/Core/Profiling/` |
| CMake + presets + CI GitHub Actions | raiz, `.github/workflows/` |

**Verificado:** 41 casos / 10.486 asserções · zero warnings com `-Wconversion -Wsign-conversion`
· preset `ci` com `-Werror` · **ASan, UBSan e TSan limpos** · 30 execuções seguidas sem flake
· paralelismo real confirmado (15 workers, 9.134 jobs roubados de 12.500).

**Bug encontrado pelos testes:** `Arena::allocate` alinhava o offset dentro do bloco em vez do
endereço absoluto — alinhamentos maiores que 64 bytes devolviam ponteiro desalinhado. Corrigido.

### F0.b — Vulkan device, bindless, VMA, swapchain ✅

| Entrega | Onde |
|---|---|
| `Window` — GLFW, sem nenhuma chamada Vulkan | `Source/Platform/` |
| `VulkanDevice` — instance, validation, surface, device 1.3, filas, VMA | `Source/RHI/Vulkan/` |
| `VulkanSwapchain` — formato, present mode, recreate | `Source/RHI/Vulkan/` |
| `VulkanBindless` — set global, UPDATE_AFTER_BIND + PARTIALLY_BOUND | `Source/RHI/Vulkan/` |
| `VulkanRenderer` — frames in flight, sync2, dynamic rendering, offscreen + readback | `Source/RHI/Vulkan/` |
| `HarpiaClearScreen` — sample, modo janela e headless | `Samples/ClearScreen/` |
| Golden image: `ImageCompare.h` + testes de render | `Tests/` |

**Verificado em AMD Radeon RX 6700 (RADV NAVI22):**
- Modo janela: 120 frames, **0 erros de validação**
- Modo headless: PNG capturado, pixel `[227,107,57,255]` **idêntico** ao cálculo independente
- Bindless: 16384 sampled images, 4096 storage buffers, 256 samplers
- 44 casos / 10.506 asserções passando

**Decisões tomadas nesta fase** (sem consulta, conforme autonomia concedida):
- Swapchain em `B8G8R8A8_UNORM`, não SRGB — o tonemap escreve sRGB final, um swapchain SRGB
  aplicaria a curva duas vezes.
- Semáforo `renderFinished` por **imagem de swapchain**, não por frame in flight — evita esperar
  em semáforo de imagem ainda em apresentação.
- Caminho offscreen construído **junto** da F0.b, não depois: é o que produz golden image e o
  que permite o CI rodar render sem GPU (lavapipe).
- `validationErrorCount()` exposto e assertado nos testes — transforma "validation em zero"
  de intenção em gate.

### F0.c — Reflection e serialização versionada ✅

| Entrega | Onde |
|---|---|
| `TypeInfo` / `FieldInfo` — campos, kinds, metadados de editor, hook de migração | `Source/Core/Reflection/` |
| `TypeRegistry` — busca por nome, registro eager via `AutoRegister` | `Source/Core/Reflection/` |
| `Reflect.h` — macros `HARPIA_REFLECT_BEGIN/FIELD/END` | `Source/Core/Reflection/` |
| `ByteStream` — writer/reader com length patchável | `Source/Core/Serialization/` |
| `Serializer` — formato name-keyed, versionado, com migração | `Source/Core/Serialization/` |

**Suporta:** escalares, enums, `std::string`, structs aninhados, `std::vector` de escalares
e de structs. Acesso a campo por ponteiro-para-membro como parâmetro de template — **sem
`offsetof`**, portanto sem UB em tipos non-standard-layout.

**Evolução de schema, testada de verdade:**
- Campo adicionado depois → ausente no dado antigo, mantém o default (`defaultedFields`)
- Campo removido depois → pulado via length prefix, sem corromper o resto (`skippedFields`)
- Semântica alterada → hook `HARPIA_MIGRATE` roda com a versão de origem
- Magic errado, truncado, vazio, tipo errado → rejeitados com status, nunca meio-aplicados

**Verificado:** 60 casos / 10.604 asserções · `-Werror` limpo · ASan, UBSan e TSan limpos.

**Bug encontrado pelos testes:** `readStruct` gravava `sourceVersion` em cada nível da
recursão, então um struct aninhado (`Point`, v1) sobrescrevia a versão do objeto raiz
(`SaveGame`, v2). Uma migração teria rodado com a versão errada. Corrigido passando o
out-param só na chamada raiz.

**Decisões tomadas:**
- Formato **name-keyed com length prefix por campo**, não ordinal compacto. Custa bytes e
  compra sobrevivência a refactor — a troca certa para uma engine dirigida por editor.
- `TypeRegistry` guarda `unique_ptr<TypeInfo>`: ponteiros estáveis enquanto o mapa cresce.
- Suítes de teste separadas (`harpia_engine` / `harpia_gpu`). O driver Vulkan vaza alocações
  JIT que o LeakSanitizer não consegue atribuir a módulo; em vez de afrouxar a detecção para
  tudo, o código da engine mantém tolerância zero e só a suíte de GPU roda com
  `detect_leaks=0`. Sem os testes de GPU, ASan reporta zero — então qualquer vazamento novo
  é nosso.

### F0.d — ECS archetype ✅ *(Input pendente)*

| Entrega | Onde |
|---|---|
| `Entity` — handle geracional; `ComponentRegistry` — ids densos por tipo | `Source/Core/ECS/Entity.h` |
| `World` — arquétipos, chunks de 16KB SoA, transições, queries | `Source/Core/ECS/World.{h,cpp}` |
| `TypeInfo::moveConstruct` / `copyConstruct` — exigidos pela transição | `Source/Core/Reflection/` |

**Entregável batido:** 10k entidades espalhadas em vários chunks, `parallelEach` visitando
cada uma exatamente uma vez.

**Verificado:** 70 casos / 21.844 asserções · `-Werror` limpo · ASan, UBSan e TSan limpos.

**Decisões:**
- **Componentes precisam ser refletidos.** É a integração que a auditoria do Dagor apontou:
  lá, registro de componente e DataBlock são sistemas separados, então editor e save cada um
  descreve o tipo do seu jeito. Aqui um `TypeRegistry` serve ECS, inspector e serialização.
- Remoção por **swap-remove**: storage fica sempre denso e a iteração nunca pula.
- Índices de query vêm de `index_sequence`, não de contador — a ordem de avaliação de pack
  expansion é indefinida e um cursor pareava array errado com componente errado.
- Ids de componente são **por processo, nunca serializados**. A chave estável é o nome do
  tipo (`ComponentRegistry::findByName`).

### Input ✅

| Entrega | Onde |
|---|---|
| `InputTypes.h` — Key, MouseButton, GamepadButton/Axis, `InputContext` | `Source/Platform/` |
| `Input` — ações por nome, bindings múltiplos, contextos, rebinding, edge detection | `Source/Platform/Input.{h,cpp}` |
| `Window::readInput` — coleta GLFW, deltas de mouse, gamepad | `Source/Platform/Window.cpp` |

**Decisão central:** `RawInputState` (o que os devices reportam) separado de `Input` (ações,
bindings, contextos). Isso torna toda a camada de ação **testável sem janela** e deixa a porta
aberta para replay de input gravado no editor.

Dead zone de analógico é **radial, não por componente** — por componente transforma um stick
circular em quadrado e corta a diagonal abaixo do alcance total. O teste fixa isso.

### F1 — Render graph + triângulo ✅

| Entrega | Onde |
|---|---|
| Pipeline HLSL → SPIR-V (DXC preferido, glslang fallback, build from source) | `cmake/HarpiaShaders.cmake` |
| `VulkanPipeline` — dynamic rendering, viewport/scissor dinâmicos | `Source/RHI/Vulkan/` |
| `RenderGraph` — passes declarados, culling, barreiras derivadas, aliasing de transientes | `Source/RHI/RenderGraph/` |
| `HarpiaTriangle` — o entregável da fase | `Samples/Triangle/` |

**Verificado em RADV NAVI22:** triângulo renderizado por pass declarado, **0 erros de validação**.
91 casos / 21.939 asserções · `-Werror`, ASan, UBSan e TSan limpos.

**Decisões:**
- **Nenhuma barreira escrita à mão, nunca mais.** Um pass declara "eu amostro isso" e o grafo
  deriva layout, stage e access de uma tabela única. Barreira manual é como um renderer acumula
  stall que ninguém acha depois.
- Aliasing é **nível de recurso**, não de memória: transientes com tempos de vida disjuntos
  compartilham o mesmo `VkImage`. Seguro sem malabarismo de bloco VMA, e já elimina as
  alocações que importam.
- O grafo dirige o dynamic rendering — um pass nunca escreve `VkRenderingInfo`.
- Shaders compilam **uma vez** num alvo global `harpia_shaders`. Dois alvos declarando regra
  para o mesmo `.spv` é erro de output duplicado, e um deles vencer em silêncio é pior.

**Dois bugs meus corrigidos aqui:** o path do compilador ficou cacheado como generator
expression antes do target glslang existir (quebrava na reconfiguração), e `HarpiaTriangle` e
`harpia_tests` geravam o mesmo `.spv`. O teste de aliasing também estava errado — pedia
compartilhamento onde os recursos coexistiam; o código estava certo.

### F1.b — Asset DB com GUID ✅

| Entrega | Onde |
|---|---|
| `AssetId` — GUID de 128 bits, texto de 32 hex | `Source/Core/Assets/AssetId.{h,cpp}` |
| `AssetDatabase` — scan, sidecar `.meta`, índice binário, detecção de move/missing | `Source/Core/Assets/AssetDatabase.{h,cpp}` |
| `AssetManager` — loaders por tipo, cache com mutex, `unloadUnused` | `Source/Core/Assets/AssetManager.{h,cpp}` |

**A garantia, testada:** renomear ou mover um arquivo **não quebra referência nenhuma**. Um
`AssetId` guardado numa cena continua carregando depois do arquivo mudar de nome e de pasta.

**Dois artefatos, papéis diferentes:**
- `<asset>.meta` — **texto**, porque vive no controle de versão ao lado do asset e precisa dar
  diff e merge como código. É a **autoridade** sobre identidade.
- `assets.db` — **binário**, cache de onde as coisas estão agora. Seguro apagar; um rescan
  reconstrói. Serializado pela nossa própria camada de reflexão, de propósito: se a evolução
  de schema não aguentar o índice de assets, não vai aguentar cena.

**Bug encontrado pelos testes:** `AssetId::toString` deslocava `56 - i*4` onde 16 dígitos hex de
metade de 64 bits exigem `60 - i*4`. A última iteração deslocava por valor **negativo — UB** — e
todo GUID escrito em sidecar saía corrompido. O round-trip texto expôs isso na primeira execução.

### F2 (parcial) — Import glTF 2.0 ✅

| Entrega | Onde |
|---|---|
| `MeshAsset` — vértices, índices, sub-meshes, materiais, bounds. **Sem Vulkan** | `Source/Core/Assets/MeshAsset.h` |
| `GltfLoader` — cgltf, hierarquia achatada em world space, texturas por GUID | `Source/Core/Assets/GltfLoader.{h,cpp}` |

Índices ficam **locais à primitiva** com `vertexOffset` por sub-mesh — que é o que
`vkCmdDrawIndexed` espera. Um caminho de draw só, sem reescrever índice no load.

Texturas de material resolvem para **GUID**, não caminho. É o retorno da F1.b: renomear a
textura não quebra o material.

**Bug encontrado pelos testes:** `recomputeBounds` indexava o array de vértices com índices
locais sem somar `vertexOffset`. Todo sub-mesh depois do primeiro reportava bounds dos vértices
errados — invisível até o frustum culling começar a descartar os objetos errados.

**Verificado:** 105 casos / 26.052 asserções · `-Werror`, ASan, UBSan e TSan limpos.

### F2 passo 1 — Mesh na GPU ✅

| Entrega | Onde |
|---|---|
| `VulkanBuffer` + `GpuUploader` — VMA, upload staged, `download` para verificação | `Source/RHI/Vulkan/VulkanBuffer.{h,cpp}` |
| `GpuMesh` — vertex storage buffer no heap bindless, index buffer, sub-meshes | `Source/RHI/GpuMesh.{h,cpp}` |
| `spirv-val` como gate de build | `cmake/HarpiaShaders.cmake` |

**Decisão central:** vértices vão para **storage buffer indexado por bindless**, não para vertex
buffer bindado. O vertex shader lê via `SV_VertexID`. É o que permite um pipeline só servir toda
mesh, e é o layout que a submissão GPU-driven da F7 vai exigir. Índices ficam em index buffer de
verdade — index fetch ainda é fixed function e ainda é o caminho mais rápido.

`vertexOffset` vai para o `vkCmdDrawIndexed` em vez de ser embutido nos índices — que é por que
o importador glTF os mantém locais à primitiva.

Upload é síncrono, um submit por chamada. Mesh e textura carregam em load time, não por frame,
então essa é a forma certa até streaming precisar de ring buffer e timeline na fila de transfer.

**Verificado:** 122 casos / 26.145 asserções · `-Werror`, ASan, UBSan e TSan limpos ·
0 erros de validação. Todo teste de upload faz round-trip por `download` — só readback prova
que o byte chegou em memória device-local.

### Ferramentas instaladas (2026-08-15)

`glslang` (configure caiu de ~2min para 6,8s), `spirv-tools`, `renderdoc` 1.45, `vulkan-tools`.
**DXC não existe no Fedora** — só importa em mesh shaders/wave intrinsics (F7); o CMake troca
sozinho quando aparecer no PATH.

### Math — fechada com glm ✅

**Corrigi uma decisão errada minha.** Eu tinha escrito matemática própria; o roadmap dizia
"biblioteca própria SIMD, **ou glm no início**", e a regra 7 diz para não escrever o que já
existe. Em todo o resto do projeto eu segui isso (VMA, cgltf, stb, GLFW, doctest, Tracy) — a
math foi a única exceção, sem motivo. Migrada para **glm 1.0.1**.

| Fica com o glm | Fica nosso |
|---|---|
| Vec/Mat/Quat, operadores, `inverse`, `transpose`, `lookAt`, `slerp`, `mix` | `perspectiveReverseZ` (far infinito + Y flip Vulkan) |
| `inverseTranspose` (via `normalMatrix`) | `orthographicReverseZ` (cascatas de sombra, F3) |
| | `encodeOctahedral` / `decodeOctahedral` |
| | `Transform`, `AABB`, `Plane`, `Frustum` (Gribb-Hartmann) |

Config do glm setada **uma vez, global**: `GLM_FORCE_DEPTH_ZERO_TO_ONE` (faixa de clip do
Vulkan) e `GLM_FORCE_CTOR_INIT` (vetor default zera em vez de vir com lixo). Uma TU que
incluísse glm sem isso discordaria em silêncio de todas as outras.

glm entra por um target `harpia_glm` com `SYSTEM` — os headers dele disparam
`-Wsign-conversion` e não são nossos para consertar. Mesmo padrão de stb e cgltf.

**Bug encontrado pelo teste:** a ortográfica reverse-Z saiu invertida — 0 no near e 1 no far.
Com `GREATER_OR_EQUAL` isso descartaria tudo. O teste de faixa de profundidade pegou.

**Verificado:** 121 casos / 26.219 asserções · `-Werror`, ASan, UBSan e TSan limpos.

### Toolchain — DXC desbloqueado (2026-08-15)

O frontend HLSL do **glslang não indexa array de buffer descriptor** — nem unbounded, nem
tamanho fixo, nem por cópia local. É exatamente o que o bindless exige, então o GBuffer estava
bloqueado. Eu tinha dito que DXC só importaria na F7; estava errado.

Vulkan SDK 1.4.357.1 instalado em `~/VulkanSDK`. **DXC 1.9 compila e valida o padrão bindless**.
O CMake trocou de compilador sozinho, sem alteração no projeto.

Para novas sessões:
```bash
export VULKAN_SDK=$HOME/VulkanSDK/1.4.357.1/x86_64
export PATH=$VULKAN_SDK/bin:$PATH
```

## Próximo

**F2 — Deferred PBR**, continuando de:
2. **GBuffer no render graph**: albedo, normal octaédrica, roughness/metallic, **motion vectors**, depth ← próximo
3. BRDF GGX + Smith + Fresnel, difuso Burley
4. IBL split-sum (BRDF LUT + prefiltered environment)
5. Luzes direcional + pontuais com clustered culling
6. Tonemap ACES

> **Motion vectors nascem no passo 2**, junto do GBuffer — não na F6 com o TAA. Se não
> nascerem agora, reabrir todo shader de geometria depois é a dívida mais cara do roadmap.

## Protocolo de retomada

Ao reabrir a sessão, ler nesta ordem:
1. [CLAUDE.md](CLAUDE.md) — regras de trabalho no repositório
2. [HARPIA-ROADMAP.md](HARPIA-ROADMAP.md) — o plano
3. Este arquivo — onde parou
4. [ARCHITECTURE.md](ARCHITECTURE.md) — camadas e invariantes

Depois: `cmake --preset=linux-debug && cmake --build --preset=linux-debug && ctest --preset=linux-debug`
para confirmar que a base está verde antes de continuar.
