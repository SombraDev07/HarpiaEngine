use crate::device::{DeviceDesc, GraphicsPipelineDesc};
use crate::types::{Extent2D, Format, FrameInfo, GraphicsPipeline};
use crate::{RhiError, Result};

pub struct NullGpu {
    extent: Extent2D,
    in_frame: bool,
    in_pass: bool,
    frame_index: u64,
    next_pipeline: u32,
}

impl NullGpu {
    pub fn new(desc: &DeviceDesc) -> Self {
        Self {
            extent: Extent2D {
                width: desc.width.max(1),
                height: desc.height.max(1),
            },
            in_frame: false,
            in_pass: false,
            frame_index: 0,
            next_pipeline: 1,
        }
    }

    pub fn begin_frame(&mut self) -> Result<FrameInfo> {
        if self.in_frame {
            return Err(RhiError::msg("begin_frame while a frame is open"));
        }
        self.in_frame = true;
        let skipped = self.extent.is_zero();
        Ok(FrameInfo {
            extent: self.extent,
            format: Format::Bgra8Unorm,
            frame_index: self.frame_index,
            skipped,
        })
    }

    pub fn begin_swapchain_pass(&mut self, _clear: [f32; 4]) -> Result<()> {
        if !self.in_frame {
            return Err(RhiError::NotInFrame);
        }
        if self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        self.in_pass = true;
        Ok(())
    }

    pub fn set_pipeline(&mut self, _pipeline: &GraphicsPipeline) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        Ok(())
    }

    pub fn draw(
        &mut self,
        _vertex_count: u32,
        _instance_count: u32,
        _first_vertex: u32,
        _first_instance: u32,
    ) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        Ok(())
    }

    pub fn end_swapchain_pass(&mut self) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        self.in_pass = false;
        Ok(())
    }

    pub fn end_frame(&mut self) -> Result<()> {
        if !self.in_frame {
            return Err(RhiError::NotInFrame);
        }
        if self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        self.in_frame = false;
        self.frame_index += 1;
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        self.extent = Extent2D { width, height };
        Ok(())
    }

    pub fn present_format(&self) -> Format {
        Format::Bgra8Unorm
    }

    pub fn extent(&self) -> Extent2D {
        self.extent
    }

    pub fn create_graphics_pipeline(&mut self, _desc: &GraphicsPipelineDesc<'_>) -> Result<GraphicsPipeline> {
        let id = self.next_pipeline;
        self.next_pipeline += 1;
        Ok(GraphicsPipeline { id })
    }

    pub fn wait_idle(&self) -> Result<()> {
        Ok(())
    }

    pub fn validation_error_count(&self) -> u32 {
        0
    }
}
