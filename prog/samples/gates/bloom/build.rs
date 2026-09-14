fn main() {
    harpia_shader_build::build(&[
        ("fullscreen.vs.glsl", "fullscreen.vs.spv"),
        ("scene.ps.glsl", "scene.ps.spv"),
        ("copy.cs.glsl", "copy.cs.spv"),
        ("composite.ps.glsl", "composite.ps.spv"),
    ]);
}
