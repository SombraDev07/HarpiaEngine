// Harpia Engine — fullscreen triangle
//
// One oversized triangle rather than two triangles for a quad: no diagonal
// seam, and the GPU rasterises one primitive instead of two. No vertex buffer;
// the positions come from SV_VertexID.

struct VSOutput {
    float4 position : SV_Position;
    float2 uv       : TEXCOORD0;
};

VSOutput main(uint vertexId : SV_VertexID)
{
    // ids 0,1,2 give uv (0,0) (2,0) (0,2), which is a triangle covering the
    // whole [0,1] square with room to spare.
    const float2 uv = float2((vertexId << 1) & 2, vertexId & 2);

    VSOutput output;
    output.uv = uv;
    // Vulkan clip space has +Y down and UV origin is top-left, so uv maps to
    // clip directly with no flip.
    output.position = float4(uv * 2.0 - 1.0, 0.0, 1.0);
    return output;
}
