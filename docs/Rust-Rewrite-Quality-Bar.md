# Barra de qualidade — reescrita Rust

**Para quem:** a próxima IA que implementa a engine em Rust.  
**Leitura obrigatória** junto com `docs/Rust-Rewrite-Roadmap.md`. Se os dois documentos discordarem neste ponto, **este ganha**: não clones a checklist; clona o 6.

Auditoria do renderer C++ (setembro 2026): **6/10**. É um deferred a sério, não um demo. Não é Filament / HDRP / Unreal. A nota sobe se contares *nomes de passes*; cai se medires *o que o pixel final mostra* e *estabilidade em movimento*.

---

## 1. Doutrina (uma frase)

**Não clones a checklist. Clona o 6 (PBR + IBL + fog + clima + bindless) e recusa o que puxa a nota para 4.**

Isto, sozinho, já supera a Tucano actual — não por ter mais passes, por ter **menos mentiras no pixel**.

Mentira no pixel = um sistema que:

- tem nome de paper (VSM, SSGI, VoxelGI) mas não faz o algoritmo,
- existe no graph / no CPU e **não é amostrado** no lighting,
- usa câmara / FOV / aspect **falsos**,
- escreve um campo no GBuffer que o lighting ignora,
- está default-off na cena de referência porque não aguenta default-on.

Se não podes defender o pixel num gate de 16 frames, **não existe**. Apaga ou não portes.

---

## 2. Nota do C++ (contexto, não objectivo)

| Área | Nota | O que isto significa para Rust |
|---|---|---|
| PBR / lighting directo | **7** | Clonar. Este é o osso. |
| Sombras | **5** | Clonar **só** CSM, mas **consertar o frustum**. Recusar o resto até CSM+TAA estarem honestos. |
| GI | **4** | Clonar IBL. Occupancy: **ligar ou apagar**. SSGI/DDGI só depois, e só se forem o algoritmo, não bleed de 8 vizinhos. |
| Post | **5.5** | Clonar bloom Karis / ACES / exposure / GTAO. **TAA não existe no C++ — tens de o inventar.** |
| Arquitectura | **6** | Clonar bindless + RHI trait. Não clonar o frame graph de lambdas. |
| Pronto para shipping | **4** | Gates verdes ≠ cena estável. Não portes GPUVM, dual terrain, OIT partido. |

Comparação rápida (não é meta de paridade):

- **Filament:** mesmo sabor de BRDF; ganha em materiais, energia, robustez.
- **HDRP:** a mesma lista, anos à frente em polish e TAA.
- **Unreal:** outro peso (Lumen, shading model completo).
- **Bevy:** materiais forward mais limpos; Tucano é mais *renderer*.

Sítio do C++: research/indie deferred ~2020. A reescrita **não** visa “mais 2020”. Visa um pixel honesto.

---

## 3. O 6 — clonar (DNA)

Estes sistemas **movem o pixel** e estão no sítio certo. Porta o *contrato*, não o C++ linha a linha.

| Peça | Contrato | Fonte |
|---|---|---|
| Deferred + GBuffer | 5 MRTs + D32, packing exacto | `GBuffer.hlsl`, roadmap §3 |
| PBR | GGX + Smith-Schlick + Schlick F + Burley; clearcoat GGX F0=0.04; fuzz Charlie | `Shaders/Common.hlsl` |
| IBL | Split-sum: irradiance + prefiltered GGX mips + BRDF LUT + `getEnergyCompensation` | `DeferredLighting.hlsl`, `GI/IBL.h` |
| Bindless | Índice inteiro no CB, heap 8192, **slot 0 = null** | `RHI.h`, `BindlessManager` |
| Fog | Froxels 160×90×64 aplicados **dentro** do lighting (`color * T + inScatter`) | `FogSystem`, `VolumetricFog.hlsl` |
| Clima | Rain 2 tempos (GBuffer wet + post); clouds half-res; water Gerstner | `Rain.hlsl`, `Clouds.hlsl`, `Water.hlsl` |
| Luz | 1 sol + lua sem sombra + ≤16 point/spot | `LightType.h`, `LightCB` |
| Clipmap | **Um**: Losasso/Hoppe, `SV_VertexID`, sem VB | `ClipmapTerrain.h` **não** `TerrainRenderer` |

Melhorias **dentro** do 6 (não são features novas):

- **Multi-scatter nas luzes directas** — o IBL tem `getEnergyCompensation`; o sol/points não. Metais rough ficam escuros vs Filament. Porta o BRDF *e* corrige isto.
- **`fuzzColor` no GBuffer** — hoje o material tem a cor, o lighting faz `lerp(albedo, white, 0.35)`. Reserva um canal ou packed RGB; não portes o campo morto.
- **Upload de mips IBL completo** — mips UNDEFINED + `SampleLevel` = GPUVM RADV.

