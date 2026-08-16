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

### F2 passo 2 — GBuffer ✅

| Target | Formato | Conteúdo |
|---|---|---|
| 0 | RGBA8 | albedo.rgb, occlusion |
| 1 | RG16 snorm | normal octaédrica |
| 2 | RG8 | roughness, metallic |
| 3 | RG16 float | motion vector (espaço UV) |
| depth | D32 | reverse-Z, limpo em 0 |

Formatos são resolvidos **contra o device** — o snorm preferido para normal não é
universalmente suportado como color attachment, então há fallback verificado.

**Motion vectors nasceram junto**, não na F6 com o TAA. São só *usados* depois, mas adicioná-los
depois significaria reabrir todo shader de geometria da engine.

Espelhos dos structs GPU vivem em `Shaders/Common.hlsli`, com `static_assert` de tamanho no lado
C++ — deriva de layout quebra o build, não o frame.

**Verificado canal a canal, por número:** albedo contra o fator de cor, normal decodificada de
volta à superfície dada, roughness/metallic nos canais e na ordem certa, motion contra uma
câmera que se moveu uma quantidade conhecida. 142 casos / 26.378 asserções ·
`-Werror`, ASan, UBSan e TSan limpos · 0 erros de validação.

**Bug encontrado pelo teste:** sem `-fvk-use-scalar-layout`, o HLSL alinha `float3` em struct a
16 bytes — `Vertex` tinha stride 64 na GPU contra 48 na CPU, e todo campo era lido do offset
errado. O device já habilitava `scalarBlockLayout`; faltava pedir ao DXC.

**Dois erros meus, não da engine:** o pass precisava de `neverCull` (nada no grafo lê o GBuffer
ainda, então o culling o removeu — corretamente), e eu havia afirmado o sinal do motion ao
contrário.

### F2 passo 3 — Lighting pass PBR ✅

| Entrega | Onde |
|---|---|
| `Brdf.hlsli` — GGX, Smith height-correlated, Schlick, Burley | `Shaders/` |
| `Fullscreen.vert.hlsl` — triângulo único, sem vertex buffer | `Shaders/` |
| `Lighting.frag.hlsl` — lê GBuffer, reconstrói posição do depth, escreve HDR | `Shaders/` |
| Samplers padrão (linear-repeat, point-clamp) no heap bindless | `VulkanRenderer` |
| `RenderGraph::imageOf/viewOf` — registrar target no bindless após o compile | `RenderGraph` |

Roughness é **perceptual** em todo lugar, com `alpha = roughness²`. Pular esse quadrado é o
motivo clássico de material parecer certo nos extremos e errado no meio.

GBuffer é lido com **point-clamp**, não linear. Não é escolha de qualidade: normal octaédrica
filtrada através de uma silhueta não é uma normal.

Escreve **HDR**. Tonemap é o passo 6 — gravar LDR aqui jogaria fora a faixa que exposição e
bloom precisam.

**Verificação:** o teste tem um **espelho da BRDF em C++**, escrito das mesmas fórmulas mas não
compartilhado com o shader — duas implementações independentes concordando é evidência, uma
concordando consigo mesma não é. A referência também quantiza roughness/metallic/albedo como o
GBuffer armazena, então compara contra o que foi de fato gravado.

145 casos / 26.401 asserções · `-Werror`, ASan, UBSan e TSan limpos · 0 erros de validação.

**O `neverCull` saiu do GBuffer:** agora o lighting lê os targets, então a dependência é real e
o grafo mantém os dois passes sozinho. Só o consumidor final precisa de pin — e na engine real
nem isso, porque o alvo final vem importado do swapchain.

### Sample Deferred — primeiro frame completo ✅

`Samples/Deferred` — **GBuffer → lighting → tonemap**, três passes declarados no grafo, 13
barreiras todas derivadas das declarações. Grade de 28 esferas: roughness no eixo X, metallic
no Y, uma luz direcional.

