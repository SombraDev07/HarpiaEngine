// Gate CSM. HLSL is spec — this host cooks .spvasm (no DXC).
// Shadow VS: instance pos_scale, push cascadeVP+world, depth-only.
// Color: NdotL * sun * Vogel-PCF(atlas) + ambient. cascade_count==0 skips shadows.
// Texel = 1/atlasSize from the CBV, never 1/2048 hardcoded.

struct LightingCB
{
    float4x4 invViewProj;
    float4 cameraPos;
    float4 sunDir;
    float4 sunColor;
    uint gbuf0, gbuf1, gbuf2, gbuf3, gbuf4;
    uint irradiance, prefiltered, brdfLut;
    float iblScale, iblMaxMip, exposure, _pad0;
    float2 invExtent;
    float2 _pad1;
    float4x4 view;
    uint shadowIdx;
    float atlasSize;
    uint cascadeCount;
    uint _pad2;
    float4 splits;
    float4x4 cascadeVP[4];
};

[[vk::push_constant]] struct Push { float4x4 viewProj; float4x4 world; } pc;
[[vk::binding(0, 0)]] ConstantBuffer<LightingCB> cb;
[[vk::binding(0, 1)]] Texture2D bindlessHeap[];
[[vk::binding(1, 2)]] SamplerState clampSamp;

float4 ShadowVS(float3 pos : POSITION, float4 ps : INSTANCE0) : SV_Position
{
    float3 world = pos * ps.w + ps.xyz;
    return mul(pc.viewProj, mul(pc.world, float4(world, 1)));
}

void ColorVS(float3 pos : POSITION, float3 nrm : NORMAL, float4 ps : INSTANCE0, float4 alb : INSTANCE1,
             out float4 clip : SV_Position, out float3 world : WORLD, out float3 wn : NRM, out float3 color : ALB)
{
    world = pos * ps.w + ps.xyz;
    clip = mul(pc.viewProj, mul(pc.world, float4(world, 1)));
    wn = normalize(mul((float3x3)pc.world, nrm));
    color = alb.rgb;
}

float4 ColorPS(float3 world : WORLD, float3 nrm : NRM, float3 alb : ALB) : SV_Target0
{
    // Full CSM + Vogel PCF lives in color.ps.spvasm.
    return float4(alb, 1);
}

float4 BlitPS(float4 pos : SV_Position) : SV_Target0
{
    float2 uv = pos.xy * cb.invExtent;
    return bindlessHeap[NonUniformResourceIndex(cb.gbuf0)].Sample(clampSamp, uv);
}
