#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Composite SSSR over the lit scene. Mirror floors use a high F0; the rest
// keeps the lit colour. `--no-ssr` skips this pass in the gate.

layout(set = 0, binding = 0, std140) uniform Apply {
    layout(offset = 0) vec2 inv_extent;
    layout(offset = 8) uint color;
    layout(offset = 12) uint ssr;
    layout(offset = 16) uint enable;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) out vec4 out_color;

vec3 aces(vec3 x) {
    return (x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14);
}

void main() {
    vec2 uv = gl_FragCoord.xy * cb.inv_extent;
    vec4 lit = texture(sampler2D(heap[nonuniformEXT(cb.color)], samp_clamp), uv);
    vec3 rgb = lit.rgb;
    if (cb.enable != 0u) {
        vec4 refl = texture(sampler2D(heap[nonuniformEXT(cb.ssr)], samp_clamp), uv);
        rgb = mix(rgb, rgb + refl.rgb, refl.a);
    }
    out_color = vec4(pow(clamp(aces(rgb), 0.0, 1.0), vec3(1.0 / 2.2)), 1.0);
}
