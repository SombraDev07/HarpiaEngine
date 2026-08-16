// Harpia Engine — deferred lighting
//
// Reads the GBuffer, reconstructs world position from depth, shades with the
// BRDF and writes HDR. Tonemapping is a separate pass in step 6 — writing an
// LDR value here would throw away the range that exposure and bloom need.

#include "Common.hlsli"
#include "Brdf.hlsli"
#include "Ibl.hlsli"

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

    // --- punctual lights, through the cluster grid ---------------------------
    // The pixel finds its own cluster from screen position and view depth, then
    // walks only the lights assignment put there. This is the whole point of
    // clustering: cost scales with lights *near this pixel*, not with lights in
    // the scene.
    if (harpiaValidStorageBuffer(g_push.punctualBuffer)
        && harpiaValidStorageBuffer(g_push.clusterIndices)) {

        const float3 toEye     = frame.cameraPosition.xyz - worldPosition;
        const float  viewDepth = max(length(toEye), g_push.nearPlane);

        const uint2 tile = uint2(input.uv * float2(HARPIA_CLUSTERS_X, HARPIA_CLUSTERS_Y));
        const uint3 cell = uint3(min(tile.x, HARPIA_CLUSTERS_X - 1),
                                 min(tile.y, HARPIA_CLUSTERS_Y - 1),
                                 harpiaDepthSlice(viewDepth, g_push.nearPlane,
                                                  g_push.farPlane));

        const uint cluster = cell.x + HARPIA_CLUSTERS_X * (cell.y + HARPIA_CLUSTERS_Y * cell.z);
        const uint base    = cluster * (HARPIA_MAX_LIGHTS_PER_CLUSTER + 1);
        const uint count   = min(g_clusterIndices[g_push.clusterIndices][base],
                                 HARPIA_MAX_LIGHTS_PER_CLUSTER);

        for (uint i = 0; i < count; ++i) {
            const uint index = g_clusterIndices[g_push.clusterIndices][base + 1 + i];
            const PunctualLight punctual = g_punctualLights[g_push.punctualBuffer][index];

            const float3 toLight  = punctual.positionRange.xyz - worldPosition;
            const float  distance = length(toLight);
            const float  range    = punctual.positionRange.w;
            if (distance >= range || distance <= 0.0) {
                continue;
            }

            const float3 lightVector = toLight / distance;

            // Inverse square, windowed so the light reaches exactly zero at its
            // range. Without the window a light pops when it leaves a cluster,
            // because the falloff was still non-zero where culling cut it off.
            const float ratio      = distance / range;
            const float window     = saturate(1.0 - ratio * ratio * ratio * ratio);
            const float attenuation = window * window / max(distance * distance, 1e-4);

            // Spot cone. A point light has cos(outer) = -1, so the smoothstep
            // is 1 everywhere and the same code path serves both.
            const float cosAngle = dot(-lightVector, normalize(punctual.directionAngles.xyz));
            const float cosOuter = punctual.directionAngles.w;
            const float cosInner = punctual.params.x;
            const float cone     = punctual.params.y > 0.5
                ? smoothstep(cosOuter, cosInner, cosAngle) : 1.0;

            if (cone <= 0.0) {
                continue;
            }

            const float3 radiance = punctual.colorIntensity.rgb * punctual.colorIntensity.w
                                  * attenuation * cone;

            color += shadeDirect(albedo, metallic, roughness, normal,
                                 viewDirection, lightVector, radiance);
        }
    }

    // Image-based ambient. This is what stops a metal from rendering black:
    // with no diffuse lobe, everything a metal shows comes from the
    // environment.
    //
    // A pass that never registers an environment leaves the index at zero,
    // which is a real slot holding some other buffer — the bounds check cannot
    // catch that, so the guard is the index being in range and the LUT index
    // read from those bytes being in range too. evaluateIbl does the second.
    if (harpiaValidStorageBuffer(g_push.environmentBuffer)) {
        const Environment environment = g_environments[g_push.environmentBuffer][0];
        color += evaluateIbl(environment,
                             environment.brdfLut,
                             g_samplers[HARPIA_SAMPLER_LINEAR_REPEAT],
                             albedo, metallic, roughness, normal, viewDirection);
    }

    return float4(color, 1.0);
}
