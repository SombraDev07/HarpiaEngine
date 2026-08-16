// Harpia Engine — image-based lighting
//
// Split-sum: the specular integral is factored into a prefiltered environment
// and a material-independent BRDF table.
//
//   specular = prefiltered(R, roughness) * (F0 * lut.r + lut.g)
//   diffuse  = irradiance(N) * albedo * (1 - metallic)
//
// The environment here is an analytic sky rather than a prefiltered cubemap.
// That is a deliberate first step, not the end state: the split-sum maths and
// the LUT are identical either way, so swapping in a cubemap loaded from an HDR
// changes which function supplies the radiance and nothing else.
#ifndef HARPIA_IBL_HLSLI
#define HARPIA_IBL_HLSLI

#include "Brdf.hlsli"
#include "Common.hlsli"

// Radiance arriving from `direction`, as a surface of the given roughness sees
// it. Roughness widens the horizon transition rather than merely fading its
// contrast: cross-fading toward an average leaves a hard edge at lower
// contrast, which still reads as a mirror. A real prefiltered cubemap softens
// the edge itself, and so does this.
float3 sampleSky(Environment environment, float3 direction, float perceptualRoughness)
{
    const float height = direction.y;
    const float blur   = perceptualRoughness * perceptualRoughness;

    // Sky gradient, concentrated near the horizon the way a real sky is.
    const float above = saturate(height);
    const float3 sky = lerp(environment.skyHorizon.rgb, environment.skyZenith.rgb,
                            above * above);

    // Transition band around the horizon: a hair wide for a mirror, most of the
    // hemisphere for a fully rough surface.
    const float width = 0.02 + blur * 1.2;
    const float t = saturate(height / width * 0.5 + 0.5);

    return lerp(environment.groundColor.rgb, sky, t) * environment.skyZenith.w;
}

// Cosine-weighted hemisphere integral of the sky around `normal`. For a
// gradient sky the closed form is close enough that sampling would only add
// noise: the hemisphere average sits between the horizon and the pole the
// normal faces.
float3 skyIrradiance(Environment environment, float3 normal)
{
    const float up = normal.y;

    const float3 upperAverage = (environment.skyZenith.rgb + environment.skyHorizon.rgb) * 0.5;
    const float3 lowerAverage = (environment.groundColor.rgb + environment.skyHorizon.rgb) * 0.5;

    // saturate(up) weights how much of the hemisphere sees sky rather than
    // ground; 0.5 at the horizon, 1 straight up.
    const float skyFraction = saturate(up * 0.5 + 0.5);
    return lerp(lowerAverage, upperAverage, skyFraction) * environment.skyZenith.w;
}

// The full IBL contribution for one surface.
//
// The LUT arrives as a bindless index rather than a Texture2D so the bounds
// check lives here, next to the use, instead of at every call site: the one
// thing this pass must never do is hand the hardware an index it invented.
float3 evaluateIbl(Environment environment,
                   uint        brdfLutIndex,
                   SamplerState lutSampler,
                   float3      albedo,
                   float       metallic,
                   float       perceptualRoughness,
                   float3      normal,
                   float3      viewDirection)
{
    if (!harpiaValidTexture(brdfLutIndex)) {
        return float3(0.0, 0.0, 0.0);
    }

    const float NoV = saturate(dot(normal, viewDirection));

    const float3 f0 = lerp(float3(0.04, 0.04, 0.04), albedo, metallic);

    // --- specular ---------------------------------------------------------
    const float3 reflection = reflect(-viewDirection, normal);

    // Stands in for the mip chain a prefiltered cubemap would provide.
    const float3 prefiltered = sampleSky(environment, reflection, perceptualRoughness);

    const float2 ab = g_textures[brdfLutIndex].SampleLevel(
        lutSampler, float2(NoV, perceptualRoughness), 0).rg;
    const float3 specular = prefiltered * (f0 * ab.x + ab.y);

    // --- diffuse ----------------------------------------------------------
    // Metals have no diffuse lobe, so their ambient comes entirely from the
    // specular term above. This is why a metal with no environment renders
    // black, and why IBL is what makes metal look like metal.
    const float3 diffuseColor = albedo * (1.0 - metallic);
    const float3 irradiance   = skyIrradiance(environment, normal);

    // Energy not taken by the specular lobe is what is left for diffuse.
    const float3 kD = (1.0 - f0) * (1.0 - metallic);
    const float3 diffuse = diffuseColor * irradiance * kD;

    return specular + diffuse;
}

#endif // HARPIA_IBL_HLSLI
