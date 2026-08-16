# AGENTS.md — trabalho paralelo no HarpiaEngine

Vários agentes trabalham neste repositório ao mesmo tempo. Este arquivo existe para que
isso não vire conflito de merge nem, pior, dois donos para a mesma invariante.

**Leia antes de escrever qualquer linha:** [CLAUDE.md](CLAUDE.md) (regras de trabalho),
[ARCHITECTURE.md](ARCHITECTURE.md) (camadas e invariantes), [HARPIA-ROADMAP.md](HARPIA-ROADMAP.md)
(o plano), [PROGRESS.md](PROGRESS.md) (estado).

---

## Mapa de propriedade

Cada agente é dono exclusivo dos diretórios da sua linha. **Não edite arquivo de outro dono.**
Se precisa de algo que pertence a outro, pare e diga ao Bruno — não contorne.

| Agente | Escopo | Diretórios que possui | Roadmap |
|---|---|---|---|
| **R — Renderer** | Deferred PBR, IBL, sombras, post | `Source/RHI/`, `Shaders/`, `Samples/` | F2 → F6 |
| **A — Áudio** | miniaudio, mixer, espacialização | `Source/Audio/` | 4.7 |
| **I — Input** | camada de ação, rebinding, gamepad | `Source/Platform/Input/` | 1.6 |
| **P — Física** | Jolt: rigid body, character, raycast | `Source/Physics/` | 4.3 |
| **N — Navegação** | Recast/Detour, behavior trees | `Source/Navigation/` | 4.5 |

Testes: cada agente cria `Tests/test_<subsistema>.cpp` e **só** edita o próprio.

### Ainda não paralelizável

- **4.8 UI** — a UI de editor precisa de ImGui integrado ao RHI; colide com R.
- **4.4 Animation** — a parte de CPU (ACL, blend trees) é independente, mas skinning é shader.
  Só depois que R expuser vértices com pesos.
- **4.6 Networking** — o roadmap trata como decisão de escopo (col 1). Não despachar antes de
  o Bruno decidir se o alvo tem multiplayer; se for single-player, isso **não** é dívida.
- **F2.5, F3, F4, F5, F6, F6.5, F7** — tudo renderer. Um dono só.

---

## Os cinco pontos de estrangulamento

### 1. O par espelhado — só o agente R toca

```
Shaders/Common.hlsli   ←→   Source/RHI/RenderTypes.h
```

São structs espelhados byte a byte, ligados por `static_assert` de tamanho. Uma edição de um
lado sem o outro não falha no build de quem editou — falha como geometria no lugar errado ou
iluminação sutilmente errada. **Nenhum agente além de R altera esses dois arquivos.**

### 2. As dependências — append only

```
cmake/HarpiaDependencies.cmake
```

Toda dep nova entra aqui, num bloco `# --- <nome> ---` **no fim**, antes do bloco `doctest`.
Nunca reordene os blocos existentes: reordenar transforma um conflito trivial em um conflito
que exige entender o arquivo inteiro.

O `add_subdirectory(Source/<Seu>)` vai no `CMakeLists.txt` raiz, na lista de targets, também
no fim. Uma linha, nada mais.

### 3. O progresso — append only, seção própria

```
PROGRESS.md
```

Anexe a **sua** seção `### <Subsistema> ✅` antes da seção `## Próximo`. Nunca edite seção de
outro agente, nunca reescreva o histórico de ninguém.

### 4. O diretório de build — um por árvore

Os presets escrevem em `build/<preset>`, relativo à árvore. Com uma árvore por agente isso
já fica separado, e é a única razão pela qual fica: dois agentes compilando no mesmo
`build/ci` produzem erros internos de compilador em código que não tem defeito.

Nunca aponte um build para a árvore de outro agente.

### 5. A GPU — cuidado que já custou caro

Rodar teste de GPU nesta máquina pode travar a placa e derrubar a sessão gráfica inteira do
Bruno. Já aconteceu sete vezes em dois dias, uma delas resetando a máquina. Se o seu subsistema
não toca GPU, você não tem com o que se preocupar. Se toca, valide antes em software:

```bash
VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json ./build/ci/bin/harpia_tests -ts=gpu
```

E saiba disto: **as validation layers não pegam índice bindless fora de faixa.** Ele só existe
como valor de registrador em runtime. O invariante nº 7 tem esse ponto cego.

---

