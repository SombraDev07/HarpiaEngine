use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let shader_dir = manifest.join("shaders");
    let assembler = manifest.join("../../../tools/assemble_spvasm.py");

    for (src_name, dst_name) in [
        ("gbuffer.vs.spvasm", "gbuffer.vs.spv"),
        ("gbuffer.ps.spvasm", "gbuffer.ps.spv"),
        ("lighting.vs.spvasm", "lighting.vs.spv"),
        ("lighting.ps.spvasm", "lighting.ps.spv"),
    ] {
        let src = shader_dir.join(src_name);
        let dst = out.join(dst_name);
        println!("cargo:rerun-if-changed={}", src.display());
        assemble(&src, &dst, &assembler);
    }
    println!("cargo:rerun-if-changed={}", assembler.display());
}

fn assemble(src: &Path, dst: &Path, py: &Path) {
    if let Ok(st) = Command::new("spirv-as")
        .args([src.as_os_str(), "-o".as_ref(), dst.as_os_str()])
        .status()
    {
        if st.success() {
            return;
        }
    }
    let st = Command::new("python3")
        .args([py.as_os_str(), src.as_os_str(), dst.as_os_str()])
        .status()
        .expect("python3 to assemble SPIR-V");
    assert!(st.success(), "failed to assemble {}", src.display());
}
