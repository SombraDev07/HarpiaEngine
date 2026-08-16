// Harpia Engine — physically based BRDF
//
// Cook-Torrance specular with GGX/Trowbridge-Reitz distribution, the
// height-correlated Smith visibility term, Schlick's Fresnel, and Burley's
// diffuse. These are the same choices Filament and every modern engine make;
// the value here is that they are written once, documented, and mirrored by a
// CPU reference the tests compare against.
//
// Roughness is perceptual throughout: alpha = roughness^2. Artists author the
// perceptual value, and skipping the square is a classic source of materials
// that look right at the extremes and wrong in the middle.
#ifndef HARPIA_BRDF_HLSLI
#define HARPIA_BRDF_HLSLI

// HARPIA_PI moved to Common.hlsli when the cubemap passes needed it without
// dragging in a BRDF they do not evaluate.
#include "Common.hlsli"

// GGX normal distribution. Describes how much of the microfacet surface is
// oriented to reflect from the light to the eye.
float distributionGGX(float NoH, float alpha)
{
    const float a2 = alpha * alpha;
    const float d  = (NoH * a2 - NoH) * NoH + 1.0;
    return a2 / max(HARPIA_PI * d * d, 1e-7);
}

// Height-correlated Smith visibility. This is G / (4 NoL NoV) already folded
// together, so the caller does not divide again.
float visibilitySmithGGXCorrelated(float NoV, float NoL, float alpha)
{
    const float a2 = alpha * alpha;
    const float lambdaV = NoL * sqrt((NoV - a2 * NoV) * NoV + a2);
    const float lambdaL = NoV * sqrt((NoL - a2 * NoL) * NoL + a2);
    return 0.5 / max(lambdaV + lambdaL, 1e-7);
}

float3 fresnelSchlick(float VoH, float3 f0)
{
    const float f = pow(1.0 - VoH, 5.0);
    return f0 + (1.0 - f0) * f;
}

// Burley diffuse. Retains more light at grazing angles on rough surfaces than
// Lambert, which is what keeps rough dielectrics from looking flat.
float diffuseBurley(float NoV, float NoL, float LoH, float roughness)
{
    const float f90 = 0.5 + 2.0 * roughness * LoH * LoH;
    const float lightScatter = 1.0 + (f90 - 1.0) * pow(1.0 - NoL, 5.0);
    const float viewScatter  = 1.0 + (f90 - 1.0) * pow(1.0 - NoV, 5.0);
    return lightScatter * viewScatter * (1.0 / HARPIA_PI);
}

// One light's contribution. `lightDirection` points from the surface towards
// the light, already normalised.
float3 shadeDirect(float3 albedo,
                   float  metallic,
                   float  perceptualRoughness,
                   float3 normal,
                   float3 viewDirection,
                   float3 lightDirection,
                   float3 lightColor)
{
    const float alpha = perceptualRoughness * perceptualRoughness;

    const float3 halfVector = normalize(viewDirection + lightDirection);

    const float NoV = abs(dot(normal, viewDirection)) + 1e-5;
    const float NoL = saturate(dot(normal, lightDirection));
    const float NoH = saturate(dot(normal, halfVector));
    const float LoH = saturate(dot(lightDirection, halfVector));

    if (NoL <= 0.0) {
        return float3(0.0, 0.0, 0.0);
    }

    // Dielectrics reflect about 4% head-on; metals use their albedo as f0 and
    // have no diffuse lobe at all.
    const float3 f0 = lerp(float3(0.04, 0.04, 0.04), albedo, metallic);
    const float3 diffuseColor = albedo * (1.0 - metallic);

    const float  D = distributionGGX(NoH, alpha);
    const float  V = visibilitySmithGGXCorrelated(NoV, NoL, alpha);
    const float3 F = fresnelSchlick(LoH, f0);

    const float3 specular = (D * V) * F;

    // Energy that was not reflected specularly is what remains for diffuse.
    const float3 diffuse = diffuseColor * diffuseBurley(NoV, NoL, LoH, perceptualRoughness)
                         * (1.0 - F);

    return (diffuse + specular) * lightColor * NoL;
}

// --- importance sampling ----------------------------------------------------
// Shared by the split-sum table and the environment prefilter: both integrate
// the same GGX lobe, one over roughness and NoV, the other over the sky. Two
// copies would be two chances for the halves of the split sum to drift apart,
// and the failure would read as slightly wrong metal rather than as a bug.

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
    const float3 up       = abs(normal.z) < 0.999 ? float3(0, 0, 1) : float3(1, 0, 0);
    const float3 tangentX = normalize(cross(up, normal));
    const float3 tangentY = cross(normal, tangentX);

    return normalize(tangentX * halfVector.x + tangentY * halfVector.y
                   + normal * halfVector.z);
}

#endif // HARPIA_BRDF_HLSLI
