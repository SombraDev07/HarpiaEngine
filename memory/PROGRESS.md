# Progress

**Fase actual: 5 (clima) FECHADA** (Linux / RADV, 2026-09). Os quatro gates verdes
a 32 frames com validation 0; fog e chuva **default-on na Sponza** e medidos lá.
**Próxima: fase 6** (terreno + vegetação + mundo), que é onde céu, nuvens e sombra
das nuvens entram — precisam de chão para se verem. Decidir o ECS aí (D28).

Quadro: `docs/Rust-Rewrite-Roadmap.md` §15. Este ficheiro é o diário; o roadmap é o mapa.

## Checks

- [x] Fase 0 — contrato (Null, `--frames`, spec bindless escrita antes do shader)
- [x] Fase 1 — `cargo run -p hello-triangle -- --frames 90` — validation 0, resize 30/60
- [x] Fase 2 — `cargo run -p gate-bindless` — 16 frames, resize 6/12, validation 0; Null `--frames 8`
- [x] Fase 3 — deferred PBR, `cargo run -p gate-pbr-grid -- --frames 90`, validation 0, resize 30/60; Null `--frames 8`
- [x] Fase 4 — CSM câmara real + TAA + Sponza 90
- [x] Fase 5 — fog → céu → clouds → water → rain, todos verdes; fog e chuva na Sponza
- [ ] Fase 6 — um clipmap + veg + mundo **(próxima)**
- [ ] Fase 7 — GI honesta (occupancy no lighting ou 0 bytes)
- [ ] Fase 8 — editor
- [ ] Fase 9 — opcional

## Verde (comandos)

```bash
cargo test --workspace
cargo run -p hello-triangle -- --frames 90
cargo run -p gate-bindless
cargo run -p gate-pbr-grid -- --frames 90
cargo run -p gate-csm
cargo run -p gate-taa
cargo run -p gate-fog
cargo run -p gate-sky
cargo run -p gate-clouds
cargo run -p gate-water
cargo run -p gate-rain
cargo run -p sponza -- --frames 90
# assets: python3 prog/tools/fetch_sponza.py  (glTF gitignored)
# pixels, não screenshots:
cargo run -p sponza -- --frames 120 --capture /tmp/spz     # .scene.png + .shadow-atlas.png
cargo run -p gate-csm -- --frames 40 --capture /tmp/csm
cargo run -p hello-triangle -- --backend null --frames 8
cargo run -p gate-bindless -- --backend null --frames 8
cargo run -p gate-pbr-grid -- --backend null --frames 8
cargo run -p gate-csm -- --backend null --frames 8
cargo run -p gate-taa -- --backend null --frames 8
cargo run -p gate-fog -- --backend null --frames 8
cargo run -p gate-sky -- --backend null --frames 8
cargo run -p gate-clouds -- --backend null --frames 8
cargo run -p gate-water -- --backend null --frames 8
cargo run -p gate-rain -- --backend null --frames 8
```

## Feito (para não redescobrir)

- Árvore: `prog/engine/{core,memory,math,drv,render,app,plugin}` — um crate por pasta (`harpia-*`). Samples em `prog/samples/`.
- Hello-triangle preto: `OpConstant %float 1` no assembler era bits `0x1`, não `1.0`. Já corrigido.
- Heap 8192 no init do Vulkan device. Slot 0 = dummy magenta 1×1, barreira VS+PS+CS. Hello-triangle **não** usa o layout bindless (PSO com layout vazio).
- `gate-bindless` mistura no PS `heap[cpu_mips]` + `heap[uav_compute]`. UAV 64×64 set 4 binding 0, GENERAL.
- Fase 3: GBuffer 5 MRT + D32, packing §3, `fuzzColor` no RT3 se fuzz>0. Lighting fullscreen: sol + IBL split-sum + energy compensation + ACES. `gate-pbr-grid`. `cascade_count == 0` no `LightingCb` = sem sombras.
- Fase 4: RHI depth-only + `set_viewport` + depth bias + D32 sampled. CSM 4 cascades atlas 2048 2×2, frustum da câmara, snap no eixo da luz, PCF Vogel. `gate-csm` 16 frames (resize 6/12). TAA: motion (câmara + object X) + history + clamp 3×3, HDR `R16G16B16A16`, `gate-taa` 16. Sponza glTF (103 prims), albedo + sol + CSM, `--frames 90` resize 30/60.
- GitHub `SombraDev07/HarpiaEngine` em `main`.

## Sessão 2026-09-09 — a Sponza estava a render lixo. Corrigido.

Sintoma: blocos verdes/amarelos por cima da geometria, cena de cabeça para baixo,
a ver o interior das superfícies. **Zero erros de validation** o tempo todo.

Três bugs independentes, todos em `LANDMINES.md` com detalhe:

1. **CBV do frame não era por draw** (raiz do lixo). Set 0 binding 0 era
   `UNIFORM_BUFFER` fixo no offset 0 → os 103 draws liam a **última** escrita do
   frame, que era `gbuf0 = scene RT`. O PS amostrava o render target que estava a
   escrever. Agora `UNIFORM_BUFFER_DYNAMIC` + anel de chunks (D13).
2. **Faltava o flip de Y do clip space do Vulkan** → imagem espelhada **e** culling
   invertido (víamos o verso de tudo). `harpia_math::perspective_vk` (D12). As malhas
   estavam enroladas ao contrário para casar com o bug; corrigidas + teste.
3. **Textura por primitiva, sem mips, sem tonemap.** Agora: uma textura por imagem
   glTF (103 prims → 25 imagens), mip chain em espaço linear, `alphaMode MASK` com
   `OpKill`, `doubleSided` com PSO sem culling, Lambert `/π` + ACES + encode sRGB (D14).

Verificado no ecrã (RX 6700 / RADV): Sponza reconhecível — mármore, panos vermelho/
azul/verde, plantas com cutout, o leão ao fundo. `sponza --frames 2400` validation 0.
`gate-pbr-grid` também verificado: agora a luz vem de cima (antes vinha de baixo).

Verde depois da mudança: `cargo test --workspace`; os 6 samples Vulkan a 90 frames
com validation 0; os 5 gates no backend Null a 8 frames.

## Sessão 2026-09-09 (parte 2) — sombras CSM. Era o assembler.

As sombras nunca funcionaram, em nenhum sample. Causa: `OpFOrdLessThanEqual`
gravado como **181** (`OpFUnordEqual`) — todo o `a <= b` do SPIR-V virou `a == b`.
No PS de sombra isso fazia `uv <= 1` ser falso, `inside` falso e `shadow = 1`
sempre. O mesmo bug desligava a *history* do TAA (`hx <= 1`). Detalhe e os números
certos da spec em `LANDMINES.md`.

