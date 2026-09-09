# Tucano → Rust — Roadmap técnico para replicar ou superar

**Para quem:** a próxima IA (ou pessoa) que vai **reescrever a engine do zero em Rust**.  
**Fonte de verdade C++:** `/home/bruno/Projects/TucanoEngine` — este documento descreve *como a engine funciona hoje*, o *contrato GPU*, as *minas*, e uma *ordem de construção* com critérios de saída.  
**Não é** o roadmap Vulkan/Linux (`docs/Vulkan-Linux-Roadmap.md`). Esse é histórico de port. Este é o **manual de reconstrução**.  
**Barra de qualidade (obrigatória):** `docs/Rust-Rewrite-Quality-Bar.md` — se este roadmap e a barra discordarem, **a barra ganha**. Não clones a checklist; clona o 6.

Idioma: português para o plano; **identificadores, structs, shaders e crates em inglês** (é o que o código usa).

---

## 0. Como usar este documento

1. **Não clones a checklist.** Clona o 6 (PBR + IBL + fog + clima + bindless). Recusa o que puxa a nota para 4. Detalhe: `docs/Rust-Rewrite-Quality-Bar.md`.
2. **Não clones o C++ linha a linha.** Clona o *contrato*: GBuffer packing, BRDF, bindless, rain, um clipmap. A implementação Rust deve ser mais limpa.
3. **Não comeces por mesh shaders nem ray tracing.** Ordem: triângulo → bindless → deferred PBR → **CSM com câmara real + TAA** → clima → terreno → editor.
4. **Cada fase tem um binário de gate** (equivalente a `Samples/Gates/`). Sem gate verde, não avanças. Sempre `--frames N` em GPU AMD/RADV.
5. **Lê os ficheiros-fonte listados** no fim de cada sistema. Este texto resume; o HLSL é a spec.
6. **Superar ≠ mais passes.** Superar = menos mentiras no pixel: CSM honesto, TAA, um clipmap, occupancy no lighting *ou* apagada, nenhum sistema chamado VSM até ser variance.

---

## 1. O que a engine é (mapa mental)

Tucano é um **renderer deferred bindless** com clima Cry-inspired, terreno clipmap, mundo streamed, vegetação GPU, GI híbrida (IBL + probes + SDF + SSGI/DDGI), editor ImGui, ECS próprio, física Jolt, Lua.

Módulos em `src/`:

| Pasta | Papel |
|---|---|
| `RHI/` | Contrato GPU. Um backend por binário: DX12 **ou** Vulkan **ou** Null. `RHI.h` é a API. Engine **não** inclui `d3d12.h`/`vulkan.h`. |
| `Renderer/` | Frame graph, GBuffer, lighting, shadows, post, weather, GI, OIT, visbuffer, meshlets |
| `Terrain/` | Clipmap (dois stacks — ver §7), VT, height query |
| `Vegetation/` | Instancing GPU, cull compute, LOD/impostor, vento |
| `World/` | Streaming de cells, cull GPU de instâncias/células, hot-reload de `.tcell` |
| `ECS/` | Archetypes/chunks SoA, sync para Scene/física |
| `Animation/` | Clips, skeleton, graph; paleta 4096 matrizes no renderer |
| `Physics/` | Jolt (static/dynamic, CharacterVirtual, raycast) |
| `Editor/` | ImGui docking, Outliner/Inspector, viewport offscreen (`sceneTextureId`) |
| `Lua/` | `LuaVM` singleton + bindings |
| `AssetPipeline/` | glTF, texturas, cook |
| `Platform/` | GLFW window, input |
| `Core/` | memória, reflection macros (`TUCANO_FIELD`) |
| `Runtime/` | DebugUI, screenshot |
| `Shaders/` | HLSL → DXC `.cso` (DX12) / `.spv` (Vulkan) |

**Decisão de produto no C++:** DX12 é produção no Windows; Vulkan é segundo backend. Na reescrita Rust, **escolhe um gráfico stack e fica com ele** (recomendação: `wgpu` para portabilidade, ou `ash`/`vulkanalia` se quiseres o mesmo nível de controlo que o Tucano Vulkan). Não faças dois backends no dia 1.

---

## 2. Decisões a manter vs. a mudar

### Manter (são o DNA visual)

