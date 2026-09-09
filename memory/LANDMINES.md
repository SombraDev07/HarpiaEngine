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

