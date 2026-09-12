// Sobe para a textura do campo os tiles que entraram na janela.
//
// O campo segue a câmara com endereçamento **toroidal**: o slot de um tile é
// `tile mod 17`, portanto quando a câmara avança um tile o que entra ocupa
// exactamente o lugar do que saiu, e sobe-se uma tira de 17 em vez da janela de
// 289. É o mesmo truque de um atlas toroidal de sombras.
//
// Porquê um compute e não um upload de sub-região: o RHI sobe **mips inteiros** e
// publica a textura no heap quando o último sobe (`finalize_sampled`) -- um
// upload por tile chamaria isso 17 vezes e publicaria à primeira, e uma textura
// já publicada está em `SHADER_READ_ONLY` sem caminho de volta. Escrever por
// storage image é o padrão que esta árvore já usa na pirâmide Hi-Z, fica em
// GENERAL (mina 4) e as barreiras saem do render graph em vez de raciocínio caso
// a caso.

#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Um tile por camada do dispatch: 16x16 grupos de 16x16 threads = 256x256.
layout(local_size_x = 16, local_size_y = 16, local_size_z = 1) in;

// slot 3 = as amostras em staging, slot 4 = para onde vai cada tile
layout(set = 3, binding = 0, std430) readonly buffer Staged { float v[]; } staged[];
layout(set = 3, binding = 0, std430) readonly buffer Slots { uvec4 v[]; } slots[];

// O binding 0 do set 4 é um array de slots, não um descritor só.
layout(set = 4, binding = 0, r32f) uniform writeonly image2D dst[];

layout(set = 0, binding = 0, std140) uniform Upload {
    // x = slot do UAV de destino, y = tiles neste dispatch, z = lado do tile
    layout(offset = 0) vec4 params;
} cb;

const uint SLOT_STAGED = 3u;
const uint SLOT_SLOTS = 4u;

void main() {
    uint tile = gl_WorkGroupID.z;
    if (tile >= uint(cb.params.y)) {
        return;
    }
    int n = int(cb.params.z);
    ivec2 local = ivec2(gl_GlobalInvocationID.xy);
    if (local.x >= n || local.y >= n) {
        return;
    }

    // xy = o slot, em tiles, dentro da textura.
    uvec4 slot = slots[SLOT_SLOTS].v[tile];
    uint at = tile * uint(n * n) + uint(local.y * n + local.x);
    float h = staged[SLOT_STAGED].v[at];

    // **Sem `nonuniformEXT`**, e não por economia: o slot vem do constant buffer,
    // portanto é uniforme em todo o dispatch, e o qualificador afirmaria o
    // contrário. Afirmá-lo declara a capability `StorageImageArrayNonUniformIndexing`,
    // que este device não tem ligada (liga a das *sampled*, que é outra feature) --
    // e o pipeline é recusado com `VUID-VkShaderModuleCreateInfo-pCode-08740`.
    // O `hiz.cs` faz o mesmo há muito: só o heap amostrado leva `nonuniformEXT`.
    imageStore(
        dst[uint(cb.params.x)],
        ivec2(slot.xy) * n + local,
        vec4(h, 0.0, 0.0, 0.0)
    );
}
