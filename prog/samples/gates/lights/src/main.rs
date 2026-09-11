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
use harpia_app::{AppConfig, Sample, run};
use harpia_math::{Mat4, Vec3, Vec4, perspective_vk};
use harpia_render::{
    CLUSTER_X, CLUSTER_Y, CLUSTER_Z, ConeStats, GBUFFER_DEPTH_FORMAT, INSTANCE_STRIDE, MaterialGpu,
    PointLight, PushConstants, ShadowAtlas, ShadowRequest, SphereInstance, SphereMesh,
    VERTEX_STRIDE, assign_lights_counted, color_desc, depth_desc, shadow_atlas_desc,
    shadow_priority, shadow_wanted_size,
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
const SLOT_SHADOWS: u32 = 3;

/// Lado do atlas de sombras.
const SHADOW_ATLAS: u32 = 2048;
/// Orçamento por frame, em texels. 512² = um tile grande, ou dezasseis de 128².
///
/// Escolhido para ser **pequeno de propósito**: o gate tem de mostrar a fila a
/// trabalhar, e com orçamento à larga o escalonador nunca adia nada e não há
/// nada para provar.
const SHADOW_BUDGET: u64 = 512 * 512;

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
    /// x = índice bindless do atlas, y = 1 se as sombras estão ligadas
    shadow: Vec4,
}

/// O que o shader precisa de saber por luz para amostrar a sombra dela.
#[repr(C)]
#[derive(Clone, Copy)]
struct ShadowEntry {
    view_proj: Mat4,
    /// x,y canto do tile no atlas; z,w lado — tudo normalizado. `z == 0` = sem sombra.
    uv_rect: Vec4,
    /// x = um texel do tile, em unidades do atlas.
    params: Vec4,
}

impl Default for ShadowEntry {
    fn default() -> Self {
        Self {
            view_proj: Mat4::IDENTITY,
            uv_rect: Vec4::ZERO,
            params: Vec4::ZERO,
        }
    }
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

#[derive(Clone, Copy)]
struct Targets {
    clustered: Texture,
    brute: Texture,
    depth: Texture,
}

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
    cone: ConeStats,
    shadow_pso: Option<GraphicsPipeline>,
    shadow_atlas: Option<Texture>,
    shadow_buf: Option<Buffer>,
    atlas: Option<ShadowAtlas>,
    entries: Vec<ShadowEntry>,
    /// `-- --no-shadows` desliga, para o A/B.
    shadows: bool,
    /// Orçamento por frame em texels; `-- --budget N` muda-o, `0` = sem tecto.
    budget: u64,
    /// Somatórios do que a fila fez, para o gate os publicar no fim.
    planned: u64,
    deferred: u64,
    frames: u64,
    /// `-- --lights N`. Menos luzes torna uma sombra individual visível, que é o
    /// que o A/B de sombras precisa: com mil luzes a média esconde qualquer uma.
    light_count: u32,
}

impl Default for LightsGate {
    /// Escrito à mão e não derivado por causa de dois campos: as sombras estão
    /// **ligadas** por omissão e o orçamento não é zero. Um `derive` dava
    /// `shadows: false`, e um gate que por omissão não testa o que diz testar é
    /// pior do que não existir.
    fn default() -> Self {
        Self {
            pso: None,
            vb: None,
            ib: None,
            inst: None,
            index_count: 0,
            instance_count: 0,
            light_buf: None,
            range_buf: None,
            index_buf: None,
            lights: Vec::new(),
            rt: None,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
            compared: false,
            cone: ConeStats::default(),
            shadow_pso: None,
            shadow_atlas: None,
            shadow_buf: None,
            atlas: None,
            entries: Vec::new(),
            shadows: true,
            budget: SHADOW_BUDGET,
            planned: 0,
            deferred: 0,
            frames: 0,
            light_count: LIGHTS as u32,
        }
    }
}

