//! Plugin ABI. Hello-triangle must run with this loader unused.
//!
//! A plugin declares work (passes, resources) or talks to the RHI trait.
//! It never includes `ash` / `vk::*`.

use std::path::Path;

use libloading::Library;
use thiserror::Error;

pub const PLUGIN_ABI_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin load: {0}")]
    Load(#[from] libloading::Error),
    #[error("{0}")]
    Message(String),
}

pub type Result<T, E = PluginError> = std::result::Result<T, E>;

/// Host-side plugin contract. Phase 1: no implementations.
pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn abi_version(&self) -> u32 {
        PLUGIN_ABI_VERSION
    }
}

/// Dynamic libraries kept alive for the process lifetime.
/// Phase 1: `load_dir` is a documented no-op so the sample never dlopens.
pub struct PluginLoader {
    _libs: Vec<Library>,
}

impl PluginLoader {
    pub fn new() -> Self {
        Self { _libs: Vec::new() }
    }

    /// Reserved. Returns `Ok` without loading anything.
    pub fn load_dir(&mut self, path: &Path) -> Result<()> {
        tracing::debug!(path = %path.display(), "plugin loader idle (phase 1)");
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self._libs.is_empty()
    }
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_triangle_needs_no_plugins() {
        let loader = PluginLoader::new();
        assert!(loader.is_empty());
        let mut loader = loader;
        loader.load_dir(Path::new("plugins")).unwrap();
        assert!(loader.is_empty());
    }
}
