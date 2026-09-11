// Vegetação: um quad virado à câmara por planta, tirado da lista que o compute
// escreveu. `gl_InstanceIndex` indexa os sobreviventes, não o array original.

#version 450
#extension GL_EXT_nonuniform_qualifier : require

struct Plant {
    vec4 pos_scale;
    vec4 color;
};
layout(set = 3, binding = 0, std430) readonly buffer In { Plant items[]; } src[];
layout(set = 3, binding = 0, std430) readonly buffer Vis { uint items[]; } visible[];

layout(set = 0, binding = 0, std140) uniform Cb {
    layout(offset = 0)   mat4 view_proj;
    layout(offset = 64)  vec4 camera_pos;
    layout(offset = 80)  vec4 sun_dir;
    layout(offset = 96)  vec4 params;
    layout(offset = 112) vec4 hiz;
    // x = slot da lista de visíveis a desenhar.
    layout(offset = 128) vec4 slots;
} cb;

layout(location = 0) out vec3 v_color;
layout(location = 1) out float v_height;

void main() {
    const vec2 QUAD[6] = vec2[6](
        vec2(0, 0), vec2(0, 1), vec2(1, 0),
        vec2(1, 0), vec2(0, 1), vec2(1, 1)
    );
    vec2 q = QUAD[gl_VertexIndex % 6];
    Plant p = src[0].items[visible[uint(cb.slots.x)].items[gl_InstanceIndex]];

    // Virado à câmara em torno do eixo vertical: a direita do quad é
    // perpendicular à direcção da câmara projectada no plano do chão.
    vec3 to_cam = cb.camera_pos.xyz - p.pos_scale.xyz;
    vec3 right = normalize(vec3(-to_cam.z, 0.0, to_cam.x));
    float s = p.pos_scale.w;
    vec3 world = p.pos_scale.xyz
               + right * ((q.x - 0.5) * s)
               + vec3(0.0, q.y * s * 2.0, 0.0);

    v_color = p.color.rgb;
    v_height = q.y;
    gl_Position = cb.view_proj * vec4(world, 1.0);
}
