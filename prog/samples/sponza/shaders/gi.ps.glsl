#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Phase 7 compose: GTAO × occupancy, SSSR, probe on miss, auto-exposure.
// Lighting already ran in the forward pass; this is the compose that *reads*
// occupancy (bar: sample it or do not create the volume).

layout(set = 0, binding = 0, std140) uniform Gi {
    layout(offset = 0)   mat4 inv_view_proj;
    layout(offset = 64)  mat4 inv_proj;
    layout(offset = 128) vec4 camera_pos;
    layout(offset = 144) vec4 screen; // xy size, zw 1/size
    layout(offset = 160) vec4 origin_extent; // xyz origin, w extent
    layout(offset = 176) uint color;
    layout(offset = 180) uint depth;
    layout(offset = 184) uint ssr;
    layout(offset = 188) uint probe;
    layout(offset = 192) uint exposure_tex;
    layout(offset = 196) uint flags; // 1 gtao, 2 occ, 4 ssr, 8 probe, 16 exposure
    layout(offset = 200) uint probe_mips;
    layout(offset = 204) uint _pad;
    layout(offset = 208) float gtao_radius;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;
layout(set = 5, binding = 0) uniform texture3D volumes[];

layout(location = 0) out vec4 out_color;

const uint OCC_SLOT = 2u;
const int SLICES = 3;
const int STEPS = 4;
const int OCC_STEPS = 4;
const float PI = 3.14159265;

vec3 world_from_depth(vec2 uv, float z) {
    vec4 clip = vec4(uv * 2.0 - 1.0, z, 1.0);
    vec4 w = cb.inv_view_proj * clip;
    return w.xyz / w.w;
}

vec3 view_from_depth(vec2 uv, float z) {
    vec4 clip = vec4(uv * 2.0 - 1.0, z, 1.0);
    vec4 v = cb.inv_proj * clip;
    return v.xyz / v.w;
}

float fetch_depth(vec2 uv) {
    return textureLod(sampler2D(heap[nonuniformEXT(cb.depth)], samp_clamp), uv, 0.0).r;
}

float integrate_arc(float h1, float h2, float n) {
    float c2n = cos(n + n);
    float k = 0.25;
    float a = k * (-cos(h1 + h1 - n) + c2n + 2.0 * h1 * sin(n));
    float b = k * (-cos(h2 + h2 - n) + c2n + 2.0 * h2 * sin(n));
    return a + b;
}

float gtao(vec2 uv, vec3 view_p, vec3 view_n) {
    float radius = max(cb.gtao_radius, 0.05);
    float ao = 0.0;
    vec3 v = normalize(-view_p);
    float noise = fract(sin(dot(uv, vec2(12.9898, 78.233))) * 43758.5453);
    for (int slice = 0; slice < SLICES; ++slice) {
        float phi = (float(slice) + noise) * PI / float(SLICES);
        vec2 omega = vec2(cos(phi), sin(phi));
        vec3 dir_vs = normalize((cb.inv_proj * vec4(omega, 0.0, 0.0)).xyz);
        vec3 t = normalize(dir_vs - v * dot(dir_vs, v));
        vec3 slice_n = cross(t, v);
        float n_proj = length(view_n - slice_n * dot(view_n, slice_n));
        vec3 nn = (view_n - slice_n * dot(view_n, slice_n)) / max(n_proj, 1e-4);
        float n_angle = atan(dot(nn, t), dot(nn, v));
        float h1 = -1.0;
        float h2 = -1.0;
        for (int s = 1; s <= STEPS; ++s) {
            float kstep = (float(s) - 0.5) / float(STEPS);
            vec2 offset = omega * (kstep * radius * cb.screen.zw * cb.screen.x / max(-view_p.z, 0.1));
            vec2 uv1 = clamp(uv + offset, vec2(0.0), vec2(1.0));
            vec2 uv2 = clamp(uv - offset, vec2(0.0), vec2(1.0));
            vec3 p1 = view_from_depth(uv1, fetch_depth(uv1));
            vec3 p2 = view_from_depth(uv2, fetch_depth(uv2));
            vec3 d1 = p1 - view_p;
            vec3 d2 = p2 - view_p;
            float l1 = length(d1);
            float l2 = length(d2);
            if (l1 > 1e-4 && l1 < radius * 4.0) {
                h1 = max(h1, dot(d1 / l1, v));
            }
            if (l2 > 1e-4 && l2 < radius * 4.0) {
                h2 = max(h2, dot(d2 / l2, v));
            }
        }
        ao += integrate_arc(-acos(clamp(h1, -1.0, 1.0)), acos(clamp(h2, -1.0, 1.0)), n_angle)
            / max(n_proj, 0.15);
    }
    return clamp(ao / float(SLICES), 0.0, 1.0);
}

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

vec2 dir_to_uv(vec3 d) {
    vec3 n = normalize(d);
    float u = atan(n.z, n.x) / (2.0 * PI) + 0.5;
    float v = acos(clamp(n.y, -1.0, 1.0)) / PI;
    return vec2(fract(u), clamp(v, 0.0, 1.0));
}

void main() {
    vec2 uv = gl_FragCoord.xy * cb.screen.zw;
    vec3 rgb = textureLod(sampler2D(heap[nonuniformEXT(cb.color)], samp_clamp), uv, 0.0).rgb;
    float z = fetch_depth(uv);
    if (z < 0.999) {
        vec3 view_p = view_from_depth(uv, z);
        vec2 px = cb.screen.zw;
        vec3 pxp = view_from_depth(uv + vec2(px.x, 0.0), fetch_depth(uv + vec2(px.x, 0.0)));
        vec3 pyp = view_from_depth(uv + vec2(0.0, px.y), fetch_depth(uv + vec2(0.0, px.y)));
        vec3 vn = normalize(cross(pyp - view_p, pxp - view_p));
        if (dot(vn, -view_p) < 0.0) {
            vn = -vn;
        }
        vec3 world = world_from_depth(uv, z);
        vec3 wx = world_from_depth(uv + vec2(px.x, 0.0), fetch_depth(uv + vec2(px.x, 0.0)));
        vec3 wy = world_from_depth(uv + vec2(0.0, px.y), fetch_depth(uv + vec2(0.0, px.y)));
        vec3 wn = normalize(cross(wx - world, wy - world));
        if (dot(wn, cb.camera_pos.xyz - world) < 0.0) {
            wn = -wn;
        }

        if ((cb.flags & 1u) != 0u) {
            rgb *= gtao(uv, view_p, vn);
        }
        if ((cb.flags & 2u) != 0u) {
            rgb *= cone_ao(world, wn);
        }

        vec4 refl = vec4(0.0);
        if ((cb.flags & 4u) != 0u) {
            refl = textureLod(sampler2D(heap[nonuniformEXT(cb.ssr)], samp_clamp), uv, 0.0);
        }
        if (refl.a < 0.08 && (cb.flags & 8u) != 0u) {
            vec3 vdir = normalize(cb.camera_pos.xyz - world);
            vec3 r = reflect(-vdir, wn);
            float lod = 0.15 * float(max(cb.probe_mips, 1u) - 1u);
            vec3 env = textureLod(
                sampler2D(heap[nonuniformEXT(cb.probe)], samp_clamp),
                dir_to_uv(r),
                lod
            ).rgb;
            refl = vec4(env, 0.35);
        }
        rgb = mix(rgb, rgb + refl.rgb, clamp(refl.a, 0.0, 1.0));
    }
    if ((cb.flags & 16u) != 0u) {
        float e = texelFetch(
            sampler2D(heap[nonuniformEXT(cb.exposure_tex)], samp_clamp),
            ivec2(0, 0),
            0
        ).r;
        rgb *= max(e, 0.05);
    }
    out_color = vec4(rgb, 1.0);
}
