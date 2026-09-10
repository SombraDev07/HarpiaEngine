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
- `prog/tools/dis_spv.py` desmonta um `.spv` quando o validador não ajuda. Foi
  assim que se confirmou que o stream estava bem formado e o problema era outro.

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


