// Lambert com um pouco de ambiente. O gate é sobre culling, não sobre shading.
#version 450

layout(set = 0, binding = 0, std140) uniform Draw {
    layout(offset = 0)   mat4 view_proj;
    layout(offset = 64)  vec4 sun_dir;
} cb;

layout(location = 0) in vec3 v_normal;
layout(location = 1) in vec3 v_color;
layout(location = 0) out vec4 out_color;

void main() {
    vec3 n = normalize(v_normal);
    float ndl = max(dot(n, normalize(cb.sun_dir.xyz)), 0.0);
    out_color = vec4(v_color * (0.15 + 0.85 * ndl), 1.0);
}
