// Phase 3 PBR gate. Source of truth; cook is .spvasm until DXC is on PATH.

struct VSIn
{
    float3 pos : POSITION;
    float3 nrm : NORMAL;
    float4 posScale : INSTANCE0;          // xyz center, w radius
    float4 albedoCoatRough : INSTANCE1;   // rgb albedo, a clearcoatRoughness
    float4 ormF0 : INSTANCE2;             // ao, roughness, metallic, dielectric F0
    float4 emissiveCoat : INSTANCE3;      // rgb emissive, a clearcoat
    float4 fuzzFuzzColor : INSTANCE4;     // x fuzz, yzw fuzzColor
};

struct VSOut
{
    float4 clip : SV_Position;
    float3 nrm : NORMAL;
    float4 albedoCoatRough : COLOR0;
    float4 ormF0 : COLOR1;
    float4 emissiveCoat : COLOR2;
    float4 fuzzFuzzColor : COLOR3;
};

struct Push
{
    float4x4 viewProj;
    float4x4 world;
};

[[vk::push_constant]] Push pc;

VSOut VSMain(VSIn i)
{
    float3 worldPos = i.pos * i.posScale.w + i.posScale.xyz;
    float4 wp = mul(pc.world, float4(worldPos, 1.0));
    VSOut o;
    o.clip = mul(pc.viewProj, wp);
    o.nrm = normalize(mul(pc.world, float4(i.nrm, 0.0)).xyz);
    o.albedoCoatRough = i.albedoCoatRough;
    o.ormF0 = i.ormF0;
    o.emissiveCoat = i.emissiveCoat;
    o.fuzzFuzzColor = i.fuzzFuzzColor;
    return o;
}

struct GBufferOut
{
    float4 albedo : SV_Target0;      // rgb albedo, a clearcoatRoughness
    float4 normal : SV_Target1;      // rgb n*0.5+0.5, a fuzz
    float4 orm : SV_Target2;         // ao, roughness, metallic, dielectric F0
    float4 emissive : SV_Target3;    // rgb emissive OR fuzzColor, a clearcoat
    float depthColor : SV_Target4;   // SV_Position.z
};

GBufferOut PSMain(VSOut i)
{
    GBufferOut o;
    float3 n = normalize(i.nrm);
    o.albedo = i.albedoCoatRough;
    o.normal = float4(n * 0.5 + 0.5, i.fuzzFuzzColor.x);
    o.orm = i.ormF0;
    // fuzzColor reaches lighting: if fuzz>0, RT3 RGB is fuzzColor (no emissive).
    o.emissive = float4(
        i.fuzzFuzzColor.x > 0.001 ? i.fuzzFuzzColor.yzw : i.emissiveCoat.rgb,
        i.emissiveCoat.a);
    o.depthColor = i.clip.z; // after interpolate ≈ FragCoord.z
    return o;
}
