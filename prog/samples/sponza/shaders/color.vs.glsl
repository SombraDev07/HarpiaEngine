// VS da Sponza, GPU-driven.
//
// Era um `.spvasm` escrito à mão, de quando não havia toolchain, e lia a matriz do
// mundo de uma push constant — um valor diferente por draw, portanto um draw por
// primitiva. Agora a matriz vem de uma tabela indexada pela primitiva, e a
// primitiva vem do `firstInstance` do comando indirecto.
//
// **`gl_InstanceIndex` e não `gl_DrawID`**: em Vulkan
// `gl_InstanceIndex = firstInstance + nº da instância`, e com `instanceCount = 1`
// o primeiro termo é tudo. Assim não é preciso `shaderDrawParameters` nem a
// extensão que ele obriga — e o índice chega ao shader pelo próprio comando de
// draw, que é onde o culling já escreve.

#version 450
#extension GL_EXT_nonuniform_qualifier : require

layout(location = 0) in vec3 in_pos;
layout(location = 1) in vec3 in_nrm;
layout(location = 2) in vec2 in_uv;

struct Prim {
    mat4 world;
    // x = índice bindless do albedo, y = corte de alfa
    vec4 material;
    vec4 centre;
    vec4 extents;
};
layout(set = 3, binding = 0, std430) readonly buffer Prims { Prim items[]; } prim_buf[];

layout(push_constant) uniform Push { mat4 view_proj; mat4 unused_world; } pc;

layout(location = 0) out vec3 v_world;
layout(location = 1) out vec3 v_nrm;
layout(location = 2) out vec2 v_uv;
// clip.w é a profundidade de vista positiva com `perspective_vk` -- o fog escolhe
// a sua fatia a partir dela.
layout(location = 3) out float v_z;
// `flat`: são constantes por primitiva e interpolá-las não faria sentido nenhum.
layout(location = 4) flat out uint v_albedo;
layout(location = 5) flat out float v_cutoff;

void main() {
    Prim p = prim_buf[0].items[gl_InstanceIndex];
    vec4 world = p.world * vec4(in_pos, 1.0);
    v_world = world.xyz;
    // Normal pela parte 3x3. A Sponza não tem escala não-uniforme; se tivesse,
    // isto precisava da inversa transposta.
    v_nrm = mat3(p.world) * in_nrm;
    v_uv = in_uv;
    v_albedo = uint(p.material.x);
    v_cutoff = p.material.y;
    vec4 clip = pc.view_proj * world;
    v_z = clip.w;
    gl_Position = clip;
}
