#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Rec.709 luma → atomic counters. One thread group does not own the image.

layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;

layout(set = 0, binding = 0, std140) uniform Luma {
    vec4 params; // x = hdr index, y = width, z = height, w = acc slot
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;
layout(set = 3, binding = 0, std430) buffer Acc { uint v[]; } acc[];

void main() {
    ivec2 id = ivec2(gl_GlobalInvocationID.xy);
    int w = int(cb.params.y);
    int h = int(cb.params.z);
    if (id.x >= w || id.y >= h) {
        return;
    }
    vec3 c = texelFetch(sampler2D(heap[nonuniformEXT(uint(cb.params.x))], samp_clamp), id, 0).rgb;
    float lum = dot(c, vec3(0.2126, 0.7152, 0.0722));
    uint slot = uint(cb.params.w);
    atomicAdd(acc[slot].v[0], uint(clamp(lum, 0.0, 64.0) * 256.0));
    atomicAdd(acc[slot].v[1], 1u);
}
