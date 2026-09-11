# Harpia vs Dagor — análise técnica, só no que temos

2026-09-10. Comparação restrita a **terreno, render e PBR**, que é o que a Harpia
tem. Tudo abaixo foi lido no código da Dagor neste host, não é de memória.

**Método e ressalva.** A Dagor está em `DagorEngine/` (84 GB), BSD-3-Clause da
Gaijin. Meti-a no `.gitignore` — nunca entra no nosso repo. A licença permite ler
e reutilizar com atribuição, mas o caminho limpo é implementar **a partir da
técnica**, não transplantar código. Nada aqui foi copiado.

**Não conto linhas como qualidade.** Onde dou números de tamanho é para contexto,
e o caso mais instrutivo é este: a biblioteca de BRDF da Dagor são **285 linhas**.
A nossa é da mesma ordem. Volume não é a história — o que o código *faz* é.

---

## 1. O que temos, exactamente

| Área | Harpia |
|---|---|
| BRDF | GGX + Smith (Schlick-k) + Schlick, difuso Burley, IBL split-sum, energy compensation, clearcoat, sheen |
| Luzes | **uma** direccional |
| Sombras | CSM 4 cascatas, atlas 2×2, PCSS |
| GI | nenhuma (só IBL) |
| Terreno | clipmap de 7 anéis por `SV_VertexID`, altura procedural, cor por declive |
| HeightQuery | leitura bloqueante, grelha de 256², compara CPU contra GPU |
| Arquitectura | passes sequenciados à mão, single-thread, bindless 8192 |

---

## 2. PBR — é aqui que a luta é mais equilibrada

**O que a Dagor tem** (`gameLibs/render/shaders/{diffuse,specular,envi}_brdf.hlsl`):

- Difuso: Lambert, Burley, **Burley "fixed"** (Frostbite, conservativo), **Chan**
  (ajuste do Call of Duty, com retro-reflectividade).
- Distribuição: Blinn, Beckmann, GGX, **GGX anisotrópico**, **Cloth**.
- Geometria: Implicit, Neumann, Cook-Torrance, Kelemen, Schlick, Smith,
  **Smith height-correlated**, Cloth.
- Fresnel: Schlick, e um Fresnel completo.

**Onde estamos ao nível:** o núcleo. GGX + Smith + Schlick + Burley + IBL
split-sum + energy compensation + clearcoat + sheen é um BRDF moderno, não um
brinquedo. A conta de linhas confirma: 285 deles contra a mesma ordem de grandeza
nossa.

**Onde eles são melhores, concretamente:**

1. **Smith height-correlated.** Eles usam `0.5 / (V + L)` (`geometrySmithCorrelated`).
   Nós usamos a aproximação de Schlick com `k = (a+1)²/8`. A correlacionada é
   **estritamente mais exacta** — não é gosto, é a forma fechada correcta do termo
   de mascaramento-sombreamento. Isto é uma dívida nossa de meia dúzia de linhas.
2. **Anisotropia.** Não conseguimos exprimir metal escovado, cabelo ou vinil.
3. **Cloth.** NDF e termo geométrico próprios para tecido. Não temos.
4. **Difuso Chan.** Mais caro e mais exacto que o Burley em ângulos rasantes.

**Onde eles não são melhores:** o `optimized-ggx.hlsl` que lá está é do John
Hable, domínio público — a mesma fonte que qualquer um usaria. Não há magia
proprietária no núcleo do BRDF.

**A conclusão honesta, e não é sobre BRDF.** A nossa maior distância nesta área
**não é a qualidade do modelo — é que só temos uma luz.** A Dagor tem luzes
clustered (omni + spot), com sombras dinâmicas, prioridades e um orçamento de
4 actualizações de sombra por frame. Podíamos ter o melhor BRDF do mundo e
continuaríamos a renderizar cenas com um único sol. Nenhum detalhe de BRDF chega
perto dessa diferença.

---

## 3. Render — aqui eles ganham de forma decisiva

**Frame graph.** `daFrameGraph`, 161 ficheiros, com nós declarativos
(`frameGraphNodes/`: ambient occlusion, VRS, camera-in-camera, animchar assíncrono,
FX opacos e transparentes…). Nós sequenciamos passes à mão em cada sample, e as
barreiras são implícitas no RHI. A diferença prática: eles reordenam, fundem e
alocam transientes automaticamente; nós não podemos sem reescrever cada sample.

