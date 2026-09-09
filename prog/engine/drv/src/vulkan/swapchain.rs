use ash::khr;
use ash::vk;
use ash::Device;

use crate::types::{Extent2D, Format};
use crate::{RhiError, Result};

pub struct Swapchain {
    pub raw: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
    pub extent: vk::Extent2D,
    pub format: vk::SurfaceFormatKHR,
}

impl Swapchain {
    pub fn extent2d(&self) -> Extent2D {
        Extent2D {
            width: self.extent.width,
            height: self.extent.height,
        }
    }

    pub fn engine_format(&self) -> Format {
        format_from_vk(self.format.format)
    }
}

pub fn format_from_vk(fmt: vk::Format) -> Format {
    match fmt {
        vk::Format::B8G8R8A8_UNORM => Format::Bgra8Unorm,
        vk::Format::B8G8R8A8_SRGB => Format::Bgra8Srgb,
        vk::Format::R8G8B8A8_UNORM => Format::Rgba8Unorm,
        vk::Format::R8G8B8A8_SRGB => Format::Rgba8Srgb,
        other => Format::Unknown(other.as_raw() as u32),
    }
}

/// Prefer BGRA UNORM (X11/Mesa presentFormat). Never assume RGBA.
pub fn select_surface_format(formats: &[vk::SurfaceFormatKHR]) -> Result<vk::SurfaceFormatKHR> {
    if formats.is_empty() {
        return Err(RhiError::msg("surface has no formats"));
    }
    const PREFER: [vk::Format; 4] = [
        vk::Format::B8G8R8A8_UNORM,
        vk::Format::B8G8R8A8_SRGB,
        vk::Format::R8G8B8A8_UNORM,
        vk::Format::R8G8B8A8_SRGB,
    ];
    for fmt in PREFER {
        if let Some(s) = formats.iter().copied().find(|s| {
            s.format == fmt && s.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        }) {
            return Ok(s);
        }
    }
    Ok(formats[0])
}

pub fn select_present_mode(modes: &[vk::PresentModeKHR]) -> vk::PresentModeKHR {
    if modes.contains(&vk::PresentModeKHR::FIFO) {
        vk::PresentModeKHR::FIFO
    } else {
        modes.first().copied().unwrap_or(vk::PresentModeKHR::FIFO)
    }
}

pub fn clamp_extent(caps: &vk::SurfaceCapabilitiesKHR, requested: Extent2D) -> vk::Extent2D {
    if caps.current_extent.width != u32::MAX {
        caps.current_extent
    } else {
        vk::Extent2D {
            width: requested
                .width
                .clamp(caps.min_image_extent.width, caps.max_image_extent.width),
            height: requested
                .height
                .clamp(caps.min_image_extent.height, caps.max_image_extent.height),
        }
    }
}

pub fn image_count(caps: &vk::SurfaceCapabilitiesKHR) -> u32 {
    let mut n = caps.min_image_count.saturating_add(1).max(2);
    if caps.max_image_count > 0 {
        n = n.min(caps.max_image_count);
    }
    n.max(caps.min_image_count)
}

pub unsafe fn destroy(device: &Device, swapchain_fn: &khr::swapchain::Device, swapchain: &mut Swapchain) {
    for view in swapchain.views.drain(..) {
        unsafe { device.destroy_image_view(view, None) };
    }
    if swapchain.raw != vk::SwapchainKHR::null() {
        unsafe { swapchain_fn.destroy_swapchain(swapchain.raw, None) };
        swapchain.raw = vk::SwapchainKHR::null();
    }
    swapchain.images.clear();
}

pub unsafe fn create(
    device: &Device,
    surface_fn: &khr::surface::Instance,
    swapchain_fn: &khr::swapchain::Device,
    phys: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
    _graphics_family: u32,
    requested: Extent2D,
    old: vk::SwapchainKHR,
) -> Result<Swapchain> {
    let caps = unsafe { surface_fn.get_physical_device_surface_capabilities(phys, surface)? };
    let formats = unsafe { surface_fn.get_physical_device_surface_formats(phys, surface)? };
    let modes = unsafe { surface_fn.get_physical_device_surface_present_modes(phys, surface)? };

    let format = select_surface_format(&formats)?;
    let extent = clamp_extent(&caps, requested);
    if extent.width == 0 || extent.height == 0 {
        return Err(RhiError::msg("swapchain extent is 0"));
    }

    let composite = if caps
        .supported_composite_alpha
        .contains(vk::CompositeAlphaFlagsKHR::OPAQUE)
    {
        vk::CompositeAlphaFlagsKHR::OPAQUE
    } else {
        vk::CompositeAlphaFlagsKHR::INHERIT
    };

    let info = vk::SwapchainCreateInfoKHR::default()
        .surface(surface)
        .min_image_count(image_count(&caps))
        .image_format(format.format)
        .image_color_space(format.color_space)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        .pre_transform(caps.current_transform)
        .composite_alpha(composite)
        .present_mode(select_present_mode(&modes))
        .clipped(true)
        .old_swapchain(old);

    let raw = unsafe { swapchain_fn.create_swapchain(&info, None)? };
    let images = unsafe { swapchain_fn.get_swapchain_images(raw)? };

    let mut views = Vec::with_capacity(images.len());
    for &image in &images {
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format.format)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            });
        views.push(unsafe { device.create_image_view(&view_info, None)? });
    }

    tracing::info!(
        format = ?format.format,
        color_space = ?format.color_space,
        width = extent.width,
        height = extent.height,
        images = images.len(),
        "swapchain created"
    );

    Ok(Swapchain {
        raw,
        images,
        views,
        extent,
        format,
    })
}
