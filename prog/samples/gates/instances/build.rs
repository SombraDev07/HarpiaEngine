fn main() {
    harpia_shader_build::build(&[
        ("cull.cs.glsl", "cull.cs.spv"),
        ("cube.vs.glsl", "cube.vs.spv"),
        ("cube.ps.glsl", "cube.ps.spv"),
    ]);
}
