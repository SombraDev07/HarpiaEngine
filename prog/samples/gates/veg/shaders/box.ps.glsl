// Lambert simples. O gate é sobre culling, não sobre luz.
#version 450

layout(set = 0, binding = 0, std140) uniform Cb {
    layout(offset = 0)   mat4 view_proj;
    layout(offset = 64)  vec4 camera_pos;
    layout(offset = 80)  vec4 sun_dir;
    layout(offset = 96)  vec4 params;
    layout(offset = 112) vec4 hiz;
} cb;

layout(location = 0) in vec3 v_normal;
layout(location = 1) in vec3 v_color;
layout(location = 0) out vec4 out_color;

void main() {
    float ndl = max(dot(normalize(v_normal), normalize(cb.sun_dir.xyz)), 0.0);
    out_color = vec4(v_color * (0.18 + 0.82 * ndl), 1.0);
}
