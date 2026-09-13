#version 450

layout(set = 0, binding = 0, std140) uniform Storm {
    layout(offset = 320) vec4 wind;  // z unused here
    layout(offset = 368) vec4 wave0;
    layout(offset = 384) vec4 wave1;
    layout(offset = 400) vec4 wave2;
    layout(offset = 416) vec4 wave3;
    layout(offset = 432) vec4 misc;  // x time, y Q
} cb;

layout(push_constant) uniform Push {
    mat4 view_proj;
    mat4 world;
} pc;

layout(location = 0) in vec3 in_pos;
layout(location = 1) in vec3 in_nrm;
layout(location = 0) out vec3 v_world;
layout(location = 1) out vec3 v_nrm;
layout(location = 2) out float v_fold;
layout(location = 3) out float v_z;

const float TAU = 6.28318531;
const float GRAV = 9.81;

void accumulate(vec4 wave, float time, float steep, vec2 xz,
                inout vec3 disp, inout vec3 nrm) {
    vec2 d = normalize(wave.xy);
    float amp = wave.z;
    float len = max(wave.w, 0.01);
    float k = TAU / len;
    float om = sqrt(GRAV * k);
    float phi = k * dot(d, xz) - om * time;
    float c = cos(phi);
    float s = sin(phi);
    float qa = steep * amp;
    disp += vec3(qa * d.x * c, amp * s, qa * d.y * c);
    float ka = k * amp;
    nrm.x -= d.x * ka * c;
    nrm.z -= d.y * ka * c;
    nrm.y -= steep * ka * s;
}

void main() {
    vec3 pos = in_pos;
    float time = cb.misc.x;
    float steep = cb.misc.y;
    vec3 disp = vec3(0.0);
    vec3 nrm = vec3(0.0, 1.0, 0.0);
    accumulate(cb.wave0, time, steep, pos.xz, disp, nrm);
    accumulate(cb.wave1, time, steep, pos.xz, disp, nrm);
    accumulate(cb.wave2, time, steep, pos.xz, disp, nrm);
    accumulate(cb.wave3, time, steep, pos.xz, disp, nrm);
    vec3 world = pos + disp;
    v_world = world;
    v_nrm = normalize(nrm);
    v_fold = nrm.y;
    gl_Position = pc.view_proj * vec4(world, 1.0);
    v_z = gl_Position.w;
}
