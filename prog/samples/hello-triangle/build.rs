fn main() {
    harpia_shader_build::build(&[
        ("triangle.vs.spvasm", "triangle.vs.spv"),
        ("triangle.ps.spvasm", "triangle.ps.spv"),
    ]);
}
