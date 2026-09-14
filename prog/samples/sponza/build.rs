fn main() {
    harpia_shader_build::build(&[
        ("shadow.vs.glsl", "shadow.vs.spv"),
        ("cull.cs.glsl", "cull.cs.spv"),
        ("scene.mesh.glsl", "scene.mesh.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../gates/veg/shaders/hiz.cs.glsl", "hiz.cs.spv"),
        ("shadow.ps.spvasm", "shadow.ps.spv"),
        ("color.vs.glsl", "color.vs.spv"),
        ("color.ps.spvasm", "color.ps.spv"),
        ("blit.vs.spvasm", "blit.vs.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../gates/rain/shaders/rain.ps.spvasm", "rain.ps.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../gates/fog/shaders/inject.cs.glsl", "inject.cs.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../gates/fog/shaders/integrate.cs.spvasm", "integrate.cs.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../gates/fog/shaders/apply.ps.spvasm", "apply.ps.spv"),
          // partilhado: uma cópia só, ou diverge
        ("../../gates/fog/shaders/fullscreen.vs.spvasm", "fullscreen.vs.spv"),
        ("blit.ps.spvasm", "blit.ps.spv"),
        ("gi.ps.glsl", "gi.ps.spv"),
        ("bloom_add.ps.glsl", "bloom_add.ps.spv"),
        ("copy_view_depth.cs.glsl", "copy_view_depth.cs.spv"),
        ("normals.cs.glsl", "normals.cs.spv"),
        ("../../gates/bloom/shaders/copy.cs.glsl", "bloom_copy.cs.spv"),
        ("../../gates/exposure/shaders/luma.cs.glsl", "luma.cs.spv"),
        ("../../gates/exposure/shaders/adapt.cs.glsl", "adapt.cs.spv"),
    ]);
}
