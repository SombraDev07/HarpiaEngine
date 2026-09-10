use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::Vec2;
use harpia_render::{
    cloud_noise_base, cloud_noise_detail, cloud_volume_desc, CloudSliceCb, NoiseVolume,
};
use harpia_rhi::{
    Device, Extent2D, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets,
    Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

/// Where the `.raw` bake lands. Gitignored: it is derived, not authored.
fn cache_dir() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.pop();
    p.pop();
    p.push("assets");
    p.push("cache");
    p
}

struct CloudsGate {
    slice_pso: Option<GraphicsPipeline>,
    base: Option<Texture>,
    detail: Option<Texture>,
    extent: Extent2D,
}

impl Default for CloudsGate {
    fn default() -> Self {
        Self {
            slice_pso: None,
            base: None,
            detail: None,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
        }
    }
}

/// A volume has no mips here, so one upload per Z slice is the whole texture.
/// `upload_texture_mip` takes mip 0 and the RHI copies every slice in one go.
fn upload_volume(gpu: &mut Gpu, v: &NoiseVolume) -> Result<Texture> {
    let tex = gpu.create_texture(&cloud_volume_desc(v.size))?;
    gpu.upload_texture_mip(tex, 0, &v.rgba)?;
    Ok(tex)
}

impl Sample for CloudsGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        let dir = cache_dir();
        let base = cloud_noise_base(&dir).map_err(|e| anyhow::anyhow!("{e}"))?;
        let detail = cloud_noise_detail(&dir).map_err(|e| anyhow::anyhow!("{e}"))?;
        tracing::info!(
            base = base.size,
            detail = detail.size,
            cache = %dir.display(),
            "cloud noise ready"
        );
        let base_tex = upload_volume(gpu, &base)?;
        let detail_tex = upload_volume(gpu, &detail)?;
        // Slot 0 is the shape volume, slot 1 the detail. The raymarch will read
        // both; the slice viewer only shows slot 0.
        gpu.bind_volume_srv(0, base_tex)?;
        gpu.bind_volume_srv(1, detail_tex)?;
        self.base = Some(base_tex);
        self.detail = Some(detail_tex);

        self.slice_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/slice.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets::default(),
            })
            .context("slice PSO")?,
        );
        self.extent = gpu.extent();
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        let mut out = Vec::new();
        if let Some(t) = self.base {
            out.push(("noise-base", t));
        }
        if let Some(t) = self.detail {
            out.push(("noise-detail", t));
        }
        out
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        let pso = self.slice_pso.as_ref().context("slice pso")?;
        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        // Walk the volume in Z and cycle the channel, so a run exercises the
        // whole texture rather than one plane of it.
        let t = info.frame_index as f32;
        let cb = CloudSliceCb {
            inv_extent: Vec2::new(1.0 / w, 1.0 / h),
            slice: (t * 0.02).fract(),
            channel: (t * 0.25).floor() % 4.0,
        };
        gpu.write_frame_bytes(cb.as_bytes())?;
        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0])?;
        gpu.set_pipeline(pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — clouds".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, CloudsGate::default())
}
