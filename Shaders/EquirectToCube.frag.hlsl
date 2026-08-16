// Harpia Engine — equirectangular radiance map to cube faces
//
// The first of the three cubemap passes. An .hdr ships as a latitude/longitude
// rectangle, which is the wrong shape to filter against: its texel density is
// wildly non-uniform, packing the poles with samples nobody looks at and
// starving the horizon where everything happens. Prefiltering that directly
// would spend its sample budget on the sky above the camera.
//
// So the environment is projected onto a cube first, and every pass after this
// one works in cube space where a texel is a roughly constant solid angle.
//
// One draw per face, into a view of that single array layer. Layered rendering
// in one draw would need multiview or a geometry shader; six draws of a
// fullscreen triangle cost nothing at startup and keep the pass readable.

#include "Cubemap.hlsli"

[[vk::push_constant]] CubePush g_push;

struct PSInput {
    float4 position : SV_Position;
    float2 uv       : TEXCOORD0;
};

float4 main(PSInput input) : SV_Target0
{
    if (!harpiaValidTexture(g_push.sourceTexture)) {
        return float4(0.0, 0.0, 0.0, 1.0);
    }

    const float3 direction = cubeFaceDirection(g_push.face, input.uv);

    // SampleLevel rather than Sample: there is no derivative to speak of at a
    // face edge, where neighbouring pixels look in directions that diverge
    // sharply, and mip selection from those derivatives picks a blurry level
    // along every seam.
    const float3 radiance = g_textures[g_push.sourceTexture].SampleLevel(
        g_samplers[HARPIA_SAMPLER_LINEAR_CLAMP],
        directionToEquirect(direction), 0).rgb;

    return float4(radiance, 1.0);
}
