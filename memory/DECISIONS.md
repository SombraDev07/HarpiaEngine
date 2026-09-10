# Decisions

Fechadas. Não reabrir sem motivo escrito aqui.

## D-1 — Produto

- Esta engine chama-se **Harpia**. Crates `harpia-*`, `ENGINE_NAME = "harpia"`.
- **Tucano** (`TucanoEngine/`) é referência C++ para *superar* (PBR/IBL/fog/clima/bindless honestos). Não se copia linha a linha, não se reusa o nome nos crates, não se aninha um `tucano-rs`.

## D7 — Pastas (disciplina Dagor, nomes Harpia)

- Código em `prog/`. Doutrina em `docs/`. Memória da IA em `memory/`.
- `prog/engine/` — um crate por pasta, irmãos: `core`, `memory`, `math`, `drv` (RHI), `render`, `scene`, `app`, `plugin`, `editor`. Sem `core/harpia-core`.
- `prog/1stPartyLibs/` — libs nossas que não são a engine. `prog/3rdPartyLibs/` — vendor (FFX). `prog/plugins/` — cdylib.
- `prog/samples/` e `prog/tools/` **não** vivem dentro de `engine/`.
- Inspirado na divisão `prog/engine` + 1st/3rd da Dagor. **Não** copiar `daECS` / `daNet` / a checklist de pastas da Gaijin. Anim/ECS/net só quando a fase os pedir.

## D0 — Stack gráfico

- Vulkan 1.3 via `ash` + `gpu-allocator`. **Proibido** wgpu / DX12 / GL / Metal.
- Um backend GPU (Vulkan) + Null para testes CPU.
- Linux: X11 + Wayland. Windows: Win32. Surface via `winit` + `ash-window`. `cfg` só em `harpia-rhi` (`window.rs`).

## D1 — Edição Rust

- Pedido: 2024 se possível, senão 2021.
- Workspace em **2024** (toolchain local: rustc 1.98).

## D2 — Bindless

- Spec: `docs/Bindless-Descriptor-Layout.md`.
- Heap sampled 8192, slot 0 = null (dummy 1×1) criado no init do device Vulkan.
- Índice inteiro no CBV (set 0), escrito **depois** de `begin_frame`.
- Hello-triangle continua com pipeline layout vazio. `gate-bindless` usa o layout completo (sets 0–4).

## D3 — Hello-triangle

- Vértices no VS via `SV_VertexID` / `VertexIndex`. Sem vertex buffer.
- Dynamic rendering (VK 1.3). Sem `VkRenderPass`.
- Default `--frames 90` (nunca unbounded por omissão). Resize programático nos frames 30 e 60.
- `--interactive` / `-i`: janela até o utilizador fechar. Sem resize automático. Aviso AMD. Não é o default.
- FIFO present. `old_swapchain` no resize. Surface vive até ao drop do device.
- `present_format` escolhido da lista da surface; preferir `B8G8R8A8_UNORM` (X11/Mesa).

## D4 — Allocator

- `harpia-memory` wraps `mimalloc` + atomics de stats. Não é um malloc novo.
- `FrameBump` wraps `bumpalo`; API existe, hello-triangle não a usa.
- Global allocator instalado nos bins, não nas libs.

## D5 — Plugins

- `harpia-plugin` tem trait + loader vazio (`libloading` no Cargo).
- Hello-triangle **não** carrega plugins.

## D6 — Shaders

- Fonte de verdade: HLSL ao lado do sample (`triangle.hlsl`, `bindless.hlsl`).
- Cook: DXC `-spirv -fspv-target-env=vulkan1.3` quando existir. Fallback: `spirv-as` ou `prog/tools/assemble_spvasm.py` sobre `.spvasm`.
- Este host **não tem DXC**. Não assumir `dxc` no PATH. SPIR-V 1.4+ lista UBO/heap no `OpEntryPoint`.
- `PushConstant` no assembler é storage class **9** (não 8 = Generic). `GLSL.std.450 FMix` exige operandos do mesmo tipo que o resultado (splat o `metallic` para `vec3`).
- `harpia-shaders` como crate de cook: fase posterior. SPD/FSR: FidelityFX em `prog/3rdPartyLibs/`, não reescritos.

## D8 — Git

- Remote: `https://github.com/SombraDev07/HarpiaEngine.git`, ramo `main`.
- Não vender `TucanoEngine/`. Não commitar as cópias `Rust-Rewrite-*.md` na raiz (`.gitignore`; canónico é `docs/`).
- Sem `git config` no repo. Identidade de commit = conta GitHub via env, se o host não tiver user.email.

## D9 — GBuffer packing / fuzzColor / HDR fase 3

- Packing = tabela do roadmap §3. Normal guardado como `n*0.5+0.5` (não octahedral verdadeiro).
- Se `fuzz > 0.001`, RT3 RGB = `fuzzColor` e o lighting **não** soma emissive. Senão RT3 RGB = emissive. O campo existe e chega ao pixel.
- Push constants 128 B: `viewProj` + `world`. UBO set 0 = `LightingCb` (1024 B reservados; `gate-bindless` continua a escrever só os primeiros 16 bytes).
- Sampler wrap em set 2 binding 0; clamp-to-edge em binding 1 (IBL / GBuffer).
- Fase 3 tonemapa no PS de lighting para o swapchain. **Sem** RT HDR persistente até o bloom (SPD) precisar. Formato `R16G16B16A16_FLOAT` já existe no RHI.
- MaterialGPU + grelha procedural; sem parser glTF **na fase 3**. Sponza entra na fase 4 (D10).

## D10 — Sponza é a cena de referência

