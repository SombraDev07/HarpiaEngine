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
#extension GL_EXT_nonuniform_qualifier : require

struct Prim {
    mat4 world;
    vec4 material;
    vec4 centre;
    vec4 extents;
};
// slot 0 = primitivas · 1 = comandos indirectos
layout(set = 3, binding = 0, std430) readonly buffer Prims { Prim items[]; } prim_buf[];
layout(set = 3, binding = 0, std430) buffer Args { uint v[]; } args[];
// Quem se via no frame anterior. Persiste entre frames: é o que permite a
// primeira fase desenhar alguma coisa antes de haver pirâmide nenhuma.
layout(set = 3, binding = 0, std430) buffer Seen { uint v[]; } seen[];

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(set = 0, binding = 0, std140) uniform Cull {
    layout(offset = 0)  vec4 planes[6];
    // x = nº de primitivas, y = fase (1 ou 2), z = slot dos comandos, w = slot do `seen`
    layout(offset = 96)  vec4 params;
    layout(offset = 112) mat4 view_proj;
    // x = índice bindless da pirâmide, y = largura, z = altura, w = nº de mips
    layout(offset = 176) vec4 hiz;
} cb;

// Está a caixa toda atrás do que já foi desenhado?
//
// O mesmo teste do `gate-veg` (D55): projecta-se a caixa, escolhe-se o nível da
// pirâmide cujo texel cobre o rectângulo, e compara-se o ponto mais próximo com o
// oclusor mais distante.
bool occluded(vec3 lo, vec3 hi) {
    vec2 mn = vec2(1e30);
    vec2 mx = vec2(-1e30);
    float nearest = 1e30;
    for (int i = 0; i < 8; ++i) {
        vec3 c = vec3(
            (i & 1) == 0 ? lo.x : hi.x,
            (i & 2) == 0 ? lo.y : hi.y,
            (i & 4) == 0 ? lo.z : hi.z);
        vec4 clip = cb.view_proj * vec4(c, 1.0);
        if (clip.w <= 1e-4) return false;
        vec3 ndc = clip.xyz / clip.w;
        mn = min(mn, ndc.xy);
        mx = max(mx, ndc.xy);
        nearest = min(nearest, ndc.z);
    }
    if (nearest <= 0.0) return false;

    vec2 uv0 = (mn * 0.5 + 0.5) * cb.hiz.yz;
    vec2 uv1 = (mx * 0.5 + 0.5) * cb.hiz.yz;
    vec2 size = max(uv1 - uv0, vec2(1.0));
    float level = clamp(ceil(log2(max(size.x, size.y) * 0.5)), 0.0, cb.hiz.w - 1.0);
    int mip = int(level);
    float scale = exp2(-level);
    ivec2 dim = max(ivec2(cb.hiz.yz * scale), ivec2(1));
    ivec2 lo_t = clamp(ivec2(floor(uv0 * scale)), ivec2(0), dim - 1);
    ivec2 hi_t = clamp(ivec2(floor(uv1 * scale)), ivec2(0), dim - 1);

    float far_depth = 0.0;
    for (int y = lo_t.y; y <= hi_t.y; ++y) {
        for (int x = lo_t.x; x <= hi_t.x; ++x) {
            far_depth = max(far_depth,
                texelFetch(sampler2D(heap[uint(cb.hiz.x)], samp_clamp), ivec2(x, y), mip).r);
        }
    }
    return nearest > far_depth;
}

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
    uint a = uint(cb.params.z);
    uint sv = uint(cb.params.w);
    uint phase = uint(cb.params.y);

    if (phase == 1u) {
        // Fase 1: desenha-se o que se via no frame anterior. É isso que enche o
        // depth buffer de que a pirâmide vai sair -- sem esta fase não haveria
        // oclusores nenhuns no primeiro teste.
        args[a].v[id * 5u + 1u] = (visible && seen[sv].v[id] != 0u) ? 1u : 0u;
        return;
    }

    // Fase 2: o teste de oclusão a sério, contra a pirâmide da fase 1. Desenha-se
    // quem passa e **ainda não foi desenhado**, para nada aparecer duas vezes.
    bool pass2 = visible && !occluded(c - e, c + e);
    bool drawn = seen[sv].v[id] != 0u && visible;
    args[a].v[id * 5u + 1u] = (pass2 && !drawn) ? 1u : 0u;
    // E o que se viu agora é o que a fase 1 do frame seguinte vai desenhar.
    seen[sv].v[id] = pass2 ? 1u : 0u;
}
