// Harpia Engine — deferred lighting
//
// Reads the GBuffer, reconstructs world position from depth, shades with the
// BRDF and writes HDR. Tonemapping is a separate pass in step 6 — writing an
// LDR value here would throw away the range that exposure and bloom need.

#include "Common.hlsli"
#include "Brdf.hlsli"

[[vk::push_constant]] LightingPush g_push;

struct PSInput {
    float4 position : SV_Position;
    float2 uv       : TEXCOORD0;
};

// Pushes the pixel back through the inverse view-projection. Reverse-Z means a
// depth of 0 is the far plane, which is what an untouched background carries.
float3 reconstructWorldPosition(float2 uv, float depth, float4x4 invViewProjection)
{
    // UV origin is top-left and Vulkan clip +Y is down, so this needs no flip.
    const float4 clip = float4(uv * 2.0 - 1.0, depth, 1.0);
    const float4 world = mul(invViewProjection, clip);
    return world.xyz / world.w;
}

float4 main(PSInput input) : SV_Target0
{
    const FrameData        frame = g_frames[g_push.frameBuffer][0];
    const DirectionalLight light = g_lights[g_push.lightBuffer][0];

    const SamplerState pointClamp = g_samplers[HARPIA_SAMPLER_POINT_CLAMP];

    const float depth = g_textures[g_push.depthTexture].Sample(pointClamp, input.uv).r;

    // Reverse-Z: nothing was drawn here, so there is no surface to shade.
    if (depth <= 0.0) {
        return float4(0.0, 0.0, 0.0, 1.0);
    }

    const float4 albedoSample   = g_textures[g_push.albedoTexture].Sample(pointClamp, input.uv);
    const float2 normalSample   = g_textures[g_push.normalTexture].Sample(pointClamp, input.uv).rg;
    const float2 materialSample = g_textures[g_push.materialTexture].Sample(pointClamp, input.uv).rg;

    const float3 albedo    = albedoSample.rgb;
    const float3 normal    = decodeOctahedral(normalSample);
    const float  roughness = materialSample.r;
    const float  metallic  = materialSample.g;

    const float3 worldPosition = reconstructWorldPosition(input.uv, depth,
                                                          frame.invViewProjection);
    const float3 viewDirection = normalize(frame.cameraPosition.xyz - worldPosition);

    // The light's direction points away from it; shading wants the vector from
    // the surface towards it.
    const float3 lightDirection = normalize(-light.direction.xyz);
    const float3 lightColor     = light.colorIntensity.rgb * light.colorIntensity.w;

    float3 color = shadeDirect(albedo, metallic, roughness, normal,
                               viewDirection, lightDirection, lightColor);

    // Flat ambient stands in until image-based lighting arrives in step 4.
    color += albedo * light.ambient.rgb;

    return float4(color, 1.0);
}
