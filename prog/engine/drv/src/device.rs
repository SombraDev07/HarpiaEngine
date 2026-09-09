use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::null::NullGpu;
use crate::types::{
    Backend, ComputePipeline, Extent2D, Format, FrameConstants, FrameInfo, GraphicsPipeline,
    Texture, TextureDesc,
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
        }
    }
}

pub struct GraphicsPipelineDesc<'a> {
    pub vs_spirv: &'a [u8],
    pub fs_spirv: &'a [u8],
    pub vs_entry: &'a str,
    pub fs_entry: &'a str,
    /// Hello-triangle: false (empty layout). Bindless gate: true.
    pub bindless: bool,
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

    fn create_texture(&mut self, desc: &TextureDesc) -> Result<Texture>;
    fn upload_texture_mip(&mut self, tex: Texture, mip: u32, rgba: &[u8]) -> Result<()>;
    fn bindless_index(&self, tex: Texture) -> Result<u32>;
    fn write_frame_constants(&mut self, c: FrameConstants) -> Result<()>;
    fn bind_graphics_bindless(&mut self) -> Result<()>;
    fn create_compute_pipeline(&mut self, desc: &ComputePipelineDesc<'_>) -> Result<ComputePipeline>;
    fn set_compute_pipeline(&mut self, pipeline: &ComputePipeline) -> Result<()>;
    fn bind_compute_bindless(&mut self) -> Result<()>;
    fn dispatch(&mut self, x: u32, y: u32, z: u32) -> Result<()>;
    fn storage_barrier(&mut self, tex: Texture) -> Result<()>;
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
    fn dispatch(&mut self, x: u32, y: u32, z: u32) -> Result<()> {
        gpu!(self, dispatch, x, y, z)
    }
    fn storage_barrier(&mut self, tex: Texture) -> Result<()> {
        gpu!(self, storage_barrier, tex)
    }
}
