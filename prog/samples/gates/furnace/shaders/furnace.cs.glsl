// White furnace test: mede se o nosso BRDF especular conserva energia.
//
// Numa "fornalha branca" a radiância incidente é 1.0 em todas as direcções. Um
// BRDF que não absorve tem, por definição, de devolver exactamente 1.0 --
// qualquer valor abaixo é energia que o modelo perdeu e que o material devia ter
// reflectido. É um teste que não depende de gosto nem de screenshot: ou dá 1.0
// ou não dá.
//
// Mede-se o albedo direccional
//     E(NoV, alpha) = integral de f(v,l) * NoL  dl,  com F = 1
// por importance sampling da GGX, onde o estimador se reduz a
//     peso = G * VoH / (NoV * NoH).
//
// Três canais, para comparar modelos lado a lado:
//   r = G de Smith com a aproximação de Schlick (k = (a+1)^2/8)  <- o que temos
//   g = G de Smith height-correlated                             <- a forma exacta
//   b = height-correlated + compensação de multiscatter          <- deve dar 1.0

#version 450

layout(set = 4, binding = 0, rgba16f) uniform writeonly image2D result;

layout(set = 0, binding = 0, std140) uniform Furnace {
    layout(offset = 0) vec4 params;   // x = N da grelha, y = amostras
} cb;

layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;

const float PI = 3.14159265359;

// Van der Corput radical inverse, base 2.
float radical_inverse(uint bits) {
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return float(bits) * 2.3283064365386963e-10;
}

vec2 hammersley(uint i, uint n) {
    return vec2(float(i) / float(n), radical_inverse(i));
}

// Meio-vector segundo a distribuição GGX, em espaço tangente (normal = +Z).
vec3 importance_sample_ggx(vec2 xi, float alpha) {
    float phi = 2.0 * PI * xi.x;
    float cos_theta = sqrt((1.0 - xi.y) / (1.0 + (alpha * alpha - 1.0) * xi.y));
    float sin_theta = sqrt(max(1.0 - cos_theta * cos_theta, 0.0));
    return vec3(sin_theta * cos(phi), sin_theta * sin(phi), cos_theta);
}

// O que a Harpia usa hoje: Smith separável com a aproximação de Schlick.
// O k = (a+1)^2/8 vem do UE4 e foi ajustado para luzes analíticas, não para
// integrar sobre a hemisfera -- é por isso que se espera que perca energia aqui.
float g_smith_schlick(float ndv, float ndl, float alpha) {
    float ap = alpha + 1.0;
    float k = (ap * ap) / 8.0;
    float gv = ndv / (ndv * (1.0 - k) + k);
    float gl = ndl / (ndl * (1.0 - k) + k);
    return gv * gl;
}

// A forma correcta sob o modelo de microsuperfície de Smith (Heitz 2014): o
// mascaramento e o sombreamento estão correlacionados pela altura, e tratá-los
// como independentes é uma aproximação.
//
// Devolve G (não G/(4 NoV NoL)), para o estimador ser o mesmo dos dois lados.
float g_smith_correlated(float ndv, float ndl, float alpha) {
    float a2 = alpha * alpha;
    float lambda_v = ndl * sqrt(ndv * ndv * (1.0 - a2) + a2);
    float lambda_l = ndv * sqrt(ndl * ndl * (1.0 - a2) + a2);
    float denom = lambda_v + lambda_l;
    // V = G / (4 NoV NoL); multiplicar de volta dá G.
    float vis = denom > 0.0 ? (0.5 / denom) : 0.0;
    return vis * 4.0 * ndv * ndl;
}

void main() {
    ivec2 id = ivec2(gl_GlobalInvocationID.xy);
    int n = int(cb.params.x);
    if (id.x >= n || id.y >= n) return;
    uint samples = uint(cb.params.y);

    // NoV nunca chega a 0: a rasar, o integral é singular e mede ruído.
    float ndv = max((float(id.x) + 0.5) / float(n), 0.02);
    // alpha = roughness^2, varrido em roughness para dar resolução onde interessa.
    float roughness = max((float(id.y) + 0.5) / float(n), 0.02);
    float alpha = roughness * roughness;

    vec3 v = vec3(sqrt(max(1.0 - ndv * ndv, 0.0)), 0.0, ndv);

    float e_schlick = 0.0;
    float e_corr = 0.0;
    for (uint i = 0u; i < samples; ++i) {
        vec3 h = importance_sample_ggx(hammersley(i, samples), alpha);
        vec3 l = 2.0 * dot(v, h) * h - v;
        float ndl = l.z;
        if (ndl <= 0.0) continue;
        float ndh = max(h.z, 1e-6);
        float vdh = max(dot(v, h), 1e-6);
        // pdf = D * NoH / (4 VoH); com f = D*G*F/(4 NoV NoL) e F = 1 fica isto.
        float w = vdh / (ndv * ndh);
        e_schlick += g_smith_schlick(ndv, ndl, alpha) * w;
        e_corr += g_smith_correlated(ndv, ndl, alpha) * w;
    }
    e_schlick /= float(samples);
    e_corr /= float(samples);

    // Compensação de multiscatter (Fdez-Agüera): devolve à superfície a energia
    // que um único salto perdeu. Com F0 = 1 tem de dar exactamente 1.0 -- se não
    // der, a compensação está mal ligada.
    float e_comp = e_corr > 0.0 ? e_corr * (1.0 + 1.0 * (1.0 / e_corr - 1.0)) : 0.0;

    imageStore(result, id, vec4(e_schlick, e_corr, e_comp, 1.0));
}
