fn main() {
    harpia_shader_build::build(&[
        ("scene.vs.spvasm", "scene.vs.spv"),
        ("scene.ps.spvasm", "scene.ps.spv"),
        ("rain.ps.spvasm", "rain.ps.spv"),
        ("blit.ps.glsl", "blit.ps.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../fog/shaders/fullscreen.vs.spvasm", "fullscreen.vs.spv"),
    ]);
}
