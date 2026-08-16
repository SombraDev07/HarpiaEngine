// Harpia Engine — data shared with shaders
//
// Every struct here has a mirror in Shaders/Common.hlsli. They must stay
// byte-identical: a silent layout drift shows up as geometry in the wrong place
// or lighting that is subtly off, with nothing in the logs.
//
// Layout rules: everything is 16-byte aligned by hand rather than relying on
// std430 inference. scalarBlockLayout is enabled on the device, but writing the
// padding out keeps the two sides readable side by side.
#pragma once

#include "Core/Math/Math.h"

#include <cstdint>

namespace harpia::rhi {

// One per frame. Previous-frame matrices are what motion vectors are made of,
// which is why they live here from the first GBuffer rather than arriving with
// TAA in F6.
struct GpuFrameData {
    Mat4 viewProjection{1.0f};
    Mat4 prevViewProjection{1.0f};
    // Deferred shading has only depth to work from, so world position is
    // reconstructed by pushing the pixel back through this.
    Mat4 invViewProjection{1.0f};
    Vec4 cameraPosition{0.0f};   // w unused
    Vec2 renderSize{0.0f};
    Vec2 invRenderSize{0.0f};
};

// A single directional light for now. Punctual lights and clustered culling are
// step 5; the BRDF does not change when they arrive.
struct GpuDirectionalLight {
    Vec4 direction{0.0f, -1.0f, 0.0f, 0.0f};  // points from the light, w unused
    Vec4 colorIntensity{1.0f, 1.0f, 1.0f, 1.0f}; // rgb colour, w intensity
    Vec4 ambient{0.03f, 0.03f, 0.03f, 0.0f};  // stands in until IBL lands in step 4
};

// The environment the IBL pass integrates against. Analytic sky for now; a
// prefiltered cubemap replaces the radiance source without changing the split-
// sum maths or the LUT.
struct GpuEnvironment {
    Vec4 skyZenith{0.20f, 0.35f, 0.65f, 1.0f};   // rgb radiance, w intensity
    Vec4 skyHorizon{0.55f, 0.62f, 0.72f, 0.0f};
    Vec4 groundColor{0.12f, 0.10f, 0.09f, 0.0f};

    std::uint32_t brdfLut = 0xFFFFFFFFu;         // bindless index
    std::uint32_t padding[3]{};
};

// One per drawable. prevModel is separate so a moving object produces motion
// vectors even when the camera is still.
struct GpuObjectData {
    Mat4 model{1.0f};
    Mat4 prevModel{1.0f};
    // Inverse transpose of model. Stored as 4x4 because a 3x3 in a buffer pads
    // to the same size anyway and the wider type avoids a packing surprise.
    Mat4 normalMatrix{1.0f};

    std::uint32_t materialIndex = 0;
    std::uint32_t padding[3]{};
};

struct GpuMaterialData {
    Vec4 baseColorFactor{1.0f};
    Vec4 emissiveFactor{0.0f};   // w unused

    float metallicFactor  = 1.0f;
    float roughnessFactor = 1.0f;
    float padding[2]{};

    // Bindless texture slots; kInvalidTexture means "use the factor alone".
    std::uint32_t baseColorTexture         = 0xFFFFFFFFu;
    std::uint32_t normalTexture            = 0xFFFFFFFFu;
    std::uint32_t metallicRoughnessTexture = 0xFFFFFFFFu;
    std::uint32_t emissiveTexture          = 0xFFFFFFFFu;
};

// What a draw needs to reach everything else. Five uint32 fits comfortably in
// the 128 bytes of push constants every device guarantees.
struct GBufferPushConstants {
    std::uint32_t frameBuffer    = 0;
    std::uint32_t objectBuffer   = 0;
    std::uint32_t materialBuffer = 0;
    std::uint32_t vertexBuffer   = 0;
    std::uint32_t objectIndex    = 0;
    std::uint32_t padding[3]{};
};

// What the lighting pass needs to find the GBuffer and the lights.
struct LightingPushConstants {
    std::uint32_t frameBuffer     = 0;
    std::uint32_t lightBuffer     = 0;
    std::uint32_t albedoTexture   = 0;
    std::uint32_t normalTexture   = 0;
    std::uint32_t materialTexture = 0;
    std::uint32_t depthTexture    = 0;
    std::uint32_t environmentBuffer = 0;
    std::uint32_t padding          = 0;
};

static_assert(sizeof(LightingPushConstants) == 32, "must match Common.hlsli");

inline constexpr std::uint32_t kInvalidTextureIndex = 0xFFFFFFFFu;

// Bindless sampler slots the renderer registers at startup.
enum class SamplerSlot : std::uint32_t {
    LinearRepeat = 0,  // material textures
    PointClamp   = 1,  // GBuffer reads: no filtering across encoded channels
    LinearClamp  = 2,  // equirectangular sources: latitude must not wrap
};

// Shared by the three cubemap passes. Mirrors CubePush in Common.hlsli.
struct CubePushConstants {
    std::uint32_t sourceTexture = 0;
    std::uint32_t face          = 0;
    float         roughness     = 0.0f;
    std::uint32_t sampleCount   = 0;
};

static_assert(sizeof(CubePushConstants) == 16, "must match Common.hlsli");

// Catch a mirror drift at compile time rather than in a frame.
static_assert(sizeof(GpuFrameData) == 224, "GpuFrameData must match Common.hlsli");
static_assert(sizeof(GpuDirectionalLight) == 48, "GpuDirectionalLight must match Common.hlsli");
static_assert(sizeof(GpuEnvironment) == 64, "GpuEnvironment must match Ibl.hlsli");
static_assert(sizeof(GpuObjectData) == 208, "GpuObjectData must match Common.hlsli");
static_assert(sizeof(GpuMaterialData) == 64, "GpuMaterialData must match Common.hlsli");
static_assert(sizeof(GBufferPushConstants) == 32, "push constants must match Common.hlsli");

} // namespace harpia::rhi
