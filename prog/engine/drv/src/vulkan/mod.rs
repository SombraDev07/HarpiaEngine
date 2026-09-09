//! Vulkan 1.3 backend. All `vk::*` / `vkCmd*` live under this module.

mod debug;
mod layers;
mod swapchain;
mod window;

use std::ffi::{CStr, CString};
use std::io::Cursor;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use ash::ext::debug_utils;
use ash::khr;
use ash::vk;
use ash::{Device, Entry, Instance};
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};

use crate::device::{DeviceDesc, GraphicsPipelineDesc};
use crate::types::{Extent2D, Format, FrameInfo, GraphicsPipeline};
use crate::{RhiError, Result, FRAMES_IN_FLIGHT};

const FRAME_TIMEOUT_NS: u64 = 2_000_000_000;
const VALIDATION_LAYER: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(b"VK_LAYER_KHRONOS_validation\0") };

struct FrameSlot {
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    image_available: vk::Semaphore,
    render_finished: vk::Semaphore,
}

struct DebugState {
    loader: debug_utils::Instance,
    messenger: vk::DebugUtilsMessengerEXT,
    user_data: *mut std::ffi::c_void,
}

pub struct VulkanGpu {
    #[allow(dead_code)]
    entry: Entry,
    instance: Instance,
    debug: Option<DebugState>,
    surface_fn: khr::surface::Instance,
    surface: vk::SurfaceKHR,
    phys: vk::PhysicalDevice,
    device: Device,
    graphics_queue: vk::Queue,
    graphics_family: u32,
    swapchain_fn: khr::swapchain::Device,
    swapchain: swapchain::Swapchain,
    /// Kept alive for phase 2 uploads. Unused in hello-triangle.
    #[allow(dead_code)]
    allocator: Option<Allocator>,
    cmd_pool: vk::CommandPool,
    empty_layout: vk::PipelineLayout,
    pipelines: Vec<vk::Pipeline>,
    frames: Vec<FrameSlot>,
    frame_index: u64,
    slot: usize,
    image_index: u32,
    in_frame: bool,
    in_pass: bool,
    pending_recreate: bool,
    zero_extent: bool,
    window_extent: Extent2D,
    validation_errors: Arc<AtomicU32>,
}

impl VulkanGpu {
    pub fn new(desc: &DeviceDesc) -> Result<Self> {
        let handles = desc.window.ok_or(RhiError::WindowRequired)?;
        let window_extent = Extent2D {
            width: desc.width.max(1),
            height: desc.height.max(1),
        };

        if desc.validation {
            layers::expose_khronos_validation();
        }

        let entry = unsafe { Entry::load()? };

        if desc.validation && !has_layer(&entry, VALIDATION_LAYER)? {
            return Err(RhiError::ValidationLayerMissing);
        }

        let app_name = CString::new(desc.app_name).map_err(|_| RhiError::BadCString)?;
        let engine_name = CString::new(harpia_core::ENGINE_NAME).map_err(|_| RhiError::BadCString)?;
        let app_info = vk::ApplicationInfo::default()
            .application_name(app_name.as_c_str())
            .application_version(harpia_core::engine_vk_version())
            .engine_name(engine_name.as_c_str())
            .engine_version(harpia_core::engine_vk_version())
            .api_version(vk::API_VERSION_1_3);

        let mut ext_ptrs = window::instance_extension_ptrs(handles.display)?;
        if desc.validation {
            let debug_name = debug_utils::NAME.as_ptr();
            if !ext_ptrs.contains(&debug_name) {
                ext_ptrs.push(debug_name);
            }
        }

        let layer_ptr = VALIDATION_LAYER.as_ptr();
        let layer_ptrs = if desc.validation {
            vec![layer_ptr]
        } else {
            Vec::new()
        };

        let validation_errors = Arc::new(AtomicU32::new(0));
        let user_data = debug::leak_error_counter(&validation_errors);

        let mut debug_ci = vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(
                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                    | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                    | vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            )
            .pfn_user_callback(Some(debug::debug_callback))
            .user_data(user_data);

        let mut instance_ci = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&ext_ptrs)
            .enabled_layer_names(&layer_ptrs);
        if desc.validation {
            instance_ci = instance_ci.push_next(&mut debug_ci);
        }

        let instance = unsafe { entry.create_instance(&instance_ci, None)? };

        let debug = if desc.validation {
            let loader = debug_utils::Instance::new(&entry, &instance);
            let messenger = unsafe { loader.create_debug_utils_messenger(&debug_ci, None)? };
            Some(DebugState {
                loader,
                messenger,
                user_data,
            })
        } else {
            unsafe { debug::release_error_counter(user_data) };
            None
        };

