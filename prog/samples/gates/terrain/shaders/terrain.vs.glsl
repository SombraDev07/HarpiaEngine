// Clipmap a partir de SV_VertexID: sem vertex buffer, sem index buffer.
//
// A malha de um clipmap é sempre a mesma grelha; só muda de escala e de sítio.
// Lê-la de memória seria pagar banda para reler o que estas três instruções
// calculam a partir do índice do vértice.
//
// Uma instância por nível. O nível 0 é uma grelha cheia à volta da câmara; os de
// fora são anéis, porque o centro já está coberto pelo nível de dentro com o
// dobro da resolução. As células do centro de um anel colapsam num ponto, o que
// as torna triângulos degenerados que o rasterizador deita fora de graça.
//
// A função de altura é a mesma que `harpia_render::terrain_height`, linha a
// linha. Se divergirem, a geometria e a colisão passam a discordar -- que é o
// bug mais desagradável que um terreno pode ter, porque parece um bug de física.

#version 450

layout(set = 0, binding = 0, std140) uniform Terrain {
    layout(offset = 0)   mat4 view_proj;
    layout(offset = 64)  vec4 camera_pos;
    layout(offset = 80)  vec4 sun_dir;
    layout(offset = 96)  vec4 sun_color;
    layout(offset = 112) vec4 params;      // x=célula base, y=escala, z=N, w=níveis
    layout(offset = 128) vec4 sky_zenith;
    layout(offset = 144) vec4 sky_horizon;
    layout(offset = 160) vec2 inv_extent;
} cb;

layout(location = 0) out vec3 v_world;
layout(location = 1) out vec3 v_normal;
layout(location = 2) out float v_view_dist;

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
    float n    = cb.params.z;
    int   cells = int(n);
    float cell = cb.params.x * float(1 << gl_InstanceIndex);

    // Seis vértices por célula: dois triângulos, sem index buffer.
    int vid    = gl_VertexIndex;
    int cell_i = vid / 6;
    int corner = vid - cell_i * 6;
    int cx     = cell_i % cells;
    int cy     = cell_i / cells;

    // CCW visto de cima (+Y). A ordem óbvia -- (0,0)(1,0)(0,1) -- dá o contrário:
    // o produto vectorial de +X com +Z aponta para **baixo**, portanto o
    // `cull_back` comia o terreno todo e ficava céu. É a mesma ordem que o
    // `SphereMesh::grid_xz` usa, pela mesma razão.
    const ivec2 OFFSETS[6] = ivec2[6](
        ivec2(0, 0), ivec2(0, 1), ivec2(1, 0),
        ivec2(1, 0), ivec2(0, 1), ivec2(1, 1)
    );
    ivec2 o = OFFSETS[corner];
    vec2 grid = vec2(float(cx + o.x), float(cy + o.y)) - n * 0.5;

    // Snap ao **dobro** da célula deste nível, não à célula.
    //
    // Ao tamanho da própria célula o terreno deixa de nadar debaixo da câmara,
    // mas cada nível alinha à sua própria grelha e a fronteira entre um nível e o
    // de dentro fica desalinhada até meia célula -- o que abre fendas por onde se
    // vê o céu. Ao dobro, a grelha de um nível alinha com a do nível de fora, que
    // é exactamente onde os dois se encontram.
    float snap = cell * 2.0;
    vec2 centre = floor(cb.camera_pos.xz / snap) * snap;
    vec2 world_xz = centre + grid * cell;

    // Anel: o quarto central de um nível > 0 já está coberto pelo nível de dentro.
    // Colapsa-se num ponto em vez de se desenhar por cima -- z-fighting entre dois
    // níveis com resoluções diferentes é visível e feio.
    if (gl_InstanceIndex > 0) {
        vec2 from_centre = abs(vec2(float(cx), float(cy)) - n * 0.5);
        if (max(from_centre.x, from_centre.y) < n * 0.25) {
            gl_Position = vec4(0.0, 0.0, 2.0, 1.0); // fora do clip, degenerado
            v_world = vec3(0.0);
            v_normal = vec3(0.0, 1.0, 0.0);
            v_view_dist = 0.0;
            return;
        }
    }

    float h = terrain_height(world_xz.x, world_xz.y);

    // Saia na fronteira interior do anel.
    //
    // Dois níveis vizinhos não podem alinhar sempre: cada um faz snap à sua
    // própria grelha, portanto a fronteira pode ficar deslocada até uma célula
    // fina, e por aí vê-se o céu. Um clipmap "a sério" resolve isto com uma tira
    // de recorte em L de tamanho variável; uma saia faz o mesmo trabalho em duas
    // linhas, ao custo de uma dobra quase invisível a rasar o chão.
    if (gl_InstanceIndex > 0) {
        vec2 vert_from_centre = abs(vec2(float(cx + o.x), float(cy + o.y)) - n * 0.5);
        if (max(vert_from_centre.x, vert_from_centre.y) <= n * 0.25 + 0.01) {
            h -= cell * 2.0;
        }
    }

    vec3 world = vec3(world_xz.x, h, world_xz.y);

    // Normal por diferenças centrais à escala da célula: mais fina e a normal
    // descreve detalhe que a malha não tem, o que dá luz a tremer nas bordas.
    float eps = cell;
    float dx = terrain_height(world_xz.x + eps, world_xz.y)
             - terrain_height(world_xz.x - eps, world_xz.y);
    float dz = terrain_height(world_xz.x, world_xz.y + eps)
             - terrain_height(world_xz.x, world_xz.y - eps);

    v_world = world;
    v_normal = normalize(vec3(-dx, 2.0 * eps, -dz));
    v_view_dist = distance(world, cb.camera_pos.xyz);
    gl_Position = cb.view_proj * vec4(world, 1.0);
}
