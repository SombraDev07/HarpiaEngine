// Repõe o comando indirecto **na GPU**, dentro do command buffer.
//
// Antes isto era um `write_storage_buffer` no início do frame: um memcpy para um
// buffer mapeado, que não espera por nada, com **dois frames em voo**. A
// reposição da CPU corria contra o `cull` do frame anterior, que ainda
// incrementava o mesmo contador, e contra o draw indirecto, que ainda o lia. O
// sintoma era intermitente e tinha cara de bug de lógica: `visiveis_gpu=130`
// contra `visiveis_cpu=65`, o dobro exacto -- um frame de incrementos por cima de
// outro (D62). Aparecia em ~1 de 12 corridas, nos dois caminhos.
//
// Aqui a ordem é por construção: esta pass escreve, a barreira que o grafo deriva
// separa, e só depois o `cull` incrementa. Um workgroup de uma invocação, porque
// dentro de um dispatch não há ordem entre workgroups -- zerar na mesma passagem
// que incrementa seria trocar uma corrida por outra.

#version 450

// slot 1 = o `VkDrawIndirectCommand` que o terreno consome
layout(set = 3, binding = 0, std430) writeonly buffer Args { uint v[]; } args[];

layout(set = 0, binding = 0, std140) uniform Cull {
    // O mesmo bloco que o `terrain_cull.cs` lê.
    // x = vértices por patch, y = patches por nível, z = instâncias iniciais
    // (0 quando o culling as conta, todas no controlo `-- --no-cull`).
    layout(offset = 128) vec4 counts;
} cb;

layout(local_size_x = 1, local_size_y = 1, local_size_z = 1) in;

const uint SLOT_ARGS = 1u;

void main() {
    args[SLOT_ARGS].v[0] = uint(cb.counts.x); // vertexCount
    args[SLOT_ARGS].v[1] = uint(cb.counts.z); // instanceCount
    args[SLOT_ARGS].v[2] = 0u;                // firstVertex
    args[SLOT_ARGS].v[3] = 0u;                // firstInstance
}
