//! Debug overlay. Own descriptor pool (not the bindless 8192). `vk::*` stays here.

use ash::vk;
use egui::{ClippedPrimitive, TextureId, TexturesDelta};
use egui_ash_renderer::{DynamicRendering, Options, Renderer};

use super::resources;
use super::VulkanGpu;
use crate::{FRAMES_IN_FLIGHT, Result, RhiError};

pub struct Overlay {
    renderer: Option<Renderer>,
    format: vk::Format,
    pending_free: Vec<(u64, Vec<TextureId>)>,
    upload_pool: vk::CommandPool,
    device: ash::Device,
}

impl Overlay {
    fn new(
        instance: &ash::Instance,
        phys: vk::PhysicalDevice,
        device: ash::Device,
        graphics_family: u32,
        format: vk::Format,
    ) -> Result<Self> {
        let srgb = matches!(
            format,
            vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB
        );
        let renderer = Renderer::with_default_allocator(
            instance,
            phys,
            device.clone(),
            DynamicRendering {
                color_attachment_format: format,
                depth_attachment_format: None,
            },
            Options {
                in_flight_frames: FRAMES_IN_FLIGHT as usize,
                srgb_framebuffer: srgb,
                ..Options::default()
            },
        )
        .map_err(|e| RhiError::msg(format!("egui renderer: {e}")))?;
        let upload_pool = unsafe {
            device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(graphics_family)
                    .flags(
                        vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER
                            | vk::CommandPoolCreateFlags::TRANSIENT,
                    ),
                None,
            )?
        };
        Ok(Self {
            renderer: Some(renderer),
            format,
            pending_free: Vec::new(),
            upload_pool,
            device,
        })
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        self.renderer.take();
        unsafe {
            self.device.destroy_command_pool(self.upload_pool, None);
        }
    }
}

/// egui-ash-renderer does `extent = clip_w.min(fb)` and does **not** subtract
/// the offset. A tooltip that starts at x=1200 with width 200 on a 1280-wide
/// swapchain becomes scissor (1200, 200) → 1400 > 1280 → validation error →
/// the app loop treats that as fatal and closes.
fn clamp_clips(
    primitives: &[ClippedPrimitive],
    fb: vk::Extent2D,
    ppp: f32,
) -> Vec<ClippedPrimitive> {
    let fb_w = fb.width as f32;
    let fb_h = fb.height as f32;
    let ppp = ppp.max(1.0e-3);
    primitives
        .iter()
        .cloned()
        .filter_map(|mut p| {
            let x0 = (p.clip_rect.min.x * ppp).clamp(0.0, fb_w);
            let y0 = (p.clip_rect.min.y * ppp).clamp(0.0, fb_h);
            let x1 = (p.clip_rect.max.x * ppp).clamp(x0, fb_w);
            let y1 = (p.clip_rect.max.y * ppp).clamp(y0, fb_h);
            if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
                return None;
            }
            p.clip_rect.min.x = x0 / ppp;
            p.clip_rect.min.y = y0 / ppp;
            p.clip_rect.max.x = x1 / ppp;
            p.clip_rect.max.y = y1 / ppp;
            Some(p)
        })
        .collect()
}

