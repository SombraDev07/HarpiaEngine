// Harpia Engine — split-sum BRDF lookup table
//
// Karis' split-sum approximation factors the specular integral into two halves:
// a prefiltered environment, and this table. The table holds a scale and a bias
// applied to F0, so it depends only on NoV and roughness — never on the
// material or the environment. Generated once at startup and reused forever.
//
//   specular = prefiltered(R, roughness) * (F0 * lut.r + lut.g)
//
// X axis is NoV, Y axis is roughness.

#include "Brdf.hlsli"

struct PSInput {
    float4 position : SV_Position;
    float2 uv       : TEXCOORD0;
};

// radicalInverseVdC, hammersley and importanceSampleGGX live in Brdf.hlsli:
// the environment prefilter integrates the same lobe and must not drift
// from this table.

// The IBL form of Smith visibility uses a different k than the direct-light
// form: alpha/2 rather than the (roughness+1)^2/8 used for punctual lights.
float visibilitySmithIbl(float NoV, float NoL, float alpha)
{
    const float k = alpha * 0.5;
    const float ggxV = NoV / (NoV * (1.0 - k) + k);
    const float ggxL = NoL / (NoL * (1.0 - k) + k);
    return ggxV * ggxL;
}

float2 integrateBrdf(float NoV, float roughness)
{
    const float alpha = roughness * roughness;

    // The integral is rotationally symmetric, so the view can be pinned to the
    // XZ plane and the normal to +Z without loss.
    const float3 view = float3(sqrt(1.0 - NoV * NoV), 0.0, NoV);
    const float3 normal = float3(0.0, 0.0, 1.0);

    float scale = 0.0;
    float bias  = 0.0;

    const uint kSampleCount = 1024;
    for (uint i = 0; i < kSampleCount; ++i) {
        const float2 xi = hammersley(i, kSampleCount);
        const float3 halfVector = importanceSampleGGX(xi, normal, alpha);
        const float3 light = normalize(2.0 * dot(view, halfVector) * halfVector - view);

        const float NoL = saturate(light.z);
        if (NoL <= 0.0) {
            continue;
        }

        const float NoH = saturate(halfVector.z);
        const float VoH = saturate(dot(view, halfVector));

        const float g   = visibilitySmithIbl(NoV, NoL, alpha);
        const float gVis = (g * VoH) / max(NoH * NoV, 1e-7);

        // Fresnel is factored out so the table is material-independent: what
        // remains is the coefficient F0 gets multiplied by, and the additive
        // term for the grazing case.
        const float fc = pow(1.0 - VoH, 5.0);
        scale += (1.0 - fc) * gVis;
        bias  += fc * gVis;
    }

    return float2(scale, bias) / float(kSampleCount);
}

float2 main(PSInput input) : SV_Target0
{
    // Never zero: NoV of exactly 0 makes the integral degenerate.
    const float NoV       = max(input.uv.x, 1e-3);
    const float roughness = max(input.uv.y, 1e-3);
    return integrateBrdf(NoV, roughness);
}
