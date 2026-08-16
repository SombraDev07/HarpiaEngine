// Harpia Engine — cluster bounds
//
// The first compute pass in the engine, and the first half of clustered
// lighting. The view frustum is diced into a fixed 3D grid; this writes the
// view-space AABB of every cell so light assignment can test spheres against
// boxes instead of against the frustum.
//
// Depth is sliced exponentially, not linearly. A linear slice spends most of
// its cells on distance nobody has geometry in, and leaves the near field —
// where lights actually overlap — in one or two cells. The exponential
// distribution is what makes a fixed cluster count enough at every scale.
//
//   z(slice) = near * (far/near)^(slice / slices)

#include "Common.hlsli"

[[vk::push_constant]] ClusterPush g_push;

struct ClusterBounds {
    float4 minPoint;   // view space, w unused
    float4 maxPoint;
};

[[vk::binding(1, 0)]] RWStructuredBuffer<ClusterBounds> g_clusterBounds[];

// Where a screen-space point lands on the near plane, in view space.
float3 screenToView(float2 screen, ClusterPush push)
{
    const float2 ndc = float2(screen.x / push.renderSize.x * 2.0 - 1.0,
                              screen.y / push.renderSize.y * 2.0 - 1.0);

    // Undo the projection's scale. tanHalfFov and aspect arrive precomputed so
    // this stays free of a matrix inverse per invocation.
    return float3(ndc.x * push.tanHalfFov * push.aspect, ndc.y * push.tanHalfFov, 1.0);
}

[numthreads(4, 4, 4)]
void main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= HARPIA_CLUSTERS_X || id.y >= HARPIA_CLUSTERS_Y
        || id.z >= HARPIA_CLUSTERS_Z) {
        return;
    }
    if (!harpiaValidStorageBuffer(g_push.clusterBuffer)) {
        return;
    }

    const float2 tileSize = g_push.renderSize
                          / float2(HARPIA_CLUSTERS_X, HARPIA_CLUSTERS_Y);

    const float3 nearCorner = screenToView(float2(id.xy) * tileSize, g_push);
    const float3 farCorner  = screenToView(float2(id.xy + 1) * tileSize, g_push);

    // Exponential depth slicing.
    const float ratio  = g_push.farPlane / g_push.nearPlane;
    const float sliceNear = g_push.nearPlane
                          * pow(ratio, float(id.z) / float(HARPIA_CLUSTERS_Z));
    const float sliceFar  = g_push.nearPlane
                          * pow(ratio, float(id.z + 1) / float(HARPIA_CLUSTERS_Z));

    // The tile is a truncated pyramid; its AABB is the extremes of the four
    // rays evaluated at both slice depths.
    const float3 a = nearCorner * sliceNear;
    const float3 b = nearCorner * sliceFar;
    const float3 c = farCorner  * sliceNear;
    const float3 d = farCorner  * sliceFar;

    const uint index = id.x + HARPIA_CLUSTERS_X * (id.y + HARPIA_CLUSTERS_Y * id.z);

    ClusterBounds bounds;
    bounds.minPoint = float4(min(min(a, b), min(c, d)), 0.0);
    bounds.maxPoint = float4(max(max(a, b), max(c, d)), 0.0);
    g_clusterBounds[g_push.clusterBuffer][index] = bounds;
}
