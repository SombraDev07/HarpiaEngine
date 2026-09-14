#version 450
#extension GL_EXT_nonuniform_qualifier : require

// View-linear depth (`clip.w`) → NDC z for SSSR Hi-Z. D32 in compute came out
// all zeros on this path (D70); the colour RT is the honest source.

layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;

layout(set = 0, binding = 0, std140) uniform Copy {
    vec4 params; // x = depth index, y = width, z = height, w unused
    vec4 clip;   // x = near, y = far
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
    float near = cb.clip.x;
    float far = cb.clip.y;
    float ndc = 1.0;
    if (d > 1e-4 && d < far * 0.999) {
        ndc = far * (near - d) / ((near - far) * d);
        ndc = clamp(ndc, 0.0, 1.0);
    }
    imageStore(uav[0], id, vec4(ndc, 0.0, 0.0, 1.0));
}
