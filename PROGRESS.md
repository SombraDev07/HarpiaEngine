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

## Próximo

**Input** (`daInput` do Dagor como referência): camada de ação abstrata, rebinding, gamepad.
Depois **F1** (render graph + triângulo) → **F1.b** (asset DB com GUID) → **F2** (deferred PBR).

## Protocolo de retomada

Ao reabrir a sessão, ler nesta ordem:
1. [CLAUDE.md](CLAUDE.md) — regras de trabalho no repositório
2. [HARPIA-ROADMAP.md](HARPIA-ROADMAP.md) — o plano
3. Este arquivo — onde parou
4. [ARCHITECTURE.md](ARCHITECTURE.md) — camadas e invariantes

Depois: `cmake --preset=linux-debug && cmake --build --preset=linux-debug && ctest --preset=linux-debug`
para confirmar que a base está verde antes de continuar.
