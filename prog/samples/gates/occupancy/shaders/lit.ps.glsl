#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Forward Lambert with occupancy cone-trace in the lighting itself.
// `enable == 0` still runs the same PS so the A/B is the volume, not a skip.

layout(set = 0, binding = 0, std140) uniform Lit {
    layout(offset = 0)  vec4 camera_pos;
    layout(offset = 16) vec4 sun_dir;
    layout(offset = 32) vec4 albedo;
    layout(offset = 48) vec4 origin_extent; // xyz origin, w = extent (cube)
    layout(offset = 64) uint enable;
} cb;

layout(set = 5, binding = 0) uniform texture3D volumes[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) in vec3 v_world;
layout(location = 1) in vec3 v_normal;
layout(location = 2) in vec3 v_albedo;

layout(location = 0) out vec4 out_color;

const uint OCC_SLOT = 2u;
const int OCC_STEPS = 4;

float sample_occ(vec3 p) {
    float extent = max(cb.origin_extent.w, 1e-4);
    vec3 uvw = (p - cb.origin_extent.xyz) / extent;
    if (any(lessThan(uvw, vec3(0.0))) || any(greaterThan(uvw, vec3(1.0)))) {
        return 0.0;
    }
    return textureLod(sampler3D(volumes[OCC_SLOT], samp_clamp), uvw, 0.0).r;
}

float cone_ao(vec3 world, vec3 n) {
    float extent = max(cb.origin_extent.w, 1e-4);
    float vs = extent / 32.0;
    vec3 up = abs(n.y) > 0.9 ? vec3(1.0, 0.0, 0.0) : vec3(0.0, 1.0, 0.0);
    vec3 t = normalize(cross(n, up));
    vec3 b = cross(n, t);
    vec3 dirs[5] = vec3[](n, normalize(n + t * 0.7), normalize(n - t * 0.7), normalize(n + b * 0.7), normalize(n - b * 0.7));
    float occ = 0.0;
    for (int d = 0; d < 5; ++d) {
        vec3 p = world + dirs[d] * vs * 1.5;
        for (int i = 0; i < OCC_STEPS; ++i) {
            p += dirs[d] * vs;
            occ += sample_occ(p);
        }
    }
    return 1.0 / (1.0 + occ * 1.1);
}

void main() {
    vec3 n = normalize(v_normal);
    vec3 l = normalize(-cb.sun_dir.xyz);
    float ndl = max(dot(n, l), 0.08);
    float ao = 1.0;
    if (cb.enable != 0u) {
        ao = cone_ao(v_world, n);
    }
    vec3 rgb = v_albedo * ndl * ao;
    out_color = vec4(rgb, 1.0);
}
