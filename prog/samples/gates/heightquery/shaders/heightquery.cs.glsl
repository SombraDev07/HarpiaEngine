// Avalia a altura do terreno numa grelha e escreve-a num R32F.
//
// A função é copiada de `harpia_render::terrain_height` linha a linha. O gate
// existe precisamente para medir se "copiada linha a linha" chega para as duas
// darem o mesmo número -- a CPU e a GPU não são obrigadas a concordar em `sin`,
// e este hash multiplica o resultado por 43758, o que amplifica qualquer
// diferença no último bit até ser visível.

#version 450

layout(set = 4, binding = 0, r32f) uniform writeonly image2D result;

layout(set = 0, binding = 0, std140) uniform Query {
    layout(offset = 0)  vec4 origin;   // xy = canto do mundo, z = passo, w = N
} cb;

layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;

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
    ivec2 id = ivec2(gl_GlobalInvocationID.xy);
    int n = int(cb.origin.w);
    if (id.x >= n || id.y >= n) return;
    float step = cb.origin.z;
    float x = cb.origin.x + float(id.x) * step;
    float z = cb.origin.y + float(id.y) * step;
    imageStore(result, id, vec4(terrain_height(x, z), 0.0, 0.0, 0.0));
}