- **Sponza (glTF Khronos / Crytek, CC-BY)** é a cena onde se testa PBR, gráficos, luz e sombras — e o que vier (clima, GI, viewport do editor).
- `pbr-grid` continua o gate de packing/BRDF. Não substitui a Sponza.
- Sample: `prog/samples/sponza`, `--frames 90`. Assets: `assets/sponza/` (LFS, submodule ou fetch). Crate `gltf` + `image`. **Não** Intel New Sponza no MVP.
- Fase 4: exit = gates `csm`/`taa` **e** Sponza 90 frames. Sem isto a sombra só foi vista num cubo.

## D11 — Fase 4: CSM honesto, TAA, Sponza albedo

- CSM: 4 cascades, atlas 2048 2×2, frustum da **câmara real**, practical splits λ=0.75, snap do centro da esfera nos eixos right/up da luz, PCF Vogel 8, `atlas_size` no CB. `LightingCb.cascade_count == 0` ⇒ `pbr-grid` continua sem sombras.
- TAA: motion = ΔNDC da câmara **e** slide X do objecto; history ping-pong HDR; neighbourhood clamp 3×3. Sem jitter Halton (isso é para Hi-Z/GTAO).
- Sponza MVP: glTF + albedo sRGB + Lambert + CSM. **Não** é o packing/IBL do `pbr-grid` no atrium — isso é polish. Fetch: `prog/tools/fetch_sponza.py` (jsDelivr; `assets/sponza/glTF/` gitignored).
- RHI: `begin_color_pass` com 0 cores + depth (atlas); `PipelineTargets.depth_only` (empty colors ≠ swapchain); `set_viewport`; depth bias; UV em location 2 só se `instance_stride == 0` e stride ≥ 32.
- Staging bindless: 64 MiB (Sponza 2k–4k + pitch 256).

## D12 — Clip space é Vulkan, num único sítio

- `harpia_math::perspective_vk` é a **única** projeção de câmara. Nega a linha Y do
  `Mat4::perspective_rh` (que já dá depth `[0,1]`).
- Winding do mundo: CCW visto de fora. `FRONT_FACE_COUNTER_CLOCKWISE` + cull-back.
  As malhas do `harpia-render` seguem isso (teste `winding_faces_outwards`).
- As matrizes de luz do CSM ficam sem flip, por causa do mapeamento do atlas
  (`uv.y = ndc.y*0.5+0.5`). Está em `LANDMINES.md`; mudar uma exige mudar a outra.
- Casters de sombra: **sem culling** (`cull_back: false`). Geometria de uma face só
  (os panos da Sponza) tem de projetar sombra; acne é trabalho do depth bias.

## D13 — CBV do frame é um anel dinâmico

- Set 0 binding 0 = `UNIFORM_BUFFER_DYNAMIC`. Um anel por slot de frame:
  `FRAME_CBV_CHUNKS` (512) × `FRAME_UBO_SIZE` (1024 B).
- Contrato: cada `write_frame_bytes` toma um chunk novo; **todos os draws e dispatches
  gravados a seguir leem esse chunk**. O RHI re-aponta o set 0 sozinho quando o chunk
  muda (só o set 0, sets 1–4 ficam).
- É isto que permite material por primitiva sem push constants (os 128 B já estão
  cheios com `viewProj` + `world`).
- Estourar o anel é erro, não silêncio. O backend Null tem o mesmo orçamento.

## D14 — Sponza: albedo partilhado, mips, cutout, tonemap

- Uma textura por imagem glTF (dedup por `source().index()`), mip chain completa
  gerada em espaço linear no `harpia-render`.
- `alphaMode: MASK` do glTF chega ao PS via `LightingCb::alpha_cutoff` (por draw) e
  faz `OpKill`. `doubleSided` usa um segundo PSO sem culling; as primitivas ficam
  ordenadas (uma face primeiro) para não trocar de PSO por draw.
- O PS da Sponza fecha com Lambert `/π`, ACES fitted (`exposure` do CB) e **encode
  sRGB**: o swapchain é `B8G8R8A8_UNORM`, ninguém mais faz o gamma.

## D15 — Captura é readback, não screenshot

- `--capture <prefixo>` no `harpia-app`: depois do último frame lê de volta cada
  textura de `Sample::capture_targets()` e grava PNG. Cores, HDR (`Rgba16Float`),
  motion (`Rg16Float`) e profundidade (`D32Float`, normalizada com min/max no log).
- `Device::read_texture` espera o device — é caminho de captura/debug, nunca frame
  quente. Todas as imagens nascem com `TRANSFER_SRC`.
- Nenhum gate volta a ser julgado por screenshot da janela.

## D16 — Volumes vivem nos sets 4/5, nunca no heap 2D

- `TextureDim::D3` + `depth_slices` no `TextureDesc`. Um volume tem view 3D e é
  inválido no heap 2D (set 1) e no UAV 2D (set 4 binding 0).
- Set 4 binding 1 = `VOLUME_UAV_SLOTS` (4) UAVs 3D. Set 5 binding 0 =
  `VOLUME_SRV_SLOTS` (8) sampled 3D. É o que `docs/Bindless-Descriptor-Layout.md`
  já dizia; agora existe.
- Bindings nomeados por índice constante no shader (`OpTypeArray`), não runtime
  array: são poucos e o índice é uniforme. `bindless_index()` **erra** para 3D.
- Volumes ficam em `GENERAL` a vida toda (escritos como UAV, lidos como SRV).
  Um dummy 1×1×1 enche os dois arrays no init.

## D17 — Fog é froxel compute, exactamente como o roadmap §9

- 160×90×64 RGBA16F × 2 (scatter, integrated). Inject `[8,8,4]` → (20,12,16),
  integrate `[8,8,1]` → (20,12,1). 90 não é múltiplo de 8: os shaders testam Y.
