// Culling de patches do terreno em compute (D47, ponto 7.3 da comparação).
//
// A Dagor faz isto em CPU: percorre os patches, testa-os, e monta lotes de draws
// instanciados com os parâmetros em constantes de VS. Aqui a CPU não percorre
// nada -- despacha uma vez, e o `drawIndirect` que se segue desenha o que este
// shader decidir.
//
// Cada thread trata um patch: 64 por nível, 7 níveis, 448 ao todo.
//
// A caixa de cada patch é **exacta em Y**, não estimada: avalia-se a altura nos
// mesmos (PATCH+1)² vértices que o VS vai avaliar. Uma caixa folgada em Y (±60, a
// escala inteira do terreno) seria correcta e não cortaria nada quando a câmara
// olha para cima ou para baixo; uma estimada por amostragem esparsa cortaria
// geometria que se vê. Avaliar os 81 é o que dá as duas coisas.

#version 450

// slot 0 = lista de patches visíveis (saída) · 1 = argumentos do draw
layout(set = 3, binding = 0, std430) writeonly buffer Out { uint items[]; } visible[];
layout(set = 3, binding = 0, std430) buffer Args { uint v[]; } args[];

layout(set = 0, binding = 0, std140) uniform Cull {
    layout(offset = 0)   vec4 planes[6];    // a·x+b·y+c·z+d > 0 = dentro
    layout(offset = 96)  vec4 camera_pos;
    layout(offset = 112) vec4 params;       // x=célula base, y=N, z=lado do patch, w=nº de patches
    layout(offset = 128) vec4 counts;       // x=vértices por patch, y=patches por nível
} cb;

// **Um workgroup por patch**, não uma thread.
//
// Com uma thread por patch cada uma corria as (PATCH+1)² = 81 avaliações de FBM
// em série -- 405 amostras de ruído numa só lane, e 448 threads ao todo, que são
// 7 workgroups para uma GPU inteira. Medido assim, o dispatch custava 0.050 ms e
// a pass do terreno poupava 0.040: o culling pagava-se a si próprio e mais nada.
// Espalhadas por 64 lanes com uma redução em memória partilhada, as mesmas 81
// amostras exactas saem em ~2 por lane.
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
    uint id = gl_WorkGroupID.x;
    uint tid = gl_LocalInvocationID.x;
    uint total = uint(cb.params.w);

    // Os campos fixos do comando. `instanceCount` fica a zero: é o atómico que o
    // enche, portanto tem de ser escrito antes de qualquer thread lhe tocar.
    if (id == 0u && tid == 0u) {
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
    uint px = p % grid;
    uint py = p / grid;

    float n = cb.params.y;
    float cell = cb.params.x * float(1u << level);
    // O mesmo snap do VS: ao **dobro** da célula, senão os níveis desalinham.
    float snap = cell * 2.0;
    vec2 centre = floor(cb.camera_pos.xz / snap) * snap;

    int c0x = int(px) * patch_side;
    int c0y = int(py) * patch_side;

    // O buraco do anel. Só se rejeita se o patch couber **todo** lá dentro --
    // rejeitar um que faça fronteira abriria um buraco no terreno.
    if (level > 0u) {
        float d0x = abs(float(c0x) - n * 0.5);
        float d1x = abs(float(c0x + patch_side - 1) - n * 0.5);
        float d0y = abs(float(c0y) - n * 0.5);
        float d1y = abs(float(c0y + patch_side - 1) - n * 0.5);
        if (max(max(d0x, d1x), max(d0y, d1y)) < n * 0.25) return;
    }

    // Intervalo exacto em Y: os mesmos vértices que o VS vai avaliar, repartidos
    // pelas lanes do workgroup.
    int side = patch_side + 1;
    int samples = side * side;
    float lo = 1e30;
    float hi = -1e30;
    for (int k = int(tid); k < samples; k += 64) {
        int i = k % side;
        int j = k / side;
        vec2 g = vec2(float(c0x + i) - n * 0.5, float(c0y + j) - n * 0.5);
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

    // A saia desce a altura na fronteira interior; a caixa tem de a conter.
    lo = s_lo[0] - cell * 2.0;
    hi = s_hi[0];

    vec2 w0 = centre + (vec2(float(c0x), float(c0y)) - n * 0.5) * cell;
    vec2 w1 = centre + (vec2(float(c0x + patch_side), float(c0y + patch_side)) - n * 0.5) * cell;
    vec3 bmin = vec3(w0.x, lo, w0.y);
    vec3 bmax = vec3(w1.x, hi, w1.y);
    vec3 c = (bmin + bmax) * 0.5;
    vec3 e = (bmax - bmin) * 0.5;

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