- Deferred + 5 MRTs + D32, packing exacto do GBuffer (albedo/coatRough, normal/fuzz, ORM/F0, emissive/coat, depth color).
- PBR: **GGX + Smith-Schlick + Schlick F + Burley diffuse** + clearcoat GGX + fuzz Charlie. IBL **split-sum** (irradiance + prefiltered GGX mips + BRDF LUT) + energy compensation estilo Filament.
- Bindless por **índice inteiro no CB**, `Texture2D bindlessHeap[]`, slot 0 = null.
- Um sol direcional + ≤16 lights locais (point/spot) + lua sem sombra.
- CSM 4 cascades num atlas 2×2 R32 — **mas o frustum tem de vir da câmara real** (o C++ hardcoda 60° / 16:9; não copies isso). Octahedral points só depois. Contact shadows: opcional tardio.
- Rain em **dois tempos**: rewrite do GBuffer (molhado) + post (streaks/cones/particles).
- Clipmap **sem VB**: grelha de `SV_VertexID`, IB full + IB anel oco, morph de LOD.
- Frame graph com passes nomeados e aliasing de RTs.

### Mudar (é aqui que Rust supera)

- **Um clipmap**, não dois (`ClipmapTerrain` VS vs `TerrainRenderer` CS). O path de produção é `SV_VertexID`.
- **Descriptor layout de primeira classe**, não `#if TUCANO_SPIRV` a espalhar `u1 space1` pelo HLSL. Em wgpu/ash, o pipeline layout é o contrato; shaders não adivinham collisions.
- **Occupancy VoxelGI no lighting no mesmo PR em que o volume nasce**, ou o volume não nasce. Hoje o 32³ existe e **não é amostrado**.
- **TAA + motion vectors** — o C++ não tem; sem isto SSR/GTAO/sombras treme.
- **Nenhum “VSM”** até ser variance (momentos) ou virtual-paged com o nome certo. O C++ faz compare binário e chama-lhe VSM.
- **OIT no mesmo backend** que o resto (no C++ é Vulkan-only).
- **Sem `waitIdle` / `submitAndWaitHeadless` no frame quente.** Cull GPU = indirect, readback só em gates.
- **Sem dois shading languages mentais** (HLSL + mapa DXC→SPIR-V). Preferir WGSL (wgpu) **ou** HLSL via naga/DXC, um só.
- **RHI sem vtables C++.** Traits + um backend. Engine nunca vê `vk*`/`dx12*`.
- **Gates desde o crate 0** (`--frames N`, abort on device lost).

---

## 3. Como é feito o RENDER (frame graph)

Orquestrador: `src/Renderer/Renderer.cpp` → `Renderer::render`. Constrói `RenderGraph`, preenche lambdas, `m_graph.execute`.

Ordem exacta dos passes (nomes do graph):

```
Shadow → GBuffer → AO → Lighting → Water → Clouds
  → SSGI → DDGIUpdate → DDGISample → SSR → Compose
  → ContactShadows → RainPost → OIT → Bloom → Exposure
  → HiZPyramid → Tonemap → (AliasA/AliasB packer smoke)
```

O que acontece **dentro** de Shadow (não são nós separados): CSM (± scroll toroidal) → tiles octa point → tiles spot → páginas VSM → máscara RT opcional.

O que acontece **dentro** de GBuffer: visbuffer opcional, meshlets + cull GPU, clipmap + VT feedback, vegetação indirect, **rain deferred** (molha albedo/normal/ORM).

**GBuffer packing (ABI — copiar à letra):**

| RT | Formato | Conteúdo |
|---|---|---|
| 0 albedo | `R8G8B8A8_UNORM_SRGB` | RGB albedo, **A = clearcoatRoughness** |
| 1 normal | `R8G8B8A8_UNORM` | RGB normal oct `n*0.5+0.5`, **A = fuzz** |
| 2 orm | `R8G8B8A8_UNORM` | R AO, G roughness, B metallic, **A = dielectric F0** |
| 3 emissive | `R8G8B8A8_UNORM` | RGB emissive, **A = clearcoat** |
| 4 depthColor | `R32_FLOAT` | `SV_Position.z` (nome enganador: não é view-linear) |
| DSV | `D32_FLOAT` | depth hardware |

HDR: `R16G16B16A16_FLOAT`. CSM atlas: `R32_FLOAT` `shadowMapSize²` (default 2048).

Ficheiros: `Renderer.h`, `Renderer.cpp`, `RenderGraph/RenderGraph.h`, `Deferred/GBufferPass.*`, `Deferred/LightingPass.*`.

---

## 4. Como é feito o PBR

CPU `Material` / `MaterialGPU` (`src/Renderer/Material.h`):

- `baseColorFactor`, `metallicFactor`, `roughnessFactor`, `aoFactor`
- `reflectance` → F0 dieléctrico `0.16 * reflectance²` (0.5 ⇒ ~4%)
- `clearcoat`, `clearcoatRoughness`
- `fuzz`, `fuzzColor` (cloth/sheen)
- `detailScale` + detail albedo/normal
- mapas: albedo, normal, metallicRoughness (R=AO G=rough B=metal), ao, emissive
- alpha mask / cutoff

