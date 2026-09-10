//! Images, staging buffers, copies. `vk::*` stays in `drv`.

use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator};
use gpu_allocator::MemoryLocation;
use ash::vk;
use ash::Device;

use crate::types::{Format, TextureDesc, TextureDim};
use crate::{RhiError, Result};

/// `copy_buffer_to_texture` row pitch. Landmine 9.
pub const ROW_PITCH_ALIGN: u32 = 256;

#[allow(dead_code)]
pub fn bytes_per_pixel(format: Format) -> Result<u32> {
    match format {
        Format::Rgba8Unorm | Format::Rgba8Srgb | Format::Bgra8Unorm | Format::Bgra8Srgb => Ok(4),
        Format::R32Float | Format::D32Float | Format::Rg16Float => Ok(4),
        Format::Rgba16Float => Ok(8),
        Format::Unknown(_) => Err(RhiError::msg("unknown texture format")),
    }
}

pub fn vk_format(format: Format) -> Result<vk::Format> {
    match format {
        Format::Rgba8Unorm => Ok(vk::Format::R8G8B8A8_UNORM),
        Format::Rgba8Srgb => Ok(vk::Format::R8G8B8A8_SRGB),
        Format::Bgra8Unorm => Ok(vk::Format::B8G8R8A8_UNORM),
        Format::Bgra8Srgb => Ok(vk::Format::B8G8R8A8_SRGB),
        Format::Rgba16Float => Ok(vk::Format::R16G16B16A16_SFLOAT),
        Format::R32Float => Ok(vk::Format::R32_SFLOAT),
        Format::Rg16Float => Ok(vk::Format::R16G16_SFLOAT),
        Format::D32Float => Ok(vk::Format::D32_SFLOAT),
        Format::Unknown(_) => Err(RhiError::msg("unknown texture format")),
    }
}

pub fn aspect_for(format: Format) -> vk::ImageAspectFlags {
    if matches!(format, Format::D32Float) {
        vk::ImageAspectFlags::DEPTH
    } else {
        vk::ImageAspectFlags::COLOR
    }
}

pub fn row_pitch_bytes(width: u32, bpp: u32) -> u32 {
    let row = width.saturating_mul(bpp);
    (row + ROW_PITCH_ALIGN - 1) & !(ROW_PITCH_ALIGN - 1)
}

pub struct GpuBuffer {
    pub buffer: vk::Buffer,
    pub allocation: Allocation,
    pub size: u64,
}

pub struct GpuImage {
    pub image: vk::Image,
    pub dim: TextureDim,
    pub depth_slices: u32,
    pub sampled_view: vk::ImageView,
    pub storage_view: Option<vk::ImageView>,
    pub allocation: Allocation,
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
    #[allow(dead_code)]
    pub format: vk::Format,
    pub engine_format: Format,
    pub bindless_slot: u32,
    pub sampled: bool,
    pub storage: bool,
    pub mips_uploaded: u32,
    pub ready: bool,
    pub transfer_prepared: bool,
    pub layout: vk::ImageLayout,
    #[allow(dead_code)]
    pub color_attachment: bool,
    #[allow(dead_code)]
    pub depth: bool,
}

pub fn create_buffer(
    device: &Device,
    allocator: &mut Allocator,
    size: u64,
    usage: vk::BufferUsageFlags,
    location: MemoryLocation,
    name: &str,
) -> Result<GpuBuffer> {
    let ci = vk::BufferCreateInfo::default()
        .size(size.max(1))
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let buffer = unsafe { device.create_buffer(&ci, None)? };
    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    let allocation = allocator.allocate(&AllocationCreateDesc {
        name,
        requirements,
        location,
        linear: true,
        allocation_scheme: AllocationScheme::GpuAllocatorManaged,
    })?;
    unsafe {
        device.bind_buffer_memory(buffer, allocation.memory(), allocation.offset())?;
    }
    Ok(GpuBuffer {
        buffer,
        allocation,
        size: size.max(1),
    })
}

pub fn destroy_buffer(device: &Device, allocator: &mut Allocator, buf: GpuBuffer) {
    unsafe {
        device.destroy_buffer(buf.buffer, None);
    }
    let _ = allocator.free(buf.allocation);
}

