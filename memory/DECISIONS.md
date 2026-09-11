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

## D40 — Terreno: clipmap por SV_VertexID, e três bugs que valem a pena

O roadmap manda «só `ClipmapTerrain` SV_VertexID», e a razão é boa: a malha de um
clipmap é sempre a mesma grelha, só muda de escala e de sítio. **Zero vertex
buffers, zero index buffers.** Um `draw` de 24 576 vértices × 7 instâncias — uma
por nível — e o VS deriva tudo dos índices.

**Medido:** 7 níveis, alcance 1024 unidades, **2 draws**, 57 345 triângulos,
**0.077 ms** de GPU no pass do terreno (0.109 ms de frame). Este é o argumento
todo a favor do SV_VertexID.

A altura é a **mesma função** em `harpia_render::terrain_height` e no GLSL, linha
a linha. A câmara do gate usa a versão CPU para não atravessar o chão que o VS
desenha; se divergirem vê-se logo, e é o ensaio do gate `heightquery`.

### Os três bugs

1. **Winding invertido.** `(0,0) (1,0) (0,1)` parece a ordem óbvia, mas num plano
   XZ o produto vectorial de +X com +Z aponta para **baixo**: com `cull_back` o
   terreno inteiro era cortado. 89% do ecrã era exactamente a cor do clear, e eu
   li as manchas pálidas como montanhas nevadas até contar as cores e ver que era
   céu. **Contar pixels desmentiu o que os olhos diziam.**
2. **Snap por nível abre fendas.** Cada nível a fazer snap à sua própria célula
   alinha à sua própria grelha, e a fronteira com o vizinho fica deslocada. Snap
   ao **dobro** da célula ajuda, mas não chega: dois níveis vizinhos não podem
   alinhar sempre. Um clipmap completo resolve isto com uma tira de recorte em L
   de tamanho variável; aqui há uma **saia** na fronteira interior de cada anel,
   que faz o mesmo em duas linhas ao custo de uma dobra a rasar o chão.
3. **Fade linear a partir de zero.** Um vale a 600 unidades ficava meio céu e
   lia-se como terreno em falta. Agora é `smoothstep(0.55·far, far)`.

## D41 — `fract(sin(x) * 43758.5)` não é portável, e o gate provou-o

O gate `heightquery` avalia a mesma função de altura em CPU e em GPU sobre uma
grelha de 256² pontos, lê o resultado de volta e compara ponto a ponto.

**Com o hash trigonométrico** — o mais copiado da internet:

```
diff média 17.23 m · diff máxima 76.92 m · 99.84% dos pontos fora da tolerância
pior ponto: CPU −45.06 m, GPU +31.85 m
```

Num terreno de ±60 m, a CPU e a GPU estavam a calcular **superfícies
completamente diferentes**. O terreno desenhado parecia perfeito; a colisão é que
teria sido noutro sítio. Isso apareceria como um bug de física — o jogador a
atravessar o chão ou a andar no ar — e ninguém iria procurar num hash.

A causa: `sin` de um argumento grande difere no último bit entre a libm da CPU e o
hardware da GPU, e o factor 43758 amplifica essa diferença até a parte
fraccionária ser outra.

**Com um hash inteiro** (xorshift-multiply sobre as coordenadas de célula):

```
diff média 0.000008 m · diff máxima 0.000185 m · 0% fora da tolerância
```

O que sobra (0.19 mm) é contracção FMA na soma das oitavas: a GPU pode fundir
multiplicação e adição, o que dá um resultado *mais* exacto e por isso diferente.
É ruído do último bit e não move nada.

**Regra:** qualquer função que a CPU e a GPU tenham de partilhar usa aritmética
**inteira** no hash. Há um teste (`hash_is_integer_and_pinned`) que trava isto sem
precisar de GPU.

## D42 — `Sample::finish`, para um gate poder medir

`read_texture` espera pelo device e é ilegal a meio de um frame, portanto não
havia sítio onde um gate lesse resultados de volta. `Sample::finish(&mut Gpu)`
corre depois do último frame; devolver `Err` faz o sample sair ≠ 0, que é o que
transforma uma medição num gate.

## D43 — Comparação com a Dagor, e o que ela mudou no plano

`docs/Harpia-vs-Dagor.md` tem a análise completa, lida no código e não de memória.
A Dagor está em `DagorEngine/` (84 GB, BSD-3 da Gaijin) e está **no `.gitignore`** —
nunca entra no repo. Implementa-se a partir da técnica, não se transplanta código.

O que mudou no que eu pensava:

- **A Dagor também não usa vertex buffer no terreno** (`setvsrc_ex(0, NULL, 0, 0)`).
  A ideia do `SV_VertexID` não é nossa e eles chegaram lá primeiro. Usam índices,
  patches, culling por patch e tesselação por hardware no LOD0 — tudo o que nós
  não temos.
- **O núcleo do BRDF deles não tem magia**: o `optimized-ggx.hlsl` é do John Hable,
  domínio público, e a biblioteca inteira são **285 linhas**. O nosso BRDF aguenta
  a comparação. Volume não era a história.
- **A distância que dói não é de shading: é que só temos uma luz.** Eles têm
  clustered (omni + spot) com sombras por prioridade e orçamento por frame. Nenhum
  detalhe de BRDF chega perto disto.
- Eles ganham decisivamente em frame graph (161 ficheiros de `daFrameGraph`), GI
  (`daGI2`: voxel + radiance cache), sombras toroidais e FSR2.
- **Onde estamos à frente e vale a pena preservar:** o `heightquery` compara CPU
  contra GPU e falha se divergirem. Procurei um equivalente do lado deles e um
  teste de furnace, e não encontrei nenhum. A classe de bug que nos deu 77 m de
  erro é invisível sem esse arnês.

Prioridades que saem daqui, por esta ordem: **Smith height-correlated** (barato e
estritamente melhor), **clustered lights** (a maior distância), **culling em
compute para o terreno** (onde podemos genuinamente passar à frente, porque eles
fazem-no em CPU), **render graph**.

## D44 — White furnace: o nosso G perdia 31 pontos de energia num quase-espelho

Primeiro item do roadmap da D43, e mede-se em vez de se opinar.

`gate-furnace` calcula o **albedo direccional** `E(NoV, α) = ∫ f·NoL dl` com `F = 1`
por importance sampling da GGX, 1024 amostras, numa grelha 64×64. Num BRDF que não
absorve, `E` **tem** de ser 1.0.

| rugosidade | Schlick-k (o que tínhamos) | height-correlated | compensado |
|---|---|---|---|
| 0.070 | **0.6922** | 0.9986 | 1.0000 |
| 0.320 | 0.6830 | 0.9538 | 0.9999 |
| 0.570 | 0.6519 | 0.8361 | 0.9998 |
| 0.945 | 0.4640 | 0.5414 | 0.9999 |

Três leituras:

1. **O `k = (α+1)²/8` do UE4 perdia 37% de energia em média**, e — o que é pior —
   **31 pontos percentuais a 0.07 de rugosidade**, que é praticamente um espelho e
   devia reflectir quase tudo. Um erro que *aumenta* quando a superfície fica mais
   lisa não é um erro benigno. Aquele `k` foi ajustado para luzes analíticas, e
   estava a ser usado como se fosse a forma correcta.
