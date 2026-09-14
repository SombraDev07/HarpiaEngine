#version 450
#extension GL_EXT_nonuniform_qualifier : require

// HDR + SPD pyramid, ACES to the swapchain. `enable == 0` is `--no-bloom`.

layout(set = 0, binding = 0, std140) uniform Bloom {
    vec2 inv_extent;
    uint hdr;
    uint pyramid;
    uint mips;
    uint enable;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) out vec4 out_color;

vec3 aces(vec3 x) {
    return (x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14);
}

void main() {
    vec2 uv = gl_FragCoord.xy * cb.inv_extent;
    vec3 rgb = texture(sampler2D(heap[nonuniformEXT(cb.hdr)], samp_clamp), uv).rgb;
    if (cb.enable != 0u) {
        vec3 bloom = vec3(0.0);
        float wsum = 0.0;
        for (int m = 1; m < int(cb.mips); ++m) {
            float w = 1.0 / float(m);
            bloom += textureLod(sampler2D(heap[nonuniformEXT(cb.pyramid)], samp_clamp), uv, float(m)).rgb * w;
            wsum += w;
        }
        rgb += bloom / max(wsum, 1e-4) * 1.2;
    }
    out_color = vec4(pow(clamp(aces(rgb), 0.0, 1.0), vec3(1.0 / 2.2)), 1.0);
}