Shader `Shaders/Common.hlsl` + `GBuffer.hlsl` + `DeferredLighting.hlsl`:

```
F0 = lerp(dielectricF0, albedo, metallic)
kD = (1 - F) * (1 - metallic)
specular = D_GGX * G_SmithSchlick * F_Schlick / (4 n·v n·l)
diffuse  = Burley (Disney)
clearcoat = segundo lóbulo GGX, F0=0.04
fuzz     = Charlie NDF + velvet V + wrap diffuse
```

IBL split-sum:

- `irradiance` lat-long (difuso)
- `prefiltered` com mips GGX (`roughness * iblMaxMip`)
- BRDF LUT 2 canais `dfg`
- `getEnergyCompensation` (Filament)

Cook do IBL: `src/Renderer/GI/IBL.h` — HDRI ou fallback procedural. **Nunca amostrar mips UNDEFINED** (GPUVM no RADV).

---

## 5. Como é feita a LUZ

### Quem ilumina

| Fonte | Como |
|---|---|
| Sol | 1 directional. Direcção/intensidade do `SkyParams` + `Celestial` (Meeus low-accuracy) se `atmosphereDrivesSun`, senão light da Scene. |
| Lua | 2º directional **sem sombra**, phase + disc params. |
| Locais | ≤16 point+spot no `LightCB`. Point shadows: octa atlas (≤8). Spot: tiles perspectiva no mesmo atlas (≤4). |
| Ambiente | IBL + AO (GTAO) + probes (≤4) + SSGI/DDGI conforme `giTier`. |
| Atmosfera | Bruneton LUTs (transmittance / scattering / irradiance) **baked na CPU no init**. |
| Fog | Froxels 160×90×64 se volumétrico on; senão height fog analítico no lighting. |

### Constant buffers (ABI lighting)

O PS de lighting **não** usa o `FrameCBData` pequeno do início de `Renderer.cpp`. Usa `LightingCB` em `LightingPass` / `DeferredLighting.hlsl` (`b1`) + `LightCB` (`b2`).

`LightCB` (nomes):

```
lightCount, octaCount, spotCount
lightPosType[16]          // xyz, w = LightType (1 point, 2 spot)
lightColorIntensity[16]
lightRangeParams[16]      // range, innerCos, outerCos, shadowSlot+1 (0 = sem sombra)
lightDirection[16]
octaPosRange[8], octaUv[8], spotUv[4], spotViewProj[4]
```

`flags` no frame: x=shadows, y=IBL, z=AO, w=ESM.  
`shadowParams`: x=`pcssLightSize`, y=`esmExponent`, z=octa on, w=PCSS on.

Path: fullscreen triangle → reconstruir world a partir de depth+invVP → sol (± RT mask) → lua → loop 16 → IBL → Bruneton/fog → fog volume.

Céu: `SkyParams` (hora, turbidity, vento, lua, estrelas). Estrelas: catálogo 128×64 equatorial (`worldToEquatorial`).

Ficheiros: `LightType.h`, `Scene.h` (`Light`), `LightingPass.*`, `DeferredLighting.hlsl`, `Sky/Celestial.*`, `Sky/SkyParams.h`, `GI/BrunetonAtmosphere.*`, `GI/IBL.h`.

---

## 6. Como são feitas as SOMBRAS

Todas opcionais via `RendererSettings`. Combinam-se; lighting faz `min` onde faz sentido.

### CSM (cascaded shadow maps)

- 4 cascades, splits default ~5 / 20 / 60 / 200 m.
- Atlas único R32, 2×2 tiles: UV `(cascade%2, cascade/2)*0.5`.
- Matriz ortho por cascade a partir da câmara + direcção do sol.
- Bias cresce com o cascade.

### Toroidal CSM (`ToroidalShadowAtlas`)

Não redesenha o cascade inteiro quando a câmara anda. Centro ortho **snap** à grelha de texels no espaço da luz; `copyTextureRegion` faz scroll; só as faixas novas são dirty. Full refresh: sol mexeu, scroll grande, recenter Z, ou ~240 frames.

### Octahedral point shadows (`OctahedralShadowAtlas`)

Um tile 2D por point light (não um cubemap). Direcção → `encodeOcta` UV. Depth = **distância linear / range**. Spots no mesmo atlas com `spotViewProj`. Atlas default 2048, tile 512, ≤8 points + ≤4 spots.

### VSM virtual (`VirtualShadowMapAtlas`)

Grelha virtual 32×32 páginas de 128² ⇒ 4096 virtual. Físico 8×8 = 64 páginas. Page table R32 (−1 = unmapped). Aloca à volta do centro da vista da luz. Só cascade 0 no lighting: `min(CSM, sampleVSM)`.