Malhas **procedurais** (`MeshPrimitives`), não asset importado — roda de um checkout limpo, sem
download, sem textura, sem pipeline de arte. É assim que PBR se valida a olho: qualquer erro na
BRDF, na codificação de normal, na convenção de profundidade ou no tonemap aparece como uma
linha que não progride suavemente.

Tonemap ACES (curva fitted) + encode sRGB entrou junto — sem ele HDR não vira imagem. O
swapchain é UNORM justamente para este pass ser o dono do encode; um alvo SRGB aplicaria a
curva duas vezes.

**Metal sai escuro com só um brilho concentrado, e isso está certo:** metal não tem lóbulo
difuso, então com uma direcional e sem ambiente não há mais nada para refletir. É exatamente o
que o IBL do passo 4 resolve.

**Bug encontrado pela imagem, não por teste unitário:** o winding das esferas estava invertido —
com backface culling ligado renderizávamos o *interior*. Sintoma era luz quase ausente, o que
parece bug de iluminação e não de topologia. Nenhum teste anterior pegaria: triângulo e testes
de GBuffer usavam `CULL_MODE_NONE`.

Agora fixado em teste: malha convexa fechada tem de produzir **imagem idêntica** com culling
ligado e desligado. Comparação do canal de normal, que é o mais estrito — face traseira carrega
a normal oposta.

146 casos / 26.432 asserções · `-Werror`, ASan, UBSan e TSan limpos · 0 erros de validação.

### Loader de textura ✅

| Entrega | Onde |
|---|---|
| `TextureAsset` — pixels RGBA8, sem Vulkan | `Source/Core/Assets/TextureAsset.h` |
| `TextureLoader` — stb_image, de arquivo e de memória | `Source/Core/Assets/TextureLoader.{h,cpp}` |
| `GpuTexture` — `VkImage`, cadeia de mips por blit, slot bindless | `Source/RHI/GpuTexture.{h,cpp}` |

**A decisão de sRGB fica no `GpuTexture`, não no importador.** Se uma textura é sRGB depende do
que ela *significa* — base color é sRGB, normal/roughness é linear, e o mesmo PNG pode ser as
duas coisas. O material sabe; o arquivo não. Errar isso deixa a iluminação consistentemente e
sutilmente errada — o tipo de erro que acaba sendo compensado na arte em vez de corrigido.

Tudo é expandido para RGBA8: imagem de três canais não tem formato de GPU universalmente
suportado, e preencher uma vez no load é melhor que ramificar em todo ponto de amostragem.

Mips saem de uma cadeia de blit com barreira por nível — cada nível transita entre
`TRANSFER_DST` e `TRANSFER_SRC` independentemente. A cadeia é pulada se o formato não filtra
linearmente: um nível honesto é melhor que cadeia quebrada.

`createSolid` dá um 1×1 para slot de material sem mapa — mais barato que ramificar no shader e
mantém todo material num caminho só.

O teste **escreve o próprio PNG** em vez de versionar um fixture: binário que ninguém lê é
fixture que ninguém mantém.

151 casos / 26.487 asserções · `-Werror`, ASan, UBSan e TSan limpos.

### IBL split-sum — a tabela BRDF 🟡

| Entrega | Onde |
|---|---|
| `BrdfLut.frag.hlsl` — integração GGX por importance sampling, 1024 amostras | `Shaders/BrdfLut.frag.hlsl` |
| `Ibl.hlsli` — `evaluateIbl`, céu analítico, irradiância em forma fechada | `Shaders/Ibl.hlsli` |
| `IblResources` — LUT R16G16 256², gerada uma vez, slot bindless | `Source/RHI/IblResources.{h,cpp}` |
| `GpuEnvironment` + `g_environments[]` | `Source/RHI/RenderTypes.h`, `Shaders/Common.hlsli` |

