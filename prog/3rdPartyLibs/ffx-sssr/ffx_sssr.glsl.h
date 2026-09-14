/**********************************************************************
Copyright (c) 2021 Advanced Micro Devices, Inc. All rights reserved.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:
The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.
THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.  IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
********************************************************************/

// Mechanical GLSL of ffx_sssr.h (same functions, HLSL types/intrinsics spelled
// out). Keep in lockstep with the .h next to this file.
//
// WaveActiveCountBits is occupancy early-out. Without subgroup ballot we treat
// the wave as full so the march never exits early — same hits, more work.
// Vulkan depth is 0=near, 1=far: do **not** define FFX_SSSR_INVERTED_DEPTH_RANGE.

#ifndef FFX_SSSR
#define FFX_SSSR
#define FFX_SSSR_FLOAT_MAX 3.402823466e+38

#ifndef WaveActiveCountBits
#define WaveActiveCountBits(x) 64u
#endif

uint ffx_sssr_asuint(float x) {
    return floatBitsToUint(x);
}

void FFX_SSSR_InitialAdvanceRay(
    vec3 origin,
    vec3 direction,
    vec3 inv_direction,
    vec2 current_mip_resolution,
    vec2 current_mip_resolution_inv,
    vec2 floor_offset,
    vec2 uv_offset,
    out vec3 position,
    out float current_t
) {
    vec2 current_mip_position = current_mip_resolution * origin.xy;
    vec2 xy_plane = floor(current_mip_position) + floor_offset;
    xy_plane = xy_plane * current_mip_resolution_inv + uv_offset;
    vec2 t = xy_plane * inv_direction.xy - origin.xy * inv_direction.xy;
    current_t = min(t.x, t.y);
    position = origin + current_t * direction;
}

bool FFX_SSSR_AdvanceRay(
    vec3 origin,
    vec3 direction,
    vec3 inv_direction,
    vec2 current_mip_position,
    vec2 current_mip_resolution_inv,
    vec2 floor_offset,
    vec2 uv_offset,
    float surface_z,
    inout vec3 position,
    inout float current_t
) {
    vec2 xy_plane = floor(current_mip_position) + floor_offset;
    xy_plane = xy_plane * current_mip_resolution_inv + uv_offset;
    vec3 boundary_planes = vec3(xy_plane, surface_z);
    vec3 t = boundary_planes * inv_direction - origin * inv_direction;

#ifdef FFX_SSSR_INVERTED_DEPTH_RANGE
    t.z = direction.z < 0.0 ? t.z : FFX_SSSR_FLOAT_MAX;
#else
    t.z = direction.z > 0.0 ? t.z : FFX_SSSR_FLOAT_MAX;
#endif

    float t_min = min(min(t.x, t.y), t.z);

#ifdef FFX_SSSR_INVERTED_DEPTH_RANGE
    bool above_surface = surface_z < position.z;
#else
    bool above_surface = surface_z > position.z;
#endif

    bool skipped_tile = ffx_sssr_asuint(t_min) != ffx_sssr_asuint(t.z) && above_surface;
    current_t = above_surface ? t_min : current_t;
    position = origin + current_t * direction;
    return skipped_tile;
}

vec2 FFX_SSSR_GetMipResolution(vec2 screen_dimensions, int mip_level) {
    return screen_dimensions * pow(0.5, float(mip_level));
}