2. **O height-correlated dá 0.9986 a baixa rugosidade** e perde energia
   progressivamente à medida que a superfície fica áspera — o que é *física
   correcta*: um único salto entre microfacetas perde mesmo energia.
3. **A compensação de multiscatter está bem ligada**: desvio máximo de **0.0005**
   em 4096 células. Isto valida código que existia e que ninguém tinha verificado.

Aplicado ao `lighting.ps`. Em pixels, no `gate-pbr-grid`: média 0.09, **máximo
165/255**, em 0.1% dos pixels. Pouca área porque a cena é dominada por IBL e
difuso e o especular de **um** sol cobre pouco — mas onde cai, muda muito. Os dois
números juntos é que contam a história: o erro do modelo era grande, o palco onde
se vê é que é pequeno, **e o palco é pequeno porque só temos uma luz**.

O `gate-pbr-grid` passou a ter alvo `lit` capturável. Sem isso era o único gate
cuja saída não se podia medir em pixels, o que contradizia a disciplina do
projecto.

## D45 — Storage buffers no set 3

O `set3` estava vazio desde a fase 0 e era o que faltava para qualquer coisa
GPU-driven. Agora é um **array de 16 storage buffers**, indexado por slot tal como
o heap de texturas é indexado por índice — três tipos de bloco diferentes sobre o
mesmo binding, que é o mesmo padrão.

Host-visible de propósito: o caso que isto serve são listas reescritas todos os
frames (luzes, clusters, mais tarde argumentos indirectos). Device-local exigia
staging e um copy por frame para ganhar banda que listas de dezenas de KB não
usam.

Desbloqueia também os draws indirectos da fase 6.5.

## D46 — Clustered lights, e o gate de correcção apanhou um bug meu

Fecha a maior distância medida contra a Dagor (D43): a engine tinha **uma** luz.

Grelha de 16×9×24 = 3456 clusters, Z exponencial como nos froxels do fog. Cada
cluster guarda um intervalo numa lista plana de índices; um pixel encontra o seu
cluster pela posição no ecrã e pela profundidade de vista.

**O mesmo shader faz clustered e força-bruta**, com um interruptor no CB. É
deliberado: se fossem shaders diferentes, uma divergência podia ser diferença de
código em vez de erro de clustering. Sendo o mesmo corpo com uma lista diferente,
qualquer diferença na imagem é da grelha.

### O bug que isto apanhou

Primeira corrida: **1.68% dos canais diferentes da força-bruta, até 6345 ULP.**

A caixa em XY estava a ser calculada só à profundidade **mais próxima** da esfera.
Parece o conservador — é aí que o frustum é mais estreito, logo a mesma distância
no mundo cobre mais ecrã — mas também empurra o **centro** para fora. Para uma luz
descentrada isso desloca o intervalo inteiro e perde as células do lado de dentro.
Aparece como um candeeiro que não ilumina a parede ao lado: fácil de não reparar,
e impossível de atribuir à causa sem um arnês.

Calculando nas duas profundidades e unindo: **0 canais diferentes, 0 ULP.** A
imagem clustered é bit-idêntica à força-bruta. Há um teste
(`coverage_contains_every_cluster_the_sphere_reaches`) que trava isto sem GPU.

### Os números

| | |
|---|---|
| clustered | **0.294 ms** |
| força-bruta | 0.917 ms |
| ganho | **3.1×**, com imagem idêntica |

1000 luzes, 189 esferas, 3456 clusters. A atribuição é em CPU por agora; passa a
compute quando os draws indirectos chegarem, e **este gate é que vai provar que a
versão em compute continua a concordar**.

## D47 — Culling em compute e um draw indirecto: onde isto realmente paga

O gate `instances` põe as instâncias num storage buffer uma vez, no arranque, e a
CPU nunca mais lhes toca. Todos os frames um compute testa cada uma contra os seis
planos, escreve a lista dos sobreviventes e enche o `instanceCount` do comando. A
CPU submete um `vkCmdDrawIndirect` e **nunca chega a saber quantas instâncias foram
desenhadas** — o contador `draws=1, triangles=0` do `--stats` diz exactamente isso,
e é honesto: quem decidiu foi a GPU.

É o ponto 7.3 da comparação com a Dagor. Eles fazem o culling dos patches do
terreno em CPU e depois montam lotes de draws instanciados com os parâmetros em
constantes de VS. Funciona, e tem um custo por instância do lado da CPU.

**Indirecto não indexado, não indexado por engano.** A primeira versão usava
`vkCmdDrawIndexedIndirect` e a validation apanhou-a: VUID-...-07312, é preciso um
index buffer ligado. Geometria que nasce do `gl_VertexIndex` não tem nenhum para
ligar — e o clipmap do terreno é exactamente esse caso. Ficaram as duas no RHI:
`draw_indirect` (quatro u32) para geometria procedural, `draw_indexed_indirect`
(cinco u32) para malhas a sério.

### A medição, que é o que interessa

`-- --cpu-cull` faz o mesmo ecrã com o culling do lado da CPU: percorre as
instâncias, monta a lista, envia-a, e faz um draw instanciado normal. Os dois modos
concordam no número de visíveis em **todas** as escalas, o que é a prova de que se
está a comparar a mesma coisa.

| cubos | CPU (compute) | CPU (`--cpu-cull`) | visíveis, os dois modos |
|---|---|---|---|
| 2 500 | 0.17 ms | 0.17 ms | 499 |
| 25 000 | 0.17 ms | 0.23 ms | 4 923 |
| 100 000 | 0.16 ms | 0.44 ms | 19 701 |
| 400 000 | 0.18 ms | 1.14 ms | 78 883 |
| 1 000 000 | **0.17 ms** | **2.72 ms** | 197 164 |

`cpu_min_ms`, 300 frames, `--vsync 0`, RX 6700. O `cpu_avg` do caminho compute sobe
com a escala mas isso é a CPU a esperar pela fence, não trabalho dela; o mínimo é
que mede o lado da CPU.

**A leitura: de 2 500 para 1 000 000 de instâncias — 400× — o custo de CPU não
muda.** 0.17 ms nas duas pontas. O outro caminho cresce linearmente e chega a
16×. A 2 500 não há diferença nenhuma, e dizer que havia seria inventar: a
arquitectura só paga a partir de umas dezenas de milhar.

### A prova de que o culling está certo

Não basta o contador. O `finish` lê os dois buffers e exige:

- o **mesmo** teste em CPU dá exactamente o mesmo número (496 = 496, não «parecido»);
- a caixa envolvente, que contém a esfera, dá um majorante (506 ≥ 496);
- os campos fixos do comando estão como o compute os escreveu;
- cada id da lista existe, passa o teste, e **aparece uma só vez** — ids repetidos
  seriam um `atomicAdd` a dar o mesmo slot a duas threads.

A primeira versão comparava esfera (GPU) com caixa (CPU) e dava 496 contra 506.
Passava, e não provava nada: qualquer erro cabia na folga entre os dois testes.
Comparar contra um teste **diferente** é comparar contra nada.