---

## 4. O 4 — recusar (checklist que baixa a nota)

Não portes estes *como estão*. Ou reimplementas o algoritmo a sério **depois** do 6 estável, ou não existem no Rust.

### 4.1 CSM com câmara falsa — consertar, não copiar

`Renderer.cpp` `computeCascadeVP`:

```
aspect = 16/9          // ignora a janela
fov    = 60°           // ignora a câmara
radius = farD * 0.5    // esfera grosseira
texel  = 1/2048        // hardcoded no shader
```

PCF é caixa 3×3/5×5 com pesos iguais.

**Rust:** frustum da **câmara real** (FOV, aspect, near/far, view). Splits estáveis (PSSM ou practical splits). Snap de texels no espaço da luz (isto *é* o toroidal útil). PCF Poisson/Vogel, texel derivado do tamanho do atlas. Gate: sombra bate no contacto sol/chão quando mudas o aspect.

Sem isto, CSM é a feature que *parece* existir e projecta o volume errado.

### 4.2 “VSM” que não é VSM — não portes o nome

`VirtualShadowMaps` + `sampleVSM` em `DeferredLighting.hlsl`:

- grelha virtual de páginas de depth **R32**
- `sampleVSM` faz `shadowCompare` binário/ESM
- **não** há momentos (mean, mean²), **não** há Chebyshev

Chamar-lhe VSM é uma mentira no pixel. No Rust:

- **Não** crates `vsm`, **não** flags `enableVsm`.
- Se um dia precisares de shadows virtuais (Nanite-style pages), chama `VirtualShadowMap` e documenta que é paginação, não variance.
- Se um dia precisares de penumbra barata, implementa **momentos de verdade** (ou PCSS honesto) e chama Variance/MSM.
- Até lá: **CSM + PCSS**. Um path.

Toroidal atlas: podes fazer *depois*, como optimização do CSM verdadeiro. Não é um produto à parte.

### 4.3 Occupancy órfão — ligar ou apagar

`VoxelGI` actualiza um volume 32³ (`Renderer.cpp` `m_voxelGI.update`). **Nenhum** shader de lighting/compose amostra occupancy. `Phase3.hlsl` amostra WorldSDF SH (`sampleWorldSh`), não este volume.

Regra dura:

```
if occupancy.exists && !sampled_in_lighting:
    delete(occupancy)   // ou
    sample_it()         // no mesmo PR que o cria
```

Meio volume “para o DDGI holes” que ninguém lê é o padrão 4/10. Superar = uma linha no lighting (`ao *= occupancy` ou bounce barato) **ou** zero bytes GPU.

### 4.4 SSGI / DDGI lite — não clones o tick

- `PSSSGI`: 8 offsets no ecrã, bleed de vizinhos, raio 24 px. Não é GI de hemisfério.
- DDGI: atlas 8×8, 128². Existe; Sponza default `giTier=Off`.

Rust: IBL primeiro. Probes (≤4) no miss do SSR, se SSR existir. SSGI/DDGI **só** quando PBR+CSM+TAA+fog estão estáveis, e só com um gate que mostre *bounce* (cena de caixas coloridas), não um blur.

### 4.5 Contact shadows hack — opcional tardio

`ContactShadows.hlsl` desloca UV com `rayDir.xz * t * texel * 40` — não é uma projecção do raio do sol. Post-escurece o HDR. Podes viver sem isto se o CSM estiver certo. Se implementares: marcha no espaço de view com depth linear.

### 4.6 ESM — default off no C++, fica de fora

`exp(-k*(z-d))` fura geometria fina. Não é um produto. PCSS cobre penumbra.

### 4.7 Dois clipmaps — um só

Produção: `ClipmapTerrain` + `ClipmapTerrain.hlsl` (`SV_VertexID`).  
Legado: `TerrainRenderer` compute 8×129².

Rust: **um crate, um shader, um gate `terrain`**. O path CS não existe.

### 4.8 Sem TAA no C++ — obrigatório no Rust

Não há velocity buffer. SSGI/clouds reprojectam com `prevViewProj` ad-hoc. O HDR principal treme: SSR, GTAO, sombras, specular.

**Rust fase 3–4 (não fase 9):**

1. Motion vectors (object + camera): RT `RG16F` ou packed.
2. TAA depois do lighting / antes ou depois do tonemap (escolhe um e documenta): history + reprojecção + clamp neighbourhood (Karis/HDR).
3. Jitter Halton na projecção; Hi-Z e GTAO usam o jitter do frame.

Sem TAA, cada pass extra (SSR, SSGI, contact) **piora** a imagem em movimento. Por isso o C++ está em 5.5 de post com todos os nomes certos.

### 4.9 Outras mentiras / incompletos — não portes cegos

