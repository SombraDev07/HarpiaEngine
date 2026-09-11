# Landmines

Copiado do roadmap C++ + o que esta sessão já toca. Actualizar quando GPUVM / validation / resize morderem.

## AMD / RADV

- GPUVM (SQC / PERMISSION_FAULTS) **não** aparece na validation. Kernel page-fault → MODE1 reset.
- Mitigação: `--frames N`, `MESA_VK_ABORT_ON_DEVICE_LOSS=1`, nunca relançar interactivo depois de GPUVM.
- Gates e samples: default com `--frames`. Nunca unbounded por omissão. `--interactive` é opt-in e arriscado em RADV.

## Swapchain

- Não destruir a `VkSurfaceKHR` no resize. Uma surface por window.
- Passar `oldSwapchain` na criação. Destruir o swapchain antigo **depois** do create.
- `present_format` real: no X11/Mesa costuma ser **BGRA**, não RGBA hardcoded.
- Extent 0×0 (minimize): não adquirir imagem; skip o frame.

## Frame

- `begin_frame` espera a fence do slot. Sem `vkDeviceWaitIdle` no frame quente.
- WaitIdle só em shutdown / testes.
- Callback de validation pode correr noutro thread → mimalloc (TLS).

## Bindless (fase 2)

- Slot 0 = dummy 1×1, barreira VS+PS+CS **antes** de qualquer sample.
- Upload **por mip**. SampleLevel em mip UNDEFINED = GPUVM. Uma barreira UNDEFINED→TRANSFER_DST para **todos** os mips, depois copies, depois TRANSFER_DST→SHADER_READ.
- Row pitch **256** em `copy_buffer_to_texture` (`bufferRowLength` em texels).
- UAV view = **mip 0** se o recurso tem mips. UAV 2D em GENERAL; sampled do mesmo recurso também em GENERAL.
- `t0` e `u0` no mesmo space = mesmo binding. UAVs de buffer a partir de `u1`. Set 3 fica vazio (placeholder). Storage image no set 4 binding 0.
- Não destruir `PipelineLayout` depois de criar o PSO.
- SPIR-V 1.4+: `OpEntryPoint` tem de listar UBO / heap / sampler / UAV, não só Input/Output.
- Caps no assembler: `RuntimeDescriptorArray=5302`, `ShaderNonUniform=5301`, `SampledImageArrayNonUniformIndexing=5307` (não os números 5074/5079 do draft antigo).

## SPIR-V do hello-triangle

- `OpConstant %float 1` no assembler mínimo (`prog/tools/assemble_spvasm.py`) **não** é 1.0: grava bits `0x1` (~0). `w≈0` → triângulo clipado, ecrã preto. Sempre `1.0` / `0.0` no `.spvasm`. O assembler agora trata inteiros em `OpConstant` de `%float` como f32.

## Hello-triangle (fase 1)

- Pipeline de dynamic rendering fica inválida se o `present_format` mudar no recreate (raro). Recriar PSO.
- Sem bindless neste binário. Não “preparar” um heap vazio mal configurado.
- `present` SUBOPTIMAL + `Resized` no mesmo ciclo: limpar `pending_recreate` depois de `resize()` senão o swapchain nasce duas vezes.
- Este host muitas vezes **não** tem `vulkan-validationlayers` no apt. `harpia-rhi` procura o JSON/SO (sistema, `VULKAN_SDK`, `HARPIA_VK_LAYER_PATH`, `prog/3rdPartyLibs/`, e por último `.deps` ao lado do repo) e exporta `VK_LAYER_PATH` **antes** de abrir o loader. Preferir `sudo apt install vulkan-validationlayers`.
- **DXC ausente** neste host. Não “corrigir” shaders só com HLSL e assumir que o cook corre — o binário come o `.spvasm`. Caps oficiais SPIR-V: ver bindless acima (5302/5301/5307), não 5074.

## Fase 3 (GBuffer / IBL / assembler)

