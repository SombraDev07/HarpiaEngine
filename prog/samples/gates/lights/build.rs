fn main() {
    harpia_shader_build::build(&[
        ("lights.vs.glsl", "lights.vs.spv"),
        ("lights.ps.glsl", "lights.ps.spv"),
    ]);
}
