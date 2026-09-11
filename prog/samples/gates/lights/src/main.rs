//! Gate `lights`: luzes pontuais em clusters, e a prova de que a grelha acerta.
//!
//! Duas coisas, e a segunda é a que interessa:
//!
//! 1. **Escala.** Mil luzes pontuais numa cena, medidas com `--stats` contra a
//!    mesma cena a percorrer todas as luzes por pixel.
//! 2. **Correcção.** A mesma imagem desenhada das duas maneiras e comparada
//!    pixel a pixel. Se a grelha atribuir uma luz ao cluster errado, a imagem
//!    clustered fica diferente da força-bruta e o gate falha.
//!
//! O ponto 2 é o que a Dagor não tem (`docs/Harpia-vs-Dagor.md` §6): eles têm o
//! sistema, ninguém verifica a atribuição. Um erro de clustering aparece como um
//! candeeiro que não ilumina a parede ao lado — fácil de não reparar, difícil de
//! atribuir à causa.

use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{perspective_vk, Mat4, Vec3, Vec4};
use harpia_render::{
    assign_lights, color_desc, depth_desc, MaterialGpu, PointLight, PushConstants, SphereInstance,
    SphereMesh, CLUSTER_X, CLUSTER_Y, CLUSTER_Z, GBUFFER_DEPTH_FORMAT, INSTANCE_STRIDE,
    VERTEX_STRIDE,
};
use harpia_rhi::{
    Buffer, Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

const LIGHTS: usize = 1000;
const HDR: Format = Format::Rgba16Float;
const HDR_FORMATS: [Format; 1] = [HDR];
const FOV_Y: f32 = 60.0;
const NEAR: f32 = 0.5;
const FAR: f32 = 200.0;

/// Slots do set 3.
const SLOT_LIGHTS: u32 = 0;
const SLOT_RANGES: u32 = 1;
const SLOT_INDICES: u32 = 2;

/// Diferença aceitável entre clustered e força-bruta, em passos de 8 bits.
///
/// Não é zero: as duas somam as mesmas contribuições por ordens diferentes e a
/// vírgula flutuante não é associativa. É apertado o suficiente para apanhar uma
/// luz atribuída ao cluster errado, que muda um pixel em dezenas de níveis.
const TOLERANCE: u8 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct LitCb {
    inv_view_proj: Mat4,
    view: Mat4,
    camera_pos: Vec4,
    sun_dir: Vec4,
    sun_color: Vec4,
    /// near, far, nº de luzes, 1 = força-bruta
    params: Vec4,
    /// largura, altura, 1/largura, 1/altura
    screen: Vec4,
    cluster_dims: Vec4,
}

impl LitCb {
    fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                std::mem::size_of::<Self>(),
            )
        }
    }
}

struct Targets {
    clustered: Texture,
    brute: Texture,
    depth: Texture,
}

#[derive(Default)]
struct LightsGate {
    pso: Option<GraphicsPipeline>,
    vb: Option<Buffer>,
    ib: Option<Buffer>,
    inst: Option<Buffer>,
    index_count: u32,
    instance_count: u32,
    light_buf: Option<Buffer>,
    range_buf: Option<Buffer>,
    index_buf: Option<Buffer>,
    lights: Vec<PointLight>,
    rt: Option<Targets>,
    extent: Extent2D,
    compared: bool,
}

/// Luzes espalhadas sobre a cena, de cores variadas e raio pequeno.
///
/// Raio pequeno de propósito: se cada luz alcançasse a cena toda, todos os
/// clusters teriam todas as luzes e o teste não testava nada.
fn make_lights() -> Vec<PointLight> {
    (0..LIGHTS)
        .map(|i| {
            let f = i as f32;
            let pos = Vec3::new(
                (f * 0.7).sin() * 26.0,
                0.8 + (f * 1.7).sin().abs() * 3.0,
                -6.0 - (f * 0.37).fract() * 52.0,
            );
            let color = Vec3::new(
                0.5 + 0.5 * (f * 1.1).sin(),
                0.5 + 0.5 * (f * 2.3).sin(),
                0.5 + 0.5 * (f * 3.7).sin(),
            );
            PointLight::new(pos, 4.5, color, 6.0)
        })
        .collect()
}

