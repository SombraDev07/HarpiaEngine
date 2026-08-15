// Harpia Engine — GBuffer, vertex stage
//
// No vertex input state: the mesh is read from a storage buffer chosen by a
// bindless index. That is what lets one pipeline serve every mesh and is the
// layout GPU-driven submission needs later.

#include "Common.hlsli"

struct VSOutput {
    float4 position     : SV_Position;
    float3 worldNormal  : NORMAL0;
    float4 worldTangent : TANGENT0;
    float2 uv           : TEXCOORD0;

    // Motion vectors are the reason these two travel down: the difference
    // between where this surface is now and where it was last frame. Adding
    // them later would mean reopening every geometry shader in the engine.
    float4 currentClip  : TEXCOORD1;
    float4 previousClip : TEXCOORD2;
};

VSOutput main(uint vertexId : SV_VertexID)
{
    const FrameData  frame  = g_frames[g_push.frameBuffer][0];
    const ObjectData object = g_objects[g_push.objectBuffer][g_push.objectIndex];
    const Vertex     vertex = g_vertices[g_push.vertexBuffer][vertexId];

    const float4 worldPosition     = mul(object.model, float4(vertex.position, 1.0));
    const float4 prevWorldPosition = mul(object.prevModel, float4(vertex.position, 1.0));

    VSOutput output;
    output.position     = mul(frame.viewProjection, worldPosition);
    output.currentClip  = output.position;
    output.previousClip = mul(frame.prevViewProjection, prevWorldPosition);

    // normalMatrix is the inverse transpose, so non-uniform scale does not
    // skew the normal.
    output.worldNormal = mul((float3x3)object.normalMatrix, vertex.normal);
    output.worldTangent = float4(
        mul((float3x3)object.model, vertex.tangent.xyz),
        vertex.tangent.w);

    output.uv = vertex.uv;
    return output;
}
