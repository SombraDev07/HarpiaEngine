// Bindless gate. Heap[idx] + CPU mips + compute UAV. Zero plugins.
struct VSOut
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

VSOut VSMain(uint vid : SV_VertexID)
{
    const float2 positions[3] = {
        float2(0.0, 0.75),
        float2(0.75, -0.75),
        float2(-0.75, -0.75)
    };
    const float2 uvs[3] = {
        float2(0.5, 0.0),
        float2(1.0, 1.0),
        float2(0.0, 1.0)
    };
    VSOut o;
    o.position = float4(positions[vid], 0.0, 1.0);
    o.uv = uvs[vid];
    return o;
}

[[vk::binding(0, 0)]] cbuffer FrameCB
{
    uint texA;
    uint texB;
    uint2 _pad;
};

[[vk::binding(0, 1)]] Texture2D bindlessHeap[];
[[vk::binding(0, 2)]] SamplerState linearSamp;

float4 PSMain(VSOut i) : SV_Target0
{
    float4 a = bindlessHeap[NonUniformResourceIndex(texA)].Sample(linearSamp, i.uv);
    float4 b = bindlessHeap[NonUniformResourceIndex(texB)].Sample(linearSamp, i.uv);
    return a * 0.5 + b * 0.5;
}

[[vk::binding(0, 4)]] RWTexture2D<float4> outImage;

[numthreads(8, 8, 1)]
void CSMain(uint3 id : SV_DispatchThreadID)
{
    uint2 p = id.xy;
    float r = ((p.x / 8) % 2 == 0) ? 1.0 : 0.2;
    float g = ((p.y / 8) % 2 == 0) ? 1.0 : 0.2;
    outImage[p] = float4(r, g, 0.15, 1.0);
}
