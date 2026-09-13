#version 450
#extension GL_EXT_nonuniform_qualifier : require

layout(set = 0, binding = 0, std140) uniform Storm {
    layout(offset = 0)   mat4 inv_view_proj;
    layout(offset = 192) vec4 camera_pos;
    layout(offset = 240) vec4 sky_zenith;
    layout(offset = 272) vec4 rain;    // x intensity, w time
    layout(offset = 288) vec4 streak;
    layout(offset = 320) vec4 wind;    // xy shear
    layout(offset = 464) vec2 inv_extent;
    layout(offset = 472) uint scene_color;
    layout(offset = 476) uint scene_depth;
    layout(offset = 480) uint rain_map;
    layout(offset = 512) mat4 rain_map_vp;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) out vec4 out_color;

float hash11(float x) {
    uint n = uint(int(floor(x))) * 374761393u;
    n = (n ^ (n >> 13)) * 1274126177u;
    n = n ^ (n >> 16);
    return float(n) * (1.0 / 4294967296.0);
}

float layer(vec2 uv, float ls, float lv, float lb, float lo, float t) {
    // Wind shears the columns: rain falls at an angle, which is what a
    // vertical curtain never does. Dagor's particles have wind; we keep
    // screen-space streaks (they cover the frame) and give them the same lean.
    vec2 s = vec2(uv.x + cb.wind.x * 0.22 * uv.y, uv.y);
    s *= ls;
    float colf = s.x * cb.streak.x;
    float col = floor(colf);
    float ch = hash11(col * 17.0 + lo * 9.0);
    float spd = 1.0 + ch * 0.5;
    float row = fract(s.y * cb.streak.y + t * cb.streak.z * lv * spd + ch + lo);
    float str = clamp((0.06 - row) / 0.06, 0.0, 1.0);
    str = str * str;
    if (ch <= 0.66) str = 0.0;
    float cx = abs(fract(colf) - 0.5);
    str *= clamp((0.5 - cx) / 0.5, 0.0, 1.0);
    return str * lb;
}

void main() {
    vec2 uv = gl_FragCoord.xy * cb.inv_extent;
    vec3 scene = texture(sampler2D(heap[nonuniformEXT(cb.scene_color)], samp_clamp), uv).rgb;
    float scenez = texture(sampler2D(heap[nonuniformEXT(cb.scene_depth)], samp_clamp), uv).r;

    float t = cb.rain.w;
    float acc = 0.0;
    acc += layer(uv, 1.0, 1.0, 1.0, 0.0, t);
    acc += layer(uv, 1.9, 1.45, 0.72, 0.37, t);
    acc += layer(uv, 3.3, 2.1, 0.45, 0.71, t);

    float exposure_sky = 1.0;
    if (cb.rain_map != 0u) {
        vec2 ndc = uv * 2.0 - 1.0;
        vec4 far4 = cb.inv_view_proj * vec4(ndc, 1.0, 1.0);
        vec3 far_pos = far4.xyz / max(far4.w, 0.0035);
        vec3 dir = normalize(far_pos - cb.camera_pos.xyz);
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

    float nearf = clamp(scenez / 3.0, 0.0, 1.0);
    float sw = acc * cb.streak.w * cb.rain.x * nearf * exposure_sky;
    vec3 total = scene + cb.sky_zenith.rgb * sw;
    out_color = vec4(total, 1.0);
}
