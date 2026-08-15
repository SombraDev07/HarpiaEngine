// Harpia Engine — F1 triangle, vertex stage
//
// No vertex buffer: positions come from SV_VertexID. The point of F1 is the
// graph and the pipeline, not geometry — that arrives with glTF in F2.

struct VSOutput {
    float4 position : SV_Position;
    float3 colour   : COLOR0;
};

VSOutput main(uint vertexId : SV_VertexID)
{
    const float2 positions[3] = {
        float2( 0.0, -0.6),
        float2( 0.6,  0.6),
        float2(-0.6,  0.6)
    };

    const float3 colours[3] = {
        float3(1.0, 0.0, 0.0),
        float3(0.0, 1.0, 0.0),
        float3(0.0, 0.0, 1.0)
    };

    VSOutput output;
    output.position = float4(positions[vertexId], 0.0, 1.0);
    output.colour   = colours[vertexId];
    return output;
}
