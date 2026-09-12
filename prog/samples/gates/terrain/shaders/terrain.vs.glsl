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
#extension GL_EXT_nonuniform_qualifier : require
// `texelFetch` sobre um `texture2D` sem amostrador precisa desta.
#extension GL_EXT_samplerless_texture_functions : require

layout(set = 0, binding = 0, std140) uniform Terrain {
    layout(offset = 0)   mat4 view_proj;
    layout(offset = 64)  vec4 camera_pos;
    layout(offset = 80)  vec4 sun_dir;
    layout(offset = 96)  vec4 sun_color;
    layout(offset = 112) vec4 params;      // x=célula base, y=escala, z=N, w=lado do patch
    layout(offset = 128) vec4 sky_zenith;
    layout(offset = 144) vec4 sky_horizon;
    layout(offset = 160) vec2 inv_extent;
    // Índice bindless do campo de altura cozido. 0 é o dummy 1x1 e quer dizer
    // «avalia o FBM aqui», que é o caminho de controlo do A/B.
    layout(offset = 172) uint field;
} cb;

// A lista que o `terrain_cull.cs` escreveu. `gl_InstanceIndex` já não é o nível:
// é a posição na lista dos patches que sobreviveram.
layout(set = 3, binding = 0, std430) readonly buffer Vis { uint items[]; } visible[];

// O campo de altura cozido, R32F, uma amostra por célula do nível 0.
layout(set = 1, binding = 0) uniform texture2D heap[];

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

// A janela do campo, espelhada de `harpia_render::heightmap`. Se um lado mudar
// sem o outro, a geometria muda de sítio -- por isso os números estão aqui com o
// nome que têm lá.
const float FIELD_SPACING = 0.5;   // = CLIPMAP_CELL
const int   FIELD_SIDE = 4352;     // = FIELD_TILES (17) * TILE_N (256)
// Um múltiplo da largura, para o resto nunca ver um operando negativo: em GLSL o
// `%` com negativos é **indefinido**, e o terreno tem coordenadas dos dois lados.
const int   FIELD_WRAP_BIAS = FIELD_SIDE * 1024;

// Altura lida do campo cozido, com endereçamento **toroidal**.
//
// O slot de um tile na textura é `tile mod 17`, e um tile são 256 amostras,
// portanto o texel de uma amostra global é `g mod (17*256)`. Daí este shader não
// precisar de saber onde está a janela: quem tem de saber é a CPU, que decide o
// que está residente. A versão anterior fixava a janela na origem do mundo e
// limitava o índice com `clamp` -- e era isso que punha 902 pixels errados na
// linha do horizonte assim que a câmara andava (D61).
//
// `texelFetch` e não um amostrador: os vértices do clipmap caem **em cima** de
// texels (o snap de cada nível é múltiplo da célula base), portanto não há meio
// texel nem filtragem a inventar valores.
ivec2 field_texel(vec2 world_xz) {
    ivec2 g = ivec2(round(world_xz / FIELD_SPACING));
    return (g + ivec2(FIELD_WRAP_BIAS)) % ivec2(FIELD_SIDE);
}

float field_at(ivec2 texel) {
    return texelFetch(heap[nonuniformEXT(cb.field)], texel, 0).r;
}

// Envolver um índice que já está quase dentro custa uma comparação; o `%` custa
// uma divisão. Os vizinhos da normal estão a menos de uma largura do centro,
// portanto uma soma ou uma subtracção chega.
int wrap1(int v) {
    if (v < 0) {
        return v + FIELD_SIDE;
    }
    return v >= FIELD_SIDE ? v - FIELD_SIDE : v;
}

// Um só sítio a decidir de onde vem a altura. O ramo é uniforme no draw inteiro.
float height_at(vec2 p) {
    return cb.field != 0u ? field_at(field_texel(p)) : terrain_height(p.x, p.y);
}

void main() {
    float n     = cb.params.z;
    int   cells = int(n);
    int   patch_side = int(cb.params.w);
    int   patches    = cells / patch_side;   // patches por lado, num nível

    // O patch que este draw desenha sai da lista, não do índice da instância: a
    // GPU é que escolheu quais sobrevivem, e são só esses que chegam aqui.
    uint patch_id = visible[0].items[gl_InstanceIndex];
    int level  = int(patch_id) / (patches * patches);
    int p      = int(patch_id) - level * patches * patches;
    int px     = p % patches;
    int py     = p / patches;

    float cell = cb.params.x * float(1 << level);

    // Seis vértices por célula: dois triângulos, sem index buffer.
    int vid    = gl_VertexIndex;
    int cell_i = vid / 6;
    int corner = vid - cell_i * 6;
    // A célula é local ao patch; `cx`/`cy` voltam a ser coordenadas no nível, que
    // é o que todo o resto desta função espera.
    int cx     = px * patch_side + (cell_i % patch_side);
    int cy     = py * patch_side + (cell_i / patch_side);

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
    if (level > 0) {
        vec2 from_centre = abs(vec2(float(cx), float(cy)) - n * 0.5);
        if (max(from_centre.x, from_centre.y) < n * 0.25) {
            gl_Position = vec4(0.0, 0.0, 2.0, 1.0); // fora do clip, degenerado
            v_world = vec3(0.0);
            v_normal = vec3(0.0, 1.0, 0.0);
            v_view_dist = 0.0;
            return;
        }
    }

    float h = height_at(world_xz);

    // Saia na fronteira interior do anel.
    //
    // Dois níveis vizinhos não podem alinhar sempre: cada um faz snap à sua
    // própria grelha, portanto a fronteira pode ficar deslocada até uma célula
    // fina, e por aí vê-se o céu. Um clipmap "a sério" resolve isto com uma tira
    // de recorte em L de tamanho variável; uma saia faz o mesmo trabalho em duas
    // linhas, ao custo de uma dobra quase invisível a rasar o chão.
    if (level > 0) {
        vec2 vert_from_centre = abs(vec2(float(cx + o.x), float(cy + o.y)) - n * 0.5);
        if (max(vert_from_centre.x, vert_from_centre.y) <= n * 0.25 + 0.01) {
            h -= cell * 2.0;
        }
    }

    vec3 world = vec3(world_xz.x, h, world_xz.y);

    // Normal por diferenças centrais à escala da célula: mais fina e a normal
    // descreve detalhe que a malha não tem, o que dá luz a tremer nas bordas.
    float eps = cell;
    float dx, dz;
    if (cb.field != 0u) {
        // Um `%` por vértice, não cinco: o texel do centro envolve-se uma vez e
        // os quatro vizinhos saem dele com somas. A distância entre amostras
        // vizinhas é `cell` em mundo, que são `1 << level` texels.
        ivec2 b = field_texel(world_xz);
        int s = 1 << level;
        dx = field_at(ivec2(wrap1(b.x + s), b.y)) - field_at(ivec2(wrap1(b.x - s), b.y));
        dz = field_at(ivec2(b.x, wrap1(b.y + s))) - field_at(ivec2(b.x, wrap1(b.y - s)));
    } else {
        dx = terrain_height(world_xz.x + eps, world_xz.y)
           - terrain_height(world_xz.x - eps, world_xz.y);
        dz = terrain_height(world_xz.x, world_xz.y + eps)
           - terrain_height(world_xz.x, world_xz.y - eps);
    }

    v_world = world;
    v_normal = normalize(vec3(-dx, 2.0 * eps, -dz));
    v_view_dist = distance(world, cb.camera_pos.xyz);
    gl_Position = cb.view_proj * vec4(world, 1.0);
}
