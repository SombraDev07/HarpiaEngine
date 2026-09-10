// Terreno: luz do sol, ambiente do céu por hemisfério, cor por declive, e o
// terreno a desvanecer para o céu ao longe.
//
// O desvanecimento não é decoração: o último nível do clipmap acaba numa borda
// dura, e sem ela dissolver no céu vê-se o mundo terminar.

#version 450

layout(set = 0, binding = 0, std140) uniform Terrain {
    layout(offset = 0)   mat4 view_proj;
    layout(offset = 64)  vec4 camera_pos;
    layout(offset = 80)  vec4 sun_dir;
    layout(offset = 96)  vec4 sun_color;
    layout(offset = 112) vec4 params;
    layout(offset = 128) vec4 sky_zenith;
    layout(offset = 144) vec4 sky_horizon;   // w = distância do fade
    layout(offset = 160) vec2 inv_extent;
} cb;

layout(location = 0) in vec3 v_world;
layout(location = 1) in vec3 v_normal;
layout(location = 2) in float v_view_dist;

layout(location = 0) out vec4 out_color;

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
    vec3 ambient = mix(cb.sky_horizon.rgb, cb.sky_zenith.rgb, n.y * 0.5 + 0.5) * 0.5;
    vec3 lit = albedo * (direct + ambient);

    // O céu que o terreno vê ao longe é o mesmo gradiente que o pass do céu usa.
    // Só desvanece perto do fim do clipmap. Linear desde zero fazia o vale a 600
    // unidades ficar meio céu, o que lia como terreno em falta.
    float far = max(cb.sky_horizon.w, 1.0);
    float fade = smoothstep(far * 0.55, far, v_view_dist);
    vec3 distant = mix(cb.sky_horizon.rgb, cb.sky_zenith.rgb, 0.35);
    out_color = vec4(mix(lit, distant, fade), 1.0);
}