### PCSS

16 taps Vogel: blocker search → penumbra `(z − avgBlocker) * lightSize / avgBlocker` → PCF adaptativo raio 1…4.5. Default `pcssLightSize = 0.035`.

### ESM

`saturate(exp(−k * (zRef − d)))`, `k = esmExponent` (80). Fura geometria fina. Default **off**.

### Contact shadows

Marcha curta em screen-space na direcção do sol sobre `depthColor`. Escurece o HDR depois do lighting. RT contact é path extra (`RTContact.hlsl`) se DXR.

Ficheiros: `Shadows/ToroidalShadows.*`, `OctahedralShadows.*`, `VirtualShadowMaps.*`, `Shaders/Shadow.hlsl`, `ShadowOcta.hlsl`, `ContactShadows.hlsl`, `DeferredLighting.hlsl` (sample).

---

## 7. Como é feito o TERRENO

**Há dois stacks. Na reescrita, implementa só A.**

### A — Produção: `ClipmapTerrain` + `ClipmapTerrain.hlsl`

Técnica Losasso/Hoppe. **Não há vertex buffer.** `SV_VertexID` gera a grelha `kGridN = 64` quads/lado. CPU só tem:

- IB full (nível 0, grelha completa)
- IB anel oco (níveis 1…N, buraco = nível mais fino)
- por frame: `origin`, `spacing` (×2 por nível), `extentHalf`, frustum `visible`

Default: **6 níveis**, `baseSpacing = 1.5 m`, `morphStart = 0.6`. Heightmap é um índice bindless + world min/size + heightScale. VT albedo opcional (`TerrainVirtualTexture`).

`update(camera)` **antes** do render. O Renderer desenha no GBuffer com o PSO de clipmap.

### B — Legado: `Clipmap` + `TerrainRenderer`

8 anéis × 129² verts, **compute** preenche VB estruturado, graphics desenha. Atlas de material 4096², tiles 64 m / 128², LRU 512. Gate: `terraincs`. Não duplicar.

### HeightmapQuery

CS 64 threads, ≤256 queries, ring de readback 3 frames. UAV **não é mappable** — upload + copy. Gate: `heightquery` (GPU vs CPU ~0.002 m).

### Virtual texture

- Coverage CPU a partir dos anéis (`terrainvt`)
- Feedback GPU no PS (`terrainvtfb`) — readback com `waitIdle` só no gate, não no jogo

Ficheiros: `Terrain/ClipmapTerrain.*`, `Shaders/ClipmapTerrain.hlsl`, `Clipmap.*`, `TerrainRenderer.*`, `HeightmapQuery.*`, `TerrainVirtualTexture.*`, `MaterialAtlas.*`.

---

## 8. Como é feita a CHUVA

Inspiração CryEngine (`SRainParams`), **reimplementada**. Dono: `Renderer::m_rain`. Corre **mesmo depois de `enabled=false`** enquanto `m_wetLevel` seca.

### Pass 1 — Deferred GBuffer (depois do GBuffer opaco)

1. Occluder: depth 512² no espaço da chuva (vista de cima).
2. Occlusion fullscreen.
3. Wetness ping-pong 512² tiled no mundo.
4. Rewrite albedo (escurece), normal (ripples), ORM (gloss).

Barreira do occluder tem de incluir **VERTEX_SHADER** — as partículas amostram depth no VS.

### Pass 2 — Post (depois de HDR / SSR)

- Composite: puddles com SSR local, mist, streaks lit
- SceneRain: 3 cones volumétricos **additive** — **nunca** bindar o HDR como SRV neste pass (MODE1 reset)
- `draw(24576 * 6)` partículas stateless + splashes no chão
- Lens drops fullscreen

Quase tudo **graphics**, zero compute. Texturas bindless via `RainCB.texIds*`.

`RainParams` chave: `amount`, `maxViewDist`, puddles/ripples/SSR, streaks/mist, `enableSceneRain`, `enableWorldSplashes`, `wind`, volume `worldPos`/`radius`. Clouds podem `driveRain` via weather map.

**Mina:** `--seconds 30` com rain GPUVM’ou RADV (2026-08-18). Gates: `--frames 16`. Nunca unbounded.

Ficheiros: `Weather/RainSystem.*`, `RainParams.h`, `Shaders/Rain.hlsl`. Gate: `Samples/Gates/rain.cpp`.

---

## 9. Nuvens, fog, água

### Clouds

Volumétrico half-res: weather map 256² → ray march 64 steps → temporal → composite full-res. Noise 3D: Perlin-Worley 128³ + Worley 32³ (CPU bake, cache `.raw`). `Texture3D` bindless space2. God rays + sombras no chão. `driveRain` lê coverage.

