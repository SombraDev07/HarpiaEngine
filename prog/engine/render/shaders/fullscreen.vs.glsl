#version 450

// Fullscreen triangle. Pair with blit.ps.glsl (and other composite PS).
// Shared: do not copy into samples.

void main() {
    vec2 p = vec2((gl_VertexIndex << 1) & 2, gl_VertexIndex & 2);
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
