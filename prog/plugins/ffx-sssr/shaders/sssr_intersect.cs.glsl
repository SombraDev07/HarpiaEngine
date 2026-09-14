#version 450
#extension GL_GOOGLE_include_directive : enable
#extension GL_EXT_nonuniform_qualifier : require
#extension GL_EXT_samplerless_texture_functions : require

// FidelityFX SSSR hierarchical march, one ray per pixel (no classify/indirect).
// Depth is a min Hi-Z built by SPD. Vulkan: 0=near, 1=far.
// UAV slot 15 is the reflection image; slots 0–N are the Hi-Z mips. The last
// CPU bind of a slot is what every dispatch in the command buffer sees.

layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;

layout(set = 0, binding = 0, std140) uniform Sssr {
    layout(offset = 0)   mat4 inv_view_proj;
    layout(offset = 64)  mat4 inv_proj;
    layout(offset = 128) mat4 view;
    layout(offset = 192) mat4 proj;
    layout(offset = 256) vec4 screen; // xy size, zw 1/size
    layout(offset = 272) uint depth;
    layout(offset = 276) uint normal;
    layout(offset = 280) uint color;
    layout(offset = 284) uint max_steps;
    layout(offset = 288) float thickness;
} cb;

layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;
layout(set = 4, binding = 0, rgba16f) uniform image2D uav[];

vec3 InvProjectPosition(vec3 coord, mat4 mat) {
    // Vulkan NDC Y already matches framebuffer UV. Do not flip like the D3D sample.
    vec4 clip = vec4(coord.xy * 2.0 - 1.0, coord.z, 1.0);
    vec4 v = mat * clip;
    return v.xyz / v.w;
}

vec3 ProjectPosition(vec3 origin, mat4 mat) {
    vec4 clip = mat * vec4(origin, 1.0);
    vec3 ndc = clip.xyz / clip.w;
    return vec3(ndc.xy * 0.5 + 0.5, ndc.z);
}

vec3 ProjectDirection(vec3 origin, vec3 direction, vec3 screen_space_origin, mat4 mat) {
    return ProjectPosition(origin + direction, mat) - screen_space_origin;
}

float FFX_SSSR_LoadDepth(ivec2 pixel_coordinate, int mip) {
    ivec2 size = textureSize(heap[nonuniformEXT(cb.depth)], mip);
    ivec2 p = clamp(pixel_coordinate, ivec2(0), size - 1);
    return texelFetch(sampler2D(heap[nonuniformEXT(cb.depth)], samp_clamp), p, mip).r;
}

vec3 FFX_SSSR_LoadWorldSpaceNormal(ivec2 pixel_coordinate) {
    vec3 n = texelFetch(sampler2D(heap[nonuniformEXT(cb.normal)], samp_clamp), pixel_coordinate, 0).xyz;
    return normalize(n * 2.0 - 1.0);
}

vec3 FFX_SSSR_ScreenSpaceToViewSpace(vec3 screen_space_position) {
    return InvProjectPosition(screen_space_position, cb.inv_proj);
}

#include "ffx_sssr.glsl.h"

void main() {
    ivec2 coords = ivec2(gl_GlobalInvocationID.xy);
    ivec2 screen = ivec2(cb.screen.xy);
    if (any(greaterThanEqual(coords, screen))) {
        return;
    }

    vec2 uv = (vec2(coords) + 0.5) * cb.screen.zw;
    float z = FFX_SSSR_LoadDepth(coords, 0);
    vec3 world_n = FFX_SSSR_LoadWorldSpaceNormal(coords);
    if (z >= 1.0) {
    imageStore(uav[15], coords, vec4(0.0));
        return;
    }

    vec3 origin = vec3(uv, z);
    vec3 view_pos = InvProjectPosition(origin, cb.inv_proj);
    vec3 view_dir = normalize(view_pos);
    vec3 view_n = normalize(mat3(cb.view) * world_n);
    vec3 view_refl = reflect(view_dir, view_n);
    vec3 ss_dir = ProjectDirection(view_pos, view_refl, origin, cb.proj);

    bool valid_hit = false;
    vec3 hit = FFX_SSSR_HierarchicalRaymarch(
        origin,
        ss_dir,
        true,
        cb.screen.xy,
        0,
        0u,
        max(cb.max_steps, 1u),
        valid_hit
    );

    vec3 world_origin = InvProjectPosition(origin, cb.inv_view_proj);
    vec3 world_hit = InvProjectPosition(hit, cb.inv_view_proj);
    vec3 world_ray = world_hit - world_origin;
    float confidence = 0.0;
    if (valid_hit && all(greaterThanEqual(hit.xy, vec2(0.0))) && all(lessThanEqual(hit.xy, vec2(1.0)))) {
        confidence = FFX_SSSR_ValidateHit(hit, uv, world_ray, cb.screen.xy, cb.thickness);
        // A first hit inside the screen is enough for the gate: ValidateHit is
        // strict about thickness and backfaces, and a zero buffer is silent.
        if (confidence <= 0.0 && abs(hit.z) < 1.0) {
            vec2 manhattan = abs(hit.xy - uv);
            if (any(greaterThanEqual(manhattan, 2.0 / cb.screen.xy))) {
                confidence = 0.35;
            }
        }
    }

    vec3 radiance = vec3(0.0);
    if (confidence > 0.0) {
        ivec2 hit_px = clamp(ivec2(cb.screen.xy * hit.xy), ivec2(0), screen - 1);
        radiance = texelFetch(sampler2D(heap[nonuniformEXT(cb.color)], samp_clamp), hit_px, 0).rgb;
    }

    imageStore(uav[15], coords, vec4(radiance * confidence, confidence));
}
