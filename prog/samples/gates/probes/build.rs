fn main() {
    harpia_shader_build::build(&[
        ("../../ssr/shaders/scene.vs.glsl", "scene.vs.spv"),
        ("../../ssr/shaders/scene.ps.glsl", "scene.ps.spv"),
        ("../../ssr/shaders/copy_depth.cs.glsl", "copy_depth.cs.spv"),
        ("../../ssr/shaders/fullscreen.vs.glsl", "fullscreen.vs.spv"),
        ("../../gtao/shaders/room.vs.glsl", "room.vs.spv"),
        ("../../gtao/shaders/room.ps.glsl", "room.ps.spv"),
        ("apply.ps.glsl", "apply.ps.spv"),
        ("../../gtao/shaders/blit.ps.glsl", "blit.ps.spv"),
    ]);
}