- Barreira de depth usa **aspect DEPTH**, não COLOR. View da D32 também. Após o GBuffer, cores → `SHADER_READ_ONLY` e `write_sampled` **antes** do lighting no mesmo frame (update-after-bind).
- MRT: 5 color + D32, dynamic rendering. PSO `color_formats` explícitos; lighting continua com formato do swapchain.
- IBL: upload **todos** os mips do prefiltered. `SampleLevel` / `OpImageSampleExplicitLod` em mip UNDEFINED = GPUVM no RADV.
- GBuffer no resize: recria texturas; as antigas **leak até ao drop do device** (90 frames, 2 resizes — slots a mais, aceitável). Não `waitIdle` no frame quente para alocar.
- `assemble_spvasm.py`: `OpUDiv` é **134** (estava 132 = `OpIMul` — o checker do `gate-bindless` CS estava errado e o gate ainda passava). `PushConstant` = **9**. `FMix` vec3 precisa de `a` vec3.
- VB/IB da fase 3 são `CpuToGpu` (host visible). Sem copy+waitIdle no init.

## Opcodes do assembler (a mina mais cara até agora, 2026-09)

`prog/tools/assemble_spvasm.py` tinha **`OpFOrdLessThanEqual = 181`**. 181 é
**`OpFUnordEqual`**. Todo o `a <= b` dos shaders virou `a == b`:

- CSM: `uv <= 1` ⇒ falso ⇒ `inside` falso ⇒ `shadow = 1` ⇒ **nunca havia sombra**,
  em `gate-csm` e na Sponza. O atlas estava perfeito o tempo todo.
- CSM: `ndz <= depth + bias` ⇒ igualdade ⇒ PCF sempre 0 (irrelevante, o `inside`
  já matava).
- TAA: `hx <= 1` / `hy <= 1` ⇒ history sempre rejeitada ⇒ **o TAA não fazia nada**.
- **Zero erros de validation** em qualquer destes casos. O SPIR-V é válido, só faz
  outra coisa.

Também estavam errados: `OpUMod` (139 = `OpSMod`; o certo é **137** — safava-se por
os operandos serem pequenos e positivos) e `OpConvertSToF` (117 = `OpConvertPtrToU`;
o certo é **111**).

Regra: **antes de acrescentar um opcode à tabela, confirma o número na spec.** Os
pares Ord/Unord são intercalados (180 FOrdEqual, 181 FUnordEqual, 182 FOrdNotEqual,
183 FUnordNotEqual, 184 FOrdLessThan, 185 FUnordLessThan, 186 FOrdGreaterThan,
187 FUnordGreaterThan, 188 FOrdLessThanEqual, 189 FUnordLessThanEqual,
190 FOrdGreaterThanEqual, 191 FUnordGreaterThanEqual). A aritmética também:
134 UDiv, 135 SDiv, 136 FDiv, 137 UMod, 138 SRem, 139 SMod. Já é o **terceiro**
opcode errado apanhado (o primeiro foi `OpUDiv` = 132). Se houver `spirv-as` no
host, ele ganha sempre.

## Mais opcodes do assembler (e como os apanhar cedo)

- `OpImageSampleDrefExplicitLod` é **90**. 91 é `OpImageSampleProjImplicitLod` —
  o spirv-val disse exactamente isso em 2 segundos. A família: 87 SampleImplicit,
  88 SampleExplicit, 89 SampleDrefImplicit, **90 SampleDrefExplicit**,
  91 SampleProjImplicit, 92 SampleProjExplicit, 95 Fetch, 96 Gather,
  97 DrefGather, 98 Read, 99 Write.
- `Rgba16f` no `ImageFormat` é **2** (1 = Rgba32f, 3 = R32f, 4 = Rgba8).
- `OpImageSampleDrefExplicitLod` **90**, `OpImageSampleProjImplicitLod` 91.
- `prog/tools/dis_spv.py` desmonta um `.spv` quando o validador não ajuda. Foi
  assim que se confirmou que o stream estava bem formado e o problema era outro.

## `OpSampledImage` fora do bloco que o consome — outra vez

Já estava escrito na fase 4 («tem de ser consumido no mesmo bloco») e mordeu na
mesma. No `sky.ps` o `OpSampledImage` da LUT de transmittance nascia no bloco de
entrada e era usado dentro do loop do raymarch. spirv-val recusa, **com mensagem
vazia**.

Os shaders do PCSS faziam o mesmo e passavam — sorte, não regra. Já foram
corrigidos (a saída é bit-a-bit igual). **Cria o `OpSampledImage` no bloco onde o
amostras**, sempre.

E dentro de um loop usa **LOD explícito** (`OpImageSampleExplicitLod` /
`OpImageSampleDrefExplicitLod` com `Lod 0`): LOD implícito pede derivadas que não
existem em fluxo de controlo não uniforme.

## O assembler agora valida ids