/// Luzes espalhadas sobre a cena, de cores variadas e raio pequeno.
///
/// Raio pequeno de propósito: se cada luz alcançasse a cena toda, todos os
/// clusters teriam todas as luzes e o teste não testava nada.
fn make_lights(n: u32) -> Vec<PointLight> {
    (0..n)
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
            // Uma em cada três é um projector. Misturadas de propósito: o gate
            // tem de provar que os dois tipos convivem na mesma lista e no mesmo
            // percurso, não que cada um funciona sozinho.
            if i % 3 == 0 {
                // **Acima** das esferas, a olhar para baixo. Postos à altura delas
                // — que era onde estavam — o cone passava-lhes ao lado e as
                // sombras mudavam 1.5% dos pixels: o gate tinha o sistema todo a
                // funcionar e quase nada para mostrar. Um projector que não vê
                // nada para tapar não testa uma shadow map.
                let pos = Vec3::new(pos.x, 7.0 + (f * 2.1).sin().abs() * 3.0, pos.z);
                let dir = Vec3::new((f * 0.9).cos() * 0.35, -1.0, (f * 1.3).sin() * 0.35);
                // Raio maior que as omni: um cone estreito com raio pequeno mal
                // toca em clusters nenhuns, e então não há nada para verificar.
                PointLight::spot(pos, dir, 16.0, 0.22, 0.38, color, 26.0)
            } else {
                PointLight::point(pos, 4.5, color, 6.0)
            }
        })
        .collect()
}

