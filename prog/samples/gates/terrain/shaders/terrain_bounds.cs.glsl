// Caixas envolventes dos patches do terreno, só dos níveis que mudaram de sítio.
//
// Um patch só muda de região do mundo quando o **snap** do seu nível muda, e o
// snap de um nível é o dobro da sua célula: 1 unidade no nível 0, 64 no nível 6.
// A 40 u/s isso é uma vez por 1.5 frames no nível 0 e uma por 96 no nível 6 --
// 1.32 níveis de 7 por frame, 19% do trabalho.
//
// Isto existe porque o culling pago frame a frame não valia a pena: a caixa exacta
// em Y são 81 avaliações de FBM por patch, e medido dava 0.018 ms de dispatch
// contra 0.022 ms poupados na pass do terreno (D48). Separar o cálculo do teste é
// o que torna o teste barato.

#version 450

// slot 2 = caixas (lo, hi) por patch
layout(set = 3, binding = 0, std430) writeonly buffer Bounds { vec2 items[]; } bounds[];

layout(set = 0, binding = 0, std140) uniform Upd {
    layout(offset = 0)  vec4 camera_pos;
    layout(offset = 16) vec4 params;    // x=célula base, y=N, z=lado do patch, w=nº de patches
    layout(offset = 32) vec4 counts;    // x=patches por nível, y=níveis a refazer
    layout(offset = 48) vec4 levels[2]; // quais, um por componente
} cb;

// Um workgroup por patch: as (PATCH+1)² amostras repartem-se pelas 64 lanes. Com
// uma thread por patch corriam em série e o dispatch custava 2.8x mais.
layout(local_size_x = 64, local_size_y = 1, local_size_z = 1) in;

shared float s_lo[64];
shared float s_hi[64];

const int OCTAVES = 5;

// Hash inteiro em [0, 1) a partir de coordenadas de célula.
//
// NÃO usar `fract(sin(x) * 43758.5)`. É o hash mais copiado da internet e não é
// portável: `sin` de um argumento grande difere no último bit entre a libm da CPU
// e o hardware da GPU, e o factor 43758 amplifica isso até a parte fraccionária
// ser outra. Com esse hash o gate `heightquery` mediu 99.84% dos pontos fora da
// tolerância e 77 m de erro máximo num terreno de +-60 m.
//
// Aritmética inteira é exacta dos dois lados.
float hash2(float x, float y) {
    uint ix = uint(int(x));
    uint iy = uint(int(y));
    uint h = ix * 374761393u + iy * 668265263u;
    h = (h ^ (h >> 13)) * 1274126177u;
    h = h ^ (h >> 16);
    return float(h) * (1.0 / 4294967296.0);
}


float smooth_t(float t) { return t * t * (3.0 - 2.0 * t); }

float value_noise(float x, float y) {
    float ix = floor(x), iy = floor(y);
    float fx = x - ix,   fy = y - iy;
    float ux = smooth_t(fx), uy = smooth_t(fy);
    float a = hash2(ix,       iy);
    float b = hash2(ix + 1.0, iy);
    float c = hash2(ix,       iy + 1.0);
    float d = hash2(ix + 1.0, iy + 1.0);
    float top    = a + (b - a) * ux;
    float bottom = c + (d - c) * ux;
    return top + (bottom - top) * uy;
}

float terrain_height(float x, float z) {
    float amplitude = 1.0;
    float frequency = 0.008;
    float sum = 0.0;
    float norm = 0.0;
    for (int i = 0; i < OCTAVES; ++i) {
        sum  += value_noise(x * frequency, z * frequency) * amplitude;
        norm += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    return (sum / norm * 2.0 - 1.0) * 60.0;
}

void main() {
    uint job = gl_WorkGroupID.x;
    uint tid = gl_LocalInvocationID.x;
    uint per_level = uint(cb.counts.x);

    uint slot = job / per_level;
    if (slot >= uint(cb.counts.y)) return;
    uint level = uint(cb.levels[slot >> 2u][slot & 3u]);
    uint p = job - slot * per_level;

    int patch_side = int(cb.params.z);
    uint grid = uint(cb.params.y) / uint(patch_side);
    float n = cb.params.y;
    float cell = cb.params.x * float(1u << level);
    float snap = cell * 2.0;
    vec2 centre = floor(cb.camera_pos.xz / snap) * snap;

    int c0x = int(p % grid) * patch_side;
    int c0y = int(p / grid) * patch_side;

    int side = patch_side + 1;
    int samples = side * side;
    float lo = 1e30;
    float hi = -1e30;
    for (int k = int(tid); k < samples; k += 64) {
        vec2 g = vec2(float(c0x + k % side) - n * 0.5, float(c0y + k / side) - n * 0.5);
        vec2 w = centre + g * cell;
        float h = terrain_height(w.x, w.y);
        lo = min(lo, h);
        hi = max(hi, h);
    }
    s_lo[tid] = lo;
    s_hi[tid] = hi;
    barrier();
    for (uint stride = 32u; stride > 0u; stride >>= 1) {
        if (tid < stride) {
            s_lo[tid] = min(s_lo[tid], s_lo[tid + stride]);
            s_hi[tid] = max(s_hi[tid], s_hi[tid + stride]);
        }
        barrier();
    }
    if (tid != 0u) return;

    // A saia desce a altura na fronteira interior do anel; a caixa tem de a conter.
    bounds[2].items[level * per_level + p] = vec2(s_lo[0] - cell * 2.0, s_hi[0]);
}