**Luzes clustered.** `clusteredLights.h` + `clusteredLightsGrid.h`: omni e spot,
máscaras por luz, sombras dinâmicas com prioridade (`MAX_SHADOW_PRIORITY = 15`) e
um orçamento explícito por frame. Nós: uma direccional.

**GI.** `daGI2` — cena voxelizada, cena de albedo, cena de media, **radiance
cache**. Nós: zero. IBL estático não é GI.

**Sombras.** Além do CSM: `toroidalStaticShadows` (atlas toroidal para estáticos,
que é exactamente o que a nossa barra de qualidade nos mandou *não* fazer na fase
4), `heightmapShadows`, `bigLightShadows`.

**Upscaling.** FSR2 e Snapdragon SSR integrados em `3rdPartyLibs`. Nós temos TAA,
que é o pré-requisito, e mais nada.

Não há como adoçar isto: em arquitectura de render estamos uma geração atrás.

---

## 4. Terreno — eles ganham em capacidade; nós numa coisa estreita

**O que descobri e me surpreendeu:** a Dagor **também não usa vertex buffer** no
terreno. `heightmapRenderer.cpp:156` faz `d3d::setvsrc_ex(0, NULL, 0, 0)`. A ideia
de gerar o vértice a partir do índice é a mesma nossa — não inventámos nada, e
eles chegaram lá primeiro.

**Onde eles vão muito além:**

| | Dagor | Harpia |
|---|---|---|
| Topologia | patches + **index buffer**, lotes instanciados | 7 anéis concêntricos, **sem IB** |
| Culling | **frustum por patch** (`LodGridRingCullData`) | nenhum |
| Tesselação | **HW tessellation no LOD0**, factor adaptativo | LOD fixo por anel |
| Fonte da altura | heightmap real + editor (`daEditorX/HeightmapLand`) | ruído procedural |
| Texturação | **virtual texture clipmap**, 5 095 linhas, com feedback | cor por declive |
| Detalhe | biomas, land classes, micro-detalhes, buracos, espelhamento | nenhum |
| Queries em CPU | `landRayTracerSoA4` — raytracer SIMD do terreno | avaliação directa da altura |
| Sombras do terreno | `heightmapShadows` dedicado | o CSM genérico |
| Robustez | recuperação de device reset no renderer | nada |

**Onde somos melhores, e é estreito:** a nossa malha não precisa de *nenhum*
buffer — nem vértices nem índices — e o clipmap inteiro sai num `draw` de 7
instâncias. A Dagor liga um IB e faz lotes de draws instanciados com parâmetros em
constantes de VS. É mais elegante do nosso lado e mede-se: **0.077 ms para 1024
unidades de alcance, 2 draws**. Mas é elegância, não capacidade — eles fazem tudo
o que nós fazemos e mais dez coisas.

---

## 5. HeightQuery — eles têm produção, nós temos uma coisa que eles não têm

**Deles** (`landMesh/heightmapQuery.cpp`): um `GpuReadbackQuerySystem` genérico
com IDs de query, máquina de estados (`IN_PROGRESS / SUCCEEDED / FAILED /
ID_NOT_FOUND`), **assíncrono e não-bloqueante**, thread-safe, com recuperação de
device reset. Até 256 queries por dispatch. Devolve normal e **três** distâncias
de impacto (sem offset, com offset, com offset e deformação).

**Nosso**: uma leitura que **espera pelo device** — e o RHI recusa-a a meio de um
frame, por isso corre no `finish`. Uma grelha, sem assíncrono, sem batching.

Em produção, o deles é melhor sem discussão.

**Mas fazemos uma coisa que eles, tanto quanto vi, não fazem:** o nosso gate
**compara a CPU contra a GPU** e falha se divergirem. Foi isso que apanhou o
`fract(sin(x)*43758)` a dar **77 m de erro máximo e 99.84% dos pontos fora da
tolerância** (D41). Na Dagor, a query devolve o resultado da GPU ao gameplay; se o
`landRayTracer` em CPU e o heightmap na GPU discordarem, nada avisa.

Não estou a dizer que eles têm esse bug — estou a dizer que **a classe de bug é
invisível sem um arnês como o nosso**, e não encontrei um. Procurei também testes
de conservação de energia (furnace) e não há.

---

## 6. Placar honesto

