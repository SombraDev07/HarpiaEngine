#version 450
#extension GL_EXT_nonuniform_qualifier : require

layout(set = 0, binding = 0, std140) uniform Storm {
    layout(offset = 432) vec4 misc; // w = exposure
    layout(offset = 464) vec2 inv_extent;
    layout(offset = 472) uint scene_color;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) out vec4 out_color;

vec3 aces(vec3 x) {
    return (x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14);
}

void main() {
    vec2 uv = gl_FragCoord.xy * cb.inv_extent;
    vec3 rgb = texture(sampler2D(heap[nonuniformEXT(cb.scene_color)], samp_clamp), uv).rgb;
    vec3 toned = clamp(aces(rgb * cb.misc.w), 0.0, 1.0);
    out_color = vec4(pow(toned, vec3(1.0 / 2.2)), 1.0);
}
