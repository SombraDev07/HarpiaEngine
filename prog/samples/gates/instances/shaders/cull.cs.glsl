// Culling em compute: decide o que se vê e escreve os argumentos do draw.
//
// A CPU não fica a saber quantas instâncias sobreviveram, e não precisa. Escreve
// as caixas uma vez, submete um `drawIndirect`, e a GPU preenche o
// `instanceCount` e a lista de visíveis.
//
// É aqui que passamos à frente da Dagor no terreno: eles fazem o culling de
// patches em CPU e depois montam lotes de draws instanciados com parâmetros em
// constantes de VS (`heightmapRenderer.cpp`). Isto não tem trabalho por
// instância do lado da CPU.

#version 450

struct Instance {
    vec4 pos_scale;     // xyz centro, w escala
    vec4 color;
};

// slot 0 = instâncias (entrada) · 1 = visíveis (saída) · 2 = argumentos do draw
layout(set = 3, binding = 0, std430) readonly buffer In { Instance items[]; } src[];
layout(set = 3, binding = 0, std430) writeonly buffer Out { uint items[]; } visible[];
layout(set = 3, binding = 0, std430) buffer Args { uint v[]; } args[];

layout(set = 0, binding = 0, std140) uniform Cull {
    layout(offset = 0)   vec4 planes[6];   // frustum, a·x+b·y+c·z+d > 0 = dentro
    layout(offset = 96)  vec4 params;      // x = nº de instâncias, y = vértices por instância
} cb;

layout(local_size_x = 64, local_size_y = 1, local_size_z = 1) in;

void main() {
    uint id = gl_GlobalInvocationID.x;
    uint count = uint(cb.params.x);

    // A invocação 0 põe os campos fixos do comando. `instanceCount` fica a zero
    // e é o atómico que o enche -- por isso tem de ser escrito **antes** de
    // qualquer thread o incrementar, e é o barrier do dispatch anterior que o
    // garante entre frames.
    if (id == 0u) {
        args[2].v[0] = uint(cb.params.y);  // vertexCount
        args[2].v[2] = 0u;                 // firstVertex
        args[2].v[3] = 0u;                 // firstInstance
    }

    if (id >= count) return;

    Instance inst = src[0].items[id];
    vec3 c = inst.pos_scale.xyz;
    // Esfera envolvente do cubo: o raio é a meia-diagonal, não a meia-aresta.
    // Usar a aresta cortaria cubos rodados a rasar o plano.
    float r = inst.pos_scale.w * 1.7320508;

    for (int i = 0; i < 6; ++i) {
        if (dot(cb.planes[i].xyz, c) + cb.planes[i].w + r < 0.0) return;
    }

    uint slot = atomicAdd(args[2].v[1], 1u);   // instanceCount
    visible[1].items[slot] = id;
}
