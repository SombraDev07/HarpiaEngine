#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Scene + SSSR. On a miss (`ssr.a` low) sample the CPU-seeded probe lat-long.

layout(set = 0, binding = 0, std140) uniform Apply {
    layout(offset = 0)   mat4 inv_view_proj;
    layout(offset = 64)  vec4 camera_pos;
    layout(offset = 80)  vec2 inv_extent;
    layout(offset = 88)  uint color;
    layout(offset = 92)  uint depth;
    layout(offset = 96)  uint normal;
    layout(offset = 100) uint ssr;
    layout(offset = 104) uint probe;
    layout(offset = 108) uint enable_ssr;
    layout(offset = 112) uint enable_probe;
    layout(offset = 116) uint probe_mips;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) out vec4 out_color;

const float PI = 3.14159265;

vec2 dir_to_uv(vec3 d) {
    vec3 n = normalize(d);
    float u = atan(n.z, n.x) / (2.0 * PI) + 0.5;
    float v = acos(clamp(n.y, -1.0, 1.0)) / PI;
    return vec2(fract(u), clamp(v, 0.0, 1.0));
}

vec3 sample_probe(vec3 dir, float roughness) {
    float lod = roughness * float(max(cb.probe_mips, 1u) - 1u);
    return textureLod(
        sampler2D(heap[nonuniformEXT(cb.probe)], samp_clamp),
        dir_to_uv(dir),
        lod
    ).rgb;
}

vec3 world_from_depth(vec2 uv, float z) {
    vec4 clip = vec4(uv * 2.0 - 1.0, z, 1.0);
    vec4 w = cb.inv_view_proj * clip;
    return w.xyz / w.w;
}

vec3 aces(vec3 x) {
    return (x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14);
}

void main() {
    vec2 uv = gl_FragCoord.xy * cb.inv_extent;
    vec3 rgb = texture(sampler2D(heap[nonuniformEXT(cb.color)], samp_clamp), uv).rgb;
    float z = texture(sampler2D(heap[nonuniformEXT(cb.depth)], samp_clamp), uv).r;
    if (z < 0.999) {
        vec4 refl = vec4(0.0);
        if (cb.enable_ssr != 0u) {
            refl = texture(sampler2D(heap[nonuniformEXT(cb.ssr)], samp_clamp), uv);
        }
        if (refl.a < 0.08 && cb.enable_probe != 0u) {
            vec3 n = normalize(
                texture(sampler2D(heap[nonuniformEXT(cb.normal)], samp_clamp), uv).xyz * 2.0 - 1.0
            );
            vec3 world = world_from_depth(uv, z);
            vec3 v = normalize(cb.camera_pos.xyz - world);
            vec3 r = reflect(-v, n);
            refl = vec4(sample_probe(r, 0.08), 1.0);
        }
        rgb = mix(rgb, rgb + refl.rgb, clamp(refl.a, 0.0, 1.0));
    }
    out_color = vec4(pow(clamp(aces(rgb), 0.0, 1.0), vec3(1.0 / 2.2)), 1.0);
}
