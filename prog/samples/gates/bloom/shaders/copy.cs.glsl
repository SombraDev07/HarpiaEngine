#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Bright-pass into pyramid mip 0. Karis SPD then downsamples that.

layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;

layout(set = 0, binding = 0, std140) uniform Copy {
    vec4 params; // x = hdr index, y = width, z = height, w = threshold
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;
layout(set = 4, binding = 0, rgba16f) uniform image2D uav[];

void main() {
    ivec2 id = ivec2(gl_GlobalInvocationID.xy);
    int w = int(cb.params.y);
    int h = int(cb.params.z);
    if (id.x >= w || id.y >= h) {
        return;
    }
    vec3 c = texelFetch(sampler2D(heap[nonuniformEXT(uint(cb.params.x))], samp_clamp), id, 0).rgb;
    float lum = dot(c, vec3(0.2126, 0.7152, 0.0722));
    float t = cb.params.w;
    vec3 bright = c * max(lum - t, 0.0) / max(lum, 1e-4);
    imageStore(uav[0], id, vec4(bright, 1.0));
}
