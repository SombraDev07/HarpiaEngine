#version 450
#extension GL_EXT_nonuniform_qualifier : require

// World normals from hardware-depth neighbours, for SSSR.

layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;

layout(set = 0, binding = 0, std140) uniform Nrm {
    mat4 inv_view_proj;
    vec4 params; // xy size, zw 1/size
    uint depth;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;
layout(set = 4, binding = 0, rgba8) uniform image2D uav[];

vec3 world_from(vec2 uv, float z) {
    vec4 clip = vec4(uv * 2.0 - 1.0, z, 1.0);
    vec4 w = cb.inv_view_proj * clip;
    return w.xyz / w.w;
}

float fetch(vec2 uv) {
    return textureLod(sampler2D(heap[nonuniformEXT(cb.depth)], samp_clamp), uv, 0.0).r;
}

void main() {
    ivec2 id = ivec2(gl_GlobalInvocationID.xy);
    ivec2 size = ivec2(cb.params.xy);
    if (any(greaterThanEqual(id, size))) {
        return;
    }
    vec2 uv = (vec2(id) + 0.5) * cb.params.zw;
    float z = fetch(uv);
    if (z >= 0.999) {
        imageStore(uav[13], id, vec4(0.5, 0.5, 1.0, 1.0));
        return;
    }
    vec2 px = cb.params.zw;
    vec3 w0 = world_from(uv, z);
    vec3 wx = world_from(uv + vec2(px.x, 0.0), fetch(uv + vec2(px.x, 0.0)));
    vec3 wy = world_from(uv + vec2(0.0, px.y), fetch(uv + vec2(0.0, px.y)));
    vec3 n = normalize(cross(wx - w0, wy - w0));
    imageStore(uav[13], id, vec4(n * 0.5 + 0.5, 1.0));
}