Controlo negativo corrido: com o raio a 0.3 em vez de 1.732 o gate falha com
exit 1 e diz porquê.

## D48 — Terreno GPU-driven: o culling de patches só paga depois de deixar de recalcular

Cada nível do clipmap passou a 64 patches de 8×8 células, e o culling é em
compute: um workgroup por patch, a caixa envolvente contra os seis planos, a lista
dos sobreviventes e o `instanceCount` escritos pela GPU. A CPU submete **um**
`drawIndirect` para o clipmap inteiro e não percorre patch nenhum. Era isto o ponto
7.3 da comparação com a Dagor, que faz este culling em CPU.

A caixa é **exacta em Y**, não estimada: o compute avalia a altura nos mesmos
`(PATCH+1)²` vértices que o VS vai avaliar. Uma caixa folgada (±60, a escala do
terreno) seria correcta e não cortaria nada com a câmara a olhar para cima ou para
baixo; uma estimada por amostragem esparsa cortaria chão que se vê.

### E depois medi

| | pass terrain | dispatch do cull | frame |
|---|---|---|---|
| com culling | 0.061 ms | +0.018 ms | 0.113 ms |
| sem culling | 0.083 ms | — | 0.112 ms |

Mediana de 8 corridas de 500 frames, `--vsync 0`. **O culling corta 26% da pass do
terreno e gasta quase tudo a decidir.** 84.8% dos patches são rejeitados, 172 032
vértices passam a 26 112, e o frame não muda.

A causa é estrutural e devia ter-me ocorrido antes de escrever o shader: para a
caixa ser exacta o compute corre 81 avaliações de FBM por patch, e o VS correria
384 vértices × 3 (a normal usa diferenças centrais). Estamos a pagar ~21% do
trabalho do VS só para decidir se o fazemos. Já foi pior: com uma thread por patch
em vez de um workgroup, as 81 amostras corriam em série numa lane e os 448 patches
davam 7 workgroups para uma GPU inteira — o dispatch custava **0.050 ms**. Um
workgroup por patch com redução em memória partilhada levou-o a 0.018 ms, com a
imagem bit-idêntica.

### E a seguir separei o cálculo do teste, e passou a pagar

A saída não foi uma pirâmide de min/max — foi notar que **um patch só muda de
região do mundo quando o snap do seu nível muda**, e o snap é o dobro da célula:
1 unidade no nível 0, 64 no nível 6. Um `terrain_bounds.cs` calcula as caixas e é
despachado só para os níveis que mexeram; o `terrain_cull.cs` lê-as. Medido no
gate: **2.2 níveis de 7 por frame**, 31% do trabalho.

| | bounds | cull | terrain | soma |
|---|---|---|---|---|
| com culling | 0.005 ms | 0.003 ms | 0.069 ms | **0.077 ms** |
| sem culling | — | — | 0.086 ms | 0.086 ms |

Mediana de 12 corridas de 600 frames. O dispatch do culling caiu de 0.018 para
**0.003 ms** (já não avalia ruído nenhum: uma leitura, o teste do anel, seis
produtos escalares), a pass do terreno caiu 20%, e a soma ficou 10% abaixo. A
imagem é bit-idêntica à versão que recalculava tudo todos os frames.

Não é um número grande. Mas é positivo, medido, e o caminho para o tornar maior
está aberto: a caixa deixou de ser recalculada, portanto o custo de decidir já não
cresce com o custo do VS.

### A prova de que não corta chão que se vê

O contador bater com a CPU não chega: os dois lados correm o **mesmo** algoritmo, e
um erro de desenho concordaria em ambos. Por isso há o controlo `-- --no-cull`, que
desenha os 448 patches, e a comparação é da imagem:

- **921 599 de 921 600 pixels idênticos.** O único que difere é verde de terreno
  dos dois lados, não céu — não é um buraco.
- Desenhar **os mesmos 448 patches por ordem inversa** muda 4 pixels na mesma zona.
  Logo o pixel não é culling: é um empate de profundidade na costura entre dois
  níveis do clipmap, decidido por quem desenha primeiro.
- Entre corridas a imagem é bi-estável **naquele mesmo pixel** e só nele: 8
  corridas, 28 pares, 21 idênticos e 7 a diferir num pixel, sempre em (1128, 712).
  (Com 3 corridas eu tinha visto 0 diferenças e escrito «idêntica entre corridas»;
  com 8 vê-se que não é. A conclusão não muda — reforça-se.)

Fica registado que o clipmap tem z-fighting na fronteira entre níveis. São 4 pixels
e não se vê, mas é real e não foi este trabalho que o criou.

## D49 — GGX anisotrópica, e três verificações minhas que não verificavam nada

O `gate-furnace` passou a três painéis: isotrópico à esquerda (o que já havia),
anisotrópico ao meio, e à direita **o mesmo material com os eixos trocados e a
vista rodada 90 graus**.

O modelo é o de Heitz 2014: `Lambda(w) = (sqrt(1 + (ax²wx² + ay²wy²)/wz²) - 1)/2`,
`G2 = 1/(1 + Lambda(v) + Lambda(l))`. A amostragem inverte a marginal em phi com
`atan2(ay·sin t, ax·cos t)` — a forma escrita com `tan(2πξ + π/2)` tem
singularidades em ξ = 0 e ξ = 0.5, e a sequência de Hammersley acerta nas duas em
cheio.

### Os números

| rugosidade | iso (ref) | aniso ax=ay | ao longo de T | ao longo de B |
|---|---|---|---|---|
| 0.070 | 0.9986 | 0.9986 | 0.9986 | 0.9994 |
| 0.445 | 0.9065 | 0.9065 | 0.9255 | 0.9512 |
| 0.945 | 0.5414 | 0.5414 | 0.7025 | 0.5866 |

Com `alpha_x == alpha_y` o caminho anisotrópico dá o isotrópico com desvio máximo
de **0.0005** — o chão do meio-float, o mesmo da compensação de multiscatter. Com
razão 4:1 a perda média cai de 0.1706 para 0.1139 (tangente) e 0.1334
(bitangente): um eixo mais liso dispersa menos.

### A parte que interessa

Escrevi primeiro duas verificações: «com ax == ay dá o isotrópico» e «ao longo da
tangente e da bitangente o albedo é diferente». Depois quebrei o modelo de
propósito — um `Lambda` a usar `ax` nas duas componentes — e **as duas passaram**.

- A redução não o apanha por construção: com `ax == ay` o erro não existe.
- O «faz alguma coisa» não o apanha porque continua a fazer — só que a coisa
  errada. E o limiar estava em 0.02 sobre o **máximo** de |T − B|, quando o máximo
  do ruído de Monte Carlo num material **isotrópico** é 0.0332. O teste passava com
  a anisotropia desligada.

O que faltava era um invariante do modelo, não do resultado: **trocar alpha_x com
alpha_y e rodar a vista 90 graus é relabelar os eixos**, e qualquer modelo correcto
devolve o mesmo número. O `Lambda` quebrado dá **2.66** contra uma tolerância de
0.03. E a separação passou a ser medida pela **média**, não pelo máximo: 0.0436 com
anisotropia, 0.0011 sem.

