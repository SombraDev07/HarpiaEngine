#version 450

layout(set = 0, binding = 0, std140) uniform Storm {
    layout(offset = 0)   mat4 inv_view_proj;
    layout(offset = 192) vec4 camera_pos;
    layout(offset = 208) vec4 sun_dir;
    layout(offset = 224) vec4 sun_color;
    layout(offset = 240) vec4 sky_zenith;
    layout(offset = 256) vec4 sky_horizon;
    layout(offset = 464) vec2 inv_extent;
} cb;

layout(location = 0) out vec4 out_color;
layout(location = 1) out float out_depth;

void main() {
    vec2 uv = gl_FragCoord.xy * cb.inv_extent;
    vec2 ndc = uv * 2.0 - 1.0;
    vec4 far4 = cb.inv_view_proj * vec4(ndc, 1.0, 1.0);
    vec3 far_pos = far4.xyz / max(far4.w, 1e-4);
    vec3 dir = normalize(far_pos - cb.camera_pos.xyz);
    vec3 sun = normalize(cb.sun_dir.xyz);

    float up = pow(clamp(dir.y, 0.0, 1.0), 0.55);
    vec3 grad = mix(cb.sky_horizon.rgb, cb.sky_zenith.rgb, vec3(up));
    float sd = clamp(dot(dir, sun), 0.0, 1.0);
    vec3 glow = cb.sun_color.rgb * pow(sd, 900.0) * 1.1;
    vec3 disc = sd > 0.9997 ? cb.sun_color.rgb : vec3(0.0);
    out_color = vec4(grad + glow + disc, 1.0);
    out_depth = 1.0e9;
}
