fn main() {
    harpia_shader_build::build(&[
        ("shadow.vs.glsl", "shadow.vs.spv"),
        ("cull.cs.glsl", "cull.cs.spv"),
        ("shadow.ps.spvasm", "shadow.ps.spv"),
        ("color.vs.glsl", "color.vs.spv"),
        ("color.ps.spvasm", "color.ps.spv"),
        ("blit.vs.spvasm", "blit.vs.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../gates/rain/shaders/rain.ps.spvasm", "rain.ps.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../gates/fog/shaders/inject.cs.spvasm", "inject.cs.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../gates/fog/shaders/integrate.cs.spvasm", "integrate.cs.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../gates/fog/shaders/apply.ps.spvasm", "apply.ps.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../gates/fog/shaders/fullscreen.vs.spvasm", "fullscreen.vs.spv"),
        ("blit.ps.spvasm", "blit.ps.spv"),
    ]);
}