O resíduo de 0.0161 no teste da troca é ruído, e há prova: medido a 1024, 4096 e
16384 amostras dá 0.0332, 0.0161, 0.0073 — parte-se a meio quando as amostras
quadruplicam, que é 1/sqrt(N). O sinal a sério não se mexe: 0.0436 nas três.

Três controlos negativos, cada um apanhado por uma verificação diferente:

| o que quebrei | quem apanha | valor contra limiar |
|---|---|---|
| `Lambda` ignora `ay` | troca de eixos | 2.66 vs 0.03 |
| amostragem ignora `ay` | conservação de energia | cria 0.2295 |
| razão 1.0 (isotrópico) | separação média | 0.0011 vs 0.02 |

Falta ligar isto ao renderer: tangente no GBuffer e o parâmetro no material. O
modelo está medido; o que falta é transporte.

## D50 — O IBL usava o G da luz directa, e a rasar errava 51×

O roadmap pedia um gate de referência: «comparar o nosso split-sum IBL com uma
integração Monte Carlo de 4096 amostras, erro máximo publicado». O `gate-ibl` faz
isso, em CPU e sem GPU nenhuma — é matemática, não é desenho — e a primeira coisa
que publicou foi um bug.

### O que estava errado

`integrate_brdf` construía a LUT com `G = Smith-Schlick, k = (a+1)²/8`. Esse `k` é
a remapeação do Karis **para luzes analíticas**; para o IBL a própria publicação
usa outro. Aplicado a um integral sobre a hemisfera, o termo colapsa a rasar:

| | G a N·V = 0.02, rugosidade 0.016 |
|---|---|
| `k = (a+1)²/8` | **0.0196** |
| height-correlated | **1.0** |

A LUT devolvia `escala·F0 + viés = 0.0164` onde a resposta certa é 0.886. O reflexo
rasante — o que faz a água, o vidro e o chão polido parecerem o que são —
simplesmente não existia.

Isto também era uma **incoerência interna**: desde D44 a luz directa usa
height-correlated. O mesmo material respondia de uma maneira ao sol e de outra ao
céu, e nada no motor apontava para isso.

### Medido, antes e depois

Grelha 32×32 de (N·V, rugosidade), 4096 amostras de referência, erro relativo em
luminância:

| F0 | médio antes | médio depois | máximo antes | máximo depois |
|---|---|---|---|---|
| dieléctrico (0.04) | 26.5% | **4.4%** | 98.1% | **18.4%** |
| metal (0.95) | 21.8% | **5.3%** | 98.3% | **23.8%** |

O pior caso mudou de sítio: era a rasar num quase-espelho (um bug), passou a ser
rugosidade alta (onde o split-sum é genuinamente fraco). Os 4–5% que sobram são a
aproximação a trabalhar.

### O erro vem decomposto, e isso responde à pergunta seguinte

O gate mede contra duas referências: a mesma integração sobre o **mapa de 8 bits**
que o pré-filtro viu (erro algorítmico) e sobre o **céu analítico** (erro total).
Dão 4.43% e 4.49%. A diferença é 0.06 pontos: **o que resta é o método, não os
dados.** Aumentar a resolução do ambiente ou passá-lo a HDR não ia ganhar nada
mensurável; melhorar o split-sum, sim. Sem a decomposição eu não saberia qual das
duas atacar.

### O efeito na imagem é pequeno, e vale a pena dizer porquê

No `gate-pbr-grid` mudam 7.7% dos pixels e o brilho médio sobe 0.1%. Parece pouco
para um erro de 26%, e a razão é precisa: o erro era máximo em **material liso a
rasar contra céu brilhante**, e aquela cena é dominada pelo sol, com esferas
rugosas. 10 359 pixels mudam mais de 8/255, todos nas esferas.

Só o `gate-pbr-grid` consome o IBL hoje, portanto o ganho visível é esse. O que
muda é que o próximo material iluminado pelo céu já nasce certo.

Dois testes sem GPU travam a regressão: a LUT tem de dar mais de 0.8 a rasar num
material liso, e o dieléctrico e o metal têm de convergir nesse limite. Com o `G`
antigo o primeiro falha.

## D51 — Projectores: uma estrutura só, e o cone corta metade dos clusters

Omni e spot são **a mesma** `Light`, não duas. Um projector é uma luz pontual com
um cone; uma omni é um projector cujo cone é a esfera inteira, que é o que
`cos_outer = -1` diz. Uma lista de luzes, uma lista de índices, um percurso por
pixel. Duas listas separadas obrigariam a dois percursos e a dois caminhos de
código que divergem com o tempo.

O cone entra no clustering como um segundo teste, depois da caixa da esfera: para
cada cluster candidato constrói-se a esfera que o envolve e faz-se cone-contra-
esfera. A esfera do cluster é conservadora face ao tronco de pirâmide — pode
aceitar um cone que passa ao lado, nunca rejeitar um que toque, que é o lado certo
do erro.

### Os números

| | |
|---|---|
| luzes | 1000, das quais **334 projectores** |
| slots de cluster pela esfera | 45 394 |
| depois do teste de cone | **21 364** |
| cortado pelo cone | **52.9%** |
| clustered | **0.386 ms** |
| força-bruta | 2.220 ms |
| ganho | **5.75×** |
| canais diferentes da força-bruta | **0**, 0 ULP |

O ganho subiu de 3.1× (só omni) para 5.75×: os projectores encarecem a
força-bruta, que os percorre todos, e não encarecem o clustered.

### O que aprendi a partir os meus próprios testes

Escrevi o teste que interessa — «nenhum ponto iluminado fica num cluster que
descartou a luz», a amostrar o interior do cone — e depois quebrei o código de
quatro maneiras. Duas passaram:

- **A direcção do cone transformada como ponto em vez de vector.** A minha câmara
  de teste estava na **origem**, e aí `transform_point3` e `transform_vector3` dão
  o mesmo. Movida para 13 unidades da origem o erro desvia o cone 19.6 graus — e o
  teste **continuou a passar**, porque o cone tinha 28 graus e sobrava
  sobreposição. Só caiu com duas correcções: estreitar o cone do teste para 11
  graus e acrescentar um invariante directo — **deslocar câmara e luz juntas não
  pode mudar nada**, que é exactamente a propriedade que a transformação errada
  quebra.
- **A rejeição «atrás do ápice».** Tirei-a e nada falhou. Fui ver porquê em vez de
  acrescentar um teste: atrás do ápice o `along` é negativo, logo `-along·sin_half`
  é positivo e cresce, e o teste de ângulo já rejeita o cone espelhado sozinho. A
  rejeição é **redundante**. Fica escrita por ser um corte barato, mas o comentário
  que eu tinha posto — «sem a última, tudo o que está atrás seria iluminado» — era
  falso e foi corrigido.

### O chão

O gate ganhou um chão (uma esfera de raio 900 por baixo). Não é decoração: sem
superfície onde o cone pouse, um projector e uma luz pontual dão a mesma imagem, e
um gate cuja imagem não distingue o que testa é mais fraco do que parece. O número
já provava a correcção; o chão é para se **ver** o que está provado. De caminho é
uma superfície rasante que cobre muitos clusters, o que torna o teste mais duro.

