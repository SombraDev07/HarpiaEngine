# Arquitetura — HarpiaEngine

## Camadas

```
Samples/            executáveis; podem usar tudo
   │
Source/RHI/         Vulkan. ÚNICO lugar onde `vk*` pode aparecer
   │
Source/Platform/    janela, input. GLFW. Zero Vulkan
   │
Source/Core/        memória, threading, profiling. Zero dependência de plataforma
```

A dependência é estritamente para baixo. `Core` não conhece `Platform`; `Platform` não conhece
`RHI`. O `Window` expõe `GLFWwindow*` e o RHI cria a surface — é assim que a regra
"nenhum `vk*` fora de `Source/RHI`" sobrevive ao contato com a realidade.

## Invariantes

Estes não são estilo. São o que impede a engine de virar legado.

1. **Nenhuma chamada `vk*` fora de `Source/RHI/`.** Substitui a abstração de RHI que
   deliberadamente não construímos (roadmap 1.4). É o que mantém um segundo backend possível
   sem pagar por ele hoje.
2. **Nenhum subsistema cria thread própria.** Tudo passa pelo `JobSystem`. Uma thread solta é
   uma race condition esperando acontecer.
3. **Zero `new`/`malloc` no caminho de render por frame.** Transiente vai para `Arena`,
   vida longa vai para `Pool`.
4. **Objetos de vida longa são referenciados por `Handle<T>`, nunca por ponteiro cru.**
   Handle obsoleto resolve para `nullptr`; ponteiro obsoleto corrompe memória.
5. **Toda alocação nossa carrega um `MemTag`.**
6. **Todo recurso Vulkan recebe nome de debug.** Um capture com `Buffer_0x7f3a` custa uma hora;
   com `CSM_Cascade2_Depth`, um minuto.
7. **Validation layers em zero.** `VulkanDevice::validationErrorCount()` é assertado nos testes.
8. **Barreiras via `synchronization2`, com stage/access explícitos.** Nunca `ALL_COMMANDS`
   por preguiça — o render graph vai gerar isso depois, e um barrier largo hoje vira stall lá.
9. **Toda fase termina em imagem verificável**, comparada numericamente (`Tests/ImageCompare.h`).

## Decisões de fundação travadas

| Decisão | Escolha | Motivo curto |
|---|---|---|
| Shader language | HLSL → SPIR-V via DXC | DSHL própria é o maior peso morto do Dagor |
| Backend | Vulkan 1.3 só | Um bem feito bate sete medianos |
| Dynamic rendering | sim | Sem render pass / framebuffer objects |
| Descritores | bindless dia 1 | Retrofit custa meses |
| Alocação GPU | VMA | Não é diferenciação |
| Build | CMake | `jam` é parte do que torna o Dagor impenetrável |
| Threading | job system, **sem fibers** | Fibers custam Tracy, gdb e cada crash |
| Memória | arena + pools, **sem TLSF** | TLSF é alocador de console |
| Reflexão | macro + `TypeRegistry` | Codegen libclang traz um parser pra manter |

## Bindless

Um único `VkDescriptorSet` global para a engine inteira, com quatro arrays:

| Binding | Tipo | Capacidade alvo |
|---|---|---|
| 0 | `SAMPLED_IMAGE` | 16384 |
| 1 | `STORAGE_BUFFER` | 4096 |
| 2 | `SAMPLER` | 256 |
| 3 | `STORAGE_IMAGE` | 1024 |

Flags: `PARTIALLY_BOUND` (o array pode ter buracos) + `UPDATE_AFTER_BIND` (escrever enquanto
gravado) + `UPDATE_UNUSED_WHILE_PENDING`. Uma textura é um `uint32` que viaja em push constant
ou buffer — nunca um slot bindado.

Índices vêm de um freelist por array e são estáveis; liberar devolve o índice para reuso.

## Frames

Dois frames em voo. Por frame: command pool transiente, command buffer, semáforo
`imageAvailable`, fence. O semáforo `renderFinished` é **por imagem de swapchain**, não por
frame — esperar num semáforo cuja imagem ainda está sendo apresentada é erro de validação.

## Caminho offscreen

Não é fallback. É como golden image é produzida, e é o que permite o CI rodar testes de render
com lavapipe (Vulkan por software) numa máquina sem GPU. `VulkanRenderer::createOffscreen()`
+ `readback()` + `captureToPng()`.

## Terceiros

Herdamos a lista de compras do Dagor, sem herdar seu código:
VMA, stb, GLFW, doctest, Tracy. Planejados: Jolt (física), ACL (animação),
Recast/Detour (navmesh), miniaudio, ImGui + ImGuizmo + imgui-node-editor, meshoptimizer, DXC.