- Z exponencial `near·(far/near)^((z+0.5+jitter)/D)`, jitter Halton(2). O apply
  inverte com `log`.
- Inject: extinção de height fog + sol com fase Henyey-Greenstein. **Sem** CSM
  ainda — sombras volumétricas / god rays ficam para quando as clouds entrarem.
- Integrate conserva energia: `S = (Sc - Sc·T_slice)/sigma`, `acc += T·S`.
- `FogCb` (176 B) é o contrato com os `.spvasm`; o teste
  `fog::tests::cb_layout_matches_the_spvasm` fixa os offsets.

## D18 — Sombra: a barra fecha em CSM honesto + PCF por hardware + PCSS

A `Rust-Rewrite-Quality-Bar.md` define sombra pronta de forma estreita: «clonar
**só** CSM, mas consertar o frustum», «até lá: **CSM + PCSS**. Um path». Recusa
VSM, ESM e contact shadows; toroidal é optimização posterior, não produto. Está
tudo feito:

- **Sampler de comparação** (set 2 binding 2, `LESS_OR_EQUAL`). Os taps usam
  `OpImageSampleDrefExplicitLod`: o hardware compara **e depois** filtra, e cada
  tap já é 2×2 PCF. Amostrar profundidade com sampler linear e comparar a seguir
  é a ordem errada — estava assim e errava em todas as silhuetas.
- **PCSS** (§6): blocker search de 8 taps → penumbra → PCF adaptativo de 16 taps,
  raio 1…4.5 texels, `pcssLightSize = 0.035`.
- **Penumbra na forma direccional**, não a do C++. Para uma cascata ortográfica a
  largura é `(z_recv − z_blk) · depthRange · lightSize` em metros, convertida a
  texels pelo world-per-texel da cascata. Sem dividir pela profundidade do
  blocker — essa é a fórmula de luz perspectiva e rebenta perto de zero.
  `Csm::radius` leva o `depthRange` e o `world_per_texel` ao shader.
- **Disco de Vogel rodado por pixel** com interleaved gradient noise; sem isto os
  16 taps repetem o mesmo padrão em todo o ecrã.
- **Taps presos ao tile** da cascata (`FClamp` ao rect), senão o raio de 4.5
  texels sangra para a cascata vizinha no atlas 2×2.
- **Cutout no pass de sombra** (`shadow.ps`): a folhagem da Sponza projectava
  rectângulos. Uma partição só (`alpha_cutoff > 0 || double_sided`) serve o pass
  de cor e o de sombra.

Fora da barra, por decisão dela: VSM, ESM, contact shadows, toroidal, octa points
(não há point lights). O bloco de sombra é partilhado à letra entre `gate-csm` e
a Sponza — se um mudar, o outro muda.

## D19 — Atmosfera é Hillaire 2020, não Bruneton

Desvio consciente ao roadmap §5 («Bruneton LUTs baked na CPU no init»).

- Mesma física, contabilidade diferente: LUTs pequenas refeitas **todos os frames**
  em vez de uma tabela 4D grande cozida no init. A hora do dia fica livre — que é
  o que uma engine com clima precisa. O `gate-sky` faz o sol descer ao longo dos
  frames para o provar.
- Encaixa no que já existe: a aerial perspective do Hillaire é um volume de
  froxels 32³, exactamente o recurso que o fog trouxe (D16).
- **Bruneton não é nuvens.** As nuvens continuam Schneider/Nubis como o §9 diz —
  Perlin-Worley 128³ + Worley 32³, ray march, meia resolução, reprojecção
  temporal. São dois sistemas.
- LUTs 2D saem por **passes fullscreen**, não compute: são 2D, um output por
  texel, sem partilha de grupo. Só a aerial perspective (3D) precisa de UAV.
- **Fechado para o céu**: transmittance (256×64) → multiscattering (32×32) →
  sky-view (192×108) → composite. O composite faz **um fetch por pixel**, não um
  march. A LUT bate com o raymarch por pixel ao bit (±1 num canal no zénite) —
  é assim que se prova que a optimização é fiel.
- A LUT de multiscattering integra sobre a esfera com **um loop achatado**
  (direcção × passo, índices por `UDiv`/`UMod`, throughput a reiniciar quando o
  passo dá a volta) em vez de dois loops aninhados: mesma integral, muito menos
  SPIR-V à mão para errar. Direcções em espiral de ângulo dourado, melhor
  distribuídas que a grelha 8×8 do Hillaire para a mesma contagem.
- **Sem bounce do chão** na LUT de multiscattering (albedo 0). Falta-lhe a luz que
  volta do solo, o que aparece como um horizonte ligeiramente mais escuro.
- Falta a **aerial perspective** (froxel 32³). Não é do gate do céu — é integração
  com a cena, entra quando a Sponza receber atmosfera.

## D20 — Dependências têm fase, e `wgpu` continua fora

- A lista fechada de libs no `AGENTS.md` foi substituída por
  `docs/Rust-Rewrite-Roadmap.md` §14.1: uma tabela do que já está dentro e do que
  entra **em que fase**. Regra: nada entra fora da fase que o pede.
- **`wgpu` está fora**, e `egui-wgpu` com ele. D0 é explícita e todo o RHI,
  bindless, froxels e LUTs assumem Vulkan explícito. Editor usa
  `egui-ash-renderer`. Reabrir isto é uma sessão, não uma linha no `Cargo.toml`.
- **ECS não se escolhe agora.** Candidatos e critério em §14.1; decide-se na fase
  6 com um gate que crie 1e6 instâncias e meça.
