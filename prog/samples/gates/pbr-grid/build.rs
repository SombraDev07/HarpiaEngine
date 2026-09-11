fn main() {
    harpia_shader_build::build(&[
        ("gbuffer.vs.spvasm", "gbuffer.vs.spv"),
        ("gbuffer.ps.spvasm", "gbuffer.ps.spv"),
        ("lighting.vs.spvasm", "lighting.vs.spv"),
        ("lighting.ps.spvasm", "lighting.ps.spv"),
        // partilhado: uma cópia só, ou diverge
        ("../../../sponza/shaders/blit.ps.spvasm", "blit.ps.spv"),
    ]);
}