pub fn create_image(
    device: &Device,
    allocator: &mut Allocator,
    desc: &TextureDesc,
    bindless_slot: u32,
) -> Result<GpuImage> {
    if desc.width == 0 || desc.height == 0 {
        return Err(RhiError::msg("texture extent is 0"));
    }
    let mip_levels = desc.mip_levels.max(1);
    let volume = desc.dim == TextureDim::D3;
    let depth_slices = if volume { desc.depth_slices.max(1) } else { 1 };
    if volume && (desc.color_attachment || desc.depth) {
        return Err(RhiError::msg("a 3D texture cannot be an attachment"));
    }
    let format = vk_format(desc.format)?;
    let mut usage = vk::ImageUsageFlags::empty();
    if desc.sampled || desc.color_attachment || desc.depth {
        // sampled GBuffer / IBL still need TRANSFER_DST for CPU upload
    }
    // TRANSFER_SRC on everything: capture (`read_texture`) must work on any
    // target, including the shadow atlas, without a second texture desc flag.
    usage |= vk::ImageUsageFlags::TRANSFER_SRC;
    if !desc.depth {
        usage |= vk::ImageUsageFlags::TRANSFER_DST;
    }
    if desc.sampled {
        usage |= vk::ImageUsageFlags::SAMPLED;
    }
    if desc.storage {
        usage |= vk::ImageUsageFlags::STORAGE;
    }
    if desc.color_attachment {
        usage |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
    }
    if desc.depth {
        usage |= vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT;
    }
    if usage.is_empty() {
        usage = vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST;
    }

    let aspect = aspect_for(desc.format);

    let ci = vk::ImageCreateInfo::default()
        .image_type(if volume {
            vk::ImageType::TYPE_3D
        } else {
            vk::ImageType::TYPE_2D
        })
        .format(format)
        .extent(vk::Extent3D {
            width: desc.width,
            height: desc.height,
            depth: depth_slices,
        })
        .mip_levels(mip_levels)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    let image = unsafe { device.create_image(&ci, None)? };
    let requirements = unsafe { device.get_image_memory_requirements(image) };
    let allocation = allocator.allocate(&AllocationCreateDesc {
        name: "texture",
        requirements,
        location: MemoryLocation::GpuOnly,
        linear: false,
        allocation_scheme: AllocationScheme::GpuAllocatorManaged,
    })?;
    unsafe {
        device.bind_image_memory(image, allocation.memory(), allocation.offset())?;
    }

    let view_type = if volume {
        vk::ImageViewType::TYPE_3D
    } else {
        vk::ImageViewType::TYPE_2D
    };
    let sampled_view = unsafe {
        device.create_image_view(
            &vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(view_type)
                .format(format)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: aspect,
                    base_mip_level: 0,
                    level_count: mip_levels,
                    base_array_layer: 0,
                    layer_count: 1,
                }),
            None,
        )?
    };

    let storage_view = if desc.storage {
        Some(unsafe {
            device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(view_type)
                    .format(format)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    }),
                None,
            )?
        })
    } else {
        None
    };

    Ok(GpuImage {
        image,
        dim: desc.dim,
        depth_slices,
        sampled_view,
        storage_view,
        allocation,
        width: desc.width,
        height: desc.height,
        mip_levels,
        format,
        engine_format: desc.format,
        bindless_slot,
        sampled: desc.sampled,
        storage: desc.storage,
        mips_uploaded: 0,
        ready: false,
        transfer_prepared: false,
        layout: vk::ImageLayout::UNDEFINED,
        color_attachment: desc.color_attachment,
        depth: desc.depth,
    })
}

pub fn destroy_image(device: &Device, allocator: &mut Allocator, img: GpuImage) {
    unsafe {
        if let Some(v) = img.storage_view {
            device.destroy_image_view(v, None);
        }
        device.destroy_image_view(img.sampled_view, None);
        device.destroy_image(img.image, None);
    }
    let _ = allocator.free(img.allocation);
}

pub fn pack_mip(width: u32, height: u32, bpp: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let expected = width as usize * height as usize * bpp as usize;
    if rgba.len() != expected {
        return Err(RhiError::msg(format!(
            "mip size {} != {}x{}x{}",
            rgba.len(),
            width,
            height,
            bpp
        )));
    }
    let pitch = row_pitch_bytes(width, bpp) as usize;
    let src_row = width as usize * bpp as usize;
    let mut out = vec![0u8; pitch * height as usize];
    for y in 0..height as usize {
        let s = y * src_row;
        let d = y * pitch;
        out[d..d + src_row].copy_from_slice(&rgba[s..s + src_row]);
    }
    Ok(out)
}

