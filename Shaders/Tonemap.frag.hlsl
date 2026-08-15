// Harpia Engine — tonemap
//
// ACES fitted curve plus exposure and sRGB encode. This is the last pass: it
// turns the HDR scene into something a display can show.
//
// The fitted approximation rather than the full ACES transform — it is the one
// Unreal and most engines ship, matches the reference closely across the range
// that matters, and costs a handful of instructions instead of a 3D LUT.

#include "Common.hlsli"

struct TonemapPush {
    uint  frameBuffer;
    uint  hdrTexture;
    float exposure;
    uint  padding;
};

[[vk::push_constant]] TonemapPush g_push;

struct PSInput {
    float4 position : SV_Position;
    float2 uv       : TEXCOORD0;
};

// Narkowicz's fit of the ACES filmic curve.
float3 acesFitted(float3 color)
{
    const float a = 2.51;
    const float b = 0.03;
    const float c = 2.43;
    const float d = 0.59;
    const float e = 0.14;
    return saturate((color * (a * color + b)) / (color * (c * color + d) + e));
}

// The swapchain is UNORM rather than SRGB precisely so this pass owns the
// encode; an SRGB target would apply the curve a second time.
float3 encodeSrgb(float3 linearColor)
{
    const float3 lo = linearColor * 12.92;
    const float3 hi = 1.055 * pow(max(linearColor, 1e-5), 1.0 / 2.4) - 0.055;
    return select(linearColor <= 0.0031308, lo, hi);
}

float4 main(PSInput input) : SV_Target0
{
    const float3 hdr =
        g_textures[g_push.hdrTexture].Sample(g_samplers[HARPIA_SAMPLER_POINT_CLAMP],
                                             input.uv).rgb;

    const float3 exposed  = hdr * g_push.exposure;
    const float3 tonemapped = acesFitted(exposed);

    return float4(encodeSrgb(tonemapped), 1.0);
}
