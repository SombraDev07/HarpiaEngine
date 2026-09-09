use thiserror::Error;

#[derive(Debug, Error)]
pub enum RhiError {
    #[error("failed to load Vulkan loader: {0}")]
    Loader(#[from] ash::LoadingError),

    #[error("vulkan: {0}")]
    Vulkan(#[from] ash::vk::Result),

    #[error("gpu allocator: {0}")]
    Allocator(#[from] gpu_allocator::AllocationError),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("no suitable Vulkan 1.3 GPU with graphics+present")]
    NoDevice,

    #[error(
        "VK_LAYER_KHRONOS_validation not found. Install with: sudo apt install vulkan-validationlayers\n\
         Or: export VK_LAYER_PATH=/path/to/explicit_layer.d  (and LD_LIBRARY_PATH if the .so is elsewhere)"
    )]
    ValidationLayerMissing,

    #[error("a window is required for the Vulkan backend")]
    WindowRequired,

    #[error("device lost")]
    DeviceLost,

    #[error("GPU hung waiting for a frame fence")]
    FrameTimeout,

    #[error("command recorded outside begin_frame/end_frame")]
    NotInFrame,

    #[error("swapchain pass mismatch (begin/end)")]
    PassMismatch,

    #[error("NUL in shader entry point")]
    BadCString,

    #[error("{0}")]
    Message(String),
}

impl RhiError {
    pub fn msg(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }

    pub fn from_vk(result: ash::vk::Result) -> Self {
        match result {
            ash::vk::Result::ERROR_DEVICE_LOST => Self::DeviceLost,
            other => Self::Vulkan(other),
        }
    }
}
