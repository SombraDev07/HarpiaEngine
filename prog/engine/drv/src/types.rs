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

    /// O índice cru. Para quem precisa de identidade sem tocar no recurso — o
    /// render graph compara handles para saber se dois acessos são ao mesmo sítio.
    pub fn raw(self) -> u32 {
        self.id
    }

    /// Um handle feito à mão, **para testes**.
    ///
    /// O render graph tem de ser testável sem GPU: a derivação de barreiras é uma
    /// função pura sobre handles e não precisa de recurso nenhum por trás. Fora de
    /// um teste isto dá um handle que não aponta para nada.
    #[doc(hidden)]
    pub const fn from_raw(id: u32) -> Self {
        Self { id }
    }
}

impl Buffer {
    pub fn raw(self) -> u32 {
        self.id
    }

    /// Ver [`Texture::from_raw`].
    #[doc(hidden)]
    pub const fn from_raw(id: u32) -> Self {
        Self { id }
    }
}

/// Um recurso a que uma barreira se aplica.
///
/// Neutro de propósito: quem o constrói é o render graph, que por D0 não pode
/// conhecer `vk::*`. A tradução para estágios, máscaras e layouts é do backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Barrier {
    Texture(Texture),
    Buffer(Buffer),
}

/// Uma barreira derivada: de que acesso para que acesso.
///
/// `src` e `dst` são códigos que o backend traduz. Um `u32` e não um enum rico
/// porque a seta das dependências vai do `harpia-render` para o `harpia-rhi`, e
/// o vocabulário de acessos vive do lado de cima.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarrierDesc {
    pub resource: Barrier,
    pub src: u32,
    pub dst: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Buffer {
    pub(crate) id: u32,
}

/// 2D image, or a volume (fog froxels, cloud noise). Layout spec set 4 / set 5.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextureDim {
    #[default]
    D2,
    D3,
}

/// Fields are additive: build with `..Default::default()` so a new one does not
/// break every call site.
#[derive(Clone, Copy, Debug)]
pub struct TextureDesc {
    pub width: u32,
    pub height: u32,
    /// Slices when `dim` is [`TextureDim::D3`]. Ignored for 2D.
    pub depth_slices: u32,
    pub dim: TextureDim,
    pub mip_levels: u32,
    pub format: Format,
    pub sampled: bool,
    pub storage: bool,
    pub color_attachment: bool,
    /// Depth-stencil attachment (not the volume extent — that is `depth_slices`).
    pub depth: bool,
}

impl Default for TextureDesc {
    fn default() -> Self {
        Self {
            width: 1,
            height: 1,
            depth_slices: 1,
            dim: TextureDim::D2,
            mip_levels: 1,
            format: Format::Rgba8Unorm,
            sampled: true,
            storage: false,
            color_attachment: false,
            depth: false,
        }
    }
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
    /// 1 for a 2D texture; slices, in order, for a volume.
    pub depth_slices: u32,
    pub format: Format,
    pub bytes: Vec<u8>,
}

/// Volume UAV slots in set 4 (binding 1) — fog writes two.
/// Slots de imagem 2D de storage (set 4, binding 0).
///
/// Era **um** descritor, escrito na criação de cada textura — o que queria dizer
/// que só a última criada estava ligada. Uma pirâmide Hi-Z precisa de um slot por
/// nível, e 16 chega para 65536×65536.
pub const STORAGE_IMAGE_SLOTS: u32 = 16;
pub const VOLUME_UAV_SLOTS: u32 = 4;
/// Volume SRV slots in set 5 (binding 0).
pub const VOLUME_SRV_SLOTS: u32 = 8;

pub const PUSH_CONSTANTS_SIZE: u32 = 128;
/// One CBV chunk: the `range` of set 0 binding 0, and the stride of the frame ring.
pub const FRAME_UBO_SIZE: u64 = 1024;
/// Chunks per frame slot. Each `write_frame_bytes` takes one, so this is the
/// per-frame budget of *distinct* constant sets (Sponza: 103 prims + blit).
pub const FRAME_CBV_CHUNKS: u32 = 512;
/// Ring bytes per frame slot.
pub const FRAME_CBV_RING_SIZE: u64 = FRAME_UBO_SIZE * FRAME_CBV_CHUNKS as u64;

/// Timestamps a frame can hold. Two are the frame itself; the rest are marks.
pub const MAX_TIMESTAMPS: u32 = 32;

/// What a frame cost, filled in once the GPU has actually finished it.
///
/// The engine could not answer "how long does the fog take" before this existed,
/// and a locked 60 Hz present made every wall-clock measurement report the
/// monitor instead of the work.
#[derive(Clone, Debug, Default)]
pub struct GpuStats {
    /// Whole frame on the GPU, milliseconds.
    pub frame_ms: f32,
    /// `(label, ms)` per [`Device::mark`], in the order they were recorded.
    pub passes: Vec<(&'static str, f32)>,
    pub draws: u32,
    pub dispatches: u32,
    pub triangles: u64,
}

/// Slots de storage buffer no set 3.
///
/// Um shader indexa-os por constante, tal como faz com os volumes. Dezasseis
/// chegam para listas de luzes, argumentos indirectos e o que a fase 6.5 pedir;
/// se um dia não chegarem, o custo de subir é um número.
pub const STORAGE_BUFFER_SLOTS: u32 = 16;
