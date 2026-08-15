# CLAUDE.md — regras de trabalho no HarpiaEngine

Engine C++20, Linux + Vulkan 1.3. Leia [HARPIA-ROADMAP.md](HARPIA-ROADMAP.md) para o plano,
[PROGRESS.md](PROGRESS.md) para o estado, [ARCHITECTURE.md](ARCHITECTURE.md) para invariantes.

## Build e teste

```bash
cmake --preset=linux-debug
cmake --build --preset=linux-debug
ctest --preset=linux-debug
```

Presets: `linux-debug`, `linux-release`, `linux-relwithdebinfo` (Tracy), `ci` (`-Werror`),
`asan`, `tsan`. Sempre rode `asan` e `tsan` depois de mexer em threading ou memória.

## Invariantes que não se negociam

Estão detalhados em [ARCHITECTURE.md](ARCHITECTURE.md). Em resumo:

1. Nenhum `vk*` fora de `Source/RHI/`
2. Nenhum subsistema cria thread — tudo pelo `JobSystem`
3. Zero `new`/`malloc` no caminho de render por frame
4. Vida longa por `Handle<T>`, nunca ponteiro cru
5. Toda alocação nossa carrega `MemTag`
6. Todo recurso Vulkan com nome de debug
7. **Validation layers em zero** — assertado em teste, é gate de merge
8. Barreiras com `synchronization2` e stage/access explícitos
9. Toda fase termina em imagem verificada numericamente

## Relação com o Dagor Engine

`/home/bruno/DagorEngine` é **referência de algoritmo, nunca dependência**.

- **Nunca** `#include` de lá. O C++ de render deles é inseparável de `d3d::`,
  do runtime de shaders (27k linhas) e da DSHL (1010 arquivos).
- O que se copia é **matemática de shader e desenho de pass**, não arquivo `.cpp`.
- Trecho derivado leva cabeçalho de atribuição BSD-3; `NOTICE.md` consolida.
- A melhor parte do Dagor é a **lista de dependências** — ACL, Recast, miniaudio, Jolt,
  enet, FSR, ImGuizmo. Todas são projetos autônomos, usáveis sem ele.

## Estilo

- Cabeçalho de arquivo explica **por que** o sistema existe, não o que ele faz.
- Comentário só onde a razão não é óbvia pelo código. Nada de comentário que repete a linha.
- `[[nodiscard]]` em tudo que retorna valor que importa.
- Ordem de include: header próprio, terceiros, std.
- Todo alvo linka `harpia_warnings`.

## Ao concluir um bloco

1. Compilar limpo em `ci` (`-Werror`)
2. `ctest` verde
3. `asan` e `tsan` verdes se mexeu em memória ou threads
4. Atualizar `PROGRESS.md`
5. Commit atômico

Não marque bloco como concluído sem os quatro primeiros.
