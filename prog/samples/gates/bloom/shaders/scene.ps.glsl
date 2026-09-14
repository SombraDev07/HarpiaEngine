#version 450

// Three bright discs on a dark field. Linear HDR, no tonemap here.

layout(set = 0, binding = 0, std140) uniform Scene {
    vec2 inv_extent;
} cb;

layout(location = 0) out vec4 out_color;

void main() {
    vec2 uv = gl_FragCoord.xy * cb.inv_extent;
    vec2 p = uv * 2.0 - 1.0;
    p.x *= cb.inv_extent.y / cb.inv_extent.x;

    vec3 rgb = vec3(0.02, 0.03, 0.05);
    vec2 c0 = vec2(-0.45, 0.10);
    vec2 c1 = vec2(0.40, 0.15);
    vec2 c2 = vec2(0.00, -0.35);
    float r0 = 0.18 / max(length(p - c0), 0.02);
    float r1 = 0.18 / max(length(p - c1), 0.02);
    float r2 = 0.22 / max(length(p - c2), 0.02);
    rgb += vec3(8.0, 1.2, 0.4) * r0 * r0;
    rgb += vec3(0.4, 1.4, 8.0) * r1 * r1;
    rgb += vec3(1.5, 8.0, 1.0) * r2 * r2;
    out_color = vec4(rgb, 1.0);
}