fn spheres() -> Vec<SphereInstance> {
    let mut out = Vec::new();
    // Chão. Uma esfera enorme por baixo, porque o gate já sabe desenhar esferas e
    // não vale a pena um segundo pipeline para um plano.
    //
    // Existe por uma razão concreta: sem superfície onde o cone pouse, um
    // projector e uma luz pontual dão a mesma imagem, e um gate cuja imagem não
    // distingue o que testa é mais fraco do que parece. O número (0 ULP contra
    // força-bruta) continuava a provar a correcção; o chão é para se **ver** o
    // que está a ser provado. E de caminho é uma superfície rasante que cobre
    // muitos clusters, o que torna o teste mais exigente.
    let mut floor = MaterialGpu::default();
    floor.base_color = [0.34, 0.33, 0.32];
    floor.roughness = 0.55;
    floor.metallic = 0.0;
    const FLOOR_R: f32 = 900.0;
    out.push(SphereInstance::from_material(
        [0.0, -FLOOR_R, -30.0],
        FLOOR_R,
        &floor,
    ));
    for row in 0..9 {
        for col in 0..21 {
            let mut m = MaterialGpu::default();
            m.base_color = [0.62, 0.60, 0.58];
            m.roughness = 0.25 + row as f32 * 0.08;
            m.metallic = if col % 3 == 0 { 1.0 } else { 0.0 };
            out.push(SphereInstance::from_material(
                [-30.0 + col as f32 * 3.0, 0.9, -8.0 - row as f32 * 6.0],
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
        self.extent = Extent2D {
            width: w,
            height: h,
        };
        Ok(())
    }
}

impl LightsGate {
    /// Escolhe as sombras deste frame, desenha as escolhidas, e diz ao shader
    /// onde cada uma ficou.
    ///
    /// A pass abre com `None` no depth clear — **carrega** o atlas do frame
    /// anterior — e só os tiles do plano é que levam limpeza. É essa a diferença
    /// entre «metade das sombras tem um frame de atraso» e «metade das luzes não
    /// tem sombra».
    fn shadows(&mut self, gpu: &mut Gpu, view: Mat4) -> Result<()> {
        let atlas_tex = self.shadow_atlas.context("atlas")?;
        let shadow_buf = self.shadow_buf.context("shadow buf")?;

        // A tabela limpa-se toda: uma luz que perdeu o tile não pode continuar a
        // apontar para conteúdo que agora é de outra.
        for e in &mut self.entries {
            *e = ShadowEntry::default();
        }
        if !self.shadows {
            gpu.write_storage_buffer(shadow_buf, shadow_entry_bytes(&self.entries))?;
            return Ok(());
        }

        // Só os projectores pedem. Uma omni precisaria de seis faces, e isso é
        // outro trabalho — está escrito no roadmap e não aqui.
        let mut reqs: Vec<ShadowRequest> = Vec::new();
        for (i, lt) in self.lights.iter().enumerate() {
            if !lt.is_spot() {
                continue;
            }
            let p = view.transform_point3(lt.position());
            let Some(prio) = shadow_priority([p.x, p.y, p.z], lt.radius(), FAR) else {
                continue;
            };
            reqs.push(ShadowRequest {
                light: i as u32,
                priority: prio,
                wanted: shadow_wanted_size(prio),
                // A cena está parada, portanto a versão é constante — e é isso que
                // faz a cache convergir e o gate poder comparar com/sem orçamento.
                version: 1,
            });
        }

        let plan = {
            let atlas = self.atlas.as_mut().context("scheduler")?;
            atlas.plan(&reqs)
        };
        self.planned += plan.render.len() as u64;
        self.deferred += plan.deferred as u64;
        self.frames += 1;

        // Desenhar os tiles escolhidos.
        if !plan.render.is_empty() {
            let pso = self.shadow_pso.as_ref().context("shadow pso")?.clone();
            gpu.begin_color_pass(&[], Some(atlas_tex), &[], None)?;
            gpu.set_pipeline(&pso)?;
            gpu.bind_graphics_bindless()?;
            gpu.bind_vertex_buffer(self.vb.context("vb")?, 0)?;
            gpu.bind_vertex_buffer(self.inst.context("inst")?, 1)?;
            gpu.bind_index_buffer(self.ib.context("ib")?)?;
            let slots: Vec<_> = plan
                .render
                .iter()
                .map(|&i| self.atlas.as_ref().unwrap().slots()[i])
                .collect();
            for slot in &slots {
                let [x, y, side] = slot.rect;
                gpu.clear_depth_rect(x, y, side, side, 1.0)?;
                gpu.set_viewport(x as f32, y as f32, side as f32, side as f32)?;
                let vp = spot_view_proj(&self.lights[slot.light as usize]);
                gpu.set_push_constants(PushConstants::new(vp).as_bytes())?;
                gpu.draw_indexed(self.index_count, self.instance_count, 0, 0, 0)?;
            }
            gpu.end_color_pass()?;
        }
        gpu.mark("shadows");

        // A tabela: só os slots com conteúdo desenhado entram.
        let atlas = self.atlas.as_ref().context("scheduler")?;
        let inv = 1.0 / SHADOW_ATLAS as f32;
        for slot in atlas.slots() {
            if slot.drawn.is_none() || slot.light as usize >= self.entries.len() {
                continue;
            }
            let side = slot.rect[2] as f32;
            self.entries[slot.light as usize] = ShadowEntry {
                view_proj: spot_view_proj(&self.lights[slot.light as usize]),
                uv_rect: Vec4::new(
                    slot.rect[0] as f32 * inv,
                    slot.rect[1] as f32 * inv,
                    side * inv,
                    side * inv,
                ),
                params: Vec4::new(inv, 0.0, 0.0, 0.0),
            };
        }
        gpu.write_storage_buffer(shadow_buf, shadow_entry_bytes(&self.entries))?;
        Ok(())
    }
}

impl LightsGate {
    /// O atlas tem geometria lá dentro, e a fila respeitou o orçamento?
    ///
    /// A comparação clustered-contra-força-bruta **não** apanha uma sombra
    /// partida: as duas correm o mesmo código de amostragem, portanto concordam
    /// mesmo que ele esteja errado. O que se verifica aqui é o outro lado — que a
    /// pass de sombras chegou a desenhar alguma coisa.
    ///
    /// Um tile cuja profundidade é constante é um tile onde nada foi desenhado:
    /// ou ficou só com a limpeza a 1.0, ou apanhou um plano de frente. Um tile com
    /// geometria a sério tem variação.
    fn check_shadows(&mut self, gpu: &mut Gpu) -> Result<()> {
        let atlas_tex = self.shadow_atlas.context("atlas")?;
        let data = gpu.read_texture(atlas_tex)?;
        let row = data.bytes.len() / SHADOW_ATLAS as usize;
        let at = |x: u32, y: u32| -> f32 {
            let i = y as usize * row + x as usize * 4;
            if i + 4 > data.bytes.len() {
                return 0.0;
            }
            f32::from_ne_bytes([
                data.bytes[i],
                data.bytes[i + 1],
                data.bytes[i + 2],
                data.bytes[i + 3],
            ])
        };

        let slots: Vec<_> = self
            .atlas
            .as_ref()
            .context("scheduler")?
            .slots()
            .iter()
            .filter(|s| s.drawn.is_some())
            .copied()
            .collect();
        let mut with_geometry = 0;
        for slot in &slots {
            let [x, y, side] = slot.rect;
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            // Amostragem esparsa: 32x32 por tile chega para ver se há variação, e
            // não custa ler o atlas inteiro em f32.
            let step = (side / 32).max(1);
            for j in (0..side).step_by(step as usize) {
                for i in (0..side).step_by(step as usize) {
                    let d = at(x + i, y + j);
                    lo = lo.min(d);
                    hi = hi.max(d);
                }
            }
            if hi - lo > 1e-4 {
                with_geometry += 1;
            }
        }

        let atlas = self.atlas.as_ref().context("scheduler")?;
        tracing::info!(
            tiles_com_conteudo = slots.len(),
            tiles_com_geometria = with_geometry,
            pior_espera_frames = atlas.worst_stale(),
            "atlas de sombras"
        );
        anyhow::ensure!(
            !slots.is_empty(),
            "a fila não deu tile a ninguém: nenhum projector tem sombra"
        );
        anyhow::ensure!(
            with_geometry * 2 >= slots.len(),
            "só {with_geometry} de {} tiles têm variação de profundidade; os outros \
             ficaram com a limpeza. A pass de sombras não está a desenhar.",
            slots.len()
        );
        Ok(())
    }
}

/// A matriz do ponto de vista de um projector.
///
/// O FOV é o **ângulo inteiro** do cone, que é o dobro do meio-ângulo guardado na
/// luz — e um pouco mais, para a borda do cone não cair exactamente no bordo do
/// mapa, onde a filtragem já não tem vizinhos.
fn spot_view_proj(light: &PointLight) -> Mat4 {
    let pos = light.position();
    let dir = light.direction();
    let up = if dir.y.abs() > 0.99 { Vec3::X } else { Vec3::Y };
    let view = Mat4::look_at_rh(pos, pos + dir, up);
    let half = light.cos_outer().clamp(-0.999, 0.999).acos();
    let fov = (half * 2.0 * 1.1).min(std::f32::consts::PI * 0.98);
    // O near não pode ser minúsculo: a precisão do depth buffer é logarítmica e
    // um near de 0.01 num alcance de 9 gasta metade dos bits no primeiro palmo.
    perspective_vk(fov, 1.0, (light.radius() * 0.02).max(0.05), light.radius()) * view
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

        self.lights = make_lights(self.light_count);
        self.light_buf = Some(gpu.create_storage_buffer(SLOT_LIGHTS, light_bytes(&self.lights))?);
        // Os dois seguintes ficam do tamanho máximo desde o início: o descriptor
        // aponta para a alocação, e um buffer que cresce a meio invalidava-o.
        let ranges = vec![0u32; (CLUSTER_X * CLUSTER_Y * CLUSTER_Z * 2) as usize];
        self.range_buf = Some(gpu.create_storage_buffer(SLOT_RANGES, as_bytes(&ranges))?);
        let indices = vec![0u32; harpia_render::MAX_LIGHT_INDICES];
        self.index_buf = Some(gpu.create_storage_buffer(SLOT_INDICES, as_bytes(&indices))?);

        // Sombras: o PSO de profundidade, o atlas, a tabela por luz, e a fila.
        self.shadow_pso = Some(
            gpu.create_graphics_pipeline(&GraphicsPipelineDesc {
                vs_spirv: include_bytes!(concat!(env!("OUT_DIR"), "/shadow.vs.spv")),
                // Sem fragment shader: só interessa a profundidade.
                fs_spirv: &[],
                vs_entry: "VSMain",
                fs_entry: "PSMain",
                bindless: true,
                targets: PipelineTargets {
                    color_formats: &[],
                    depth_format: Some(GBUFFER_DEPTH_FORMAT),
                    vertex_stride: VERTEX_STRIDE,
                    instance_stride: INSTANCE_STRIDE,
                    depth_test: true,
                    // Sem `depth_only` um `color_formats` vazio **não** dá zero
                    // alvos: o RHI cai para o formato da swapchain, e a validation
                    // reclama que o pipeline tem 1 e a pass tem 0.
                    depth_only: true,
                    // Não se corta nada: geometria de uma face só não projectaria
                    // sombra nenhuma. A acne trata-se com `depth_bias`.
                    cull_back: false,
                    depth_bias: true,
                },
            })
            .context("shadow PSO")?,
        );
        self.shadow_atlas = Some(gpu.create_texture(&shadow_atlas_desc(SHADOW_ATLAS))?);
        self.entries = vec![ShadowEntry::default(); self.light_count as usize];
        self.shadow_buf =
            Some(gpu.create_storage_buffer(SLOT_SHADOWS, shadow_entry_bytes(&self.entries))?);
        self.atlas = Some(ShadowAtlas::new(
            SHADOW_ATLAS,
            if self.budget == 0 {
                u64::MAX
            } else {
                self.budget
            },
        ));

        self.recreate(gpu, gpu.extent())?;
        tracing::info!(
            luzes = self.light_count,
            esferas = self.instance_count,
            clusters = CLUSTER_X * CLUSTER_Y * CLUSTER_Z,
            "lights"
        );
        Ok(())
    }

    fn capture_targets(&self) -> Vec<(&'static str, Texture)> {
        let mut out = self.rt.as_ref().map_or_else(Vec::new, |rt| {
            vec![("clustered", rt.clustered), ("brute", rt.brute)]
        });
        if let Some(a) = self.shadow_atlas {
            out.push(("shadow-atlas", a));
        }
        out
    }

    fn frame(&mut self, gpu: &mut Gpu, info: FrameInfo) -> Result<()> {
        if info.extent.width != self.extent.width || info.extent.height != self.extent.height {
            self.recreate(gpu, info.extent)?;
        }
        // Copiados, não emprestados: a pass de sombras precisa de `&mut self` e
        // um empréstimo de `self.rt` a atravessá-la bloqueia-a.
        let rt = *self.rt.as_ref().context("rt")?;
        let pso = self.pso.as_ref().context("pso")?.clone();

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
        let (assignment, cone) =
            assign_lights_counted(&self.lights, view, NEAR, FAR, tan_half, w / h);
        self.cone = cone;
        if assignment.dropped > 0 {
            tracing::warn!(perdidas = assignment.dropped, "lista de índices cheia");
        }
        let flat: Vec<u32> = assignment
            .ranges
            .iter()
            .flat_map(|r| [r.offset, r.count])
            .collect();
        gpu.write_storage_buffer(self.range_buf.context("ranges")?, as_bytes(&flat))?;
        gpu.write_storage_buffer(
            self.index_buf.context("indices")?,
            as_bytes(&assignment.indices),
        )?;

        self.shadows(gpu, view)?;

        let base = LitCb {
            inv_view_proj: view_proj.inverse(),
            view,
            camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
            sun_dir: Vec4::new(sun.x, sun.y, sun.z, 0.0),
            sun_color: Vec4::new(1.4, 1.35, 1.2, 1.0),
            params: Vec4::new(NEAR, FAR, self.light_count as f32, 0.0),
            screen: Vec4::new(w, h, 1.0 / w, 1.0 / h),
            cluster_dims: Vec4::new(CLUSTER_X as f32, CLUSTER_Y as f32, CLUSTER_Z as f32, 0.0),
            shadow: Vec4::new(
                gpu.bindless_index(self.shadow_atlas.context("atlas")?)? as f32,
                if self.shadows { 1.0 } else { 0.0 },
                0.0,
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
            gpu.set_pipeline(&pso)?;
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
            luzes = self.light_count,
            canais = total,
            spots = self.lights.iter().filter(|l| l.is_spot()).count(),
            slots_pela_esfera = self.cone.sphere_slots,
            slots_depois_do_cone = self.cone.cone_slots,
            cortado_pelo_cone = format!("{:.1}", self.cone.saved() * 100.0),
            sombras = self.shadows,
            orcamento_texels = self.budget,
            tiles_por_frame = format!("{:.2}", self.planned as f64 / self.frames.max(1) as f64),
            adiados_por_frame = format!("{:.2}", self.deferred as f64 / self.frames.max(1) as f64),
            com_sombra = self.entries.iter().filter(|e| e.uv_rect.z > 0.0).count(),
            pior_espera_frames = self.atlas.as_ref().map_or(0, |a| a.worst_stale()),
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

        if self.shadows {
            self.check_shadows(gpu)?;
        }
        Ok(())
    }
}

fn as_bytes<T>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

fn shadow_entry_bytes(v: &[ShadowEntry]) -> &[u8] {
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
    let user_set = std::env::args().any(|a| a == "--frames" || a == "--interactive" || a == "-i");
    if !user_set {
        config.max_frames = std::num::NonZeroU32::new(16);
    }
    let shadows = !config.extra.iter().any(|a| a == "--no-shadows");
    let light_count = config
        .extra
        .iter()
        .position(|a| a == "--lights")
        .and_then(|i| config.extra.get(i + 1))
        .map(|v| v.parse::<u32>())
        .transpose()
        .context("`--lights` quer um número")?
        .unwrap_or(LIGHTS as u32)
        .clamp(1, LIGHTS as u32);
    let budget = config
        .extra
        .iter()
        .position(|a| a == "--budget")
        .and_then(|i| config.extra.get(i + 1))
        .map(|v| v.parse::<u64>())
        .transpose()
        .context("`--budget` quer um número de texels (0 = sem tecto)")?
        .unwrap_or(SHADOW_BUDGET);
    run(
        config,
        LightsGate {
            shadows,
            budget,
            light_count,
            ..Default::default()
        },
    )
}
