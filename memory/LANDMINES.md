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

## Bindless (ainda não ligado — não esquecer na fase 2)

- Slot 0 = dummy 1×1, barreira VS+PS+CS **antes** de qualquer sample.
- Upload **por mip**. SampleLevel em mip UNDEFINED = GPUVM.
- `t0` e `u0` no mesmo space = mesmo binding. UAVs de buffer a partir de `u1`.
- Arrays HLSL `RWStructuredBuffer[N]` ≠ N descriptors no SPIR-V. Buffers soltos.
- Não destruir `PipelineLayout` depois de criar o PSO.

## SPIR-V do hello-triangle

- `OpConstant %float 1` no assembler mínimo (`prog/tools/assemble_spvasm.py`) **não** é 1.0: grava bits `0x1` (~0). `w≈0` → triângulo clipado, ecrã preto. Sempre `1.0` / `0.0` no `.spvasm`. O assembler agora trata inteiros em `OpConstant` de `%float` como f32.

## Hello-triangle (fase 1)

- Pipeline de dynamic rendering fica inválida se o `present_format` mudar no recreate (raro). Recriar PSO.
- Sem bindless neste binário. Não “preparar” um heap vazio mal configurado.
- `present` SUBOPTIMAL + `Resized` no mesmo ciclo: limpar `pending_recreate` depois de `resize()` senão o swapchain nasce duas vezes.
- Este host muitas vezes **não** tem `vulkan-validationlayers` no apt. `harpia-rhi` procura o JSON/SO (sistema, `VULKAN_SDK`, `HARPIA_VK_LAYER_PATH`, `prog/3rdPartyLibs/`, e por último `.deps` ao lado do repo) e exporta `VK_LAYER_PATH` **antes** de abrir o loader. Preferir `sudo apt install vulkan-validationlayers`.