`assemble_spvasm.py` recusa um `%id` usado e nunca definido, e um `%id` definido
duas vezes. Isto nasceu de uma hora perdida: um `blit.ps` gerado por regex ficou
com `%ptr_cb = OpTypePointer Uniform %Fog` (o `%Fog ` com espaço foi substituído,
o do fim da linha não). O módulo inválido era esse, e eu andei a bissectar **outro**
shader contra uma baseline contaminada.

Regra: **descobre qual módulo falha antes de bissectar.** Troca um shader por um
trivial e conta os erros — o `vkCreateShaderModule` falha uma vez por módulo.

## `OpKill` com bloco de merge vazio é rejeitado

O spirv-val recusa um fragment shader em que o ramo do `OpKill` funde num bloco
que só tem `OpReturn` — e a mensagem vem **vazia**, o que não ajuda nada. O mesmo
`OpKill` passa noutro shader onde o bloco de merge tem trabalho a seguir.
Bissecção: mínimo passa, sem kill passa, `OpReturn` em vez de kill passa, kill +
merge vazio falha.

No `shadow.ps` da Sponza a saída é ter um output de cor a escrever o alpha que foi
testado. O pass é depth-only, portanto o valor é deitado fora — mas o shader fica
legível em vez de ter um no-op só para agradar ao validador. **Não apagues esse
output.**

## Verificação visual: usa `--capture`, não screenshots

- `--capture <prefixo>` grava `<prefixo>.<nome>.png` de cada `capture_targets()`
  do sample, lendo a textura de volta da GPU depois do último frame.
- Screenshots da janela correm contra o compositor e o WM: `gnome-screenshot -w`
  apanha a janela com foco, e uma corrida com `wmctrl` dá capturas da janela errada
  (ou do ecrã bloqueado). Isso já produziu diffs "idênticos" falsos que apontaram
  para o diagnóstico errado durante meia hora.
- O atlas de sombra sai como PNG cinzento com min/max no log — foi assim que se
  provou que o atlas estava certo e o problema era o lookup.

## Convenção de clip space (mordeu na Sponza, 2026-09)

- **Vulkan tem +Y para baixo no framebuffer.** `glam::Mat4::perspective_rh` já dá
  profundidade `[0,1]` mas mantém o +Y do OpenGL. Sem negar a linha Y a imagem sai
  **espelhada na vertical** — e como o *facing* é decidido **depois** da viewport,
  a winding também inverte: `FRONT_FACE_COUNTER_CLOCKWISE` + cull-back passa a
  matar as faces **da frente** e mostra o interior de tudo.
- Uma só entrada: `harpia_math::perspective_vk`. Não chamar `Mat4::perspective_rh`
  em samples.
- Sintoma na Sponza: víamos o intradorso das arcadas e do chão, com a cena de
  cabeça para baixo — parecia "reflexo", era culling invertido.
- As malhas (`SphereMesh::uv`, `plane_xz`) estavam enroladas **ao contrário** para
  casar com o bug. Já corrigidas; `mesh::tests::winding_faces_outwards` fixa isso.
- As matrizes da luz (CSM) ficam **sem** o flip: o lookup do atlas assume
  `uv.y = ndc.y*0.5+0.5`. Se alguém puser o flip na luz, tem de virar o `v` nos
  PS que amostram o atlas.

## CBV por draw (mordeu na Sponza, 2026-09)

- Set 0 binding 0 era `UNIFORM_BUFFER` **não dinâmico**: um buffer por slot de frame,
  escrito no offset 0. Escrever o CB N vezes no mesmo frame **não** dá N constantes —
  a GPU só vê a **última**, porque o CPU já escreveu tudo antes do submit.
- Na Sponza isso punha `gbuf0` = índice do próprio scene RT em **todos** os draws do
  pass de cor → o PS amostrava o render target que estava a escrever → lixo
  verde/amarelo em blocos (DCC do RADV), sem **um único erro de validation**.
- Agora é `UNIFORM_BUFFER_DYNAMIC` + anel por slot (`FRAME_CBV_CHUNKS = 512` chunks
  de `FRAME_UBO_SIZE`). Cada `write_frame_bytes` toma um chunk; `draw`/`draw_indexed`/
  `dispatch` re-apontam o set 0 se o chunk mudou desde o bind.
- Regra: **escreve o CB, depois desenha.** Estourar o anel é erro explícito
  (também no backend Null, para os gates apanharem em CPU).
