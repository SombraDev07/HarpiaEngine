#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Tucano/Cry rain post, without the 24k particles that GPUVM'd RADV.
// Streaks live on three view-space planes (not UV-painted onto surfaces).
// Rainfall + rainfall_n are the Tucano maps; the gaussian layer is the same
// contract as Rain.hlsl rainStreakLayerVS. Lighting is HG backscatter, not
// `scene + sky * hash`.

layout(set = 0, binding = 0, std140) uniform Storm {
    layout(offset = 0)   mat4 inv_view_proj;
    layout(offset = 192) vec4 camera_pos;
    layout(offset = 208) vec4 sun_dir;
    layout(offset = 224) vec4 sun_color;
    layout(offset = 240) vec4 sky_zenith;
    layout(offset = 272) vec4 rain;    // x intensity, w time
    layout(offset = 288) vec4 streak;
    layout(offset = 320) vec4 wind;
    layout(offset = 464) vec2 inv_extent;
    layout(offset = 472) uint scene_color;
    layout(offset = 476) uint scene_depth;
    layout(offset = 480) uint rain_map;
    layout(offset = 512) mat4 rain_map_vp;
    layout(offset = 576) uvec4 rain_tex0; // rainfall, rainfall_n, _, _
    layout(offset = 608) vec4 rain_view;  // fov_tan, aspect
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 0) uniform sampler samp_wrap;
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) out vec4 out_color;

float hash11(float x) {
    uint n = uint(int(floor(x))) * 374761393u;
    n = (n ^ (n >> 13)) * 1274126177u;
    n = n ^ (n >> 16);
    return float(n) * (1.0 / 4294967296.0);
}

float hash21(vec2 p) {
    return hash11(p.x * 13.0 + p.y * 47.0 + 19.19);
}

vec2 unpack_xy(vec4 n) {
    return n.xy * 2.0 - 1.0;
}

vec3 rain_plane_view(vec2 uv, float plane_z, float aspect, float fov_tan) {
    vec2 ndc = uv * 2.0 - 1.0;
    ndc.y = -ndc.y;
    return vec3(ndc.x * aspect * fov_tan * plane_z, ndc.y * fov_tan * plane_z, plane_z);
}

float textured_layer(vec3 view_p, float t, float layer, vec2 wind) {
    uint rainfall = cb.rain_tex0.x;
    if (rainfall == 0u) {
        return 0.0;
    }
    float scale = mix(0.45, 0.95, layer * 0.4);
    float speed = (1.15 + layer * 0.5) * max(cb.streak.z, 0.15);
    vec2 tc;
    tc.x = view_p.x * scale + layer * 7.3 + wind.x * t * 0.25;
    tc.y = -view_p.y * scale * 1.2 - t * speed;
    tc.x += (-view_p.y) * wind.x * 0.08;
    float rain = texture(sampler2D(heap[nonuniformEXT(rainfall)], samp_wrap), tc * 0.28).r;
    float rain2 = texture(sampler2D(heap[nonuniformEXT(rainfall)], samp_wrap), tc * 0.48 + 0.27).r;
    float a = clamp(rain * 0.65 + rain2 * 0.4, 0.0, 1.0);
    uint rainfall_n = cb.rain_tex0.y;
    if (rainfall_n != 0u) {
        vec2 nxy = unpack_xy(texture(sampler2D(heap[nonuniformEXT(rainfall_n)], samp_wrap), tc * 0.28));
        a *= mix(0.45, 1.2, clamp(length(nxy), 0.0, 1.0));
    }
    a *= mix(1.0, 0.4, layer / 3.0);
    return a;
}

float streak_layer(vec3 view_p, float t, float layer, vec2 wind) {
    float density = 2.2 + layer * 1.3;
    float length_scale = 1.6 + layer * 0.55;
    float speed = (1.5 + layer * 0.55) * max(cb.streak.z, 0.2);
    vec2 slant = normalize(vec2(wind.x * 0.35, 1.0));
    vec2 p;
    p.x = view_p.x * density + layer * 5.1;
    p.y = (-view_p.y) * length_scale - t * speed;
    p.x += p.y * slant.x * 0.2 + wind.x * t * 0.1;
    vec2 cell = floor(p);
    vec2 f = fract(p);
    float rnd = hash21(cell + vec2(layer * 19.0));
    if (rnd < 0.55) {
        return 0.0;
    }
    float cx = rnd * 0.6 + 0.2;
    float half_w = mix(0.012, 0.032, hash21(cell.yx));
    float streak = exp(-pow((f.x - cx) / max(half_w, 1e-4), 2.0));
    float head = smoothstep(0.0, 0.2, f.y) * smoothstep(1.0, 0.5, f.y);
    streak *= head * mix(0.5, 1.3, pow(smoothstep(0.15, 0.5, f.y), 2.0));
    return streak * rnd;
}

