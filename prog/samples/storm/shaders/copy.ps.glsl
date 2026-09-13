#version 450
#extension GL_EXT_nonuniform_qualifier : require

layout(set = 0, binding = 0, std140) uniform Storm {
    layout(offset = 464) vec2 inv_extent;
    layout(offset = 472) uint scene_color;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) out vec4 out_color;

void main() {
    vec2 uv = gl_FragCoord.xy * cb.inv_extent;
    out_color = texture(sampler2D(heap[nonuniformEXT(cb.scene_color)], samp_clamp), uv);
}