- `dynamicOffset + range <= size` e offset múltiplo de `minUniformBufferOffsetAlignment`
  — validado no `Bindless::create`.

## Texturas glTF (Sponza)

- Uma textura GPU por **imagem**, não por primitiva: 103 primitivas partilham 25
  imagens. Antes eram 103 cópias (RAM + slots bindless + upload).
- **Mips gerados em espaço linear.** O albedo é sRGB: fazer a média dos texels em
  sRGB escurece. `gltf_scene::downsample_srgb` decodifica, faz box 2×2, recodifica.
- `sampled_desc` (não `color_desc`) para texturas amostradas: sem `COLOR_ATTACHMENT`.

## Fase 4 (CSM / TAA / Sponza)

- Pass depth-only: `begin_color_pass(&[], Some(atlas), …)`. `PipelineTargets.depth_only` — **não** tratar `color_formats` vazio como swapchain (hello-triangle).
- `OpSampledImage` tem de ser consumido **no mesmo bloco** (TAA PS: recriar o sampled image no ramo do neighbourhood).
- Snap de texels: o centro da esfera no light-view já está em (0,0) — snap em light-space XY é no-op. Snap **antes** do `look_at`, nos eixos right/up da luz, com `world_per_texel` do tile (`atlas/2`).
- `LightingCb` 528 B: campos de sombra a partir do offset 176. Shaders da fase 3 não os lêem; `cascade_count == 0` no PS do `gate-csm` / Sponza salta o PCF.
- UV: location 2 no VS só quando não há instance buffer. Com `instance_stride > 0`, location 2 continua a ser a primeira attr de instância (`pbr-grid` / `gate-csm`).
- Staging do heap: **64 MiB**. 1 MiB rebentava no primeiro albedo 2k da Sponza (`row pitch 256`).
- Sponza: `python3 prog/tools/fetch_sponza.py` (jsDelivr). Sem `assets/sponza/glTF/Sponza.gltf` o sample sai com erro claro, não GPUVM.

## Fase 5 (fog / céu / nuvens)

- PSO fullscreen com `PipelineTargets::default()` = formato da swapchain (BGRA). A
  desenhar para um RT `Rgba8Unorm` dá
  `VUID-vkCmdDraw-dynamicRenderingUnusedAttachments-08910`. **Declara sempre
  `color_formats`** do RT de destino. Apanhado duas vezes: fog e clouds.
- O `blit` final **não** volta a fazer tonemap: quem compôs já fez ACES + sRGB.
  Dois tonemaps dão um céu leitoso que parece bug de exposição e não é.
- `--capture` de um volume sai como grelha de slices num PNG — é o único modo
  honesto de ver um froxel ou o Perlin-Worley antes de acreditar na composição.
- Gates: o pacote da Sponza chama-se **`sponza`**, não `harpia-sponza`. E o
  `validation_errors=…` do `tracing` vem com escapes ANSI no meio — `grep
  'validation_errors=0'` **falha sempre** num pipe; tira os escapes primeiro
  (`sed 's/\x1b\[[0-9;]*m//g'`). Já deu um falso vermelho num sweep inteiro.
- Acumulação temporal sem jitter que mexe por frame é uma média de amostras
  idênticas: parece que funciona (a imagem não muda) e não reduz ruído nenhum.
  Prova com números — |laplaciano| na banda do problema, antes e depois — e usa
  uma **matriz errada de propósito** como controlo.
- Fase de scattering: confirma **de que ponta** apontam os vectores antes de
  acreditar num `dot`. HG/Cornette-Shanks querem o ângulo entre a propagação
  incidente e a dispersada; com os dois vectores a sair do ponto o cosseno vem
  simétrico e o lóbulo para a frente vai parar atrás da câmara. O fog tinha-o
  trocado, as nuvens não — e nenhum dos dois parecia errado a olho.
- `set_viewport` fora de um pass devolve `PassMismatch`, que o `harpia_app`
  reporta como «swapchain pass mismatch (begin/end)» — mensagem que não aponta
  para o viewport. `begin_color_pass` já repõe o viewport inteiro; não é preciso
  repor à mão depois do pass das cascatas.
- Um `str.replace` de Python que não casa **não falha** — só não faz nada, e o
  resultado é um alvo de `--capture` que nunca aparece. Põe sempre `assert old in s`
  antes de reescrever um ficheiro.