Corrigidos na tabela: `OpFOrdLessThanEqual` 181→**188**, `OpUMod` 139→**137**,
`OpConvertSToF` 117→**111**, e acrescentados os pares `OpFUnord*`.

Para chegar lá foi preciso parar de julgar por screenshot: `--capture` (D15) lê a
textura de volta da GPU. Foi o dump do atlas que provou que o atlas estava certo e
o problema era o lookup.

Verificado: `gate-csm` com as três esferas a projetar sombra no chão; Sponza com o
pátio em sombra e a faixa de sol que entra pela abertura; `gate-taa` com `resolved`
≠ `color` (a history passou a ser usada). Sem acne — o `depth_bias` (1.25 / 1.75)
chega com os casters sem culling.

## Sessão 2026-09-09 (parte 3) — fase 5 arrancou: fog em froxels

`gate-fog` verde: 16 frames, validation 0, e a captura mostra as duas filas de
esferas a dissolver-se com a distância, com height fog no chão. (A cena mudou na
parte 5: leva agora uma grelha de oclusores para mostrar os feixes.)

O que foi preciso no RHI (não existia): **texturas 3D**. `TextureDim::D3` +
`depth_slices` no `TextureDesc`, views 3D, e os sets 4/5 que a
`docs/Bindless-Descriptor-Layout.md` já especificava mas ninguém tinha
implementado — set 4 binding 1 = UAVs de volume, set 5 binding 0 = SRVs de
volume. Volumes **nunca** entram no heap 2D (D16). Dummy 1×1×1 em GENERAL a
encher os dois arrays, senão um descriptor por escrever é erro de validation.

Fog (roadmap §9, à risca): froxels 160×90×64 RGBA16F, inject `[8,8,4]` dispatch
(20,12,16), integrate `[8,8,1]` dispatch (20,12,1), jitter Halton(2), Z
exponencial. Inject = extinção de height fog + in-scattering do sol com fase
Henyey-Greenstein. Integrate marcha os 64 slices com a integração que conserva
energia. Apply amostra o volume em `(uv, log(z/near)/log(far/near))` e compõe
`cena * transmitância + in-scattering`, ACES + sRGB.

O assembler ganhou `OpLoopMerge`, `OpImageRead`, `OpULessThan`, dim `3D` e os
formatos de imagem — números confirmados na spec desta vez (`Rgba16f` é **2**).

`--capture` agora também escreve volumes: os 64 slices saem numa grelha 8×8 num
PNG. Foi assim que se confirmou o froxel antes de olhar para a composição.

Falta na fase 5: clouds, water, rain. E o fog ainda não amostra o CSM — sem
sombras volumétricas / god rays.

## Sessão 2026-09-09 (parte 4) — a barra da sombra fechou

Sampler de comparação + PCSS + cutout nos casters. Detalhe em D18. Verificado em
pixels: `gate-csm` com penumbra que abre com a distância e dura no contacto;
Sponza com a sombra da arcada suave no chão do átrio e a folhagem a projectar
folhas em vez de rectângulos (14 primitivas no caminho de cutout).

Dois opcodes errados apanhados outra vez — mas agora em segundos, porque o
validador está ligado e há `prog/tools/dis_spv.py`. Ver `LANDMINES.md`.

O que a barra manda **não** fazer e não foi feito: VSM, ESM, contact shadows,
toroidal atlas, octa point shadows.

## Sessão 2026-09-10 — atmosfera (parcial) antes das nuvens

`gate-sky` verde: 16 frames, validation 0. A cadeia do Hillaire inteira —
transmittance (256×64) → multiscattering (32×32) → sky-view (192×108) →
composite com **um fetch por pixel**. O sol desce ao longo dos frames; as LUTs
são refeitas todas as vezes (D19).

Prova de que a optimização é fiel: a saída pelo sky-view bate com o raymarch por
pixel ao bit (±1 num canal no zénite). Falta a aerial perspective (froxel 32³),
que é integração com a cena, não gate de céu.

Bruneton é o **céu**, não as nuvens — ver D19. As nuvens continuam Schneider.

Uma hora perdida por bissectar o shader errado: o módulo inválido era um `blit.ps`
gerado por regex, não o que eu estava a cortar. O assembler passou a validar ids e
`LANDMINES.md` tem a regra: descobre qual módulo falha **antes** de bissectar.

## Sessão 2026-09-10 (parte 2) — nuvens arrancaram pelo noise

`gate-clouds` verde: 16 frames, validation 0. Ainda **não há raymarch** — o que
está feito é o caminho todo até à GPU, provado em pixels:

- Bake CPU do Perlin-Worley 128³ (RGBA8: R forma, G/B/A Worley FBM a subir de
  frequência) e do Worley 32³ de detalhe. **Tileável** — costura num volume 128³
  é uma risca em todo o céu, e há teste que o fixa.
- Cache `.raw` em `assets/cache/` (gitignored). O bake leva **2.1 s** em debug com
  threads do `std` sobre slices; depois é leitura de disco. `rayon` só na fase 6
  (§14.1).
- RHI: `upload_texture_mip` e `pack_mip` passaram a saber de slices — um volume
  sobe inteiro num mip. `finalize_sampled` já não mete volumes no heap 2D.
- Um `slice.ps` mostra o volume no ecrã e os dois volumes são alvos de `--capture`
  (grelha de slices). Foi assim que se confirmou que os canais estão certos.

Falta o que interessa: o raymarch (meia resolução, 64 passos, march de luz),
reprojecção temporal, e a sombra das nuvens a alimentar os froxels do fog.

## Dependências

`docs/Rust-Rewrite-Roadmap.md` §14.1 diz o que entra em que fase (D20). Duas
coisas por resolver antes de lá chegar: qual ECS (fase 6, decidir com um gate de
1e6 instâncias) e o que é «box3d» na frase «jolt ou box3d» — não existe crate com
esse nome.

## Sessão 2026-09-10 (parte 3) — o raymarch das nuvens

`gate-clouds` verde com nuvens a sério: 16 frames, validation 0, e a captura mostra
céu azul, disco do sol, e cúmulos brancos com a base sombreada. Detalhe em D21.

