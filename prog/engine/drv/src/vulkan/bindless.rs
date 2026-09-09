//! Bindless heap 8192, slot 0 = dummy. Spec: `docs/Bindless-Descriptor-Layout.md`.

use ash::vk;
use ash::Device;
use gpu_allocator::vulkan::Allocator;
use gpu_allocator::MemoryLocation;
use harpia_core::{BINDLESS_HEAP_SIZE, BINDLESS_NULL_SLOT};

use super::resources::{self, GpuBuffer, GpuImage};
use crate::types::{Format, FrameConstants, TextureDesc};
use crate::{RhiError, Result, FRAMES_IN_FLIGHT};

const UBO_SIZE: u64 = 256;
const SET_COUNT: usize = 5;
const STAGING_SIZE: u64 = 1024 * 1024;

pub struct Bindless {
    pub set_layouts: [vk::DescriptorSetLayout; SET_COUNT],
    pub pipeline_layout: vk::PipelineLayout,
    pub pool: vk::DescriptorPool,
    pub set0: Vec<vk::DescriptorSet>,
    pub set1: vk::DescriptorSet,
    pub set2: vk::DescriptorSet,
    pub set3: vk::DescriptorSet,
    pub set4: vk::DescriptorSet,
    pub sampler: vk::Sampler,
    pub frame_ubos: Vec<GpuBuffer>,
    pub staging: GpuBuffer,
    pub next_slot: u32,
}

