use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_rhi::{Device, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

struct Triangle {
    pipeline: Option<GraphicsPipeline>,
}

impl Sample for Triangle {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        let pipeline = gpu
            .create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/triangle.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/triangle.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: false,
                targets: harpia_rhi::PipelineTargets::default(),
            })
            .context("hello-triangle PSO")?;
        self.pipeline = Some(pipeline);
        Ok(())
    }

    fn frame(&mut self, gpu: &mut Gpu, _info: FrameInfo) -> Result<()> {
        let pipeline = self.pipeline.as_ref().context("pipeline not init")?;
        gpu.begin_swapchain_pass([0.08, 0.10, 0.18, 1.0])?;
        gpu.set_pipeline(pipeline)?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — hello-triangle".into();
    run(config, Triangle { pipeline: None })
}
