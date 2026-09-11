fn main() {
    harpia_shader_build::build(&[
        ("lights.vs.glsl", "lights.vs.spv"),
        ("shadow.vs.glsl", "shadow.vs.spv"),
        ("lights.ps.glsl", "lights.ps.spv"),
    ]);
}
