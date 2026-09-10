use anyhow::{Context, Result};
use harpia_math::{perspective_vk, Mat4, Vec2, Vec3, Vec4};
use harpia_app::{run, AppConfig, Sample};
use harpia_render::{
    color_desc, transmittance_desc, AtmosphereCb, TRANSMITTANCE_H, TRANSMITTANCE_W,
};
use harpia_rhi::{
    Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const LUT: Format = Format::Rgba16Float;
const LUT_FORMATS: [Format; 1] = [LUT];
const COMPOSITE: Format = Format::Rgba8Unorm;
const COMPOSITE_FORMATS: [Format; 1] = [COMPOSITE];

struct SkyGate {
    transmittance_pso: Option<GraphicsPipeline>,
    sky_pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    transmittance: Option<Texture>,
    composite: Option<Texture>,
    extent: Extent2D,
}

impl Default for SkyGate {
    fn default() -> Self {
        Self {
            transmittance_pso: None,
            sky_pso: None,
            blit_pso: None,
            transmittance: None,
            composite: None,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
        }
    }
}

impl SkyGate {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.composite = Some(gpu.create_texture(&color_desc(w, h, COMPOSITE))?);
        self.extent = Extent2D { width: w, height: h };
        Ok(())
    }
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

impl Sample for SkyGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.transmittance_pso = Some(
            fullscreen(
                gpu,
                include_bytes!(concat!(env!("OUT_DIR"), "/transmittance.ps.spv")),
                &LUT_FORMATS,
            )
            .context("transmittance PSO")?,
        );
        self.sky_pso = Some(
            fullscreen(
                gpu,
                include_bytes!(concat!(env!("OUT_DIR"), "/sky.ps.spv")),
                &COMPOSITE_FORMATS,
            )
            .context("sky PSO")?,
        );
        // Blit is the only pass that targets the swapchain, so it keeps the
        // default (empty colour formats = present format).
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
        self.transmittance = Some(gpu.create_texture(&transmittance_desc())?);
        self.recreate(gpu, gpu.extent())?;
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        let mut out = Vec::new();
        if let Some(t) = self.composite {
            out.push(("sky", t));
        }
        if let Some(t) = self.transmittance {
            out.push(("transmittance-lut", t));
        }
        out
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let composite = self.composite.context("composite")?;
        let transmittance = self.transmittance.context("transmittance")?;
        let transmittance_pso = self.transmittance_pso.as_ref().context("lut pso")?;
        let sky_pso = self.sky_pso.as_ref().context("sky pso")?;
        let blit_pso = self.blit_pso.as_ref().context("blit pso")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;

        // The sun sinks through the frames: the LUTs are rebuilt every frame, so
        // the time of day is free to move. That is the whole reason for picking
        // Hillaire over a Bruneton bake.
        let elevation = (18.0 - info.frame_index as f32 * 0.6).max(1.0).to_radians();
        let sun = Vec3::new(0.0, elevation.sin(), -elevation.cos()).normalize();

        let eye = Vec3::new(0.0, 1.0, 0.0);
        let view = Mat4::look_at_rh(eye, eye + Vec3::new(0.0, 0.18, -1.0), Vec3::Y);
        let proj = perspective_vk(60.0_f32.to_radians(), w / h, 0.1, 1000.0);
        let view_proj = proj * view;

        let base = AtmosphereCb {
            inv_view_proj: view_proj.inverse(),
            // w is the altitude in km: the ray direction comes from the world
            // matrices, the radius from here.
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, 0.5),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            sun_illuminance: Vec4::new(20.0, 20.0, 20.0, 1.0),
            ..Default::default()
        };

        // 1. transmittance LUT. Its own inv_extent — the pass is 256x64.
        let mut lut_cb = base;
        lut_cb.inv_extent = Vec2::new(1.0 / TRANSMITTANCE_W as f32, 1.0 / TRANSMITTANCE_H as f32);
        gpu.write_frame_bytes(lut_cb.as_bytes())?;
        gpu.begin_color_pass(&[transmittance], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(transmittance_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        // 2. sky: raymarch per pixel, sun visibility from the LUT.
        let mut sky_cb = base;
        sky_cb.transmittance_lut = gpu.bindless_index(transmittance)?;
        sky_cb.inv_extent = Vec2::new(1.0 / w, 1.0 / h);
        gpu.write_frame_bytes(sky_cb.as_bytes())?;
        gpu.begin_color_pass(&[composite], None, &[[0.0, 0.0, 0.0, 1.0]], None)?;
        gpu.set_pipeline(sky_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_color_pass()?;

        // 3. present. A fresh chunk re-points the index at the composite.
        let mut blit_cb = sky_cb;
        blit_cb.transmittance_lut = gpu.bindless_index(composite)?;
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
    config.title = "Harpia — sky".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, SkyGate::default())
}