O que está lá: intersecção da concha esférica, oclusão pelo chão, 64 passos com um
march de luz de 6 passos por amostra, densidade Nubis (Perlin-Worley remapeado
contra o próprio FBM → cobertura → erosão pelo detalhe), Cornette-Shanks com a
normalização `3/(8π)` e multiple scattering em 3 oitavas, jitter IGN no arranque.
Meia resolução para um RT `Rgba16Float` `(scatter.rgb, transmitância.a)`; o
composite full-res faz `céu * tr + scat` e só aí ACES + sRGB.

O problema que custou foi o **horizonte**, não a nuvem. Num raio rasante o passo
passa dos 500 m e o resultado foi primeiro um leque de aliasing e depois confetti.
O que resolveu não foi tuning de passos: foi **subir a cobertura com a distância**
para as nuvens distantes fundirem num banco contínuo — que é o que um horizonte
real é. LOD do detalhe e limite de span ajudaram; medi cada um em pixels (o LOD
sozinho mexeu 33/255 e só na faixa do horizonte).

Uma ronda inteira de afinação do céu não fez nada porque o `main.rs` do gate
sobrepunha o `ambient` ao default do `cloud_noise.rs`. Só apareceu ao sondar o
valor cru em vez de raciocinar sobre ele.

Sweep verde depois da mudança: `cargo test --workspace` (34 testes), os **9**
samples Vulkan a 32 frames com validation 0, e os 8 gates no backend Null.

## Sessão 2026-09-10 (parte 4) — reprojecção temporal das nuvens

`gate-clouds` continua verde a 32 frames, agora com um pass temporal a meia
resolução entre o march e o composite. Ping-pong de dois acumuladores, sem cópia.

Sem depth buffer para as nuvens, a reprojecção usa **profundidade analítica**: o
raio é intersectado com o meio da concha e esse ponto passa pela view-projection
do frame anterior. Custa zero render targets. A história é limitada à caixa 3×3
do frame actual antes de misturar (blend 0.92).

Duas coisas tiveram de mudar no march para isto valer alguma coisa: o jitter
passou a mexer por frame (índice do frame em `wind.w`), e a origem deixou de
estar presa a `(0, r0, 0)` — com a câmara sempre no eixo do planeta não havia
paralaxe nenhuma para reprojectar.

A câmara do gate passou a orbitar **e** a rodar de propósito: com uma câmara
parada a reprojecção parece certa mesmo quando a matemática está errada.

Números, porque a olho não se via: na banda do horizonte o |laplaciano| médio cai
de 11.99 para 6.64 (**−44.6%**). Com a matriz do frame anterior substituída por
identidade a redução é **−2.8%** — o clamp rejeita a história desalinhada em vez
de a esborratar. Esse par é a prova de que a reprojecção *e* o clamp funcionam.

Sweep verde: `cargo test --workspace`, 9 samples Vulkan a 32 frames com
validation 0, 8 gates no backend Null.

## Sessão 2026-09-10 (parte 5) — feixes de luz: o CSM entrou no fog

O froxel já era marchado; faltava-lhe **uma tap de comparação no CSM**. É daí que
vêm os feixes, sem pass de god rays e sem radial blur. `gate-fog` ganhou cascatas
(partilha o `shadow.vs` do gate CSM, não uma cópia) e uma grelha de oclusores com
folgas — a arcada da Sponza reduzida ao que este gate tem.

Pelo caminho apareceu um bug a sério, já commitado há sessões: **a fase de
Henyey-Greenstein tinha o sinal trocado** (D24). Os dois vectores saíam do froxel,
por isso o `dot` era o simétrico do cosseno certo e o lóbulo para a frente caía
atrás da câmara: olhar para um sol baixo através de neblina ficava *escuro*. Não
se via como bug — com o sol atrás da câmara a névoa parecia bem. Só apareceu ao
perguntar porque é que o efeito era tão fraco e ir aos números. As nuvens **não**
têm o mesmo erro; lá o vector aponta da outra ponta.

Como foi encontrado, e vale a pena repetir: despejei o `%shadow` directamente no
volume dos froxels e olhei para a grelha de slices. Mostrava as silhuetas certas
das esferas, gama 0.0008–1.0 — ou seja a procura estava boa e o problema era o
tamanho do efeito (4.6% dos froxels na sombra), não a matemática da sombra.

Números finais: com a grelha de oclusores, ligar as sombras muda **20.2%** dos
pixels em mais de 8/255. Antes, com esferas soltas e a fase errada, o máximo era
7/255 em todo o ecrã.

O assembler ganhou um check de **uso antes da definição** — um `%v2` declarado
depois do struct que o usa passava aqui e só o driver reclamava, apontando para o
struct em vez da linha que falta. Os 43 shaders da árvore continuam a assemblar.

## Sessão 2026-09-10 (parte 6) — a Sponza ficou HDR e recebeu o fog

Critério de saída da fase 5 para o fog: **default-on na cena de referência**. Feito,
`sponza --frames 32` validation 0, e a captura continua a Sponza de sempre — mármore,
panos, folhagem, o leão ao fundo — agora com neblina que cresce com a distância.

O que teve de mudar: o `color.ps` deixou de fazer ACES + sRGB e o alvo passou a
`Rgba16Float` **linear**, com o view depth (`clip.w`) num segundo MRT. Quem faz o
tonemap é o apply do fog. Compor névoa por cima de uma imagem já codificada é o
erro clássico que dá halos.

Os shaders do froxel não foram copiados: o `build.rs` aponta para os do gate. Uma
segunda cópia do inject/integrate/apply seria uma segunda implementação.

Medido: ligar as sombras volumétricas muda **50.7%** dos pixels em mais de 8/255
(média 12.9, máximo 59). No `gate-fog` era 20.2% — a Sponza tem muito mais
geometria a tapar o sol.

O sol da Sponza continua alto, que é o que a fase 4 validou. Baixá-lo daria feixes
muito mais dramáticos pela arcada; fica como escolha de arte, não de motor.

## Sessão 2026-09-10 (parte 7) — os testes de layout passaram a ler os shaders

`fog::cb_layout_matches_the_spvasm` e companhia tinham o nome trocado: só
comparavam os offsets do Rust com números escritos no mesmo ficheiro. Agora
`spvasm_layout::assert_prefix_matches` lê o `.spvasm` e compara membro a membro.
Quatro blocos (`Fog`, `Atmos`, `Cloud`, `Lighting`) contra **17 shaders**.

Vale a pena porque um CB desalinhado **não falha validation** — o bloco tem o
tamanho certo, a GPU lê os bytes errados, e sai uma imagem plausível e errada.
Foi a classe de bug mais cara desta árvore e esta noite mexi em quatro layouts à
mão.

