// Harpia Engine — GGX prefilter of the environment cube
//
// The second half of the split sum. The table says how much of F0 survives at a
// given roughness and NoV; this says what the surface is looking at. Each mip
// holds the environment convolved with the GGX lobe of one roughness, so a
// shading pixel picks its blur by choosing a mip rather than by integrating at
// runtime.
//
// The approximation is Karis': the integral wants a view direction, a normal
// and a light direction, but a prefiltered cube is indexed by one vector only.
// So N = V = R is assumed. That costs the stretched highlight a surface shows
// at grazing angles — a sphere lit from the side keeps a round hotspot instead
// of a comet-shaped one. Every real-time engine makes the same trade, because
// the alternative is a second lookup dimension for an error most scenes hide.
//
// Weighting by NoL rather than averaging is not cosmetic. Samples whose light
// direction falls below the horizon carry no energy, and letting them into the
// mean darkens every rough surface uniformly — a bug that looks like a wrong
// exposure rather than a wrong integral.

#include "Cubemap.hlsli"
#include "Brdf.hlsli"

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

    const float3 normal = cubeFaceDirection(g_push.face, input.uv);
    const float  alpha  = g_push.roughness * g_push.roughness;

    float3 prefiltered = float3(0.0, 0.0, 0.0);
    float  totalWeight = 0.0;

    for (uint i = 0; i < g_push.sampleCount; ++i) {
        const float2 xi         = hammersley(i, g_push.sampleCount);
        const float3 halfVector = importanceSampleGGX(xi, normal, alpha);

        // N = V = R, so the reflection of the view about the half vector is
        // just the mirror of the normal about it.
        const float3 light = normalize(2.0 * dot(normal, halfVector) * halfVector - normal);

        const float NoL = dot(normal, light);
        if (NoL <= 0.0) {
            continue;
        }

        // SampleLevel(0) on purpose: the source is the mirror mip, and letting
        // hardware mip selection pick a blurrier level would convolve an
        // already-convolved image, compounding the blur per roughness step.
        prefiltered += g_cubeTextures[g_push.sourceTexture].SampleLevel(
            g_samplers[HARPIA_SAMPLER_LINEAR_CLAMP], light, 0).rgb * NoL;
        totalWeight += NoL;
    }

    // A lobe so tight that every sample missed the hemisphere would divide by
    // zero; at that roughness the mirror direction is the honest answer anyway.
    if (totalWeight <= 0.0) {
        return float4(g_cubeTextures[g_push.sourceTexture].SampleLevel(
            g_samplers[HARPIA_SAMPLER_LINEAR_CLAMP], normal, 0).rgb, 1.0);
    }

    return float4(prefiltered / totalWeight, 1.0);
}