- Um CB desalinhado do shader **não falha validation**: o bloco continua do
  tamanho certo, a GPU é que lê os bytes errados e sai uma imagem plausível e
  errada. Os testes `*_layout_matches_the_spvasm` agora **lêem mesmo** o
  `.spvasm` (`spvasm_layout::assert_prefix_matches`) e comparam membro a membro —
  antes só comparavam os offsets do Rust com números escritos no mesmo ficheiro,
  o que prova que o struct não mexeu, não que ainda casa com o shader.
- Qualquer coisa que dependa do relógio ou do input **quebra a reprodutibilidade
  do `--capture`**, que é a base de toda a verificação desta árvore. Por isso o
  `dt` é fixo e o `Input` fica vazio quando `--frames N` está ligado (D27). Se
  acrescentares estado que evolui no tempo, verifica-o contra capturas de
  referência antes e depois.
- **SSR com espessura fixa só apanha silhuetas.** Se os reflexos saírem como
  contornos ocos, é isso: a janela de profundidade tem de cobrir uma passada
  inteira e crescer com a distância.
- Duas vezes nesta sessão apareceu **um** erro de validation
  (`vkCreateShaderModule`, spirv-val) logo a seguir a editar um shader, e não
  reproduziu em 9 corridas seguintes (incluindo reconstruções forçadas, com o
  `.spv` correcto confirmado dentro do binário). **Não sei a causa.** Regra:
  volta a correr antes de acreditar num falhanço isolado logo após editar um
  shader — mas **nunca** o descartes sem repetir.
- Ao **gerar** decorações de bloco por fórmula, confere a fórmula contra um
  membro conhecido. `112 + i*16` em vez de `96 + i*16` deslocou todos os `%v4` da
  chuva em 16 bytes: zero erros de validation, e o sintoma foi uma faixa branca no
  ecrã. Corre `cargo test -p harpia-render` **antes** de ir depurar a imagem.
- **`fract(sin(x) * 43758.5)` não é portável entre CPU e GPU.** Medido: 77 m de
  erro máximo e 99.84% dos pontos fora da tolerância num terreno de ±60 m (D41).
  Qualquer função partilhada entre os dois lados usa hash **inteiro**.
- `read_texture` espera pelo device: só pode ser chamado fora de um frame. Usa
  `Sample::finish` (D42).

- **Um draw indirecto indexado exige um index buffer ligado** mesmo que o
  `firstIndex`/`vertexOffset` sejam zero (VUID-vkCmdDrawIndexedIndirect-None-07312).
  Para geometria que nasce do `gl_VertexIndex` a variante é `vkCmdDrawIndirect`,
  com quatro u32 em vez de cinco (D47).
- **Ao mapear um quad 2D para as seis faces de um cubo, a permutação dos eixos tem
  de ser cíclica.** `(q.x, q.y, sign)` para o eixo Z em vez de `(q.y, q.x, sign)` é
  uma transposição: inverte o winding, e o back-face culling comeu as duas faces de
  Z. Zero erros de validation. Não se vê a olho — o cubo continua a parecer um
  sólido de três faces. Viu-se a calcular os 36 vértices em Python e a comparar o
  normal do winding com o normal pretendido: 4 triângulos em 12 invertidos. A
  correcção repôs 24.1% → 32.2% de cobertura do frame.
- Verificar um resultado da GPU contra um teste de CPU **diferente** não verifica
  nada: a folga entre os dois absorve o erro. Esfera na GPU contra caixa na CPU dava
  496 contra 506 e passava. O teste da CPU tem de ser o mesmo, linha por linha, e a
  igualdade tem de ser exacta (D47).
- **Uma thread por item é pouco quando o item custa.** 448 patches, uma thread cada,
  81 avaliações de FBM em série por thread: 7 workgroups para uma GPU inteira e
  0.050 ms de dispatch. Um workgroup por patch, com as amostras repartidas pelas 64
  lanes e uma redução em memória partilhada: 0.018 ms, imagem bit-idêntica (D48).
- A caixa envolvente de um patch de terreno vem das alturas nos **vértices**, e o
  que se desenha entre eles é a interpolação linear, não o campo verdadeiro. Um
  teste que amostre `terrain_height` no interior de uma célula acusa a caixa de um
  erro que é do teste — falhou por 13 mm ao primeiro. O que a caixa tem de conter
  são os vértices que o VS emite (D48).
