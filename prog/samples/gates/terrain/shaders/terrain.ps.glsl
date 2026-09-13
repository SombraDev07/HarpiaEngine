// Terreno: luz do sol, ambiente do céu por hemisfério, cor por declive, e o
// terreno a desvanecer para o céu ao longe.
//
// O desvanecimento não é decoração: o último nível do clipmap acaba numa borda
// dura, e sem ela dissolver no céu vê-se o mundo terminar. O céu que o terreno
// vê ao longe é a LUT de sky-view (Hillaire), a mesma que o pass do céu usa —
// um gradiente separado lia como dois mundos.

#version 450
#extension GL_EXT_nonuniform_qualifier : require

layout(set = 0, binding = 0, std140) uniform Terrain {
    layout(offset = 0)   mat4 view_proj;
    layout(offset = 64)  vec4 camera_pos;    // xyz, w = altitude km
    layout(offset = 80)  vec4 sun_dir;
    layout(offset = 96)  vec4 sun_color;
    layout(offset = 112) vec4 params;
    layout(offset = 128) vec4 sky_zenith;
    layout(offset = 144) vec4 sky_horizon;   // w = distância do fade
    layout(offset = 160) vec2 inv_extent;
    layout(offset = 176) uint skyview;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) in vec3 v_world;
layout(location = 1) in vec3 v_normal;
layout(location = 2) in float v_view_dist;

layout(location = 0) out vec4 out_color;

// Must match `harpia_render::atmosphere::BOTTOM_RADIUS_KM`.
const float BOTTOM_RADIUS_KM = 6360.0;
const float PI = 3.14159265;

// Same parameterisation as `sky_hdr.ps.glsl` / `sky.ps.spvasm`. Keep in sync.
vec3 sample_skyview(vec3 dir) {
    dir = normalize(dir);
    vec3 sun = normalize(cb.sun_dir.xyz);
    float r0 = BOTTOM_RADIUS_KM + cb.camera_pos.w;
    float r0sq = r0 * r0;
    float botsq = BOTTOM_RADIUS_KM * BOTTOM_RADIUS_KM;
    float slen = sqrt(sun.x * sun.x + sun.z * sun.z);
    float dlen = sqrt(dir.x * dir.x + dir.z * dir.z);
    float lvcos = clamp((sun.x * dir.x + sun.z * dir.z) / max(slen * dlen, 1e-6), -1.0, 1.0);
    float vhor = sqrt(max(r0sq - botsq, 0.0));
    float beta = acos(clamp(vhor / r0, -1.0, 1.0));
    float zenhor = PI - beta;
    float zenith = acos(clamp(dir.y, -1.0, 1.0));
    float lutv;
    if (zenith < zenhor) {
        float sc = sqrt(max(1.0 - zenith / max(zenhor, 1e-6), 0.0));
        lutv = (1.0 - sc) * 0.5;
    } else {
        lutv = 0.499;
    }
    float lutu = sqrt(max((-lvcos) * 0.5 + 0.5, 0.0));
    return textureLod(
        sampler2D(heap[nonuniformEXT(cb.skyview)], samp_clamp),
        vec2(lutu, clamp(lutv, 0.0, 1.0)),
        0.0
    ).rgb;
}

void main() {
    vec3 n = normalize(v_normal);
    vec3 sun = normalize(cb.sun_dir.xyz);

    // Rocha onde é íngreme, erva onde é plano. O declive é a única informação de
    // material que um terreno procedural tem de graça.
    float slope = clamp(1.0 - n.y, 0.0, 1.0);
    vec3 grass = vec3(0.16, 0.26, 0.11);
    vec3 rock  = vec3(0.30, 0.28, 0.25);
    vec3 albedo = mix(grass, rock, smoothstep(0.15, 0.45, slope));
    // Neve nos cumes, e só onde não é vertical: não pega em paredes.
    float snow = smoothstep(34.0, 50.0, v_world.y) * (1.0 - smoothstep(0.25, 0.5, slope));
    albedo = mix(albedo, vec3(0.86, 0.86, 0.83), snow);

    float ndl = max(dot(n, sun), 0.0);
    vec3 direct = cb.sun_color.rgb * (ndl / 3.14159265);

    vec3 ambient;
    vec3 distant;
    if (cb.skyview != 0u) {
        // Hemisfério: a normal aponta para o céu que ilumina esta face.
        vec3 hemi = n.y >= 0.0 ? n : vec3(n.x, abs(n.y), n.z);
        ambient = sample_skyview(hemi) * 0.12;
        vec3 view = normalize(v_world - cb.camera_pos.xyz);
        distant = sample_skyview(view);
    } else {
        ambient = mix(cb.sky_horizon.rgb, cb.sky_zenith.rgb, n.y * 0.5 + 0.5) * 0.5;
        distant = mix(cb.sky_horizon.rgb, cb.sky_zenith.rgb, 0.35);
    }
    vec3 lit = albedo * (direct + ambient);

    // Só desvanece perto do fim do clipmap. Linear desde zero fazia o vale a 600
    // unidades ficar meio céu, o que lia como terreno em falta.
    float far = max(cb.sky_horizon.w, 1.0);
    float fade = smoothstep(far * 0.55, far, v_view_dist);
    out_color = vec4(mix(lit, distant, fade), 1.0);
}