### Fog

Compute only. Froxels `160×90×64` RGBA16F. Inject `[8,8,4]` dispatch `(20,12,16)`; integrate `[8,8,1]` dispatch `(20,12,1)`. Jitter Halton. Lighting amostra `integratedBindless()`. 3D UAV em **GENERAL**. Fallback: height fog no PS.

### Water

Um fullscreen pass, **sem depth test** (desenha até ao horizonte). Gerstner octaves, foam/SSS/shore, SSR vs depth **point-sampled**, refracção + Beer-Lambert. Rain acopla `rainIntensity`. Preferir IBL para reflexos (céu Bruneton não bate com o horizonte da água).

Ficheiros: `CloudSystem.*`, `CloudNoise.*`, `Shaders/Clouds.hlsl`, `FogSystem.*`, `Shaders/VolumetricFog.hlsl`, `WaterSystem.*`, `Shaders/Water.hlsl`.

---

## 10. GI, probes, visbuffer, meshlets, OIT

| Sistema | Algoritmo | Nota |
|---|---|---|
| `giTier` Off/Low/Medium/High | Low = SSGI; Medium+ = DDGI update+sample (async compute se houver fila) | FeatureGate e Sponza default Off |
| WorldSDF | CPU seed triângulos → 64³ × 4 cascades → GPU JFA + finalize. Atlas SDF+SH1 | SPIR-V: `RWTexture3D` set 4, **não** bindless 2D |
| VoxelGI | Occupancy 32³ CPU AABB, upload aligned 256 | **Ainda não entra no lighting** — oportunidade de superar |
| ReflectionProbes | ≤4 probes, faces 64 → lat-long 128×64, 5 mips GGX. Seed CPU do SDF | Precisa `enableSSR` ou voxel GI para correr |
| VisBuffer | Triangle-id deferred, resolve materiais | Sem mesh shaders |
| Meshlets | meshoptimizer CPU; cull CS frustum+cone+Hi-Z; compact; `drawIndexedIndirectCount` | Arrays HLSL `buf[3]` **não** mapeiam no set Vulkan (1 descriptor/binding) — buffers soltos |
| OIT | Linked list por pixel: clear CS, collect PS atomics, resolve CS | Vulkan `fragmentStoresAndAtomics`; DX12 no-op |
| Hi-Z | Copy depth → reduce mips; cull usa mip 2 do **frame anterior** | Dummy 1×1 sampled: barrier VS+PS+CS |

---

## 11. Mundo, vegetação, skinning

**Streaming:** disk → workers deserializam `.tcell` → **upload GPU só na main thread** (RHI single-thread). Hysteresis, HLOD, prediction, persistence.

**CellCull / InstanceCull:** CS space1 `t0` + UAV `u1`/`u2`. Gates `cells` / `instances` usam readback. O jogo deve ser **indirect sem wait**.

**Vegetação:** até 65536 instâncias, 16 tipos, 3 LOD (full / simplified / impostor). Cull CS 64 threads, vento no CS, `drawIndexedIndirect`. Layout Vulkan: `[t0][u1 out][u2..u4 vis][u5..u7 args]`. Não rebuild CPU das instâncias todos os frames.

**Skinning:** `Vertex` traz bone indices+weights sempre (20 bytes extra). Paleta `float4x4[4096]` em space1 `t0`. `skinInfo.y == 0` ⇒ rígido. Graph de animação no editor está **crash-gated**.

---

## 12. Contrato RHI (o que o crate `rhi` tem de expor)

Espelhar `src/RHI/RHI.h`, não a implementação DX12/Vulkan.

**Device:** create buffer/texture/sampler, root signature, PSO graphics/compute, begin/end frame, waitIdle, upload, `writeResourceTable` (texture/buffer/sampler), `presentFormat`, caps (mesh, RT, async compute).

**CommandList:** `transition`, set PSO/root, viewport/scissor, MRT + depth, clears, VB/IB, root CBV/constants/SRV/UAV/sampler tables, draw / drawIndexed / indirect(+count), dispatch, UAV/alias barriers, copies (incl. `copyTextureRegion` para toroidal), `copyBufferToTexture` com row pitch 256.

**Texture / Buffer:** `bindlessIndex()`, `bindlessUavIndex()`, `srvIndex()`, `uavIndex()`, mapped ptr só em upload heaps.

**Swapchain:** acquire, present, resize **sem destruir a surface** (GLFW: uma surface por window; passar `oldSwapchain`).

### Bindless / root (espelhar, mesmo em wgpu)

