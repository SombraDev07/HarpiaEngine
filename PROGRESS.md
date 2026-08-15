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

## Próximo

**F1.b — Asset DB com GUID.** A decisão da coluna 4 que precisa estar na coluna 1: identidade
estável independente de caminho no disco. Depois **F2** (deferred PBR + glTF + motion vectors).

## Protocolo de retomada

Ao reabrir a sessão, ler nesta ordem:
1. [CLAUDE.md](CLAUDE.md) — regras de trabalho no repositório
2. [HARPIA-ROADMAP.md](HARPIA-ROADMAP.md) — o plano
3. Este arquivo — onde parou
4. [ARCHITECTURE.md](ARCHITECTURE.md) — camadas e invariantes

Depois: `cmake --preset=linux-debug && cmake --build --preset=linux-debug && ctest --preset=linux-debug`
para confirmar que a base está verde antes de continuar.
