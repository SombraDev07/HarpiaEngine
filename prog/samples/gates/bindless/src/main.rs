use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_rhi::{
    ComputePipeline, ComputePipelineDesc, Device, Format, FrameConstants, FrameInfo, Gpu,
    GraphicsPipeline, GraphicsPipelineDesc, Texture, TextureDesc,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

struct BindlessGate {
    gfx: Option<GraphicsPipeline>,
    cs: Option<ComputePipeline>,
    cpu_tex: Option<Texture>,
    uav_tex: Option<Texture>,
}

impl Sample for BindlessGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.gfx = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/bindless.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/bindless.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
            })
            .context("bindless graphics PSO")?,
        );
        self.cs = Some(
            gpu.create_compute_pipeline(&ComputePipelineDesc {
                cs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/bindless.cs.spv")),
                cs_entry: "CSMain",
            })
            .context("bindless compute PSO")?,
        );

        let cpu = gpu
            .create_texture(&TextureDesc {
                width: 4,
                height: 4,
                mip_levels: 3,
                format: Format::Rgba8Unorm,
                sampled: true,
                storage: false,
            })
            .context("cpu checker")?;
        gpu.upload_texture_mip(cpu, 0, &checker_mip0())?;
        gpu.upload_texture_mip(cpu, 1, &checker_mip1())?;
        gpu.upload_texture_mip(cpu, 2, &checker_mip2())?;
        self.cpu_tex = Some(cpu);

        let uav = gpu
            .create_texture(&TextureDesc {
                width: 64,
                height: 64,
                mip_levels: 1,
                format: Format::Rgba8Unorm,
                sampled: true,
                storage: true,
            })
            .context("uav")?;
        self.uav_tex = Some(uav);
        Ok(())
    }

    fn frame(&mut self, gpu: &mut Gpu, _info: FrameInfo) -> Result<()> {
        let gfx = self.gfx.as_ref().context("gfx")?;
        let cs = self.cs.as_ref().context("cs")?;
        let cpu = self.cpu_tex.context("cpu tex")?;
        let uav = self.uav_tex.context("uav tex")?;
        let tex_a = gpu.bindless_index(cpu)?;
        let tex_b = gpu.bindless_index(uav)?;

        gpu.write_frame_constants(FrameConstants {
            tex_a,
            tex_b,
            _pad: [0, 0],
        })?;
        gpu.bind_compute_bindless()?;
        gpu.set_compute_pipeline(cs)?;
        gpu.dispatch(8, 8, 1)?;
        gpu.storage_barrier(uav)?;

        gpu.begin_swapchain_pass([0.08, 0.10, 0.18, 1.0])?;
        gpu.set_pipeline(gfx)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }
}

fn checker_mip0() -> [u8; 64] {
    let mut out = [0u8; 64];
    for y in 0..4 {
        for x in 0..4 {
            let i = (y * 4 + x) * 4;
            let on = (x + y) % 2 == 0;
            if on {
                out[i] = 40;
                out[i + 1] = 200;
                out[i + 2] = 80;
                out[i + 3] = 255;
            } else {
                out[i] = 200;
                out[i + 1] = 40;
                out[i + 2] = 40;
                out[i + 3] = 255;
            }
        }
    }
    out
}

fn checker_mip1() -> [u8; 16] {
    let mut out = [0u8; 16];
    for y in 0..2 {
        for x in 0..2 {
            let i = (y * 2 + x) * 4;
            out[i] = 40;
            out[i + 1] = 80;
            out[i + 2] = 220;
            out[i + 3] = 255;
        }
    }
    out
}

fn checker_mip2() -> [u8; 4] {
    [220, 200, 40, 255]
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — bindless".into();
    let user_set_frames = std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(
        config,
        BindlessGate {
            gfx: None,
            cs: None,
            cpu_tex: None,
            uav_tex: None,
        },
    )
}
