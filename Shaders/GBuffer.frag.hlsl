// Harpia Engine — GBuffer, fragment stage
//
// Four targets plus depth:
//   0  albedo.rgb, occlusion
//   1  octahedral normal (two channels — a unit vector needs no third)
//   2  roughness, metallic
//   3  motion vector, in UV space
//
// Depth is reverse-Z, so it is written by the fixed-function stage with the
// projection from Core/Math.

#include "Common.hlsli"

[[vk::push_constant]] GBufferPush g_push;

struct PSInput {
    float4 position     : SV_Position;
    float3 worldNormal  : NORMAL0;
    float4 worldTangent : TANGENT0;
    float2 uv           : TEXCOORD0;
    float4 currentClip  : TEXCOORD1;
    float4 previousClip : TEXCOORD2;
};

struct PSOutput {
    float4 albedo   : SV_Target0;
    float2 normal   : SV_Target1;
    float2 material : SV_Target2;
    float2 motion   : SV_Target3;
};

float4 sampleOrFactor(uint textureIndex, float2 uv, float4 factor)
{
    if (textureIndex == HARPIA_INVALID_TEXTURE) {
        return factor;
    }
    // Sampler 0 is the engine's default trilinear repeat sampler.
    return g_textures[textureIndex].Sample(g_samplers[0], uv) * factor;
}

PSOutput main(PSInput input)
{
    const ObjectData   object   = g_objects[g_push.objectBuffer][g_push.objectIndex];
    const MaterialData material = g_materials[g_push.materialBuffer][object.materialIndex];

    PSOutput output;

    const float4 baseColor =
        sampleOrFactor(material.baseColorTexture, input.uv, material.baseColorFactor);
    output.albedo = float4(baseColor.rgb, 1.0);

    // Interpolation shortens the normal; renormalising is not optional.
    output.normal = encodeOctahedral(normalize(input.worldNormal));

    float roughness = material.roughnessFactor;
    float metallic  = material.metallicFactor;
    if (material.metallicRoughnessTexture != HARPIA_INVALID_TEXTURE) {
        // glTF packs roughness in G and metallic in B.
        const float4 packed =
            g_textures[material.metallicRoughnessTexture].Sample(g_samplers[0], input.uv);
        roughness *= packed.g;
        metallic  *= packed.b;
    }
    output.material = float2(roughness, metallic);

    // Both positions divided by their own w, then the difference. Storing it in
    // UV space means the resolve pass does not need the projection again.
    const float2 currentNdc  = input.currentClip.xy / input.currentClip.w;
    const float2 previousNdc = input.previousClip.xy / input.previousClip.w;
    output.motion = (currentNdc - previousNdc) * 0.5;

    return output;
}
