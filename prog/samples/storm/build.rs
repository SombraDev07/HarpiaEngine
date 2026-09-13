fn main() {
    harpia_shader_build::build(&[
        ("opaque.vs.glsl", "opaque.vs.spv"),
        ("opaque.ps.glsl", "opaque.ps.spv"),
        ("sky.ps.glsl", "sky.ps.spv"),
        ("water.vs.glsl", "water.vs.spv"),
        ("water.ps.glsl", "water.ps.spv"),
        ("copy.ps.glsl", "copy.ps.spv"),
        ("rain.ps.glsl", "rain.ps.spv"),
        ("blit.ps.glsl", "blit.ps.spv"),
        ("../../gates/fog/shaders/fullscreen.vs.spvasm", "fullscreen.vs.spv"),
    ]);
}
