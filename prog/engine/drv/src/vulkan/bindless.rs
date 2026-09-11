//! Bindless heap 8192, slot 0 = dummy. Spec: `docs/Bindless-Descriptor-Layout.md`.

use ash::vk;
use ash::Device;
use gpu_allocator::vulkan::Allocator;
use gpu_allocator::MemoryLocation;
use harpia_core::{BINDLESS_HEAP_SIZE, BINDLESS_NULL_SLOT};

use super::resources::{self, GpuBuffer, GpuImage};
use crate::types::{
    Format, FrameConstants, TextureDesc, TextureDim, FRAME_CBV_CHUNKS, FRAME_CBV_RING_SIZE,
    FRAME_UBO_SIZE, PUSH_CONSTANTS_SIZE, STORAGE_BUFFER_SLOTS, VOLUME_SRV_SLOTS,
    VOLUME_UAV_SLOTS,
};
use crate::{RhiError, Result, FRAMES_IN_FLIGHT};

/// `range` of set 0 binding 0. The buffer behind it is a ring of these.
const UBO_SIZE: u64 = FRAME_UBO_SIZE;
const SET_COUNT: usize = 6;
/// CPU staging for texture uploads. Sponza albedos can be 2k–4k (pitch 256).
const STAGING_SIZE: u64 = 64 * 1024 * 1024;

pub struct Bindless {
    pub set_layouts: [vk::DescriptorSetLayout; SET_COUNT],
    pub pipeline_layout: vk::PipelineLayout,
    pub pool: vk::DescriptorPool,
    pub set0: Vec<vk::DescriptorSet>,
    pub set1: vk::DescriptorSet,
    pub set2: vk::DescriptorSet,
    pub set3: vk::DescriptorSet,
    pub set4: vk::DescriptorSet,
    /// Set 5: sampled 3D (fog / clouds). Kept apart from the 2D heap, per spec.
    pub set5: vk::DescriptorSet,
    pub sampler: vk::Sampler,
    pub clamp_sampler: vk::Sampler,
    /// Depth compare (`LESS_OR_EQUAL`). Hardware PCF: compare **then** filter.
    /// Sampling depth with a plain linear sampler blends depths and compares
    /// after, which is the wrong order and wrong at every silhouette.
    pub compare_sampler: vk::Sampler,
    pub frame_ubos: Vec<GpuBuffer>,
    pub staging: GpuBuffer,
    pub next_slot: u32,
}

impl Bindless {
    pub fn create(
        device: &Device,
        allocator: &mut Allocator,
        min_ubo_align: u64,
    ) -> Result<Self> {
        if min_ubo_align == 0 || UBO_SIZE % min_ubo_align != 0 {
            return Err(RhiError::msg(format!(
                "FRAME_UBO_SIZE {UBO_SIZE} is not a multiple of minUniformBufferOffsetAlignment {min_ubo_align}"
            )));
        }
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

        let clamp_sampler = unsafe {
            device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .max_lod(16.0),
                None,
            )?
        };

