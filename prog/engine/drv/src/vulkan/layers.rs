//! Locate `VK_LAYER_KHRONOS_validation` so `cargo run` works when the distro
//! package is missing. Env is set **before** the loader is opened.

use std::path::{Path, PathBuf};

const JSON_NAME: &str = "VkLayer_khronos_validation.json";
const SO_NAME: &str = "libVkLayer_khronos_validation.so";

pub fn expose_khronos_validation() {
    if let Some((json_dir, lib_dir)) = locate() {
        prepend_env("VK_LAYER_PATH", &json_dir);
        if let Some(lib) = lib_dir {
            prepend_env("LD_LIBRARY_PATH", &lib);
        }
        tracing::info!(
            json_dir = %json_dir.display(),
            "Khronos validation layer found (prefer: sudo apt install vulkan-validationlayers)"
        );
    }
}

fn locate() -> Option<(PathBuf, Option<PathBuf>)> {
    for json in candidate_json_files() {
        if !json.is_file() {
            continue;
        }
        let json_dir = json.parent()?.to_path_buf();
        let lib_dir = find_so_dir(&json_dir);
        return Some((json_dir, lib_dir));
    }
    None
}

fn candidate_json_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let push_dir = |out: &mut Vec<PathBuf>, dir: PathBuf| {
        let p = dir.join(JSON_NAME);
        if !out.contains(&p) {
            out.push(p);
        }
    };

    if let Ok(paths) = std::env::var("VK_LAYER_PATH") {
        for part in paths.split(':').filter(|s| !s.is_empty()) {
            push_dir(&mut out, PathBuf::from(part));
        }
    }
    if let Ok(p) = std::env::var("HARPIA_VK_LAYER_PATH") {
        push_dir(&mut out, PathBuf::from(p));
    }
    if let Ok(sdk) = std::env::var("VULKAN_SDK") {
        let sdk = PathBuf::from(sdk);
        push_dir(&mut out, sdk.join("share/vulkan/explicit_layer.d"));
        push_dir(&mut out, sdk.join("etc/vulkan/explicit_layer.d"));
    }

    push_dir(&mut out, PathBuf::from("/usr/share/vulkan/explicit_layer.d"));
    push_dir(
        &mut out,
        PathBuf::from("/usr/local/share/vulkan/explicit_layer.d"),
    );
    if let Ok(home) = std::env::var("HOME") {
        push_dir(
            &mut out,
            PathBuf::from(home).join(".local/share/vulkan/explicit_layer.d"),
        );
    }

    if let Some(root) = workspace_root() {
        push_dir(
            &mut out,
            root.join("prog/3rdPartyLibs/vulkan-validationlayers"),
        );
        // Last resort: unpacked distro debs sitting next to the repo (not a Harpia dep).
        push_dir(
            &mut out,
            root.join("TucanoEngine/.deps/root/usr/share/vulkan/explicit_layer.d"),
        );
    }

    out
}

fn find_so_dir(json_dir: &Path) -> Option<PathBuf> {
    let mut dirs = vec![json_dir.to_path_buf()];
    for rel in [
        "lib",
        "lib/x86_64-linux-gnu",
        "../lib",
        "../lib/x86_64-linux-gnu",
        "../../lib/x86_64-linux-gnu",
        "../../../lib/x86_64-linux-gnu",
    ] {
        dirs.push(json_dir.join(rel));
    }
    for dir in dirs {
        if dir.join(SO_NAME).is_file() {
            return dir.canonicalize().ok().or(Some(dir));
        }
    }
    None
}

fn workspace_root() -> Option<PathBuf> {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..8 {
        if p.join("Cargo.toml").is_file() && p.join("prog/engine").is_dir() {
            return Some(p);
        }
        if !p.pop() {
            break;
        }
    }
    None
}

fn prepend_env(key: &str, dir: &Path) {
    let added = dir.to_string_lossy();
    let value = match std::env::var(key) {
        Ok(old) if !old.is_empty() => format!("{added}:{old}"),
        _ => added.into_owned(),
    };
    unsafe {
        std::env::set_var(key, value);
    }
}