- **Física**: o pedido foi «Jolt ou box3d». `box3d` não existe — `Box2D` é 2D. Se
  a intenção era Bullet são `bullet3-sys`. Jolt em Rust é por bindings e traz
  toolchain C++; `rapier3d` é o Rust puro. **Fica por clarificar** antes de a fase
  de física abrir.
- `rayon` entra para trabalho offline (bake de noise, clipmap, mips), **não** para
  o frame graph — esse continua single-thread por D0.

## D21 — Nuvens: Nubis a meia resolução, e o horizonte resolve-se por cobertura

- **Raymarch a meia resolução** para um RT `Rgba16Float` que leva `(scattering.rgb,
  transmitância.a)`; o composite full-res faz `céu * tr + scat`. Meia resolução é
  o padrão do Nubis e é o que torna 64 passos × 6 de luz acessível.
- Densidade à Schneider: Perlin-Worley remapeado **contra o seu próprio FBM de
  Worley**, gradiente de altura (base dura, topo mole), cobertura, e só então a
  erosão pelo volume de detalhe 32³ — mais forte na base.
- Fase **Cornette-Shanks com a normalização `3/(8π)`**. Sem ela a fase valia ~4×
  a mais e tudo saía branco lavado. Multiple scattering pela aproximação do
  Hillaire: 3 oitavas com extinção/scattering/anisotropia a decair.
- **O horizonte não se arranja com mais passos.** Num raio rasante o span é dezenas
  de km e o passo passa dos 500 m, muito acima da escala do detalhe — dá um leque
  de aliasing e depois confetti. Três medidas, por esta ordem de efeito:
  1. **cobertura a subir com a distância** (0.36 → 0.66 entre 6 e 24 km): as nuvens
     distantes fundem-se num **banco contínuo**, que é o que um horizonte real é.
  2. LOD do detalhe — a erosão do volume 32³ desvanece até 24 km e fica só a forma.
  3. span limitado a 28 km + densidade a esbater até 34 km.
  O resto do ruído que sobra é trabalho da **reprojecção temporal**, não de tuning.
- O ambiente do `CloudCb` é **radiância, não cor**: com o sol a ~3.4 o céu tem de
  ficar bem abaixo disso ou o ACES leva tudo a branco. O sample não pode
  sobrepor-se ao default do `cloud_noise.rs` — foi o que escondeu uma ronda inteira
  de afinação.

## D22 — Reprojecção das nuvens: profundidade analítica, sem alvo extra

- As nuvens não têm depth buffer e não vale a pena inventar um. A reprojecção
  intersecta o raio com o **meio da concha** e empurra esse ponto pela
  view-projection do frame anterior. É aproximado — uma nuvem não é uma
  superfície — mas o erro é de segunda ordem ao pé do movimento da câmara e
  **não custa um render target**.
- História limitada à caixa **3×3 do frame actual** antes da mistura (Karis).
  Sem isto uma câmara a rodar arrasta rastos.
- Blend 0.92 ≈ doze frames de história: chega para enterrar o jitter e é curto o
  suficiente para o clamp recuperar numa curva.
- O jitter do march tem de **mexer por frame** (rácio dourado sobre o IGN, com o
  índice do frame em `wind.w`) ou a acumulação temporal está a fazer a média das
  mesmas amostras.
- A origem do march deixou de estar presa ao eixo do planeta: era
  `(0, r0, 0)`, agora é `(cam.x, r0, cam.z)` e `|origin|²` vem do vector. Sem
  isto a câmara podia andar de lado sem se mexer no campo de nuvens — e a
  reprojecção não teria paralaxe nenhuma para corrigir.
- **Medido, não presumido:** na faixa do horizonte o |laplaciano| médio cai de
  11.99 para 6.64 (−44.6%). Com a matriz anterior trocada por identidade cai
  para −2.8%, ou seja o clamp **rejeita** a história desalinhada em vez de a
  esborratar. É esse par de números que prova que a reprojecção está viva e que
  o clamp funciona; nenhum dos dois se via a olho.

## D23 — Sombras volumétricas: o CSM entra no inject, não num pass novo

- O froxel já é marchado. Dar-lhe **uma tap de comparação no CSM** é o que
  transforma névoa uniforme em feixes — não é preciso um pass de god rays nem
  um radial blur. Uma tap, sem PCF: o froxel já é muito mais grosso que um texel
  de sombra e o integrate suaviza ao longo do raio.
- `cascade_count == 0` salta a procura inteira, por isso um sample sem atlas paga
  zero e comporta-se exactamente como antes.
- `shadow_strength` 0.85, não 1.0: um froxel na sombra ainda recebe céu e
  ressalto, e uma fronteira de feixe totalmente preta lê-se como uma aresta dura
  no ar.
- O gate partilha o `shadow.vs` do gate CSM em vez de ter uma cópia — o layout de
  vértice/instância é o mesmo e duas cópias divergem.
- O cenário do `gate-fog` passou a ter uma **grelha com folgas** (a arcada da
  Sponza reduzida ao que este gate tem). Sem um oclusor grande e com folgas não
  há feixes nenhuns: com as esferas soltas só 4.6% dos froxels ficavam na sombra
  e o efeito não chegava a 7/255.

## D24 — A fase de Henyey-Greenstein estava com o sinal trocado

- O ângulo da HG é entre a **direcção de propagação** da luz (sol → froxel) e a
  direcção dispersada (froxel → olho). No inject os dois vectores partiam do
  froxel, portanto o produto escalar era o **simétrico** desse cosseno.
- Consequência: o lóbulo para a frente caía **atrás** da câmara. Olhar para um sol
  baixo através de neblina ficava escuro, que é o contrário do que a natureza faz
  — e tornava os feixes impossíveis.
- Não se via como bug: com o sol atrás da câmara a névoa até parecia bem.
  Apareceu ao perguntar *porque é que o efeito é tão fraco* e ir ver os números,
  não a imagem.
