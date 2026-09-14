#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Jimenez GTAO, 3 slices × 4 steps. Horizon search in screen space, closed-form
// inner integral. `--no-gtao` is `enable == 0`.

layout(set = 0, binding = 0, std140) uniform Gtao {
    layout(offset = 0)  mat4 inv_view_proj;
    layout(offset = 64) mat4 inv_proj;
    layout(offset = 128) mat4 view;
    layout(offset = 192) vec4 screen; // xy size, zw 1/size
    layout(offset = 208) uint color;
    layout(offset = 212) uint depth;
    layout(offset = 216) uint enable;
    layout(offset = 220) uint _pad0;
    layout(offset = 224) vec4 radius; // x = world radius, y = unused
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) out vec4 out_color;

const int SLICES = 3;
const int STEPS = 4;
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

// Jimenez 2016, inner integral of a slice given horizon angles and projected n.
float integrate_arc(float h1, float h2, float n) {
    float c2n = cos(n + n);
    float k = 0.25;
    float a = k * (-cos(h1 + h1 - n) + c2n + 2.0 * h1 * sin(n));
    float b = k * (-cos(h2 + h2 - n) + c2n + 2.0 * h2 * sin(n));
    return a + b;
}

float gtao(vec2 uv, vec3 view_p, vec3 view_n) {
    float radius = max(cb.radius.x, 0.05);
    float ao = 0.0;
    vec3 v = normalize(-view_p);
    float noise = fract(sin(dot(uv, vec2(12.9898, 78.233))) * 43758.5453);
    for (int slice = 0; slice < SLICES; ++slice) {
        float phi = (float(slice) + noise) * PI / float(SLICES);
        vec2 omega = vec2(cos(phi), sin(phi));
        vec3 dir_ss = vec3(omega, 0.0);
        vec3 dir_vs = normalize((cb.inv_proj * vec4(dir_ss, 0.0)).xyz);
        vec3 t = normalize(dir_vs - v * dot(dir_vs, v));
        vec3 slice_n = cross(t, v);
        float n_proj = length(view_n - slice_n * dot(view_n, slice_n));
        vec3 nn = (view_n - slice_n * dot(view_n, slice_n)) / max(n_proj, 1e-4);
        float n_angle = atan(dot(nn, t), dot(nn, v));

        float h1 = -1.0;
        float h2 = -1.0;
        for (int s = 1; s <= STEPS; ++s) {
            float k = (float(s) - 0.5) / float(STEPS);
            vec2 offset = omega * (k * radius * cb.screen.zw * cb.screen.x / max(-view_p.z, 0.1));
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
        float a1 = acos(clamp(h1, -1.0, 1.0));
        float a2 = acos(clamp(h2, -1.0, 1.0));
        ao += integrate_arc(-a1, a2, n_angle) / max(n_proj, 0.15);
    }
    return clamp(ao / float(SLICES), 0.0, 1.0);
}

vec3 aces(vec3 x) {
    return (x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14);
}

void main() {
    vec2 uv = gl_FragCoord.xy * cb.screen.zw;
    vec3 lit = textureLod(sampler2D(heap[nonuniformEXT(cb.color)], samp_clamp), uv, 0.0).rgb;
    float z = fetch_depth(uv);
    vec3 rgb = lit;
    if (cb.enable != 0u && z < 0.999) {
        vec3 view_p = view_from_depth(uv, z);
        vec2 px = cb.screen.zw;
        vec3 pxp = view_from_depth(uv + vec2(px.x, 0.0), fetch_depth(uv + vec2(px.x, 0.0)));
        vec3 pyp = view_from_depth(uv + vec2(0.0, px.y), fetch_depth(uv + vec2(0.0, px.y)));
        vec3 vn = normalize(cross(pyp - view_p, pxp - view_p));
        if (dot(vn, -view_p) < 0.0) {
            vn = -vn;
        }
        float ao = gtao(uv, view_p, vn);
        rgb *= ao;
    }
    out_color = vec4(pow(clamp(aces(rgb), 0.0, 1.0), vec3(1.0 / 2.2)), 1.0);
}
