//! Authoring ≠ GPU. The editor crate holds paths and transforms; `harpia-scene`
//! holds the `Buffer`/`Texture` the cook writes. No `vk::*`.

mod authoring;
mod dialog;

pub use authoring::{
    AssetPath, AuthoringError, AuthoringId, AuthoringNode, AuthoringScene, LocalTransform,
    MaterialRef, MeshRef, PrefabRef, RuntimePath, extract_authoring, spawn_authoring,
};
pub use dialog::{pick_authoring_scene, save_authoring_scene};