- **As nuvens não têm este bug**: lá o `dir` é olho → ponto, e `dot(dir, sun)` já é
  o cosseno certo. A diferença é só qual das duas pontas o vector aponta — que é
  precisamente por isso que este erro passa despercebido.

## D25 — A Sponza passou a HDR linear, e o tonemap mudou de sítio

- O `color.ps` deixou de fazer ACES + sRGB. O alvo de cena é **`Rgba16Float`
  linear** e quem faz o tonemap é o **apply do fog** — a névoa tem de ser composta
  em luz, não por cima de uma imagem já codificada. Compor fog depois do tonemap
  é o erro clássico que dá halos e uma cena leitosa.
- O `color.ps` ganhou um segundo MRT com o **view depth** (`clip.w`, positivo com
  `perspective_vk`), que é como o apply escolhe a slice do froxel. O clear desse
  alvo é o plano longe do fog, senão o céu não leva marcha nenhuma.
- Os shaders do froxel **não foram copiados**: o `build.rs` da Sponza aponta para
  `../../gates/fog/shaders/`. Uma segunda cópia do inject/integrate/apply seria
  uma segunda implementação para manter em dia.
- Fog **default-on** (§15 fase 5 exige-o). O inject lê as mesmas cascatas que a
  cena, por isso os feixes pela arcada não custam um pass.
- Medido: ligar as sombras volumétricas muda **50.7%** dos pixels em mais de
  8/255 (média 12.9, máximo 59). No `gate-fog` é 20.2% — a Sponza tem muito mais
  geometria a tapar o sol, que é exactamente o ponto.

## D26 — Água: Gerstner primeiro, SSR depois

- **Gerstner numa grelha**, não um height map: a onda desloca vértices em X e Z,
  não só em Y, e é isso que dá a crista afiada e o vale largo em vez de um
  seno. A superfície só é tão detalhada como a grelha (192² quads, 73k triângulos)
  — Gerstner desloca, não tessela.
- Cada onda viaja à velocidade de água funda `sqrt(g/k)`. Sem a dispersão as
  quatro ondas andam à mesma velocidade e a superfície lê-se como uma chapa
  rígida a deslizar. Há teste que fixa `omega(34 m) ≈ 1.345 rad/s`.
- **Normal pela derivada analítica** da mesma soma. Tirá-la dos vizinhos custava
  o motivo de fazer isto no VS.
- **Sem culling**: com esteepness alta a crista dobra e o culling abriria buracos
  exactamente onde a onda é mais interessante.
- **Sem SSR nesta fatia.** A reflexão é o céu analítico. Aos ângulos rasantes,
  onde Fresnel domina, o que uma superfície real reflecte é sobretudo céu — por
  isso isto é a aproximação honesta e não um placeholder. SSR (point-sample do
  depth, §5) é a fatia seguinte.
- Corpo de água por **Beer-Lambert sobre o caminho até ao fundo** (`seabed/N·V`):
  a olhar a direito vê-se o tom raso, rasante vê-se o fundo. É o que faz o
  primeiro plano escuro e o horizonte claro sem nenhum truque.
- Céu e água escrevem **radiância linear** no mesmo pass HDR (o céu primeiro, sem
  depth; a superfície por cima, com depth) e o tonemap é um pass fullscreen no
  fim. Pôr o céu num pass próprio obrigava o RHI a fazer LOAD de um attachment em
  vez de CLEAR — não vale a pena por isto.
- `pow(cos θ, 90)` para o brilho do sol ainda vale 25% a 10° do disco: um sol do
  tamanho de um punho. 900 põe-no onde deve estar.

## D27 — Câmara e input: os gates continuam a não ver nada

- `Input` vive em **`harpia-core`** e não sabe o que é winit. O `app` (que tem
  winit) preenche-o, o `render` (que tem a `Camera`) lê-o, e nenhum dos dois
  precisa de depender do outro.
- **Gates nunca recebem input, e o `dt` é fixo em `1/60`.** Um sample com
  `--frames N` é uma medição: uma tecla premida a meio de um `--capture` mudava
  em surdina os pixels pelos quais ele é julgado, e o `dt` de relógio fazia cada
  captura depender de quão ocupada estava a máquina. Só `--interactive` tem
  relógio e input a sério (com `clamp` de 1 ms a 100 ms, para um breakpoint não
  teletransportar a câmara).
- `Sample::update(&Input, dt)` é um método com **implementação vazia por
  omissão**: separa simulação de render sem obrigar nenhum dos 10 samples
  existentes a mudar uma linha.
- Olhar exige o **botão direito** premido. Agarrar o cursor sem pedir torna a
  janela impossível de largar, e uma janela de gate que engole o ponteiro é pior
  do que uma que o ignora.
- `WindowEvent::Focused(false)` limpa o estado: uma tecla largada com outra
  janela em foco nunca reporta o release, e sem isto a câmara continua a voar
  depois de um alt-tab.
- Delta do rato vem de `DeviceEvent::MouseMotion`, **não** de `CursorMoved`: a
  posição do cursor encosta à borda da janela e o arrasto deixava de rodar.
- **Medido:** 18 das 22 capturas de referência ficaram **bit-identical**. As duas
  amostras convertidas (Sponza, água) diferem em **12 e 2 pixels** de 921 600 —
  todos em cima do limiar da comparação de sombra, onde um ulp na direcção faz o
  snap da cascata cair para o outro texel. E provei que o `update()` não é
  canalização morta: a subir a câmara 1 unidade/s mudam 515 740 pixels.

## D28 — ECS: `bevy_ecs` é a recomendação, o gate é que decide (D20 continua aberta)