## D52 — Sombras dinâmicas com orçamento: o tecto é uma optimização, não outra resposta

Fecha o ponto 7.2. Mil luzes não podem ter mil shadow maps por frame e não
precisam: a maior parte não se mexe, e das que se mexem a maior parte está longe.
O que faz falta é decidir **quais** valem o custo, e não voltar a desenhar as que
continuam válidas.

O escalonador está em `prog/engine/render/src/shadow_atlas.rs` e não sabe nada de
Vulkan: é uma fila por prioridade, um tecto de trabalho, e uma cache com versões.

### Três decisões, e a razão de cada uma

**O orçamento conta-se em texels, não em mapas.** Um mapa de 512² custa 64 vezes um
de 64². «Oito mapas por frame» deixa o custo variar 64× consoante o que a fila
calhar a escolher, o que é o mesmo que não haver orçamento.

**A cache é o que torna o orçamento honesto.** Sem conteúdo persistente, «só
actualizei metade» significa «metade das luzes não tem sombra». Com cache significa
«metade tem um frame de atraso». Por isso um slot guarda a **versão** que tem
desenhada e só é refeito quando a versão da luz muda. Para isso o `begin_color_pass`
passou a interpretar `depth_clear: None` como **LOAD** em vez de limpar a 1.0, e há
um `clear_depth_rect` para limpar só os tiles do plano. Nenhum chamador passava
`None` com depth ligado, por isso a mudança de significado não parte nada.

**Omni não entra.** Uma luz pontual precisa de seis faces; é outro trabalho e está
no roadmap, não aqui.

### O invariante, e é ele que prova tudo o resto

Numa cena parada, o resultado **com** orçamento tem de ser idêntico ao resultado
**sem** tecto nenhum. Medido no `gate-lights`, 60 frames, contra `-- --budget 0`:

| orçamento | pixels diferentes |
|---|---|
| 512×512 texels | **0** |
| 128×128 texels (16× menos) | **0** |

Ao frame 300 a pass de sombras custa **0.000 ms**: a cache convergiu e não há nada
para desenhar. É esse o ponto.

### Os números da fila

1000 luzes, 334 projectores, atlas 2048², orçamento 512×512 texels:

| | |
|---|---|
| tiles desenhados por frame | 0.40 |
| adiados por frame | 3.00 |
| pior espera | 11 frames |
| projectores com sombra | **24 de 334** |

Os 24 não são um bug: 334 projectores a pedir tiles de 512²/256² são 87 M texels
num atlas de 4 M. O escalonador serve os mais importantes e adia o resto, que é o
que um sistema com orçamento faz sob pressão. Dizer «temos sombras dinâmicas para
1000 luzes» seria mentira; o que temos é uma fila que escolhe bem e nunca estoura o
tecto.

### O efeito na imagem é pequeno, e a razão é precisa

Sombras ligadas contra desligadas: **1.73% dos pixels, delta máximo 5**. Parece
nada. Mas cada uma das 24 luzes com sombra é uma de mil, e uma contribui até 59
níveis: apagar **todas** as 24 dá 5.19% e delta 59, e uma sombra de esfera só tapa
uma delas de cada vez.

Antes de aceitar isto verifiquei se o mecanismo estava partido, e a maneira foi
desenhar o factor de sombra em vez da cor: o poço de luz do projector aparece com
as sombras redondas das esferas lá dentro, exactamente como deve ser. Era o
mecanismo certo com um efeito pequeno, não um mecanismo partido.

### O que o gate trava

A comparação clustered-contra-força-bruta **não** apanha uma sombra partida: as
duas correm o mesmo código de amostragem e concordam mesmo que ele esteja errado.
Por isso há uma segunda verificação que lê o atlas e exige que os tiles tenham
**variação de profundidade** — um tile constante é um tile onde nada foi desenhado.
Controlo negativo: com o `draw_indexed` da pass de sombras removido, 0 de 24 tiles
têm geometria e o gate sai com 1.

E o escalonador tem 13 testes sem GPU. Quebrei-o de cinco maneiras e quatro foram
apanhadas à primeira; a quinta — tirar o desempate por índice da luz — passou, e a
razão é que o `sort_by` do Rust é estável, portanto a ordem já era determinista
desde que o chamador não reordenasse. A propriedade que eu tinha afirmado no
comentário era mais forte do que isso e não tinha teste. Agora tem.

Dois deles chegaram a **pendurar** em vez de falhar, porque eu tinha escrito
`while !plan.render.is_empty() {}`. Um teste que pendura bloqueia o CI e não diz
nada; agora há um `fill_cache` com limite de 200 frames que falha a dizer porquê.

## D53 — Render graph, e a corrida que estava lá desde a fase 1

Cada sample sequenciava as suas passes à mão e as barreiras eram raciocinadas caso
a caso. Funcionou até às 29 passes da árvore, e esta sessão deu três provas de que
tinha deixado de funcionar:

- o `storage_barrier_buffer` teve de ser **alargado à mão** quando um compute
  passou a alimentar outro compute, e escrevi eu próprio que sem isso falharia «de
  forma intermitente» (D48);
- `depth_clear: None` queria dizer «limpa na mesma», e ninguém via isso de onde se
  chamava (D52);
- o atlas de sombras transita de profundidade para amostrado porque o RHI o faz
  implicitamente ao ligar, e **nada verificava** que o fazia.

### O que o grafo é, e o que não é

Uma pass declara o que toca e com que tipo de acesso; o grafo valida, deriva as
barreiras entre acessos consecutivos do mesmo recurso, e resolve os `loadOp`.

**Não** faz aliasing de memória nem reordena passes. As duas são optimizações, e um
grafo que reordena antes de se saber se deriva as barreiras certas é uma máquina de
bugs que ninguém depura. Entram quando isto estiver provado em mais do que um
sample.

Por D0 o grafo não conhece `vk::*`: fala em `Access` e emite `BarrierDesc` neutros
que o backend traduz. A vantagem prática é que **toda a derivação é testável sem
GPU**, e é lá que estão os 13 testes.

A regra é uma só: entre dois acessos consecutivos ao mesmo recurso há barreira se
algum deles escreve **ou se o layout muda**. Duas leituras no mesmo layout não
levam nada — sem essa metade, o grafo estaria sempre correcto e sempre lento, que é
a maneira fácil de fingir que funciona.

### O porte, e o que ele custou

O `gate-terrain` foi o primeiro: `bounds` → `cull` → `terrain` → `blit`, com um
compute a alimentar outro compute e um buffer de argumentos a alimentar um draw
indirecto. As duas barreiras escritas à mão saíram.

| | |
|---|---|
| imagem, à mão vs derivada | **0 pixels diferentes** |
| custo de construir e compilar o grafo | **abaixo da resolução do relógio** |

0.060 ms de CPU por frame com e sem grafo, medido com `--validation 0` para tirar a
camada do meio. Quatro passes e cinco recursos não se sentem.

### A parte que valeu a pena

Um controlo negativo mostrou que o grafo **não apanha um acesso que alguém se
esqueça de declarar** — e não pode: só sabe o que lhe dizem. Mas há quem saiba, e
foi isso que motivou ligar a **synchronization validation** do Vulkan. É o outro
lado do par: o grafo declara, a camada verifica.

