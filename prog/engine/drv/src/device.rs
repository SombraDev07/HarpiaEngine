use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::null::NullGpu;
use crate::types::{
    BarrierDesc,
    Backend, Buffer, ComputePipeline, Extent2D, Format, FrameConstants, FrameInfo,
    GpuStats, GraphicsPipeline, PipelineTargets, Texture, TextureData, TextureDesc,
};
use crate::vulkan::VulkanGpu;
use crate::Result;

/// WSI handles. Must not outlive the `winit` window. Copied only for `create`.
#[derive(Clone, Copy)]
pub struct WindowHandles {
    pub display: RawDisplayHandle,
    pub window: RawWindowHandle,
}

pub struct DeviceDesc {
    pub backend: Backend,
    pub validation: bool,
    pub app_name: &'static str,
    pub width: u32,
    pub height: u32,
    pub window: Option<WindowHandles>,
    /// `false` picks the fastest present mode available instead of FIFO, so a
    /// frame time can be measured instead of the monitor's refresh.
    pub vsync: bool,
}

impl Default for DeviceDesc {
    fn default() -> Self {
        Self {
            backend: Backend::Vulkan,
            validation: true,
            app_name: "harpia",
            width: 1280,
            height: 720,
            window: None,
            vsync: true,
        }
    }
}

pub struct GraphicsPipelineDesc<'a> {
    pub vs_spirv: &'a [u8],
    pub fs_spirv: &'a [u8],
    pub vs_entry: &'a str,
    pub fs_entry: &'a str,
    /// Hello-triangle: false (empty layout). Bindless / PBR: true.
    pub bindless: bool,
    pub targets: PipelineTargets<'a>,
}

pub struct ComputePipelineDesc<'a> {
    pub cs_spirv: &'a [u8],
    pub cs_entry: &'a str,
}

/// Public device. Engine code never downcasts to Vulkan types.
pub enum Gpu {
    Null(NullGpu),
    Vulkan(VulkanGpu),
}

pub fn create(desc: &DeviceDesc) -> Result<Gpu> {
    match desc.backend {
        Backend::Null => Ok(Gpu::Null(NullGpu::new(desc))),
        Backend::Vulkan => Ok(Gpu::Vulkan(VulkanGpu::new(desc)?)),
    }
}

macro_rules! gpu {
    ($self:expr, $method:ident) => {
        match $self {
            Gpu::Null(g) => g.$method(),
            Gpu::Vulkan(g) => g.$method(),
        }
    };
    ($self:expr, $method:ident, $($arg:expr),+ $(,)?) => {
        match $self {
            Gpu::Null(g) => g.$method($($arg),+),
            Gpu::Vulkan(g) => g.$method($($arg),+),
        }
    };
}

