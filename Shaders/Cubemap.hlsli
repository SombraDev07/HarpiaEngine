// Harpia Engine — cube face geometry
//
// Every cubemap pass renders one face at a time into a view of a single array
// layer, so each pass needs the same thing: given the pixel's UV and which face
// it belongs to, what direction does it look at. That mapping lives here once
// rather than being retyped per pass, because getting a single axis sign wrong
// produces a cubemap that looks plausible until a reflection slides across a
// seam and jumps.
//
// The convention is Vulkan's cube layout: +X, -X, +Y, -Y, +Z, -Z in that order,
// with faces looking outward and V running down.
#ifndef HARPIA_CUBEMAP_HLSLI
#define HARPIA_CUBEMAP_HLSLI

#include "Common.hlsli"

// Direction through the texel at `uv` on the given face. Not normalised by the
// caller's expectation — it is normalised here, because every user wants a unit
// vector and forgetting costs a subtly wrong integral rather than a crash.
float3 cubeFaceDirection(uint face, float2 uv)
{
    // UV covers the face corner to corner; st is the same span centred on zero.
    const float s = 2.0 * uv.x - 1.0;
    const float t = 2.0 * uv.y - 1.0;

    float3 direction;
    switch (face) {
        case 0: direction = float3( 1.0,   -t,   -s); break; // +X
        case 1: direction = float3(-1.0,   -t,    s); break; // -X
        case 2: direction = float3(   s,  1.0,    t); break; // +Y
        case 3: direction = float3(   s, -1.0,   -t); break; // -Y
        case 4: direction = float3(   s,   -t,  1.0); break; // +Z
        default: direction = float3(  -s,   -t, -1.0); break; // -Z
    }
    return normalize(direction);
}

// Equirectangular lookup. Longitude wraps, latitude does not — which is why the
// sampler for the source map must repeat in U and clamp in V. Sampling with
// repeat on both makes the pole bleed to the opposite pole.
float2 directionToEquirect(float3 direction)
{
    const float longitude = atan2(direction.z, direction.x);
    const float latitude  = acos(clamp(direction.y, -1.0, 1.0));

    return float2(longitude / (2.0 * HARPIA_PI) + 0.5,
                  latitude / HARPIA_PI);
}

#endif // HARPIA_CUBEMAP_HLSLI