Testado a sério: com um `Offset` trocado de propósito num shader o teste falha e
diz o ficheiro, o membro e os dois offsets.

## Sessão 2026-09-10 (parte 8) — água

`gate-water` verde à primeira: 16 frames, validation 0, e a captura é um mar a
sério — cristas de Gerstner, o caminho de brilho do sol a partir-se nas faces das
ondas, primeiro plano escuro (olha-se através da água) e horizonte claro (Fresnel
rasante). Detalhe em D26.

Gerstner numa grelha de 192² quads (73k triângulos), quatro ondas, cada uma à
velocidade de água funda `sqrt(g/k)` — sem a dispersão as quatro andam juntas e a
superfície parece uma chapa. Normal pela derivada analítica da mesma soma.

Sem SSR: a reflexão é o céu analítico. Aos ângulos rasantes, onde o Fresnel
domina, é sobretudo céu que uma superfície real reflecte, portanto isto é a
aproximação honesta e não um placeholder.

Provado que as ondas andam: entre o frame 20 e o 40 a faixa do céu tem delta
**0.00** (câmara parada) e a faixa da água **22.25**. Se o termo do tempo estivesse
morto os dois seriam zero e a imagem continuava bonita.

## Sessão 2026-09-10 (parte 9) — câmara e input

Não havia câmara: só `Camera` (eye/target/fov → matrizes) e zero input — o app
tratava três eventos e nenhum era de teclado. Todas as câmaras dos samples eram
literais. Para a fase 6 isso é bloqueante: não se valida um clipmap que não se
pode sobrevoar.

Agora há `harpia_core::Input` (sem winit), `FlyCamera` no `render` (WASD, Q/E,
Shift para acelerar, botão direito ou setas para olhar), e `Sample::update(&Input,
dt)` com implementação vazia por omissão — nenhum dos 10 samples existentes mudou
uma linha por causa disto. Sponza e água já voam. Detalhe em D27.

**A regra que não se quebra:** gates não vêem input e o `dt` é fixo em 1/60. Um
`--capture` é uma medição; se dependesse do relógio ou de uma tecla, toda a
verificação desta árvore ia abaixo.

Provado com números, não com fé: das 22 capturas de referência, **18 ficaram
bit-identical**. As duas amostras convertidas diferem em **12 e 2 pixels** de
921 600, todos em cima do limiar da comparação de sombra. E que o `update()` está
mesmo a ser chamado: a subir a câmara 1 unidade/s mudam 515 740 pixels.

48 testes a passar.

## ECS investigado (D28) — recomendação: `bevy_ecs`

Números do crates.io a 2026-09-10. `bevy_ecs` 0.19.1 (1.84M downloads recentes,
actualizado em Agosto) **não traz `wgpu` nem `winit`** — era o único motivo sério
para o excluir por causa da D0, e não se aplica. `hecs` é o plano B (3
dependências contra 18). `flecs_ecs` fora: parado desde Nov 2025 e traz toolchain
C++. O `ecs_bench_suite` está arquivado desde 2022, portanto **não há benchmark
mantido** — o gate de 1e6 instâncias da D20 é a única forma de decidir.

Decisão fica em aberto até à fase 6, como a D20 manda. Física continua adiada.

## Sessão 2026-09-10 (parte 10) — SSR e espuma na água

`gate-water` fechado: 32 frames, validation 0. Passou a ter rochas (o SSR precisa
de algo para encontrar — água a reflectir só céu não prova nada), um passe opaco
que escreve cor e view depth, e a água num segundo alvo HDR porque **amostra o que
reflecte e não pode estar a escrever para lá**.

O erro que custou: **espessura fixa no SSR só apanha silhuetas.** Os primeiros
reflexos saíram como anéis ocos. Com passo de 3.2 unidades e janela de 0.9, o
interior da rocha está muito mais perto do que o raio nesse passo. A janela tem de
cobrir uma passada inteira e crescer com a distância.

E uma descoberta ao afinar: **a espuma de crista era código morto**. Com `Q=0.62`,
`Σ Q·k·A ≈ 0.22`, o termo de Jacobiano nunca sai de `[0.78, 1.22]`, e o limiar
estava em 0.38 — nunca disparava. Estas ondas não rebentam. Está documentado em
D29 que o que se mede agora é «quão perto de dobrar», com ganho: escolha de arte
assumida, não física.

Medido: ligar o SSR muda **3.7%** dos pixels em mais de 8/255 (máx 157). E os
outros gates continuam determinísticos — 18 capturas bit-identical, as 3 da Sponza
são as mesmas 12 pixels de limiar de sombra já explicadas na parte 9.

## Sessão 2026-09-10 (parte 11) — chuva, e a fase 5 fechou

`gate-rain` verde e a chuva **default-on na Sponza**. Detalhe em D30, D31, D32.

O gate: material molhado (escurece *e* aperta o especular — só uma das duas dá
geada ou verniz), ondulações num anel por célula do mundo em vizinhança 2×2 com
fade de distância, e bátegas em três camadas, aditivas.

O que fazia falta para a Sponza: bátegas em espaço de ecrã **não sabem que há
telhado**, e chuva a atravessar a pedra da arcada é um bug óbvio. Fiz um **mapa de
chuva**: um depth top-down 1024² da mesma geometria, a reutilizar o `shadow.vs` e o
PSO depth-only que já existiam. Um pixel cuja superfície não é a coisa mais alta
naquela posição está sob cobertura.

Medido na Sponza: bátegas com delta médio **1.925** na nave aberta, **0.061** sob a
arcada esquerda e **0.000** sob a direita. Não chove dentro.

O bug que quase passou: gerei as decorações do bloco com `112 + i*16` em vez de
`96 + i*16`, e **todos os `%v4` ficaram 16 bytes à frente** — o shader lia `ripple`
onde queria `streak`. **Zero erros de validation**, porque o bloco tem o tamanho
certo. O `cb_layout_matches_the_spvasm` apanha-o; confirmei a repor o offset errado.
Lição no LANDMINES: corre os testes antes de ir depurar a imagem.

51 testes; 11 samples Vulkan a 32 frames com validation 0; 10 gates no Null.

## Sessão 2026-09-10 (parte 12) — fase 5.5 e o ECS decidido

**O toolchain sempre esteve cá.** `glslang-tools` 15.1.0 e `spirv-tools` 2025.1
estavam instalados desde Maio/Junho; a minha verificação inicial (`command -v`)
deu `NAO` para tudo e escrevi na revisão que faltavam. Falso. Pior: o `spirv-as`
real assembla e valida os 54 shaders, e o `build.rs` já o preferia — o assembler
em Python era um fallback que nunca precisou de existir neste host.

