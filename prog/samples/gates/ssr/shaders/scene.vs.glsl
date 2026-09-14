#version 450

layout(location = 0) in vec3 in_pos;
layout(location = 1) in vec3 in_nrm;
layout(location = 2) in vec4 i_pos_scale;
layout(location = 3) in vec4 i_albedo;
layout(location = 4) in vec4 i_orm;
layout(location = 5) in vec4 i_emissive;
layout(location = 6) in vec4 i_fuzz;

layout(push_constant) uniform Push { mat4 view_proj; mat4 world; } pc;

layout(location = 0) out vec3 v_world;
layout(location = 1) out vec3 v_normal;
layout(location = 2) out vec3 v_albedo;
layout(location = 3) out vec3 v_emissive;

void main() {
    vec3 world = in_pos * i_pos_scale.w + i_pos_scale.xyz;
    v_world = world;
    v_normal = in_nrm;
    v_albedo = i_albedo.rgb;
    v_emissive = i_emissive.rgb;
    gl_Position = pc.view_proj * vec4(world, 1.0);
}
