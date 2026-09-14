fn main() {
    harpia_shader_build::build(&[
        ("room.vs.glsl", "room.vs.spv"),
        ("room.ps.glsl", "room.ps.spv"),
        ("gtao.ps.glsl", "gtao.ps.spv"),
        ("blit.ps.glsl", "blit.ps.spv"),
        ("../../bloom/shaders/fullscreen.vs.glsl", "fullscreen.vs.spv"),
    ]);
}
