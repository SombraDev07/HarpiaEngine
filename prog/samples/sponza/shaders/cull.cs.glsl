// Culling das primitivas da Sponza, em compute.
//
// Escreve `instanceCount` — 1 ou 0 — em cada comando de um
// `vkCmdDrawIndexedIndirect` múltiplo. Um comando com zero instâncias não desenha
// nada e custa quase nada; assim a lista de comandos é fixa e só os contadores
// mudam, o que evita ter de compactar nada.
//
// `firstInstance` já traz o índice da primitiva, escrito uma vez no arranque: é
// por aí que o VS sabe qual é a sua matriz e o seu material, sem `gl_DrawID`.

#version 450

struct Prim {
    mat4 world;
    vec4 material;
    vec4 centre;
    vec4 extents;
};
// slot 0 = primitivas · 1 = comandos indirectos
layout(set = 3, binding = 0, std430) readonly buffer Prims { Prim items[]; } prim_buf[];
layout(set = 3, binding = 0, std430) buffer Args { uint v[]; } args[];

layout(set = 0, binding = 0, std140) uniform Cull {
    layout(offset = 0)  vec4 planes[6];
    // x = nº de primitivas
    layout(offset = 96) vec4 params;
} cb;

layout(local_size_x = 64, local_size_y = 1, local_size_z = 1) in;

void main() {
    uint id = gl_GlobalInvocationID.x;
    if (id >= uint(cb.params.x)) return;

    Prim p = prim_buf[0].items[id];
    vec3 c = p.centre.xyz;
    vec3 e = p.extents.xyz;

    // Conservador: fora só se estiver inteiramente fora de algum plano.
    bool visible = true;
    for (int i = 0; i < 6; ++i) {
        vec3 n = cb.planes[i].xyz;
        float r = e.x * abs(n.x) + e.y * abs(n.y) + e.z * abs(n.z);
        if (dot(n, c) + cb.planes[i].w + r < 0.0) { visible = false; break; }
    }
    // O comando tem cinco u32; o `instanceCount` é o segundo.
    args[1].v[id * 5u + 1u] = visible ? 1u : 0u;
}
