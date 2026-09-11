// Iluminação por clusters, com um interruptor para força-bruta.
//
// O mesmo shader faz as duas coisas de propósito: se a versão clustered e a
// versão que percorre todas as luzes forem shaders diferentes, uma divergência
// pode ser diferença de código e não erro de clustering. Sendo o mesmo corpo com
// uma lista diferente, qualquer diferença na imagem é do clustering.
//
// É esse o gate que a Dagor não tem: eles têm o sistema, nós temos a prova de
// que a grelha atribui as luzes certas.

#version 450
#extension GL_EXT_nonuniform_qualifier : require

// Omni e spot são a mesma estrutura: uma omni é um spot com o cone aberto à
// esfera toda, que é o que `cos_outer = -1` diz. Uma lista, um percurso.
struct Light {
    vec4 position_radius;   // xyz posição, w raio
    vec4 color;             // rgb cor, w cosseno do ângulo interior
    vec4 dir_cos_outer;     // xyz direcção, w cosseno do exterior (-1 = omni)
};

struct ClusterRange {
    uint offset;
    uint count;
};

// O binding 0 do set 3 é um **array** de storage buffers, tal como o set 1 é um
// array de texturas. Três tipos diferentes sobre o mesmo array é o mesmo padrão
// que já usamos para o heap: o slot é que escolhe qual é qual.
//   slot 0 = luzes · slot 1 = intervalos por cluster · slot 2 = índices
layout(set = 3, binding = 0, std430) readonly buffer Lights { Light lights[]; } light_buf[];
layout(set = 3, binding = 0, std430) readonly buffer Ranges { ClusterRange ranges[]; } range_buf[];
layout(set = 3, binding = 0, std430) readonly buffer Indices { uint indices[]; } index_buf[];

layout(set = 0, binding = 0, std140) uniform Lit {
    layout(offset = 0)   mat4 inv_view_proj;
    layout(offset = 64)  mat4 view;
    layout(offset = 128) vec4 camera_pos;
    layout(offset = 144) vec4 sun_dir;
    layout(offset = 160) vec4 sun_color;
    // x = near, y = far, z = nº de luzes, w = 1 se for força-bruta
    layout(offset = 176) vec4 params;
    // x,y = dimensões do ecrã, z,w = 1/dimensões
    layout(offset = 192) vec4 screen;
    layout(offset = 208) vec4 cluster_dims;   // x,y,z = grelha
} cb;

layout(location = 0) in vec3 v_world;
layout(location = 1) in vec3 v_normal;
layout(location = 2) in vec3 v_albedo;
layout(location = 3) in vec2 v_material;      // x = rugosidade, y = metálico

layout(location = 0) out vec4 out_color;

const float PI = 3.14159265359;

float d_ggx(float ndh, float alpha) {
    float a2 = alpha * alpha;
    float d = ndh * ndh * (a2 - 1.0) + 1.0;
    return a2 / max(PI * d * d, 1e-7);
}

// Smith height-correlated -- a mesma forma que o gate `furnace` provou ser a
// correcta (D44). Devolve V = G / (4 NoV NoL), que é como entra no BRDF.
float v_smith_correlated(float ndv, float ndl, float alpha) {
    float a2 = alpha * alpha;
    float lv = ndl * sqrt(ndv * ndv * (1.0 - a2) + a2);
    float ll = ndv * sqrt(ndl * ndl * (1.0 - a2) + a2);
    return 0.5 / max(lv + ll, 1e-6);
}

vec3 f_schlick(vec3 f0, float vdh) {
    return f0 + (vec3(1.0) - f0) * pow(1.0 - vdh, 5.0);
}

vec3 shade(vec3 n, vec3 v, vec3 l, vec3 radiance, vec3 albedo, float rough, float metal) {
    float ndl = max(dot(n, l), 0.0);
    if (ndl <= 0.0) return vec3(0.0);
    vec3 h = normalize(v + l);
    float ndv = max(dot(n, v), 1e-4);
    float ndh = max(dot(n, h), 0.0);
    float vdh = max(dot(v, h), 0.0);
    float alpha = max(rough * rough, 1e-3);

    vec3 f0 = mix(vec3(0.04), albedo, metal);
    vec3 spec = f_schlick(f0, vdh) * d_ggx(ndh, alpha) * v_smith_correlated(ndv, ndl, alpha);
    vec3 kd = (vec3(1.0) - f_schlick(f0, ndv)) * (1.0 - metal);
    vec3 diff = kd * albedo / PI;
    return (diff + spec) * radiance * ndl;
}

