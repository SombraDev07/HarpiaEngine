#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Froxel inject. One thread per froxel: extinction from height fog, in-scattering
// from the sun through a Henyey-Greenstein phase. Writes set 4 binding 1 slot 0.
//
// Translated from inject.cs.spvasm. The CSM tap and the HG flip are the same
// operations; a new field at the end (cloud_shadow) is skipped when the index
// is 0, so a capture without a map is the same picture as the assembly.

layout(local_size_x = 8, local_size_y = 8, local_size_z = 4) in;

layout(set = 4, binding = 1, rgba16f) uniform writeonly image3D uav[];

layout(set = 0, binding = 0, std140) uniform Fog {
    layout(offset = 0)   mat4 inv_view;
    layout(offset = 64)  vec4 camera_pos;
    layout(offset = 80)  vec4 sun_dir;
    layout(offset = 96)  vec4 sun_color;
    layout(offset = 112) vec4 fog;       // density, height falloff, g, base height
    layout(offset = 128) vec4 froxel;    // near, far, tan(fovY/2), aspect
    layout(offset = 144) vec4 misc;      // jitter, slices, exposure, cloud base Y
    layout(offset = 176) uint shadow_idx;
    layout(offset = 180) uint cascade_count;
    layout(offset = 184) float atlas_size;
    layout(offset = 188) float shadow_strength;
    layout(offset = 192) vec4 splits;
    layout(offset = 208) mat4 cascade0;
    layout(offset = 272) mat4 cascade1;
    layout(offset = 336) mat4 cascade2;
    layout(offset = 400) mat4 cascade3;
    layout(offset = 464) uint cloud_shadow;
    layout(offset = 468) float cloud_extent;
    layout(offset = 472) vec2 cloud_origin;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;
layout(set = 2, binding = 2) uniform samplerShadow samp_cmp;

const float PI4 = 12.5663706;
const float SHADOW_BIAS = 0.0015;

void main() {
    uvec3 id = gl_GlobalInvocationID;
    if (id.y >= 90u) return;

    float uvx = (float(id.x) + 0.5) / 160.0;
    float uvy = (float(id.y) + 0.5) / 90.0;
    float tz = (float(id.z) + 0.5 + cb.misc.x) / cb.misc.y;
    float near = cb.froxel.x;
    float far = cb.froxel.y;
    float d = near * pow(far / near, tz);

    float ndx = uvx * 2.0 - 1.0;
    float ndy = uvy * 2.0 - 1.0;
    float xv = ndx * cb.froxel.z * cb.froxel.w * d;
    float yv = -(ndy * cb.froxel.z * d);
    float zv = -d;
    vec4 wpos4 = cb.inv_view * vec4(xv, yv, zv, 1.0);
    vec3 wpos = wpos4.xyz;

    float shadow = 1.0;
    if (cb.cascade_count > 0u) {
        uint cidx = d < cb.splits.x ? 0u : d < cb.splits.y ? 1u : d < cb.splits.z ? 2u : 3u;
        mat4 cm = cidx == 0u ? cb.cascade0
                : cidx == 1u ? cb.cascade1
                : cidx == 2u ? cb.cascade2
                : cb.cascade3;
        vec4 clip = cm * wpos4;
        float invcw = 1.0 / max(clip.w, 1e-4);
        vec3 snd = clip.xyz * invcw;
        float tx = float(cidx % 2u) * 0.5;
        float ty = float(cidx / 2u) * 0.5;
        float texel = 1.0 / max(cb.atlas_size, 1.0);
        vec2 suv = clamp(
            vec2(snd.x * 0.5 + 0.5, snd.y * 0.5 + 0.5) * 0.5 + vec2(tx, ty),
            vec2(tx, ty),
            vec2(tx + 0.5 - texel, ty + 0.5 - texel)
        );
        float zref = snd.z - SHADOW_BIAS;
        float lit0 = textureLod(
            sampler2DShadow(heap[nonuniformEXT(cb.shadow_idx)], samp_cmp),
            vec3(suv, zref),
            0.0
        );
        float lit = d < cb.splits.w ? lit0 : 1.0;
        shadow = 1.0 - (1.0 - lit) * cb.shadow_strength;
    }

    // Cloud shadow: transmittance at the XZ where this froxel's sun ray hits
    // the layer base. Slot 0 is the dummy, so a missing map is a no-op.
    if (cb.cloud_shadow != 0u) {
        vec3 sun = normalize(cb.sun_dir.xyz);
        if (abs(sun.y) > 1e-4) {
            float t = (cb.misc.w - wpos.y) / sun.y;
            vec2 hit = wpos.xz + sun.xz * t;
            vec2 uv = (hit - cb.cloud_origin) / max(cb.cloud_extent, 1e-4) + 0.5;
            float tr = 1.0;
            if (uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0) {
                tr = textureLod(
                    sampler2D(heap[nonuniformEXT(cb.cloud_shadow)], samp_clamp),
                    uv,
                    0.0
                ).r;
            }
            shadow *= tr;
        }
    }

    float dens = cb.fog.x;
    float hfall = cb.fog.y;
    float g = cb.fog.z;
    float sigma = dens * exp(-max(wpos.y - cb.fog.w, 0.0) * hfall);

    vec3 vd = normalize(cb.camera_pos.xyz - wpos);
    vec3 sunn = normalize(cb.sun_dir.xyz);
    // HG angle is light travel (sun → froxel) vs scatter (froxel → eye).
    // Both vectors here point away from the froxel, so the cosine flips.
    float cosg = -dot(vd, sunn);
    float g2 = g * g;
    float hd = max(1.0 + g2 - 2.0 * g * cosg, 1e-4);
    float phase = (1.0 - g2) / (PI4 * pow(hd, 1.5));

    vec3 insc = cb.sun_color.rgb * (phase * sigma * shadow);
    imageStore(uav[0], ivec3(id), vec4(insc, sigma));
}
