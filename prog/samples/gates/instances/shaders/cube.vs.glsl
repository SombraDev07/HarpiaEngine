// Cubos por SV_VertexID e pela lista de visíveis que o compute escreveu.
//
// Sem vertex buffer e sem index buffer: 36 vértices por cubo saem do índice, e
// `gl_InstanceIndex` indexa a lista de sobreviventes, não o array original. É
// isso que torna um `drawIndexedIndirect` suficiente.

#version 450

struct Instance {
    vec4 pos_scale;
    vec4 color;
};

layout(set = 3, binding = 0, std430) readonly buffer In { Instance items[]; } src[];
layout(set = 3, binding = 0, std430) readonly buffer Vis { uint items[]; } visible[];

layout(set = 0, binding = 0, std140) uniform Draw {
    layout(offset = 0)   mat4 view_proj;
    layout(offset = 64)  vec4 sun_dir;
} cb;

layout(location = 0) out vec3 v_normal;
layout(location = 1) out vec3 v_color;

// Seis faces × dois triângulos. As faces são geradas a partir do eixo e do sinal,
// o que evita uma tabela de 36 posições.
void main() {
    int vid = gl_VertexIndex;
    int face = vid / 6;
    int corner = vid - face * 6;
    int axis = face / 2;             // 0 = X, 1 = Y, 2 = Z
    float sign = (face % 2 == 0) ? 1.0 : -1.0;

    // Quad no plano da face: (0,0) (0,1) (1,0) · (1,0) (0,1) (1,1), CCW visto de fora.
    const vec2 QUAD[6] = vec2[6](
        vec2(0, 0), vec2(0, 1), vec2(1, 0),
        vec2(1, 0), vec2(0, 1), vec2(1, 1)
    );
    vec2 q = QUAD[corner] * 2.0 - 1.0;
    // Com o sinal negativo a face olha para o outro lado, e o quad tem de inverter
    // com ela ou metade do cubo fica com winding ao contrário.
    q.x *= sign;

    // `q.y` vai para o eixo seguinte e `q.x` para o de trás -- uma permutação
    // cíclica, que é o que mantém o triedro com a mesma mão nas seis faces.
    // Escrito à mão como `(q.x, q.y, sign)` para o eixo Z, as duas faces de Z
    // saíam com o winding trocado e o back-face culling comia-as: o cubo ficava
    // com quatro faces e um entalhe. Foi preciso calcular os 36 vértices e
    // comparar o normal do winding com o normal pretendido para o ver.
    vec3 local;
    vec3 n = vec3(0.0);
    if (axis == 0)      { local = vec3(sign, q.y, q.x); n.x = sign; }
    else if (axis == 1) { local = vec3(q.x, sign, q.y); n.y = sign; }
    else                { local = vec3(q.y, q.x, sign); n.z = sign; }

    Instance inst = src[0].items[visible[1].items[gl_InstanceIndex]];
    vec3 world = local * inst.pos_scale.w + inst.pos_scale.xyz;

    v_normal = n;
    v_color = inst.color.rgb;
    gl_Position = cb.view_proj * vec4(world, 1.0);
}