## Invariantes que valem para todos

Detalhados em [ARCHITECTURE.md](ARCHITECTURE.md). Os que mais pegam agente novo:

1. **Nenhum `vk*` fora de `Source/RHI/`.** Se seu subsistema precisa de GPU, ele pede a R.
2. **Nenhum subsistema cria thread.** Tudo pelo `JobSystem` (`Source/Core/Threading/JobSystem.h`).
   Uma thread solta é race condition esperando acontecer — e o preset `tsan` vai te pegar.
3. **Zero `new`/`malloc` em caminho por frame.** Transiente vai para `Arena`, vida longa para
   `Pool` (`Source/Core/Memory/`).
4. **Vida longa por `Handle<T>`, nunca ponteiro cru.**
5. **Toda alocação carrega `MemTag`.**

A dependência entre camadas é estritamente para baixo: `Core` não conhece `Platform`,
`Platform` não conhece `RHI`. Seu subsistema em `Source/<Seu>/` fica no nível de `Platform` —
pode usar `Core`, não pode usar `RHI`.

### O que reusar em vez de reescrever

| Precisa de | Use | Onde |
|---|---|---|
| Threads / paralelismo | `JobSystem` | `Source/Core/Threading/JobSystem.h` |
| Memória transiente | `Arena` | `Source/Core/Memory/Arena.h` |
| Memória de vida longa | `Pool` | `Source/Core/Memory/Pool.h` |
| Vetores, matrizes, quaternions | `Core/Math` | `Source/Core/Math/Math.h` |
| Entidades e componentes | `World` | `Source/Core/ECS/World.h` |
| Carregar arquivo por GUID | `AssetManager` | `Source/Core/Assets/AssetManager.h` |
| Reflexão de tipo | `TypeRegistry` | `Source/Core/Reflection/TypeRegistry.h` |

O `World` é totalmente templated: componentes se registram por tipo no ponto de uso, sem
arquivo central. Dois agentes podem adicionar componentes sem colidir.

---

## Protocolo de trabalho

### Uma árvore de trabalho por agente — não apenas uma branch

A primeira versão deste arquivo mandava cada agente criar uma branch. **Isso estava
errado e custou caro.** Branches num único diretório são mutuamente exclusivas, não
paralelas: quando um agente faz `git checkout`, os arquivos mudam debaixo de todos os
outros. Em uma hora isso produziu três incidentes:

- `internal compiler error: bus error` em arquivos que não tinham defeito nenhum — dois
  builds no mesmo `build/ci`, com arquivos mudando sob o `mmap` do compilador
- `ctest` com SEGFAULT que não reproduzia à mão — o binário sendo relinkado enquanto rodava
- trabalho não commitado desaparecendo do disco na troca de branch de outro agente

Nenhum desses sintomas aponta para a causa. É por isso que a regra existe.

Cada agente trabalha na **sua própria árvore**, criada a partir do repositório principal:

```bash
git worktree add ../harpia-<subsistema> -b agent/<subsistema>
```

Isso dá diretório e branch próprios compartilhando o mesmo `.git`, então os commits são
visíveis entre agentes sem que ninguém mexa nos arquivos de ninguém. O `build/` nasce
dentro da sua árvore, o que resolve a colisão de build pelo mesmo caminho.

**Nunca** rode `git checkout <outra-branch>` numa árvore que não é sua. Se precisa do
trabalho de outro agente, ele já está no `.git` — use `git log`, `git show`, ou faça merge
da branch dele na sua.

Se você começou a trabalhar na árvore principal por engano, não faça checkout para sair.
Pare, avise o Bruno, e mova os arquivos para a sua árvore antes de trocar qualquer coisa.

### Antes de dizer que terminou

Os quatro do [CLAUDE.md](CLAUDE.md), sem exceção:

```bash
cmake --build --preset=ci      # -Werror, tem que estar limpo
ctest --preset=ci              # verde
ctest --preset=asan            # se mexeu em memória
ctest --preset=tsan            # se mexeu em threads
```

Depois atualize a **sua** seção no `PROGRESS.md` e faça commit atômico.

### Quando travar

Se a tarefa exige tocar arquivo de outro dono, **pare e reporte**. Não contorne com cópia
local, não duplique o tipo, não edite "só um pouquinho". Um par espelhado com dois donos é
como se perde uma tarde de debug num sintoma que não parece ter nada a ver com a causa.