/// GPU commands. Valid between [`Device::begin_frame`] and [`Device::end_frame`]
/// except create/resize/idle. Dynamic CBV writes only **after** `begin_frame`.
pub trait Device {
    fn begin_frame(&mut self) -> Result<FrameInfo>;
    fn begin_swapchain_pass(&mut self, clear: [f32; 4]) -> Result<()>;
    fn set_pipeline(&mut self, pipeline: &GraphicsPipeline) -> Result<()>;
    fn draw(
        &mut self,
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
    ) -> Result<()>;
    fn end_swapchain_pass(&mut self) -> Result<()>;
    fn end_frame(&mut self) -> Result<()>;
    fn resize(&mut self, width: u32, height: u32) -> Result<()>;
    fn present_format(&self) -> Format;
    fn extent(&self) -> Extent2D;
    fn create_graphics_pipeline(&mut self, desc: &GraphicsPipelineDesc<'_>) -> Result<GraphicsPipeline>;
    fn wait_idle(&self) -> Result<()>;
    fn validation_error_count(&self) -> u32;

    /// Timestamp the command stream. The span from the previous mark (or the
    /// start of the frame) shows up in [`Device::take_stats`] under `label`.
    fn mark(&mut self, label: &'static str);
    /// Stats for the most recently *completed* frame, not the one being built.
    /// Empty until a frame has been through the GPU and back.
    fn take_stats(&mut self) -> GpuStats;

    fn create_texture(&mut self, desc: &TextureDesc) -> Result<Texture>;
    fn upload_texture_mip(&mut self, tex: Texture, mip: u32, rgba: &[u8]) -> Result<()>;
    fn bindless_index(&self, tex: Texture) -> Result<u32>;
    /// Copy mip 0 back to the CPU. Waits for the device: capture, not hot path.
    fn read_texture(&mut self, tex: Texture) -> Result<TextureData>;
    /// Volumes (fog froxels, cloud noise) live in set 4 / set 5, never the 2D heap.
    fn bind_volume_uav(&mut self, slot: u32, tex: Texture) -> Result<()>;
    fn bind_volume_srv(&mut self, slot: u32, tex: Texture) -> Result<()>;
    fn write_frame_constants(&mut self, c: FrameConstants) -> Result<()>;
    fn bind_graphics_bindless(&mut self) -> Result<()>;
    fn create_compute_pipeline(&mut self, desc: &ComputePipelineDesc<'_>) -> Result<ComputePipeline>;
    fn set_compute_pipeline(&mut self, pipeline: &ComputePipeline) -> Result<()>;
    fn bind_compute_bindless(&mut self) -> Result<()>;
    fn dispatch(&mut self, x: u32, y: u32, z: u32) -> Result<()>;
    fn storage_barrier(&mut self, tex: Texture) -> Result<()>;
    fn write_frame_bytes(&mut self, data: &[u8]) -> Result<()>;
    fn set_push_constants(&mut self, data: &[u8]) -> Result<()>;
    fn set_viewport(&mut self, x: f32, y: f32, width: f32, height: f32) -> Result<()>;
    fn begin_color_pass(
        &mut self,
        colors: &[Texture],
        depth: Option<Texture>,
        clears: &[[f32; 4]],
        depth_clear: Option<f32>,
    ) -> Result<()>;
    fn end_color_pass(&mut self) -> Result<()>;
    fn create_vertex_buffer(&mut self, bytes: &[u8]) -> Result<Buffer>;
    fn create_index_buffer(&mut self, bytes: &[u8]) -> Result<Buffer>;
    /// Buffer que um shader indexa, ligado a um slot do set 3.
    fn create_storage_buffer(&mut self, slot: u32, bytes: &[u8]) -> Result<Buffer>;
    /// Reescreve-o. O tamanho não pode crescer.
    fn write_storage_buffer(&mut self, buffer: Buffer, bytes: &[u8]) -> Result<()>;
    /// Desenha com argumentos vindos de um buffer. Cinco `u32` por draw.
    /// Emite as barreiras que o render graph derivou.
    /// Liga a imagem 2D de storage no mip pedido.
    fn bind_storage_image(&mut self, tex: Texture, mip: u32, slot: u32) -> Result<()>;
    fn barriers(&mut self, list: &[BarrierDesc]) -> Result<()>;
    fn clear_depth_rect(&mut self, x: u32, y: u32, w: u32, h: u32, value: f32) -> Result<()>;
    fn draw_indirect(&mut self, args: Buffer, offset: u64, draws: u32) -> Result<()>;
    fn draw_indexed_indirect(&mut self, args: Buffer, offset: u64, draws: u32) -> Result<()>;
    /// Barreira de compute-escreve para indirect/vertex-lê.
    fn storage_barrier_buffer(&mut self, buffer: Buffer) -> Result<()>;
    /// Lê um buffer. Fora de um frame: espera pelo device.
    fn read_buffer(&mut self, buffer: Buffer, bytes: usize) -> Result<Vec<u8>>;
    fn bind_vertex_buffer(&mut self, buf: Buffer, binding: u32) -> Result<()>;
    fn bind_index_buffer(&mut self, buf: Buffer) -> Result<()>;
    fn draw_indexed(
        &mut self,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    ) -> Result<()>;
}

impl Device for Gpu {
    fn begin_frame(&mut self) -> Result<FrameInfo> {
        gpu!(self, begin_frame)
    }
    fn begin_swapchain_pass(&mut self, clear: [f32; 4]) -> Result<()> {
        gpu!(self, begin_swapchain_pass, clear)
    }
    fn set_pipeline(&mut self, pipeline: &GraphicsPipeline) -> Result<()> {
        gpu!(self, set_pipeline, pipeline)
    }
    fn draw(
        &mut self,
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
    ) -> Result<()> {
        gpu!(self, draw, vertex_count, instance_count, first_vertex, first_instance)
    }
    fn end_swapchain_pass(&mut self) -> Result<()> {
        gpu!(self, end_swapchain_pass)
    }
    fn end_frame(&mut self) -> Result<()> {
        gpu!(self, end_frame)
    }
    fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        gpu!(self, resize, width, height)
    }
    fn present_format(&self) -> Format {
        gpu!(self, present_format)
    }
    fn extent(&self) -> Extent2D {
        gpu!(self, extent)
    }
    fn create_graphics_pipeline(&mut self, desc: &GraphicsPipelineDesc<'_>) -> Result<GraphicsPipeline> {
        gpu!(self, create_graphics_pipeline, desc)
    }
    fn wait_idle(&self) -> Result<()> {
        gpu!(self, wait_idle)
    }
    fn validation_error_count(&self) -> u32 {
        gpu!(self, validation_error_count)
    }
    fn create_texture(&mut self, desc: &TextureDesc) -> Result<Texture> {
        gpu!(self, create_texture, desc)
    }
    fn upload_texture_mip(&mut self, tex: Texture, mip: u32, rgba: &[u8]) -> Result<()> {
        gpu!(self, upload_texture_mip, tex, mip, rgba)
    }
    fn bindless_index(&self, tex: Texture) -> Result<u32> {
        gpu!(self, bindless_index, tex)
    }
    fn read_texture(&mut self, tex: Texture) -> Result<TextureData> {
        gpu!(self, read_texture, tex)
    }
    fn bind_volume_uav(&mut self, slot: u32, tex: Texture) -> Result<()> {
        gpu!(self, bind_volume_uav, slot, tex)
    }
    fn bind_volume_srv(&mut self, slot: u32, tex: Texture) -> Result<()> {
        gpu!(self, bind_volume_srv, slot, tex)
    }
    fn write_frame_constants(&mut self, c: FrameConstants) -> Result<()> {
        gpu!(self, write_frame_constants, c)
    }
    fn bind_graphics_bindless(&mut self) -> Result<()> {
        gpu!(self, bind_graphics_bindless)
    }
    fn create_compute_pipeline(&mut self, desc: &ComputePipelineDesc<'_>) -> Result<ComputePipeline> {
        gpu!(self, create_compute_pipeline, desc)
    }
    fn set_compute_pipeline(&mut self, pipeline: &ComputePipeline) -> Result<()> {
        gpu!(self, set_compute_pipeline, pipeline)
    }
    fn bind_compute_bindless(&mut self) -> Result<()> {
        gpu!(self, bind_compute_bindless)
    }
    fn mark(&mut self, label: &'static str) {
        gpu!(self, mark, label)
    }

    fn take_stats(&mut self) -> GpuStats {
        gpu!(self, take_stats)
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) -> Result<()> {
        gpu!(self, dispatch, x, y, z)
    }
    fn storage_barrier(&mut self, tex: Texture) -> Result<()> {
        gpu!(self, storage_barrier, tex)
    }
    fn write_frame_bytes(&mut self, data: &[u8]) -> Result<()> {
        gpu!(self, write_frame_bytes, data)
    }
    fn set_push_constants(&mut self, data: &[u8]) -> Result<()> {
        gpu!(self, set_push_constants, data)
    }
    fn set_viewport(&mut self, x: f32, y: f32, width: f32, height: f32) -> Result<()> {
        gpu!(self, set_viewport, x, y, width, height)
    }
    fn begin_color_pass(
        &mut self,
        colors: &[Texture],
        depth: Option<Texture>,
        clears: &[[f32; 4]],
        depth_clear: Option<f32>,
    ) -> Result<()> {
        gpu!(self, begin_color_pass, colors, depth, clears, depth_clear)
    }
    fn end_color_pass(&mut self) -> Result<()> {
        gpu!(self, end_color_pass)
    }
    fn create_vertex_buffer(&mut self, bytes: &[u8]) -> Result<Buffer> {
        gpu!(self, create_vertex_buffer, bytes)
    }
    fn create_storage_buffer(&mut self, slot: u32, bytes: &[u8]) -> Result<Buffer> {
        gpu!(self, create_storage_buffer, slot, bytes)
    }

    fn write_storage_buffer(&mut self, buffer: Buffer, bytes: &[u8]) -> Result<()> {
        gpu!(self, write_storage_buffer, buffer, bytes)
    }

    fn bind_storage_image(&mut self, tex: Texture, mip: u32, slot: u32) -> Result<()> {
        gpu!(self, bind_storage_image, tex, mip, slot)
    }

    fn barriers(&mut self, list: &[BarrierDesc]) -> Result<()> {
        gpu!(self, barriers, list)
    }

    fn clear_depth_rect(&mut self, x: u32, y: u32, w: u32, h: u32, value: f32) -> Result<()> {
        gpu!(self, clear_depth_rect, x, y, w, h, value)
    }

    fn draw_indirect(&mut self, args: Buffer, offset: u64, draws: u32) -> Result<()> {
        gpu!(self, draw_indirect, args, offset, draws)
    }

    fn draw_indexed_indirect(&mut self, args: Buffer, offset: u64, draws: u32) -> Result<()> {
        gpu!(self, draw_indexed_indirect, args, offset, draws)
    }

    fn storage_barrier_buffer(&mut self, buffer: Buffer) -> Result<()> {
        gpu!(self, storage_barrier_buffer, buffer)
    }

    fn read_buffer(&mut self, buffer: Buffer, bytes: usize) -> Result<Vec<u8>> {
        gpu!(self, read_buffer, buffer, bytes)
    }

    fn create_index_buffer(&mut self, bytes: &[u8]) -> Result<Buffer> {
        gpu!(self, create_index_buffer, bytes)
    }
    fn bind_vertex_buffer(&mut self, buf: Buffer, binding: u32) -> Result<()> {
        gpu!(self, bind_vertex_buffer, buf, binding)
    }
    fn bind_index_buffer(&mut self, buf: Buffer) -> Result<()> {
        gpu!(self, bind_index_buffer, buf)
    }
    fn draw_indexed(
        &mut self,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    ) -> Result<()> {
        gpu!(
            self,
            draw_indexed,
            index_count,
            instance_count,
            first_index,
            vertex_offset,
            first_instance
        )
    }
}
