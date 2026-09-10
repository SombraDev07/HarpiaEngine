fn main() {
    harpia_shader_build::build(&[
        ("shadow.vs.spvasm", "shadow.vs.spv"),
        ("color.vs.spvasm", "color.vs.spv"),
        ("color.ps.spvasm", "color.ps.spv"),
        ("blit.vs.spvasm", "blit.vs.spv"),
        ("blit.ps.spvasm", "blit.ps.spv"),
    ]);
}
