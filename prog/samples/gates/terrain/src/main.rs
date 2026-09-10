//! Fase 6, gate do terreno: clipmap por `SV_VertexID`.
//!
//! Sem vertex buffer e sem index buffer — o roadmap pede assim, e a razão é que a
//! malha de um clipmap é sempre a mesma grelha. Um `draw` por nível, e o VS
//! calcula a posição a partir do índice do vértice e do índice da instância.
//!
//! A câmara voa (WASD) e mantém-se acima do chão usando a **mesma** função de
//! altura que o VS desenha. Se as duas discordarem vê-se logo: a câmara atravessa
//! o terreno ou flutua. É o ensaio do gate `heightquery`, que vem a seguir.

use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{Vec2, Vec3, Vec4};
use harpia_render::{
    clipmap_range, clipmap_vertex_count, color_desc, depth_desc, terrain_height, FlyCamera,
    TerrainCb, CLIPMAP_LEVELS, GBUFFER_DEPTH_FORMAT,
};
use harpia_rhi::{
    Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const FOV_Y: f32 = 60.0;
/// Quanto a câmara se mantém acima do chão quando bate nele.
const EYE_CLEARANCE: f32 = 2.0;
/// Linear: o blit é que faz o tonemap, como no resto da árvore.
const HDR: Format = Format::Rgba16Float;
const HDR_FORMATS: [Format; 1] = [HDR];

struct TerrainGate {
    pso: Option<GraphicsPipeline>,
    blit_pso: Option<GraphicsPipeline>,
    color: Option<Texture>,
    depth: Option<Texture>,
    extent: Extent2D,
    cam: FlyCamera,
}

impl Default for TerrainGate {
    fn default() -> Self {
        Self {
            pso: None,
            blit_pso: None,
            color: None,
            depth: None,
            extent: Extent2D { width: 0, height: 0 },
            cam: FlyCamera {
                speed: 40.0,
                boost: 8.0,
                fov_y: FOV_Y.to_radians(),
                near: 0.5,
                // O far tem de cobrir o último nível, senão o clipmap é cortado
                // pelo frustum antes de acabar e vê-se a borda.
                far: clipmap_range() * 1.5,
                ..FlyCamera::looking_at(Vec3::new(0.0, 90.0, 0.0), Vec3::new(120.0, 60.0, -120.0))
            },
        }
    }
}

impl TerrainGate {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.color = Some(gpu.create_texture(&color_desc(w, h, HDR))?);
        self.depth = Some(gpu.create_texture(&depth_desc(w, h))?);
        self.extent = Extent2D { width: w, height: h };
        Ok(())
    }
}

impl Sample for TerrainGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/terrain.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/terrain.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &HDR_FORMATS,
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    depth_test: true,
                    cull_back: true,
                    ..Default::default()
                },
            })
            .context("terrain PSO")?,
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
        tracing::info!(
            levels = CLIPMAP_LEVELS,
            verts_por_nivel = clipmap_vertex_count(),
            alcance = clipmap_range(),
            "clipmap"
        );
        Ok(())
    }

    fn update(&mut self, input: &harpia_app::SampleInput, dt: f32) {
        self.cam.update(input, dt);
        // A mesma função que o VS desenha. Se divergirem isto passa a ver-se.
        let ground = terrain_height(self.cam.position.x, self.cam.position.z);
        self.cam.position.y = self.cam.position.y.max(ground + EYE_CLEARANCE);
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.color.map_or_else(Vec::new, |c| vec![("terrain", c)])
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let pso = self.pso.as_ref().context("pso")?;
        let blit_pso = self.blit_pso.as_ref().context("blit pso")?;
        let color = self.color.context("color")?;
        let depth = self.depth.context("depth")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        // Sem input (todo o modo gate) a câmara está parada, portanto anda-se de
        // propósito: um clipmap parado não prova que o snap funciona.
        if info.frame_index > 0 {
            let t = info.frame_index as f32 * 0.9;
            self.cam.position.x = t;
            self.cam.position.z = -t * 0.6;
            let ground = terrain_height(self.cam.position.x, self.cam.position.z);
            self.cam.position.y = (ground + 45.0).max(70.0);
        }
        let camera = self.cam.camera(w / h);
        let sun = Vec3::new(0.42, 0.70, -0.58).normalize();

        let mut cb = TerrainCb {
            view_proj: camera.view_proj(),
            camera_pos: Vec4::new(camera.eye.x, camera.eye.y, camera.eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            inv_extent: Vec2::new(1.0 / w, 1.0 / h),
            sky_horizon: Vec4::new(0.62, 0.72, 0.86, clipmap_range() * 0.85),
            ..Default::default()
        };
        gpu.write_frame_bytes(cb.as_bytes())?;

        // O clear é radiância do céu, não uma cor: o alvo é linear.
        gpu.begin_color_pass(&[color], Some(depth), &[[0.62, 0.72, 0.86, 1.0]], Some(1.0))?;
        gpu.set_pipeline(pso)?;
        gpu.bind_graphics_bindless()?;
        // Um draw por nível. Sem buffers: o VS deriva tudo dos índices.
        gpu.draw(clipmap_vertex_count(), CLIPMAP_LEVELS, 0, 0)?;
        gpu.end_color_pass()?;
        gpu.mark("terrain");

        cb.scene = gpu.bindless_index(color)?;
        gpu.write_frame_bytes(cb.as_bytes())?;
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
    config.title = "Harpia — terrain".into();
    let interactive = config.max_frames.is_none();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    if !interactive {
        config.resize_at = vec![(6, 800, 600), (12, 1280, 720)];
    }
    run(config, TerrainGate::default())
}