        let surface_fn = khr::surface::Instance::new(&entry, &instance);
        let surface = unsafe {
            window::create_surface(&entry, &instance, handles.display, handles.window)?
        };

        let (phys, graphics_family) = pick_device(&instance, &surface_fn, surface)?;
        let props = unsafe { instance.get_physical_device_properties(phys) };
        let name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) };
        tracing::info!(gpu = %name.to_string_lossy(), "Vulkan device");

        let mut avail13 = vk::PhysicalDeviceVulkan13Features::default();
        let mut avail12 = vk::PhysicalDeviceVulkan12Features::default();
        let mut avail = vk::PhysicalDeviceFeatures2::default()
            .push_next(&mut avail13)
            .push_next(&mut avail12);
        unsafe { instance.get_physical_device_features2(phys, &mut avail) };
        if avail13.dynamic_rendering == vk::FALSE {
            return Err(RhiError::msg("GPU lacks dynamic rendering (Vulkan 1.3)"));
        }

        let mut vk13 = vk::PhysicalDeviceVulkan13Features::default().dynamic_rendering(true);
        let mut vk12 = vk::PhysicalDeviceVulkan12Features::default()
            .descriptor_indexing(avail12.descriptor_indexing == vk::TRUE)
            .descriptor_binding_partially_bound(avail12.descriptor_binding_partially_bound == vk::TRUE)
            .descriptor_binding_sampled_image_update_after_bind(
                avail12.descriptor_binding_sampled_image_update_after_bind == vk::TRUE,
            )
            .descriptor_binding_variable_descriptor_count(
                avail12.descriptor_binding_variable_descriptor_count == vk::TRUE,
            )
            .runtime_descriptor_array(avail12.runtime_descriptor_array == vk::TRUE)
            .shader_sampled_image_array_non_uniform_indexing(
                avail12.shader_sampled_image_array_non_uniform_indexing == vk::TRUE,
            );
        let mut features2 = vk::PhysicalDeviceFeatures2::default()
            .push_next(&mut vk13)
            .push_next(&mut vk12);

        let prios = [1.0f32];
        let queue_ci = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(graphics_family)
            .queue_priorities(&prios);
        let device_exts = [khr::swapchain::NAME.as_ptr()];
        let device_ci = vk::DeviceCreateInfo::default()
            .push_next(&mut features2)
            .queue_create_infos(std::slice::from_ref(&queue_ci))
            .enabled_extension_names(&device_exts);

        let device = unsafe { instance.create_device(phys, &device_ci, None)? };
        let graphics_queue = unsafe { device.get_device_queue(graphics_family, 0) };
        let swapchain_fn = khr::swapchain::Device::new(&instance, &device);

        let allocator = match Allocator::new(&AllocatorCreateDesc {
            instance: instance.clone(),
            device: device.clone(),
            physical_device: phys,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        }) {
            Ok(a) => Some(a),
            Err(e) => {
                tracing::warn!("gpu-allocator init failed (ok until phase 2): {e}");
                None
            }
        };

        let swapchain = unsafe {
            swapchain::create(
                &device,
                &surface_fn,
                &swapchain_fn,
                phys,
                surface,
                graphics_family,
                window_extent,
                vk::SwapchainKHR::null(),
            )?
        };

        let pool_ci = vk::CommandPoolCreateInfo::default()
            .queue_family_index(graphics_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let cmd_pool = unsafe { device.create_command_pool(&pool_ci, None)? };

        let alloc_ci = vk::CommandBufferAllocateInfo::default()
            .command_pool(cmd_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(FRAMES_IN_FLIGHT);
        let cmds = unsafe { device.allocate_command_buffers(&alloc_ci)? };

        let mut frames = Vec::with_capacity(FRAMES_IN_FLIGHT as usize);
        for i in 0..FRAMES_IN_FLIGHT as usize {
            let fence = unsafe {
                device.create_fence(
                    &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )?
            };
            let image_available =
                unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)? };
            let render_finished =
                unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)? };
            frames.push(FrameSlot {
                cmd: cmds[i],
                fence,
                image_available,
                render_finished,
            });
        }

        let empty_layout = unsafe {
            device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)?
        };

        Ok(Self {
            entry,
            instance,
            debug,
            surface_fn,
            surface,
            phys,
            device,
            graphics_queue,
            graphics_family,
            swapchain_fn,
            swapchain,
            allocator,
            cmd_pool,
            empty_layout,
            pipelines: Vec::new(),
            frames,
            frame_index: 0,
            slot: 0,
            image_index: 0,
            in_frame: false,
            in_pass: false,
            pending_recreate: false,
            zero_extent: false,
            window_extent,
            validation_errors,
        })
    }

    pub fn begin_frame(&mut self) -> Result<FrameInfo> {
        if self.in_frame {
            return Err(RhiError::msg("begin_frame while a frame is open"));
        }
        if self.zero_extent {
            return Ok(self.skipped_info());
        }
        if self.pending_recreate {
            self.recreate_swapchain()?;
            self.pending_recreate = false;
        }

        let slot = (self.frame_index as usize) % self.frames.len();
        let fence = self.frames[slot].fence;
        unsafe {
            match self.device.wait_for_fences(&[fence], true, FRAME_TIMEOUT_NS) {
                Ok(()) => {}
                Err(vk::Result::TIMEOUT) => return Err(RhiError::FrameTimeout),
                Err(e) => return Err(RhiError::from_vk(e)),
            }
        }

        let mut tries = 0;
        let image_index = loop {
            tries += 1;
            if tries > 3 {
                return Err(RhiError::msg("swapchain acquire failed after recreate"));
            }
            let image_available = self.frames[slot].image_available;
            match unsafe {
                self.swapchain_fn.acquire_next_image(
                    self.swapchain.raw,
                    FRAME_TIMEOUT_NS,
                    image_available,
                    vk::Fence::null(),
                )
            } {
                Ok((idx, suboptimal)) => {
                    if suboptimal {
                        self.pending_recreate = true;
                    }
                    break idx;
                }
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.recreate_swapchain()?;
                    continue;
                }
                Err(vk::Result::TIMEOUT) => return Err(RhiError::FrameTimeout),
                Err(e) => return Err(RhiError::from_vk(e)),
            }
        };

        unsafe {
            self.device.reset_fences(&[fence])?;
        }

        self.slot = slot;
        self.image_index = image_index;
        self.in_frame = true;
        self.in_pass = false;

        let cmd = self.frames[slot].cmd;
        unsafe {
            self.device
                .reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
            let begin = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device.begin_command_buffer(cmd, &begin)?;
        }

        Ok(FrameInfo {
            extent: self.swapchain.extent2d(),
            format: self.swapchain.engine_format(),
            frame_index: self.frame_index,
            skipped: false,
        })
    }

    pub fn begin_swapchain_pass(&mut self, clear: [f32; 4]) -> Result<()> {
        if !self.in_frame {
            return Err(RhiError::NotInFrame);
        }
        if self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        let cmd = self.frames[self.slot].cmd;
        let image = self.swapchain.images[self.image_index as usize];
        let view = self.swapchain.views[self.image_index as usize];
        let extent = self.swapchain.extent;

        unsafe {
            image_barrier(
                &self.device,
                cmd,
                image,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            );

            let attachment = vk::RenderingAttachmentInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue {
                    color: vk::ClearColorValue { float32: clear },
                });
            let rendering = vk::RenderingInfo::default()
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent,
                })
                .layer_count(1)
                .color_attachments(std::slice::from_ref(&attachment));
            self.device.cmd_begin_rendering(cmd, &rendering);

            let viewport = vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: extent.width as f32,
                height: extent.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            };
            let scissor = vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent,
            };
            self.device.cmd_set_viewport(cmd, 0, &[viewport]);
            self.device.cmd_set_scissor(cmd, 0, &[scissor]);
        }

        self.in_pass = true;
        Ok(())
    }

    pub fn set_pipeline(&mut self, pipeline: &GraphicsPipeline) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        let pso = self
            .pipelines
            .get(pipeline.id as usize)
            .copied()
            .filter(|p| *p != vk::Pipeline::null())
            .ok_or_else(|| RhiError::msg("invalid pipeline"))?;
        unsafe {
            self.device.cmd_bind_pipeline(
                self.frames[self.slot].cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pso,
            );
        }
        Ok(())
    }

    pub fn draw(
        &mut self,
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
    ) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        unsafe {
            self.device.cmd_draw(
                self.frames[self.slot].cmd,
                vertex_count,
                instance_count,
                first_vertex,
                first_instance,
            );
        }
        Ok(())
    }

    pub fn end_swapchain_pass(&mut self) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        let cmd = self.frames[self.slot].cmd;
        let image = self.swapchain.images[self.image_index as usize];
        unsafe {
            self.device.cmd_end_rendering(cmd);
            image_barrier(
                &self.device,
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

        let slot = self.slot;
        let cmd = self.frames[slot].cmd;
        unsafe {
            self.device.end_command_buffer(cmd)?;
        }

        let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let submit = vk::SubmitInfo::default()
            .wait_semaphores(std::slice::from_ref(&self.frames[slot].image_available))
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(std::slice::from_ref(&cmd))
            .signal_semaphores(std::slice::from_ref(&self.frames[slot].render_finished));

        unsafe {
            self.device
                .queue_submit(self.graphics_queue, &[submit], self.frames[slot].fence)
                .map_err(RhiError::from_vk)?;
        }

        let present = vk::PresentInfoKHR::default()
            .wait_semaphores(std::slice::from_ref(&self.frames[slot].render_finished))
            .swapchains(std::slice::from_ref(&self.swapchain.raw))
            .image_indices(std::slice::from_ref(&self.image_index));

        match unsafe { self.swapchain_fn.queue_present(self.graphics_queue, &present) } {
            Ok(_) => {}
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                self.pending_recreate = true;
            }
            Err(e) => return Err(RhiError::from_vk(e)),
        }

        self.in_frame = false;
        self.frame_index += 1;
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        self.window_extent = Extent2D { width, height };
        if width == 0 || height == 0 {
            self.zero_extent = true;
            return Ok(());
        }
        if !self.zero_extent
            && width == self.swapchain.extent.width
            && height == self.swapchain.extent.height
        {
            return Ok(());
        }
        self.zero_extent = false;
        self.recreate_swapchain()?;
        self.pending_recreate = false;
        Ok(())
    }

    pub fn present_format(&self) -> Format {
        self.swapchain.engine_format()
    }

    pub fn extent(&self) -> Extent2D {
        self.swapchain.extent2d()
    }

    pub fn create_graphics_pipeline(&mut self, desc: &GraphicsPipelineDesc<'_>) -> Result<GraphicsPipeline> {
        if desc.vs_spirv.is_empty() || desc.fs_spirv.is_empty() {
            return Err(RhiError::msg("SPIR-V modules are empty"));
        }
        let vs_words = ash::util::read_spv(&mut Cursor::new(desc.vs_spirv))?;
        let fs_words = ash::util::read_spv(&mut Cursor::new(desc.fs_spirv))?;
        let vs_mod = unsafe {
            self.device.create_shader_module(
                &vk::ShaderModuleCreateInfo::default().code(&vs_words),
                None,
            )?
        };
        let fs_mod = unsafe {
            self.device.create_shader_module(
                &vk::ShaderModuleCreateInfo::default().code(&fs_words),
                None,
            )?
        };

        let vs_entry = CString::new(desc.vs_entry).map_err(|_| RhiError::BadCString)?;
        let fs_entry = CString::new(desc.fs_entry).map_err(|_| RhiError::BadCString)?;
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vs_mod)
                .name(vs_entry.as_c_str()),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(fs_mod)
                .name(fs_entry.as_c_str()),
        ];

        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .line_width(1.0);
        let msaa = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let blend_attachment = vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA);
        let blend = vk::PipelineColorBlendStateCreateInfo::default()
            .attachments(std::slice::from_ref(&blend_attachment));
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

        let color_format = self.swapchain.format.format;
        let mut rendering = vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(std::slice::from_ref(&color_format));

        let ci = vk::GraphicsPipelineCreateInfo::default()
            .push_next(&mut rendering)
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&raster)
            .multisample_state(&msaa)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(self.empty_layout);

        let created = unsafe {
            self.device
                .create_graphics_pipelines(vk::PipelineCache::null(), &[ci], None)
        };
        unsafe {
            self.device.destroy_shader_module(vs_mod, None);
            self.device.destroy_shader_module(fs_mod, None);
        }
        let pipelines = created.map_err(|(_p, e)| RhiError::from_vk(e))?;
        let pso = pipelines[0];
        let id = self.pipelines.len() as u32;
        self.pipelines.push(pso);
        Ok(GraphicsPipeline { id })
    }

    pub fn wait_idle(&self) -> Result<()> {
        unsafe { self.device.device_wait_idle().map_err(RhiError::from_vk) }
    }

    pub fn validation_error_count(&self) -> u32 {
        self.validation_errors
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn skipped_info(&self) -> FrameInfo {
        FrameInfo {
            extent: self.window_extent,
            format: self.swapchain.engine_format(),
            frame_index: self.frame_index,
            skipped: true,
        }
    }

    fn wait_all_fences(&self) -> Result<()> {
        let fences: Vec<vk::Fence> = self.frames.iter().map(|f| f.fence).collect();
        unsafe {
            match self.device.wait_for_fences(&fences, true, FRAME_TIMEOUT_NS) {
                Ok(()) => Ok(()),
                Err(vk::Result::TIMEOUT) => Err(RhiError::FrameTimeout),
                Err(e) => Err(RhiError::from_vk(e)),
            }
        }
    }

    fn recreate_swapchain(&mut self) -> Result<()> {
        if self.window_extent.is_zero() {
            self.zero_extent = true;
            return Ok(());
        }
        self.wait_all_fences()?;
        unsafe {
            for view in self.swapchain.views.drain(..) {
                self.device.destroy_image_view(view, None);
            }
            let old = self.swapchain.raw;
            match swapchain::create(
                &self.device,
                &self.surface_fn,
                &self.swapchain_fn,
                self.phys,
                self.surface,
                self.graphics_family,
                self.window_extent,
                old,
            ) {
                Ok(new) => {
                    if old != vk::SwapchainKHR::null() {
                        self.swapchain_fn.destroy_swapchain(old, None);
                    }
                    self.swapchain = new;
                    Ok(())
                }
                Err(e) => {
                    // Keep old swapchain object if create failed; views are already gone.
                    if old != vk::SwapchainKHR::null() {
                        self.swapchain_fn.destroy_swapchain(old, None);
                        self.swapchain.raw = vk::SwapchainKHR::null();
                    }
                    Err(e)
                }
            }
        }
    }
}