Falta o **environment prefiltrado de verdade**: o rádiance ainda vem de um céu analítico, não
de um cubemap com cadeia de mips. A matemática do split-sum e a LUT são idênticas nos dois
casos — trocar a fonte de radiância não mexe em mais nada — por isso o passo 4 fica amarelo e
não verde.

**O bug que custou caro.** O `Lighting.frag.hlsl` passou a ler dois índices bindless novos, mas
o `test_lighting.cpp` não foi atualizado junto: `environmentBuffer` ficou no default `0`, que
era um slot real segurando o vertex buffer do quad. O shader leu bytes de vértice como
`Environment`, tirou dali um `brdfLut` que era lixo e indexou `g_textures` com ele. Índice fora
do array de descritores é UB no Vulkan; na AMD o SQC falta no load do descritor, e o resultado
é `ring gfx_0.0.0 timeout`, reset do amdgpu e `gnome-shell` morto — sete vezes em dois dias, uma
delas levando a máquina inteira junto.

**As validation layers não pegam isso.** O teste rodava com `enableValidation = true`, checava
`validationErrorCount() == 0` e passava. Um índice bindless OOB só existe como valor de
registrador em runtime: só a GPU-Assisted Validation o enxerga. O invariante nº 7 tem esse ponto
cego, e ele continua aberto até a GPU-AV entrar nos presets de teste.

Por isso o clamp virou parte do contrato, não uma checagem avulsa: `harpiaValidTexture` /
`harpiaValidStorageBuffer` / `harpiaValidSampler` em `Common.hlsli` espelham
`VulkanBindless::kMax*`, e `create()` agora **falha** em vez de encolher o array quando o device
oferece menos — encolher deixaria o shader clampando contra um limite que não existe mais, que é
exatamente a falha que o esquema deveria excluir. `evaluateIbl` recebe o índice da LUT em vez de
um `Texture2D` para o bounds check morar junto do uso, e não em cada call site que pode esquecê-lo.

Teste de GPU novo ou alterado vale validar antes no rasterizador de software, onde errar custa
um teste vermelho em vez da sessão do usuário:

```bash
VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json ./build/ci/bin/harpia_tests -ts=gpu
```

152 casos / 27.277 asserções · `-Werror`, ASan e TSan limpos · 0 erros de validação.

### Navegação — Recast/Detour direto (4.5) ✅

Navmesh, crowd e behavior tree sem gerar navmesh à mão. Recast rasteriza a
superfície caminhável; Detour responde path e nearest; DetourCrowd faz o RVO.
Behavior trees são Sequence/Selector/Action/Condition/Inverter. LOD de percepção
é política de tick (Full / Reduced / Sleep), não um sensor.

Recast v1.6.0 entra no fim de `cmake/HarpiaDependencies.cmake`, antes do
doctest. Bake corre pelo `JobSystem` quando ele está de pé — nenhuma thread
própria. Alocações Recast/Detour passam por `MemTag::Scene`. Geometria vem de
`MeshAsset` em leitura; o arquivo do asset não é tocado.

Componentes `NavAgent` e `Perception` registram-se no `World` no ponto de uso.

Testes em `Tests/test_navigation.cpp` e no alvo CPU-only `harpia_navigation_tests`.
13 casos / 55 asserções · `-Werror`, ASan e TSan limpos.

```bash
cmake --build --preset=ci --target harpia_navigation_tests
./build/ci/bin/harpia_navigation_tests
```

## Próximo

**F2 — Deferred PBR**, continuando de:
2. ~~GBuffer no render graph~~ ✅ (ver abaixo)
3. ~~BRDF GGX + Smith + Fresnel, difuso Burley~~ ✅ (ver abaixo)
4. **IBL split-sum** — tabela BRDF ✅, environment prefiltrado pendente ← aqui
5. Luzes direcional + pontuais com clustered culling
6. ~~Tonemap ACES~~ ✅ (entrou junto do sample)

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
