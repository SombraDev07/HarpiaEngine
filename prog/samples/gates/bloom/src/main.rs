//! Fase 7, gate `bloom`: FidelityFX SPD (Karis) on a bright HDR field.
//!
//! `-- --no-bloom` skips the pyramid. The pixel has to change.

use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_ffx_spd::{self as spd, Dispatch};
use harpia_math::Vec4;
use harpia_render::{color_desc, Access, Pass, RenderGraph};
use harpia_rhi::{
    ComputePipeline, ComputePipelineDesc, Device, Extent2D, Format, FrameInfo, Gpu,
    GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets, Texture, TextureDesc, TextureDim,
    STORAGE_IMAGE_SLOTS,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const HDR: Format = Format::Rgba16Float;
const HDR_FORMATS: [Format; 1] = [HDR];
const THRESHOLD: f32 = 1.0;
const ATOMIC_SLOT: u32 = 9;

#[repr(C)]
#[derive(Clone, Copy)]
struct SceneCb {
    inv_extent: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CopyCb {
    params: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CompositeCb {
    inv_extent: [f32; 2],
    hdr: u32,
    pyramid: u32,
    mips: u32,
    enable: u32,
    _pad: [u32; 2],
}

struct Targets {
    hdr: Texture,
    pyramid: Texture,
    dispatch: Dispatch,
}

struct BloomGate {
    scene_pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    copy_pso: Option<ComputePipeline>,
    spd_pso: Option<ComputePipeline>,
    atomic: Option<harpia_rhi::Buffer>,
    rt: Option<Targets>,
    extent: Extent2D,
    bloom: bool,
}

impl Default for BloomGate {
    fn default() -> Self {
        Self {
            scene_pso: None,
            blit_pso: None,
            copy_pso: None,
            spd_pso: None,
            atomic: None,
            rt: None,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
            bloom: true,
        }
    }
}

impl BloomGate {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        let dispatch = spd::setup(w, h, None);
        let mips = dispatch.texture_mips().min(STORAGE_IMAGE_SLOTS);
        self.rt = Some(Targets {
            hdr: gpu.create_texture(&color_desc(w, h, HDR))?,
            pyramid: gpu.create_texture(&TextureDesc {
                width: w,
                height: h,
                depth_slices: 1,
                dim: TextureDim::D2,
                mip_levels: mips,
                format: HDR,
                sampled: true,
                storage: true,
                color_attachment: false,
                depth: false,
            })?,
            dispatch,
        });
        self.extent = Extent2D {
            width: w,
            height: h,
        };
        Ok(())
    }
}

fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((v as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}

impl Sample for BloomGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.scene_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/scene.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &HDR_FORMATS,
                    ..Default::default()
                },
            })
            .context("bloom scene PSO")?,
        );
        self.blit_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/composite.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets::default(),
            })
            .context("bloom blit PSO")?,
        );
        self.copy_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/copy.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("bloom copy CS")?,
        );
        self.spd_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: spd::karis_spirv(),
                cs_entry: "CSMain",
            })
            .context("spd karis CS")?,
        );
        self.atomic = Some(gpu.create_storage_buffer(ATOMIC_SLOT, &spd::atomic_zeros())?);
        Ok(())
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if self.extent != info.extent {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("bloom rt")?;
        let w = info.extent.width.max(1);
        let h = info.extent.height.max(1);
        let hdr_idx = gpu.bindless_index(rt.hdr)?;
        let pyr_idx = gpu.bindless_index(rt.pyramid)?;
        let mips = rt.dispatch.texture_mips().min(STORAGE_IMAGE_SLOTS);
        let enable = u32::from(self.bloom);

        let mut g = RenderGraph::new();
        let hdr = g.texture("hdr", rt.hdr);
        let pyr = g.texture("pyramid", rt.pyramid);
        g.pass(Pass::new("scene").uses(hdr, Access::ColorWrite));
        if self.bloom {
            g.pass(
                Pass::new("copy")
                    .uses(hdr, Access::Sampled)
                    .uses(pyr, Access::StorageWrite),
            );
            g.pass(
                Pass::new("spd")
                    .uses(pyr, Access::StorageWrite)
                    .uses(pyr, Access::StorageRead),
            );
            g.pass(
                Pass::new("blit")
                    .uses(hdr, Access::Sampled)
                    .uses(pyr, Access::Sampled),
            );
        } else {
            g.pass(Pass::new("blit").uses(hdr, Access::Sampled));
        }
        if let Err(errors) = g.validate() {
            anyhow::bail!(
                "render graph inválido: {}",
                errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
        let mut plans = g.compile().into_iter();
        let mut next = || plans.next().map(|p| p.barriers).unwrap_or_default();

        gpu.barriers(&next())?;
        gpu.write_frame_bytes(as_bytes(&SceneCb {
            inv_extent: [1.0 / w as f32, 1.0 / h as f32],
            _pad: [0.0, 0.0],
        }))?;
        gpu.begin_color_pass(&[rt.hdr], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(self.scene_pso.as_ref().context("scene pso")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;
        gpu.mark("scene");

        if self.bloom {
            gpu.barriers(&next())?;
            gpu.bind_storage_image(rt.pyramid, 0, 0)?;
            gpu.write_frame_bytes(as_bytes(&CopyCb {
                params: Vec4::new(hdr_idx as f32, w as f32, h as f32, THRESHOLD),
            }))?;
            gpu.bind_compute_bindless()?;
            gpu.set_compute_pipeline(self.copy_pso.as_ref().context("copy pso")?)?;
            gpu.dispatch((w + 7) / 8, (h + 7) / 8, 1)?;
            gpu.mark("bloom-copy");

            gpu.barriers(&next())?;
            for mip in 0..mips {
                gpu.bind_storage_image(rt.pyramid, mip, mip)?;
            }
            gpu.write_frame_bytes(&rt.dispatch.constants_bytes())?;
            gpu.bind_compute_bindless()?;
            gpu.set_compute_pipeline(self.spd_pso.as_ref().context("spd pso")?)?;
            gpu.dispatch(rt.dispatch.groups_x, rt.dispatch.groups_y, 1)?;
            gpu.mark("spd");
        }

        gpu.barriers(&next())?;
        gpu.write_frame_bytes(as_bytes(&CompositeCb {
            inv_extent: [1.0 / w as f32, 1.0 / h as f32],
            hdr: hdr_idx,
            pyramid: pyr_idx,
            mips,
            enable,
            _pad: [0, 0],
        }))?;
        gpu.begin_swapchain_pass([0.02, 0.03, 0.05, 1.0])?;
        gpu.set_pipeline(self.blit_pso.as_ref().context("blit pso")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.rt
            .as_ref()
            .map(|rt| vec![("hdr", rt.hdr), ("pyramid", rt.pyramid)])
            .unwrap_or_default()
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — bloom (FFX SPD)".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    let bloom = !config.extra.iter().any(|a| a == "--no-bloom");
    run(
        config,
        BloomGate {
            bloom,
            ..BloomGate::default()
        },
    )
}
