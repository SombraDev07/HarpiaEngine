// Mais clara em cima, como uma folha apanha mais luz na ponta.
#version 450

layout(location = 0) in vec3 v_color;
layout(location = 1) in float v_height;
layout(location = 0) out vec4 out_color;

void main() {
    out_color = vec4(v_color * (0.45 + 0.55 * v_height), 1.0);
}
