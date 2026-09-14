fn main() {
    harpia_shader_build::build(&[
        ("../../gtao/shaders/room.vs.glsl", "room.vs.spv"),
        ("lit.ps.glsl", "lit.ps.spv"),
        ("blit.ps.glsl", "blit.ps.spv"),
        ("../../bloom/shaders/fullscreen.vs.glsl", "fullscreen.vs.spv"),
    ]);
}
