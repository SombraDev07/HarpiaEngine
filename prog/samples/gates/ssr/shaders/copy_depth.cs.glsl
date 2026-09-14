#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Depth buffer → Hi-Z mip 0. SPD min fills the rest.

layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;

layout(set = 0, binding = 0, std140) uniform Copy {
    vec4 params; // x = depth index, y = width, z = height
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;
layout(set = 4, binding = 0, r32f) uniform image2D uav[];

void main() {
    ivec2 id = ivec2(gl_GlobalInvocationID.xy);
    int w = int(cb.params.y);
    int h = int(cb.params.z);
    if (id.x >= w || id.y >= h) {
        return;
    }
    float d = texelFetch(sampler2D(heap[nonuniformEXT(uint(cb.params.x))], samp_clamp), id, 0).r;
    imageStore(uav[0], id, vec4(d, 0.0, 0.0, 1.0));
}