Investigado a 2026-09-10, com o WebSearch em baixo — números tirados directamente
do crates.io, não de memória:

| crate | versão | downloads recentes | actualizado |
|---|---|---|---|
| `bevy_ecs` | 0.19.1 | 1 841 616 | 2026-08-13 |
| `hecs` | 0.11.1 | 119 304 | 2026-07-28 |
| `shipyard` | 0.11.5 | 19 578 | 2026-07-10 |
| `flecs_ecs` | 0.2.2 | 1 385 | 2025-11-17 |
| `evenio` | 0.6.0 | 448 | 2024-05-19 |

- **O medo da D0 não se confirma:** as dependências não-opcionais do `bevy_ecs`
  0.19.1 são 18, todas utilitárias (`bevy_platform`, `bevy_ptr`, `bevy_tasks`,
  `arrayvec`, `bitflags`, `smallvec`, …). **Nenhum `wgpu`, nenhum `winit`,
  nenhum crate de gráficos.** O `wgpu` mora no `bevy_render`, que não vem atrás.
  Era esta a única razão séria para o excluir.
- **Não existe benchmark cross-ECS mantido:** o `ecs_bench_suite` do rust-gamedev
  está arquivado desde Nov 2022. Ou seja, o gate de 1e6 instâncias que a D20 já
  exige não é zelo — é a única forma de decidir com números.
- Recomendação: `bevy_ecs`, com `hecs` (3 dependências contra 18) como plano B se
  o peso incomodar. `flecs_ecs` fora: parado há 10 meses e traz toolchain C++, o
  mesmo motivo que travou o Jolt.
- **A decisão continua por fechar** — abre-se na fase 6 com o gate a medir.

## D29 — SSR da água: marcha no mundo, espessura que acompanha o passo

- A marcha é **no espaço do mundo**, projectando cada passo de volta ao ecrã, e
  não em espaço de ecrã. Marchar em ecrã exige montar o raio por pixel para
  manter o passo uniforme; marchar o raio do mundo é uma multiplicação de matriz
  por passo e acerta na comparação de profundidade a qualquer ângulo.
- **Sem saída antecipada.** Um `break` de dentro de um ciclo é onde o SPIR-V à
  mão se estraga; carregar «já acertou» no phi custa uns passos que o GPU ia
  correr na mesma pelo resto da wavefront.
- **A espessura fixa foi o erro que deu anéis em vez de reflexos.** Com passo de
  3.2 unidades e janela de 0.9, só as silhuetas acertavam: o interior da rocha
  está muito mais perto do que o raio nesse passo. A janela tem de cobrir **um
  passo inteiro** (a marcha ultrapassa até uma passada) **e crescer com a
  distância** (um passo longe abrange mais profundidade que um perto).
- Falhar o ecrã cai para o céu analítico. Aos ângulos rasantes é ao mesmo tempo o
  que o SSR não consegue ver e o que uma superfície real reflecte ali.
- A água precisa de **dois alvos HDR**: amostra o que reflecte, portanto não pode
  estar a escrever para lá. Uma cópia fullscreen é mais barata do que ensinar o
  RHI a fazer LOAD de um attachment.
- **Espuma de crista era código morto.** Com `Q = 0.62` e estas amplitudes,
  `Σ Q·k·A ≈ 0.22`, logo o termo de Jacobiano nunca sai de `[0.78, 1.22]` — e o
  limiar estava em 0.38. Estas ondas **não rebentam**; o que se mede agora é
  «quão perto de dobrar», com ganho para se ver nas cristas mais afiadas. Isto é
  uma escolha de arte assumida, não física.
- Espuma é luz dispersa, não um realce: a 1.15 linear ficava uma laje branca ao
  lado de água nos 0.2.
- **Medido:** ligar o SSR muda **3.7%** dos pixels em mais de 8/255 (máximo 157) —
  forte e localizado exactamente onde estão os reflexos.

## D30 — Chuva: material molhado + post, e o bug de offsets que quase passou

- Duas metades, como o roadmap manda: **material molhado** (albedo escurece
  *e* o lóbulo especular aperta — fazer só uma das duas dá geada ou verniz) e
  **bátegas em espaço de ecrã**. Uma gota atravessa o ecrã em meia dúzia de
  frames e nunca se vê parada: geometria custava muito para mostrar o que
  ninguém consegue resolver.
- Superfícies viradas para cima acumulam água: a normal é puxada para cima e
  leva **ondulações**, um anel por célula do mundo numa vizinhança 2×2. Uma só
  célula corta os anéis na fronteira e lê-se como uma grelha assim que se repara.
- As ondulações **desvanecem com a distância**: um anel a 100 m é menor que um
  pixel e só compra aliasing.
- Três camadas de bátegas. Uma só lê-se como uma cortina repetida; três a escalas
  e velocidades diferentes não, e é muito mais barato do que mais amostras.
- Bátegas são **aditivas**: a chuva dispersa luz do céu para o olho, não tapa a
  cena — veda-a.
- O composite é um alvo a sério e não a swapchain directamente: o `--capture` não
  lê a swapchain, e uma captura sem bátegas não julga este gate.
- **Medido:** ligar a chuva muda **70.5%** dos pixels em mais de 8/255.

### O bug que interessa registar

Gerei as decorações do bloco com `112 + i*16` em vez de `96 + i*16`, portanto
**todos os `%v4` ficaram declarados 16 bytes à frente**: o shader lia `ripple`
onde queria `streak`. Não houve **um único erro de validation** — o bloco tem o
tamanho certo, a GPU só lia os bytes errados — e o sintoma foi uma faixa branca
saturada à direita do ecrã, que não parece um problema de layout.

