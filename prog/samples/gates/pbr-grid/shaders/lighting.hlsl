// Lighting fullscreen. 1 sun + IBL split-sum + multi-scatter on the sun + ACES.
// GBuffer packing: roadmap §3. fuzzColor in RT3 when fuzz>0.

struct LightingCB
{
    float4x4 invViewProj;
    float4 cameraPos;
    float4 sunDir;
    float4 sunColor;
    uint gbuf0, gbuf1, gbuf2, gbuf3, gbuf4;
    uint irradiance, prefiltered, brdfLut;
    float iblScale;
    float iblMaxMip;
    float exposure;
    float _pad0;
    float2 invExtent;
    float2 _pad1;
};

[[vk::binding(0, 0)]] ConstantBuffer<LightingCB> cb;
[[vk::binding(0, 1)]] Texture2D bindlessHeap[];
[[vk::binding(0, 2)]] SamplerState wrapSamp;
[[vk::binding(1, 2)]] SamplerState clampSamp;

float4 VSMain(uint vid : SV_VertexID) : SV_Position
{
    const float2 p[3] = { float2(-1, -1), float2(3, -1), float2(-1, 3) };
    return float4(p[vid], 0, 1);
}

float4 PSMain(float4 pos : SV_Position) : SV_Target0
{
    float2 uv = pos.xy * cb.invExtent;
    // Full BRDF lives in lighting.ps.spvasm (no DXC on this host).
    return float4(uv, 0, 1);
}
