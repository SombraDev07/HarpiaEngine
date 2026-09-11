// Mesh shader da Sponza: um workgroup por meshlet, um dispatch para a cena toda.
//
// ## Porque é que isto existe
//
// Os meshlets foram feitos e medidos com draws indirectos clássicos (D58): a
// oclusão passava a cortar 116 unidades em vez de 3, e mesmo assim o frame ficava
// 0.948 ms contra 0.737 — o custo fixo por comando (~0.1 ms por mil) batia o
// ganho em **todos** os tamanhos entre 128 e 2048 triângulos.
//
// Aqui não há comandos. Um `vkCmdDrawMeshTasksEXT` lança um workgroup por
// meshlet, e o custo por unidade passa a ser o de um workgroup de compute — que é
// o que a conta precisava.
//
// ## O formato
//
// Um meshlet não guarda índices globais: guarda até 64 vértices únicos e até 124
// triângulos como índices **locais** de 8 bits. Com índices globais, três
// triângulos vizinhos buscavam o mesmo vértice três vezes; assim o workgroup
// carrega cada vértice uma vez e emite-o uma vez.

#version 450
#extension GL_EXT_mesh_shader : require
#extension GL_EXT_nonuniform_qualifier : require

layout(local_size_x = 64, local_size_y = 1, local_size_z = 1) in;
layout(triangles, max_vertices = 64, max_primitives = 124) out;

struct Prim {
    mat4 world;
    vec4 material;   // x = índice bindless do albedo, y = corte de alfa
    vec4 centre;
    vec4 extents;
};

// slot 0 = tabela por meshlet · 1 = comandos (o `instanceCount` diz se é visível)
// 5 = vértices · 6 = índices de vértice por meshlet · 7 = triângulos · 8 = intervalos
layout(set = 3, binding = 0, std430) readonly buffer Prims { Prim items[]; } prim_buf[];
layout(set = 3, binding = 0, std430) readonly buffer Args { uint v[]; } args[];
layout(set = 3, binding = 0, std430) readonly buffer Verts { float v[]; } vert_buf[];
layout(set = 3, binding = 0, std430) readonly buffer MVerts { uint v[]; } mvert_buf[];
layout(set = 3, binding = 0, std430) readonly buffer MTris { uint v[]; } mtri_buf[];
layout(set = 3, binding = 0, std430) readonly buffer Ranges { uvec4 v[]; } range_buf[];

// A segunda matriz não é uma matriz: `[0][0]` traz o **meshlet base** deste
// dispatch. `vkCmdDrawMeshTasksEXT` não tem offset de workgroup, e a cena é
// desenhada em dois lances — os de uma face e os de duas, que são dois PSOs.
layout(push_constant) uniform Push { mat4 view_proj; mat4 extra; } pc;

// A mesma interface que o `color.vs` dá ao `color.ps`: o pixel shader não muda.
layout(location = 0) out vec3 v_world[];
layout(location = 1) out vec3 v_nrm[];
layout(location = 2) out vec2 v_uv[];
layout(location = 3) out float v_z[];
layout(location = 4) perprimitiveEXT flat out uint v_albedo[];
layout(location = 5) perprimitiveEXT flat out float v_cutoff[];

const uint SLOT_PRIMS = 0u;
const uint SLOT_ARGS = 1u;
const uint SLOT_VERTS = 5u;
const uint SLOT_MVERTS = 6u;
const uint SLOT_MTRIS = 7u;
const uint SLOT_RANGES = 8u;
/// Oito floats por vértice: posição, normal, uv.
const uint VERT_FLOATS = 8u;

void main() {
    uint mid = gl_WorkGroupID.x + uint(pc.extra[0][0]);

    // Cortado pelo culling? Um workgroup que não emite nada custa o lançamento e
    // mais nada -- que é toda a diferença face a um comando de draw com
    // `instanceCount` a zero.
    if (args[SLOT_ARGS].v[mid * 5u + 1u] == 0u) {
        SetMeshOutputsEXT(0u, 0u);
        return;
    }

    uvec4 r = range_buf[SLOT_RANGES].v[mid];
    // **Limitados**, e não por desconfiança da tabela: escrever para lá do que o
    // `layout(max_vertices/max_primitives)` declara é comportamento indefinido, e
    // em RADV o que acontece é a GPU pendurar. Custou-me um reset do driver e a
    // sessão gráfica para aprender que uma guarda de duas instruções aqui vale
    // mais do que a certeza de que a tabela está certa.
    uint vcount = min(r.y, 64u);
    uint tcount = min(r.w, 124u);
    SetMeshOutputsEXT(vcount, tcount);

    Prim p = prim_buf[SLOT_PRIMS].items[mid];
    uint albedo = uint(p.material.x);
    float cutoff = p.material.y;

    // Os vértices, um por lane e a dar a volta se forem mais de 64.
    for (uint i = gl_LocalInvocationIndex; i < vcount; i += 64u) {
        uint gv = mvert_buf[SLOT_MVERTS].v[r.x + i];
        uint b = gv * VERT_FLOATS;
        vec3 pos = vec3(vert_buf[SLOT_VERTS].v[b], vert_buf[SLOT_VERTS].v[b + 1u],
                        vert_buf[SLOT_VERTS].v[b + 2u]);
        vec3 nrm = vec3(vert_buf[SLOT_VERTS].v[b + 3u], vert_buf[SLOT_VERTS].v[b + 4u],
                        vert_buf[SLOT_VERTS].v[b + 5u]);
        vec2 uv = vec2(vert_buf[SLOT_VERTS].v[b + 6u], vert_buf[SLOT_VERTS].v[b + 7u]);

        vec4 world = p.world * vec4(pos, 1.0);
        vec4 clip = pc.view_proj * world;
        gl_MeshVerticesEXT[i].gl_Position = clip;
        v_world[i] = world.xyz;
        v_nrm[i] = mat3(p.world) * nrm;
        v_uv[i] = uv;
        // `clip.w` é a profundidade de vista positiva com `perspective_vk`.
        v_z[i] = clip.w;
    }

    // E os triângulos, desempacotando os três índices locais de 8 bits.
    for (uint t = gl_LocalInvocationIndex; t < tcount; t += 64u) {
        uint packed = mtri_buf[SLOT_MTRIS].v[r.z + t];
        gl_PrimitiveTriangleIndicesEXT[t] =
            uvec3(packed & 0xffu, (packed >> 8u) & 0xffu, (packed >> 16u) & 0xffu);
        // Por primitiva e não por vértice: o albedo é constante no meshlet inteiro,
        // e escrevê-lo por vértice gastaria interpoladores para nada.
        v_albedo[t] = albedo;
        v_cutoff[t] = cutoff;
    }
}
