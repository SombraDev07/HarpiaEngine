// Culling de vegetação: frustum, e depois oclusão contra a pirâmide Hi-Z.
//
// ## Porque não é como a Dagor faz
//
// A erva deles é cortada por **feedback do pixel shader**: desenha-se tudo, o PS
// marca num bitvector qual a instância que sobreviveu ao teste de profundidade, e
// um compute compacta esse bitvector numa lista densa para a passagem seguinte.
// Funciona, e o PS deles tem três caminhos de código com intrínsecas de wave só
// para reduzir o tráfego de atómicos — e mesmo assim o shader de compactação está
// desligado fora do DX12 de desktop (`if (!hardware.dx12 || hardware.xbox ||
// hardware.scarlett) dont_render;`).
//
// Aqui o teste é feito **antes** de rasterizar: projecta-se a caixa da instância,
// lê-se o nível da pirâmide cujo texel cobre o rectângulo, e compara-se. Um
// atómico por instância que sobrevive, em vez de um por fragmento; nenhuma
// intrínseca de wave, portanto nenhum caminho desligado por plataforma. E a
// pirâmide é do **mesmo frame** -- construída do prepass dos oclusores -- por isso
// não há o frame de atraso que um Hi-Z do frame anterior traz.

#version 450
#extension GL_EXT_nonuniform_qualifier : require

struct Plant {
    vec4 pos_scale;   // xyz base, w escala
    vec4 color;
};
// slot 0 = plantas · 1 = visíveis (saída) · 2 = argumentos do draw
layout(set = 3, binding = 0, std430) readonly buffer In { Plant items[]; } src[];
layout(set = 3, binding = 0, std430) writeonly buffer Out { uint items[]; } visible[];
layout(set = 3, binding = 0, std430) buffer Args { uint v[]; } args[];

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(set = 0, binding = 0, std140) uniform Cull {
    layout(offset = 0)   vec4 planes[6];
    layout(offset = 96)  mat4 view_proj;
    // x = nº de plantas, y = vértices por planta, z = 1 se a oclusão está ligada
    layout(offset = 160) vec4 params;
    // x = índice bindless da pirâmide, y = largura, z = altura, w = nº de mips
    layout(offset = 176) vec4 hiz;
    // x = slot da lista de visíveis, y = slot dos argumentos.
    //
    // Vêm do CB porque o gate corre o culling **duas vezes** por frame, com e sem
    // oclusão, para poder comparar as duas imagens. Os índices são uniformes, por
    // isso não é preciso `nonuniformEXT`.
    layout(offset = 192) vec4 slots;
} cb;

layout(local_size_x = 64, local_size_y = 1, local_size_z = 1) in;

// A planta ocupa uma caixa: base no chão, altura e largura pela escala.
void plant_bounds(Plant p, out vec3 lo, out vec3 hi) {
    float s = p.pos_scale.w;
    lo = p.pos_scale.xyz + vec3(-s * 0.5, 0.0, -s * 0.5);
    hi = p.pos_scale.xyz + vec3(s * 0.5, s * 2.0, s * 0.5);
}

// Está a caixa totalmente atrás do que já foi desenhado?
//
// Conservador nos dois sentidos: se qualquer canto ficar atrás da câmara
// desiste-se (a projecção deixa de fazer sentido), e escolhe-se o nível da
// pirâmide cujo texel **cobre** o rectângulo inteiro, para o máximo que se lê ser
// o máximo de tudo o que a caixa tapa.
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
        if (clip.w <= 1e-4) return false;     // atravessa o plano da câmara
        vec3 ndc = clip.xyz / clip.w;
        mn = min(mn, ndc.xy);
        mx = max(mx, ndc.xy);
        nearest = min(nearest, ndc.z);
    }
    if (nearest <= 0.0) return false;

    // NDC -> texels do nível 0.
    vec2 uv0 = (mn * 0.5 + 0.5) * cb.hiz.yz;
    vec2 uv1 = (mx * 0.5 + 0.5) * cb.hiz.yz;
    vec2 size = max(uv1 - uv0, vec2(1.0));
    // O nível onde o rectângulo cabe em 2x2 texels.
    float level = ceil(log2(max(size.x, size.y) * 0.5));
    level = clamp(level, 0.0, cb.hiz.w - 1.0);
    int mip = int(level);
    float scale = exp2(-level);
    ivec2 lo_t = ivec2(floor(uv0 * scale));
    ivec2 hi_t = ivec2(floor(uv1 * scale));
    ivec2 dim = max(ivec2(cb.hiz.yz * scale), ivec2(1));
    lo_t = clamp(lo_t, ivec2(0), dim - 1);
    hi_t = clamp(hi_t, ivec2(0), dim - 1);

    float far_depth = 0.0;
    for (int y = lo_t.y; y <= hi_t.y; ++y) {
        for (int x = lo_t.x; x <= hi_t.x; ++x) {
            far_depth = max(far_depth,
                texelFetch(sampler2D(heap[nonuniformEXT(uint(cb.hiz.x))], samp_clamp),
                           ivec2(x, y), mip).r);
        }
    }
    // Oclusa se mesmo o ponto mais próximo está atrás do oclusor mais distante.
    return nearest > far_depth;
}

void main() {
    uint id = gl_GlobalInvocationID.x;
    uint total = uint(cb.params.x);

    if (id == 0u) {
        args[uint(cb.slots.y)].v[0] = uint(cb.params.y);  // vertexCount
        args[uint(cb.slots.y)].v[2] = 0u;                 // firstVertex
        args[uint(cb.slots.y)].v[3] = 0u;                 // firstInstance
    }
    if (id >= total) return;

    Plant p = src[0].items[id];
    vec3 lo, hi;
    plant_bounds(p, lo, hi);
    vec3 c = (lo + hi) * 0.5;
    vec3 e = (hi - lo) * 0.5;

    for (int i = 0; i < 6; ++i) {
        vec3 n = cb.planes[i].xyz;
        float r = e.x * abs(n.x) + e.y * abs(n.y) + e.z * abs(n.z);
        if (dot(n, c) + cb.planes[i].w + r < 0.0) return;
    }
    if (cb.params.z > 0.5 && occluded(lo, hi)) return;

    uint slot = atomicAdd(args[uint(cb.slots.y)].v[1], 1u);
    visible[uint(cb.slots.x)].items[slot] = id;
}
