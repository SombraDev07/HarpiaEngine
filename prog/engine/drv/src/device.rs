use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::null::NullGpu;
use crate::types::{Backend, Extent2D, Format, FrameInfo, GraphicsPipeline};
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

/// GPU commands. Valid between [`Device::begin_frame`] and [`Device::end_frame`]
/// except create/resize/idle.
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
}

impl Device for Gpu {
    fn begin_frame(&mut self) -> Result<FrameInfo> {
        match self {
            Gpu::Null(g) => g.begin_frame(),
            Gpu::Vulkan(g) => g.begin_frame(),
        }
    }

    fn begin_swapchain_pass(&mut self, clear: [f32; 4]) -> Result<()> {
        match self {
            Gpu::Null(g) => g.begin_swapchain_pass(clear),
            Gpu::Vulkan(g) => g.begin_swapchain_pass(clear),
        }
    }

    fn set_pipeline(&mut self, pipeline: &GraphicsPipeline) -> Result<()> {
        match self {
            Gpu::Null(g) => g.set_pipeline(pipeline),
            Gpu::Vulkan(g) => g.set_pipeline(pipeline),
        }
    }

    fn draw(
        &mut self,
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
    ) -> Result<()> {
        match self {
            Gpu::Null(g) => g.draw(vertex_count, instance_count, first_vertex, first_instance),
            Gpu::Vulkan(g) => g.draw(vertex_count, instance_count, first_vertex, first_instance),
        }
    }

    fn end_swapchain_pass(&mut self) -> Result<()> {
        match self {
            Gpu::Null(g) => g.end_swapchain_pass(),
            Gpu::Vulkan(g) => g.end_swapchain_pass(),
        }
    }

    fn end_frame(&mut self) -> Result<()> {
        match self {
            Gpu::Null(g) => g.end_frame(),
            Gpu::Vulkan(g) => g.end_frame(),
        }
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        match self {
            Gpu::Null(g) => g.resize(width, height),
            Gpu::Vulkan(g) => g.resize(width, height),
        }
    }

    fn present_format(&self) -> Format {
        match self {
            Gpu::Null(g) => g.present_format(),
            Gpu::Vulkan(g) => g.present_format(),
        }
    }

    fn extent(&self) -> Extent2D {
        match self {
            Gpu::Null(g) => g.extent(),
            Gpu::Vulkan(g) => g.extent(),
        }
    }

    fn create_graphics_pipeline(&mut self, desc: &GraphicsPipelineDesc<'_>) -> Result<GraphicsPipeline> {
        match self {
            Gpu::Null(g) => g.create_graphics_pipeline(desc),
            Gpu::Vulkan(g) => g.create_graphics_pipeline(desc),
        }
    }

    fn wait_idle(&self) -> Result<()> {
        match self {
            Gpu::Null(g) => g.wait_idle(),
            Gpu::Vulkan(g) => g.wait_idle(),
        }
    }

    fn validation_error_count(&self) -> u32 {
        match self {
            Gpu::Null(g) => g.validation_error_count(),
            Gpu::Vulkan(g) => g.validation_error_count(),
        }
    }
}
