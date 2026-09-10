fn main() {
    harpia_shader_build::build(&[
        ("forward.vs.spvasm", "forward.vs.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../csm/shaders/shadow.vs.spvasm", "shadow.vs.spv"),
        ("forward.ps.spvasm", "forward.ps.spv"),
        ("fullscreen.vs.spvasm", "fullscreen.vs.spv"),
        ("apply.ps.spvasm", "apply.ps.spv"),
        ("inject.cs.spvasm", "inject.cs.spv"),
        ("integrate.cs.spvasm", "integrate.cs.spv"),
        ("blit.ps.spvasm", "blit.ps.spv"),
    ]);
}
