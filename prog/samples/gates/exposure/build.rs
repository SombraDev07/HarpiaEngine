fn main() {
    harpia_shader_build::build(&[
        ("../../bloom/shaders/fullscreen.vs.glsl", "fullscreen.vs.spv"),
        ("../../bloom/shaders/scene.ps.glsl", "scene.ps.spv"),
        ("luma.cs.glsl", "luma.cs.spv"),
        ("adapt.cs.glsl", "adapt.cs.spv"),
        ("tonemap.ps.glsl", "tonemap.ps.spv"),
        ("../../gtao/shaders/blit.ps.glsl", "blit.ps.spv"),
    ]);
}
