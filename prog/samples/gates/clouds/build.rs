fn main() {
    harpia_shader_build::build(&[
        ("fullscreen.vs.spvasm", "fullscreen.vs.spv"),
        ("clouds.ps.spvasm", "clouds.ps.spv"),
        ("composite.ps.spvasm", "composite.ps.spv"),
        ("reproject.ps.spvasm", "reproject.ps.spv"),
        ("blit.ps.spvasm", "blit.ps.spv"),
    ]);
}
