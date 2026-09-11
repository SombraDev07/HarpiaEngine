// VS do gate das luzes: esferas instanciadas sobre um chão.
// Layout de vértice e de instância partilhado com o resto da árvore.

#version 450

layout(location = 0) in vec3 in_pos;
layout(location = 1) in vec3 in_nrm;
layout(location = 2) in vec4 i_pos_scale;   // xyz posição, w escala
layout(location = 3) in vec4 i_albedo;
layout(location = 4) in vec4 i_orm;         // x oclusão, y rugosidade, z metálico
layout(location = 5) in vec4 i_emissive;
layout(location = 6) in vec4 i_fuzz;

layout(push_constant) uniform Push { mat4 view_proj; mat4 world; } pc;

layout(location = 0) out vec3 v_world;
layout(location = 1) out vec3 v_normal;
layout(location = 2) out vec3 v_albedo;
layout(location = 3) out vec2 v_material;

void main() {
    vec3 world = in_pos * i_pos_scale.w + i_pos_scale.xyz;
    v_world = world;
    v_normal = in_nrm;
    v_albedo = i_albedo.rgb;
    v_material = vec2(i_orm.y, i_orm.z);
    gl_Position = pc.view_proj * vec4(world, 1.0);
}