| Área | Quem ganha | Distância |
|---|---|---|
| Núcleo do BRDF | empate técnico | eles: Smith correlacionado, aniso, cloth |
| **Luzes** | **Dagor** | enorme — clustered vs uma direccional |
| Arquitectura de render | **Dagor** | frame graph, GI, VRS, FSR2 |
| Terreno (capacidade) | **Dagor** | VT, tesselação, culling, biomas, editor |
| Terreno (elegância da malha) | Harpia | estreito, mas real |
| HeightQuery (produção) | **Dagor** | assíncrono vs bloqueante |
| HeightQuery (correcção) | Harpia | temos arnês CPU-vs-GPU, eles não |
| Verificação | **Harpia** | gates com validation, A/B em pixels, layouts testados |

---

## 7. Roadmap para superar — nos pontos que importam

«Superar a Dagor» em bloco não é um objectivo a sério: são dez anos e um jogo AAA
a correr. Em **pontos específicos** é, e estes são escolhidos por serem
alcançáveis e por medirem algo.

### 7.1 PBR — três semanas de trabalho, e ficamos à frente em correcção

**Apanhar (barato, faz-se já):**

- [x] **Smith height-correlated** em vez do Schlick-k. **Feito** (D44): o furnace
      mediu 0.6922 contra 0.9986 a 0.07 de rugosidade — 31 pontos de energia
      perdida num quase-espelho.
- [~] **GGX anisotrópico** (Heitz 2014). O modelo está escrito e **medido** no
      `gate-furnace`, que ganhou dois painéis (D49): com `ax == ay` reduz-se ao
      isotrópico com desvio 0.0005, e a razão 4:1 baixa a perda média de 0.1706
      para 0.1139. Falta a tangente no GBuffer e o parâmetro no material.
- [ ] **Sheen/cloth** com NDF própria, não a aproximação que temos.

**Superar:**

- [x] **White furnace test como gate.** **Feito** (D44), `gate-furnace`. Falha com
      código ≠ 0 se a compensação não conservar energia; hoje desvia 0.0005.
- [x] **Invariante de troca de eixos** para a anisotropia (D49): trocar `ax` com
      `ay` e rodar a vista 90 graus tem de dar o mesmo albedo. É o que apanha um
      modelo anisotrópico partido — as duas verificações que escrevi antes desta
      passavam com o `Lambda` quebrado de propósito.
- [x] **Multiscatter GGX** validado pelo furnace: desvio máximo 0.0005 em 4096
      células. A compensação que existia está correcta — agora está provado.
- [x] Gate de **referência**: `gate-ibl` compara o split-sum com Monte Carlo de
      4096 amostras, em CPU e sem GPU (D50). **Apanhou um bug logo**: a LUT usava
      o `G` da luz directa e a rasar errava 51×. Erro médio 26.5% → **4.4%**,
      máximo 98.1% → **18.4%**. O erro vem decomposto em algorítmico e total, e a
      diferença (0.06 pontos) diz que o que resta é o método e não os dados.

### 7.2 Luzes — sem isto o resto é conversa

- [x] **Clustered lights**: 16×9×24, Z exponencial, storage buffers no set 3
      (D45/D46). **Feito** para omni **e spot** (D51): a mesma estrutura para os
      dois, e o teste de cone contra a esfera do cluster corta **52.9%** dos slots
      que a esfera envolvente pediria.
- [x] Gate `lights`: 1000 luzes, 334 delas projectores, com chão — **0.386 ms
      contra 2.220 ms, 5.75×**, e a imagem continua bit-idêntica à força-bruta
      (0 canais, 0 ULP). Era 3.1× só com omni: os projectores encarecem a
      força-bruta e não o clustered.
- [x] Sombras dinâmicas com **orçamento por frame** e prioridade (D52). Fila por
      prioridade, tecto em **texels** (não em mapas), e atlas com cache por versão.
      O invariante: numa cena parada, com orçamento de 512×512 **e** de 128×128, a
      imagem é idêntica ao pixel a não haver tecto nenhum — o orçamento é uma
      optimização, não outra resposta. Ao frame 300 a pass custa 0.000 ms.
      24 de 334 projectores têm sombra, porque 334 tiles de 512² são 87 M texels
      num atlas de 4 M: a fila serve os importantes e adia o resto.
- [ ] **Sombras de omni** (seis faces por luz). Só os projectores é que entram hoje.
- [ ] **Versões a sério**: hoje a cena está parada e a versão é constante. Falta
      ligá-la ao que se mexe, que é onde o orçamento passa a ter trabalho contínuo.
