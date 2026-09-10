//! Vulkan 1.3 backend. All `vk::*` / `vkCmd*` live under this module.

mod bindless;
mod debug;
mod layers;
mod resources;
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

use crate::device::{ComputePipelineDesc, DeviceDesc, GraphicsPipelineDesc};
use crate::types::{
    Buffer, ComputePipeline, Extent2D, Format, FrameConstants, FrameInfo, GraphicsPipeline,
    GpuStats, Texture, TextureData, TextureDesc, TextureDim, MAX_TIMESTAMPS,
    PUSH_CONSTANTS_SIZE,
};
use crate::{RhiError, Result, FRAMES_IN_FLIGHT};

const FRAME_TIMEOUT_NS: u64 = 2_000_000_000;
const VALIDATION_LAYER: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(b"VK_LAYER_KHRONOS_validation\0") };

struct FrameSlot {
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    image_available: vk::Semaphore,
    render_finished: vk::Semaphore,
    /// Timestamps for this slot. Read back when the slot comes round again, by
    /// which point its fence has been waited on and the GPU is definitely done.
    queries: vk::QueryPool,
    /// Label per mark, in write order. Index 0 is the frame start.
    labels: Vec<&'static str>,
    written: u32,
    /// False until this slot has recorded a frame worth reading.
    has_results: bool,
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
    upload_cmd: vk::CommandBuffer,
    empty_layout: vk::PipelineLayout,
    pipelines: Vec<vk::Pipeline>,
    compute_pipelines: Vec<vk::Pipeline>,
    bindless: Option<bindless::Bindless>,
    images: Vec<resources::GpuImage>,
    buffers: Vec<resources::GpuBuffer>,
    frames: Vec<FrameSlot>,
    frame_index: u64,
    slot: usize,
    /// Nanoseconds per timestamp tick, from the device limits.
    timestamp_period: f32,
    vsync: bool,
    /// Filled in at `begin_frame` from the slot that just completed.
    last_stats: GpuStats,
    stat_draws: u32,
    stat_dispatches: u32,
    stat_triangles: u64,
    image_index: u32,
    in_frame: bool,
    in_pass: bool,
    offscreen: bool,
    /// Next free chunk of this frame's CBV ring.
    cbv_next: u32,
    /// Byte offset of the chunk the *next* draw must read.
    cbv_offset: u32,
    /// What set 0 currently points at, per bind point. `None` = not bound.
    cbv_bound_gfx: Option<u32>,
    cbv_bound_cs: Option<u32>,
    color_pass_rt: Vec<Texture>,
    color_pass_depth: Option<Texture>,
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
        let storage_write_without_format = avail.features.shader_storage_image_write_without_format;
        let _ = avail;
        if avail13.dynamic_rendering == vk::FALSE {
            return Err(RhiError::msg("GPU lacks dynamic rendering (Vulkan 1.3)"));
        }
        require_true(avail12.descriptor_indexing, "descriptorIndexing")?;
        require_true(avail12.descriptor_binding_partially_bound, "descriptorBindingPartiallyBound")?;
        require_true(
            avail12.descriptor_binding_sampled_image_update_after_bind,
            "descriptorBindingSampledImageUpdateAfterBind",
        )?;
        require_true(
            avail12.descriptor_binding_variable_descriptor_count,
            "descriptorBindingVariableDescriptorCount",
        )?;
        require_true(avail12.runtime_descriptor_array, "runtimeDescriptorArray")?;
        require_true(
            avail12.shader_sampled_image_array_non_uniform_indexing,
            "shaderSampledImageArrayNonUniformIndexing",
        )?;