```
0  push constants 128 B (viewProj+world)
1  CBV b1
2  CBV b2
3  unbounded sampled 2D (+ 3D noutro set)
4  samplers
5  storage buffers (structured)
+  storage images (UAV 2D/3D) noutro set
```

Heap sampled 8192, **índice 0 = null para sempre**. Shaders indexam `NonUniformResourceIndex`.

**Regra Vulkan que o C++ descobriu à força:** em set 2, `t0 space1` e `u0 space1` são o **mesmo binding 0**. UAVs de buffer começam em **`u1`**. Arrays de `RWStructuredBuffer` no HLSL viram um binding — SPIR-V precisa de buffers **separados** (`Visible0/1/2`, não `Visible[3]`).

**Não destroias um RootSignature / PipelineLayout** depois de criar o PSO. O PSO guarda o handle. (Bug do clipmap CS: 497 erros de validation.)

---

## 13. Minas (copiar para o AGENTS.md da engine Rust)

1. RADV GPUVM (SQC / PERMISSION_FAULTS) **não aparece na validation**. Kernel page-fault → MODE1 reset → GNOME cai. Mitigação: `--frames N`, `MESA_VK_ABORT_ON_DEVICE_LOSS=1`, nunca relançar interactivo depois de GPUVM.
2. Upload de textura **por mip**. IBL prefiltered com mips UNDEFINED + `SampleLevel` = GPUVM.
3. Barreiras: `VK_REMAINING_MIP_LEVELS` / `ARRAY_LAYERS`. UAV view = mip 0 só se o recurso tem mips.
4. Dummy 1×1 sampled: destino VS+PS+CS. Dummy 3D UAV em GENERAL.
5. Rain: VS precisa de ShaderResource+VERTEX no occluder. Cones não lêem HDR.
6. Hot-reload de PSO: **antes** de gravar o frame; **não** recriar o pipeline layout.
7. ImGui: pool/descriptor **separado** do bindless 8192. `sceneTextureId`: não `vkFreeDescriptorSets` de um set ainda no cmd in-flight (adiar um ciclo FIF). Viewport RT usa `presentFormat()` (BGRA no X11/Mesa), não RGBA hardcoded.
8. `beginFrame` espera a fence do slot — writes em dynamic CB **depois** de beginFrame.
9. Row pitch 256 em `copyBufferToTexture`.
10. Validation callback pode correr noutro thread → heap TLS (rpmalloc `memoryInitThreadHeap`).

---

## 14. Mapa de crates Rust (proposta)

```
tucano/
  crates/
    tucano-rhi/          # trait Device, wgpu ou ash
    tucano-shaders/      # WGSL ou HLSL cook
    tucano-render/       # frame graph, GBuffer, lighting, shadows, post
    tucano-weather/      # rain, clouds, fog, water
    tucano-terrain/      # um clipmap + height query + VT opcional
    tucano-world/        # streaming, cell/instance cull (math pura + adapter GPU)
    tucano-vegetation/
    tucano-gi/           # IBL, probes, SDF, SSGI/DDGI
    tucano-scene/        # Camera, Mesh, Material, Light — sem GPU
    tucano-ecs/          # opcional fase 5; pode começar sem
    tucano-anim/
    tucano-physics/      # rapier ou jolt-rs, fase 5
    tucano-assets/       # glTF (gltf crate), imagens
    tucano-editor/       # egui ou imgui-rs, fase 6
    tucano-app/          # window winit/glfw, loop
  bins/
    hello-triangle
    pbr-grid
    gates/*              # um bin por feature, --frames 16
```

Regra: `tucano-render` depende de `tucano-rhi` + `tucano-scene`. **Não** depende de editor, Lua, ECS. Weather/terrain/gi são crates chamados pelo frame graph, não o contrário.

---

## 15. Fases da reescrita (com exit)

Cada fase: código + **um binário que corre N frames e sai 0**. Sem pixel-identical vs C++. “Reconhecível” chega.

### Fase 0 — Contrato (3–7 dias)

- Traits `Device`, `CommandList`, `Texture`, `Buffer`, `SwapChain`.
- Backend Null (CPU stub) para testes sem GPU.
- Window + loop `--frames`.
- **Exit:** `hello-triangle` Null compile; doc do bindless layout escrita *antes* do primeiro shader.

### Fase 1 — Triângulo GPU (1–2 sem)

- Swapchain, dynamic rendering ou render pass único, VS+PS, validation 0.
- Resize: surface estável, `old_swapchain`.
- **Exit:** janela, triângulo, 0 erros validation, `--frames 90`.

### Fase 2 — Bindless + upload (2 sem)

- Heap sampled 8192, slot 0 dummy 1×1 com barrier VS+PS+CS.
- `bindless_index` na textura. Shader amostra `heap[idx]`.
- Upload mips completos. VMA ou wgpu allocator.
- **Exit:** triângulo texturizado; compute UAV simples.

