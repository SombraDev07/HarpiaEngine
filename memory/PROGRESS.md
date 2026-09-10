# Progress

**Fase actual:** 5 (clima) **em curso** (Linux / RADV, 2026-09) — fog, céu e nuvens
verdes e vistos em pixels. Falta water e rain. Fase 4 fechada com PCSS.

Quadro: `docs/Rust-Rewrite-Roadmap.md` §15. Este ficheiro é o diário; o roadmap é o mapa.

## Checks

- [x] Fase 0 — contrato (Null, `--frames`, spec bindless escrita antes do shader)
- [x] Fase 1 — `cargo run -p hello-triangle -- --frames 90` — validation 0, resize 30/60
- [x] Fase 2 — `cargo run -p gate-bindless` — 16 frames, resize 6/12, validation 0; Null `--frames 8`
- [x] Fase 3 — deferred PBR, `cargo run -p gate-pbr-grid -- --frames 90`, validation 0, resize 30/60; Null `--frames 8`
- [x] Fase 4 — CSM câmara real + TAA + Sponza 90
- [~] Fase 5 — fog → clouds → water → rain (fog, céu, clouds **feitos**; falta water, rain)
- [ ] Fase 6 — um clipmap + veg + mundo
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
esferas a dissolver-se com a distância, com height fog no chão.

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

## Próximo (fase 5) — o que fazer, em ordem

Não mesh shaders, RT, FSR, editor. Não VSM.

1. ~~Fog: froxels, 3D GENERAL. Gate `fog`.~~ **feito**
2. ~~Céu: Hillaire completo. Gate `sky`.~~ **feito** (falta aerial perspective)
3. ~~Clouds: raymarch Nubis a meia resolução. Gate `clouds`.~~ **feito**
4. **Reprojecção temporal das nuvens** — é o que come o ruído que sobra no
   horizonte, e sem ela a meia resolução nota-se em movimento.
5. **Sombra das nuvens nos froxels do fog.** É daqui que vêm os god rays **sem
   acrescentar um pass** — o fog já marcha, só lhe falta ler a transmitância das
   nuvens.
6. Water: point-sample depth no SSR.
7. Rain **por último** (GBuffer wet + post; cones sem HDR SRV; `--frames 16` only).
8. Default-on na Sponza (`--frames` curto) ou o pass não entra. Marcar fase 5 `[x]`
   no roadmap §15 **neste mesmo PR**.

### Dívida conhecida (documentada, não esquecida)

- Multiscattering do céu sem bounce do chão (albedo 0) → horizonte um pouco escuro.
- Aerial perspective (froxel 32³) por fazer — entra quando a Sponza receber céu.
- Sponza ainda não tem céu nem nuvens: os gates provam os passes isolados.
