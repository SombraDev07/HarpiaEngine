//! Shader compilation for the samples' `build.rs`, in one place.
//!
//! Eleven build scripts used to carry a copy of this. They also silently fell
//! back to `prog/tools/assemble_spvasm.py` whenever `spirv-as` was not on the
//! path — which on this host it always was. That fallback cost the project months
//! of hand-debugging an assembler that never needed to exist here (see
//! `docs/AAA-Gap-Analysis.md` §3.1), so it is now loud and last.
//!
//! Two source kinds:
//!
//! * `.spvasm` — SPIR-V assembly, what the tree has today. Assembled by
//!   `spirv-as`.
//! * `.glsl` — GLSL 450, compiled by `glslangValidator`. This is where new
//!   shaders should go.
//!
//! Either way the result goes through **`spirv-val`**, so a broken module fails
//! the build instead of the frame. Before this, the only thing validating SPIR-V
//! was the Vulkan layer at runtime, which meant finding out in a screenshot.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Which stage a `.glsl` source is. `.spvasm` carries its own entry point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Vertex,
    Fragment,
    Compute,
    /// `foo.mesh.glsl`. Precisa de `--target-env vulkan1.3` e da extensão
    /// `GL_EXT_mesh_shader` declarada no shader.
    Mesh,
}

impl Stage {
    fn glslang_flag(self) -> &'static str {
        match self {
            Stage::Vertex => "vert",
            Stage::Fragment => "frag",
            Stage::Compute => "comp",
            Stage::Mesh => "mesh",
        }
    }

    /// GLSL always calls its entry point `main`, but every PSO in this tree asks
    /// for `VSMain`/`PSMain`/`CSMain`. glslang can rename it on the way out, so
    /// a GLSL shader stays a drop-in replacement for the assembly it succeeds.
    fn entry_point(self) -> &'static str {
        match self {
            Stage::Vertex => "VSMain",
            Stage::Fragment => "PSMain",
            Stage::Compute => "CSMain",
            Stage::Mesh => "MSMain",
        }
    }

    /// Guess from the name the tree already uses: `foo.vs.glsl`, `foo.ps.glsl`,
    /// `foo.cs.glsl`.
    fn from_name(name: &str) -> Option<Self> {
        let stem = name.strip_suffix(".glsl")?;
        if stem.ends_with(".vs") {
            Some(Stage::Vertex)
        } else if stem.ends_with(".ps") || stem.ends_with(".fs") {
            Some(Stage::Fragment)
        } else if stem.ends_with(".cs") {
            Some(Stage::Compute)
        } else if stem.ends_with(".mesh") {
            Some(Stage::Mesh)
        } else {
            None
        }
    }
}

/// Compile every `(source, output)` pair into `OUT_DIR`.
///
/// Paths are relative to `<manifest>/shaders`, so `../../fog/shaders/x.spvasm`
/// works and keeps one copy of a shader shared between samples.
pub fn build(pairs: &[(&str, &str)]) {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let shader_dir = manifest.join("shaders");

    for (src_name, dst_name) in pairs {
        let src = shader_dir.join(src_name);
        let dst = out.join(dst_name);
        println!("cargo:rerun-if-changed={}", src.display());
        compile(&src, &dst);
    }
}

fn compile(src: &Path, dst: &Path) {
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if name.ends_with(".glsl") {
        let stage = Stage::from_name(name)
            .unwrap_or_else(|| panic!("{name}: cannot tell the stage; name it .vs/.ps/.cs.glsl"));
        compile_glsl(src, dst, stage);
    } else {
        assemble_spvasm(src, dst);
    }
    validate(dst);
}

fn compile_glsl(src: &Path, dst: &Path, stage: Stage) {
    let status = Command::new("glslangValidator")
        .arg("-V") // Vulkan SPIR-V
        .arg("--target-env")
        .arg("vulkan1.3")
        .arg("-S")
        .arg(stage.glslang_flag())
        .arg("--source-entrypoint")
        .arg("main")
        .arg("-e")
        .arg(stage.entry_point())
        .arg("-o")
        .arg(dst)
        .arg(src)
        .status();
    match status {
        Ok(st) if st.success() => {}
        Ok(st) => panic!(
            "glslangValidator failed ({st}) on {}. Its own message is above.",
            src.display()
        ),
        Err(e) => panic!(
            "glslangValidator is not on PATH ({e}), and {} is GLSL. \
             Install it: sudo apt install glslang-tools",
            src.display()
        ),
    }
}

fn assemble_spvasm(src: &Path, dst: &Path) {
    if let Ok(st) = Command::new("spirv-as")
        .args([src.as_os_str(), "-o".as_ref(), dst.as_os_str()])
        .status()
    {
        if st.success() {
            return;
        }
        panic!(
            "spirv-as rejected {}. Fix the assembly rather than falling back.",
            src.display()
        );
    }

    // Only if spirv-as is genuinely absent. Say so, because the home-grown
    // assembler is a strictly worse tool and using it should be a decision.
    println!(
        "cargo:warning=spirv-as not found, falling back to assemble_spvasm.py for {}. \
         Install the real thing: sudo apt install glslang-tools spirv-tools",
        src.display()
    );
    let py = fallback_assembler();
    let st = Command::new("python3")
        .args([py.as_os_str(), src.as_os_str(), dst.as_os_str()])
        .status()
        .expect("python3 to assemble SPIR-V");
    assert!(st.success(), "failed to assemble {}", src.display());
}

fn validate(spv: &Path) {
    match Command::new("spirv-val").arg(spv).status() {
        Ok(st) if st.success() => {}
        Ok(st) => panic!(
            "spirv-val rejected {} ({st}). Its message is above; \
             a module that fails here would have failed on the GPU.",
            spv.display()
        ),
        // Without spirv-val the Vulkan layer is the only validator, which means
        // finding out at runtime. Warn, do not fail: the build still works.
        Err(_) => println!(
            "cargo:warning=spirv-val not found, so {} is unvalidated until it reaches the GPU. \
             Install it: sudo apt install spirv-tools",
            spv.display()
        ),
    }
}

fn fallback_assembler() -> PathBuf {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    // <sample>/ -> up to prog/, then tools/
    let mut dir = manifest.as_path();
    while let Some(parent) = dir.parent() {
        let candidate = parent.join("tools/assemble_spvasm.py");
        if candidate.exists() {
            return candidate;
        }
        dir = parent;
    }
    panic!("cannot find prog/tools/assemble_spvasm.py from {}", manifest.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_comes_from_the_name_the_tree_uses() {
        assert_eq!(Stage::from_name("color.vs.glsl"), Some(Stage::Vertex));
        assert_eq!(Stage::from_name("color.ps.glsl"), Some(Stage::Fragment));
        assert_eq!(Stage::from_name("blur.cs.glsl"), Some(Stage::Compute));
        assert_eq!(Stage::from_name("color.fs.glsl"), Some(Stage::Fragment));
        assert_eq!(Stage::from_name("mystery.glsl"), None);
        assert_eq!(Stage::from_name("color.vs.spvasm"), None);
    }

    #[test]
    fn entry_points_match_what_the_psos_ask_for() {
        assert_eq!(Stage::Vertex.entry_point(), "VSMain");
        assert_eq!(Stage::Fragment.entry_point(), "PSMain");
        assert_eq!(Stage::Compute.entry_point(), "CSMain");
    }
}