- [x] **Superar:** gate de correcção contra força-bruta. **Apanhou logo um bug**
      (caixa XY não conservadora, 1.68% dos canais errados); depois da correcção a
      imagem é **bit-idêntica**, 0 ULP. Era exactamente o tipo de erro invisível a
      olho de que falava este ponto.

### 7.3 Terreno — aqui podemos genuinamente passar à frente

A abertura é esta: **a Dagor faz o culling de patches em CPU** e depois monta
lotes de draws instanciados com parâmetros em constantes de VS. Um clipmap
totalmente GPU-driven não tem esse trabalho nenhum.

- [x] **Culling em compute → draw indirecto.** Feito e medido no gate `instances`
      (D47): 2 500 a 1 000 000 de instâncias com **o mesmo 0.17 ms de CPU**, contra
      0.17 → 2.72 ms a fazer o mesmo culling em CPU. Os dois modos dão o mesmo
      número de visíveis em todas as escalas. Falta aplicá-lo aos patches do
      terreno, que é onde a comparação com eles se fecha.
- [x] **Aplicado ao clipmap**: cada nível são 64 patches, o compute corta-os e o
      terreno inteiro sai de **um** `drawIndirect` (D48). 84.8% dos patches
      rejeitados, 172 032 vértices para 26 112, e a imagem prova-se contra um
      controlo `--no-cull`: 921 599 de 921 600 pixels idênticos.
      À primeira não acelerou nada — a caixa exacta em Y custava 81 avaliações de
      FBM por patch e o dispatch comia o que a pass poupava.
- [x] **As caixas deixaram de ser recalculadas.** Um patch só muda de região do
      mundo quando o snap do seu nível muda (1 unidade no nível 0, 64 no nível 6):
      2.2 níveis de 7 por frame. O culling caiu para 0.003 ms e a soma
      bounds+cull+terrain ficou 10% abaixo de não cortar nada (D48).
- [ ] **Bounds a partir de um heightmap a sério**, com min/max em mips, quando o
      terreno deixar de ser ruído procedural. Aí o `terrain_bounds.cs` passa a uma
      leitura de textura e o custo de decidir vai a zero.
- [ ] **Z-fighting na costura entre níveis**: 4 pixels mudam de dono conforme a
      ordem de desenho. Não se vê, mas é real (D48).
- [ ] **Tesselação por hardware no anel interior**, com factor por aresta a partir
      do erro de ecrã (não uma constante).
- [ ] **Virtual texture** para a texturação do terreno, com feedback buffer. É a
      peça grande — 5 000 linhas do lado deles — e é o que separa «terreno com cor
      por declive» de «terreno com materiais».
- [ ] Heightmap a sério (dados, não só ruído) com streaming por tiles.
- [ ] **Superar:** o gate `heightquery` estendido a **normais** e a **declives**,
      e corrido sobre o heightmap real. Se a CPU e a GPU concordarem à décima de
      milímetro em altura *e* normal, temos uma garantia que eles não publicam.

### 7.4 HeightQuery — apanhar, e manter o que temos

- [ ] **Anel de readback assíncrono**: N frames de latência, IDs de query, estados.
      Copiar a *forma* da solução deles, que é a certa.
- [ ] Batching: centenas de queries por dispatch.
- [ ] **Manter o arnês CPU-vs-GPU** a correr em CI. É a parte em que já estamos à
      frente e custa nada preservá-la.

### 7.5 Arquitectura — a dívida que sustenta tudo o resto

- [ ] **Render graph** com transientes, barreiras automáticas e reordenação. Sem
      isto, cada feature nova é sequenciada à mão e as barreiras são adivinhadas.
- [ ] **Gravação de command buffers em paralelo** (a D0 escolheu single-thread e
      foi a escolha certa até aqui; deixa de ser).
- [ ] GI: o `daGI2` deles é voxel + radiance cache. A nossa fase 7 já prevê
      «occupancy honesta ou 0 bytes» — manter essa honestidade.

---

## 8. O que eu diria se tivesse de resumir numa frase

O nosso BRDF aguenta a comparação; tudo o resto à volta dele não. A diferença que
mais custa não é técnica de shading, é que **temos uma luz e nenhum frame graph**.
E a única coisa em que estamos claramente à frente — verificar com números em vez
de acreditar na imagem — não se vê num screenshot, mas foi o que apanhou 77 metros
de erro que teriam passado por bug de física.
