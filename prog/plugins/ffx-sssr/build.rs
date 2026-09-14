use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let sssr = manifest.join("../../3rdPartyLibs/ffx-sssr");
    println!(
        "cargo:rerun-if-changed={}",
        sssr.join("ffx_sssr.glsl.h").display()
    );
    println!("cargo:rerun-if-changed={}", sssr.join("ffx_sssr.h").display());
    harpia_shader_build::build_with_includes(
        &[
            ("sssr_intersect.cs.glsl", "sssr_intersect.cs.spv"),
            ("sssr_apply.ps.glsl", "sssr_apply.ps.spv"),
        ],
        &[&sssr],
    );
}