vec3 FFX_SSSR_HierarchicalRaymarch(
    vec3 origin,
    vec3 direction,
    bool is_mirror,
    vec2 screen_size,
    int most_detailed_mip,
    uint min_traversal_occupancy,
    uint max_traversal_intersections,
    out bool valid_hit
) {
    vec3 inv_direction = vec3(
        direction.x != 0.0 ? 1.0 / direction.x : FFX_SSSR_FLOAT_MAX,
        direction.y != 0.0 ? 1.0 / direction.y : FFX_SSSR_FLOAT_MAX,
        direction.z != 0.0 ? 1.0 / direction.z : FFX_SSSR_FLOAT_MAX
    );

    int current_mip = most_detailed_mip;
    vec2 current_mip_resolution = FFX_SSSR_GetMipResolution(screen_size, current_mip);
    vec2 current_mip_resolution_inv = 1.0 / current_mip_resolution;

    vec2 uv_offset = vec2(0.005 * exp2(float(most_detailed_mip))) / screen_size;
    uv_offset = mix(uv_offset, -uv_offset, lessThan(direction.xy, vec2(0.0)));
    vec2 floor_offset = mix(vec2(1.0), vec2(0.0), lessThan(direction.xy, vec2(0.0)));

    float current_t;
    vec3 position;
    FFX_SSSR_InitialAdvanceRay(
        origin,
        direction,
        inv_direction,
        current_mip_resolution,
        current_mip_resolution_inv,
        floor_offset,
        uv_offset,
        position,
        current_t
    );

    bool exit_due_to_low_occupancy = false;
    int i = 0;
    while (i < int(max_traversal_intersections) && current_mip >= most_detailed_mip && !exit_due_to_low_occupancy) {
        vec2 current_mip_position = current_mip_resolution * position.xy;
        float surface_z = FFX_SSSR_LoadDepth(ivec2(current_mip_position), current_mip);
        exit_due_to_low_occupancy = !is_mirror && WaveActiveCountBits(true) <= min_traversal_occupancy;
        bool skipped_tile = FFX_SSSR_AdvanceRay(
            origin,
            direction,
            inv_direction,
            current_mip_position,
            current_mip_resolution_inv,
            floor_offset,
            uv_offset,
            surface_z,
            position,
            current_t
        );
        current_mip += skipped_tile ? 1 : -1;
        current_mip_resolution *= skipped_tile ? 0.5 : 2.0;
        current_mip_resolution_inv *= skipped_tile ? 2.0 : 0.5;
        ++i;
    }

    valid_hit = (i <= int(max_traversal_intersections));
    return position;
}

float FFX_SSSR_ValidateHit(
    vec3 hit,
    vec2 uv,
    vec3 world_space_ray_direction,
    vec2 screen_size,
    float depth_buffer_thickness
) {
    if (any(lessThan(hit.xy, vec2(0.0))) || any(greaterThan(hit.xy, vec2(1.0)))) {
        return 0.0;
    }

    vec2 manhattan_dist = abs(hit.xy - uv);
    if (all(lessThan(manhattan_dist, vec2(2.0) / screen_size))) {
        return 0.0;
    }

    ivec2 texel_coords = ivec2(screen_size * hit.xy);
    float surface_z = FFX_SSSR_LoadDepth(texel_coords / 2, 1);
#ifdef FFX_SSSR_INVERTED_DEPTH_RANGE
    if (surface_z == 0.0) {
#else
    if (surface_z == 1.0) {
#endif
        return 0.0;
    }

    vec3 hit_normal = FFX_SSSR_LoadWorldSpaceNormal(texel_coords);
    if (dot(hit_normal, world_space_ray_direction) > 0.0) {
        return 0.0;
    }

    vec3 view_space_surface = FFX_SSSR_ScreenSpaceToViewSpace(vec3(hit.xy, surface_z));
    vec3 view_space_hit = FFX_SSSR_ScreenSpaceToViewSpace(hit);
    float hit_distance = length(view_space_surface - view_space_hit);

    vec2 fov = 0.05 * vec2(screen_size.y / screen_size.x, 1.0);
    vec2 border = smoothstep(vec2(0.0), fov, hit.xy) * (1.0 - smoothstep(1.0 - fov, vec2(1.0), hit.xy));
    float vignette = border.x * border.y;

    float confidence = 1.0 - smoothstep(0.0, depth_buffer_thickness, hit_distance);
    confidence *= confidence;
    return vignette * confidence;
}

#endif
