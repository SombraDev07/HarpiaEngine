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

// Van der Corput radical inverse — the low-discrepancy sequence that makes a
// few hundred samples behave like many thousand random ones.
float radicalInverseVdC(uint bits)
{
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return float(bits) * 2.3283064365386963e-10;
}

float2 hammersley(uint i, uint count)
{
    return float2(float(i) / float(count), radicalInverseVdC(i));
}

// Samples a half-vector from the GGX distribution. Importance sampling is what
// keeps the sample count affordable: samples land where the lobe actually is.
float3 importanceSampleGGX(float2 xi, float3 normal, float alpha)
{
    const float phi      = 2.0 * HARPIA_PI * xi.x;
    const float cosTheta = sqrt((1.0 - xi.y) / (1.0 + (alpha * alpha - 1.0) * xi.y));
    const float sinTheta = sqrt(1.0 - cosTheta * cosTheta);

    const float3 halfVector = float3(sinTheta * cos(phi), sinTheta * sin(phi), cosTheta);

    // Build a basis around the normal to move the sample into world space.
    const float3 up      = abs(normal.z) < 0.999 ? float3(0, 0, 1) : float3(1, 0, 0);
    const float3 tangentX = normalize(cross(up, normal));
    const float3 tangentY = cross(normal, tangentX);

    return normalize(tangentX * halfVector.x + tangentY * halfVector.y
                   + normal * halfVector.z);
}

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
