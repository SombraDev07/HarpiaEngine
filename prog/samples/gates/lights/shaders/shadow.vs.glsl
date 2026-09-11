// Profundidade vista de um projector, para o atlas de sombras.
//
// O mesmo layout de vértice e de instância do `lights.vs`, para o que se desenha
// no mapa ser exactamente a mesma geometria que se desenha no ecrã. Um VS de
// sombra que calcule a posição de outra maneira é a origem clássica do
// shadow acne que não se resolve com bias nenhum.

#version 450

layout(location = 0) in vec3 in_pos;
layout(location = 1) in vec3 in_nrm;
layout(location = 2) in vec4 i_pos_scale;
layout(location = 3) in vec4 i_albedo;
layout(location = 4) in vec4 i_orm;
layout(location = 5) in vec4 i_emissive;
layout(location = 6) in vec4 i_fuzz;

layout(push_constant) uniform Push { mat4 view_proj; mat4 world; } pc;

void main() {
    vec3 world = in_pos * i_pos_scale.w + i_pos_scale.xyz;
    gl_Position = pc.view_proj * vec4(world, 1.0);
}
