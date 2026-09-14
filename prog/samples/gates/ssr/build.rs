fn main() {
    harpia_shader_build::build(&[
        ("scene.vs.glsl", "scene.vs.spv"),
        ("scene.ps.glsl", "scene.ps.spv"),
        ("copy_depth.cs.glsl", "copy_depth.cs.spv"),
        ("fullscreen.vs.glsl", "fullscreen.vs.spv"),
    ]);
}
