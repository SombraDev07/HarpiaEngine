fn main() {
    harpia_shader_build::build(&[
        ("forward.vs.spvasm", "forward.vs.spv"),
        ("forward.ps.spvasm", "forward.ps.spv"),
        ("fullscreen.vs.spvasm", "fullscreen.vs.spv"),
        ("taa.ps.spvasm", "taa.ps.spv"),
        ("blit.ps.spvasm", "blit.ps.spv"),
    ]);
}
