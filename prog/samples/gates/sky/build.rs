fn main() {
    harpia_shader_build::build(&[
        ("fullscreen.vs.spvasm", "fullscreen.vs.spv"),
        ("transmittance.ps.spvasm", "transmittance.ps.spv"),
        ("multiscatter.ps.spvasm", "multiscatter.ps.spv"),
        ("skyview.ps.spvasm", "skyview.ps.spv"),
        ("sky.ps.spvasm", "sky.ps.spv"),
        ("blit.ps.spvasm", "blit.ps.spv"),
    ]);
}