### Fase 3 — Deferred PBR (2–4 sem)

- 5 MRTs + depth. GBuffer packing **idêntico** à tabela §3, **com `fuzzColor` a chegar ao lighting** (ou o campo não existe).
- Lighting fullscreen: 1 sol + IBL split-sum + tonemap.
- Multi-scatter nas luzes **directas** (`getEnergyCompensation` também no sol/points — o C++ só aplica no IBL).
- Material GPU + glTF mínimo (sem Sponza ainda).
- **Exit:** `pbr-grid` 90 frames, validation 0. Sem clima, sem sombras.

### Fase 4 — Sombras + TAA (2–3 sem)

Barra: `docs/Rust-Rewrite-Quality-Bar.md` §4.1 e §4.8.

- CSM 4 cascades atlas 2×2 com **frustum da câmara real** (FOV, aspect, near/far). Snap de texels. PCF Poisson/Vogel. Texel derivado do atlas — **não** `1/2048` hardcoded, **não** FOV 60°/16:9.
- Motion vectors (object + camera) + **TAA** (history + neighbourhood clamp). O C++ não tem; sem isto o resto treme.
- Depois, se o gate CSM+TAA estiver verde: PCSS. Octa só se houver point lights de teste.
- **Não** portes VSM, ESM, nem contact nesta fase. Toroidal = optimização do CSM verdadeiro, depois.
- **Exit:** gates `csm` e `taa` 16 frames. Aspect ratio mudado → sombra continua a bater no contacto.

### Fase 5 — Clima (3–4 sem)

Ordem: fog compute → clouds (sem driveRain) → water → **rain por último** (é o mais perigoso).

- Fog: froxels, 3D GENERAL.
- Clouds: noise cache em disco.
- Water: point-sample depth no SSR.
- Rain: GBuffer wet + post; cones sem HDR SRV; `--frames 16` only.
- **Exit:** gates `fog` `clouds` `water` `rain`.

### Fase 6 — Terreno + vegetação + mundo (3–5 sem)

- Só `ClipmapTerrain` SV_VertexID.
- HeightQuery GPU vs CPU.
- Veg cull + indirect (buffers soltos, sem arrays HLSL).
- Instance cull 2500 cubos → 1 indirect.
- Streaming: CPU first; GPU cull sem `waitIdle` no frame.
- **Exit:** gates `terrain` `heightquery` `veg` `instances`. VT/feedback a seguir.

### Fase 7 — GI + post extra (2–3 sem)

Barra: occupancy **no lighting neste PR** ou o volume não nasce. SSGI de 8 taps **não** é o exit.

- GTAO, bloom, auto-exposure (já no path default Tucano). TAA já veio da fase 4.
- SSR, probes (seed CPU + captura) no miss do SSR.
- Occupancy 32³ amostrada no lighting **ou omitida**. Meio volume órfão é recusado.
- WorldSDF JFA só se o compose/lighting ler o atlas.
- SSGI/DDGI: **fora do exit**. Só depois, com gate de bounce (caixas coloridas), não bleed de vizinhos.
- **Exit:** gates `ssr` `probes`. Occupancy: gate `occupancy` (pixel muda com o volume) **ou** zero bytes GPU.

### Fase 8 — Editor (2–4 sem)

- egui/imgui pool **separado**.
- Viewport = RT offscreen `present_format` + ImGui image, free de descriptors atrasado.
- Outliner / Inspector gerados por reflection (o C++ usa `TUCANO_FIELD` — em Rust: `bevy_reflect` ou macros próprias).
- File dialog: rfd / native; não bloquear o GPU loop sem fence.
- **Exit:** `--frames 8` docking + viewport 3D. Sem crash resize.

### Fase 9 — Opcional (depois do editor)

- Mesh shaders se a GPU tiver a extensão; fallback VS obrigatório.
- Ray query shadows/reflections; senão SSR/CSM continuam.
- OIT linked-list no mesmo backend.
- Physics + ECS + Lua/Rhai se o produto precisar — **não** bloqueiam o renderer.

---

## 16. Como superar (não só copiar)

Fonte normativa: `docs/Rust-Rewrite-Quality-Bar.md`.

Não clones a checklist. Clona o 6 (PBR + IBL + fog + clima + bindless) e recusa o que puxa a nota para 4.

**Estes quatro, no sítio, já batem o C++** — não por ter mais passes, por ter menos mentiras no pixel:

