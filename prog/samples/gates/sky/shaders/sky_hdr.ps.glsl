#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Sky composite in linear HDR. Same LUT mapping as `sky.ps.spvasm`, without
// ACES/sRGB: the terrain blit is the one that tonemaps.
//
// One fetch per pixel. The sun disc is added on top of the LUT, only above the
// horizon, matching the assembly.

layout(set = 0, binding = 0, std140) uniform Atmos {
    layout(offset = 0)   mat4 inv_view_proj;
    layout(offset = 64)  vec4 camera_pos;       // xyz world, w = altitude km
    layout(offset = 80)  vec4 sun_dir;
    layout(offset = 96)  vec4 sun_illuminance;
    layout(offset = 112) vec4 radii;            // bottom km, top km, mie g, ms
    layout(offset = 216) uint skyview_lut;
    layout(offset = 224) vec2 inv_extent;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(location = 0) out vec4 out_color;

const float PI = 3.14159265;
const float SUN_COS = 0.99995;

void main() {
    vec2 uv = gl_FragCoord.xy * cb.inv_extent;
    vec2 ndc = uv * 2.0 - 1.0;
    vec4 farw = cb.inv_view_proj * vec4(ndc, 1.0, 1.0);
    vec3 farpos = farw.xyz / max(farw.w, 1e-6);
    vec3 dir = normalize(farpos - cb.camera_pos.xyz);
    vec3 sun = normalize(cb.sun_dir.xyz);

    float bottom = cb.radii.x;
    float r0 = bottom + cb.camera_pos.w;
    float r0sq = r0 * r0;
    float botsq = bottom * bottom;

    float vzcos = clamp(dir.y, -1.0, 1.0);
    float slen = sqrt(sun.x * sun.x + sun.z * sun.z);
    float dlen = sqrt(dir.x * dir.x + dir.z * dir.z);
    float lvcos = clamp((sun.x * dir.x + sun.z * dir.z) / max(slen * dlen, 1e-6), -1.0, 1.0);

    float vhor = sqrt(max(r0sq - botsq, 0.0));
    float beta = acos(clamp(vhor / r0, -1.0, 1.0));
    float zenhor = PI - beta;
    float zenith = acos(vzcos);

    float lutv;
    if (zenith < zenhor) {
        float sc = sqrt(max(1.0 - zenith / max(zenhor, 1e-6), 0.0));
        lutv = (1.0 - sc) * 0.5;
    } else {
        // Below the planet horizon the LUT stores an unlit ground (albedo 0).
        // This gate has terrain; empty pixels past the clipmap should keep the
        // atmospheric horizon, not a black planet.
        lutv = 0.499;
    }
    float lutu = sqrt(max((-lvcos) * 0.5 + 0.5, 0.0));

    vec3 lum = textureLod(
        sampler2D(heap[nonuniformEXT(cb.skyview_lut)], samp_clamp),
        vec2(lutu, clamp(lutv, 0.0, 1.0)),
        0.0
    ).rgb;

    vec3 disc = vec3(0.0);
    if (dot(dir, sun) > SUN_COS && dir.y > 0.0) {
        disc = lum * cb.sun_illuminance.rgb;
    }
    out_color = vec4(lum + disc, 1.0);
}
