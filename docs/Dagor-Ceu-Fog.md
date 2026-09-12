# Dagor — céu, nuvens e fog (base para fechar a fase 6)

2026-09-12. Lido no código da Dagor neste host. Terceiro documento da série:
`docs/Harpia-vs-Dagor.md` cobriu PBR, render e terreno; `docs/Dagor-Fase6.md`
cobriu vegetação, colocação e mundo; este cobre o que o `INDEX.md` ainda põe na
fase 6 e não está feito — **céu na cena aberta, nuvens, a sombra delas no fog, e a
perspectiva aérea** (a dívida da D19).

**Ressalva de método.** Li seams, APIs e estruturas: o raymarch das nuvens tem
1785 linhas e o shader de propagação do fog 1060, e não os li linha a linha. O que
está aqui é o que se confirma no código, não o que se supõe dele. A `DagorEngine/`
está no `.gitignore`, é BSD-3 da Gaijin, e nada aqui é copiado — implementa-se a
partir da técnica.

---

## 1. O mapa

| Peça | Onde | Tamanho |
|---|---|---|
| Céu + nuvens | `gameLibs/daSkies2/` | biblioteca inteira |
| Raymarch das nuvens | `.../shaders/clouds2/daClouds2.dshl` | 1 785 l |
| Orquestrador | `.../daSkies.cpp` | 1 430 l |
| LUTs de altitude | `.../shaders/skies2/prepareAltScattering.dshl` | 1 158 l |
| Aplicação das nuvens | `.../shaders/clouds2/daCloudsApply.dshl` | 705 l |
| Sombra das nuvens | `.../cloudsShadows.cpp` + `clouds_shadow.dshl` | 512 + 191 l |
| Scattering em **CPU** | `.../daScatteringCPU.cpp` | 481 l |
| Panorama do céu | `.../daSkiesPanorama.cpp` | 843 l |
| Fog volumétrico | `gameLibs/render/volumetricLights/` | — |
| Luz e propagação | `.../volfog_ff_light_calc_propagate.dshl` | 1 060 l |
| Aplicação do fog | `.../use_volfog.dshl` | 637 l |
| Fog distante | `.../volfog_df_*.dshl` | ~900 l |

Nota estrutural: **o fog não vive no `daSkies2`**. Céu e nuvens são uma
biblioteca; o fog volumétrico é outra. Falam-se por um seam de macros (secção 5).

---

## 2. Fog volumétrico — froxels, e um segundo sistema por cima

**A grelha.** `128×128×64`, e a qualidade alta não muda o algoritmo: dobra a
resolução (`calc_froxel_resolution` é literalmente `res * 2`). A nossa é 160×90×64,
portanto estamos na mesma família e na mesma ordem de grandeza.

**A ordem das passes**, pelos próprios scopes de profiler:

1. `volfog_ff_occlusion` — oclusão por froxel;
2. `volfog_ff_fill_media` — escreve o meio (`volfog_ff_initial_media`) a partir de
   `get_media(worldPos, …)`, com jitter por frame e o **heightmap incluído**: o
   terreno participa na densidade;
3. `volfog_shadow` — a sombra própria do fog;
4. `volfog_ff_light_calc_propagate` — ilumina e integra.

**O que ilumina o fog é a pilha inteira.** O shader de propagação inclui `csm`,
`esm`, `static_shadow`, `fom_shadows`, `skies_shadows`, `depth_above`,
`clustered/lights_cb` e `use_gi`. Não é «o sol mais um termo ambiente» — é o mesmo
lighting da cena, amostrado por froxel.

**E a sombra das nuvens entra aqui**, que é o item que o nosso `INDEX.md` põe na
fase 6:

```
shadow *= getFOMShadow(jittered_world_pos);
half cloudExtinction = base_clouds_transmittance(jittered_world_pos);
shadow_for_scattering = shadow * cloudExtinction;
shadow *= clouds_shadow_from_extinction(cloudExtinction);
```

A subtileza que vale copiar são os **dois termos**: o que multiplica o scattering
usa a extinção directa; o que multiplica o resto passa por uma função à parte. Um
só termo para as duas coisas dá nuvens que ou escurecem de mais o ar ou de menos o
chão.

**O segundo sistema.** Além dos froxels há *distant fog*: um raymarch a baixa
resolução (`volfog_df_raymarch`), com geração de mips de peso, reconstrução e
**reprojecção temporal** (`distant_fog_reprojection_dist`), válido até 5 km e
misturado nas últimas fatias do volume de froxels (`volfog_blended_slice_cnt`).
É a resposta deles ao alcance que um volume de froxels não cobre.

---

## 3. Nuvens — um campo cozido, não ruído avaliado por frame

**O campo é uma textura 3D comprimida** (`TEXFMT_ATI1N`, BC4), gerada por compute
e regenerada quando o tempo meteorológico muda — não é FBM avaliado no raymarch.
A resolução escala com a qualidade: `xz = clamp(256 * quality, 128, 2048)` e
`y = clamp(32 * quality, 32, 192)`. Há ainda uma versão **downsampled** do campo
(`downsampleCloudsField.dshl`) para o traço barato.