Encontrei-o a despejar os valores intermédios num alvo e a olhar para os números
(o hash da coluna tinha só dois valores, o que só acontece se o multiplicador
fosse ~1.35 em vez de 28 — e 1.35 é `ripple.x`). O teste
`rain::cb_layout_matches_the_spvasm` **apanha-o**: confirmei a pôr o offset errado
outra vez, e falha a dizer o ficheiro, o membro e os dois offsets. Foi exactamente
para isto que a D-anterior ligou os testes aos `.spvasm`.

## D31 — Mapa de chuva: é o que impede que chova dentro da arcada

- Bátegas em espaço de ecrã **não sabem que existe um telhado**. Na Sponza isso
  seria chuva a cair através da pedra — um bug óbvio a olho, e o motivo pelo qual
  quase adiei a chuva na cena de referência.
- Solução padrão e barata: **um depth top-down** (1024², ortográfico) sobre a
  geometria. Um pixel cuja superfície não é a coisa mais alta naquela posição está
  sob cobertura. Reutiliza o `shadow.vs` e o PSO depth-only que já existiam — o
  pass extra é uma passagem de profundidade, não um sistema novo.
- A olhar de cima o `up` degenera, por isso a base usa `-Z`.
- A máscara usa a **superfície atrás do pixel**, não o ar à frente dela. É
  aproximado — uma bátega no ar em frente a uma parede coberta é suprimida — mas é
  o que os jogos fazem, e funciona porque o telhado está por cima de ambos.
- Fora do mapa é céu aberto, não telhado. Sem isto o mundo inteiro para lá do
  mapa deixava de ter chuva.
- **Medido na Sponza:** delta médio das bátegas na nave aberta **1.925**, sob a
  arcada esquerda **0.061**, sob a direita **0.000**. A chuva não atravessa a
  pedra.
- `rain.ps` passou a escrever **linear** e o tonemap mudou para o blit do gate. É
  isso que permite a Sponza intercalar o mesmo pass antes do composite do fog, em
  vez de ter uma segunda cópia do shader.

## D32 — Fase 5 fechada: o que entra na Sponza e o que não entra, e porquê

O critério era «default-on na Sponza ou o pass não entra». Resultado honesto:

- **Fog: dentro** (D25), com sombras volumétricas (D23).
- **Chuva: dentro** (D31), mascarada pelo mapa de chuva.
- **Céu Hillaire: fora.** A Sponza é um interior; pelas aberturas vê-se uma
  fracção do ecrã. O pass é verde no `gate-sky` e entra com o terreno na fase 6,
  que é onde o céu se vê.
- **Nuvens: fora**, pela mesma razão, e porque a sombra das nuvens precisa de
  chão onde cair.
- **Água: fora.** Não há água na Sponza. O `gate-water` é a prova.

Isto não é uma excepção ao critério: o critério existe para impedir passes que só
funcionam isolados. Fog e chuva estão integrados e medidos na cena de referência;
os outros dois têm gates verdes e um sítio marcado na fase 6.

## D33 — Correcção: o Box3D existe, e a D20 estava errada nessa parte

Disse duas vezes que «`box3d` não existe». **Errado.** O Erin Catto (autor do
Box2D) lançou o **Box3D** em **Junho de 2026** — C17, MIT,
`github.com/erincatto/box3d`: CCD, solver «Soft Step», hulls/cápsulas/esferas/
malhas/height fields, joints com limites e molas, SIMD multithreaded e
**determinismo cross-platform**. Saiu depois do meu conhecimento, o que explica o
erro mas não o desculpa: a verificação era uma pesquisa.

Recomendação com os factos certos: **`rapier3d` para arrancar** (Rust puro, zero
toolchain externa, e a física não é o gargalo agora), **reavaliar Box3D quando
houver rede ou replay**, porque aí o determinismo cross-platform deixa de ser
luxo. O Jolt continua a ser a opção madura em C++ se a toolchain deixar de ser um
problema. Decisão do Bruno; a D20 fica corrigida nesta parte.

## D34 — Demos compilados numa pasta

`prog/tools/build_demos.sh` compila em release e junta os 11 samples em `demos/`
com nomes que dizem para que servem (`03-luz-pbr`, `04-sombras`, `sponza`, …),
mais um README. Os gates já eram um-por-funcionalidade — isto é empacotamento,
não código novo. A pasta está no `.gitignore`; o script é que é commitado.

## D35 — Fase 5.5: o toolchain sempre esteve cá

- `glslang-tools` 15.1.0 e `spirv-tools` 2025.1 estavam **instalados desde
  Maio/Junho de 2026**. A premissa «não há DXC, logo escrevemos `.spvasm`» era
  verdade sobre o DXC e falsa sobre tudo o resto. O `spirv-as` real assembla e
  valida os 54 shaders da árvore.
- **`spirv-val` passou a correr no build.** Apanhou logo um bug a sério que a
  camada de validação em runtime **deixava passar**: no `water.ps` havia um
  `OpSampledImage` consumido noutro bloco. O gate corria verde porque a RADV
  tolera. Corrigido; a imagem ficou bit-idêntica.
- Os 11 `build.rs` duplicados deram lugar a `harpia-shader-build`, que aceita
  `.spvasm` (via `spirv-as`) **e** `.glsl` (via `glslangValidator`), valida sempre,
  e só cai para o Python com um aviso ruidoso.
- GLSL fica drop-in: o glslang renomeia o entry point para `VSMain`/`PSMain`/
  `CSMain`, portanto um `.glsl` substitui um `.spvasm` sem tocar no PSO.
- **Gate cumprido:** `blit.ps` da chuva reescrito em GLSL, e o composite é
  **bit-idêntico** ao do assembly que substituiu.
