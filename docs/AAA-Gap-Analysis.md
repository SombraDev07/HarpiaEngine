# Isto é AAA? — revisão honesta e o caminho até lá

Escrito a 2026-09-10, depois de fechar a fase 5. Números medidos neste host
(RX 6700 / RADV, Vulkan 1.4.318), não estimados.

---

## 1. A resposta directa

**Não.** E não é por falta de qualidade no que existe — é porque o que existe é
um **renderer**, e um motor AAA é três coisas de que aqui só há uma:

1. um renderer,
2. **conteúdo a escala** (streaming, LOD, culling, um pipeline de assets),
3. **ferramentas** (editor, cozedura, profiling, iteração rápida).

O renderer está bom. Não há nada das outras duas.

Vale a pena dizer o que **está** acima da média, porque é real e não se deita
fora: a **disciplina de verificação**. Gates que saem com código ≠ 0 se a
validation abrir a boca, capturas por readback em vez de screenshots, testes que
lêem os `.spvasm` e comparam offset a offset, e A/B numérico antes de declarar
que uma feature funciona. Muito código de produção não tem isto. É a fundação
certa; o que falta é altura.

---

## 2. O que existe, em números

| | |
|---|---|
| Rust | **12 585** linhas |
| SPIR-V escrito à mão | **8 653** linhas em **54** shaders |
| Testes | 51 |
| Samples | 11, todos a `validation_errors=0` |
| Plataformas | 1 (Linux) · 1 driver (RADV) · 1 GPU |
| Draw calls na Sponza | 4 sítios, 103 primitivas, **sem culling** |
| Present mode | **FIFO fixo** (vsync) |
| Instrumentação de tempo | **nenhuma** |

Funcionalidades: Vulkan 1.3 com dynamic rendering, heap bindless de 8192,
deferred PBR com IBL split-sum, CSM 4 cascatas com PCSS, TAA, fog em froxels com
sombras volumétricas, atmosfera Hillaire, nuvens Nubis com reprojecção temporal,
água Gerstner com SSR, chuva com mapa de exposição.

### O número que não existia

Corri a Sponza 600 frames: **10.89 s**, ou seja ~16.4 ms/frame. Isso **não é** a
performance da engine — é o refresh do monitor, porque o present mode é `FIFO`
e não há forma de o desligar. As nuvens dão exactamente o mesmo número, o que
confirma que estou a medir o vsync e não o trabalho.

**A engine não sabe quanto custa.** Não há timestamp queries, não há tempo de
frame, não há contadores. Isto sozinho impede qualquer afirmação sobre AAA:
AAA é, no fundo, uma afirmação sobre *performance a escala*, e aqui não há nem
performance medida nem escala.

---

## 3. O que falta, por ordem de alavancagem

Ordenado por quanto desbloqueia, não por dificuldade.

### 3.1 O toolchain de shaders — o travão de tudo

**O problema.** 8 653 linhas de assembly SPIR-V escritas à mão, montadas por um
assembler em Python feito em casa. Ao longo deste projecto esse assembler já teve
**pelo menos oito opcodes errados** — `OpFOrdLessThanEqual` gravado como
`OpFUnordEqual` desligou *todas* as sombras da engine e ninguém deu por isso
durante fases inteiras. Cada shader novo custa horas e traz risco de uma classe
de bug que nenhuma outra engine tem.

Nenhum motor AAA escreve SPIR-V à mão. Isto não é rigor, é uma dívida.

**Descoberta desta revisão:** a premissa registada no `INDEX.md` — *«DXC não está
no PATH, logo escrevemos `.spvasm`»* — está **desactualizada**. Neste host:

```
glslang-tools  15.1.0-2~ubuntu0.24.04.2   (universe)   NÃO instalado
spirv-tools    2025.1~rc1-1~ubuntu0.24.04.2 (universe) NÃO instalado
```

Um `sudo apt install glslang-tools spirv-tools` dá `glslangValidator`, `glslc`,
`spirv-as`, `spirv-val` e `spirv-opt`. O `build.rs` já tenta `spirv-as` antes do
Python — passaria a usá-lo sozinho.

**O que fazer, por ordem:**

1. Instalar os dois pacotes. Custo: um comando. Ganho: `spirv-val` deixa de
   depender da camada de validação em runtime, e o assembler passa a ser
   fallback e não caminho principal.
