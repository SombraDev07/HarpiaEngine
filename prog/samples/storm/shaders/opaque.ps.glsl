#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Wet PBR + clustered-style point/spot lights. The wetness terms are the ones
// pinned in `harpia_render::apply_wetness`: porosity (albedo²), roughness drop,
// metalness killed. Doing only one of the three is frost or varnish.

layout(set = 0, binding = 0, std140) uniform Storm {
    layout(offset = 128) mat4 view;
    layout(offset = 192) vec4 camera_pos;
    layout(offset = 208) vec4 sun_dir;
    layout(offset = 224) vec4 sun_color;
    layout(offset = 240) vec4 sky_zenith;
    layout(offset = 256) vec4 sky_horizon;
    layout(offset = 272) vec4 rain;     // x intensity, y wetness, z ripple, w time
    layout(offset = 304) vec4 ripple;
    layout(offset = 320) vec4 wind;     // z puddle, w water level
    layout(offset = 480) uint rain_map;
    layout(offset = 484) uint light_count;
    layout(offset = 496) vec4 material; // rgb dry albedo, a dry roughness; a < 0 = emissive
    layout(offset = 512) mat4 rain_map_vp;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

struct Light {
    vec4 position_radius;
    vec4 color;
    vec4 dir_cos_outer;
};
layout(set = 3, binding = 0, std430) readonly buffer Lights { Light lights[]; } light_buf[];

layout(location = 0) in vec3 v_world;
layout(location = 1) in vec3 v_nrm;
layout(location = 2) in float v_z;
layout(location = 0) out vec4 out_color;
layout(location = 1) out float out_depth;

const float PI = 3.14159265359;
const float WET_POROSITY = 0.6;
const float WET_ROUGHNESS = 0.1;
const float POOL_THR = 0.82;
const float SHORE_BORDER = 0.65;

float clamp_range(float v, float lo, float hi) {
    return clamp((v - lo) / max(hi - lo, 1e-3), 0.0, 1.0);
}

float hash11(float x) {
    uint n = uint(int(floor(x))) * 374761393u;
    n = (n ^ (n >> 13)) * 1274126177u;
    n = n ^ (n >> 16);
    return float(n) * (1.0 / 4294967296.0);
}

float d_ggx(float ndh, float alpha) {
    float a2 = alpha * alpha;
    float d = ndh * ndh * (a2 - 1.0) + 1.0;
    return a2 / max(PI * d * d, 1e-7);
}

float v_smith_correlated(float ndv, float ndl, float alpha) {
    float a2 = alpha * alpha;
    float lv = ndl * sqrt(ndv * ndv * (1.0 - a2) + a2);
    float ll = ndv * sqrt(ndl * ndl * (1.0 - a2) + a2);
    return 0.5 / max(lv + ll, 1e-6);
}

vec3 f_schlick(vec3 f0, float vdh) {
    return f0 + (vec3(1.0) - f0) * pow(1.0 - vdh, 5.0);
}

vec3 shade(vec3 n, vec3 v, vec3 l, vec3 radiance, vec3 albedo, float rough, float metal) {
    float ndl = max(dot(n, l), 0.0);
    if (ndl <= 0.0) return vec3(0.0);
    vec3 h = normalize(v + l);
    float ndv = max(dot(n, v), 1e-4);
    float ndh = max(dot(n, h), 0.0);
    float vdh = max(dot(v, h), 0.0);
    float alpha = max(rough * rough, 1e-3);
    vec3 f0 = mix(vec3(0.02), albedo, metal);
    vec3 spec = f_schlick(f0, vdh) * d_ggx(ndh, alpha) * v_smith_correlated(ndv, ndl, alpha);
    vec3 kd = (vec3(1.0) - f_schlick(f0, ndv)) * (1.0 - metal);
    return (kd * albedo / PI + spec) * radiance * ndl;
}

float attenuation(float dist, float radius) {
    float t = clamp(1.0 - pow(dist / max(radius, 1e-4), 4.0), 0.0, 1.0);
    return (t * t) / (dist * dist + 1.0);
}

float cone_falloff(vec3 l, vec4 dir_cos_outer, float cos_inner) {
    float cos_outer = dir_cos_outer.w;
    if (cos_outer <= -1.0) return 1.0;
    float cd = dot(-l, dir_cos_outer.xyz);
    return clamp((cd - cos_outer) / max(cos_inner - cos_outer, 1e-4), 0.0, 1.0);
}

float cell_ripple(vec2 cell, vec2 pxz, float t) {
    float h0 = hash11(cell.x * 13.0 + cell.y * 7.0);
    float h1 = hash11(cell.x * 5.0 + cell.y * 17.0 + 3.0);
    vec2 ctr = (cell + vec2(h0, h1)) * cb.ripple.x;
    float dist = length(pxz - ctr);
    float wave = sin(dist * cb.ripple.y - t * cb.ripple.z + h0 * 6.28318531);
    return wave * exp(-dist * cb.ripple.w);
}

void main() {
    if (cb.material.a < 0.0) {
        out_color = vec4(cb.material.rgb, 1.0);
        out_depth = v_z;
        return;
    }

    vec3 N0 = normalize(v_nrm);
    float wetness = clamp(cb.rain.y, 0.0, 1.0);
    float puddle = clamp(cb.wind.z, 0.0, 1.0);
    float water_level = cb.wind.w;
    float shore = 1.0 - clamp_range(v_world.y - water_level, -SHORE_BORDER, SHORE_BORDER);
    float w = clamp(max(wetness, shore) * (0.55 + 0.45 * puddle), 0.0, 1.0);

    // A roof that still soaks the deck is just a box. The rain map is the
    // occluder the streaks already use; wetness has to read it too.
    if (cb.rain_map != 0u) {
        vec4 rclip = cb.rain_map_vp * vec4(v_world, 1.0);
        vec3 rndc = rclip.xyz / max(rclip.w, 0.0035);
        vec2 ruv = rndc.xy * 0.5 + 0.5;
        float mapz = textureLod(sampler2D(heap[nonuniformEXT(cb.rain_map)], samp_clamp), ruv, 0.0).r;
        bool inside = ruv.x >= 0.0 && ruv.x <= 1.0 && ruv.y >= 0.0 && ruv.y <= 1.0;
        bool covered = inside && rndc.z > mapz + 0.0035;
        if (covered) w *= 0.12;
    }

    vec3 albedo = cb.material.rgb;
    float rough = cb.material.a;
    float metal = 0.0;
    float dark = clamp_range(w, 0.0, 0.35) * WET_POROSITY;
    albedo = mix(albedo, albedo * albedo, dark);

    float pool = clamp((N0.y - POOL_THR) / max(1.0 - POOL_THR, 1e-3), 0.0, 1.0) * w;
    // Standing water is a puddle on a floor, not a film on a boulder.
    float wet_r = mix(0.34, WET_ROUGHNESS, pool);
    rough = mix(rough, wet_r, clamp_range(w, 0.2, 1.0));
    vec2 pxz = v_world.xz;
    vec2 base = floor(pxz / max(cb.ripple.x, 1e-3));
    float acc = 0.0;
    acc += cell_ripple(base + vec2(0.0, 0.0), pxz, cb.rain.w);
    acc += cell_ripple(base + vec2(1.0, 0.0), pxz, cb.rain.w);
    acc += cell_ripple(base + vec2(0.0, 1.0), pxz, cb.rain.w);
    acc += cell_ripple(base + vec2(1.0, 1.0), pxz, cb.rain.w);
    float fade = clamp(1.0 - length(pxz - cb.camera_pos.xz) / 34.0, 0.0, 1.0);
    vec3 Nt = normalize(N0 + vec3(0.0, 1.0, 0.0) * (pool * 0.45));
    Nt = normalize(Nt + vec3(acc * cb.rain.z * pool * fade * 0.18, 0.0, acc * cb.rain.z * pool * fade * 0.18));

    vec3 V = normalize(cb.camera_pos.xyz - v_world);
    vec3 sun = normalize(cb.sun_dir.xyz);
    vec3 color = shade(Nt, V, sun, cb.sun_color.rgb, albedo, rough, metal);
    color += albedo * cb.sky_zenith.rgb * 0.22;

    uint n = min(cb.light_count, 32u);
    for (uint i = 0u; i < n; ++i) {
        Light lt = light_buf[0].lights[i];
        vec3 d = lt.position_radius.xyz - v_world;
        float dist = length(d);
        if (dist >= lt.position_radius.w) continue;
        vec3 l = d / max(dist, 1e-4);
        float cone = cone_falloff(l, lt.dir_cos_outer, lt.color.w);
        if (cone <= 0.0) continue;
        color += shade(Nt, V, l,
            lt.color.rgb * (attenuation(dist, lt.position_radius.w) * cone),
            albedo, rough, metal);
    }

    out_color = vec4(color, 1.0);
    out_depth = v_z;
}
