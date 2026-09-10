//! Shared types. No GPU, no `vk::*`.

mod input;
pub use input::{Input, Key, MouseButton};

pub const ENGINE_NAME: &str = "harpia";
pub const ENGINE_VERSION_MAJOR: u32 = 0;
pub const ENGINE_VERSION_MINOR: u32 = 1;
pub const ENGINE_VERSION_PATCH: u32 = 0;

/// Packed as Vulkan `make_api_version` variant: variant 0, major, minor, patch.
pub const fn engine_vk_version() -> u32 {
    (ENGINE_VERSION_MAJOR << 22) | (ENGINE_VERSION_MINOR << 12) | ENGINE_VERSION_PATCH
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
}

impl Error {
    pub fn msg(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Monotonic frame counter from the app loop (not the in-flight slot).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameNumber(pub u64);

/// Bindless heap size. Slot 0 is permanently the null texture.
pub const BINDLESS_HEAP_SIZE: u32 = 8192;
pub const BINDLESS_NULL_SLOT: u32 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_slot_is_zero() {
        assert_eq!(BINDLESS_NULL_SLOT, 0);
        assert_eq!(BINDLESS_HEAP_SIZE, 8192);
    }
}
