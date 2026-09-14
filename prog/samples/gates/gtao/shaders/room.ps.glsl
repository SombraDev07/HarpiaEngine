#version 450

// Lit colour + world normal + hardware-depth colour (for Hi-Z / GTAO).

layout(set = 0, binding = 0, std140) uniform Scene {
    layout(offset = 0) vec4 camera_pos;
    layout(offset = 16) vec4 sun_dir;
    layout(offset = 32) vec4 albedo;
} cb;

layout(location = 0) in vec3 v_world;
layout(location = 1) in vec3 v_normal;
layout(location = 2) in vec3 v_albedo;

layout(location = 0) out vec4 out_color;
layout(location = 1) out vec4 out_normal;
layout(location = 2) out float out_depth;

void main() {
    vec3 n = normalize(v_normal);
    vec3 l = normalize(-cb.sun_dir.xyz);
    float ndl = max(dot(n, l), 0.08);
    vec3 rgb = v_albedo * ndl;
    out_color = vec4(rgb, 1.0);
    out_normal = vec4(n * 0.5 + 0.5, 1.0);
    out_depth = gl_FragCoord.z;
}
