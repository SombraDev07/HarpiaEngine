use anyhow::{Context, Result};
use harpia_app::{run, AppConfig, Sample};
use harpia_math::{perspective_vk, Mat4, Vec3, Vec4};
use harpia_render::{
    color_desc, depth_desc, generate_ibl, LightingCb, MaterialGpu, PushConstants, SphereInstance,
    SphereMesh, GBUFFER_COLOR_FORMATS, GBUFFER_DEPTH_FORMAT, INSTANCE_STRIDE, VERTEX_STRIDE,
};
use harpia_rhi::{
    Buffer, Device, Extent2D, Format, FrameInfo, Gpu, GraphicsPipeline, GraphicsPipelineDesc,
    PipelineTargets, Texture,
};

#[global_allocator]
static ALLOC: harpia_memory::Allocator = harpia_memory::Allocator::new();

struct GBuffer {
    albedo: Texture,
    normal: Texture,
    orm: Texture,
    emissive: Texture,
    depth_color: Texture,
    depth: Texture,
}

struct PbrGrid {
    gbuf_pso: Option<GraphicsPipeline>,
    light_pso: Option<GraphicsPipeline>,
    vb: Option<Buffer>,
    ib: Option<Buffer>,
    inst: Option<Buffer>,
    index_count: u32,
    instance_count: u32,
    gbuffer: Option<GBuffer>,
    gbuf_extent: Extent2D,
    irr: Option<Texture>,
    pre: Option<Texture>,
    brdf: Option<Texture>,
    ibl_scale: f32,
    ibl_max_mip: f32,
}

impl Default for PbrGrid {
    fn default() -> Self {
        Self {
            gbuf_pso: None,
            light_pso: None,
            vb: None,
            ib: None,
            inst: None,
            index_count: 0,
            instance_count: 0,
            gbuffer: None,
            gbuf_extent: Extent2D {
                width: 0,
                height: 0,
            },
            irr: None,
            pre: None,
            brdf: None,
            ibl_scale: 1.0,
            ibl_max_mip: 0.0,
        }
    }
}

impl PbrGrid {
    fn recreate_gbuffer(&mut self, gpu: &mut Gpu, extent: Extent2D) -> Result<()> {
        let w = extent.width.max(1);
        let h = extent.height.max(1);
        self.gbuffer = Some(GBuffer {
            albedo: gpu.create_texture(&color_desc(w, h, Format::Rgba8Srgb))?,
            normal: gpu.create_texture(&color_desc(w, h, Format::Rgba8Unorm))?,
            orm: gpu.create_texture(&color_desc(w, h, Format::Rgba8Unorm))?,
            emissive: gpu.create_texture(&color_desc(w, h, Format::Rgba8Unorm))?,
            depth_color: gpu.create_texture(&color_desc(w, h, Format::R32Float))?,
            depth: gpu.create_texture(&depth_desc(w, h))?,
        });
        self.gbuf_extent = Extent2D { width: w, height: h };
        Ok(())
    }

    fn upload_mips(gpu: &mut Gpu, img: &harpia_render::RgbaImage) -> Result<Texture> {
        let tex = gpu.create_texture(&img.desc())?;
        for (mip, bytes) in img.mips.iter().enumerate() {
            gpu.upload_texture_mip(tex, mip as u32, bytes)?;
        }
        Ok(tex)
    }
}