impl Bindless {
    pub fn create(device: &Device, allocator: &mut Allocator) -> Result<Self> {
        let sampler = unsafe {
            device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::REPEAT)
                    .address_mode_v(vk::SamplerAddressMode::REPEAT)
                    .address_mode_w(vk::SamplerAddressMode::REPEAT)
                    .max_lod(16.0),
                None,
            )?
        };

        let ubo_binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::ALL)];
        let set0_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&ubo_binding),
                None,
            )?
        };

        let heap_binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .descriptor_count(BINDLESS_HEAP_SIZE)
            .stage_flags(
                vk::ShaderStageFlags::VERTEX
                    | vk::ShaderStageFlags::FRAGMENT
                    | vk::ShaderStageFlags::COMPUTE,
            )];
        let heap_flags = [vk::DescriptorBindingFlags::UPDATE_AFTER_BIND
            | vk::DescriptorBindingFlags::PARTIALLY_BOUND
            | vk::DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT];
        let mut heap_binding_flags =
            vk::DescriptorSetLayoutBindingFlagsCreateInfo::default().binding_flags(&heap_flags);
        let set1_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default()
                    .flags(vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL)
                    .bindings(&heap_binding)
                    .push_next(&mut heap_binding_flags),
                None,
            )?
        };

        let sampler_binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .descriptor_count(1)
            .stage_flags(
                vk::ShaderStageFlags::VERTEX
                    | vk::ShaderStageFlags::FRAGMENT
                    | vk::ShaderStageFlags::COMPUTE,
            )];
        let set2_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&sampler_binding),
                None,
            )?
        };

        let set3_layout = unsafe {
            device.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default(), None)?
        };

        let storage_binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::COMPUTE)];
        let set4_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&storage_binding),
                None,
            )?
        };

        let set_layouts = [set0_layout, set1_layout, set2_layout, set3_layout, set4_layout];
        let pipeline_layout = unsafe {
            device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts),
                None,
            )?
        };

        let pool_sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER,
                descriptor_count: FRAMES_IN_FLIGHT,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLED_IMAGE,
                descriptor_count: BINDLESS_HEAP_SIZE,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLER,
                descriptor_count: 4,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_IMAGE,
                descriptor_count: 8,
            },
        ];
        let pool = unsafe {
            device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .flags(vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND)
                    .max_sets(8)
                    .pool_sizes(&pool_sizes),
                None,
            )?
        };

        let set0_layouts = vec![set0_layout; FRAMES_IN_FLIGHT as usize];
        let set0 = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&set0_layouts),
            )?
        };

        let counts = [BINDLESS_HEAP_SIZE];
        let mut var = vk::DescriptorSetVariableDescriptorCountAllocateInfo::default()
            .descriptor_counts(&counts);
        let set1 = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(std::slice::from_ref(&set1_layout))
                    .push_next(&mut var),
            )?[0]
        };

        let set2 = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(std::slice::from_ref(&set2_layout)),
            )?[0]
        };
        let set3 = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(std::slice::from_ref(&set3_layout)),
            )?[0]
        };
        let set4 = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(std::slice::from_ref(&set4_layout)),
            )?[0]
        };

        let mut frame_ubos = Vec::with_capacity(FRAMES_IN_FLIGHT as usize);
        for i in 0..FRAMES_IN_FLIGHT {
            frame_ubos.push(resources::create_buffer(
                device,
                allocator,
                UBO_SIZE,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                MemoryLocation::CpuToGpu,
                &format!("frame-cb-{i}"),
            )?);
        }

        let staging = resources::create_buffer(
            device,
            allocator,
            STAGING_SIZE,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
            "staging",
        )?;

        let samp_info = vk::DescriptorImageInfo::default().sampler(sampler);
        let samp_write = vk::WriteDescriptorSet::default()
            .dst_set(set2)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .image_info(std::slice::from_ref(&samp_info));
        unsafe {
            device.update_descriptor_sets(std::slice::from_ref(&samp_write), &[]);
        }

        for (i, set) in set0.iter().enumerate() {
            let info = vk::DescriptorBufferInfo::default()
                .buffer(frame_ubos[i].buffer)
                .offset(0)
                .range(std::mem::size_of::<FrameConstants>() as u64);
            let write = vk::WriteDescriptorSet::default()
                .dst_set(*set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(std::slice::from_ref(&info));
            unsafe {
                device.update_descriptor_sets(std::slice::from_ref(&write), &[]);
            }
        }

        Ok(Self {
            set_layouts,
            pipeline_layout,
            pool,
            set0,
            set1,
            set2,
            set3,
            set4,
            sampler,
            frame_ubos,
            staging,
            next_slot: BINDLESS_NULL_SLOT + 1,
        })
    }

    pub fn alloc_slot(&mut self) -> Result<u32> {
        if self.next_slot >= BINDLESS_HEAP_SIZE {
            return Err(RhiError::msg("bindless heap exhausted"));
        }
        let slot = self.next_slot;
        self.next_slot += 1;
        Ok(slot)
    }

    pub fn write_sampled(
        &self,
        device: &Device,
        slot: u32,
        view: vk::ImageView,
        layout: vk::ImageLayout,
    ) {
        let info = vk::DescriptorImageInfo::default()
            .image_view(view)
            .image_layout(layout);
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set1)
            .dst_binding(0)
            .dst_array_element(slot)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .image_info(std::slice::from_ref(&info));
        unsafe {
            device.update_descriptor_sets(std::slice::from_ref(&write), &[]);
        }
    }

    pub fn write_storage(&self, device: &Device, view: vk::ImageView) {
        let info = vk::DescriptorImageInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::GENERAL);
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set4)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
            .image_info(std::slice::from_ref(&info));
        unsafe {
            device.update_descriptor_sets(std::slice::from_ref(&write), &[]);
        }
    }

    pub fn write_constants(&self, slot: usize, c: FrameConstants) -> Result<()> {
        let buf = self
            .frame_ubos
            .get(slot)
            .ok_or_else(|| RhiError::msg("bad frame slot"))?;
        let ptr = buf
            .allocation
            .mapped_ptr()
            .ok_or_else(|| RhiError::msg("frame CBV is not mapped"))?
            .as_ptr()
            .cast::<u8>();
        unsafe {
            std::ptr::copy_nonoverlapping(
                (&c as *const FrameConstants).cast::<u8>(),
                ptr,
                std::mem::size_of::<FrameConstants>(),
            );
        }
        Ok(())
    }

    pub fn bind_graphics(&self, device: &Device, cmd: vk::CommandBuffer, slot: usize) {
        let sets = [self.set0[slot], self.set1, self.set2, self.set3, self.set4];
        unsafe {
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &sets,
                &[],
            );
        }
    }

    pub fn bind_compute(&self, device: &Device, cmd: vk::CommandBuffer, slot: usize) {
        let sets = [self.set0[slot], self.set1, self.set2, self.set3, self.set4];
        unsafe {
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.pipeline_layout,
                0,
                &sets,
                &[],
            );
        }
    }

    pub fn destroy(mut self, device: &Device, allocator: &mut Allocator) {
        unsafe {
            device.destroy_pipeline_layout(self.pipeline_layout, None);
            for layout in self.set_layouts {
                device.destroy_descriptor_set_layout(layout, None);
            }
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_sampler(self.sampler, None);
        }
        for ubo in self.frame_ubos.drain(..) {
            resources::destroy_buffer(device, allocator, ubo);
        }
        resources::destroy_buffer(device, allocator, self.staging);
    }
}

pub fn dummy_desc() -> TextureDesc {
    TextureDesc {
        width: 1,
        height: 1,
        mip_levels: 1,
        format: Format::Rgba8Unorm,
        sampled: true,
        storage: false,
    }
}

pub fn dummy_pixel() -> [u8; 4] {
    [255, 0, 255, 255]
}

pub fn create_dummy(
    device: &Device,
    allocator: &mut Allocator,
) -> Result<GpuImage> {
    resources::create_image(device, allocator, &dummy_desc(), BINDLESS_NULL_SLOT)
}
