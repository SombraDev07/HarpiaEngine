# Dagor — terreno, vegetação e mundo (base para a fase 6)

2026-09-11. Lido no código da Dagor neste host, não é de memória. Complementa
`docs/Harpia-vs-Dagor.md`, que cobriu PBR, render e a parte do terreno que se vê no
`heightmapRenderer`; aqui entra o que faltava: **vegetação, colocação, grama e
mundo**, mais as duas peças de terreno que ficaram de fora (a grade de LOD e a
pirâmide de alturas).

**Método e licença.** A Dagor está em `DagorEngine/`, BSD-3-Clause da Gaijin, no
`.gitignore` — nunca entra no nosso repo. A licença deixa reutilizar com
atribuição, mas o caminho limpo é implementar **a partir da técnica**. Nada aqui é
copiado; o que interessa é o que o código faz e porquê.

---

## 1. O mapa

| Peça | Onde | Tamanho | O que é |
|---|---|---|---|
| Grade de LOD do terreno | `gameLibs/publicInclude/heightmap/lodGrid.h` | 77 l | anéis de patches + IB partilhado |
| Culling de altura | `.../heightmap/heightmapCulling.h` | 181 l | pirâmide **min/max** por LOD |
| Malha do terreno | `gameLibs/heightmap/heightmapRenderer.cpp` | 596 l | desenha sem VB (já sabíamos) |
| Texturação | `gameLibs/landMesh/virtualtexture.cpp` | 5 095 l | clipmap de virtual texture |
| Materiais do chão | `.../landMesh/landClass.h` | 104 l | land classes e detalhes |
| Vegetação e props | `gameLibs/rendInst/` | 37 212 l | instâncias, LODs, impostores |
| Visibilidade | `.../rendInst/visibility/extraVisibility.cpp` | 1 518 l | culling **em CPU**, com jobs |
| Desenho | `.../rendInst/render/extra/riExtraRendererT.h` | 1 053 l | merge de draws + multidraw |
| Impostores | `.../rendInst/render/impostor.cpp` | 1 291 l | último LOD, assado offline |
| Grama | `daNetGame/shaders/grass_generate_inc.dshl` | — | gerada **em compute** |
| Colocação | `gameLibs/gpuObjects/gpuObjects.cpp` | 1 174 l | objectos colocados na GPU |

---

## 2. Terreno — as duas peças que nos faltam

**A grade.** `LodGrid` são anéis concêntricos de patches; o patch tem 32×32
(`default_patch_bits = 5`) e **todos os patches partilham um index buffer**
(`LodGridVertexData`, com um IB de triângulos e outro de quads). A triangulação
alterna a diagonal por `(x + y) & mask` — um losango, não todos os triângulos na
mesma direcção, que é o que evita o padrão visível nas encostas.

Nós não temos IB nenhum e geramos o vértice do índice, o que é mais limpo e está
medido (0.077 ms). Isto não é para copiar; é para saber que a diagonal alternada
existe e porquê.

**A pirâmide de alturas — isto sim.** `HeightmapHeightCulling` guarda **min/max de
altura por LOD** (8 níveis, blocos de 128), com `getMinMax(lod, canto, tamanho)` e
uma variante interpolada, e soma o intervalo de deslocamento (displacement) ao
resultado. É assim que eles têm a caixa de um patch **sem avaliar o campo de
altura**.

É exactamente o ponto em aberto do nosso roadmap §7.3: hoje o `terrain_bounds.cs`
avalia FBM 81 vezes por patch, e a D48 já mediu isso a comer o que o culling
poupava. Com min/max em mips, decidir passa a uma leitura de textura.

**Texturação.** Land classes (`LandClassDetailTextures`): pilha de detalhes com
albedo, normal e reflectância, tile, ruído do weight map, deslocamento mín/máx,
poças, flowmap e ids de physmat. Por cima, virtual texture com feedback, biome
query e um atlas de pesos por célula. São 5 000 linhas — é a peça grande, e não é
para a fase 6 inteira.

---

## 3. Vegetação — o desenho é bom, o culling é a nossa abertura

**Dois sistemas.** `riGen` são as instâncias **pregeradas no nível**, em células
(`RendInstGenData::Cell`, com `cellSz`, `SUBCELL_DIV` e `grid2world`), com máscaras
hierárquicas de exclusão (`RoHugeHierBitMap2d<4,3>`) para apagar vegetação debaixo
de estradas e edifícios. `riExtra` são as dinâmicas, vindas do ECS, numa tiled
scene à parte.

**A visibilidade é um objecto, não um estado global.** `RiGenVisibility`, um por
vista (câmara, sombras, reflexos), preenchido por `prepareRIGenVisibility` com
frustum, oclusão, filtro externo, `forShadow` e — o detalhe que interessa —
`minSizeToDistRatio`, que corta pelo **tamanho aparente**, não pela distância. O
resultado são listas por LOD (`riexData[lod]`, `minSqDistances[lod]`).

**E é tudo em CPU.** Procurei `draw_indirect` e `dispatch` em `rendInst`,
`landMesh` e `heightmap`: **zero ocorrências nos três**. Aparecem só em
`gpuObjects` e na grama. O culling de vegetação deles é SIMD mais jobs
(`parallel_for`), o nosso é um dispatch (D47: 1 000 000 de instâncias com o mesmo
0.17 ms de CPU). Aqui estamos à frente, e agora está confirmado para vegetação e
não só para terreno.

