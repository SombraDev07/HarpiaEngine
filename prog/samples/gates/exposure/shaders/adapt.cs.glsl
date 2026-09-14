#version 450

#extension GL_EXT_nonuniform_qualifier : require

// One thread: turn the atomic luma sum into a 1×1 exposure and reset the counters.

layout(local_size_x = 1, local_size_y = 1, local_size_z = 1) in;

layout(set = 0, binding = 0, std140) uniform Adapt {
    vec4 params; // x = dt, y = speed, z = acc slot
} cb;

layout(set = 3, binding = 0, std430) buffer Acc { uint v[]; } acc[];
layout(set = 4, binding = 0, r32f) uniform image2D uav[];

const uint EXP_SLOT = 14u;
const float GREY = 0.18;
const float EMIN = 0.05;
const float EMAX = 12.0;

void main() {
    uint slot = uint(cb.params.z);
    uint sum = acc[slot].v[0];
    uint count = max(acc[slot].v[1], 1u);
    float avg = float(sum) / (256.0 * float(count));
    float target = clamp(GREY / max(avg, 1e-4), EMIN, EMAX);
    float prev = imageLoad(uav[EXP_SLOT], ivec2(0, 0)).r;
    if (prev < EMIN * 0.5) {
        prev = 1.0;
    }
    float w = 1.0 - exp(-cb.params.x * cb.params.y);
    float next = prev + (target - prev) * clamp(w, 0.0, 1.0);
    imageStore(uav[EXP_SLOT], ivec2(0, 0), vec4(next, 0.0, 0.0, 1.0));
    acc[slot].v[0] = 0u;
    acc[slot].v[1] = 0u;
}