impl Drop for VulkanGpu {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            for pso in self.pipelines.drain(..) {
                self.device.destroy_pipeline(pso, None);
            }
            self.device.destroy_pipeline_layout(self.empty_layout, None);
            for frame in &self.frames {
                self.device.destroy_fence(frame.fence, None);
                self.device.destroy_semaphore(frame.image_available, None);
                self.device.destroy_semaphore(frame.render_finished, None);
            }
            self.device.destroy_command_pool(self.cmd_pool, None);
            swapchain::destroy(&self.device, &self.swapchain_fn, &mut self.swapchain);
            self.allocator.take();
            self.device.destroy_device(None);
            self.surface_fn.destroy_surface(self.surface, None);
            if let Some(debug) = self.debug.take() {
                debug.loader.destroy_debug_utils_messenger(debug.messenger, None);
                debug::release_error_counter(debug.user_data);
            }
            self.instance.destroy_instance(None);
        }
    }
}

fn has_layer(entry: &Entry, name: &CStr) -> Result<bool> {
    let layers = unsafe { entry.enumerate_instance_layer_properties()? };
    Ok(layers.iter().any(|l| {
        let n = unsafe { CStr::from_ptr(l.layer_name.as_ptr()) };
        n == name
    }))
}

fn pick_device(
    instance: &Instance,
    surface_fn: &khr::surface::Instance,
    surface: vk::SurfaceKHR,
) -> Result<(vk::PhysicalDevice, u32)> {
    let devices = unsafe { instance.enumerate_physical_devices()? };
    let mut best: Option<(i32, vk::PhysicalDevice, u32)> = None;
    for phys in devices {
        let props = unsafe { instance.get_physical_device_properties(phys) };
        if vk::api_version_major(props.api_version) < 1
            || (vk::api_version_major(props.api_version) == 1
                && vk::api_version_minor(props.api_version) < 3)
        {
            continue;
        }
        let exts = unsafe { instance.enumerate_device_extension_properties(phys)? };
        let has_swapchain = exts.iter().any(|e| {
            let n = unsafe { CStr::from_ptr(e.extension_name.as_ptr()) };
            n == khr::swapchain::NAME
        });
        if !has_swapchain {
            continue;
        }
        let Some(family) = find_graphics_present(instance, surface_fn, phys, surface)? else {
            continue;
        };
        let score = match props.device_type {
            vk::PhysicalDeviceType::DISCRETE_GPU => 1000,
            vk::PhysicalDeviceType::INTEGRATED_GPU => 100,
            _ => 10,
        };
        if best.map(|(s, _, _)| s).unwrap_or(-1) < score {
            best = Some((score, phys, family));
        }
    }
    best.map(|(_, p, f)| (p, f)).ok_or(RhiError::NoDevice)
}

fn find_graphics_present(
    instance: &Instance,
    surface_fn: &khr::surface::Instance,
    phys: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
) -> Result<Option<u32>> {
    let families = unsafe { instance.get_physical_device_queue_family_properties(phys) };
    for (i, fam) in families.iter().enumerate() {
        let i = i as u32;
        if !fam.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
            continue;
        }
        let present = unsafe { surface_fn.get_physical_device_surface_support(phys, i, surface)? };
        if present {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

unsafe fn image_barrier(
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
    let barrier = vk::ImageMemoryBarrier::default()
        .src_access_mask(src_access)
        .dst_access_mask(dst_access)
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
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