1. **CSM com frustum da câmara real** + PCF decente + texel do atlas. O C++ usa FOV 60° e aspect 16:9 em `computeCascadeVP`.
2. **TAA + motion vectors.** O C++ não tem; SSR/GTAO/sombras treme.
3. **Um clipmap** (`SV_VertexID`). Sem o path compute legado.
4. **Occupancy no lighting, ou apagada.** Volume 32³ órfão é recusado.
5. **Nenhum sistema chamado VSM** até ser variance (momentos) ou virtual-paged com o nome certo. O C++ faz compare binário.

Depois: probes honestos, SSR Hi-Z, octa, PCSS. Cada um com gate de *pixel*. SSGI de 8 vizinhos e ESM **não** entram no MVP.

Ainda vale (higiene, não checklist):

- Cull 100% indirect — zero `waitIdle` fora de testes.
- Layout GPU explícito (wgpu bind groups), sem `#if SPIRV`.
- Hot-reload sem matar o pipeline layout.
- Testes CPU: BRDF, cascade splits, clipmap placement, octa encode.

Teste de honestidade antes de mergear um pass: o nome bate com o paper? O lighting lê o recurso? A câmara é a real? Default-on aguenta 90 frames? O gate prova a feature, não só validation 0.

---

## 17. Índice de fontes (abrir nesta ordem)

### Render / PBR / luz / sombra

- `src/RHI/RHI.h`
- `src/Renderer/Renderer.h` + `Renderer.cpp` (`render()`, `addPass`)
- `src/Renderer/RenderGraph/RenderGraph.h`
- `src/Renderer/Material.h`
- `src/Renderer/LightType.h` + `Scene.h`
- `src/Renderer/Deferred/GBufferPass.cpp` + `LightingPass.cpp`
- `Shaders/Common.hlsl` + `GBuffer.hlsl` + `DeferredLighting.hlsl`
- `src/Renderer/Shadows/ToroidalShadows.*` `OctahedralShadows.*` `VirtualShadowMaps.*`
- `Shaders/Shadow.hlsl` `ShadowOcta.hlsl` `ContactShadows.hlsl`

### Clima

- `src/Renderer/Weather/RainSystem.*` `RainParams.h` `Shaders/Rain.hlsl`
- `CloudSystem.*` `Shaders/Clouds.hlsl`
- `FogSystem.*` `Shaders/VolumetricFog.hlsl`
- `WaterSystem.*` `Shaders/Water.hlsl`

### Terreno / veg / mundo / GI

- `src/Terrain/ClipmapTerrain.h` `Shaders/ClipmapTerrain.hlsl`
- `src/Vegetation/VegetationGPU.h` `VegetationDispatch.cpp` `Shaders/VegetationCull.hlsl`
- `src/World/GpuCellCuller.cpp` `InstanceCloudCuller.cpp` `StreamingScheduler.h`
- `src/Renderer/GI/WorldSDF.*` `VoxelGI.*` `ReflectionProbes.*` `BrunetonAtmosphere.*` `IBL.h`
- `Shaders/SdfJfa.hlsl` `SdfFinalize.hlsl` `Phase3.hlsl`

### Gates de referência (o que “verde” significa)

`Samples/Gates/*.cpp` + `Samples/Common/FeatureGate.h` — um binário, uma feature, 16 frames, cena limpa.

### Lições Vulkan já pagas

`docs/Vulkan-Linux-Roadmap.md` — descriptor map, GPUVM, rain, clipmap CS layout, hot-reload PSO, editorvp descriptors.

### Barra de qualidade (o que *não* clonar)

`docs/Rust-Rewrite-Quality-Bar.md` — nota 6/10 do renderer C++, o 6 vs o 4, teste de honestidade, evidência (`computeCascadeVP`, `sampleVSM`, `PSSSGI`, VoxelGI órfão).

---

## 18. Prompt curto para a próxima IA

> Reescreve Tucano em Rust seguindo `docs/Rust-Rewrite-Roadmap.md` e `docs/Rust-Rewrite-Quality-Bar.md`. A barra ganha se discordarem. Não clones a checklist do C++. Clona o 6: PBR, IBL, fog, clima, bindless. Recusa o 4: VSM que não é variance, occupancy órfão, segundo clipmap, CSM com FOV 60°/16:9, SSGI de 8 taps como GI. Obrigatório cedo: CSM com frustum da câmara real + TAA com motion vectors. Occupancy entra no lighting no mesmo PR em que o volume nasce, ou não nasce. Não comeces por mesh shaders nem DXR. Fase N só depois do exit da N−1. Rain em dois passes; nunca `--frames` unbounded em AMD. RHI trait, engine sem tipos wgpu/ash. Superar = menos mentiras no pixel, não mais passes.

---

*Gerado a partir do código em `/home/bruno/Projects/TucanoEngine` (setembro 2026). Se o C++ divergir, o HLSL e `Renderer::render` ganham.*
