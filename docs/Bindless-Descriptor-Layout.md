# Bindless descriptor layout

**Escrito antes do primeiro shader.** Hello-triangle não usa este layout (zero descriptors). O contrato existe para a fase 2 não inventar um mapping à DXC.

Barra: heap 8192, **slot 0 = null para sempre**. Índice inteiro no constant buffer, não handles opacos no shader.

## Princípio

O pipeline layout é o contrato. Shaders **não** adivinham collisions de `register(t0, space1)` vs `u0`. Em HLSL usamos `[[vk::binding(n, set)]]` explícito. DXC é o cook (`-spirv -fspv-target-env=vulkan1.3`); naga/WGSL estão fora.

## Push constants

| Bytes | Conteúdo (fase 3+) |
|---|---|
| 0–127 | `viewProj` (64) + `world` (64). 128 B no total. |

Estágios: vertex + fragment (+ compute quando o pass for compute).

Push constants **não** são um descriptor set. Ficam no `PipelineLayout` (`VkPushConstantRange`).

## Descriptor sets

| Set | Binding | Tipo Vulkan | Count | HLSL | Notas |
|---|---|---|---|---|---|
| 0 | 0 | `UNIFORM_BUFFER` | 1 | `b1` / `[[vk::binding(0, 0)]]` | CBV1 (`LightingCB` / frame grande) |
| 0 | 1 | `UNIFORM_BUFFER` | 1 | `b2` / `[[vk::binding(1, 0)]]` | CBV2 (`LightCB`) |
| 1 | 0 | `SAMPLED_IMAGE` | **8192**, variable, partially bound, update-after-bind | `Texture2D bindlessHeap[]` | **Índice 0 = null** (dummy 1×1) |
| 2 | 0 | `SAMPLER` | pequeno (linear / point / clamp / wrap) | `s0…` | Pool **separado** do heap 8192 |
| 3 | 1… | `STORAGE_BUFFER` | N buffers **soltos** | `StructuredBuffer` / `RWStructuredBuffer` | **Não** começar em `u0` se o set também tiver `t0` |
| 4 | 0… | `STORAGE_IMAGE` | UAV 2D/3D | `RWTexture2D` / `RWTexture3D` | 3D UAV em `GENERAL` |
| 5 | 0 | `SAMPLED_IMAGE` (3D) | heap 3D ou bindings nomeados | `Texture3D` | Clouds / fog. **Não** misturar com o heap 2D do set 1 |

Set 1 flags (binding 0):

- `DESCRIPTOR_BINDING_UPDATE_AFTER_BIND`
- `DESCRIPTOR_BINDING_PARTIALLY_BOUND`
- `DESCRIPTOR_BINDING_VARIABLE_DESCRIPTOR_COUNT`
- `SHADER_SAMPLED_IMAGE_ARRAY_NON_UNIFORM_INDEXING` no shader

Pool / layout do set 1: `UPDATE_AFTER_BIND`. `descriptorCount = 8192`.

## Slot 0 = null

A textura no índice 0 é um dummy **1×1** `R8G8B8A8_UNORM` (magenta ou preto), criada no init do device.

Antes de qualquer draw/dispatch que possa amostrar o heap:

1. Upload do pixel.
2. Barreira para `SHADER_READ` com destinos **VERTEX | FRAGMENT | COMPUTE**.

Shaders podem indexar `heap[0]` sem GPUVM. Nunca reutilizar o slot 0 para uma textura real.

## Indexação no shader

```hlsl
[[vk::binding(0, 1)]] Texture2D bindlessHeap[] : register(t0, space1);

float4 sampleBindless(uint idx, float2 uv, SamplerState s)
{
    return bindlessHeap[NonUniformResourceIndex(idx)].Sample(s, uv);
}
```

O CPU escreve `uint texId` no constant buffer. `texId == 0` é válido (null).

## Minas de mapping (C++ → não repetir)

1. No mesmo set, `t0 space1` e `u0 space1` colidem no **binding 0**. UAVs de buffer começam em **`u1`**.
2. `RWStructuredBuffer buf[3]` no HLSL = **um** descriptor no SPIR-V. Usar `Visible0`, `Visible1`, `Visible2` (bindings separados).
3. Não destruir o `VkPipelineLayout` / `RootSignature` depois de criar o PSO. O PSO guarda o handle.
4. ImGui / editor: descriptor pool **à parte**. Não `vkFreeDescriptorSets` de um set ainda in-flight (adiar um ciclo FIF).
5. Viewport do editor: amostrar `present_format()`, não `RGBA8` hardcoded.

## O que a fase 1 **não** faz

- Hello-triangle **não** usa o heap (pipeline layout vazio).
- O shader do triângulo não declara descriptor nenhum.

Fase 2: heap + dummy + `bindless_index()` + upload por mip + compute UAV. Este documento não muda sem entrada em `memory/DECISIONS.md`.
