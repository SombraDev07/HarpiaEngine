#version 450
#extension GL_EXT_nonuniform_qualifier : require

layout(set = 0, binding = 0, std140) uniform Storm {
    layout(offset = 64)  mat4 view_proj;
    layout(offset = 192) vec4 camera_pos;
    layout(offset = 208) vec4 sun_dir;
    layout(offset = 224) vec4 sun_color;
    layout(offset = 240) vec4 sky_zenith;
    layout(offset = 256) vec4 sky_horizon;
    layout(offset = 272) vec4 rain;
    layout(offset = 304) vec4 ripple;
    layout(offset = 320) vec4 wind;
    layout(offset = 336) vec4 shallow;
    layout(offset = 352) vec4 deep;
    layout(offset = 432) vec4 misc;
    layout(offset = 448) vec4 ssr;
    layout(offset = 464) vec2 inv_extent;
    layout(offset = 472) uint scene_color;
    layout(offset = 476) uint scene_depth;
    layout(offset = 484) uint light_count;
} cb;

struct Light {
    vec4 position_radius;
    vec4 color;
    vec4 dir_cos_outer;
};
layout(set = 3, binding = 0, std430) readonly buffer Lights { Light lights[]; } light_buf[];

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) in vec3 v_world;
layout(location = 1) in vec3 v_nrm;
layout(location = 2) in float v_fold;
layout(location = 3) in float v_z;
layout(location = 0) out vec4 out_color;

const float PI = 3.14159265359;

float hash11(float x) {
    uint n = uint(int(floor(x))) * 374761393u;
    n = (n ^ (n >> 13)) * 1274126177u;
    n = n ^ (n >> 16);
    return float(n) * (1.0 / 4294967296.0);
}

float cell_ripple(vec2 cell, vec2 pxz, float t) {
    float h0 = hash11(cell.x * 13.0 + cell.y * 7.0);
    float h1 = hash11(cell.x * 5.0 + cell.y * 17.0 + 3.0);
    vec2 ctr = (cell + vec2(h0, h1)) * cb.ripple.x;
    float dist = length(pxz - ctr);
    float wave = sin(dist * cb.ripple.y - t * cb.ripple.z + h0 * 6.28318531);
    return wave * exp(-dist * cb.ripple.w);
}