`spirv-val` no build apanhou logo um bug que o runtime deixava passar: um
`OpSampledImage` consumido noutro bloco no `water.ps`. Imagem bit-idêntica depois
da correcção.

Os 11 `build.rs` deram lugar a `harpia-shader-build` (aceita `.spvasm` e `.glsl`,
valida sempre). O `blit.ps` da chuva é o primeiro em GLSL, **bit-idêntico**.

**A engine passou a saber quanto custa.** `--vsync 0` e `--stats` com timestamps.
Sponza: **1.569 ms de GPU**, não os 16.4 ms que o vsync mostrava. Cascatas 0.433 ·
cena 0.770 · froxels 0.110 · rain map 0.082 · chuva 0.082 · fog apply 0.052.

**ECS decidido: `bevy_ecs`** (D38), pelo `gate-ecs` de 1e6 entidades. Ganha no
`iterate` (o caso de todos os frames) por 15–22%; o `hecs` ganha no spawn e no
churn, que acontecem muito menos. Fecha a D20.

## Fase 6 — o que fazer, em ordem

Não mesh shaders, RT, FSR, editor.

1. ~~Decidir o ECS e usá-lo.~~ **feito** (D38, D39). A Sponza é entidades e tem
   frustum culling: 621 → 596 draws, 1.353 → 1.218 ms, imagem bit-idêntica.
2. **Física por clarificar** — «jolt ou box3d»: `box3d` não existe. Adiado por
   decisão tua.
3. Terreno: ~~clipmap~~ **feito** (D40) e ~~`heightquery`~~ **feito** (D41, que
   apanhou 77 m de divergência entre CPU e GPU). Falta vegetação e instâncias.
4. **Céu Hillaire e nuvens entram aqui** — num interior não se viam (D32). A
   sombra das nuvens nos froxels do fog também, que é onde passa a haver chão.
5. Aerial perspective (froxel 32³), a dívida do D19.

### Dívida conhecida (documentada, não esquecida)

- Multiscattering do céu sem bounce do chão (albedo 0) → horizonte um pouco escuro.
- Aerial perspective (froxel 32³) por fazer.
- A máscara da chuva usa a superfície **atrás** do pixel, não o ar à frente: uma
  bátega em frente a uma parede coberta é suprimida. É o que os jogos fazem.
- Um erro de validation isolado apareceu 4× logo após editar shaders e **não
  reproduziu em ~47 corridas**. Causa desconhecida — ver LANDMINES.

## Registo: a ordem que a fase 5 seguiu (toda feita)

Não mesh shaders, RT, FSR, editor. Não VSM.

1. ~~Fog: froxels, 3D GENERAL. Gate `fog`.~~ **feito**
2. ~~Céu: Hillaire completo. Gate `sky`.~~ **feito** (falta aerial perspective)
3. ~~Clouds: raymarch Nubis a meia resolução. Gate `clouds`.~~ **feito**
4. ~~Reprojecção temporal das nuvens.~~ **feito** (D22)
5. ~~Sombras volumétricas no fog (CSM no inject).~~ **feito** (D23)
6. **Sombra das nuvens** nos mesmos froxels — falta ler a transmitância das nuvens
   onde o inject já lê o CSM. **Mas** o sítio natural para isto é a fase 6: a
   Sponza é interior e o `gate-clouds` não tem chão, portanto não há onde a sombra
   cair. Mesma razão adia o céu Hillaire na Sponza (pelas aberturas vê-se quase
   nada). Fazer com o terreno, não antes.
7. ~~Fog default-on na Sponza.~~ **feito** (D25)
8. ~~Water: Gerstner, Fresnel, absorção, SSR e espuma.~~ **feito** (D26, D29)
9. Rain **por último** (GBuffer wet + post; cones sem HDR SRV; `--frames 16` only).
10. Marcar fase 5 `[x]` no roadmap §15 **no mesmo PR** que fechar o último gate.

### Dívida conhecida (documentada, não esquecida)

- Multiscattering do céu sem bounce do chão (albedo 0) → horizonte um pouco escuro.
- Aerial perspective (froxel 32³) por fazer — entra quando a Sponza receber céu.
- Sponza ainda não tem céu nem nuvens: os gates provam os passes isolados.

## `instances`: um draw, e a CPU não sabe quantos cubos saíram

O culling passou para a GPU. As instâncias vão para um storage buffer no arranque e
a CPU nunca mais lhes toca: todos os frames um compute testa-as contra os seis
planos, escreve a lista dos sobreviventes e enche o `instanceCount` do comando de
draw. A CPU submete um `vkCmdDrawIndirect` e fica sem saber o resultado — o
`--stats` mostra `draws=1, dispatches=1, triangles=0`, e o zero é honesto.

É o ponto 7.3 da comparação com a Dagor, que faz este culling em CPU.

**A medição.** `-- --cpu-cull` refaz o mesmo ecrã com o culling do lado da CPU, e
os dois modos concordam no número de visíveis em todas as escalas — sem isso não se
estaria a comparar a mesma coisa. `cpu_min_ms`, 300 frames, `--vsync 0`:

| cubos | compute | CPU | visíveis |
|---|---|---|---|
| 2 500 | 0.17 ms | 0.17 ms | 499 |
| 100 000 | 0.16 ms | 0.44 ms | 19 701 |
| 1 000 000 | **0.17 ms** | **2.72 ms** | 197 164 |

400× mais instâncias, o mesmo custo de CPU. A 2 500 não há diferença nenhuma e não
vale a pena fingir que há: isto paga a partir das dezenas de milhar.

**Dois erros pelo caminho, nenhum visível.** O primeiro: usei
`drawIndexedIndirect` para geometria que nasce do `gl_VertexIndex` e não tem index
buffer — a validation apanhou-o. Ficaram as duas variantes no RHI, porque o clipmap
do terreno é o mesmo caso e as malhas a sério são o outro.

O segundo não deu erro nenhum. O mapeamento do quad para as faces do cubo estava
transposto no eixo Z, o winding invertia-se, e o back-face culling comia duas das
seis faces. A imagem continuava a mostrar cubos plausíveis. Só apareceu a calcular
os 36 vértices em Python e a comparar o normal do winding com o pretendido: 4
triângulos em 12 invertidos. A cobertura do frame foi de 24.1% para 32.2%.