Ligou-se, e ela encontrou logo uma corrida **que estava lá desde a fase 1**:

> `SYNC-HAZARD-WRITE-AFTER-READ` na imagem da swapchain.

O submit espera pelo `image_available` em `COLOR_ATTACHMENT_OUTPUT`, mas a
transição de layout era emitida em `TOP_OF_PIPE` — ou seja, podia correr **antes**
de a espera fazer efeito, e escrever o layout de uma imagem que o motor de
apresentação ainda estava a ler. Em todos os 18 binários, porque todos apresentam.

A validation normal nunca deu sinal disto, e não tinha como: não é uso indevido da
API, é uma corrida. A correcção é uma linha — emitir a transição em
`COLOR_ATTACHMENT_OUTPUT` — e depois dela os 18 passam com a sync validation ligada.

Custa 0.25 ms de CPU por frame, e por isso só entra com `--validation 1`, que é o
modo dos gates e não o do jogo.

### E um teste meu que prometia mais do que verificava

Tinha um teste chamado «uma mudança de layout precisa de barreira **mesmo entre
duas leituras**». A sequência lá dentro era `DepthWrite → Sampled`, que é
escrita→leitura e sai pelo outro ramo. Tirei o teste de layout do código e nenhum
teste falhou. Agora há um com duas leituras a sério — profundidade testada numa
pass e amostrada na seguinte — e esse apanha.

## D54 — Porte ao render graph: todas as barreiras explícitas da árvore saíram

Oito samples declaram o frame como grafo: `sponza`, `terrain`, `lights`,
`instances`, `fog`, `heightquery`, `furnace`, `bindless`. **Não resta uma única
chamada a `storage_barrier*` num sample** — todas as barreiras explícitas da árvore
vêm agora de uma declaração.

Cada porte foi verificado da mesma maneira: capturar com as barreiras à mão,
capturar com as derivadas, comparar.

| sample | alvos comparados | pixels diferentes |
|---|---|---|
| `terrain` | 1 | **0** |
| `instances` | 1 | **0** |
| `lights` | 3, incl. o atlas de 4 M texels | **0** |
| `sponza` | 3 | **0** |
| `fog` | 5, incl. os dois volumes | **0** |

O `furnace` é o mais fácil de auditar porque publica números e não pixels: depois do
porte dá os mesmos 0.3724 / 0.1707 / 0.0005 / 0.0436 / 0.0161.

### O que cada porte ensinou

**`lights`** é o caso para que isto foi feito: o atlas é `persistent_texture` e a
pass pede `Load::Keep`. Declarar o atlas como transiente faz o grafo **recusar o
frame** — «pede `Load::Keep` em `shadow-atlas`, que não tem conteúdo nenhum para
guardar». É exactamente a classe do bug que o `depth_clear: None` escondia (D52).

Obrigou também a arrumar a ordem: **planear, declarar, desenhar**. O grafo precisa
de saber se há tiles a redesenhar, e isso só se sabe depois de a fila decidir.
Declarar uma pass de sombras que não vai acontecer emitiria uma barreira a mais em
todos os frames em que a cache já está boa — que são a maioria.

**`sponza`** apanhou um erro meu durante o próprio porte. Pus as chamadas onde
estavam os `storage_barrier` antigos, que era **depois** dos dispatches — e a
barreira que protege o `scatter` saía depois de ele já ter sido lido. As barreiras
vão antes da pass que protegem, e a lista sequencial do grafo torna isso óbvio de
uma maneira que a versão à mão não tornava.

**`heightquery` e `furnace`** obrigaram a acrescentar `Access::TransferRead`: um
`read_texture` não é uma amostragem, é uma cópia, com outro estágio e outro layout.
Uma variante nova de vocabulário, não um remendo.

### Uma coisa que ainda não é verdade, e convém dizer

As barreiras do grafo para **ligações de saída** somam-se às transições implícitas
que o RHI faz ao abrir uma pass — não as substituem. O `begin_color_pass` transita
o layout incondicionalmente, e torná-lo condicional não é seguro sem o grafo (duas
passes seguidas a escrever o mesmo alvo precisam da dependência mesmo com o layout
igual).

Medido na Sponza, que é a cena com mais passes: **0.734 ms contra 0.730 ms** de
GPU, e o CPU igual. 0.5%, dentro do ruído. Fica como dívida e não como urgência: o
passo é o RHI passar a confiar no grafo quando ele existe.

### E os que não foram portados

`sky`, `clouds`, `water`, `taa`, `rain`, `csm`, `pbr-grid`, `hello-triangle` não
tinham barreira explícita nenhuma: dependem das transições implícitas. Portá-los
acrescentaria declarações sem tirar código. Ficam de fora **por agora**, e a
synchronization validation cobre-os — passam todos com ela ligada.

## D55 — Culling por oclusão: Hi-Z do mesmo frame, e a prova de que não corta a mais

Fecha o gate `veg` da fase 6, e não com culling por frustum — que o `instances` já
provou — mas com a parte que separa «desenhar o que está no ecrã» de «desenhar o
que se vê».

### O que a Dagor faz

A erva deles (`prog/daNetGame/shaders/grass_generate.dshl`) corta por **feedback do
pixel shader**: desenha-se tudo, o PS marca num bitvector qual a instância que
passou o teste de profundidade, e um compute (`grass_compact_instance_indices_cs`)
compacta esse bitvector numa lista densa. O PS tem **três caminhos de código** com
intrínsecas de wave (`WaveActiveMin`, `WaveActiveBitOr`, `WavePrefixSum`) só para
reduzir o tráfego de atómicos — e o shader de compactação abre com
`if (!hardware.dx12 || hardware.xbox || hardware.scarlett) dont_render;`, ou seja,
está **desligado fora do DX12 de desktop**.

### O que fizemos, e porquê

O teste é feito **antes** de rasterizar. Um prepass desenha os oclusores, constrói-
se uma pirâmide de máximos a partir dessa profundidade, e o compute projecta a
caixa de cada planta e compara. Três diferenças que importam:

* **um atómico por instância que sobrevive**, em vez de um por fragmento;
* **nenhuma intrínseca de wave**, portanto nada que se desligue por plataforma;
* a pirâmide é do **mesmo frame**, portanto não há o frame de atraso de um Hi-Z
  reaproveitado — nem o pop-in que vem com ele.

### Os números

60 000 plantas, 1280×720, 11 níveis de pirâmide:

| | |
|---|---|
| passam o frustum | 34 089 |
| sobrevivem à oclusão | **4 701** (86.2% cortados) |
| pass da vegetação, com oclusão | **0.015 ms** |
| pass da vegetação, sem oclusão | 0.070 ms |
| custo: pirâmide + culling | 0.032 + 0.008 ms |
| **soma** | **0.055 ms contra 0.070** |

4.7× mais barata a desenhar, 21% mais barata no total. A construção da pirâmide é
a maior parte do custo e **não cresce com o número de plantas** — só com a
resolução — por isso a conta melhora quanto mais vegetação houver.

### A prova, que é a parte que eles não publicam