impl VulkanGpu {
    fn wait_other_frames(&self) -> Result<()> {
        let fences: Vec<vk::Fence> = self
            .frames
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.slot)
            .map(|(_, f)| f.fence)
            .collect();
        if fences.is_empty() {
            return Ok(());
        }
        unsafe {
            self.device
                .wait_for_fences(&fences, true, super::FRAME_TIMEOUT_NS)
                .map_err(RhiError::from_vk)?;
        }
        Ok(())
    }

    /// Composite egui on the swapchain after the sample has presented.
    ///
    /// The sample must have ended the swapchain pass (image in PRESENT). This
    /// opens a LOAD pass, draws, and returns the image to PRESENT. No-op on an
    /// empty tessellation with no texture deltas.
    pub fn draw_ui(
        &mut self,
        pixels_per_point: f32,
        primitives: &[ClippedPrimitive],
        textures_delta: &TexturesDelta,
    ) -> Result<()> {
        if !self.in_frame {
            return Err(RhiError::NotInFrame);
        }
        if self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        if primitives.is_empty() && textures_delta.set.is_empty() && textures_delta.free.is_empty()
        {
            return Ok(());
        }

        let format = self.swapchain.format.format;
        if self.ui.as_ref().is_some_and(|u| u.format != format) {
            self.ui = None;
        }
        if self.ui.is_none() {
            self.ui = Some(Overlay::new(
                &self.instance,
                self.phys,
                self.device.clone(),
                self.graphics_family,
                format,
            )?);
        }

        let frame_index = self.frame_index;
        let cmd = self.frames[self.slot].cmd;
        let image = self.swapchain.images[self.image_index as usize];
        let view = self.swapchain.views[self.image_index as usize];
        let extent = self.swapchain.extent;
        let queue = self.graphics_queue;
        let clipped = clamp_clips(primitives, extent, pixels_per_point);

        // Glyphs/atlas updates submit a layout transition of the font image.
        // The other in-flight slot may still be sampling it (egui DrawIndexed).
        // begin_frame only waited *this* slot's fence — WAR without this wait.
        if !textures_delta.set.is_empty() {
            self.wait_other_frames()?;
        }

        let device = self.device.clone();
        let ui = self.ui.as_mut().expect("overlay just created");
        let mut i = 0;
        while i < ui.pending_free.len() {
            if frame_index >= ui.pending_free[i].0 + u64::from(FRAMES_IN_FLIGHT) {
                let ids = ui.pending_free.remove(i).1;
                if let Some(renderer) = ui.renderer.as_mut() {
                    let _ = renderer.free_textures(&ids);
                }
            } else {
                i += 1;
            }
        }
        if !textures_delta.free.is_empty() {
            ui.pending_free
                .push((frame_index, textures_delta.free.clone()));
        }
        let renderer = ui
            .renderer
            .as_mut()
            .ok_or_else(|| RhiError::msg("egui renderer dropped"))?;
        if !textures_delta.set.is_empty() {
            renderer
                .set_textures(queue, ui.upload_pool, textures_delta.set.as_slice())
                .map_err(|e| RhiError::msg(format!("egui textures: {e}")))?;
        }

        unsafe {
            resources::image_barrier(
                &device,
                cmd,
                image,
                vk::ImageLayout::PRESENT_SRC_KHR,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::COLOR_ATTACHMENT_READ,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            );

            let attachment = vk::RenderingAttachmentInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::LOAD)
                .store_op(vk::AttachmentStoreOp::STORE);
            let rendering = vk::RenderingInfo::default()
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent,
                })
                .layer_count(1)
                .color_attachments(std::slice::from_ref(&attachment));
            device.cmd_begin_rendering(cmd, &rendering);
        }

        renderer
            .cmd_draw(cmd, extent, pixels_per_point, &clipped)
            .map_err(|e| RhiError::msg(format!("egui draw: {e}")))?;

        unsafe {
            device.cmd_end_rendering(cmd);
            resources::image_barrier(
                &device,
                cmd,
                image,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::ImageLayout::PRESENT_SRC_KHR,
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                vk::AccessFlags::empty(),
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::clamp_clips;
    use ash::vk;
    use egui::{epaint::Primitive, ClippedPrimitive, Pos2, Rect, Mesh};

    fn dummy(min: [f32; 2], max: [f32; 2]) -> ClippedPrimitive {
        ClippedPrimitive {
            clip_rect: Rect::from_min_max(Pos2::new(min[0], min[1]), Pos2::new(max[0], max[1])),
            primitive: Primitive::Mesh(Mesh::default()),
        }
    }

    #[test]
    fn tooltip_past_the_right_edge_fits_the_fb() {
        let out = clamp_clips(
            &[dummy([1200.0, 400.0], [1400.0, 500.0])],
            vk::Extent2D {
                width: 1280,
                height: 720,
            },
            1.0,
        );
        assert_eq!(out.len(), 1);
        let r = out[0].clip_rect;
        let w = (r.max.x - r.min.x).min(1280.0);
        assert!(r.min.x.max(0.0) + w <= 1280.0 + 0.5);
    }
}