**E a verificação, que também estava errada.** Comparava a esfera da GPU com uma
caixa na CPU: 496 contra 506, passava, e não provava nada — qualquer erro cabia na
folga entre os dois testes. Agora a CPU faz o **mesmo** teste, exige igualdade
exacta, e lê a lista de volta para confirmar que cada id existe, passa o teste e
aparece uma só vez. Com o raio errado no shader o gate falha com exit 1.

## O terreno deixou de precisar da CPU para decidir — e isso não o acelerou

Cada nível do clipmap são agora 64 patches, e quem escolhe quais se desenham é um
compute shader: um workgroup por patch, a caixa envolvente contra os seis planos,
e a lista e o `instanceCount` escritos pela GPU. A CPU submete **um**
`drawIndirect` para o clipmap inteiro. 448 patches, 84.8% cortados, 172 032
vértices reduzidos a 26 112.

À primeira isto não acelerou nada. O frame ficou igual: a pass do terreno caía de
0.083 para 0.061 ms e o dispatch do culling custava 0.018 — pagava-se a si próprio
e mais nada. A caixa exacta em Y obriga a avaliar 81 alturas por patch, quando o VS
avaliaria 384 vértices: 21% do trabalho só para decidir se o faz.

A saída foi notar que **um patch só muda de região do mundo quando o snap do seu
nível muda**, e o snap é o dobro da célula: 1 unidade no nível 0, 64 no nível 6. As
caixas passaram para um `terrain_bounds.cs` despachado só para os níveis que
mexeram — 2.2 de 7 por frame — e o culling passou a lê-las.

| | bounds | cull | terrain | soma |
|---|---|---|---|---|
| com culling | 0.005 ms | 0.003 ms | 0.069 ms | **0.077 ms** |
| sem culling | — | — | 0.086 ms | 0.086 ms |

O dispatch do culling caiu de 0.018 para 0.003 ms e a soma ficou 10% abaixo, com a
imagem bit-idêntica à versão que recalculava tudo. Não é um número grande, mas é
positivo e medido — e o custo de decidir deixou de crescer com o custo do VS.

**Como se prova que não corta chão que se vê.** O contador bater com a CPU não
chega — os dois lados correm o mesmo algoritmo e um erro de desenho concordaria em
ambos. Há um controlo `-- --no-cull` que desenha os 448, e compara-se a imagem:
921 599 de 921 600 pixels idênticos. O pixel que difere é verde de terreno dos dois
lados, e desenhar os **mesmos** 448 patches por ordem inversa muda 4 pixels na
mesma zona — é empate de profundidade na costura entre níveis, não culling. Em 8
corridas (28 pares) aquele pixel é bi-estável e mais nenhum muda.

Pelo caminho: com uma thread por patch o dispatch custava 0.050 ms, porque 448
threads são 7 workgroups e cada uma corria as 81 amostras em série. Um workgroup
por patch, com redução em memória partilhada, deu 0.018 ms e imagem bit-idêntica.

## GGX anisotrópica, e o que aprendi a partir as minhas próprias verificações

O `gate-furnace` tem agora três painéis: isotrópico, anisotrópico, e o mesmo
material com os eixos trocados. O modelo é o de Heitz 2014. Com `alpha_x ==
alpha_y` reduz-se ao isotrópico com desvio de **0.0005** — o chão do meio-float — e
com razão 4:1 a perda média de energia cai de 0.1706 para 0.1139 ao longo da
tangente.

Mas o que vale a pena contar é outra coisa. Escrevi duas verificações — «reduz-se
ao isotrópico» e «a anisotropia faz alguma coisa» — parti o modelo de propósito, e
**as duas passaram**. A primeira não podia apanhar o erro (com os eixos iguais ele
não existe); a segunda tinha o limiar abaixo do ruído, e passava com a anisotropia
**desligada**.

A que faltava não era sobre o resultado, era sobre o modelo: trocar `alpha_x` com
`alpha_y` e rodar a vista 90 graus é relabelar os eixos, e o número tem de ser o
mesmo. O modelo partido dá 2.66 contra uma tolerância de 0.03.

E o resíduo de 0.0161 que sobra é ruído, com prova: a 1024, 4096 e 16384 amostras
dá 0.0332, 0.0161, 0.0073 — parte-se a meio ao quadruplicar, que é 1/sqrt(N). O
sinal fica em 0.0436 nas três.

Três controlos negativos, cada um apanhado por uma verificação diferente. Nenhuma
das três é redundante, e descobri isso a partir cada uma.

## O gate de referência do IBL apanhou um bug de seis meses

O roadmap pedia: comparar o split-sum com uma integração Monte Carlo e publicar o
erro máximo. O `gate-ibl` faz isso em CPU, sem GPU, e a primeira coisa que publicou
foi que a nossa LUT estava errada.

`integrate_brdf` usava `G = Smith-Schlick com k = (a+1)²/8`. Esse `k` é a
remapeação para luzes **analíticas**; num integral sobre a hemisfera colapsa a
rasar. A N·V = 0.02 dava `G = 0.0196` onde a forma correcta dá `1.0`, e a LUT
devolvia 0.0164 onde a resposta é 0.886. O reflexo rasante não existia.

Era também uma incoerência interna: desde D44 a luz directa usa height-correlated.
O mesmo material respondia de uma maneira ao sol e de outra ao céu.

| F0 | médio antes | depois | máximo antes | depois |
|---|---|---|---|---|
| dieléctrico | 26.5% | **4.4%** | 98.1% | **18.4%** |
| metal | 21.8% | **5.3%** | 98.3% | **23.8%** |

O gate mede contra duas referências — sobre o mapa de 8 bits e sobre o céu
analítico — e dão 4.43% e 4.49%. **O que resta é o método, não os dados**: subir a
resolução do ambiente não ganharia nada mensurável.

Na imagem a mudança é pequena (7.7% dos pixels no `pbr-grid`, +0.1% de brilho) e a
razão é precisa: o erro era máximo em material liso a rasar contra céu brilhante, e
aquela cena é sol e esferas rugosas. Dizer «corrigi um erro de 26%» sem dizer isto
seria vender o peixe.

## Projectores, e dois testes meus que não testavam o que eu julgava

Omni e spot passaram a ser a mesma `Light`: um projector é uma luz pontual com um
cone, e uma omni é um projector com o cone aberto à esfera toda. Uma lista, um
percurso por pixel.