Um culling por oclusão que corta de mais não dá erro nenhum: dá geometria que
desaparece, e num campo de erva ninguém dá por isso. Por isso o gate desenha **as
duas versões no mesmo frame** — o mesmo depth, a mesma geometria, e a única
diferença é o teste Hi-Z — e compara os 3 686 400 canais.

**0 canais diferentes.** E o gate falha se forem mais de 32.

O limiar é contado, não escolhido: cinco corridas do código certo deram 0, 0, 0, 0
e 3 (a ordem dos sobreviventes muda e duas folhas à mesma profundidade trocam de
vencedor). Do outro lado, os dois erros que introduzi de propósito dão 176 e 803.

### E apanhou um bug meu antes de eu o ver

A primeira versão cortava só 27% e tirava 60 pixels de erva que se via. Testei três
hipóteses erradas antes da certa — folga na profundidade (não mudou nada), alargar
o rectângulo projectado (60 → 59), tirar o chão (mesmos 60 pixels exactos, o que
devia ter-me dito logo que a cena não era o problema).

A causa: no `hiz.cs` cada nível lia `texelFetch(..., 0)` — **sempre o mip 0** — em
vez do nível anterior. A pirâmide era lixo a partir do primeiro nível. Corrigido,
o culling passou de 27% para 86% e o erro de 60 pixels para zero. Ler o mip errado
é perfeitamente legal: nem a validation normal nem a de sincronização dizem nada.

### O que foi preciso no RHI

* **Vistas de storage por mip.** Havia uma só, do mip 0 — com ela, uma pirâmide era
  impossível, e era essa a razão de o motor não ter culling por oclusão.
* **O binding 0 do set 4 passou a array** de `STORAGE_IMAGE_SLOTS`. Era um
  descritor único, escrito na criação de cada textura, o que queria dizer que só a
  **última criada** estava ligada.
* `descriptorBindingStorageImageUpdateAfterBind`, porque o alvo é recriado quando a
  janela muda de tamanho — a meio de um frame.
* **Um array de clears vazio passou a significar LOAD**, como o `depth_clear: None`
  desde D52. Queria dizer «limpa a preto», e por isso a pass da vegetação apagava
  os oclusores que a pass anterior tinha desenhado — sem erro nenhum, só um ecrã
  preto com erva.

## D56 — A Sponza passou a GPU-driven: 596 draws para 15

O `veg` provou o culling por oclusão, mas aplicá-lo à Sponza esbarrava numa coisa
mais básica: ela desenhava **um draw por primitiva**, com um par de VB/IB por
primitiva e uma escrita de CBV por draw para o pixel shader saber o seu albedo.
Um draw indirecto múltiplo precisa de **um** VB e **um** IB ligados, portanto o
Hi-Z não entrava sem primeiro mudar isto.

### Antes de construir, medi onde paga

A pirâmide Hi-Z custa 0.032 ms fixos. A pergunta é onde há mais do que isso para
poupar:

| | pass de geometria |
|---|---|
| `gate-terrain` | 0.042 ms |
| `sponza` | **0.350 ms** (cena) + 0.196 (cascatas) |

**No terreno não vale a pena**: mesmo cortando tudo poupava 0.042 e custava 0.032.
Fica registado que foi medido, não esquecido.

### O que mudou

* **Um VB e um IB para a cena toda.** 103 primitivas, 192 496 vértices, 786 801
  índices. Os índices não são rebaseados: cada comando leva o seu `vertexOffset`,
  que é para isso que ele existe.
* **Uma tabela por primitiva** (matriz do mundo, índice de albedo, corte de alfa,
  caixa envolvente) num storage buffer.
* **Uma lista fixa de comandos indirectos**, com as primitivas de uma face
  primeiro e as de duas a seguir, porque são dois PSOs e cada um quer um intervalo
  contíguo. Duas listas: uma que o culling escreve, e outra sempre a 1 para as
  sombras e o mapa de chuva, que **não** respeitam o culling da câmara — um caster
  fora do ecrã continua a projectar sombra dentro dele.
* **O culling em compute** escreve `instanceCount` 0 ou 1. A lista de comandos não
  se compacta: um comando com zero instâncias não desenha nada e custa quase nada.
* Os VS passaram de `.spvasm` escrito à mão a GLSL; os PS levaram um patch de duas
  cargas (o albedo e o corte vêm agora do VS como varyings `flat`).

**`gl_InstanceIndex`, não `gl_DrawID`.** Em Vulkan
`gl_InstanceIndex = firstInstance + nº da instância`, e com `instanceCount = 1` o
primeiro termo é tudo. O índice da primitiva vai no `firstInstance` do comando —
que é onde o culling já escreve — e chega ao shader sem precisar de
`shaderDrawParameters`.

### Os números

| | antes | depois |
|---|---|---|
| draws por frame | 596 | **15** |
| CPU do frame (`cpu_min`) | 0.360 ms | **0.120 ms** |
| GPU | 0.736 ms | 0.736 ms |
| imagem | — | **0 pixels** de diferença no composite, 1 pixel com delta 1 na cena |

Três vezes menos CPU. A GPU não mudou, e não devia: desenha-se exactamente a mesma
geometria com o mesmo shading — o que mudou foi quem manda.

### Dois erros, e nenhum deu erro

**O `firstInstance` estava a ser ignorado.** Precisa da feature
`drawIndirectFirstInstance`, e a validation **não** o apanha: o conteúdo do buffer
indirecto é do lado da GPU e ela não o lê. O sintoma foi a Sponza com a geometria
certa e a textura errada.

**O CBV deixou de ser escrito.** Era escrito por primitiva, porque o índice do
albedo vivia lá dentro; ao tirar isso, deixei de o escrever de todo, e a pass da
cena passou a ler o que ficou da pass anterior. O sintoma foi a cena com um banho
vermelho e as texturas certas por baixo. Uma escrita por pass resolve.

### E a verificação

O ECS continua a marcar `Visible` com a mesma caixa e os mesmos planos. Já não
escolhe o que se desenha — passou a ser a **referência**: o `finish` lê os comandos
de volta, conta os que têm `instanceCount == 1`, e exige que dê o mesmo que a CPU.
**78 de 103, nos dois.**

## D57 — Hi-Z na Sponza: escrito, verificado, e **desligado por omissão**

O caminho está feito e funciona: duas fases, pirâmide do mesmo frame, imagem
idêntica ao pixel. E **custa mais do que poupa**, por uma razão que se mede.

### Como funciona

É o esquema clássico de duas fases, que existe porque os oclusores da cena **são**
a cena — não há um prepass separado como no `veg`:

1. **Fase 1** desenha o que se via no frame anterior (um buffer `seen` persistente).
   É isso que enche o depth buffer.
2. A **pirâmide** sai desse depth.
3. **Fase 2** testa todas as primitivas contra a pirâmide e desenha as que passam e
   ainda não foram desenhadas. O resultado é a união das duas.
4. O que passou o teste fica marcado como visto, para a fase 1 do frame seguinte.

É conservador de propósito: uma primitiva que se via antes e está tapada agora é
desenhada na fase 1 à mesma, e corrige-se no frame seguinte.

### Os números, que são o ponto

