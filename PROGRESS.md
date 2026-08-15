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

## Próximo

**F0.c — Reflection: TypeRegistry + macro + serialização versionada.**
Entregável: struct salva, edita, carrega. É o gargalo do projeto — editor, serialização,
save game, prefab, undo e hot reload dependem dela.

Depois: F0.d (ECS archetype + Input) → F1 (render graph + triângulo).

## Protocolo de retomada

Ao reabrir a sessão, ler nesta ordem:
1. [CLAUDE.md](CLAUDE.md) — regras de trabalho no repositório
2. [HARPIA-ROADMAP.md](HARPIA-ROADMAP.md) — o plano
3. Este arquivo — onde parou
4. [ARCHITECTURE.md](ARCHITECTURE.md) — camadas e invariantes

Depois: `cmake --preset=linux-debug && cmake --build --preset=linux-debug && ctest --preset=linux-debug`
para confirmar que a base está verde antes de continuar.