O cone entra no clustering como segundo teste, depois da caixa da esfera: cone
contra a esfera envolvente de cada cluster candidato. 1000 luzes, 334 projectores:
os slots caem de 45 394 para **21 364**, 52.9% cortados. O clustered faz 0.386 ms
contra 2.220 da força-bruta — **5.75×**, era 3.1× só com omni — e continua
**bit-idêntico**: 0 canais diferentes, 0 ULP.

Escrevi o teste que interessa (nenhum ponto iluminado num cluster que descartou a
luz), quebrei o código de quatro maneiras, e **duas passaram**:

A direcção do cone transformada como ponto em vez de vector passou porque a minha
câmara de teste estava na **origem** — ali as duas transformações dão o mesmo.
Movida para longe, o erro desvia o cone 19.6 graus e o teste **continuou a passar**,
porque o cone tinha 28 graus e sobrava sobreposição. Precisou de duas correcções:
estreitar o cone para 11 graus e um invariante directo — deslocar câmara e luz
juntas não pode mudar nada.

A rejeição «atrás do ápice» também podia sair sem nada falhar. Fui ver porquê antes
de escrever mais um teste: é **redundante** nesta formulação. Ficou escrita por ser
um corte barato, mas o comentário que eu tinha posto a dizer que era essencial era
falso e foi corrigido.

O gate ganhou chão. Sem superfície onde o cone pouse, um projector e uma omni dão a
mesma imagem — e um gate cuja imagem não distingue o que testa é mais fraco do que
parece.

## Sombras dinâmicas: o orçamento é uma optimização, não outra resposta

Fecha o ponto 7.2. O escalonador (`shadow_atlas.rs`) é uma fila por prioridade com
um tecto de trabalho e uma cache por versão, e não sabe nada de Vulkan.

Três decisões que valem a pena: o orçamento conta-se em **texels** e não em mapas
(um 512² custa 64 vezes um 64², portanto «oito mapas» não é orçamento nenhum); a
cache é o que torna o tecto honesto (sem ela «actualizei metade» quer dizer «metade
das luzes não tem sombra»); e as omni ficam de fora, porque precisam de seis faces
e isso é outro trabalho.

**O invariante.** Numa cena parada o resultado com orçamento tem de ser igual ao
resultado sem tecto. Com 512×512 texels: **0 pixels diferentes**. Com 128×128, 16×
menos: **0 pixels diferentes**. Ao frame 300 a pass de sombras custa **0.000 ms** —
a cache convergiu.

Para isso o `begin_color_pass` passou a ler `depth_clear: None` como **carrega** em
vez de limpar, e há um `clear_depth_rect` para limpar só os tiles do plano.

**Os números.** 1000 luzes, 334 projectores, atlas 2048²: 0.40 tiles desenhados por
frame, 3.00 adiados, pior espera 11 frames, e **24 de 334** projectores com sombra.
Os 24 não são um bug — 334 tiles de 512² são 87 M texels num atlas de 4 M. A fila
serve os importantes e adia o resto, que é o que um sistema com orçamento faz.

**E uma coisa que quase dei por partida.** Sombras ligadas contra desligadas mudam
1.73% dos pixels e o delta máximo é 5. Parecia avariado. Em vez de culpar a cena,
desenhei o **factor de sombra** em vez da cor: apareceu o poço de luz do projector
com as sombras redondas das esferas lá dentro, exactamente como deve ser. Era o
mecanismo certo com efeito pequeno — cada uma das 24 luzes é uma de mil.

O escalonador tem 13 testes sem GPU. Quebrei-o de cinco maneiras: quatro foram
apanhadas, e a quinta mostrou que eu tinha afirmado no comentário uma propriedade
mais forte do que a que estava testada. Dois testes chegaram a **pendurar** em vez
de falhar, porque tinham `while !plan.render.is_empty() {}` — agora têm limite.

## Render graph, e uma corrida que estava cá desde a fase 1

As passes passaram a **declarar** o que tocam, e as barreiras saem daí em vez de
serem raciocinadas caso a caso. O grafo valida, deriva as barreiras entre acessos
consecutivos do mesmo recurso, e resolve os `loadOp`. Não faz aliasing nem
reordena — isso são optimizações, e um grafo que reordena antes de se saber se
deriva as barreiras certas é indepurável.

Por D0 não conhece Vulkan: emite descritores neutros que o backend traduz. Toda a
derivação é testável sem GPU, e são 13 testes.

O `gate-terrain` foi o primeiro porte: **0 pixels diferentes** das barreiras à mão,
e o custo de construir o grafo fica **abaixo da resolução do relógio** (0.060 ms
com e sem, medido com a validação desligada).

**E depois o que interessa.** Um controlo mostrou que o grafo não apanha um acesso
que alguém se esqueça de declarar — só sabe o que lhe dizem. Isso motivou ligar a
*synchronization validation* do Vulkan, que é o outro lado do par: o grafo declara,
a camada verifica. Ligou-se, e apareceu logo:

> `SYNC-HAZARD-WRITE-AFTER-READ` na imagem da swapchain.

A transição de layout era emitida em `TOP_OF_PIPE`, mas o submit só espera pelo
semáforo do acquire em `COLOR_ATTACHMENT_OUTPUT` — a transição podia correr antes
da espera e escrever o layout de uma imagem que o motor de apresentação ainda lia.
**Em todos os 18 binários, desde a fase 1**, sempre com `validation_errors=0`,
porque a validation normal não vê corridas.

Uma linha a corrigir. Depois dela os 18 passam com a sync validation ligada.

## O porte: nenhum sample escreve barreiras à mão

Oito samples declaram o frame como grafo — `sponza`, `terrain`, `lights`,
`instances`, `fog`, `heightquery`, `furnace`, `bindless` — e **não resta uma única
chamada a `storage_barrier*`** na árvore.

Cada um verificado da mesma maneira: capturar com as barreiras à mão, capturar com
as derivadas, comparar. Cinco samples, treze alvos, **0 pixels diferentes** em
todos — incluindo o atlas de sombras de 4 M texels e os dois volumes do fog. O
`furnace` publica números em vez de pixels e dá exactamente os mesmos.

O `lights` é o caso para que isto foi feito: declarar o atlas como transiente em vez
de persistente faz o grafo **recusar o frame**, que é a classe do bug que o
`depth_clear: None` escondia.

A Sponza apanhou um erro meu durante o próprio porte: pus as chamadas onde estavam
os `storage_barrier` antigos — **depois** dos dispatches — e a barreira que protege
o volume de scatter saía depois de ele já ter sido lido.