| | soma das passes | frame |
|---|---|---|
| com oclusão | **0.399 ms** | 0.780 ms |
| sem oclusão | 0.353 ms | 0.736 ms |

Poupa **3 primitivas de 78** e a pirâmide custa 0.033 ms. Frame 6% **mais lento**.

A razão não é o método — é a granularidade. A Sponza tem 103 primitivas com uma
média de 15 000 triângulos cada; o chão inteiro é uma. Uma primitiva desse tamanho
quase nunca está **inteiramente** tapada, e o teste é conservador por construção.
Para comparar, no `gate-veg` a mesma máquina corta 86.2% — lá as unidades são
plantas pequenas.

**O que falta para pagar: meshlets.** Dividir cada primitiva em grupos de ~64–128
triângulos com a sua própria caixa. Aí a unidade de teste passa a ser da ordem das
plantas do `veg` e o culling tem o que cortar. É o passo seguinte e está no roadmap.

### O que fica

O código fica, **desligado por omissão**, atrás de `-- --occlusion`. Fica porque é
a fundação do culling por meshlet e porque está verificado: com oclusão ligada a
imagem é **idêntica ao pixel** nos três alvos (cena, composite, atlas de sombras).

O que não fica é a pretensão: ligá-lo por omissão seria vender como optimização uma
coisa que medi a tornar o frame 6% mais lento.

## D58 — Meshlets: a granularidade certa, e mesmo assim não paga

O D57 concluiu que o culling por oclusão na Sponza não paga porque as unidades são
grandes de mais — 103 primitivas com ~15 000 triângulos cada. A conclusão era certa
e a cura óbvia: partir em meshlets. Fi-lo, e **também não paga** — por outra razão,
que também se mede.

### O que foi feito

`build_meshlets` parte cada primitiva em grupos de N triângulos, ordenados por
**código de Morton do centróide** antes do corte. Cortá-los pela ordem do ficheiro
daria grupos com triângulos de sítios distantes e caixas a cobrir meia primitiva —
que é exactamente o que o culling não quer.

Cada meshlet leva a matriz e o material da sua primitiva, por isso a tabela que o
shader lê tem **a mesma forma** de antes: **nenhum shader mudou** para isto.

Com 128 triângulos, a Sponza passa de 103 a **2 097** meshlets, e a oclusão passa de
cortar 3 para cortar **116**.

### E o frame ficou mais lento em todos os tamanhos

| triângulos/meshlet | meshlets | cortados pela oclusão | frame |
|---|---|---|---|
| 128 | 2 097 | 116 | 0.948 ms |
| 256 | 1 076 | 34 | 0.856 ms |
| 512 | 569 | 13 | 0.837 ms |
| 1 024 | 327 | 6 | 0.821 ms |
| 2 048 | 204 | 2 | 0.813 ms |
| **um por primitiva** | **103** | 1 | **0.737 ms** |

Monótono: quanto maiores, melhor, e o melhor de todos é não os usar. O culling
melhora com meshlets pequenos e o custo por comando de draw piora mais depressa.

**A causa é o custo fixo por comando indirecto.** 1 768 comandos para 262 267
triângulos são 148 triângulos por draw; a conta dá cerca de +0.1 ms por cada mil
comandos. Não é o culling que está errado — é desenhar um meshlet por draw.

**O que faria pagar: mesh shaders.** Com `VK_EXT_mesh_shader` os meshlets são
workgroups de um dispatch e o custo por comando desaparece. É a fase 9 do roadmap, e
este trabalho é a fundação dela: a partição, as caixas e a tabela ficam iguais.

### O que fica

`-- --meshlets N` para experimentar; por omissão **um meshlet por primitiva**, que é
o que a medição diz. A imagem é idêntica ao pixel em todos os tamanhos e com a
oclusão ligada ou desligada — o que muda é só quanto custa.

### E um erro que a medição apanhou

A primeira versão ordenava os triângulos por Morton **sempre**, mesmo com um só
meshlet por primitiva. Aí a ordenação não agrupa nada e só estraga a localidade que
o ficheiro já tinha: **0.736 → 0.772 ms**, 5% do frame por nada. Agora só ordena
quando há mais de um grupo.

## D59 — Mesh shaders: escritos, e **por verificar** — um GPU hang levou a sessão gráfica

Os três resultados negativos anteriores (Hi-Z no terreno, Hi-Z na Sponza,
meshlets) apontavam todos para o mesmo sítio: o custo fixo por comando de draw. A
resposta é `VK_EXT_mesh_shader`, que este device suporta. Comecei-o, e **não está
provado**.

### O que está feito e verificado (sem GPU)

* **RHI**: a extensão pedida se existir, `create_mesh_pipeline`, `draw_mesh_tasks`,
  e as flags de estágio `MESH_EXT`/`TASK_EXT` nos layouts dos descritores. O
  pipeline de mesh partilha o caminho do de vértices — a única diferença real é a
  flag do estágio, porque o `pVertexInputState` é ignorado pela especificação
  quando há um mesh shader.
* **Formato canónico**: `pack_meshlets` converte os grupos de intervalos-de-índices
  para até 64 vértices únicos e 124 triângulos como índices locais de 8 bits.
* **`build_meshlets` respeita o tecto de vértices**, e tem de ser na construção:
  128 triângulos agrupados por vizinhança precisam de bem mais de 64 vértices, e
  deixar a embalagem partir depois desalinha as tabelas dos comandos.
* **Seis testes e quatro controlos negativos**, sem GPU: os triângulos sobrevivem
  ao empacotamento, os limites são respeitados, os intervalos ladrilham o index
  buffer sem buracos, e — o que interessa para não partir o que funcionava — **sem
  tectos o resultado é exactamente a primitiva**, com os índices pela ordem do
  ficheiro e a caixa da primitiva.

### O que **não** está verificado

**O caminho de mesh shader nunca completou um frame.** Na primeira execução:

> `amdgpu 0000:0a:00.0: GPU reset(1) succeeded!`
> `[drm] device wedged, but recovered through reset`

A GPU recuperou. A sessão gráfica não: o socket do X desapareceu e desde então
nenhum sample corre. Portanto **não sei** se o caminho desenha certo nem se é mais
rápido, e não vou dizer que sim.

### O que causou o hang, e o que ficou a impedi-lo

A criação do pipeline dava `VUID-VkGraphicsPipelineCreateInfo-layout-07988`: os
layouts dos descritores não declaravam o estágio de mesh. Usar um pipeline que a
validation rejeitou é comportamento indefinido, e em RADV o que aconteceu foi
pendurar. Corrigido.

Ficaram duas guardas que não existiam:

* o shader **limita** o que passa a `SetMeshOutputsEXT` ao que declarou em
  `max_vertices`/`max_primitives` — escrever para lá disso é UB, e o que se vê é a
  GPU a pendurar sem dizer onde;
* o host **verifica a tabela antes de a subir**: cada intervalo dentro dos limites
  e dentro dos buffers. Falhar aqui dá um número; falhar na GPU dá um reset do
  driver.

### O caminho por omissão

Não mudou de comportamento, e isso também não pôde ser verificado na GPU — por isso
ficou preso por dois testes de CPU novos, com controlos negativos que os partem.
`-- --mesh` é opt-in.