pub fn mip_extent(base_w: u32, base_h: u32, mip: u32) -> (u32, u32) {
    ((base_w >> mip).max(1), (base_h >> mip).max(1))
}

pub fn write_staging(buf: &GpuBuffer, data: &[u8]) -> Result<()> {
    if data.len() as u64 > buf.size {
        return Err(RhiError::msg("staging buffer too small for this mip"));
    }
    let ptr = buf
        .allocation
        .mapped_ptr()
        .ok_or_else(|| RhiError::msg("staging is not mapped"))?
        .as_ptr()
        .cast::<u8>();
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
    }
    Ok(())
}

pub unsafe fn cmd_copy_mip(
    device: &Device,
    cmd: vk::CommandBuffer,
    staging: vk::Buffer,
    image: vk::Image,
    mip: u32,
    width: u32,
    height: u32,
    bpp: u32,
) {
    let pitch = row_pitch_bytes(width, bpp);
    let region = vk::BufferImageCopy {
        buffer_offset: 0,
        buffer_row_length: pitch / bpp,
        buffer_image_height: 0,
        image_subresource: vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: mip,
            base_array_layer: 0,
            layer_count: 1,
        },
        image_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
        image_extent: vk::Extent3D {
            width,
            height,
            depth: 1,
        },
    };
    unsafe {
        device.cmd_copy_buffer_to_image(
            cmd,
            staging,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            std::slice::from_ref(&region),
        );
    }
}

pub unsafe fn cmd_copy_to_buffer(
    device: &Device,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    buffer: vk::Buffer,
    width: u32,
    height: u32,
    depth: u32,
    bpp: u32,
    aspect: vk::ImageAspectFlags,
) {
    let pitch = row_pitch_bytes(width, bpp);
    let region = vk::BufferImageCopy {
        buffer_offset: 0,
        buffer_row_length: pitch / bpp,
        buffer_image_height: 0,
        image_subresource: vk::ImageSubresourceLayers {
            aspect_mask: aspect,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        },
        image_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
        image_extent: vk::Extent3D {
            width,
            height,
            depth,
        },
    };
    unsafe {
        device.cmd_copy_image_to_buffer(
            cmd,
            image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            buffer,
            std::slice::from_ref(&region),
        );
    }
}

/// Drop the 256-byte row padding that `cmd_copy_to_buffer` writes. Slices are
/// laid out one after another, each `pitch * height` bytes.
pub fn unpack_rows(padded: &[u8], width: u32, height: u32, depth: u32, bpp: u32) -> Vec<u8> {
    let pitch = row_pitch_bytes(width, bpp) as usize;
    let row = width as usize * bpp as usize;
    let slice = pitch * height as usize;
    let mut out = Vec::with_capacity(row * height as usize * depth as usize);
    for z in 0..depth as usize {
        for y in 0..height as usize {
            let s = z * slice + y * pitch;
            out.extend_from_slice(&padded[s..s + row]);
        }
    }
    out
}

pub unsafe fn image_barrier(
    device: &Device,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
    src_access: vk::AccessFlags,
    dst_access: vk::AccessFlags,
    src_stage: vk::PipelineStageFlags,
    dst_stage: vk::PipelineStageFlags,
) {
    unsafe {
        image_barrier_aspect(
            device,
            cmd,
            image,
            old_layout,
            new_layout,
            src_access,
            dst_access,
            src_stage,
            dst_stage,
            vk::ImageAspectFlags::COLOR,
        );
    }
}

pub unsafe fn image_barrier_aspect(
    device: &Device,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
    src_access: vk::AccessFlags,
    dst_access: vk::AccessFlags,
    src_stage: vk::PipelineStageFlags,
    dst_stage: vk::PipelineStageFlags,
    aspect: vk::ImageAspectFlags,
) {
    let barrier = vk::ImageMemoryBarrier::default()
        .src_access_mask(src_access)
        .dst_access_mask(dst_access)
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: aspect,
            base_mip_level: 0,
            level_count: vk::REMAINING_MIP_LEVELS,
            base_array_layer: 0,
            layer_count: vk::REMAINING_ARRAY_LAYERS,
        });
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            src_stage,
            dst_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );
    }
}

pub fn shader_read_stages() -> vk::PipelineStageFlags {
    vk::PipelineStageFlags::VERTEX_SHADER
        | vk::PipelineStageFlags::FRAGMENT_SHADER
        | vk::PipelineStageFlags::COMPUTE_SHADER
}
