//! Fase 7, gate `exposure`: average luma → 1×1 R32 → ACES. Dual RT.

use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::Vec4;
use harpia_render::{color_desc, ADAPT_SPEED, EXPOSURE_UAV_SLOT};
use harpia_rhi::{
    Buffer, ComputePipeline, ComputePipelineDesc, Device, Extent2D, Format, FrameInfo, Gpu,
    GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets, Texture, TextureDesc, TextureDim,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const HDR: Format = Format::Rgba16Float;
const HDR_ONLY: [Format; 1] = [HDR];
const LDR: [Format; 1] = [Format::Rgba8Unorm];
const ACC_SLOT: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
struct SceneCb {
    inv_extent: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LumaCb {
    params: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AdaptCb {
    params: Vec4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TonemapCb {
    inv_extent: [f32; 2],
    hdr: u32,
    exposure: u32,
    enable: u32,
    _pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BlitCb {
    inv_extent: [f32; 2],
    src: u32,
    _pad: u32,
}

struct Targets {
    hdr: Texture,
    exp: Texture,
    on: Texture,
    off: Texture,
}

struct ExposureGate {
    scene_pso: Option<GraphicsPipeline>,
    luma_pso: Option<ComputePipeline>,
    adapt_pso: Option<ComputePipeline>,
    tone_pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    acc: Option<Buffer>,
    rt: Option<Targets>,
    extent: Extent2D,
    compared: bool,
}

impl Default for ExposureGate {
    fn default() -> Self {
        Self {
            scene_pso: None,
            luma_pso: None,
            adapt_pso: None,
            tone_pso: None,
            blit_pso: None,
            acc: None,
            rt: None,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
            compared: false,
        }
    }
}

fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((v as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}

impl ExposureGate {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        let exp = gpu.create_texture(&TextureDesc {
            width: 1,
            height: 1,
            depth_slices: 1,
            dim: TextureDim::D2,
            mip_levels: 1,
            format: Format::R32Float,
            sampled: true,
            storage: true,
            color_attachment: false,
            depth: false,
        })?;
        // Storage + sampled starts in GENERAL. Don't `upload_texture_mip`: that
        // barriers from TRANSFER_DST and trips validation. Adapt treats 0 as 1.0.
        self.rt = Some(Targets {
            hdr: gpu.create_texture(&color_desc(w, h, HDR))?,
            exp,
            on: gpu.create_texture(&color_desc(w, h, Format::Rgba8Unorm))?,
            off: gpu.create_texture(&color_desc(w, h, Format::Rgba8Unorm))?,
        });
        self.extent = Extent2D {
            width: w,
            height: h,
        };
        Ok(())
    }
}

impl Sample for ExposureGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.scene_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/scene.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &HDR_ONLY,
                    ..Default::default()
                },
            })
            .context("exposure scene")?,
        );
        self.luma_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/luma.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("luma CS")?,
        );
        self.adapt_pso = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/adapt.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("adapt CS")?,
        );
        self.tone_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/tonemap.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &LDR,
                    ..Default::default()
                },
            })
            .context("tonemap")?,
        );
        self.blit_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/blit.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets::default(),
            })
            .context("blit")?,
        );
        self.acc = Some(gpu.create_storage_buffer(ACC_SLOT, &[0u8; 8])?);
        Ok(())
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if self.extent != info.extent {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let w = info.extent.width.max(1);
        let h = info.extent.height.max(1);
        gpu.write_frame_bytes(as_bytes(&SceneCb {
            inv_extent: [1.0 / w as f32, 1.0 / h as f32],
            _pad: [0.0, 0.0],
        }))?;
        gpu.begin_color_pass(&[rt.hdr], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(self.scene_pso.as_ref().context("scene")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        gpu.bind_storage_image(rt.exp, 0, EXPOSURE_UAV_SLOT)?;
        gpu.write_frame_bytes(as_bytes(&LumaCb {
            params: Vec4::new(
                gpu.bindless_index(rt.hdr)? as f32,
                w as f32,
                h as f32,
                ACC_SLOT as f32,
            ),
        }))?;
        gpu.bind_compute_bindless()?;
        gpu.set_compute_pipeline(self.luma_pso.as_ref().context("luma")?)?;
        gpu.dispatch(w.div_ceil(8), h.div_ceil(8), 1)?;
        gpu.storage_barrier_buffer(self.acc.context("acc")?)?;

        gpu.write_frame_bytes(as_bytes(&AdaptCb {
            params: Vec4::new(1.0 / 60.0, ADAPT_SPEED, ACC_SLOT as f32, 0.0),
        }))?;
        gpu.bind_compute_bindless()?;
        gpu.set_compute_pipeline(self.adapt_pso.as_ref().context("adapt")?)?;
        gpu.dispatch(1, 1, 1)?;
        gpu.storage_barrier(rt.exp)?;
        gpu.mark("exposure");

        let tone = TonemapCb {
            inv_extent: [1.0 / w as f32, 1.0 / h as f32],
            hdr: gpu.bindless_index(rt.hdr)?,
            exposure: gpu.bindless_index(rt.exp)?,
            enable: 1,
            _pad: [0; 3],
        };
        gpu.write_frame_bytes(as_bytes(&tone))?;
        gpu.begin_color_pass(&[rt.on], None, &[[0.0; 4]], None)?;
        gpu.set_pipeline(self.tone_pso.as_ref().context("tone")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        let mut off = tone;
        off.enable = 0;
        gpu.write_frame_bytes(as_bytes(&off))?;
        gpu.begin_color_pass(&[rt.off], None, &[[0.0; 4]], None)?;
        gpu.set_pipeline(self.tone_pso.as_ref().context("tone")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        gpu.write_frame_bytes(as_bytes(&BlitCb {
            inv_extent: [1.0 / w as f32, 1.0 / h as f32],
            src: gpu.bindless_index(rt.on)?,
            _pad: 0,
        }))?;
        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0])?;
        gpu.set_pipeline(self.blit_pso.as_ref().context("blit")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }

    fn finish(&mut self, gpu: &mut Gpu) -> Result<()> {
        if self.compared {
            return Ok(());
        }
        self.compared = true;
        let rt = self.rt.as_ref().context("rt")?;
        let on = gpu.read_texture(rt.on)?;
        let off = gpu.read_texture(rt.off)?;
        anyhow::ensure!(on.bytes.len() == off.bytes.len());
        let mut differing = 0u64;
        for (a, b) in on.bytes.iter().zip(off.bytes.iter()) {
            if a != b {
                differing += 1;
            }
        }
        let pct = 100.0 * differing as f64 / on.bytes.len().max(1) as f64;
        tracing::info!(
            canais_diferentes = differing,
            percent = format!("{pct:.2}"),
            "auto-exposure on contra 1.0"
        );
        anyhow::ensure!(
            pct > 5.0,
            "auto-exposure mudou só {pct:.3}%: o 1×1 não está no tonemap"
        );
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.rt
            .as_ref()
            .map(|rt| vec![("on", rt.on), ("off", rt.off), ("hdr", rt.hdr)])
            .unwrap_or_default()
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — auto-exposure".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, ExposureGate::default())
}
