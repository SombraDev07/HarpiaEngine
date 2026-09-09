/// GPU backend. Null is CPU-only (tests, no WSI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Null,
    Vulkan,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extent2D {
    pub width: u32,
    pub height: u32,
}

impl Extent2D {
    pub fn is_zero(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Swapchain / RT formats the rest of the engine is allowed to name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Bgra8Unorm,
    Bgra8Srgb,
    Rgba8Unorm,
    Rgba8Srgb,
    Unknown(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveTopology {
    TriangleList,
}

#[derive(Clone, Copy, Debug)]
pub struct FrameInfo {
    pub extent: Extent2D,
    pub format: Format,
    pub frame_index: u64,
    /// True when the window is 0×0 or the swapchain cannot be acquired. Do not record.
    pub skipped: bool,
}

/// Opaque PSO. Valid only on the `Gpu` that created it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GraphicsPipeline {
    pub(crate) id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComputePipeline {
    pub(crate) id: u32,
}

/// Texture handle. `Texture::NULL` is bindless slot 0 (dummy 1×1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Texture {
    pub(crate) id: u32,
}

impl Texture {
    pub const NULL: Texture = Texture { id: 0 };
}

#[derive(Clone, Copy, Debug)]
pub struct TextureDesc {
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
    pub format: Format,
    pub sampled: bool,
    pub storage: bool,
}

/// Per-frame CBV (set 0 binding 0). Written **after** `begin_frame`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameConstants {
    pub tex_a: u32,
    pub tex_b: u32,
    pub _pad: [u32; 2],
}
