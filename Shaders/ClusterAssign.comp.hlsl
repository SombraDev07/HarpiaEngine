// Harpia Engine — light assignment
//
// Second half of clustered lighting. One thread per cluster: test every
// punctual light's sphere of influence against the cluster's view-space box and
// record the ones that survive.
//
// The test is sphere-versus-AABB by closest point, which is exact for a point
// light. A spot light is culled by the sphere of its range too — a cone test
// would reject more, but it costs branch divergence in the inner loop to save
// work in a shader that is already bound by the list write. The cone is applied
// where it matters, during shading.
//
// Each cluster writes into its own fixed slice of the index buffer. No atomic,
// no shared pool: a pool sized wrong overflows past the buffer and corrupts
// memory, while a fixed stride can only drop a light from a crowded cluster.
// One of those failures is bounded and visible; the other is neither.

#include "Common.hlsli"

[[vk::push_constant]] ClusterLightPush g_push;

struct ClusterBounds {
    float4 minPoint;
    float4 maxPoint;
};

[[vk::binding(1, 0)]] StructuredBuffer<ClusterBounds>   g_bounds[];
[[vk::binding(1, 0)]] StructuredBuffer<PunctualLight>   g_punctual[];
[[vk::binding(1, 0)]] RWStructuredBuffer<uint>          g_lightIndices[];

// Squared distance from a point to a box; zero inside. Squared because the
// comparison against the range only needs the ordering, and a sqrt per light
// per cluster is 3456 * lights square roots for nothing.
// `point` is reserved in HLSL, hence `probe`.
float distanceSquaredToBox(float3 probe, float3 boxMin, float3 boxMax)
{
    const float3 closest = clamp(probe, boxMin, boxMax);
    const float3 delta   = probe - closest;
    return dot(delta, delta);
}

[numthreads(4, 4, 4)]
void main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= HARPIA_CLUSTERS_X || id.y >= HARPIA_CLUSTERS_Y
        || id.z >= HARPIA_CLUSTERS_Z) {
        return;
    }
    if (!harpiaValidStorageBuffer(g_push.clusterBuffer)
        || !harpiaValidStorageBuffer(g_push.lightBuffer)
        || !harpiaValidStorageBuffer(g_push.indexBuffer)) {
        return;
    }

    const uint cluster = id.x + HARPIA_CLUSTERS_X * (id.y + HARPIA_CLUSTERS_Y * id.z);
    const ClusterBounds bounds = g_bounds[g_push.clusterBuffer][cluster];

    const uint base = cluster * (HARPIA_MAX_LIGHTS_PER_CLUSTER + 1);
    uint found = 0;

    for (uint i = 0; i < g_push.lightCount && found < HARPIA_MAX_LIGHTS_PER_CLUSTER; ++i) {
        const PunctualLight light = g_punctual[g_push.lightBuffer][i];

        // Clusters live in view space; lights are authored in world space.
        const float3 viewPosition = mul(g_push.view, float4(light.positionRange.xyz, 1.0)).xyz;
        const float  range        = light.positionRange.w;

        if (distanceSquaredToBox(viewPosition, bounds.minPoint.xyz, bounds.maxPoint.xyz)
            <= range * range) {
            g_lightIndices[g_push.indexBuffer][base + 1 + found] = i;
            ++found;
        }
    }

    // The count leads its own slice, so shading reads one value to know how far
    // to walk instead of consulting a second buffer.
    g_lightIndices[g_push.indexBuffer][base] = found;
}