- `patch` é palavra reservada em GLSL (tesselação). `patch_id` compila.
- Quando um filtro de binários (`grep -E '^(gate-|...)$'`) não casa o que devia, o
  varrimento diz «0 falhados» e parece verde. Imprime **quantos** correram e
  confere o número antes de acreditares.
- **Um limiar tem de ser calibrado contra o ruído, não escolhido.** «O máximo de
  |T − B| > 0.02» parecia razoável e passava com a anisotropia **desligada**: o
  máximo do ruído de Monte Carlo num material isotrópico é 0.0332. Sobre a média o
  ruído é 0.0011 e o sinal 0.0436. Mede o ruído primeiro (D49).
- Quando um resíduo pode ser ruído ou erro, **varia o número de amostras**. Se se
  partir a meio ao quadruplicar, é 1/sqrt(N) e é ruído; se não se mexer, é erro.
  Mediu-se 0.0332 → 0.0161 → 0.0073 para o resíduo e 0.0436 → 0.0436 → 0.0436 para
  o sinal (D49).
- Uma verificação que não falha quando quebras o código de propósito **não é uma
  verificação**. Quebra sempre, uma vez por cada afirmação que o teste faz. Duas
  das três que escrevi para a GGX anisotrópica passaram com o modelo partido (D49).
- `tan(2*pi*xi + pi/2)` na amostragem anisotrópica tem singularidades em xi = 0 e
  xi = 0.5, e a sequência de Hammersley acerta nas duas. `atan2(ay*sin t, ax*cos t)`
  é a mesma inversão sem singularidade nenhuma.
- **O `k` do Smith-Schlick não é o mesmo para luz directa e para IBL**, e usar o da
  luz directa (`k = (a+1)²/8`) num integral sobre a hemisfera colapsa o termo a
  rasar: `G = 0.0196` onde a forma height-correlated dá `1.0`. A LUT do IBL perdia
  o reflexo rasante inteiro e ninguém reparava porque a imagem continuava
  plausível (D50).
- Quando duas partes do motor implementam o **mesmo** modelo (a luz directa e o
  IBL), uma pode ficar para trás numa correcção sem que nada acuse. D44 corrigiu o
  `G` da luz directa; a LUT do IBL ficou seis meses com o antigo. Procura as outras
  cópias quando corriges um modelo.
- Um erro medido num LUT ou num integral **não se traduz proporcionalmente na
  imagem**: 26% de erro médio deu 0.1% de brilho no `pbr-grid`, porque o erro era
  máximo onde aquela cena não tem sinal. Mede os dois, e diz qual é qual (D50).
- **Uma câmara de teste na origem não distingue `transform_point3` de
  `transform_vector3`.** Sem translação as duas dão o mesmo, e um teste escrito
  assim fica cego à classe inteira de erros «direcção transformada como ponto».
  Há um teste que verifica que a câmara de teste **não** está na origem (D51).
- Um teste de cone/frustum com ângulo largo tolera erros de direcção grandes: com
  28 graus de meio-ângulo, um desvio de 19.6 graus passava. Para testar direcção,
  usa o **caso estreito** — ou, melhor, um invariante que não dependa da margem
  (deslocar câmara e objecto juntos não pode mudar o resultado) (D51).
- Quando tiras uma linha e nenhum teste falha, há duas hipóteses e só uma é «falta
  um teste»: a outra é que a linha era **redundante**. Vai ver qual das duas antes
  de escrever o teste — e corrige o comentário que dizia que ela era essencial (D51).
- **`while <condição> {}` num teste é um pendurado à espera de acontecer.** Dois
  testes do escalonador de sombras penduraram quando quebrei a cache de propósito,
  em vez de falharem. Põe sempre um limite de iterações com uma mensagem (D52).
- `color_formats: &[]` num PSO **não** dá zero alvos de cor: sem `depth_only: true`
  o RHI cai para o formato da swapchain, e a validation reclama que o pipeline tem
  1 alvo e a pass tem 0 (D52).
- Os samplers do set 2 são **bindings separados** (0 wrap, 1 clamp, 2 compare), não
  um array. Declarar `uniform sampler samplers[]` e usar `samplers[1]` pede 2
  descritores num binding que só tem 1 (D52).
- Quando um efeito mede pouco, verifica se o mecanismo está partido **antes** de
  culpar a cena: desenha o valor intermédio em vez da cor. As sombras mudavam 1.7%
  dos pixels e pareciam partidas; a visualização do factor de sombra mostrou o poço
  do projector com as sombras certas lá dentro (D52).