// Queda suave que chega exactamente a zero no raio. Sem o corte, uma luz
// contribuiria fora do seu cluster e o clustered discordaria da força-bruta --
// por razão legítima, o que tornaria o gate inútil.
float attenuation(float dist, float radius) {
    float t = clamp(1.0 - pow(dist / max(radius, 1e-4), 4.0), 0.0, 1.0);
    return (t * t) / (dist * dist + 1.0);
}

// Queda do cone, entre o ângulo interior e o exterior.
//
// `l` aponta do ponto **para** a luz, e a direcção do cone aponta para fora dela:
// por isso o cosseno que interessa é o de `-l` contra a direcção. Trocar o sinal
// aqui acende tudo o que está atrás do projector e nada do que está à frente, e é
// um erro que numa imagem parece «o cone está do lado errado» e não «o sinal está
// trocado» -- por isso é o teste de correcção que o apanha, não o olho.
//
// Tem de chegar exactamente a zero no ângulo exterior, pela mesma razão que a
// atenuação radial: o teste de cone no lado da CPU corta ali, e se o shader ainda
// contribuísse um pouco para lá disso, o clustered discordava da força-bruta.
float cone_falloff(vec3 l, vec4 dir_cos_outer, float cos_inner) {
    float cos_outer = dir_cos_outer.w;
    if (cos_outer <= -1.0) return 1.0;          // omni
    float cd = dot(-l, dir_cos_outer.xyz);
    // `max` no denominador: com interior == exterior a queda é um degrau, e sem
    // isto seria uma divisão por zero.
    return clamp((cd - cos_outer) / max(cos_inner - cos_outer, 1e-4), 0.0, 1.0);
}

void main() {
    vec3 n = normalize(v_normal);
    vec3 v = normalize(cb.camera_pos.xyz - v_world);
    float rough = v_material.x;
    float metal = v_material.y;

    // Sol, sempre.
    vec3 sun = normalize(cb.sun_dir.xyz);
    vec3 color = shade(n, v, sun, cb.sun_color.rgb, v_albedo, rough, metal);
    color += v_albedo * 0.03;   // ambiente mínimo, para o não-iluminado não ser preto

    uint light_count = uint(cb.params.z);
    bool brute = cb.params.w > 0.5;

    if (brute) {
        for (uint i = 0u; i < light_count; ++i) {
            Light lt = light_buf[0].lights[i];
            vec3 d = lt.position_radius.xyz - v_world;
            float dist = length(d);
            if (dist >= lt.position_radius.w) continue;
            vec3 l = d / max(dist, 1e-4);
            float cone = cone_falloff(l, lt.dir_cos_outer, lt.color.w);
            if (cone <= 0.0) continue;
            color += shade(n, v, l,
                           lt.color.rgb * (attenuation(dist, lt.position_radius.w) * cone),
                           v_albedo, rough, metal);
        }
    } else {
        // Cluster deste pixel: XY do ecrã, Z pela profundidade de vista.
        vec3 dims = cb.cluster_dims.xyz;
        vec2 uv = gl_FragCoord.xy * cb.screen.zw;
        float near = cb.params.x;
        float far = cb.params.y;
        float view_z = -(cb.view * vec4(v_world, 1.0)).z;

        uint cx = uint(clamp(uv.x * dims.x, 0.0, dims.x - 1.0));
        uint cy = uint(clamp(uv.y * dims.y, 0.0, dims.y - 1.0));
        float slice = view_z <= near ? 0.0
                    : log(view_z / near) / log(far / near) * dims.z;
        uint cz = uint(clamp(slice, 0.0, dims.z - 1.0));

        uint cluster = (cz * uint(dims.y) + cy) * uint(dims.x) + cx;
        ClusterRange r = range_buf[1].ranges[cluster];
        for (uint i = 0u; i < r.count; ++i) {
            Light lt = light_buf[0].lights[index_buf[2].indices[r.offset + i]];
            vec3 d = lt.position_radius.xyz - v_world;
            float dist = length(d);
            if (dist >= lt.position_radius.w) continue;
            vec3 l = d / max(dist, 1e-4);
            float cone = cone_falloff(l, lt.dir_cos_outer, lt.color.w);
            if (cone <= 0.0) continue;
            color += shade(n, v, l,
                           lt.color.rgb * (attenuation(dist, lt.position_radius.w) * cone),
                           v_albedo, rough, metal);
        }
    }

    out_color = vec4(color, 1.0);
}