fn spheres() -> Vec<SphereInstance> {
    let mut out = Vec::new();
    for row in 0..9 {
        for col in 0..21 {
            let mut m = MaterialGpu::default();
            m.base_color = [0.62, 0.60, 0.58];
            m.roughness = 0.25 + row as f32 * 0.08;
            m.metallic = if col % 3 == 0 { 1.0 } else { 0.0 };
            out.push(SphereInstance::from_material(
                [
                    -30.0 + col as f32 * 3.0,
                    0.9,
                    -8.0 - row as f32 * 6.0,
                ],
                1.1,
                &m,
            ));
        }
    }
    out
}

impl LightsGate {
    fn recreate(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.rt = Some(Targets {
            clustered: gpu.create_texture(&color_desc(w, h, HDR))?,
            brute: gpu.create_texture(&color_desc(w, h, HDR))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
        });
        self.extent = Extent2D { width: w, height: h };
        Ok(())
    }
}

impl Sample for LightsGate {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/lights.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/lights.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &HDR_FORMATS,
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE,
                    instance_stride: INSTANCE_STRIDE,
                    depth_test: true,
                    cull_back: true,
                    ..Default::default()
                },
            })
            .context("lights PSO")?,
        );

        let mesh = SphereMesh::uv(20, 14);
        self.index_count = mesh.indices.len() as u32;
        self.vb = Some(gpu.create_vertex_buffer(mesh.vertex_bytes())?);
        self.ib = Some(gpu.create_index_buffer(mesh.index_bytes())?);
        let inst = spheres();
        self.instance_count = inst.len() as u32;
        self.inst = Some(gpu.create_vertex_buffer(instance_bytes(&inst))?);

        self.lights = make_lights();
        self.light_buf = Some(gpu.create_storage_buffer(SLOT_LIGHTS, light_bytes(&self.lights))?);
        // Os dois seguintes ficam do tamanho máximo desde o início: o descriptor
        // aponta para a alocação, e um buffer que cresce a meio invalidava-o.
        let ranges = vec![0u32; (CLUSTER_X * CLUSTER_Y * CLUSTER_Z * 2) as usize];
        self.range_buf = Some(gpu.create_storage_buffer(SLOT_RANGES, as_bytes(&ranges))?);
        let indices = vec![0u32; harpia_render::MAX_LIGHT_INDICES];
        self.index_buf = Some(gpu.create_storage_buffer(SLOT_INDICES, as_bytes(&indices))?);

        self.recreate(gpu, gpu.extent())?;
        tracing::info!(
            luzes = LIGHTS,
            esferas = self.instance_count,
            clusters = CLUSTER_X * CLUSTER_Y * CLUSTER_Z,
            "lights"
        );
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        self.rt.as_ref().map_or_else(Vec::new, |rt| {
            vec![("clustered", rt.clustered), ("brute", rt.brute)]
        })
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        let rt = self.rt.as_ref().context("rt")?;
        let pso = self.pso.as_ref().context("pso")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let eye = Vec3::new(0.0, 7.0, 14.0);
        let view = Mat4::look_at_rh(eye, Vec3::new(0.0, 1.0, -24.0), Vec3::Y);
        let proj = perspective_vk(FOV_Y.to_radians(), w / h, NEAR, FAR);
        let view_proj = proj * view;
        let sun = Vec3::new(0.3, 0.8, 0.4).normalize();

        // Atribuição em CPU, por agora. Move-se para compute quando os draws
        // indirectos chegarem (fase 6.5) -- e aí este gate prova que a versão em
        // compute continua a concordar com a força-bruta.
        let tan_half = (FOV_Y.to_radians() * 0.5).tan();
        let assignment = assign_lights(&self.lights, view, NEAR, FAR, tan_half, w / h);
        if assignment.dropped > 0 {
            tracing::warn!(perdidas = assignment.dropped, "lista de índices cheia");
        }
        let flat: Vec<u32> = assignment
            .ranges
            .iter()
            .flat_map(|r| [r.offset, r.count])
            .collect();
        gpu.write_storage_buffer(self.range_buf.context("ranges")?, as_bytes(&flat))?;
        gpu.write_storage_buffer(self.index_buf.context("indices")?, as_bytes(&assignment.indices))?;

        let base = LitCb {
            inv_view_proj: view_proj.inverse(),
            view,
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            sun_color: Vec4::new(1.4, 1.35, 1.2, 1.0),
            params: Vec4::new(NEAR, FAR, LIGHTS as f32, 0.0),
            screen: Vec4::new(w, h, 1.0 / w, 1.0 / h),
            cluster_dims: Vec4::new(
                CLUSTER_X as f32,
                CLUSTER_Y as f32,
                CLUSTER_Z as f32,
                0.0,
            ),
        };

        for (target, brute, label) in [
            (rt.clustered, 0.0f32, "clustered"),
            (rt.brute, 1.0f32, "brute force"),
        ] {
            let mut cb = base;
            cb.params.w = brute;
            gpu.write_frame_bytes(cb.as_bytes())?;
            gpu.begin_color_pass(
                &[target],
                Some(rt.depth),
                &[[0.01, 0.012, 0.02, 1.0]],
                Some(1.0),
            )?;
            gpu.set_pipeline(pso)?;
            gpu.bind_graphics_bindless()?;
            gpu.set_push_constants(PushConstants::new(view_proj).as_bytes())?;
            gpu.bind_vertex_buffer(self.vb.context("vb")?, 0)?;
            gpu.bind_vertex_buffer(self.inst.context("inst")?, 1)?;
            gpu.bind_index_buffer(self.ib.context("ib")?)?;
            gpu.draw_indexed(self.index_count, self.instance_count, 0, 0, 0)?;
            gpu.end_color_pass()?;
            gpu.mark(label);
        }

        gpu.begin_swapchain_pass([0.0, 0.0, 0.0, 1.0])?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }

    fn finish(&mut self, gpu: &mut Gpu) -> Result<()> {
        if self.compared {
            return Ok(());
        }
        self.compared = true;
        let rt = self.rt.as_ref().context("rt")?;
        let (a, b) = (rt.clustered, rt.brute);
        let clustered = gpu.read_texture(a)?;
        let brute = gpu.read_texture(b)?;
        anyhow::ensure!(
            clustered.bytes.len() == brute.bytes.len(),
            "as duas capturas têm tamanhos diferentes"
        );

        // Rgba16Float: compara-se em half cru, que chega para detectar uma luz
        // no cluster errado sem precisar de descodificar.
        let mut differing = 0u64;
        let mut worst = 0u16;
        let total = clustered.bytes.len() / 2;
        for i in (0..clustered.bytes.len()).step_by(2) {
            let x = u16::from_ne_bytes([clustered.bytes[i], clustered.bytes[i + 1]]);
            let y = u16::from_ne_bytes([brute.bytes[i], brute.bytes[i + 1]]);
            let d = x.abs_diff(y);
            if d > 0 {
                differing += 1;
            }
            worst = worst.max(d);
        }
        let pct = 100.0 * differing as f64 / total as f64;
        tracing::info!(
            luzes = LIGHTS,
            canais = total,
            canais_diferentes = differing,
            percent = format!("{pct:.3}"),
            maior_diferenca_ulp = worst,
            "clustered contra forca-bruta"
        );
        anyhow::ensure!(
            worst <= TOLERANCE as u16,
            "clustered e força-bruta divergem em {worst} ULP (tolerância {TOLERANCE}). \
             A grelha está a atribuir luzes ao cluster errado."
        );
        Ok(())
    }
}

fn as_bytes<T>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

fn light_bytes(v: &[PointLight]) -> &[u8] {
    as_bytes(v)
}

fn instance_bytes(v: &[SphereInstance]) -> &[u8] {
    as_bytes(v)
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — lights".into();
    let user_set =
        std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    run(config, LightsGate::default())
}
