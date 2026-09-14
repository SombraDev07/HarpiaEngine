//! Wave P: authoring crate + viewport RT in `present_format()`, blit PSO at load.
//!
//! Not E0: no docking, no Sponza. Overlay of `storm -- --interactive` is not this.

use anyhow::{Context, Result};
use harpia_app::{AppConfig, Sample, run};
use harpia_editor::AuthoringScene;
use harpia_render::color_desc;
use harpia_rhi::{
    Device, Extent2D, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc, PipelineTargets,
    Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

#[repr(C)]
#[derive(Clone, Copy)]
struct BlitCb {
    inv_extent: [f32; 2],
    src: u32,
    _pad: u32,
}

fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((v as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}

struct EditorSample {
    blit_pso: Option<GraphicsPipeline>,
    viewport: Option<Texture>,
    extent: Extent2D,
    /// Loaded at init so the sample actually *uses* authoring, not just links it.
    scene: AuthoringScene,
}

impl Default for EditorSample {
    fn default() -> Self {
        let mut cube = harpia_editor::AuthoringNode::named("ViewportCube");
        cube.mesh = Some("meshes/cube.gltf".into());
        cube.runtime = harpia_editor::RuntimePath::Gpu;
        Self {
            blit_pso: None,
            viewport: None,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
            scene: AuthoringScene { nodes: vec![cube] },
        }
    }
}

impl EditorSample {
    fn recreate_viewport(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        let format = gpu.present_format();
        self.viewport = Some(gpu.create_texture(&color_desc(w, h, format))?);
        self.extent = Extent2D {
            width: w,
            height: h,
        };
        Ok(())
    }
}

impl Sample for EditorSample {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        // PSO at load. Recreating this on a click is a hitch (Swarm S1).
        self.blit_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/fullscreen.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/blit.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets::default(),
            })
            .context("editor blit PSO")?,
        );
        let _ = self.scene.to_ron().context("authoring ron")?;
        Ok(())
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if self.extent != info.extent {
            self.recreate_viewport(gpu, info.extent)?;
        }
        let viewport = self.viewport.context("viewport")?;
        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;

        gpu.begin_color_pass(&[viewport], None, &[[0.08, 0.10, 0.18, 1.0]], None)?;
        gpu.end_color_pass()?;

        gpu.write_frame_bytes(as_bytes(&BlitCb {
            inv_extent: [1.0 / w, 1.0 / h],
            src: gpu.bindless_index(viewport)?,
            _pad: 0,
        }))?;
        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0])?;
        gpu.set_pipeline(self.blit_pso.as_ref().context("blit")?)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — editor".into();
    let user_set_frames =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set_frames {
        config.max_frames = std::num::NonZeroU32::new(8);
        config.resize_at = vec![(3, 800, 600), (6, 1280, 720)];
    }
    run(config, EditorSample::default())
}