impl Sample for PbrGrid {
    fn init(&mut self, gpu: &mut Gpu) -> Result<()> {
        self.gbuf_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/gbuffer.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/gbuffer.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &GBUFFER_COLOR_FORMATS,
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE,
                    instance_stride: INSTANCE_STRIDE,
                    depth_test: true,
                    cull_back: true,
                    ..Default::default()
                },
            })
            .context("gbuffer PSO")?,
        );
        self.light_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/lighting.vs.spv")),
                fs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/lighting.ps.spv")),
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets::default(),
            })
            .context("lighting PSO")?,
        );

        let mesh = SphereMesh::uv(24, 16);
        self.index_count = mesh.indices.len() as u32;
        self.vb = Some(gpu.create_vertex_buffer(mesh.vertex_bytes())?);
        self.ib = Some(gpu.create_index_buffer(mesh.index_bytes())?);

        let instances = grid_instances();
        self.instance_count = instances.len() as u32;
        self.inst = Some(gpu.create_vertex_buffer(instance_bytes(&instances))?);

        let ibl = generate_ibl();
        self.ibl_scale = ibl.ibl_scale;
        self.ibl_max_mip = (ibl.prefiltered.mip_levels.saturating_sub(1)) as f32;
        self.irr = Some(Self::upload_mips(gpu, &ibl.irradiance)?);
        self.pre = Some(Self::upload_mips(gpu, &ibl.prefiltered)?);
        self.brdf = Some(Self::upload_mips(gpu, &ibl.brdf_lut)?);

        self.recreate_gbuffer(gpu, gpu.extent())?;
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        match self.gbuffer.as_ref() {
            Some(g) => vec![("albedo", g.albedo), ("normal", g.normal), ("orm", g.orm)],
            None => Vec::new(),
        }
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.gbuf_extent.width
            || info.extent.height != self.gbuf_extent.height
        {
            self.recreate_gbuffer(gpu, info.extent)?;
        }
        let gbuf = self.gbuffer.as_ref().context("gbuffer")?;
        let gbuf_pso = self.gbuf_pso.as_ref().context("gbuf pso")?;
        let light_pso = self.light_pso.as_ref().context("light pso")?;
        let vb = self.vb.context("vb")?;
        let ib = self.ib.context("ib")?;
        let inst = self.inst.context("inst")?;

        let w = info.extent.width.max(1) as f32;
        let h = info.extent.height.max(1) as f32;
        let eye = Vec3::new(0.0, 0.55, 8.6);
        let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
        let proj = perspective_vk(60.0_f32.to_radians(), w / h, 0.1, 80.0);
        let view_proj = proj * view;
        let inv_view_proj = view_proj.inverse();
        let sun = Vec3::new(0.38, 0.84, 0.38).normalize();

        let cb = LightingCb {
            inv_view_proj,
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            sun_color: Vec4::new(4.2, 3.8, 3.3, 1.0),
            gbuf0: gpu.bindless_index(gbuf.albedo)?,
            gbuf1: gpu.bindless_index(gbuf.normal)?,
            gbuf2: gpu.bindless_index(gbuf.orm)?,
            gbuf3: gpu.bindless_index(gbuf.emissive)?,
            gbuf4: gpu.bindless_index(gbuf.depth_color)?,
            irradiance: gpu.bindless_index(self.irr.context("irr")?)?,
            prefiltered: gpu.bindless_index(self.pre.context("pre")?)?,
            brdf_lut: gpu.bindless_index(self.brdf.context("brdf")?)?,
            ibl_scale: self.ibl_scale,
            ibl_max_mip: self.ibl_max_mip,
            exposure: 0.85,
            alpha_cutoff: 0.0,
            inv_extent: harpia_math::Vec2::new(1.0 / w, 1.0 / h),
            view,
            ..Default::default()
        };
        gpu.write_frame_bytes(cb.as_bytes())?;

        gpu.begin_color_pass(
            &[
                gbuf.albedo,
                gbuf.normal,
                gbuf.orm,
                gbuf.emissive,
                gbuf.depth_color,
            ],
            Some(gbuf.depth),
            &[
                [0.0, 0.0, 0.0, 1.0],
                [0.5, 0.5, 1.0, 0.0],
                [1.0, 1.0, 0.0, 0.04],
                [0.0, 0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
            ],
            Some(1.0),
        )?;
        gpu.set_pipeline(gbuf_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.set_push_constants(PushConstants::new(view_proj).as_bytes())?;
        gpu.bind_vertex_buffer(vb, 0)?;
        gpu.bind_vertex_buffer(inst, 1)?;
        gpu.bind_index_buffer(ib)?;
        gpu.draw_indexed(self.index_count, self.instance_count, 0, 0, 0)?;
        gpu.end_color_pass()?;

        gpu.begin_swapchain_pass([0.02, 0.03, 0.05, 1.0])?;
        gpu.set_pipeline(light_pso)?;
        gpu.bind_graphics_bindless()?;
        gpu.draw(3, 1, 0, 0)?;
        gpu.end_swapchain_pass()?;
        Ok(())
    }
}

fn grid_instances() -> Vec<SphereInstance> {
    let mut out = Vec::new();
    let radius = 0.52;
    for row in 0..3 {
        for col in 0..7 {
            let x = (col as f32 - 3.0) * 1.38;
            let y = (1 - row as i32) as f32 * 1.38;
            let mut mat = MaterialGpu::default();
            match row {
                0 => {
                    mat.base_color = [0.91, 0.91, 0.91];
                    mat.metallic = 0.0;
                    mat.roughness = col as f32 / 6.0;
                }
                1 => {
                    mat.base_color = [1.0, 0.76, 0.33];
                    mat.metallic = 1.0;
                    mat.roughness = col as f32 / 6.0;
                }
                _ => match col {
                    0 | 1 => {
                        mat.base_color = [0.12, 0.12, 0.14];
                        mat.roughness = 0.25;
                        mat.clearcoat = 1.0;
                        mat.clearcoat_roughness = if col == 0 { 0.05 } else { 0.45 };
                    }
                    2 | 3 => {
                        mat.base_color = [0.16, 0.12, 0.12];
                        mat.roughness = 0.55;
                        mat.fuzz = 0.9;
                        mat.fuzz_color = [0.86, 0.14, 0.18];
                    }
                    4 | 5 => {
                        mat.base_color = [0.08, 0.08, 0.08];
                        mat.roughness = 0.4;
                        mat.emissive = [1.0, 0.45, 0.08];
                    }
                    _ => {
                        mat.base_color = [0.2, 0.22, 0.28];
                        mat.roughness = 0.35;
                        mat.clearcoat = 0.5;
                        mat.fuzz = 0.45;
                        mat.fuzz_color = [0.2, 0.45, 0.95];
                    }
                },
            }
            out.push(SphereInstance::from_material([x, y, 0.0], radius, &mat));
        }
    }
    out
}

fn instance_bytes(instances: &[SphereInstance]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            instances.as_ptr().cast::<u8>(),
            std::mem::size_of_val(instances),
        )
    }
}

fn main() -> Result<std::process::ExitCode> {
    let mut config = AppConfig::parse(std::env::args())?;
    config.title = "Harpia — pbr-grid".into();
    run(config, PbrGrid::default())
}
