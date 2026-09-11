use crate::device::{ComputePipelineDesc, DeviceDesc, GraphicsPipelineDesc};
use crate::types::{
    Buffer, ComputePipeline, Extent2D, Format, FrameConstants, FrameInfo, GraphicsPipeline, Texture,
    TextureData, TextureDesc, FRAME_CBV_CHUNKS,
};
use crate::{RhiError, Result};

struct NullTex {
    bindless_slot: u32,
    mip_levels: u32,
    uploaded: u32,
}

pub struct NullGpu {
    extent: Extent2D,
    in_frame: bool,
    in_pass: bool,
    frame_index: u64,
    next_pipeline: u32,
    next_compute: u32,
    next_slot: u32,
    next_buffer: u32,
    /// Same budget as Vulkan, so a gate that overflows the ring fails on CPU too.
    cbv_next: u32,
    textures: Vec<NullTex>,
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
            next_compute: 1,
            next_slot: 1,
            next_buffer: 1,
            cbv_next: 0,
            textures: vec![NullTex {
                bindless_slot: 0,
                mip_levels: 1,
                uploaded: 1,
            }],
        }
    }

    pub fn begin_frame(&mut self) -> Result<FrameInfo> {
        if self.in_frame {
            return Err(RhiError::msg("begin_frame while a frame is open"));
        }
        self.in_frame = true;
        self.cbv_next = 0;
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

    /// O backend Null não tem GPU: um storage buffer é um handle e nada mais.
    pub fn create_storage_buffer(&mut self, _slot: u32, bytes: &[u8]) -> crate::Result<crate::types::Buffer> {
        self.create_vertex_buffer(bytes)
    }

    pub fn write_storage_buffer(&mut self, _buffer: crate::types::Buffer, _bytes: &[u8]) -> crate::Result<()> {
        Ok(())
    }

    pub fn mark(&mut self, _label: &'static str) {}

    /// The Null backend never touches a GPU, so there is nothing to time.
    pub fn take_stats(&mut self) -> crate::types::GpuStats {
        crate::types::GpuStats::default()
    }

    pub fn validation_error_count(&self) -> u32 {
        0
    }

    pub fn create_texture(&mut self, desc: &TextureDesc) -> Result<Texture> {
        let slot = self.next_slot;
        self.next_slot += 1;
        let id = self.textures.len() as u32;
        self.textures.push(NullTex {
            bindless_slot: slot,
            mip_levels: desc.mip_levels.max(1),
            uploaded: 0,
        });
        Ok(Texture { id })
    }

    pub fn upload_texture_mip(&mut self, tex: Texture, mip: u32, _rgba: &[u8]) -> Result<()> {
        let t = self
            .textures
            .get_mut(tex.id as usize)
            .ok_or_else(|| RhiError::msg("invalid texture"))?;
        if mip >= t.mip_levels {
            return Err(RhiError::msg("mip out of range"));
        }
        t.uploaded = t.uploaded.saturating_add(1).min(t.mip_levels);
        Ok(())
    }

    pub fn bind_volume_uav(&mut self, slot: u32, _tex: Texture) -> Result<()> {
        if slot >= crate::types::VOLUME_UAV_SLOTS {
            return Err(RhiError::msg("volume UAV slot out of range"));
        }
        Ok(())
    }

    pub fn bind_volume_srv(&mut self, slot: u32, _tex: Texture) -> Result<()> {
        if slot >= crate::types::VOLUME_SRV_SLOTS {
            return Err(RhiError::msg("volume SRV slot out of range"));
        }
        Ok(())
    }

    /// No pixels on the Null backend; capture is a Vulkan-only path.
    pub fn read_texture(&mut self, _tex: Texture) -> Result<TextureData> {
        Err(RhiError::msg("read_texture needs the Vulkan backend"))
    }

    pub fn bindless_index(&self, tex: Texture) -> Result<u32> {
        Ok(self
            .textures
            .get(tex.id as usize)
            .ok_or_else(|| RhiError::msg("invalid texture"))?
            .bindless_slot)
    }

    pub fn write_frame_constants(&mut self, _c: FrameConstants) -> Result<()> {
        if !self.in_frame {
            return Err(RhiError::NotInFrame);
        }
        self.take_cbv_chunk()
    }

    pub fn bind_graphics_bindless(&mut self) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        Ok(())
    }

    pub fn create_compute_pipeline(&mut self, _desc: &ComputePipelineDesc<'_>) -> Result<ComputePipeline> {
        let id = self.next_compute;
        self.next_compute += 1;
        Ok(ComputePipeline { id })
    }

    pub fn set_compute_pipeline(&mut self, _pipeline: &ComputePipeline) -> Result<()> {
        if !self.in_frame || self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        Ok(())
    }

    pub fn bind_compute_bindless(&mut self) -> Result<()> {
        if !self.in_frame || self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        Ok(())
    }

    pub fn dispatch(&mut self, _x: u32, _y: u32, _z: u32) -> Result<()> {
        if !self.in_frame || self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        Ok(())
    }

    pub fn storage_barrier(&mut self, _tex: Texture) -> Result<()> {
        if !self.in_frame {
            return Err(RhiError::NotInFrame);
        }
        Ok(())
    }

    pub fn write_frame_bytes(&mut self, _data: &[u8]) -> Result<()> {
        if !self.in_frame {
            return Err(RhiError::NotInFrame);
        }
        self.take_cbv_chunk()
    }

    fn take_cbv_chunk(&mut self) -> Result<()> {
        if self.cbv_next >= FRAME_CBV_CHUNKS {
            return Err(RhiError::msg(
                "frame CBV ring exhausted: more write_frame_bytes than FRAME_CBV_CHUNKS",
            ));
        }
        self.cbv_next += 1;
        Ok(())
    }

    pub fn set_push_constants(&mut self, _data: &[u8]) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        Ok(())
    }

    pub fn set_viewport(&mut self, _x: f32, _y: f32, _width: f32, _height: f32) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        Ok(())
    }

    pub fn begin_color_pass(
        &mut self,
        colors: &[Texture],
        depth: Option<Texture>,
        _clears: &[[f32; 4]],
        _depth_clear: Option<f32>,
    ) -> Result<()> {
        if !self.in_frame {
            return Err(RhiError::NotInFrame);
        }
        if self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        if colors.is_empty() && depth.is_none() {
            return Err(RhiError::msg(
                "begin_color_pass needs a color RT or a depth target",
            ));
        }
        self.in_pass = true;
        Ok(())
    }

    pub fn end_color_pass(&mut self) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        self.in_pass = false;
        Ok(())
    }

    pub fn create_vertex_buffer(&mut self, _bytes: &[u8]) -> Result<Buffer> {
        let id = self.next_buffer;
        self.next_buffer += 1;
        Ok(Buffer { id })
    }

    pub fn create_index_buffer(&mut self, _bytes: &[u8]) -> Result<Buffer> {
        let id = self.next_buffer;
        self.next_buffer += 1;
        Ok(Buffer { id })
    }

    pub fn bind_vertex_buffer(&mut self, _buf: Buffer, _binding: u32) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        Ok(())
    }

    pub fn bind_index_buffer(&mut self, _buf: Buffer) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        Ok(())
    }

    pub fn draw_indexed(
        &mut self,
        _index_count: u32,
        _instance_count: u32,
        _first_index: u32,
        _vertex_offset: i32,
        _first_instance: u32,
    ) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        Ok(())
    }
}