| Item | Mentira / buraco | Rust |
|---|---|---|
| Direct multi-scatter | Só IBL tem energy compensation | Corrige no BRDF do 6 |
| Anisotropy / transmission / SSS | Ausentes vs glTF/Filament | Fora do MVP |
| OIT | Vulkan-only | Ou todos os backends, ou cutout only |
| Arrays HLSL `RWStructuredBuffer[N]` | 1 binding no SPIR-V | Buffers soltos |
| `waitIdle` no frame | Feedback VT, alguns culling gates | Indirect; readback só em teste |

---

## 5. O que já supera a Tucano (ordem)

Não esperes pela fase 9. Estes quatro, no sítio, **já** batem o C++:

1. **CSM com frustum da câmara real** + PCF decente + texel do atlas.  
2. **TAA + motion vectors.**  
3. **Um clipmap** (`SV_VertexID`).  
4. **Occupancy no lighting, ou apagado.**  
5. **Nenhum sistema chamado VSM** até ser variance ou virtual-paged com o nome certo.

Depois, se sobrar tempo: probes honestos, SSR com Hi-Z, octa points, PCSS. Cada um com gate de *pixel*, não de “o pass correu”.

---

## 6. Teste de honestidade (ANTES de mergear um pass)

A próxima IA responde **por escrito** no PR / no commit message:

1. **Nome:** o algoritmo no shader é o do paper que o nome cita? Se não, muda o nome ou não merges.
2. **Amostra:** o lighting (ou o compose final) lê o recurso? Se o CPU escreve e o PS ignora → apaga.
3. **Câmara:** FOV/aspect/near/far vêm da `Camera` do frame, não de constantes 60°/16:9?
4. **Default-on:** a cena de referência (Sponza / pbr-grid) pode deixar isto ligado 90 frames sem GPUVM nem flicker insuportável?
5. **Gate:** `--frames 16` prova a *feature*, não só “0 erros de validation”.

Se algum ponto é “não”, o pass não entra no graph.

---

## 7. Impacto nas fases (`Rust-Rewrite-Roadmap.md`)

A ordem triângulo → bindless → deferred PBR **mantém-se**. O que muda:

| Fase | Ajuste vs roadmap original |
|---|---|
| 3 Deferred PBR | Packing GBuffer **com `fuzzColor` útil** ou sem o campo. Multi-scatter nas luzes directas. |
| 4 Sombras | **Só CSM honesto.** Motion vectors + TAA neste bloco (o C++ não tem). PCSS a seguir. Octa se houver points. **Sem VSM, sem ESM, sem contact** até CSM+TAA verdes. |
| 5 Clima | Igual (fog → clouds → water → rain). TAA já existe, senão rain/streaks treme. |
| 6 Terreno | Um clipmap. Sem `TerrainRenderer`. |
| 7 GI | Occupancy **no lighting neste PR** ou não criar o volume. SSGI/DDGI **não** são o exit da fase. IBL + probes + opcional SSR chega. |
| 8+ | Editor, mesh shaders, RT — depois do pixel honesto. |

---

## 8. Prompt extra (colar com o §18 do roadmap)

> Segue `docs/Rust-Rewrite-Quality-Bar.md`. Não clones a checklist do C++. Clona o 6: PBR, IBL, fog, clima, bindless. Recusa o 4: VSM que não é variance, occupancy órfão, segundo clipmap, CSM com FOV 60°/16:9 hardcoded, SSGI de 8 taps como se fosse GI. Obrigatório cedo: CSM com frustum da câmara real + TAA com motion vectors. Occupancy entra no lighting no mesmo PR em que o volume nasce, ou não nasce. Superar = menos mentiras no pixel, não mais passes.

---

## 9. Evidência (abrir se duvidares)

- CSM falso: `src/Renderer/Renderer.cpp` `computeCascadeVP` (~L154).
- VSM binário: `Shaders/DeferredLighting.hlsl` `sampleVSM` (~L128) — `shadowCompare`, sem momentos.
- Occupancy morto: `src/Renderer/GI/VoxelGI.*`; grep occupancy nos `.hlsl` de lighting = vazio. `Phase3.hlsl` `sampleWorldSh` é SDF, não VoxelGI.
- SSGI bleed: `Shaders/Phase3.hlsl` `PSSSGI` — `offsets[8]`.
- `fuzzColor` morto: `GBuffer.hlsl` não escreve RGB da sheen; `DeferredLighting.hlsl` `fuzzCol = lerp(albedo, 1, 0.35)`.
- Sem TAA: não há pass TAA nem RT de velocity no graph de `Renderer::render`.

---

*Se o C++ melhorar estes pontos, actualiza este ficheiro. A doutrina não muda: o pixel tem de ser honesto com o nome.*
