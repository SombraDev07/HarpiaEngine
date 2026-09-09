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
    Rgba16Float,
    R32Float,
    Rg16Float,
    D32Float,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Buffer {
    pub(crate) id: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct TextureDesc {
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
    pub format: Format,
    pub sampled: bool,
    pub storage: bool,
    pub color_attachment: bool,
    pub depth: bool,
}

/// Vertex / instance layout.
/// Empty `color_formats` ⇒ swapchain format (1 attachment), unless `depth_only`.
#[derive(Clone, Copy, Debug)]
pub struct PipelineTargets<'a> {
    pub color_formats: &'a [Format],
    pub depth_format: Option<Format>,
    pub vertex_stride: u32,
    pub instance_stride: u32,
    pub depth_test: bool,
    pub cull_back: bool,
    /// No color attachments (shadow atlas). Empty `color_formats` then means swapchain unless this is set.
    pub depth_only: bool,
    pub depth_bias: bool,
}

impl Default for PipelineTargets<'static> {
    fn default() -> Self {
        Self {
            color_formats: &[],
            depth_format: None,
            vertex_stride: 0,
            instance_stride: 0,
            depth_test: false,
            cull_back: false,
            depth_only: false,
            depth_bias: false,
        }
    }
}

/// Per-frame CBV (set 0 binding 0). Written **after** `begin_frame`.
/// First 16 bytes stay `tex_a`/`tex_b` so `gate-bindless` still works.
/// Phase 3 writes a larger `LightingCb` over the same chunk.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameConstants {
    pub tex_a: u32,
    pub tex_b: u32,
    pub _pad: [u32; 2],
}

/// CPU copy of a texture (mip 0), tightly packed — no row padding.
/// `D32Float` bytes are `f32` depth, not colour.
#[derive(Clone, Debug)]
pub struct TextureData {
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub bytes: Vec<u8>,
}

pub const PUSH_CONSTANTS_SIZE: u32 = 128;
/// One CBV chunk: the `range` of set 0 binding 0, and the stride of the frame ring.
pub const FRAME_UBO_SIZE: u64 = 1024;
/// Chunks per frame slot. Each `write_frame_bytes` takes one, so this is the
/// per-frame budget of *distinct* constant sets (Sponza: 103 prims + blit).
pub const FRAME_CBV_CHUNKS: u32 = 512;
/// Ring bytes per frame slot.
pub const FRAME_CBV_RING_SIZE: u64 = FRAME_UBO_SIZE * FRAME_CBV_CHUNKS as u64;
