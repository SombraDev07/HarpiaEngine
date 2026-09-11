//! Render hardware interface.
//!
//! **`vk::*` lives only in this crate** (`vulkan/` and `window.rs`). Engine
//! crates talk to [`Device`]. RHI is single-threaded: create, record, submit
//! on the thread that owns the device.

mod device;
mod error;
mod null;
mod types;
mod vulkan;

pub use device::{
    create, ComputePipelineDesc, Device, DeviceDesc, Gpu, GraphicsPipelineDesc, WindowHandles,
};
pub use error::RhiError;
pub use types::{
    Backend, Barrier, BarrierDesc, Buffer, ComputePipeline, Extent2D, Format, FrameConstants,
    FrameInfo, GpuStats,
    GraphicsPipeline,
    PipelineTargets, PrimitiveTopology, Texture, TextureData, TextureDesc, TextureDim,
    FRAME_CBV_CHUNKS, FRAME_UBO_SIZE, PUSH_CONSTANTS_SIZE, STORAGE_BUFFER_SLOTS,
    STORAGE_IMAGE_SLOTS,
    VOLUME_SRV_SLOTS, VOLUME_UAV_SLOTS,
};

pub type Result<T, E = RhiError> = std::result::Result<T, E>;

/// In-flight frames. `begin_frame` waits the fence of the current slot.
pub const FRAMES_IN_FLIGHT: u32 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_device_draws_without_gpu() {
        let mut gpu = create(&DeviceDesc {
            vsync: true,
            backend: Backend::Null,
            validation: false,
            app_name: "test",
            width: 64,
            height: 64,
            window: None,
        })
        .unwrap();

        let info = gpu.begin_frame().unwrap();
        assert!(!info.skipped);
        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0]).unwrap();
        let pso = gpu
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: &[],
                fs_spirv: &[],
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: false,
                targets: PipelineTargets::default(),
            })
            .unwrap();
        gpu.set_pipeline(&pso).unwrap();
        gpu.draw(3, 1, 0, 0).unwrap();
        gpu.end_swapchain_pass().unwrap();
        gpu.end_frame().unwrap();
        assert_eq!(gpu.validation_error_count(), 0);
    }

    #[test]
    fn null_bindless_upload_and_dispatch() {
        let mut gpu = create(&DeviceDesc {
            vsync: true,
            backend: Backend::Null,
            validation: false,
            app_name: "test",
            width: 64,
            height: 64,
            window: None,
        })
        .unwrap();
        let tex = gpu
            .create_texture(&TextureDesc {
                width: 4,
                height: 4,
                depth_slices: 1,
                dim: TextureDim::D2,
                mip_levels: 1,
                format: Format::Rgba8Unorm,
                sampled: true,
                storage: false,
                color_attachment: false,
                depth: false,
            })
            .unwrap();
        gpu.upload_texture_mip(tex, 0, &[0u8; 64]).unwrap();
        assert_ne!(gpu.bindless_index(tex).unwrap(), 0);
        assert_eq!(gpu.bindless_index(Texture::NULL).unwrap(), 0);
        let _ = gpu.begin_frame().unwrap();
        gpu.write_frame_constants(FrameConstants {
            tex_a: 1,
            tex_b: 0,
            _pad: [0, 0],
        })
        .unwrap();
        gpu.bind_compute_bindless().unwrap();
        gpu.dispatch(1, 1, 1).unwrap();
        gpu.end_frame().unwrap();
    }

    #[test]
    fn null_mrt_and_indexed_draw() {
        let mut gpu = create(&DeviceDesc {
            vsync: true,
            backend: Backend::Null,
            validation: false,
            app_name: "test",
            width: 64,
            height: 64,
            window: None,
        })
        .unwrap();
        let rt = gpu
            .create_texture(&TextureDesc {
                width: 64,
                height: 64,
                depth_slices: 1,
                dim: TextureDim::D2,
                mip_levels: 1,
                format: Format::Rgba8Unorm,
                sampled: true,
                storage: false,
                color_attachment: true,
                depth: false,
            })
            .unwrap();
        let depth = gpu
            .create_texture(&TextureDesc {
                width: 64,
                height: 64,
                depth_slices: 1,
                dim: TextureDim::D2,
                mip_levels: 1,
                format: Format::D32Float,
                sampled: false,
                storage: false,
                color_attachment: false,
                depth: true,
            })
            .unwrap();
        let vb = gpu.create_vertex_buffer(&[0u8; 24]).unwrap();
        let ib = gpu.create_index_buffer(&[0u8, 0, 0, 0]).unwrap();
        let _ = gpu.begin_frame().unwrap();
        gpu.write_frame_bytes(&[0u8; 16]).unwrap();
        gpu.begin_color_pass(&[rt], Some(depth), &[[0.0; 4]], Some(1.0))
            .unwrap();
        gpu.bind_vertex_buffer(vb, 0).unwrap();
        gpu.bind_index_buffer(ib).unwrap();
        gpu.set_push_constants(&[0u8; 128]).unwrap();
        gpu.draw_indexed(3, 1, 0, 0, 0).unwrap();
        gpu.end_color_pass().unwrap();
        gpu.end_frame().unwrap();
    }

    #[test]
    fn null_depth_only_pass_and_viewport() {
        let mut gpu = create(&DeviceDesc {
            vsync: true,
            backend: Backend::Null,
            validation: false,
            app_name: "test",
            width: 64,
            height: 64,
            window: None,
        })
        .unwrap();
        let atlas = gpu
            .create_texture(&TextureDesc {
                width: 64,
                height: 64,
                depth_slices: 1,
                dim: TextureDim::D2,
                mip_levels: 1,
                format: Format::D32Float,
                sampled: true,
                storage: false,
                color_attachment: false,
                depth: true,
            })
            .unwrap();
        let _ = gpu.begin_frame().unwrap();
        gpu.begin_color_pass(&[], Some(atlas), &[], Some(1.0))
            .unwrap();
        gpu.set_viewport(0.0, 0.0, 32.0, 32.0).unwrap();
        gpu.set_viewport(32.0, 0.0, 32.0, 32.0).unwrap();
        gpu.end_color_pass().unwrap();
        gpu.end_frame().unwrap();
    }
}
