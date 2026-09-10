fn main() {
    harpia_shader_build::build(&[
        ("bindless.vs.spvasm", "bindless.vs.spv"),
        ("bindless.ps.spvasm", "bindless.ps.spv"),
        ("bindless.cs.spvasm", "bindless.cs.spv"),
    ]);
}