Isto é o mesmo movimento que acabámos de fazer no terreno: **cozer o campo e ler**
em vez de avaliar ruído em cada amostra.

**O traço é temporal**, com variante *checkerboard* opcional
(`setCloudsCheckerboardTrace`), e os comentários do `cloudsRenderer.cpp` são sobre
ghosting: o clamp do histórico colapsa com a taxa de paralaxe, e a advecção da
erosão move o conteúdo relativamente ao histórico. Quem já pagou TAA reconhece o
problema.

**Sombra das nuvens em três sabores:** transmitância volumétrica a sério (estrutura
3D dentro da camada, válida dentro do raio da cascata), uma BSM, e uma variante
*statistical*. É daqui que sai o `base_clouds_transmittance` que o fog consome.

**O resto é produto, não técnica de render:** buraco nas nuvens por posição
(gameplay), camada *strata* separada, textura de tempo externa, e origem das nuvens
em **double** — escala planetária.

---

## 4. Céu — Bruneton, com três coisas que não temos

**LUTs precomputadas** por compute: transmitância, irradiância e multiple
scattering (`indirect_irradiance_ms_cs`). A nossa atmosfera é Hillaire, que é desta
linhagem; o núcleo não nos falta.

**O que nos falta, e é concreto:**

1. **Perspectiva aérea** — existe, e chama-se `skies_frustum_scattering`: um volume
   de froxels integrado por `skies_integrate_froxel_scattering_cs`, com
   `skies_froxels_resolution` e `skies_froxels_dist` como globais, **duplo** para
   reprojecção (`prev_skies_frustum_scattering`), e escalado por qualidade. É
   exactamente a dívida da nossa D19, com outro nome.
2. **Panorama em cache** — o céu é desenhado para um panorama comprimido e reusado
   (`daSkiesPanorama.cpp`, `panoramaCompressor.h`, `applyPanorama.dshl`). É uma
   optimização, não uma feature: só entra com medição.
3. **Um scattering em CPU** (`daScatteringCPU.cpp`) que inclui os **mesmos
   `.hlsli`** que os shaders (`atmosphere_params.hlsli`, `texture_sizes.hlsli`).
   Uma definição, dois consumidores. É a resposta deles ao problema que a nossa D41
   mediu em 77 m de erro: não se escreve a fórmula duas vezes.

---

## 5. O seam — como os três se compõem

O céu **não conhece** o fog. Pede-o por uma macro que o projecto define:

```
macro NO_CUSTOM_FOG(code)
  #define apply_sky_custom_fog(a,b,c)
  #define get_volumetric_light_sky(a,b) float4(0,0,0,1)
endmacro
define_macro_if_not_defined CUSTOM_FOG_SKY(code)
  NO_CUSTOM_FOG(code)
```

Por omissão não há fog e o céu compila na mesma. Quem tiver fog define a macro e o
céu passa a integrá-lo. É a razão de o `daSkies2` ser uma biblioteca reutilizável e
de o fog poder ser outra — e é o oposto do que fazemos hoje, em que cada sample
sequencia as suas passes à mão.

---

## 6. O que levar para fechar a fase 6, por ordem

Cada ponto com o gate que o prova. Temos as peças todas; o que falta é compô-las.

1. **Sombra das nuvens nos froxels do fog.** Temos nuvens com transmitância e temos
   froxels. É uma multiplicação — com **dois** termos, um para o scattering e outro
   para o resto. *Gate:* a mesma cena com e sem, medida em pixels e com o custo da
   pass; a sombra tem de se ver no ar, não só no chão.
2. **Perspectiva aérea** (D19): volume pequeno de froxels integrado a partir das
   LUTs, com histórico. *Gate:* montanha distante com e sem, e o erro contra uma
   integração de referência por marcha em CPU — que é o arnês que já sabemos fazer.
3. **Céu e nuvens na cena aberta.** O `gate-terrain` é agora exterior: hoje desenha
   um gradiente. *Gate:* atmosfera e nuvens compostas com o terreno, `--capture`
   comparado contra o gradiente, e o custo por pass.
4. **O seam.** Fog e céu não se conhecem: um contrato de função, com um stub que
   devolve «sem fog». *Gate:* o `gate-sky` continua a correr sem fog nenhum.
5. **Campo de nuvens cozido e comprimido**, se o traço pesar. Mesmo movimento do
   terreno. *Medir primeiro.*

**O que não levar agora:** o fog distante (segundo sistema — só depois de o de
froxels estar honesto), o panorama (optimização sem medição), o buraco nas nuvens e
a camada strata (produto), e as variantes node-based de todos os shaders (é a
ferramenta de autoria deles, não técnica de render).

---

## 7. Onde estamos, honestamente

Temos as três peças e não temos a composição: fog em froxels 160×90×64 com sombras
volumétricas, nuvens Nubis com reprojecção, atmosfera Hillaire — cada uma provada
no seu gate, nenhuma a ver com as outras numa cena aberta. A Dagor tem exactamente
as mesmas três peças e tem-nas **ligadas**, com um seam explícito e com a sombra
das nuvens a atravessar o fog.

A distância aqui não é de algoritmo. É de integração.
