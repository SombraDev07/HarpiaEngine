// Tonemap the linear composite onto the swapchain.
//
// The first shader in this tree written in GLSL instead of hand-written SPIR-V
// assembly. It replaces blit.ps.spvasm one-for-one and must produce the same
// pixels -- that is the whole point of migrating this one first.
//
// Only the block members this shader reads are declared, each with its byte
// offset from RainCb. A GLSL block does not have to mirror the whole struct, but
// every offset it does name has to be right, so the offsets are the thing the
// layout test checks.

#version 450
#extension GL_EXT_nonuniform_qualifier : require

layout(set = 0, binding = 0, std140) uniform Rain {
    layout(offset = 256) vec2 inv_extent;
    layout(offset = 264) uint scene_color;
    layout(offset = 272) vec4 misc; // x = exposure
} cb;

// set 1 = the sampled 2D heap, set 2 binding 1 = the clamp sampler. Separate
// image and sampler, which is what the bindless layout in docs/ specifies.
layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) out vec4 out_color;

// Narkowicz's ACES fit, written the same way round as the assembly it replaces
// so the two agree bit for bit.
vec3 aces(vec3 x) {
    return (x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14);
}

void main() {
    vec2 uv = gl_FragCoord.xy * cb.inv_extent;
    vec3 rgb = texture(
        sampler2D(heap[nonuniformEXT(cb.scene_color)], samp_clamp), uv
    ).rgb;

    vec3 x = rgb * cb.misc.x;
    vec3 toned = clamp(aces(x), 0.0, 1.0);
    // The swapchain is B8G8R8A8_UNORM: nothing else encodes sRGB.
    out_color = vec4(pow(toned, vec3(1.0 / 2.2)), 1.0);
}
