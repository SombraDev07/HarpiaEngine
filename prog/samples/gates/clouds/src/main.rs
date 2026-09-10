use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{perspective_vk, Mat4, Vec2, Vec3, Vec4};
use harpia_render::{
    cloud_noise_base, cloud_noise_detail, cloud_volume_desc, color_desc, CloudCb, NoiseVolume,
};
use harpia_rhi::{
    Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

/// Scattered light and transmittance, at half resolution — §9 says half-res.
const CLOUD_RT: Format = Format::Rgba16Float;
const CLOUD_FORMATS: [Format; 1] = [CLOUD_RT];
const COMPOSITE: Format = Format::Rgba8Unorm;
const COMPOSITE_FORMATS: [Format; 1] = [COMPOSITE];

const GROUND_KM: f32 = 6360.0;
const CAMERA_ALT_KM: f32 = 0.5;
const FOV_Y: f32 = 60.0;

fn cache_dir() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..4 {
        p.pop();
    }
    p.push("assets");
    p.push("cache");
    p
}

struct Targets {
    clouds: Texture,
    composite: Texture,
}

struct CloudsGate {
    clouds_pso: Option<GraphicsPipeline>,
    composite_pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    base: Option<Texture>,
    detail: Option<Texture>,
    rt: Option<Targets>,
    extent: Extent2D,
}

impl Default for CloudsGate {
    fn default() -> Self {
        Self {
            clouds_pso: None,
            composite_pso: None,
            blit_pso: None,
            base: None,
            detail: None,
            rt: None,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
        }
    }
}

impl CloudsGate {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(2);
        let h = extent.height.max(2);
        self.rt = Some(Targets {
            clouds: gpu.create_texture(&color_desc(w / 2, h / 2, CLOUD_RT))?,
            composite: gpu.create_texture(&color_desc(w, h, COMPOSITE))?,
        });
        self.extent = Extent2D { width: w, height: h };
        Ok(())
    }
}

/// A volume has no mips: one upload carries every slice.
fn upload_volume(gpu: &mut Gpu, v: &NoiseVolume) -> Result<Texture> {
    let tex = gpu.create_texture(&cloud_volume_desc(v.size))?;
    gpu.upload_texture_mip(tex, 0, &v.rgba)?;
    Ok(tex)
}

fn fullscreen(
    gpu: &mut Gpu,
    fs: &'static [u8],
    formats: &'static [Format],
) -> Result<GraphicsPipeline> {
    gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
        vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
        fs_spirv: fs,
        vs_entry: "VSMain",
        fs_entry: "PSMain",
        bindless: true,
        targets: PipelineTargets {
            color_formats: formats,
            ..Default::default()
        },
    })
    .map_err(Into::into)
}

impl Sample for CloudsGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        let dir = cache_dir();
        let base = cloud_noise_base(&dir).map_err(|e| anyhow::anyhow!("{e}"))?;
        let detail = cloud_noise_detail(&dir).map_err(|e| anyhow::anyhow!("{e}"))?;
        let base_tex = upload_volume(gpu, &base)?;
        let detail_tex = upload_volume(gpu, &detail)?;
        // Slot 0 shape, slot 1 detail. The raymarch reads both by constant index.
        gpu.bind_volume_srv(0, base_tex)?;
        gpu.bind_volume_srv(1, detail_tex)?;
        self.base = Some(base_tex);
        self.detail = Some(detail_tex);
        tracing::info!(base = base.size, detail = detail.size, "cloud noise ready");

        self.clouds_pso = Some(
            fullscreen(
                gpu,
                include_bytes!(concat!(env!("OUT_DIR"), "/clouds.ps.spv")),
                &CLOUD_FORMATS,
            )
            .context("clouds PSO")?,
        );
        self.composite_pso = Some(
            fullscreen(
                gpu,
                include_bytes!(concat!(env!("OUT_DIR"), "/composite.ps.spv")),
                &COMPOSITE_FORMATS,
            )
            .context("composite PSO")?,
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
            .context("blit PSO")?,
        );
        self.recreate(gpu, gpu.extent())?;
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        let mut out = Vec::new();
        if let Some(rt) = self.rt.as_ref() {
            out.push(("clouds", rt.composite));
            out.push(("cloud-rt", rt.clouds));
        }
        out
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let clouds_pso = self.clouds_pso.as_ref().context("clouds pso")?;
        let composite_pso = self.composite_pso.as_ref().context("composite pso")?;
        let blit_pso = self.blit_pso.as_ref().context("blit pso")?;

        let w = info.extent.width.max(2) as f32;
        let h = info.extent.height.max(2) as f32;
        let eye = Vec3::new(0.0, 1.0, 0.0);
        let view = Mat4::look_at_rh(eye, eye + Vec3::new(0.0, 0.30, -1.0), Vec3::Y);
        let proj = perspective_vk(FOV_Y.to_radians(), w / h, 0.1, 1000.0);
        let view_proj = proj * view;
        let sun_elev = 26.0_f32.to_radians();
        let sun = Vec3::new(0.35, sun_elev.sin(), -sun_elev.cos()).normalize();
        // The wind moves the volume rather than the clouds, which is what makes
        // a tileable noise worth having.
        let t = info.frame_index as f32 * 0.01;

        let base = CloudCb {
            inv_view_proj: view_proj.inverse(),
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, CAMERA_ALT_KM),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            wind: Vec4::new(t, 0.0, t * 0.6, 0.0),
            // Sky radiance, not a colour: the clouds are lit at ~3, so the sky has to
            // sit well under that or everything tonemaps to white.
            ambient: Vec4::new(0.10, 0.15, 0.26, GROUND_KM),
            ..Default::default()
        };

        // 1. raymarch at half resolution.
        let mut cloud_cb = base;
        cloud_cb.inv_extent = Vec2::new(2.0 / w, 2.0 / h);
        gpu.write_frame_bytes(cloud_cb.as_bytes())?;
        gpu.begin_color_pass(&[rt.clouds], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(clouds_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        // 2. composite full res: sky behind, clouds over, tonemap.
        let mut comp_cb = base;
        comp_cb.cloud_rt = gpu.bindless_index(rt.clouds)?;
        comp_cb.inv_extent = Vec2::new(1.0 / w, 1.0 / h);
        gpu.write_frame_bytes(comp_cb.as_bytes())?;
        gpu.begin_color_pass(&[rt.composite], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(composite_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        // 3. present.
        let mut blit_cb = comp_cb;
        blit_cb.cloud_rt = gpu.bindless_index(rt.composite)?;
        gpu.write_frame_bytes(blit_cb.as_bytes())?;
        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0])?;
        gpu.set_pipeline(blit_pso)?;
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
