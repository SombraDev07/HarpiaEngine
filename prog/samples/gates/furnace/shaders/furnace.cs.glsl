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
// A imagem tem dois painéis de N x N, lado a lado.
//
// Esquerda, isotrópico:
//   r = G de Smith com a aproximação de Schlick (k = (a+1)^2/8)  <- o que tínhamos
//   g = G de Smith height-correlated                             <- a forma exacta
//   b = height-correlated + compensação de multiscatter          <- deve dar 1.0
//
// Meio, anisotrópico (Heitz 2014):
//   r = alpha_x = alpha_y                <- tem de dar o mesmo que o `g` da esquerda
//   g = alpha_y = alpha_x/4, vista ao longo da tangente
//   b = o mesmo, vista ao longo da bitangente
//
// Direita, o **mesmo material com os eixos trocados**, visto do outro lado:
//   r = E(ay, ax) com a vista na bitangente   <- tem de dar o `g` do meio
//   g = E(ay, ax) com a vista na tangente     <- tem de dar o `b` do meio
//
// Este terceiro painel é o que apanha um modelo anisotrópico a sério. Trocar
// alpha_x com alpha_y e rodar a vista 90 graus é relabelar os eixos: qualquer
// modelo correcto devolve o mesmo número. Um Lambda que use `ax` nas duas
// componentes passa a redução (com ax == ay não se nota) e passa o teste de «a
// anisotropia faz alguma coisa» (faz, só que a errada) -- e falha este. Escrevi
// as duas primeiras verificações antes desta, quebrei o Lambda de propósito, e
// **passaram as duas**. Foi essa a razão de existir deste painel.

#version 450

// O binding 0 do set 4 é um **array** de slots, não um descritor só: uma
// pirâmide escreve-se nível a nível e cada nível é um slot.
layout(set = 4, binding = 0, rgba16f) uniform writeonly image2D result[];

layout(set = 0, binding = 0, std140) uniform Furnace {
    layout(offset = 0) vec4 params;   // x = N da grelha, y = amostras, z = razão de anisotropia
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

// GGX anisotrópica, amostrada pela NDF.
//
// O phi sai de `atan2(ay sin t, ax cos t)`, que é a inversão da marginal em phi
// escrita sem `tan`/`atan` -- a forma com `tan(2*pi*xi + pi/2)` tem singularidades
// em xi = 0 e xi = 0.5, e a sequência de Hammersley acerta nas duas em cheio.
vec3 importance_sample_ggx_aniso(vec2 xi, float ax, float ay) {
    float t = 2.0 * PI * xi.x;
    vec2 d = normalize(vec2(ax * cos(t), ay * sin(t)));
    float a2 = 1.0 / (d.x * d.x / (ax * ax) + d.y * d.y / (ay * ay));
    float tan2 = a2 * xi.y / max(1.0 - xi.y, 1e-6);
    float cos_theta = inversesqrt(1.0 + tan2);
    float sin_theta = sqrt(max(1.0 - cos_theta * cos_theta, 0.0));
    return normalize(vec3(sin_theta * d.x, sin_theta * d.y, cos_theta));
}

// Lambda de Smith para a GGX anisotrópica. Com ax == ay reduz-se ao isotrópico,
// e é isso que o painel da direita mede.
float lambda_aniso(vec3 w, float ax, float ay) {
    float a2 = (ax * ax * w.x * w.x + ay * ay * w.y * w.y) / max(w.z * w.z, 1e-12);
    return 0.5 * (sqrt(1.0 + a2) - 1.0);
}

// G2 height-correlated, a mesma forma do isotrópico escrita em Lambda.
float g2_aniso(vec3 v, vec3 l, float ax, float ay) {
    return 1.0 / (1.0 + lambda_aniso(v, ax, ay) + lambda_aniso(l, ax, ay));
}

// O albedo direccional anisotrópico, com a vista num azimute dado.
float e_aniso(float ndv, float phi_v, float ax, float ay, uint samples) {
    float sin_v = sqrt(max(1.0 - ndv * ndv, 0.0));
    vec3 v = vec3(sin_v * cos(phi_v), sin_v * sin(phi_v), ndv);
    float e = 0.0;
    for (uint i = 0u; i < samples; ++i) {
        vec3 h = importance_sample_ggx_aniso(hammersley(i, samples), ax, ay);
        vec3 l = 2.0 * dot(v, h) * h - v;
        if (l.z <= 0.0) continue;
        float ndh = max(h.z, 1e-6);
        float vdh = max(dot(v, h), 1e-6);
        // O mesmo estimador do isotrópico: com pdf = D*NoH/(4 VoH) o D cancela.
        e += g2_aniso(v, l, ax, ay) * vdh / (ndv * ndh);
    }
    return e / float(samples);
}

void main() {
    ivec2 id = ivec2(gl_GlobalInvocationID.xy);
    int n = int(cb.params.x);
    if (id.x >= 3 * n || id.y >= n) return;
    uint samples = uint(cb.params.y);

    int panel = id.x / n;
    int col = id.x - panel * n;
    // NoV nunca chega a 0: a rasar, o integral é singular e mede ruído.
    float ndv = max((float(col) + 0.5) / float(n), 0.02);
    // alpha = roughness^2, varrido em roughness para dar resolução onde interessa.
    float roughness = max((float(id.y) + 0.5) / float(n), 0.02);
    float alpha = roughness * roughness;

    float ratio = max(cb.params.z, 1.0);
    float ax = alpha;
    float ay = max(alpha / ratio, 1e-3);

    if (panel == 1) {
        imageStore(result[0], id, vec4(
            e_aniso(ndv, 0.0, alpha, alpha, samples),
            e_aniso(ndv, 0.0, ax, ay, samples),
            e_aniso(ndv, 0.5 * PI, ax, ay, samples),
            1.0));
        return;
    }
    if (panel == 2) {
        // Os eixos trocados e a vista rodada 90 graus: o mesmo material.
        imageStore(result[0], id, vec4(
            e_aniso(ndv, 0.5 * PI, ay, ax, samples),
            e_aniso(ndv, 0.0, ay, ax, samples),
            0.0,
            1.0));
        return;
    }

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

    imageStore(result[0], id, vec4(e_schlick, e_corr, e_comp, 1.0));
}
