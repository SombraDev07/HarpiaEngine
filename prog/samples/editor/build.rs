fn main() {
    harpia_shader_build::build(&[
        (
            "../../../engine/render/shaders/fullscreen.vs.glsl",
            "fullscreen.vs.spv",
        ),
        ("../../../engine/render/shaders/blit.ps.glsl", "blit.ps.spv"),
    ]);
}
