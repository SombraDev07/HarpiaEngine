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
    ]);
}
