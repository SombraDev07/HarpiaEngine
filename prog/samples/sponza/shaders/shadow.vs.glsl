// VS das cascatas, GPU-driven. A mesma tabela do `color.vs`.
#version 450
#extension GL_EXT_nonuniform_qualifier : require

layout(location = 0) in vec3 in_pos;
layout(location = 1) in vec3 in_nrm;
layout(location = 2) in vec2 in_uv;

struct Prim {
    mat4 world;
    vec4 material;
    vec4 centre;
    vec4 extents;
};
layout(set = 3, binding = 0, std430) readonly buffer Prims { Prim items[]; } prim_buf[];

layout(push_constant) uniform Push { mat4 view_proj; mat4 unused_world; } pc;

layout(location = 0) out vec2 v_uv;
layout(location = 1) flat out uint v_albedo;
layout(location = 2) flat out float v_cutoff;

void main() {
    Prim p = prim_buf[0].items[gl_InstanceIndex];
    v_uv = in_uv;
    v_albedo = uint(p.material.x);
    v_cutoff = p.material.y;
    gl_Position = pc.view_proj * (p.world * vec4(in_pos, 1.0));
}
