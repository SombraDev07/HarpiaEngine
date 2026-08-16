// Harpia Engine — diffuse irradiance cube
//
// The other lobe. The prefiltered chain answers "what does a mirror of this
// roughness see"; this answers "how much light arrives at a surface facing this
// way", which is the cosine-weighted integral of the whole hemisphere around
// the normal.
//
// It gets its own tiny cube rather than a mip of the environment because the
// two are different integrals, not different resolutions of one. A cosine lobe
// covers the entire hemisphere no matter what, so the result is smooth by
// construction — 32x32 per face is already more resolution than the signal has.
//
// Sampled uniformly in spherical coordinates rather than by importance: a
// cosine lobe is wide enough that low-discrepancy sampling buys nothing, and
// the fixed grid makes the sin(theta) Jacobian explicit instead of hidden
// inside a pdf.

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

    const float3 normal = cubeFaceDirection(g_push.face, input.uv);

    // A basis around the normal. The up vector is swapped near the poles for
    // the same reason it is in importanceSampleGGX: cross() with a parallel
    // vector is zero, and normalising that is a NaN that spreads.
    const float3 up      = abs(normal.y) < 0.999 ? float3(0.0, 1.0, 0.0)
                                                 : float3(1.0, 0.0, 0.0);
    const float3 right   = normalize(cross(up, normal));
    const float3 forward = cross(normal, right);

    const float sampleDelta = 0.025;

    float3 irradiance = float3(0.0, 0.0, 0.0);
    float  samples    = 0.0;

    for (float phi = 0.0; phi < 2.0 * HARPIA_PI; phi += sampleDelta) {
        for (float theta = 0.0; theta < 0.5 * HARPIA_PI; theta += sampleDelta) {
            const float sinTheta = sin(theta);
            const float cosTheta = cos(theta);

            const float3 tangentSample =
                float3(sinTheta * cos(phi), sinTheta * sin(phi), cosTheta);

            const float3 direction = tangentSample.x * right
                                   + tangentSample.y * forward
                                   + tangentSample.z * normal;

            // cos weights the lobe; sin is the Jacobian of the spherical grid.
            // Dropping sin is the classic error here — it over-weights the pole
            // and leaves every surface too bright in the direction it faces.
            irradiance += g_cubeTextures[g_push.sourceTexture].SampleLevel(
                g_samplers[HARPIA_SAMPLER_LINEAR_CLAMP], direction, 0).rgb
                        * cosTheta * sinTheta;
            samples += 1.0;
        }
    }

    // PI from the integral of the cosine lobe over the hemisphere; the division
    // turns the sum back into an average.
    return float4(HARPIA_PI * irradiance / max(samples, 1.0), 1.0);
}
