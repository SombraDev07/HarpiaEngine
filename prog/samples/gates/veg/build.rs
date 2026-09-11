fn main() {
    harpia_shader_build::build(&[
        ("hiz.cs.glsl", "hiz.cs.spv"),
        ("cull.cs.glsl", "cull.cs.spv"),
        ("box.vs.glsl", "box.vs.spv"),
        ("box.ps.glsl", "box.ps.spv"),
        ("veg.vs.glsl", "veg.vs.spv"),
        ("veg.ps.glsl", "veg.ps.spv"),
    ]);
}
