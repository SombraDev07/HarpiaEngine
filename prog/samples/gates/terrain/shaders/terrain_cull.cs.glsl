// Culling de patches do terreno em compute (D48, ponto 7.3 da comparação).
//
// A Dagor faz isto em CPU: percorre os patches, testa-os, e monta lotes de draws
// instanciados com os parâmetros em constantes de VS. Aqui a CPU não percorre
// nada -- despacha, e o `drawIndirect` que se segue desenha o que este shader
// decidir.
//
// A caixa em Y vem do `terrain_bounds.cs`, que só a recalcula para os níveis que
// mudaram de sítio. Aqui não há ruído nenhum: uma leitura, o teste do anel, seis
// produtos escalares. É por isso que cabe numa thread por patch.

#version 450

// slot 0 = lista de visíveis (saída) · 1 = argumentos do draw · 2 = caixas
layout(set = 3, binding = 0, std430) writeonly buffer Out { uint items[]; } visible[];
layout(set = 3, binding = 0, std430) buffer Args { uint v[]; } args[];
layout(set = 3, binding = 0, std430) readonly buffer Bounds { vec2 items[]; } bounds[];

layout(set = 0, binding = 0, std140) uniform Cull {
    layout(offset = 0)   vec4 planes[6];    // a·x+b·y+c·z+d > 0 = dentro
    layout(offset = 96)  vec4 camera_pos;
    layout(offset = 112) vec4 params;       // x=célula base, y=N, z=lado do patch, w=nº de patches
    layout(offset = 128) vec4 counts;       // x=vértices por patch, y=patches por nível
} cb;

layout(local_size_x = 64, local_size_y = 1, local_size_z = 1) in;

void main() {
    uint id = gl_GlobalInvocationID.x;
    uint total = uint(cb.params.w);

    // Os campos fixos do comando. `instanceCount` fica a zero: é o atómico que o
    // enche, portanto tem de ser escrito antes de qualquer thread lhe tocar.
    if (id == 0u) {
        args[1].v[0] = uint(cb.counts.x);  // vertexCount
        args[1].v[2] = 0u;                 // firstVertex
        args[1].v[3] = 0u;                 // firstInstance
    }
    if (id >= total) return;

    uint per_level = uint(cb.counts.y);
    uint level = id / per_level;
    uint p = id - level * per_level;
    int patch_side = int(cb.params.z);
    uint grid = uint(cb.params.y) / uint(patch_side);

    float n = cb.params.y;
    float cell = cb.params.x * float(1u << level);
    // O mesmo snap do VS: ao **dobro** da célula, senão os níveis desalinham.
    float snap = cell * 2.0;
    vec2 centre = floor(cb.camera_pos.xz / snap) * snap;

    int c0x = int(p % grid) * patch_side;
    int c0y = int(p / grid) * patch_side;

    // O buraco do anel. Só se rejeita se o patch couber **todo** lá dentro --
    // rejeitar um que faça fronteira abriria um buraco no terreno.
    if (level > 0u) {
        float d0x = abs(float(c0x) - n * 0.5);
        float d1x = abs(float(c0x + patch_side - 1) - n * 0.5);
        float d0y = abs(float(c0y) - n * 0.5);
        float d1y = abs(float(c0y + patch_side - 1) - n * 0.5);
        if (max(max(d0x, d1x), max(d0y, d1y)) < n * 0.25) return;
    }

    vec2 y = bounds[2].items[id];
    vec2 w0 = centre + (vec2(float(c0x), float(c0y)) - n * 0.5) * cell;
    vec2 w1 = centre + (vec2(float(c0x + patch_side), float(c0y + patch_side)) - n * 0.5) * cell;
    vec3 c = vec3((w0.x + w1.x) * 0.5, (y.x + y.y) * 0.5, (w0.y + w1.y) * 0.5);
    vec3 e = vec3((w1.x - w0.x) * 0.5, (y.y - y.x) * 0.5, (w1.y - w0.y) * 0.5);

    // Conservador: fora só se estiver inteiramente fora de algum plano. Um falso
    // positivo custa 384 vértices; um falso negativo é um buraco no chão.
    for (int i = 0; i < 6; ++i) {
        vec3 nrm = cb.planes[i].xyz;
        float r = e.x * abs(nrm.x) + e.y * abs(nrm.y) + e.z * abs(nrm.z);
        if (dot(nrm, c) + cb.planes[i].w + r < 0.0) return;
    }

    uint slot = atomicAdd(args[1].v[1], 1u);
    visible[0].items[slot] = id;
}