- Um shader GLSL declara só os membros que lê, com offsets explícitos, portanto
  os *índices* de membro não têm significado entre as duas formas — os **offsets**
  têm. `assert_glsl_offsets` verifica isso e mantém o GLSL dentro da mesma rede.

## D36 — A engine passou a saber quanto custa

- Present mode configurável (`--vsync 0` → MAILBOX/IMMEDIATE). Com FIFO fixo,
  **todas** as medições davam o refresh do monitor: era por isso que a Sponza e
  as nuvens «custavam» exactamente os mesmos 16.4 ms.
- `VkQueryPool` de timestamps por slot de frame, lidos quando o slot volta a dar
  a volta — nessa altura já se esperou pela fence, portanto os resultados existem
  sem bloquear.
- `gpu.mark("nome")` marca o command stream; o relatório mostra o intervalo desde
  a marca anterior. Contadores de draws, dispatches e triângulos vêm de graça.
- `--stats` descarta os primeiros 8 frames (criação de pipelines, primeiros
  uploads) para a média não ficar a mentir sobre o regime estável.
- **O primeiro número real desta engine:** Sponza a `--vsync 0` custa
  **1.569 ms de GPU** (621 draws, 1.57 M triângulos), repartidos em cascatas
  0.433 · cena 0.770 · froxels do fog 0.110 · rain map 0.082 · chuva 0.082 ·
  fog apply 0.052. As **cascatas custam mais de metade do que a cena inteira** —
  informação accionável que era impossível de obter antes.

## D37 — Um check de sweep que dava falso verde

O meu sweep procurava `test result: FAILED` na saída dos testes. Quando os testes
**não compilavam**, não havia linha nenhuma e o sweep dizia «TESTES OK». Aconteceu
mesmo: os literais de `DeviceDesc` nos `#[cfg(test)]` do `harpia-rhi` ficaram sem
o campo `vsync` novo e passaram despercebidos. Um check de verificação tem de
falhar quando **não há** resultado, não só quando o resultado é mau.

## D38 — ECS decidido: **`bevy_ecs`**, e com números (fecha a D20)

`gate-ecs`, 1e6 entidades, release, três corridas estáveis:

| | `bevy_ecs` | `hecs` | rácio |
|---|---|---|---|
| spawn | 58–72 ms | 29–31 ms | **hecs 2.0–2.4× mais rápido** |
| **iterate** (por frame) | 0.88–1.39 ms | 1.10–1.68 ms | **bevy 15–22% mais rápido** |
| fragmented (query esparsa) | 0.08–0.12 ms | 0.12 ms | bevy 6–32% mais rápido |
| churn (despawn+spawn) | 9.3–11.3 ms | 7.4–8.8 ms | hecs 1.25–1.29× mais rápido |

Os checksums dos dois mundos batem (4.453e8), portanto fizeram a mesma
aritmética — sem isso a comparação era entre dois programas diferentes.

**Escolha: `bevy_ecs`.** O `iterate` é o caso quente — acontece todos os frames,
e é lá que ganha. O `hecs` ganha na construção e no churn, que acontecem muito
menos: 58 ms para 1e6 são 58 ns por entidade, portanto um chunk de streaming de
10 k entidades custa 0.6 ms, o que não é um problema.

Somando ao que a D28 já tinha: 1.84 M downloads recentes, actualizado em Ago 2026,
e **sem `wgpu` nem `winit`** nas dependências (confirmado outra vez com
`cargo tree`: zero ocorrências). A D0 mantém-se.

`hecs` fica como plano B documentado se as 18 dependências transitivas vierem a
incomodar, ou se o perfil mudar para um mundo de churn muito alto.

## D39 — A Sponza passou a entidades, e o culling ganhou o seu lugar

- `harpia-scene` (novo crate): `Mesh`, `Material`, `WorldTransform`, `Bounds`,
  `TwoSided`, `Visible`, mais `Frustum` e o sistema `cull_to_frustum`. Não sabe
  nada de Vulkan — guarda os handles opacos que o RHI devolve.
- A Sponza deixou de ter `Vec<GpuPrim>` e um `usize` a marcar onde começavam as
  primitivas de duas faces. `TwoSided` é uma componente e a partição é uma query.
  **Não havia onde pendurar uma bounding box**, e é por isso que não havia culling.
- As caixas são calculadas no `init`, que é o último sítio onde os vértices ainda
  existem em CPU: depois disso só há um handle de buffer.
- **O pass de sombra e o mapa de chuva não respeitam o `Visible`.** Um caster fora
  do ecrã continua a projectar sombra para dentro dele; cortá-lo faria a sombra
  desaparecer. Só o pass de cor é cullado.
- Frustum por Gribb-Hartmann a partir da view-projection, o que o faz funcionar
  com a `perspective_vk` sem casos especiais — o flip de Y e o depth em `[0,1]` já
  estão dentro da matriz. O plano near é só `r2`, não `r3 + r2`, precisamente por
  o depth ir de 0 a 1.
- O teste é conservador de propósito: «está inteiramente fora de algum plano?».
  Um falso positivo custa um draw, um falso negativo faz geometria desaparecer.

**Medido, `--vsync 0 --stats`:**

| | sem culling | com culling |
|---|---|---|
| draws | 621 | **596** |
| GPU frame | 1.353 ms | **1.218 ms** |
| pass da cena | 0.662 ms | **0.598 ms** |

78 das 103 primitivas visíveis. E o que prova que está certo: a imagem é
**bit-idêntica** com e sem culling. Nada visível foi cortado.

O ganho é modesto **nesta cena** porque a câmara vê quase todo o átrio — é o
mecanismo que interessa, e ele passa a existir para o terreno da fase 6, onde a
maior parte do mundo está sempre fora do ecrã.

