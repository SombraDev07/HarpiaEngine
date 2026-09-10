fn main() {
    harpia_shader_build::build(&[
        ("water.vs.spvasm", "water.vs.spv"),
        ("water.ps.spvasm", "water.ps.spv"),
        ("sky.ps.spvasm", "sky.ps.spv"),
        ("tonemap.ps.spvasm", "tonemap.ps.spv"),
        ("opaque.vs.spvasm", "opaque.vs.spv"),
        ("opaque.ps.spvasm", "opaque.ps.spv"),
        ("copy.ps.spvasm", "copy.ps.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../fog/shaders/fullscreen.vs.spvasm", "fullscreen.vs.spv"),
    ]);
}
