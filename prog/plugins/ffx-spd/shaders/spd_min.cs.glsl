#version 450
#extension GL_GOOGLE_include_directive : enable
#extension GL_EXT_nonuniform_qualifier : require

// FidelityFX SPD with min() — hierarchical Z for SSSR.
// Vulkan depth is 0=near, 1=far; min keeps the closest surface in each tile.

layout(local_size_x = 256, local_size_y = 1, local_size_z = 1) in;

layout(set = 0, binding = 0, std140) uniform SpdCb {
    uint mips;
    uint numWorkGroups;
    ivec2 workGroupOffset;
} spdConstants;

layout(set = 4, binding = 0, r32f) coherent uniform image2D uav[];
layout(set = 3, binding = 0, std430) coherent buffer SpdAtomic {
    uint counter[6];
} spd_atomic[];

#define A_GPU
#define A_GLSL
#define SPD_NO_WAVE_OPERATIONS

#include "ffx_a.h"

shared AU1 spdCounter;
shared AF1 spdIntermediateR[16][16];
shared AF1 spdIntermediateG[16][16];
shared AF1 spdIntermediateB[16][16];
shared AF1 spdIntermediateA[16][16];

AF4 SpdLoadSourceImage(ASU2 p, AU1 slice) {
    return imageLoad(uav[0], p);
}

AF4 SpdLoad(ASU2 p, AU1 slice) {
    return imageLoad(uav[6], p);
}

void SpdStore(ASU2 p, AF4 value, AU1 mip, AU1 slice) {
    if (mip == 5) {
        imageStore(uav[6], p, value);
        return;
    }
    imageStore(uav[mip + 1], p, value);
}

void SpdIncreaseAtomicCounter(AU1 slice) {
    spdCounter = atomicAdd(spd_atomic[9].counter[slice], 1);
}

AU1 SpdGetAtomicCounter() {
    return spdCounter;
}

void SpdResetAtomicCounter(AU1 slice) {
    spd_atomic[9].counter[slice] = 0;
}

AF4 SpdLoadIntermediate(AU1 x, AU1 y) {
    AF1 f = spdIntermediateR[x][y];
    return AF4(f, f, f, f);
}

void SpdStoreIntermediate(AU1 x, AU1 y, AF4 value) {
    spdIntermediateR[x][y] = value.x;
    spdIntermediateG[x][y] = value.y;
    spdIntermediateB[x][y] = value.z;
    spdIntermediateA[x][y] = value.w;
}

AF4 SpdReduce4(AF4 v0, AF4 v1, AF4 v2, AF4 v3) {
    return min(min(v0, v1), min(v2, v3));
}

#include "ffx_spd.h"

void main() {
    SpdDownsample(
        AU2(gl_WorkGroupID.xy),
        AU1(gl_LocalInvocationIndex),
        AU1(spdConstants.mips),
        AU1(spdConstants.numWorkGroups),
        AU1(gl_WorkGroupID.z),
        AU2(spdConstants.workGroupOffset)
    );
}
