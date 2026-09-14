#version 450

// Mesh in object space, instance via push `world`.

layout(location = 0) in vec3 in_pos;
layout(location = 1) in vec3 in_nrm;

layout(push_constant) uniform Push { mat4 view_proj; mat4 world; } pc;

layout(location = 0) out vec3 v_world;
layout(location = 1) out vec3 v_normal;
layout(location = 2) out vec3 v_albedo;

layout(set = 0, binding = 0, std140) uniform Scene {
    layout(offset = 0) vec4 camera_pos;
    layout(offset = 16) vec4 sun_dir;
    layout(offset = 32) vec4 albedo;
} cb;

void main() {
    vec4 world = pc.world * vec4(in_pos, 1.0);
    v_world = world.xyz;
    v_normal = mat3(pc.world) * in_nrm;
    v_albedo = cb.albedo.rgb;
    gl_Position = pc.view_proj * world;
}
