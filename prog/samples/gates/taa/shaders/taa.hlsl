// Gate TAA. HLSL is spec — cook is .spvasm.
// Motion = current NDC - previous NDC (camera + object X slide).
// Resolve: history sample + 3x3 neighbourhood clamp.

struct TaaCB
{
    float4x4 prevViewProj;
    float2 invExtent;
    float objectX;
    float prevObjectX;
    uint colorIdx, historyIdx, motionIdx, historyValid;
    float blend;
    float _p0, _p1, _p2;
};