        let mut props12 = vk::PhysicalDeviceVulkan12Properties::default();
        let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut props12);
        unsafe { instance.get_physical_device_properties2(phys, &mut props2) };
        let _ = props2;
        let max_uab_sampled = props12.max_descriptor_set_update_after_bind_sampled_images;
        if max_uab_sampled < harpia_core::BINDLESS_HEAP_SIZE {
            return Err(RhiError::msg(format!(
                "GPU maxDescriptorSetUpdateAfterBindSampledImages is {max_uab_sampled} (need {})",
                harpia_core::BINDLESS_HEAP_SIZE
            )));
        }

        let mut vk13 = vk::PhysicalDeviceVulkan13Features::default().dynamic_rendering(true);
        let mut vk12 = vk::PhysicalDeviceVulkan12Features::default()
            .descriptor_indexing(true)
            .descriptor_binding_partially_bound(true)
            .descriptor_binding_sampled_image_update_after_bind(true)
            .descriptor_binding_variable_descriptor_count(true)
            .runtime_descriptor_array(true)
            .shader_sampled_image_array_non_uniform_indexing(true);
        let mut features2 = vk::PhysicalDeviceFeatures2::default()
            .features(
                vk::PhysicalDeviceFeatures::default()
                    .shader_sampled_image_array_dynamic_indexing(true)
                    .shader_storage_image_write_without_format(
                        storage_write_without_format == vk::TRUE,
                    ),
            )
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

        let mut allocator = Some(Allocator::new(&AllocatorCreateDesc {
            instance: instance.clone(),
            device: device.clone(),
            physical_device: phys,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })?);

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
                desc.vsync,
            )?
        };

        let pool_ci = vk::CommandPoolCreateInfo::default()
            .queue_family_index(graphics_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let cmd_pool = unsafe { device.create_command_pool(&pool_ci, None)? };

        let alloc_ci = vk::CommandBufferAllocateInfo::default()
            .command_pool(cmd_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(FRAMES_IN_FLIGHT + 1);
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
            let queries = unsafe {
                device.create_query_pool(
                    &vk::QueryPoolCreateInfo::default()
                        .query_type(vk::QueryType::TIMESTAMP)
                        .query_count(MAX_TIMESTAMPS),
                    None,
                )?
            };
            frames.push(FrameSlot {
                cmd: cmds[i],
                fence,
                queries,
                labels: Vec::new(),
                written: 0,
                has_results: false,
                image_available,
                render_finished,
            });
        }
        let upload_cmd = cmds[FRAMES_IN_FLIGHT as usize];

        let empty_layout = unsafe {
            device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)?
        };

        let alloc = allocator
            .as_mut()
            .ok_or_else(|| RhiError::msg("allocator missing"))?;
        let limits = unsafe { instance.get_physical_device_properties(phys) }.limits;
        let min_ubo_align = limits.min_uniform_buffer_offset_alignment;
        // Nanoseconds per timestamp tick. 0 means the queue cannot timestamp, in
        // which case the stats stay at zero rather than reporting nonsense.
        let timestamp_period = limits.timestamp_period;
        let heap = bindless::Bindless::create(&device, alloc, min_ubo_align)?;
        let mut dummy = bindless::create_dummy(&device, alloc)?;
        let packed = resources::pack_mip(1, 1, 1, 4, &bindless::dummy_pixel())?;
        resources::write_staging(&heap.staging, &packed)?;
        unsafe {
            device.reset_command_buffer(upload_cmd, vk::CommandBufferResetFlags::empty())?;
            device.begin_command_buffer(
                upload_cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            resources::image_barrier(
                &device,
                upload_cmd,
                dummy.image,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::TRANSFER_WRITE,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
            );
            resources::cmd_copy_mip(
                &device,
                upload_cmd,
                heap.staging.buffer,
                dummy.image,
                0,
                1,
                1,
                1,
                4,
            );
            resources::image_barrier(
                &device,
                upload_cmd,
                dummy.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::SHADER_READ,
                vk::PipelineStageFlags::TRANSFER,
                resources::shader_read_stages(),
            );
            device.end_command_buffer(upload_cmd)?;
            device.queue_submit(
                graphics_queue,
                &[vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&upload_cmd))],
                vk::Fence::null(),
            )?;
            device.queue_wait_idle(graphics_queue)?;
        }
        heap.write_sampled(
            &device,
            harpia_core::BINDLESS_NULL_SLOT,
            dummy.sampled_view,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        );
        dummy.ready = true;
        dummy.mips_uploaded = 1;
        dummy.transfer_prepared = true;
        dummy.layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;

        let alloc = allocator
            .as_mut()
            .ok_or_else(|| RhiError::msg("allocator missing"))?;
        let mut volume_dummy = bindless::create_volume_dummy(&device, alloc)?;
        unsafe {
            device.reset_command_buffer(upload_cmd, vk::CommandBufferResetFlags::empty())?;
            device.begin_command_buffer(
                upload_cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            resources::image_barrier(
                &device,
                upload_cmd,
                volume_dummy.image,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::GENERAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                resources::shader_read_stages(),
            );
            device.end_command_buffer(upload_cmd)?;
            device.queue_submit(
                graphics_queue,
                &[vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&upload_cmd))],
                vk::Fence::null(),
            )?;
            device.queue_wait_idle(graphics_queue)?;
        }
        let volume_uav_view = volume_dummy
            .storage_view
            .ok_or_else(|| RhiError::msg("volume dummy has no storage view"))?;
        for slot in 0..crate::types::VOLUME_UAV_SLOTS {
            heap.write_volume_uav(&device, slot, volume_uav_view);
        }
        for slot in 0..crate::types::VOLUME_SRV_SLOTS {
            heap.write_volume_srv(
                &device,
                slot,
                volume_dummy.sampled_view,
                vk::ImageLayout::GENERAL,
            );
        }
        volume_dummy.ready = true;
        volume_dummy.layout = vk::ImageLayout::GENERAL;

        let mut images = Vec::new();
        images.push(dummy);
        images.push(volume_dummy);

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
            upload_cmd,
            empty_layout,
            pipelines: Vec::new(),
            compute_pipelines: Vec::new(),
            bindless: Some(heap),
            images,
            buffers: Vec::new(),
            frames,
            frame_index: 0,
            slot: 0,
            timestamp_period,
            vsync: desc.vsync,
            last_stats: GpuStats::default(),
            stat_draws: 0,
            stat_dispatches: 0,
            stat_triangles: 0,
            image_index: 0,
            in_frame: false,
            in_pass: false,
            offscreen: false,
            cbv_next: 0,
            cbv_offset: 0,
            cbv_bound_gfx: None,
            cbv_bound_cs: None,
            color_pass_rt: Vec::new(),
            color_pass_depth: None,
            pending_recreate: false,
            zero_extent: false,
            window_extent,
            validation_errors,
        })
    }

    /// Timestamp here, labelled. Shows up in [`VulkanGpu::take_stats`] as the
    /// span since the previous mark.
    pub fn mark(&mut self, label: &'static str) {
        if !self.in_frame || self.timestamp_period <= 0.0 {
            return;
        }
        let slot = self.slot;
        if self.frames[slot].written >= MAX_TIMESTAMPS {
            return; // silently cap rather than corrupt the pool
        }
        let cmd = self.frames[slot].cmd;
        let index = self.frames[slot].written;
        unsafe {
            self.device.cmd_write_timestamp(
                cmd,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                self.frames[slot].queries,
                index,
            );
        }
        self.frames[slot].labels.push(label);
        self.frames[slot].written += 1;
    }

    pub fn take_stats(&mut self) -> GpuStats {
        std::mem::take(&mut self.last_stats)
    }

    /// Pull the timestamps written the last time this slot was used.
    ///
    /// Only safe once the slot's fence has been waited on, which `begin_frame`
    /// has just done -- otherwise the GPU may still be writing them.
    fn collect_stats(&mut self, slot: usize) {
        if !self.frames[slot].has_results || self.timestamp_period <= 0.0 {
            return;
        }
        let count = self.frames[slot].written as usize;
        if count < 2 {
            return;
        }
        let mut ticks = vec![0u64; count];
        let ok = unsafe {
            self.device.get_query_pool_results(
                self.frames[slot].queries,
                0,
                &mut ticks,
                vk::QueryResultFlags::TYPE_64,
            )
        };
        if ok.is_err() {
            return; // NOT_READY: skip this frame's numbers rather than block
        }
        let to_ms = |a: u64, b: u64| {
            (b.saturating_sub(a) as f64 * self.timestamp_period as f64 / 1.0e6) as f32
        };
        let mut passes = Vec::with_capacity(count.saturating_sub(2));
        for i in 1..count - 1 {
            passes.push((self.frames[slot].labels[i], to_ms(ticks[i - 1], ticks[i])));
        }
        self.last_stats = GpuStats {
            frame_ms: to_ms(ticks[0], ticks[count - 1]),
            passes,
            draws: self.stat_draws,
            dispatches: self.stat_dispatches,
            triangles: self.stat_triangles,
        };
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
        // The fence is signalled, so last time round this slot is now readable.
        self.collect_stats(slot);

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
        self.offscreen = false;
        self.cbv_next = 0;
        self.cbv_offset = 0;
        self.cbv_bound_gfx = None;
        self.cbv_bound_cs = None;

        let cmd = self.frames[slot].cmd;
        unsafe {
            self.device
                .reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
            let begin = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device.begin_command_buffer(cmd, &begin)?;
            // Reset outside a render pass, which is the only place it is legal.
            self.device
                .cmd_reset_query_pool(cmd, self.frames[slot].queries, 0, MAX_TIMESTAMPS);
        }
        self.frames[slot].labels.clear();
        self.frames[slot].written = 0;
        self.stat_draws = 0;
        self.stat_dispatches = 0;
        self.stat_triangles = 0;
        self.mark("frame");

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
            resources::image_barrier(
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
        self.offscreen = false;
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
        self.stat_draws += 1;
        self.stat_triangles += (vertex_count as u64 / 3) * instance_count.max(1) as u64;
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        if self.cbv_bound_gfx.is_some() {
            self.sync_cbv_graphics();
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
        if !self.in_pass || self.offscreen {
            return Err(RhiError::PassMismatch);
        }
        let cmd = self.frames[self.slot].cmd;
        let image = self.swapchain.images[self.image_index as usize];
        unsafe {
            self.device.cmd_end_rendering(cmd);
            resources::image_barrier(
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

        self.mark("present");
        let slot = self.slot;
        let cmd = self.frames[slot].cmd;
        unsafe {
            self.device.end_command_buffer(cmd)?;
        }
        self.frames[slot].has_results = self.frames[slot].written >= 2;

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
        if desc.vs_spirv.is_empty() {
            return Err(RhiError::msg("vertex SPIR-V is empty"));
        }
        if desc.targets.depth_only && !desc.targets.color_formats.is_empty() {
            return Err(RhiError::msg("depth_only PSO must have empty color_formats"));
        }
        if !desc.targets.depth_only && desc.fs_spirv.is_empty() {
            return Err(RhiError::msg("fragment SPIR-V is empty"));
        }
        let vs_words = ash::util::read_spv(&mut Cursor::new(desc.vs_spirv))?;
        let vs_mod = unsafe {
            self.device.create_shader_module(
                &vk::ShaderModuleCreateInfo::default().code(&vs_words),
                None,
            )?
        };
        let fs_mod = if desc.fs_spirv.is_empty() {
            None
        } else {
            let fs_words = ash::util::read_spv(&mut Cursor::new(desc.fs_spirv))?;
            Some(unsafe {
                self.device.create_shader_module(
                    &vk::ShaderModuleCreateInfo::default().code(&fs_words),
                    None,
                )?
            })
        };

        let vs_entry = CString::new(desc.vs_entry).map_err(|_| RhiError::BadCString)?;
        let fs_entry = CString::new(desc.fs_entry).map_err(|_| RhiError::BadCString)?;
        let mut stages = vec![vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vs_mod)
            .name(vs_entry.as_c_str())];
        if let Some(fs) = fs_mod {
            stages.push(
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(fs)
                    .name(fs_entry.as_c_str()),
            );
        }

        let vertex_input;
        let mut binding_descs = Vec::new();
        let mut attr_descs = Vec::new();
        if desc.targets.vertex_stride > 0 {
            binding_descs.push(vk::VertexInputBindingDescription {
                binding: 0,
                stride: desc.targets.vertex_stride,
                input_rate: vk::VertexInputRate::VERTEX,
            });
            attr_descs.push(vk::VertexInputAttributeDescription {
                location: 0,
                binding: 0,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: 0,
            });
            attr_descs.push(vk::VertexInputAttributeDescription {
                location: 1,
                binding: 0,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: 12,
            });
            if desc.targets.instance_stride == 0 && desc.targets.vertex_stride >= 32 {
                attr_descs.push(vk::VertexInputAttributeDescription {
                    location: 2,
                    binding: 0,
                    format: vk::Format::R32G32_SFLOAT,
                    offset: 24,
                });
            }
        }
        if desc.targets.instance_stride > 0 {
            binding_descs.push(vk::VertexInputBindingDescription {
                binding: 1,
                stride: desc.targets.instance_stride,
                input_rate: vk::VertexInputRate::INSTANCE,
            });
            let n = desc.targets.instance_stride / 16;
            for i in 0..n {
                attr_descs.push(vk::VertexInputAttributeDescription {
                    location: 2 + i,
                    binding: 1,
                    format: vk::Format::R32G32B32A32_SFLOAT,
                    offset: i * 16,
                });
            }
        }
        vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&binding_descs)
            .vertex_attribute_descriptions(&attr_descs);
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let cull = if desc.targets.cull_back {
            vk::CullModeFlags::BACK
        } else {
            vk::CullModeFlags::NONE
        };
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(cull)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .depth_bias_enable(desc.targets.depth_bias)
            .depth_bias_constant_factor(if desc.targets.depth_bias { 1.25 } else { 0.0 })
            .depth_bias_clamp(0.0)
            .depth_bias_slope_factor(if desc.targets.depth_bias { 1.75 } else { 0.0 })
            .line_width(1.0);
        let msaa = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(desc.targets.depth_test)
            .depth_write_enable(desc.targets.depth_test)
            .depth_compare_op(vk::CompareOp::LESS)
            .min_depth_bounds(0.0)
            .max_depth_bounds(1.0);
        let blend_attachment = vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA);
        let color_vk: Vec<vk::Format> = if desc.targets.depth_only {
            Vec::new()
        } else if desc.targets.color_formats.is_empty() {
            vec![self.swapchain.format.format]
        } else {
            desc.targets
                .color_formats
                .iter()
                .copied()
                .map(resources::vk_format)
                .collect::<Result<Vec<_>>>()?
        };
        let blend_attachments = vec![blend_attachment; color_vk.len()];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

        let depth_vk = match desc.targets.depth_format {
            Some(f) => resources::vk_format(f)?,
            None => vk::Format::UNDEFINED,
        };
        let mut rendering = vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(&color_vk)
            .depth_attachment_format(depth_vk);

        let ci = vk::GraphicsPipelineCreateInfo::default()
            .push_next(&mut rendering)
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&raster)
            .multisample_state(&msaa)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(if desc.bindless {
                self.bindless
                    .as_ref()
                    .ok_or_else(|| RhiError::msg("bindless layout missing"))?
                    .pipeline_layout
            } else {
                self.empty_layout
            });

        let created = unsafe {
            self.device
                .create_graphics_pipelines(vk::PipelineCache::null(), &[ci], None)
        };
        unsafe {
            self.device.destroy_shader_module(vs_mod, None);
            if let Some(fs) = fs_mod {
                self.device.destroy_shader_module(fs, None);
            }
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
                self.vsync,
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

    pub fn create_texture(&mut self, desc: &TextureDesc) -> Result<Texture> {
        let slot = self
            .bindless
            .as_mut()
            .ok_or_else(|| RhiError::msg("bindless missing"))?
            .alloc_slot()?;
        let mut img = {
            let alloc = self
                .allocator
                .as_mut()
                .ok_or_else(|| RhiError::msg("allocator missing"))?;
            resources::create_image(&self.device, alloc, desc, slot)?
        };
        if desc.storage || desc.dim == TextureDim::D3 {
            let image = img.image;
            let cmd = self.upload_cmd;
            unsafe {
                self.device
                    .reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
                self.device.begin_command_buffer(
                    cmd,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )?;
                resources::image_barrier(
                    &self.device,
                    cmd,
                    image,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::GENERAL,
                    vk::AccessFlags::empty(),
                    vk::AccessFlags::SHADER_WRITE | vk::AccessFlags::SHADER_READ,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    resources::shader_read_stages() | vk::PipelineStageFlags::COMPUTE_SHADER,
                );
                self.device.end_command_buffer(cmd)?;
                self.device.queue_submit(
                    self.graphics_queue,
                    &[vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cmd))],
                    vk::Fence::null(),
                )?;
                self.device.queue_wait_idle(self.graphics_queue)?;
            }
            // A volume is bound with bind_volume_uav / bind_volume_srv: its view
            // type is 3D and would be invalid in the 2D heap (set 1 / set 4:0).
            if desc.dim == TextureDim::D2 {
                let view = img.sampled_view;
                let storage = img
                    .storage_view
                    .ok_or_else(|| RhiError::msg("storage view missing"))?;
                let heap = self
                    .bindless
                    .as_ref()
                    .ok_or_else(|| RhiError::msg("bindless missing"))?;
                heap.write_sampled(&self.device, slot, view, vk::ImageLayout::GENERAL);
                heap.write_storage(&self.device, storage);
            }
            img.ready = true;
            img.layout = vk::ImageLayout::GENERAL;
        }
        let id = self.images.len() as u32;
        self.images.push(img);
        Ok(Texture { id })
    }

    pub fn upload_texture_mip(&mut self, tex: Texture, mip: u32, rgba: &[u8]) -> Result<()> {
        let (w, h, d, image, bpp) = {
            let img = self
                .images
                .get(tex.id as usize)
                .ok_or_else(|| RhiError::msg("invalid texture"))?;
            if mip >= img.mip_levels {
                return Err(RhiError::msg("mip out of range"));
            }
            let (w, h) = resources::mip_extent(img.width, img.height, mip);
            // A volume uploads every slice at once: one mip is the whole texture.
            let d = if img.dim == TextureDim::D3 {
                (img.depth_slices >> mip).max(1)
            } else {
                1
            };
            let bpp = resources::bytes_per_pixel(img.engine_format)?;
            (w, h, d, img.image, bpp)
        };
        let packed = resources::pack_mip(w, h, d, bpp, rgba)?;
        let heap = self
            .bindless
            .as_ref()
            .ok_or_else(|| RhiError::msg("bindless missing"))?;
        resources::write_staging(&heap.staging, &packed)?;
        let staging = heap.staging.buffer;
        let prepared = self
            .images
            .get(tex.id as usize)
            .ok_or_else(|| RhiError::msg("invalid texture"))?
            .transfer_prepared;
        let record = |device: &ash::Device, cmd: vk::CommandBuffer| unsafe {
            if !prepared {
                resources::image_barrier(
                    device,
                    cmd,
                    image,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::AccessFlags::empty(),
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                );
            }
            resources::cmd_copy_mip(device, cmd, staging, image, mip, w, h, d, bpp);
        };
        if self.in_frame {
            record(&self.device, self.frames[self.slot].cmd);
        } else {
            self.submit_now(|device, cmd| {
                record(device, cmd);
                Ok(())
            })?;
        }
        let img = self
            .images
            .get_mut(tex.id as usize)
            .ok_or_else(|| RhiError::msg("invalid texture"))?;
        img.mips_uploaded += 1;
        img.transfer_prepared = true;
        if img.mips_uploaded >= img.mip_levels && img.sampled && !img.storage {
            self.finalize_sampled(tex)?;
        }
        Ok(())
    }

    fn finalize_sampled(&mut self, tex: Texture) -> Result<()> {
        let img = self
            .images
            .get(tex.id as usize)
            .ok_or_else(|| RhiError::msg("invalid texture"))?;
        let image = img.image;
        let view = img.sampled_view;
        let slot = img.bindless_slot;
        let dim = img.dim;
        let barrier = |device: &ash::Device, cmd: vk::CommandBuffer| unsafe {
            resources::image_barrier(
                device,
                cmd,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::SHADER_READ,
                vk::PipelineStageFlags::TRANSFER,
                resources::shader_read_stages(),
            );
        };
        if self.in_frame {
            barrier(&self.device, self.frames[self.slot].cmd);
        } else {
            self.submit_now(|device, cmd| {
                barrier(device, cmd);
                Ok(())
            })?;
        }
        if dim == TextureDim::D2 {
            let heap = self
                .bindless
                .as_ref()
                .ok_or_else(|| RhiError::msg("bindless missing"))?;
            heap.write_sampled(
                &self.device,
                slot,
                view,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
        }
        if let Some(img) = self.images.get_mut(tex.id as usize) {
            img.ready = true;
            img.layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        }
        Ok(())
    }

    /// Copy mip 0 of a texture back to the CPU. Capture / debug path: it waits
    /// for the device, so never call it inside a hot frame.
    pub fn read_texture(&mut self, tex: Texture) -> Result<TextureData> {
        if self.in_frame {
            return Err(RhiError::msg("read_texture during a frame"));
        }
        let (image, width, height, slices, format, layout) = {
            let img = self
                .images
                .get(tex.id as usize)
                .ok_or_else(|| RhiError::msg("invalid texture"))?;
            (
                img.image,
                img.width,
                img.height,
                img.depth_slices,
                img.engine_format,
                img.layout,
            )
        };
        let bpp = resources::bytes_per_pixel(format)?;
        let aspect = resources::aspect_for(format);
        let size =
            resources::row_pitch_bytes(width, bpp) as u64 * height as u64 * slices as u64;

        unsafe {
            self.device.device_wait_idle()?;
        }
        let alloc = self
            .allocator
            .as_mut()
            .ok_or_else(|| RhiError::msg("allocator missing"))?;
        let readback = resources::create_buffer(
            &self.device,
            alloc,
            size,
            vk::BufferUsageFlags::TRANSFER_DST,
            gpu_allocator::MemoryLocation::GpuToCpu,
            "readback",
        )?;
        let buffer = readback.buffer;

        // UNDEFINED means nothing was ever written; a barrier from it would
        // discard the contents, so treat it as an error instead of a black PNG.
        if layout == vk::ImageLayout::UNDEFINED {
            let alloc = self
                .allocator
                .as_mut()
                .ok_or_else(|| RhiError::msg("allocator missing"))?;
            resources::destroy_buffer(&self.device, alloc, readback);
            return Err(RhiError::msg("texture has never been written"));
        }

        let result = self.submit_now(|device, cmd| {
            unsafe {
                resources::image_barrier_aspect(
                    device,
                    cmd,
                    image,
                    layout,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    vk::AccessFlags::empty(),
                    vk::AccessFlags::TRANSFER_READ,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    vk::PipelineStageFlags::TRANSFER,
                    aspect,
                );
                resources::cmd_copy_to_buffer(
                    device, cmd, image, buffer, width, height, slices, bpp, aspect,
                );
                resources::image_barrier_aspect(
                    device,
                    cmd,
                    image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    layout,
                    vk::AccessFlags::TRANSFER_READ,
                    vk::AccessFlags::empty(),
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    aspect,
                );
            }
            Ok(())
        });

        let bytes = result.and_then(|()| {
            let ptr = readback
                .allocation
                .mapped_ptr()
                .ok_or_else(|| RhiError::msg("readback is not mapped"))?
                .as_ptr()
                .cast::<u8>();
            let padded = unsafe { std::slice::from_raw_parts(ptr, size as usize) };
            Ok(resources::unpack_rows(padded, width, height, slices, bpp))
        });

        let alloc = self
            .allocator
            .as_mut()
            .ok_or_else(|| RhiError::msg("allocator missing"))?;
        resources::destroy_buffer(&self.device, alloc, readback);

        Ok(TextureData {
            width,
            height,
            depth_slices: slices,
            format,
            bytes: bytes?,
        })
    }

    pub fn bindless_index(&self, tex: Texture) -> Result<u32> {
        let img = self
            .images
            .get(tex.id as usize)
            .ok_or_else(|| RhiError::msg("invalid texture"))?;
        if img.dim == TextureDim::D3 {
            return Err(RhiError::msg(
                "a 3D texture has no 2D heap slot; bind it with bind_volume_uav / bind_volume_srv",
            ));
        }
        Ok(img.bindless_slot)
    }

    /// Set 4 binding 1, slot `slot`: the volume a compute shader writes.
    pub fn bind_volume_uav(&mut self, slot: u32, tex: Texture) -> Result<()> {
        let (view, dim) = {
            let img = self
                .images
                .get(tex.id as usize)
                .ok_or_else(|| RhiError::msg("invalid texture"))?;
            (img.storage_view, img.dim)
        };
        if dim != TextureDim::D3 {
            return Err(RhiError::msg("bind_volume_uav needs a 3D texture"));
        }
        if slot >= crate::types::VOLUME_UAV_SLOTS {
            return Err(RhiError::msg("volume UAV slot out of range"));
        }
        let view = view.ok_or_else(|| RhiError::msg("volume has no storage view"))?;
        self.bindless
            .as_ref()
            .ok_or_else(|| RhiError::msg("bindless missing"))?
            .write_volume_uav(&self.device, slot, view);
        Ok(())
    }

    /// Set 5 binding 0, slot `slot`: the volume a shader samples. Volumes stay
    /// in GENERAL, so the same image can be read and written across dispatches.
    pub fn bind_volume_srv(&mut self, slot: u32, tex: Texture) -> Result<()> {
        let (view, dim, layout) = {
            let img = self
                .images
                .get(tex.id as usize)
                .ok_or_else(|| RhiError::msg("invalid texture"))?;
            (img.sampled_view, img.dim, img.layout)
        };
        if dim != TextureDim::D3 {
            return Err(RhiError::msg("bind_volume_srv needs a 3D texture"));
        }
        if slot >= crate::types::VOLUME_SRV_SLOTS {
            return Err(RhiError::msg("volume SRV slot out of range"));
        }
        self.bindless
            .as_ref()
            .ok_or_else(|| RhiError::msg("bindless missing"))?
            .write_volume_srv(&self.device, slot, view, layout);
        Ok(())
    }

    pub fn write_frame_constants(&mut self, c: FrameConstants) -> Result<()> {
        if !self.in_frame {
            return Err(RhiError::NotInFrame);
        }
        let offset = self.alloc_cbv_chunk()?;
        self.bindless
            .as_ref()
            .ok_or_else(|| RhiError::msg("bindless missing"))?
            .write_constants(self.slot, offset, c)
    }

    /// Take a fresh chunk of the frame ring. Every draw / dispatch recorded
    /// after this reads *this* chunk — that is what makes per-draw constants work.
    pub fn write_frame_bytes(&mut self, data: &[u8]) -> Result<()> {
        if !self.in_frame {
            return Err(RhiError::NotInFrame);
        }
        let offset = self.alloc_cbv_chunk()?;
        self.bindless
            .as_ref()
            .ok_or_else(|| RhiError::msg("bindless missing"))?
            .write_bytes(self.slot, offset, data)
    }

    fn alloc_cbv_chunk(&mut self) -> Result<u32> {
        let offset = bindless::Bindless::chunk_offset(self.cbv_next)?;
        self.cbv_next += 1;
        self.cbv_offset = offset;
        Ok(offset)
    }

    /// Set 0 still points at the chunk bound earlier; re-point it at the last one written.
    fn sync_cbv_graphics(&mut self) {
        if self.cbv_bound_gfx == Some(self.cbv_offset) {
            return;
        }
        if let Some(heap) = self.bindless.as_ref() {
            heap.bind_graphics_cbv(
                &self.device,
                self.frames[self.slot].cmd,
                self.slot,
                self.cbv_offset,
            );
            self.cbv_bound_gfx = Some(self.cbv_offset);
        }
    }

    fn sync_cbv_compute(&mut self) {
        if self.cbv_bound_cs == Some(self.cbv_offset) {
            return;
        }
        if let Some(heap) = self.bindless.as_ref() {
            heap.bind_compute_cbv(
                &self.device,
                self.frames[self.slot].cmd,
                self.slot,
                self.cbv_offset,
            );
            self.cbv_bound_cs = Some(self.cbv_offset);
        }
    }

    pub fn bind_graphics_bindless(&mut self) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        let cbv = self.cbv_offset;
        let heap = self
            .bindless
            .as_ref()
            .ok_or_else(|| RhiError::msg("bindless missing"))?;
        heap.bind_graphics(&self.device, self.frames[self.slot].cmd, self.slot, cbv);
        self.cbv_bound_gfx = Some(cbv);
        Ok(())
    }

    pub fn create_compute_pipeline(&mut self, desc: &ComputePipelineDesc<'_>) -> Result<ComputePipeline> {
        if desc.cs_spirv.is_empty() {
            return Err(RhiError::msg("SPIR-V module is empty"));
        }
        let words = ash::util::read_spv(&mut Cursor::new(desc.cs_spirv))?;
        let module = unsafe {
            self.device.create_shader_module(
                &vk::ShaderModuleCreateInfo::default().code(&words),
                None,
            )?
        };
        let entry = CString::new(desc.cs_entry).map_err(|_| RhiError::BadCString)?;
        let layout = self
            .bindless
            .as_ref()
            .ok_or_else(|| RhiError::msg("bindless layout missing"))?
            .pipeline_layout;
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(module)
            .name(entry.as_c_str());
        let ci = vk::ComputePipelineCreateInfo::default()
            .stage(stage)
            .layout(layout);
        let created = unsafe {
            self.device
                .create_compute_pipelines(vk::PipelineCache::null(), &[ci], None)
        };
        unsafe {
            self.device.destroy_shader_module(module, None);
        }
        let pipelines = created.map_err(|(_p, e)| RhiError::from_vk(e))?;
        let id = self.compute_pipelines.len() as u32;
        self.compute_pipelines.push(pipelines[0]);
        Ok(ComputePipeline { id })
    }

    pub fn set_compute_pipeline(&mut self, pipeline: &ComputePipeline) -> Result<()> {
        if !self.in_frame || self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        let pso = self
            .compute_pipelines
            .get(pipeline.id as usize)
            .copied()
            .filter(|p| *p != vk::Pipeline::null())
            .ok_or_else(|| RhiError::msg("invalid compute pipeline"))?;
        unsafe {
            self.device.cmd_bind_pipeline(
                self.frames[self.slot].cmd,
                vk::PipelineBindPoint::COMPUTE,
                pso,
            );
        }
        Ok(())
    }

    pub fn bind_compute_bindless(&mut self) -> Result<()> {
        if !self.in_frame || self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        let cbv = self.cbv_offset;
        let heap = self
            .bindless
            .as_ref()
            .ok_or_else(|| RhiError::msg("bindless missing"))?;
        heap.bind_compute(&self.device, self.frames[self.slot].cmd, self.slot, cbv);
        self.cbv_bound_cs = Some(cbv);
        Ok(())
    }

    pub fn dispatch(&mut self, x: u32, y: u32, z: u32) -> Result<()> {
        self.stat_dispatches += 1;
        if !self.in_frame || self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        if self.cbv_bound_cs.is_some() {
            self.sync_cbv_compute();
        }
        unsafe {
            self.device
                .cmd_dispatch(self.frames[self.slot].cmd, x, y, z);
        }
        Ok(())
    }

    pub fn storage_barrier(&mut self, tex: Texture) -> Result<()> {
        if !self.in_frame {
            return Err(RhiError::NotInFrame);
        }
        let image = self
            .images
            .get(tex.id as usize)
            .ok_or_else(|| RhiError::msg("invalid texture"))?
            .image;
        unsafe {
            resources::image_barrier(
                &self.device,
                self.frames[self.slot].cmd,
                image,
                vk::ImageLayout::GENERAL,
                vk::ImageLayout::GENERAL,
                vk::AccessFlags::SHADER_WRITE,
                vk::AccessFlags::SHADER_READ,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                resources::shader_read_stages(),
            );
        }
        Ok(())
    }

    pub fn set_push_constants(&mut self, data: &[u8]) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        let layout = self
            .bindless
            .as_ref()
            .ok_or_else(|| RhiError::msg("bindless layout missing"))?
            .pipeline_layout;
        let mut tmp = [0u8; PUSH_CONSTANTS_SIZE as usize];
        let n = data.len().min(tmp.len());
        tmp[..n].copy_from_slice(&data[..n]);
        unsafe {
            self.device.cmd_push_constants(
                self.frames[self.slot].cmd,
                layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                &tmp,
            );
        }
        Ok(())
    }

    pub fn begin_color_pass(
        &mut self,
        colors: &[Texture],
        depth: Option<Texture>,
        clears: &[[f32; 4]],
        depth_clear: Option<f32>,
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
        let cmd = self.frames[self.slot].cmd;
        let mut attachments = Vec::with_capacity(colors.len());
        let mut extent = vk::Extent2D {
            width: 1,
            height: 1,
        };
        if colors.is_empty() {
            if let Some(dtex) = depth {
                let img = self
                    .images
                    .get(dtex.id as usize)
                    .ok_or_else(|| RhiError::msg("invalid depth texture"))?;
                extent = vk::Extent2D {
                    width: img.width,
                    height: img.height,
                };
            }
        }
        for (i, tex) in colors.iter().enumerate() {
            let img = self
                .images
                .get(tex.id as usize)
                .ok_or_else(|| RhiError::msg("invalid color texture"))?;
            if i == 0 {
                extent = vk::Extent2D {
                    width: img.width,
                    height: img.height,
                };
            }
            let old = img.layout;
            let image = img.image;
            let view = img.sampled_view;
            unsafe {
                resources::image_barrier(
                    &self.device,
                    cmd,
                    image,
                    old,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    if old == vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL {
                        vk::AccessFlags::SHADER_READ
                    } else {
                        vk::AccessFlags::empty()
                    },
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                    if old == vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL {
                        resources::shader_read_stages()
                    } else {
                        vk::PipelineStageFlags::TOP_OF_PIPE
                    },
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                );
            }
            if let Some(img) = self.images.get_mut(tex.id as usize) {
                img.layout = vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL;
            }
            let clear = clears.get(i).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]);
            attachments.push(
                vk::RenderingAttachmentInfo::default()
                    .image_view(view)
                    .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .clear_value(vk::ClearValue {
                        color: vk::ClearColorValue { float32: clear },
                    }),
            );
        }

        let mut depth_attachment = vk::RenderingAttachmentInfo::default();
        if let Some(dtex) = depth {
            let img = self
                .images
                .get(dtex.id as usize)
                .ok_or_else(|| RhiError::msg("invalid depth texture"))?;
            let old = img.layout;
            let image = img.image;
            let view = img.sampled_view;
            let aspect = resources::aspect_for(img.engine_format);
            unsafe {
                resources::image_barrier_aspect(
                    &self.device,
                    cmd,
                    image,
                    old,
                    vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                    if old == vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
                        || old == vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL
                    {
                        vk::AccessFlags::SHADER_READ
                    } else {
                        vk::AccessFlags::empty()
                    },
                    vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE
                        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ,
                    if old == vk::ImageLayout::UNDEFINED {
                        vk::PipelineStageFlags::TOP_OF_PIPE
                    } else {
                        resources::shader_read_stages()
                    },
                    vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                        | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                    aspect,
                );
            }
            if let Some(img) = self.images.get_mut(dtex.id as usize) {
                img.layout = vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL;
            }
            let z = depth_clear.unwrap_or(1.0);
            depth_attachment = vk::RenderingAttachmentInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: z,
                        stencil: 0,
                    },
                });
        }

        unsafe {
            let mut rendering = vk::RenderingInfo::default()
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent,
                })
                .layer_count(1)
                .color_attachments(&attachments);
            if depth.is_some() {
                rendering = rendering.depth_attachment(&depth_attachment);
            }
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

        self.color_pass_rt = colors.to_vec();
        self.color_pass_depth = depth;
        self.in_pass = true;
        self.offscreen = true;
        Ok(())
    }

    pub fn set_viewport(&mut self, x: f32, y: f32, width: f32, height: f32) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        let cmd = self.frames[self.slot].cmd;
        let viewport = vk::Viewport {
            x,
            y,
            width,
            height,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let scissor = vk::Rect2D {
            offset: vk::Offset2D {
                x: x.max(0.0) as i32,
                y: y.max(0.0) as i32,
            },
            extent: vk::Extent2D {
                width: width.max(1.0) as u32,
                height: height.max(1.0) as u32,
            },
        };
        unsafe {
            self.device.cmd_set_viewport(cmd, 0, &[viewport]);
            self.device.cmd_set_scissor(cmd, 0, &[scissor]);
        }
        Ok(())
    }

    pub fn end_color_pass(&mut self) -> Result<()> {
        if !self.in_pass || !self.offscreen {
            return Err(RhiError::PassMismatch);
        }
        let cmd = self.frames[self.slot].cmd;
        unsafe {
            self.device.cmd_end_rendering(cmd);
        }
        let colors = std::mem::take(&mut self.color_pass_rt);
        let depth = self.color_pass_depth.take();
        for tex in colors {
            let img = self
                .images
                .get(tex.id as usize)
                .ok_or_else(|| RhiError::msg("invalid color texture"))?;
            let image = img.image;
            let view = img.sampled_view;
            let slot = img.bindless_slot;
            let sampled = img.sampled;
            unsafe {
                resources::image_barrier(
                    &self.device,
                    cmd,
                    image,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                    vk::AccessFlags::SHADER_READ,
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    resources::shader_read_stages(),
                );
            }
            if let Some(img) = self.images.get_mut(tex.id as usize) {
                img.layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
                img.ready = true;
            }
            if sampled {
                let heap = self
                    .bindless
                    .as_ref()
                    .ok_or_else(|| RhiError::msg("bindless missing"))?;
                heap.write_sampled(
                    &self.device,
                    slot,
                    view,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                );
            }
        }
        if let Some(dtex) = depth {
            let img = self
                .images
                .get(dtex.id as usize)
                .ok_or_else(|| RhiError::msg("invalid depth texture"))?;
            let image = img.image;
            let view = img.sampled_view;
            let slot = img.bindless_slot;
            let sampled = img.sampled;
            let aspect = resources::aspect_for(img.engine_format);
            let new_layout = if sampled {
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
            } else {
                vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL
            };
            if sampled {
                unsafe {
                    resources::image_barrier_aspect(
                        &self.device,
                        cmd,
                        image,
                        vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                        new_layout,
                        vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                        vk::AccessFlags::SHADER_READ,
                        vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                            | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                        resources::shader_read_stages(),
                        aspect,
                    );
                }
                let heap = self
                    .bindless
                    .as_ref()
                    .ok_or_else(|| RhiError::msg("bindless missing"))?;
                heap.write_sampled(&self.device, slot, view, new_layout);
            }
            if let Some(img) = self.images.get_mut(dtex.id as usize) {
                img.layout = new_layout;
            }
        }
        self.in_pass = false;
        self.offscreen = false;
        Ok(())
    }

    pub fn create_vertex_buffer(&mut self, bytes: &[u8]) -> Result<Buffer> {
        self.create_host_buffer(bytes, vk::BufferUsageFlags::VERTEX_BUFFER, "vb")
    }

    pub fn create_index_buffer(&mut self, bytes: &[u8]) -> Result<Buffer> {
        self.create_host_buffer(bytes, vk::BufferUsageFlags::INDEX_BUFFER, "ib")
    }

    fn create_host_buffer(
        &mut self,
        bytes: &[u8],
        usage: vk::BufferUsageFlags,
        name: &str,
    ) -> Result<Buffer> {
        let alloc = self
            .allocator
            .as_mut()
            .ok_or_else(|| RhiError::msg("allocator missing"))?;
        let buf = resources::create_buffer(
            &self.device,
            alloc,
            bytes.len().max(1) as u64,
            usage,
            gpu_allocator::MemoryLocation::CpuToGpu,
            name,
        )?;
        resources::write_staging(&buf, bytes)?;
        let id = self.buffers.len() as u32;
        self.buffers.push(buf);
        Ok(Buffer { id })
    }

    pub fn bind_vertex_buffer(&mut self, buf: Buffer, binding: u32) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        let gpu = self
            .buffers
            .get(buf.id as usize)
            .ok_or_else(|| RhiError::msg("invalid vertex buffer"))?;
        unsafe {
            self.device.cmd_bind_vertex_buffers(
                self.frames[self.slot].cmd,
                binding,
                &[gpu.buffer],
                &[0],
            );
        }
        Ok(())
    }

    pub fn bind_index_buffer(&mut self, buf: Buffer) -> Result<()> {
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        let gpu = self
            .buffers
            .get(buf.id as usize)
            .ok_or_else(|| RhiError::msg("invalid index buffer"))?;
        unsafe {
            self.device.cmd_bind_index_buffer(
                self.frames[self.slot].cmd,
                gpu.buffer,
                0,
                vk::IndexType::UINT32,
            );
        }
        Ok(())
    }

    pub fn draw_indexed(
        &mut self,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    ) -> Result<()> {
        self.stat_draws += 1;
        self.stat_triangles += (index_count as u64 / 3) * instance_count.max(1) as u64;
        if !self.in_pass {
            return Err(RhiError::PassMismatch);
        }
        if self.cbv_bound_gfx.is_some() {
            self.sync_cbv_graphics();
        }
        unsafe {
            self.device.cmd_draw_indexed(
                self.frames[self.slot].cmd,
                index_count,
                instance_count,
                first_index,
                vertex_offset,
                first_instance,
            );
        }
        Ok(())
    }

    fn submit_now(
        &mut self,
        record: impl FnOnce(&ash::Device, vk::CommandBuffer) -> Result<()>,
    ) -> Result<()> {
        let cmd = self.upload_cmd;
        unsafe {
            self.device
                .reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
            self.device.begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
        }
        record(&self.device, cmd)?;
        unsafe {
            self.device.end_command_buffer(cmd)?;
            self.device.queue_submit(
                self.graphics_queue,
                &[vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cmd))],
                vk::Fence::null(),
            )?;
            self.device.queue_wait_idle(self.graphics_queue)?;
        }
        Ok(())
    }
}

impl Drop for VulkanGpu {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            for pso in self.pipelines.drain(..) {
                self.device.destroy_pipeline(pso, None);
            }
            for pso in self.compute_pipelines.drain(..) {
                self.device.destroy_pipeline(pso, None);
            }
            self.device.destroy_pipeline_layout(self.empty_layout, None);
            if let Some(alloc) = self.allocator.as_mut() {
                for buf in self.buffers.drain(..) {
                    resources::destroy_buffer(&self.device, alloc, buf);
                }
                for img in self.images.drain(..) {
                    resources::destroy_image(&self.device, alloc, img);
                }
                if let Some(heap) = self.bindless.take() {
                    heap.destroy(&self.device, alloc);
                }
            }
            for frame in &self.frames {
                self.device.destroy_query_pool(frame.queries, None);
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

fn require_true(flag: vk::Bool32, name: &str) -> Result<()> {
    if flag == vk::FALSE {
        Err(RhiError::msg(format!("GPU lacks {name}")))
    } else {
        Ok(())
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