vec3 analytic_sky(vec3 R, vec3 sun, vec3 sun_c, vec3 zenith, vec3 horiz) {
    float up = pow(clamp(R.y, 0.0, 1.0), 0.55);
    vec3 grad = mix(horiz, zenith, vec3(up));
    float sd = clamp(dot(R, sun), 0.0, 1.0);
    vec3 glow = sun_c * pow(sd, 900.0) * 1.1;
    vec3 disc = sd > 0.9997 ? sun_c : vec3(0.0);
    return grad + glow + disc;
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

void main() {
    vec3 N = normalize(v_nrm);
    // Rain hits the water: perturb the normal with the same cell rings the
    // ground uses, so the puddles and the surface speak the same language.
    vec2 pxz = v_world.xz;
    vec2 base = floor(pxz / max(cb.ripple.x, 1e-3));
    float acc = 0.0;
    acc += cell_ripple(base + vec2(0.0, 0.0), pxz, cb.rain.w);
    acc += cell_ripple(base + vec2(1.0, 0.0), pxz, cb.rain.w);
    acc += cell_ripple(base + vec2(0.0, 1.0), pxz, cb.rain.w);
    acc += cell_ripple(base + vec2(1.0, 1.0), pxz, cb.rain.w);
    float rain_n = acc * cb.rain.x * cb.rain.z * 0.12;
    N = normalize(N + vec3(rain_n, 0.0, rain_n * 0.7));

    vec3 cam = cb.camera_pos.xyz;
    vec3 V = normalize(cam - v_world);
    vec3 sun = normalize(cb.sun_dir.xyz);
    vec3 sun_c = cb.sun_color.rgb;
    vec3 zenith = cb.sky_zenith.rgb;
    vec3 horiz = cb.sky_horizon.rgb;
    float seabed = cb.sky_horizon.w;
    float ndv = max(dot(N, V), 0.02);
    float fresnel = 0.02 + 0.98 * pow(1.0 - ndv, 5.0);
    vec3 R = reflect(-V, N);

    float steps = max(cb.ssr.x, 1.0);
    float thick = cb.ssr.y;
    float maxdist = cb.ssr.z;
    float foamk = cb.ssr.w;
    float stride = maxdist / steps;
    uint nsteps = uint(steps);

    float hit = 0.0;
    vec2 huv = vec2(0.0);
    for (uint i = 0u; i < nsteps; ++i) {
        float t = (float(i) + 1.0) * stride;
        vec3 p = v_world + R * t;
        vec4 clip = cb.view_proj * vec4(p, 1.0);
        float cw = max(clip.w, 1e-4);
        vec2 ndc = clip.xy / cw;
        vec2 suv = ndc * 0.5 + 0.5;
        float scenez = textureLod(sampler2D(heap[nonuniformEXT(cb.scene_depth)], samp_clamp), suv, 0.0).r;
        float delta = cw - scenez;
        float window = max(thick, stride) + cw * 0.045;
        bool inside = suv.x >= 0.0 && suv.x <= 1.0 && suv.y >= 0.0 && suv.y <= 1.0 && cw > 1e-4;
        if (inside && delta > 0.0 && delta < window && hit < 0.5) {
            hit = 1.0;
            huv = suv;
        }
    }

    vec3 skyR = analytic_sky(R, sun, sun_c, zenith, horiz);
    vec3 ssrc = textureLod(sampler2D(heap[nonuniformEXT(cb.scene_color)], samp_clamp), huv, 0.0).rgb;
    float emax = max(abs(huv.x - 0.5), abs(huv.y - 0.5));
    float efade = clamp((0.5 - emax) / 0.12, 0.0, 1.0);
    vec3 refl = mix(skyR, ssrc, vec3(hit * efade));

    float path = seabed / ndv;
    float trans = exp(-path * cb.deep.w);
    vec3 bodyc = mix(cb.deep.rgb, cb.shallow.rgb, vec3(trans));

    vec3 H = normalize(V + sun);
    float spec = pow(clamp(dot(N, H), 0.0, 1.0), cb.misc.z) * fresnel;
    vec3 lit = mix(bodyc, refl, vec3(fresnel)) + sun_c * spec;

    // Lanterns on the water — the thing the water gate never had.
    uint n = min(cb.light_count, 32u);
    for (uint i = 0u; i < n; ++i) {
        Light lt = light_buf[0].lights[i];
        vec3 d = lt.position_radius.xyz - v_world;
        float dist = length(d);
        if (dist >= lt.position_radius.w) continue;
        vec3 l = d / max(dist, 1e-4);
        float cone = cone_falloff(l, lt.dir_cos_outer, lt.color.w);
        if (cone <= 0.0) continue;
        vec3 Hl = normalize(V + l);
        float sp = pow(clamp(dot(N, Hl), 0.0, 1.0), cb.misc.z);
        float att = attenuation(dist, lt.position_radius.w) * cone;
        lit += lt.color.rgb * (sp * fresnel * att);
        lit += bodyc * lt.color.rgb * (0.04 * att);
    }

    float foldf = clamp((1.0 - v_fold) * 3.2, 0.0, 1.0);
    vec2 ownuv = gl_FragCoord.xy * cb.inv_extent;
    float behind = textureLod(sampler2D(heap[nonuniformEXT(cb.scene_depth)], samp_clamp), ownuv, 0.0).r;
    float col = max(behind - v_z, 0.0);
    float shoref = pow(1.0 - clamp(col / 0.30, 0.0, 1.0), 2.0);
    float foam = clamp(max(foldf, shoref) * foamk, 0.0, 1.0);
    vec3 total = mix(lit, vec3(0.62), vec3(foam));
    out_color = vec4(total, 1.0);
}