        // Dynamic: one bound set, one chunk per draw. Landmine: a plain
        // UNIFORM_BUFFER means every draw in the frame reads the *last* write.
        let compare_sampler = unsafe {
            device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .compare_enable(true)
                    .compare_op(vk::CompareOp::LESS_OR_EQUAL)
                    .max_lod(0.0),
                None,
            )?
        };

        let ubo_binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
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

        let sampler_stage = vk::ShaderStageFlags::VERTEX
            | vk::ShaderStageFlags::FRAGMENT
            | vk::ShaderStageFlags::COMPUTE;
        let sampler_binding = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(1)
                .stage_flags(sampler_stage),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(1)
                .stage_flags(sampler_stage),
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(1)
                .stage_flags(sampler_stage),
        ];
        let set2_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&sampler_binding),
                None,
            )?
        };

        // Set 3: storage buffers. Estava vazio, e é o que faltava para qualquer
        // coisa GPU-driven -- listas de luzes, argumentos indirectos, contadores.
        // Uma imagem não serve: estas listas são de tamanho variável e indexadas.
        let storage_buffers = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(STORAGE_BUFFER_SLOTS)
            .stage_flags(
                vk::ShaderStageFlags::COMPUTE
                    | vk::ShaderStageFlags::FRAGMENT
                    | vk::ShaderStageFlags::VERTEX,
            )];
        let set3_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&storage_buffers),
                None,
            )?
        };

        // Set 4: binding 0 is the 2D UAV (gate-bindless), binding 1 the volume
        // UAVs (fog froxels). 3D UAVs live in GENERAL — landmine 4.
        let storage_binding = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(VOLUME_UAV_SLOTS)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
        ];
        let set4_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&storage_binding),
                None,
            )?
        };

        let volume_srv_binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .descriptor_count(VOLUME_SRV_SLOTS)
            .stage_flags(
                vk::ShaderStageFlags::VERTEX
                    | vk::ShaderStageFlags::FRAGMENT
                    | vk::ShaderStageFlags::COMPUTE,
            )];
        let set5_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&volume_srv_binding),
                None,
            )?
        };

        let set_layouts = [
            set0_layout,
            set1_layout,
            set2_layout,
            set3_layout,
            set4_layout,
            set5_layout,
        ];
        let pc_range = vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            offset: 0,
            size: PUSH_CONSTANTS_SIZE,
        };
        let pipeline_layout = unsafe {
            device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&set_layouts)
                    .push_constant_ranges(std::slice::from_ref(&pc_range)),
                None,
            )?
        };

        let pool_sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
                descriptor_count: FRAMES_IN_FLIGHT,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLED_IMAGE,
                descriptor_count: BINDLESS_HEAP_SIZE + VOLUME_SRV_SLOTS,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLER,
                descriptor_count: 6,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_IMAGE,
                descriptor_count: 8 + VOLUME_UAV_SLOTS,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: STORAGE_BUFFER_SLOTS,
            },
        ];
        let pool = unsafe {
            device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .flags(vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND)
                    .max_sets(10)
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
        let set5 = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(std::slice::from_ref(&set5_layout)),
            )?[0]
        };

        let mut frame_ubos = Vec::with_capacity(FRAMES_IN_FLIGHT as usize);
        for i in 0..FRAMES_IN_FLIGHT {
            frame_ubos.push(resources::create_buffer(
                device,
                allocator,
                FRAME_CBV_RING_SIZE,
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

        let wrap_info = vk::DescriptorImageInfo::default().sampler(sampler);
        let clamp_info = vk::DescriptorImageInfo::default().sampler(clamp_sampler);
        let wrap_write = vk::WriteDescriptorSet::default()
            .dst_set(set2)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .image_info(std::slice::from_ref(&wrap_info));
        let clamp_write = vk::WriteDescriptorSet::default()
            .dst_set(set2)
            .dst_binding(1)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .image_info(std::slice::from_ref(&clamp_info));
        let compare_info = vk::DescriptorImageInfo::default().sampler(compare_sampler);
        let compare_write = vk::WriteDescriptorSet::default()
            .dst_set(set2)
            .dst_binding(2)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .image_info(std::slice::from_ref(&compare_info));
        unsafe {
            device.update_descriptor_sets(&[wrap_write, clamp_write, compare_write], &[]);
        }

        for (i, set) in set0.iter().enumerate() {
            let info = vk::DescriptorBufferInfo::default()
                .buffer(frame_ubos[i].buffer)
                .offset(0)
                .range(UBO_SIZE);
            let write = vk::WriteDescriptorSet::default()
                .dst_set(*set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
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
            set5,
            sampler,
            clamp_sampler,
            compare_sampler,
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

    /// Set 4 binding 1, slot `n`: a 3D UAV. Layout is always GENERAL.
    pub fn write_volume_uav(&self, device: &Device, slot: u32, view: vk::ImageView) {
        let info = vk::DescriptorImageInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::GENERAL);
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set4)
            .dst_binding(1)
            .dst_array_element(slot)
            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
            .image_info(std::slice::from_ref(&info));
        unsafe {
            device.update_descriptor_sets(std::slice::from_ref(&write), &[]);
        }
    }

    /// Set 5 binding 0, slot `n`: a sampled 3D image.
    pub fn write_volume_srv(
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
            .dst_set(self.set5)
            .dst_binding(0)
            .dst_array_element(slot)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .image_info(std::slice::from_ref(&info));
        unsafe {
            device.update_descriptor_sets(std::slice::from_ref(&write), &[]);
        }
    }

    /// Liga um buffer a um slot do set 3.
    ///
    /// O descriptor fica escrito até alguém o substituir: um buffer de luzes que
    /// se reescreve todos os frames liga-se uma vez e não mais.
    pub fn write_storage_buffer(&self, device: &Device, slot: u32, buffer: vk::Buffer, size: u64) {
        let info = vk::DescriptorBufferInfo::default()
            .buffer(buffer)
            .offset(0)
            .range(size.max(1));
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set3)
            .dst_binding(0)
            .dst_array_element(slot)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(std::slice::from_ref(&info));
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

    pub fn write_constants(&self, slot: usize, offset: u32, c: FrameConstants) -> Result<()> {
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&c as *const FrameConstants).cast::<u8>(),
                std::mem::size_of::<FrameConstants>(),
            )
        };
        self.write_bytes(slot, offset, bytes)
    }

    /// Copy one chunk of the frame ring. `offset` is a multiple of [`UBO_SIZE`].
    pub fn write_bytes(&self, slot: usize, offset: u32, data: &[u8]) -> Result<()> {
        let buf = self
            .frame_ubos
            .get(slot)
            .ok_or_else(|| RhiError::msg("bad frame slot"))?;
        if data.len() as u64 > UBO_SIZE {
            return Err(RhiError::msg("frame CBV write larger than one chunk"));
        }
        if offset as u64 + UBO_SIZE > buf.size {
            return Err(RhiError::msg("frame CBV chunk out of the ring"));
        }
        let ptr = buf
            .allocation
            .mapped_ptr()
            .ok_or_else(|| RhiError::msg("frame CBV is not mapped"))?
            .as_ptr()
            .cast::<u8>();
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.add(offset as usize), data.len());
        }
        Ok(())
    }

    pub fn bind_graphics(&self, device: &Device, cmd: vk::CommandBuffer, slot: usize, cbv: u32) {
        let sets = [self.set0[slot], self.set1, self.set2, self.set3, self.set4, self.set5];
        unsafe {
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &sets,
                &[cbv],
            );
        }
    }

    /// Re-point set 0 at another chunk. Sets 1–4 stay bound.
    pub fn bind_graphics_cbv(&self, device: &Device, cmd: vk::CommandBuffer, slot: usize, cbv: u32) {
        unsafe {
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                std::slice::from_ref(&self.set0[slot]),
                &[cbv],
            );
        }
    }

    pub fn bind_compute(&self, device: &Device, cmd: vk::CommandBuffer, slot: usize, cbv: u32) {
        let sets = [self.set0[slot], self.set1, self.set2, self.set3, self.set4, self.set5];
        unsafe {
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.pipeline_layout,
                0,
                &sets,
                &[cbv],
            );
        }
    }

    pub fn bind_compute_cbv(&self, device: &Device, cmd: vk::CommandBuffer, slot: usize, cbv: u32) {
        unsafe {
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.pipeline_layout,
                0,
                std::slice::from_ref(&self.set0[slot]),
                &[cbv],
            );
        }
    }

    /// Byte offset of chunk `n` in the frame ring.
    pub fn chunk_offset(n: u32) -> Result<u32> {
        if n >= FRAME_CBV_CHUNKS {
            return Err(RhiError::msg(
                "frame CBV ring exhausted: more write_frame_bytes than FRAME_CBV_CHUNKS",
            ));
        }
        Ok(n * UBO_SIZE as u32)
    }

    pub fn destroy(mut self, device: &Device, allocator: &mut Allocator) {
        unsafe {
            device.destroy_pipeline_layout(self.pipeline_layout, None);
            for layout in self.set_layouts {
                device.destroy_descriptor_set_layout(layout, None);
            }
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_sampler(self.clamp_sampler, None);
            device.destroy_sampler(self.compare_sampler, None);
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
        depth_slices: 1,
        dim: TextureDim::D2,
        mip_levels: 1,
        format: Format::Rgba8Unorm,
        sampled: true,
        storage: false,
        color_attachment: false,
        depth: false,
    }
}

pub fn dummy_pixel() -> [u8; 4] {
    [255, 0, 255, 255]
}

pub fn create_dummy(device: &Device, allocator: &mut Allocator) -> Result<GpuImage> {
    resources::create_image(device, allocator, &dummy_desc(), BINDLESS_NULL_SLOT)
}

/// 1×1×1 volume. Every slot of set 4 binding 1 and set 5 binding 0 points here
/// until a real volume claims it — those arrays are not PARTIALLY_BOUND, so a
/// stale descriptor is a validation error the moment a shader is dispatched.
pub fn volume_dummy_desc() -> TextureDesc {
    TextureDesc {
        width: 1,
        height: 1,
        depth_slices: 1,
        dim: TextureDim::D3,
        mip_levels: 1,
        format: Format::Rgba16Float,
        sampled: true,
        storage: true,
        color_attachment: false,
        depth: false,
    }
}

pub fn create_volume_dummy(device: &Device, allocator: &mut Allocator) -> Result<GpuImage> {
    resources::create_image(device, allocator, &volume_dummy_desc(), BINDLESS_NULL_SLOT)
}