vec3 streak_light(vec3 V) {
    vec3 sun_l = normalize(cb.sun_dir.xyz);
    float cos_th = dot(V, sun_l);
    float g = 0.65;
    float g2 = g * g;
    float ph = (1.0 - g2) / (12.566 * pow(abs(1.0 + g2 - 2.0 * g * cos_th), 1.5));
    return cb.sky_zenith.rgb * vec3(0.82, 0.9, 1.05) * 0.3 + cb.sun_color.rgb * ph;
}

void main() {
    vec2 uv = gl_FragCoord.xy * cb.inv_extent;
    vec3 scene = texture(sampler2D(heap[nonuniformEXT(cb.scene_color)], samp_clamp), uv).rgb;
    float scenez = texture(sampler2D(heap[nonuniformEXT(cb.scene_depth)], samp_clamp), uv).r;

    vec2 ndc = uv * 2.0 - 1.0;
    vec4 far4 = cb.inv_view_proj * vec4(ndc, 1.0, 1.0);
    vec3 far_pos = far4.xyz / max(far4.w, 1e-4);
    vec3 V = normalize(far_pos - cb.camera_pos.xyz);

    float exposure_sky = 1.0;
    if (cb.rain_map != 0u) {
        vec3 dir = V;
        vec4 ctr4 = cb.inv_view_proj * vec4(0.0, 0.0, 1.0, 1.0);
        vec3 ctr = ctr4.xyz / max(ctr4.w, 0.0035);
        vec3 fwd = normalize(ctr - cb.camera_pos.xyz);
        float tdist = scenez / max(dot(dir, fwd), 0.0035);
        vec3 wpos = cb.camera_pos.xyz + dir * tdist;
        vec4 rclip = cb.rain_map_vp * vec4(wpos, 1.0);
        vec3 rndc = rclip.xyz / max(rclip.w, 0.0035);
        vec2 ruv = rndc.xy * 0.5 + 0.5;
        float mapz = textureLod(sampler2D(heap[nonuniformEXT(cb.rain_map)], samp_clamp), ruv, 0.0).r;
        bool inside = ruv.x >= 0.0 && ruv.x <= 1.0 && ruv.y >= 0.0 && ruv.y <= 1.0;
        bool open = !inside || rndc.z <= mapz + 0.0035;
        exposure_sky = open ? 1.0 : 0.0;
    }

    float t = cb.rain.w;
    vec2 wind = cb.wind.xy;
    float aspect = max(cb.rain_view.y, 1e-3);
    float fov_tan = max(cb.rain_view.x, 1e-3);
    float inten = cb.streak.w * cb.rain.x * exposure_sky;

    float streaks = 0.0;
    float tex_rain = 0.0;
    for (int i = 0; i < 3; ++i) {
        float w = (i == 0) ? 1.0 : (i == 1 ? 0.62 : 0.38);
        float plane_z = mix(4.0, 28.0, float(i) / 2.0);
        vec3 view_p = rain_plane_view(uv, plane_z, aspect, fov_tan);
        float soft = smoothstep(0.0, 1.0, clamp((scenez - plane_z) * 0.35, 0.0, 1.0));
        float layer = float(i);
        streaks += streak_layer(view_p, t, layer, wind) * w * soft;
        tex_rain += textured_layer(view_p, t, layer, wind) * w * soft;
    }

    float sky_boost = scenez > 1.0e6 ? 1.25 : 1.0;
    streaks = clamp(streaks * inten * 1.05 * sky_boost, 0.0, 1.0);
    tex_rain = clamp(tex_rain * inten * 1.1 * sky_boost, 0.0, 1.0);

    vec3 col = streak_light(V);
    vec3 total = scene;
    total += col * tex_rain * 0.45;
    total += col * streaks * 0.85;
    total += col * streaks * streaks * 0.3;

    float mist = cb.rain.x * 0.10;
    if (scenez < 1.0e6) {
        mist *= clamp(scenez / 80.0, 0.0, 1.0);
    }
    mist *= exposure_sky;
    vec3 tint = cb.sky_zenith.rgb * vec3(0.85, 0.92, 1.05) * 0.14 + col * 0.12;
    total = mix(total, tint, clamp(mist * 0.22, 0.0, 0.35));

    out_color = vec4(max(total, vec3(0.0)), 1.0);
}
