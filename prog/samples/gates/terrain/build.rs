fn main() {
    harpia_shader_build::build(&[
        ("terrain.vs.glsl", "terrain.vs.spv"),
        ("terrain.ps.glsl", "terrain.ps.spv"),
        ("blit.ps.glsl", "blit.ps.spv"),
        // partilhado: uma cópia só, ou diverge
        ("../../fog/shaders/fullscreen.vs.spvasm", "fullscreen.vs.spv"),
    ]);
}
