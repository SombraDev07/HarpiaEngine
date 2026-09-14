use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let spd = manifest.join("../../3rdPartyLibs/ffx-spd");
    println!("cargo:rerun-if-changed={}", spd.join("ffx_a.h").display());
    println!("cargo:rerun-if-changed={}", spd.join("ffx_spd.h").display());
    harpia_shader_build::build_with_includes(
        &[
            ("spd_karis.cs.glsl", "spd_karis.cs.spv"),
            ("spd_min.cs.glsl", "spd_min.cs.spv"),
        ],
        &[&spd],
    );
}
