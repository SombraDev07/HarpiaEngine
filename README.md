# HarpiaEngine

Game engine em C++20, alvo Linux + Vulkan 1.3.
Plano completo em [HARPIA-ROADMAP.md](HARPIA-ROADMAP.md).

## Estado

**F2 em andamento.** Meshes residentes na GPU via bindless; GBuffer é o próximo passo.

| Bloco | Status |
|---|---|
| **F0.a** Memory, Threading, CMake, CI, Tracy | ✅ |
| **F0.b** Vulkan device, bindless heap, VMA, offscreen | ✅ |
| **F0.c** Reflection: TypeRegistry + serialização versionada | ✅ |
| **F0.d** ECS archetype + Input | ✅ |
| **F1** Render graph + primeiro triângulo | ✅ |
| **F1.b** Asset DB com GUID + sidecars .meta | ✅ |
| **F2** Import glTF 2.0 | parcial ✅ |
| **F2** Mesh na GPU (bindless + VMA) | ✅ |
| F2 GBuffer + PBR + motion vectors | próximo |

## Build

```bash
cmake --preset=linux-debug
cmake --build --preset=linux-debug
ctest --preset=linux-debug
```

Presets: `linux-debug`, `linux-release`, `linux-relwithdebinfo` (Tracy ligado),
`ci` (warnings como erro), `asan`, `tsan`.

Dependências vêm por `FetchContent` — nada a instalar à mão.

### Sanitizers (Fedora)

Os presets `asan` e `tsan` precisam das runtimes do GCC:

```bash
sudo dnf install libasan libubsan libtsan
```

## O que existe

### `Source/Core/Memory`

- **`MemoryTracker`** — contadores por subsistema (`MemTag`), thread-safe.
  Bytes vivos, high-water mark, contagem de alloc/free.
- **`Arena`** — bump allocator de capacidade fixa. `Scope` RAII aninhável.
  Sem destrutores por design: só tipos trivialmente destrutíveis.
- **`Pool<T>`** — pool tipado com **handles geracionais**. Handle obsoleto
  resolve para `nullptr` em vez de virar use-after-free.

### `Source/Core/Threading`

- **`JobSystem`** — grafo de dependências com work stealing.
  `submit`, `submitAfter`, `wait`, `waitIdle`, `parallelFor`.
  `wait()` executa trabalho pendente enquanto espera, então esperar de dentro
  de um job não trava. Sem fibers, por decisão (roadmap 1.1).

### `Source/Core/Profiling`

- **`Profiler.h`** — macros Tracy. Com Tracy desligado compilam para nada.

## Convenções

- Headers incluídos como `<Core/...>`; a raiz de include é `Source/`.
- Todo alvo linka `harpia_warnings` (`-Wall -Wextra -Wconversion -Wsign-conversion`...).
- Nenhum subsistema cria thread própria — tudo passa pelo `JobSystem`.
- Zero `new`/`malloc` no caminho de render por frame.

## Testes

122 casos, 26.145 asserções, doctest. `Tests/`.

Duas suítes: `harpia_engine` (tolerância zero a leak) e `harpia_gpu`.
