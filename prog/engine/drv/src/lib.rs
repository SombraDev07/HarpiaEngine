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
    Backend, ComputePipeline, Extent2D, Format, FrameConstants, FrameInfo, GraphicsPipeline,
    PrimitiveTopology, Texture, TextureDesc,
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
                mip_levels: 1,
                format: Format::Rgba8Unorm,
                sampled: true,
                storage: false,
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
}