E uma coisa que ainda não é verdade: as barreiras de attachment **somam-se** às
transições implícitas do RHI em vez de as substituírem. Medido na Sponza, 0.734
contra 0.730 ms de GPU — 0.5%, dentro do ruído. Dívida registada.

## Vegetação: culling por oclusão, e a pirâmide que estava a ser lixo

O gate `veg` fecha a fase 6, e não com culling por frustum — o `instances` já o
provou — mas com oclusão contra uma pirâmide Hi-Z construída **no mesmo frame**.

A Dagor corta a erva por feedback do pixel shader: desenha tudo, o PS marca um
bitvector, um compute compacta. O PS deles tem três caminhos de código com
intrínsecas de wave só para aliviar os atómicos, e a compactação está desligada
fora do DX12 de desktop. Aqui o teste é antes de rasterizar: um atómico por
instância que sobrevive, nenhuma intrínseca, e sem frame de atraso.

| | |
|---|---|
| plantas | 60 000 |
| passam o frustum | 34 089 |
| sobrevivem à oclusão | **4 701** (86.2% cortados) |
| pass da vegetação | **0.015 ms** contra 0.070 |
| custo da pirâmide + culling | 0.040 ms |

E a prova: o gate desenha as **duas** versões no mesmo frame e compara os 3 686 400
canais. **Zero diferentes.**

**O bug que isto apanhou era meu.** A primeira versão cortava 27% e fazia
desaparecer 60 pixels de erva. Testei três hipóteses erradas — folga de
profundidade, rectângulo alargado, cena sem chão — e as três deram exactamente os
mesmos 60 pixels, o que me devia ter dito logo que o problema não era a cena. A
causa: cada nível da pirâmide lia `texelFetch(..., 0)`, sempre o mip 0 em vez do
anterior. Corrigido: 27% → 86%, e 60 pixels → zero. Ler o mip errado é legal e
nenhuma camada de validação diz nada.

## A Sponza passou a GPU-driven: 596 draws para 15

Aplicar o Hi-Z à Sponza esbarrava numa coisa mais básica: ela desenhava um draw por
primitiva, com um par de VB/IB por primitiva. Um draw indirecto múltiplo precisa de
um só de cada.

**Antes de construir, medi onde paga.** A pirâmide custa 0.032 ms fixos. A pass do
terreno custa 0.042 — mesmo cortando tudo não pagava, e fica registado como medido.
A da Sponza custa 0.350 mais 0.196 das cascatas: aí paga.

O que mudou: um VB e um IB para as 103 primitivas (192 496 vértices), uma tabela
por primitiva, e uma lista fixa de comandos cujo `instanceCount` o compute escreve.
Os VS passaram de `.spvasm` escrito à mão a GLSL; os PS levaram um patch de duas
cargas. O índice da primitiva vai no `firstInstance` e chega ao shader como
`gl_InstanceIndex`, sem precisar de `gl_DrawID`.

| | antes | depois |
|---|---|---|
| draws | 596 | **15** |
| CPU do frame | 0.360 ms | **0.120 ms** |
| GPU | 0.736 ms | 0.736 ms |
| imagem | — | **0 pixels** no composite |

**Dois erros, e nenhum deu erro.** O `firstInstance` precisa da feature
`drawIndirectFirstInstance` e a validation não o apanha — o conteúdo do buffer
indirecto é do lado da GPU. E ao tirar a escrita de CBV que era por primitiva,
deixei de a escrever de todo: a pass da cena passou a ler o que ficou da anterior,
e a Sponza ficou com um banho vermelho por cima das texturas certas.

O ECS continua a marcar `Visible`, mas já não escolhe o que se desenha — passou a
ser a **referência**: o `finish` lê os comandos de volta e exige que o número bata
com o da CPU. 78 de 103, nos dois.

## Hi-Z na Sponza: feito, verificado, e desligado

O caminho está escrito — duas fases, pirâmide do mesmo frame, imagem **idêntica ao
pixel** nos três alvos — e custa mais do que poupa.

| | soma das passes | frame |
|---|---|---|
| com oclusão | **0.399 ms** | 0.780 ms |
| sem oclusão | 0.353 ms | 0.736 ms |

Poupa 3 primitivas de 78 e a pirâmide custa 0.033 ms: o frame fica 6% mais lento.

A razão não é o método, é a granularidade. A Sponza tem 103 primitivas com ~15 000
triângulos cada — o chão inteiro é uma — e uma primitiva dessas quase nunca está
**inteiramente** tapada. A mesma máquina corta 86.2% no `gate-veg`, onde as unidades
são plantas de seis vértices.

Fica desligado por omissão, atrás de `-- --occlusion`. Fica porque é a fundação do
culling por meshlet, que é o que falta para isto pagar. Ligá-lo por omissão seria
vender como optimização uma coisa que medi a tornar o frame mais lento.

## Meshlets: a granularidade certa, e mesmo assim não paga

O passo anterior concluiu que a oclusão não pagava na Sponza porque as unidades eram
grandes de mais. A cura óbvia era partir em meshlets. Fiz, e **também não paga** —
por outra razão, que também se mede.

Cada primitiva parte-se em grupos de N triângulos ordenados por código de Morton do
centróide (cortá-los pela ordem do ficheiro daria caixas a cobrir meia primitiva).
Cada meshlet leva a matriz e o material da sua primitiva, por isso a tabela tem a
mesma forma: **nenhum shader mudou**.

Com 128 triângulos a Sponza passa de 103 a 2 097 meshlets e a oclusão passa de
cortar 3 para **116**. E o frame:

| tris/meshlet | meshlets | cortados | frame |
|---|---|---|---|
| 128 | 2 097 | 116 | 0.948 ms |
| 512 | 569 | 13 | 0.837 ms |
| 2 048 | 204 | 2 | 0.813 ms |
| **um por primitiva** | **103** | 1 | **0.737 ms** |

Monótono, e o melhor é não os usar. A causa é o **custo fixo por comando
indirecto**: ~0.1 ms por cada mil. Não é o culling que está errado — é desenhar um
meshlet por draw.

O que faria pagar são **mesh shaders**: os meshlets passam a workgroups de um
dispatch e o custo por comando desaparece. Este trabalho é a fundação disso.

Fica `-- --meshlets N` para experimentar, e por omissão um meshlet por primitiva.
A imagem é idêntica ao pixel em todos os tamanhos.

E um erro que a medição apanhou: a primeira versão ordenava por Morton **sempre**,
mesmo com um meshlet só. Aí não agrupa nada e estraga a localidade que o ficheiro já
tinha — 0.736 → 0.772 ms, 5% por nada.