2. Escrever os shaders novos em **GLSL** (compilado por `glslc`) ou em
   **[Slang](https://shader-slang.org/)**, que passou para governação da Khronos
   no final de 2024 e tem SPIR-V como alvo de primeira classe, com módulos e
   genéricos — feito precisamente para bases de shaders que crescem.
   *Cautela:* o crate `shader-slang` está na 0.1.0 (Jul 2025) e o docs.rs
   reporta build falhado; invocar o `slangc` pelo `build.rs` é mais seguro que
   as bindings.
3. **Não reescrever os 54 shaders de uma vez.** Migrar quando se toca, mantendo o
   assembler para o que já está verificado. Os testes `*_layout_matches_the_spvasm`
   dão a rede de segurança para migrar um de cada vez.

**Gate:** um shader não trivial escrito em GLSL/Slang, compilado no `build.rs`,
com o mesmo output em pixels que a versão `.spvasm` que substitui.

### 3.2 Não há como medir nada

Sem isto, «optimizar» é adivinhar.

**O que fazer:**
- Present mode configurável (`--vsync 0` → `IMMEDIATE`/`MAILBOX`) para poder
  correr sem trava.
- `VkQueryPool` com timestamps por pass. É o mínimo: saber que o fog custa X ms e
  as nuvens Y ms.
- **[tracy-client](https://crates.io/crates/tracy-client)** para CPU e zonas GPU.
  O Tracy é o que a indústria usa e integra-se em poucas linhas.
- Contadores por frame: draws, triângulos, bytes carregados.

**Gate:** `sponza --frames 600 --vsync 0 --stats` imprime ms de CPU, ms de GPU por
pass, draws e triângulos. Sem isto nada do resto é verificável.

### 3.3 Não há cena — tudo está escrito à mão em cada sample

Cada `frame()` constrói o mundo em código. Não há entidades, nem hierarquia de
transformações, nem componentes. É por isso que a Sponza tem 4 sítios de draw e
não 103: não há nada que os organize.

**O que fazer:** fechar a decisão do ECS (D28: `bevy_ecs` 0.19.1, 1.84 M
downloads recentes, actualizado em Ago 2026, **sem `wgpu` nem `winit` nas
dependências**; `hecs` 0.11.1 como plano B com 3 dependências contra 18) com o
gate de 1e6 instâncias que a D20 já exige — não há benchmark cross-ECS mantido
em Rust desde que o `ecs_bench_suite` foi arquivado em Nov 2022, portanto medir é
a única saída.

### 3.4 Não há pipeline de assets

O glTF é lido em runtime, a cada arranque. As texturas sobem como RGBA8 sem
compressão. Não há cozedura, não há streaming, não há budget de memória.

**O que fazer:**
- Formato cozido próprio (`serde` + `bincode`/`rkyv`), com as malhas já
  optimizadas por **[meshopt](https://crates.io/crates/meshopt)** 0.6.2
  (vertex cache, overdraw, vertex fetch, e é também quem gera meshlets).
- Texturas em **KTX2 + BC7/BC5** (ou Basis Universal). RGBA8 não compactado é
  4× a memória e 4× a banda.
- Streaming por distância com um budget explícito.

**Gate:** a Sponza cozida carrega em <200 ms e ocupa menos VRAM do que hoje.

### 3.5 Não há culling, nem LOD, nem GPU-driven

Desenha-se tudo, sempre. Numa cena de tamanho real isto colapsa.

**Caminho, na ordem em que rende:**
1. Frustum culling em CPU + ordenação por material.
2. **Draws indirectos** e culling em compute (o heap bindless que já existe é
   metade do trabalho).
3. **Meshlets + two-pass occlusion culling** com um visibility buffer. A
   comunidade estima que isto dá **60–70% do benefício do Nanite** sem a
   compressão de geometria — a [Bevy fez precisamente este caminho](https://jms55.github.io/posts/2024-06-09-virtual-geometry-bevy-0-14/)
   e é leitura directa (MIT/Apache).
4. Cluster LOD à Nanite só depois, se fizer falta.

**Gate:** 100 000 instâncias com culling, a medir, contra o mesmo sem culling.

### 3.6 O frame é single-thread por decisão (D0)

Foi a escolha certa para chegar aqui. Deixa de ser quando a cena crescer:
gravação de command buffers em paralelo é das poucas coisas que o Vulkan oferece
de graça e a arquitectura actual não aproveita.

### 3.7 O que não existe de todo

Animação e skinning · física · áudio · UI · editor · rede · save/load ·
localização · input remapeável · suporte a mais de uma plataforma.

Nada disto é opcional num motor AAA. É a maior parte do trabalho que falta, e é
por isso que a resposta à pergunta é «não».

---

## 4. Roadmap revisto

O roadmap actual salta da fase 5 (clima) para a 6 (terreno). Proponho **meter uma
fase antes**, porque é curta e desbloqueia todas as outras.

### Fase 5.5 — Ferramentas (nova, curta, faz-se antes de tudo)

- [ ] `apt install glslang-tools spirv-tools`; `build.rs` usa `spirv-as`/`spirv-val`.
- [ ] Um shader novo em GLSL ou Slang, com output idêntico ao `.spvasm` que substitui.
- [ ] Present mode configurável; `--stats` com timestamps por pass.
- [ ] `tracy-client` ligado.
- **Exit:** `sponza --frames 600 --vsync 0 --stats` dá números por pass.

### Fase 6 — Mundo (como já estava, mais o que ficou da 5)

- [ ] ECS decidido pelo gate de 1e6 instâncias (D28/D20).
- [ ] Terreno em clipmap, `heightquery`, vegetação, instâncias.
- [ ] **Céu Hillaire e nuvens entram aqui** — num interior não se viam (D32).
- [ ] Sombra das nuvens nos froxels do fog.
- [ ] Aerial perspective (froxel 32³), a dívida do D19.

### Fase 6.5 — Escala (nova)

- [ ] Pipeline de assets: cozedura, `meshopt`, KTX2/BC7.
- [ ] Frustum culling + draws indirectos + culling em compute.
- [ ] Meshlets + two-pass occlusion + visibility buffer.
- **Exit:** 100 k instâncias com números antes e depois.

### Fase 7 — GI + post (como estava)

Acrescentar: **upscaler temporal**. Em 2026 o campo é DLSS 4 (RTX 50), FSR 4
(RDNA 4, agora parte do «FSR Redstone») e XeSS. O TAA que já existe é o pré-
requisito — motion vectors e history já lá estão.

### Fase 8 — Editor

`egui` + `egui-ash-renderer` (o `egui-wgpu` está fora por D0).

### Fase 9 — Física, animação, áudio, rede

- **Física:** ver a correcção na secção 5.
- Animação/skinning, áudio (`kira`/`cpal`), rede (`renet`/`quinn`).

---

## 5. Correcção: o **Box3D existe**

Disse-te duas vezes que «`box3d` não existe, o Box2D é 2D». **Estava errado.**

O Erin Catto — autor do Box2D — lançou o **Box3D** em **Junho de 2026**, ou seja
depois do meu conhecimento. É C17 portátil, licença MIT, em
[github.com/erincatto/box3d](https://github.com/erincatto/box3d). Traz continuous
collision detection, um solver rígido «Soft Step», hulls convexos / cápsulas /
esferas / malhas / height fields, sensores, joints com limites e molas,
multithreading com SIMD e **determinismo cross-platform**. Nasceu de problemas
reais que ele teve com a física do Unreal num open-world autoritativo no
servidor.

Isso muda a decisão que estava adiada. As três opções reais:

| | Prós | Contras |
|---|---|---|
| **Box3D** | Determinismo cross-platform (crítico para rede), MIT, C17 simples de ligar por FFI, autor com 20 anos de historial | Muito novo (Jun 2026), sem bindings Rust prontas, traz toolchain C |
| **Jolt** | Maduro, usado no Horizon Forbidden West, rápido | C++, bindings por manter |
| **rapier3d** | Rust puro, sem toolchain externa, integra-se sem fricção | Mais lento em cenas grandes; foco 2026 do Dimforge é robótica e GPU |

**A minha recomendação:** `rapier3d` para arrancar, porque não traz toolchain
nenhuma e a física não é o gargalo agora; e reavaliar Box3D quando houver rede ou
replay, porque aí o determinismo cross-platform deixa de ser um luxo. Mas a
decisão é tua e agora tens os factos certos.

---

## 6. Sobre estudar o código de outras engines

Ofereceste-te para arranjar source de outras engines. É útil, **mas com cuidado**:

**Cuidado legal, a sério.** Ler fonte do Unreal e reproduzir estruturas no teu
motor é um risco real — a licença é restritiva e «li e reimplementei» não é uma
defesa confortável. O mesmo para qualquer motor proprietário com fonte sob NDA.

**O que vale mesmo a pena, e é seguro:**

- **Bevy** (MIT/Apache, Rust) — a implementação de meshlets/virtual geometry é a
  referência mais próxima do que precisas, na tua linguagem.
- **Godot** (MIT) — o renderer Vulkan é legível e mostra como se organiza um
  motor completo (cena, servidores, importadores).
- **niagara**, do Arseny Kapoulkine (autor do meshoptimizer) — série em vídeo a
  construir um renderer Vulkan GPU-driven do zero. É exactamente a fase 6.5.
- **Vulkan samples do Sascha Willems** (MIT) — já têm versões em Slang.
- **GPUOpen / AMD** — a série sobre mesh shaders e compressão de meshlets.

**O que não preciso:** o motor inteiro. O que rende é a técnica específica, não a
árvore de código. Se quiseres ajudar mais depressa, o mais valioso é escolheres
**uma** área da secção 3 e eu ir buscar as referências dela.

---

## 7. Se tivesse de escolher três coisas

1. **`apt install glslang-tools spirv-tools`** e parar de escrever SPIR-V à mão.
   Um comando remove a maior fonte de bugs e de lentidão deste projecto.
2. **Medir.** Present mode configurável e timestamps por pass. Sem números não há
   conversa sobre AAA.
3. **Decidir o ECS** com o gate que a D20 já exige, e passar a Sponza a usá-lo.
   É o que transforma isto de «samples» em «motor».

O resto — culling, LOD, assets, física, editor — é muito trabalho, mas é trabalho
conhecido. Estas três são as que estão a travar o resto.
