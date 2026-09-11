// Pirâmide de profundidade máxima (Hi-Z), um nível por dispatch.
//
// Guarda-se o **máximo** e não o mínimo: um patch de vegetação só está oclusa se
// o seu ponto mais próximo estiver atrás do oclusor **mais distante** de cada
// texel que ela cobre. Guardar o mínimo cortaria geometria que se vê.
//
// O nível 0 copia o depth buffer; os seguintes são o máximo dos quatro filhos.
// Dimensões ímpares tratam-se com `min` no índice — sem isso, a última coluna de
// um nível ímpar perdia-se e o máximo saía pequeno de mais, que é o lado errado
// do erro.

#version 450
#extension GL_EXT_nonuniform_qualifier : require

// O binding 0 do set 4 é um **array** de slots, não um descritor só: uma
// pirâmide escreve-se nível a nível e cada nível é um slot.
layout(set = 4, binding = 0, r32f) uniform writeonly image2D dst[];
layout(set = 1, binding = 0) uniform texture2D heap[];
layout(set = 2, binding = 1) uniform sampler samp_clamp;

layout(set = 0, binding = 0, std140) uniform Hiz {
    // x = índice bindless da origem, y = largura do destino, z = altura do destino,
    // w = 1 se a origem é o depth buffer (nível 0)
    layout(offset = 0) vec4 params;
    // x = largura da origem, y = altura da origem, z = slot de destino,
    // w = **mip de origem**
    layout(offset = 16) vec4 src_size;
} cb;

layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;

void main() {
    ivec2 id = ivec2(gl_GlobalInvocationID.xy);
    int dw = int(cb.params.y);
    int dh = int(cb.params.z);
    if (id.x >= dw || id.y >= dh) return;

    int sw = int(cb.src_size.x);
    int sh = int(cb.src_size.y);
    uint src = uint(cb.params.x);

    float d;
    if (cb.params.w > 0.5) {
        // Nível 0: uma cópia do depth buffer, texel a texel.
        d = texelFetch(sampler2D(heap[nonuniformEXT(src)], samp_clamp), min(id, ivec2(sw - 1, sh - 1)), 0).r;
    } else {
        ivec2 b = id * 2;
        ivec2 m = ivec2(sw - 1, sh - 1);
        // O **mip de origem**, não o 0.
        //
        // Estava `0` em todas as leituras, portanto o nível 2 lia texels do nível
        // 0 nas coordenadas do nível 1: a pirâmide era lixo a partir do primeiro
        // nível. O sintoma foram 60 pixels de erva a desaparecer no topo da banda,
        // espalhados — e nenhum erro de validation, porque ler o mip errado é
        // perfeitamente legal.
        int sm = int(cb.src_size.w);
        float a0 = texelFetch(sampler2D(heap[nonuniformEXT(src)], samp_clamp), min(b, m), sm).r;
        float a1 = texelFetch(sampler2D(heap[nonuniformEXT(src)], samp_clamp), min(b + ivec2(1, 0), m), sm).r;
        float a2 = texelFetch(sampler2D(heap[nonuniformEXT(src)], samp_clamp), min(b + ivec2(0, 1), m), sm).r;
        float a3 = texelFetch(sampler2D(heap[nonuniformEXT(src)], samp_clamp), min(b + ivec2(1, 1), m), sm).r;
        d = max(max(a0, a1), max(a2, a3));
    }
    imageStore(dst[uint(cb.src_size.z)], id, vec4(d, 0.0, 0.0, 0.0));
}
