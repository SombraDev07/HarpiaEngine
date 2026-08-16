// Harpia Engine — shader-side mirror of Source/RHI/RenderTypes.h
//
// These structs must stay byte-identical to the C++ side. The C++ header has
// static_asserts on their sizes; if you change one, change both.
//
// Bindless: one descriptor set, arrays declared at the binding slots
// VulkanBindless publishes. Several typed views alias the same binding, which
// is the standard pattern and needs DXC — glslang's HLSL front end cannot index
// a buffer descriptor array at all.
#ifndef HARPIA_COMMON_HLSLI
#define HARPIA_COMMON_HLSLI

struct Vertex {
    float3 position;
    float3 normal;
    float4 tangent;   // w carries the bitangent sign
    float2 uv;
};

struct FrameData {
    float4x4 viewProjection;
    float4x4 prevViewProjection;
    float4x4 invViewProjection;
    float4   cameraPosition;
    float2   renderSize;
    float2   invRenderSize;
};

// Mirrors GpuEnvironment. Every shared struct lives here so there is one place
// to compare against RenderTypes.h.
struct Environment {
    float4 skyZenith;    // rgb radiance looking straight up, w intensity
    float4 skyHorizon;
    float4 groundColor;
    uint   brdfLut;      // bindless index of the split-sum table
    uint3  padding;
};

struct DirectionalLight {
    float4 direction;       // points from the light
    float4 colorIntensity;  // rgb colour, w intensity
    float4 ambient;
};

struct ObjectData {
    float4x4 model;
    float4x4 prevModel;
    float4x4 normalMatrix;
    uint     materialIndex;
    uint3    padding;
};

struct MaterialData {
    float4 baseColorFactor;
    float4 emissiveFactor;
    float  metallicFactor;
    float  roughnessFactor;
    float2 padding;
    uint   baseColorTexture;
    uint   normalTexture;
    uint   metallicRoughnessTexture;
    uint   emissiveTexture;
};

#define HARPIA_INVALID_TEXTURE 0xFFFFFFFFu

// Sampler slots the renderer registers at startup.
#define HARPIA_SAMPLER_LINEAR_REPEAT 0
#define HARPIA_SAMPLER_POINT_CLAMP   1

// Mirrors VulkanBindless::kMax*, which create() refuses to run without. A
// bindless index reaches a shader as a plain uint, from a push constant a call
// site forgot to fill or from buffer bytes that mean something else entirely —
// nothing distinguishes it from a real slot. Indexing past the descriptor array
// is undefined behaviour, and on AMD the scalar unit faults on the descriptor
// load: ring timeout, GPU reset, desktop session gone. The bound is what keeps
// a stale index a visibly wrong pixel instead.
#define HARPIA_MAX_TEXTURES        16384
#define HARPIA_MAX_STORAGE_BUFFERS 4096
#define HARPIA_MAX_SAMPLERS        256

bool harpiaValidTexture(uint index)       { return index < HARPIA_MAX_TEXTURES; }
bool harpiaValidStorageBuffer(uint index) { return index < HARPIA_MAX_STORAGE_BUFFERS; }
bool harpiaValidSampler(uint index)       { return index < HARPIA_MAX_SAMPLERS; }

// Binding 0: sampled images. Binding 1: storage buffers. Binding 2: samplers.
[[vk::binding(0, 0)]] Texture2D<float4> g_textures[];
[[vk::binding(2, 0)]] SamplerState      g_samplers[];

[[vk::binding(1, 0)]] StructuredBuffer<Vertex>       g_vertices[];
[[vk::binding(1, 0)]] StructuredBuffer<FrameData>    g_frames[];
[[vk::binding(1, 0)]] StructuredBuffer<ObjectData>   g_objects[];
[[vk::binding(1, 0)]] StructuredBuffer<MaterialData>     g_materials[];
[[vk::binding(1, 0)]] StructuredBuffer<DirectionalLight> g_lights[];
[[vk::binding(1, 0)]] StructuredBuffer<Environment>      g_environments[];

// Push constant blocks. HLSL allows only one per shader, so each shader
// declares the variable itself:
//
//   [[vk::push_constant]] GBufferPush g_push;
//
struct GBufferPush {
    uint frameBuffer;
    uint objectBuffer;
    uint materialBuffer;
    uint vertexBuffer;
    uint objectIndex;
    uint3 padding;
};

struct LightingPush {
    uint frameBuffer;
    uint lightBuffer;
    uint albedoTexture;
    uint normalTexture;
    uint materialTexture;
    uint depthTexture;
    uint environmentBuffer;
    uint padding;
};

// --- normal encoding --------------------------------------------------------
// Mirrors Core/Math encodeOctahedral; the C++ test sweeps the whole sphere.

float2 octahedralWrap(float2 v)
{
    return (1.0 - abs(v.yx)) * select(v.xy >= 0.0, 1.0, -1.0);
}

float2 encodeOctahedral(float3 n)
{
    n /= (abs(n.x) + abs(n.y) + abs(n.z));
    n.xy = n.z >= 0.0 ? n.xy : octahedralWrap(n.xy);
    return n.xy;
}

float3 decodeOctahedral(float2 encoded)
{
    float3 n = float3(encoded.x, encoded.y, 1.0 - abs(encoded.x) - abs(encoded.y));
    if (n.z < 0.0) {
        n.xy = octahedralWrap(n.xy);
    }
    return normalize(n);
}

#endif // HARPIA_COMMON_HLSLI