**O desenho, esse, é para aprender.** Os registos vão para uma lista, são
**fundidos** quando partilham estado (`coalesce_drawcalls`), ordenados por
`drawOrder / pool / elem`, e saem por `MultidrawContext` em blocos de 2 048. O id
do draw viaja em `startInstanceLocation` — que é **o mesmo truque do nosso
`firstInstance`** (D56), encontrado por eles primeiro. Cada draw traz um `uint`
empacotado: 12 bits de offset de material e 20 de offset de matrizes
(`packedMultidrawParams.hlsli`).

**Impostores.** O último LOD é um impostor octaédrico **assado por uma ferramenta**
(`tools/sceneTools/impostorBaker`), com renderer e multidraw próprios e até atlas
de sombra de impostor. É o que nos falta para floresta a sério, e o roadmap já o
tinha anotado.

**Vento.** `applyWindAnimationOffset` desloca o vértice com peso vindo da cor do
vértice (rigidez) e um fade por distância, e — o ponto que não se pode perder —
escreve `prevWorldPos`, ou seja, **alimenta motion vectors**. Vegetação animada sem
motion vectors é borrão garantido no TAA, e nós temos TAA desde a fase 4.

---

## 4. Grama — gerada na GPU, e é o modelo a seguir

O compute lê a máscara do terreno e escreve instâncias num
`RWStructuredBuffer<GrassInstance>` com contador indirecto; os tipos de grama vivem
num cbuffer e os pesos vêm por canal da máscara (isto é, **por bioma**). Tem
intervalos de compilação para quads vs instancing e para **oclusão por Hi-Z**,
compressão dos atributos (`Half2ToUint`), apagar grama num raio (`erase_grass`,
para explosões) e passes separados de prepass, visibilidade e resolve.

Nós já temos a metade difícil: culling por oclusão Hi-Z do mesmo frame, com 86.2%
cortados e imagem idêntica (D55). O que falta é a **geração** — hoje a nossa
vegetação é colocada na CPU.

---

## 5. gpuObjects — povoar o mundo sem dados na CPU

Junta triângulos do terreno (até 65 536), soma as áreas e coloca objectos
**proporcionalmente à área**, restrito a biomas (até 32), gerando as matrizes na
GPU. Há ainda um colocador por volume. É o caminho para um mundo povoado sem uma
lista de instâncias em memória — e encaixa directamente no culling em compute que
já temos.

---

## 6. Mundo

As células vêm no dump binário do nível; o que faz streaming a sério são as
**texturas**, por procura. A vegetação tem um raio de interesse e um pool de
células (`initRIGen(cell_pool_sz, poi_radius)`). Não encontrei streaming assíncrono
de geometria por célula em `landMesh`/`rendInst` — o que não prova que não exista
noutra camada, prova que não está ali.

O frame é montado com nós declarativos (`opaqueStaticNodes`, `rendinstUpdateNode`,
`rendinstTransparentNode`, `ambientOcclusionNodes`, `downsampleDepth`…). Temos
grafo desde a D53, mas os nossos nós são escritos à mão em cada sample.

---

## 7. O que levar para a fase 6, por ordem

Cada ponto com o gate que o prova. Nada disto é "portar" — é implementar a técnica.

1. **Heightmap a sério + pirâmide min/max.** Substituir o FBM por dados, e os
   bounds dos patches passarem a uma leitura de mip em vez de 81 avaliações.
   *Gate:* `terrain` com heightmap real; as caixas do compute batem com as da CPU à
   décima de milímetro (o arnês do `heightquery` já faz isso para alturas).
2. **Instâncias em células, com LODs.** Já temos culling em compute a 1 M de
   instâncias; falta a célula, a distância por LOD e o corte por **tamanho
   aparente**, que é mais honesto que distância.
   *Gate:* `veg` com LODs, contagem por LOD estável e imagem idêntica ao controlo.
3. **Grama gerada em compute** a partir de máscara/bioma, ligada ao Hi-Z que já
   existe. *Gate:* densidade medida por área, e A/B em pixels contra a colocação
   em CPU.
4. **Vento com motion vectors.** Sem isto o TAA borra. *Gate:* diferença do TAA
   com e sem vento, medida, não olhada.
5. **Impostor como último LOD**, assado offline. *Gate:* triângulos por frame antes
   e depois, com a imagem a não mudar acima de um limiar medido.
6. **Land classes simples** (albedo/normal/reflectância + weight map) antes de
   pensar em virtual texture.

**O que não levar agora:** a virtual texture completa (5 000 linhas), o par
riGen/riExtra (dois sistemas para o mesmo, herança de dez anos), sweep masks,
terraform e destruição. E não copiar o culling em CPU: é a única área destas em que
já estamos claramente à frente.

---

## 8. Numa frase

Em vegetação eles ganham em **capacidade** — LODs, impostores, vento, dois sistemas
de instâncias, grama procedural — e nós ganhamos no **método**: o culling deles é
CPU do princípio ao fim, o nosso é um dispatch, e isso mede-se em 1 000 000 de
instâncias. A fase 6 é pegar na capacidade sem perder o método.
