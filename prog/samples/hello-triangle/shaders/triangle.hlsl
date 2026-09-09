// Matches triangle.hlsl VSMain / PSMain. Zero descriptors (bindless doc is the contract; heap is phase 2).
struct VSOut
{
    float4 position : SV_Position;
    float3 color : COLOR0;
};

VSOut VSMain(uint vid : SV_VertexID)
{
    const float2 positions[3] = {
        float2(0.0, 0.75),
        float2(0.75, -0.75),
        float2(-0.75, -0.75)
    };
    const float3 colors[3] = {
        float3(1.0, 0.0, 0.0),
        float3(0.0, 1.0, 0.0),
        float3(0.0, 0.0, 1.0)
    };
    VSOut o;
    o.position = float4(positions[vid], 0.0, 1.0);
    o.color = colors[vid];
    return o;
}

float4 PSMain(VSOut i) : SV_Target0
{
    return float4(i.color, 1.0);
}
