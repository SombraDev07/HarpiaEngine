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

