// Harpia Engine — F1 triangle, fragment stage

struct PSInput {
    float4 position : SV_Position;
    float3 colour   : COLOR0;
};

float4 main(PSInput input) : SV_Target0
{
    return float4(input.colour, 1.0);
}
