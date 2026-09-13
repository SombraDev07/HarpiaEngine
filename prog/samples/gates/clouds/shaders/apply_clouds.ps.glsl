#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Apply half-res clouds over an HDR scene: `scene * transmittance + scatter`.
// No ACES — the terrain blit tonemaps. Depth 1 (clear) is sky; anything closer
// is terrain and keeps the scene colour, so the layer stays above the hills.

layout(set = 0, binding = 0, std140) uniform Cloud {
    layout(offset = 192) vec2 inv_extent;
    layout(offset = 200) uint cloud_rt;
    layout(offset = 272) uint scene;
    layout(offset = 276) uint depth;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) out vec4 out_color;

void main() {
    vec2 uv = gl_FragCoord.xy * cb.inv_extent;
    vec3 scene = textureLod(
        sampler2D(heap[nonuniformEXT(cb.scene)], samp_clamp),
        uv,
        0.0
    ).rgb;

    if (cb.depth != 0u) {
        float d = textureLod(
            sampler2D(heap[nonuniformEXT(cb.depth)], samp_clamp),
            uv,
            0.0
        ).r;
        if (d < 1.0) {
            out_color = vec4(scene, 1.0);
            return;
        }
    }

    vec4 cloud = textureLod(
        sampler2D(heap[nonuniformEXT(cb.cloud_rt)], samp_clamp),
        uv,
        0.0
    );
    out_color = vec4(scene * cloud.a + cloud.rgb, 1.0);
}
