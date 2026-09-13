#version 450

layout(location = 0) in vec3 in_pos;
layout(location = 1) in vec3 in_nrm;

layout(location = 0) out vec3 v_world;
layout(location = 1) out vec3 v_nrm;
layout(location = 2) out float v_z;

layout(push_constant) uniform Push {
    mat4 view_proj;
    mat4 world;
} pc;

void main() {
    vec4 wp = pc.world * vec4(in_pos, 1.0);
    v_world = wp.xyz;
    mat3 m = mat3(pc.world);
    v_nrm = transpose(inverse(m)) * in_nrm;
    gl_Position = pc.view_proj * wp;
    v_z = gl_Position.w;
}
