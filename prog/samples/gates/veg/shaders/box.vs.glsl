// Caixas instanciadas: o chão e os oclusores. Sem vertex buffer.
//
// A permutação dos eixos é **cíclica** — `q.y` para o eixo seguinte, `q.x` para o
// de trás. Escrita à mão para o eixo Z como `(q.x, q.y, sign)` invertia o winding
// e o back-face culling comia duas das seis faces, sem erro de validation nenhum
// (LANDMINES). Aqui está a versão certa.

#version 450
#extension GL_EXT_nonuniform_qualifier : require

struct Box {
    vec4 centre_pad;
    vec4 half_extent;   // xyz meia-extensão
    vec4 color;
};
// Slot **3**. O índice do array é o slot, e ler `box_buf[0]` lia o buffer das
// plantas como se fossem caixas: o chão aparecia deformado e os muros não
// apareciam de todo, sem erro nenhum.
layout(set = 3, binding = 0, std430) readonly buffer Boxes { Box items[]; } box_buf[];

layout(set = 0, binding = 0, std140) uniform Cb {
    layout(offset = 0)   mat4 view_proj;
    layout(offset = 64)  vec4 camera_pos;
    layout(offset = 80)  vec4 sun_dir;
    layout(offset = 96)  vec4 params;    // x = nº de caixas, y = vértices por caixa
    layout(offset = 112) vec4 hiz;       // x = índice bindless, y = largura, z = altura, w = mips
} cb;

layout(location = 0) out vec3 v_normal;
layout(location = 1) out vec3 v_color;

void main() {
    int vid = gl_VertexIndex;
    int face = vid / 6;
    int corner = vid - face * 6;
    int axis = face / 2;
    float sign = (face % 2 == 0) ? 1.0 : -1.0;

    const vec2 QUAD[6] = vec2[6](
        vec2(0, 0), vec2(0, 1), vec2(1, 0),
        vec2(1, 0), vec2(0, 1), vec2(1, 1)
    );
    vec2 q = QUAD[corner] * 2.0 - 1.0;
    q.x *= sign;

    vec3 local;
    vec3 n = vec3(0.0);
    if (axis == 0)      { local = vec3(sign, q.y, q.x); n.x = sign; }
    else if (axis == 1) { local = vec3(q.x, sign, q.y); n.y = sign; }
    else                { local = vec3(q.y, q.x, sign); n.z = sign; }

    Box b = box_buf[3].items[gl_InstanceIndex];
    vec3 world = b.centre_pad.xyz + local * b.half_extent.xyz;

    v_normal = n;
    v_color = b.color.rgb;
    gl_Position = cb.view_proj * vec4(world, 1.0);
}
