// Aerial perspective 32³. One thread per froxel: march from the camera to the
// slice's end along the pixel ray, write (in-scatter.rgb, transmittance).
//
// The far plane is 2 km (clipmap range), not Hillaire's 32 km. A 32 km volume
// on this world would leave the hills in slice 0 with no haze. `mie.w` scales
// the coefficients so a kilometre of air is visible; the numbers are still
// Rayleigh/Mie, not a mix-to-skyview fade.

#version 450

layout(local_size_x = 8, local_size_y = 8, local_size_z = 4) in;

layout(set = 4, binding = 1, rgba16f) uniform writeonly image3D uav[];

layout(set = 0, binding = 0, std140) uniform Aerial {
    layout(offset = 0)   mat4 inv_view_proj;
    layout(offset = 64)  vec4 camera_pos;       // xyz metres, w altitude km
    layout(offset = 80)  vec4 sun_dir;
    layout(offset = 96)  vec4 sun_illuminance;
    layout(offset = 112) vec4 radii;            // bottom, top, mie g, far km
    layout(offset = 128) vec4 rayleigh;         // rgb km^-1, H km
    layout(offset = 144) vec4 mie;              // scatter, absorb, H, density scale
    layout(offset = 160) vec4 steps;            // march count
} cb;

const float PI = 3.14159265;
const uint SIZE = 32u;

void main() {
    uvec3 id = gl_GlobalInvocationID;
    if (id.x >= SIZE || id.y >= SIZE || id.z >= SIZE) {
        return;
    }

    vec2 uv = (vec2(id.xy) + 0.5) / float(SIZE);
    float tz = (float(id.z) + 0.5) / float(SIZE);
    vec2 ndc = uv * 2.0 - 1.0;
    vec4 far4 = cb.inv_view_proj * vec4(ndc, 1.0, 1.0);
    vec3 far_p = far4.xyz / max(far4.w, 1e-6);
    vec3 cam = cb.camera_pos.xyz;
    vec3 dir = normalize(far_p - cam);

    float far_m = max(cb.radii.w, 0.01) * 1000.0;
    float end_m = tz * far_m;
    int n = max(int(cb.steps.x), 1);
    float dt_m = end_m / float(n);
    float dt_km = dt_m * 0.001;
    float scale = max(cb.mie.w, 0.0);

    vec3 sun = normalize(cb.sun_dir.xyz);
    float mu = dot(dir, sun);
    float pr = (3.0 / (16.0 * PI)) * (1.0 + mu * mu);
    float g = cb.radii.z;
    float g2 = g * g;
    float hd = max(1.0 + g2 - 2.0 * g * mu, 1e-4);
    float pm = (1.0 - g2) / (4.0 * PI * pow(hd, 1.5));

    vec3 tr = vec3(1.0);
    vec3 insc = vec3(0.0);
    for (int i = 0; i < n; i++) {
        float t = (float(i) + 0.5) * dt_m;
        vec3 p = cam + dir * t;
        float h = max(p.y, 0.0) * 0.001;
        float hr = exp(-h / max(cb.rayleigh.w, 1e-4));
        float hm = exp(-h / max(cb.mie.z, 1e-4));
        vec3 beta_s = (cb.rayleigh.rgb * hr + vec3(cb.mie.x) * hm) * scale;
        vec3 beta_t = beta_s + vec3(cb.mie.y) * hm * scale;
        vec3 phase_s = (cb.rayleigh.rgb * hr * pr + vec3(cb.mie.x) * hm * pm) * scale;
        insc += tr * phase_s * cb.sun_illuminance.rgb * dt_km;
        tr *= exp(-beta_t * dt_km);
    }

    float tavg = (tr.r + tr.g + tr.b) * (1.0 / 3.0);
    imageStore(uav[0], ivec3(id), vec4(insc, tavg));
}
