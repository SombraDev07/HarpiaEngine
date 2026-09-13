fn main() {
    harpia_shader_build::build(&[
        ("terrain_bounds.cs.glsl", "terrain_bounds.cs.spv"),
        ("field_upload.cs.glsl", "field_upload.cs.spv"),
        ("terrain_cull.cs.glsl", "terrain_cull.cs.spv"),
        ("args_reset.cs.glsl", "args_reset.cs.spv"),
        ("terrain.vs.glsl", "terrain.vs.spv"),
        ("terrain.ps.glsl", "terrain.ps.spv"),
        ("blit.ps.glsl", "blit.ps.spv"),
        // partilhado: uma cópia só, ou diverge
        ("../../fog/shaders/fullscreen.vs.spvasm", "fullscreen.vs.spv"),
        ("../../sky/shaders/transmittance.ps.spvasm", "transmittance.ps.spv"),
        ("../../sky/shaders/multiscatter.ps.spvasm", "multiscatter.ps.spv"),
        ("../../sky/shaders/skyview.ps.spvasm", "skyview.ps.spv"),
        ("../../sky/shaders/sky_hdr.ps.glsl", "sky_hdr.ps.spv"),
        ("../../clouds/shaders/clouds.ps.spvasm", "clouds.ps.spv"),
        ("../../clouds/shaders/reproject.ps.spvasm", "reproject.ps.spv"),
        ("../../clouds/shaders/apply_clouds.ps.glsl", "apply_clouds.ps.spv"),
    ]);
}
