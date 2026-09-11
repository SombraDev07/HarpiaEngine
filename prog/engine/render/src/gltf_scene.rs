//! Minimal glTF import (Sponza). No `vk::*`.
//!
//! Images are shared: Sponza has 103 primitives over ~25 textures, so a
//! per-primitive copy costs both RAM and bindless slots. Mips are built here
//! (in linear space — the albedo is sRGB-encoded) because sampling a mip that
//! was never uploaded is a GPUVM on RADV.

use std::collections::HashMap;

use gltf::mesh::Mode;
use harpia_math::{Mat4, Vec3};

use crate::mesh::MeshVertex;

#[derive(Debug)]
pub struct GltfError(pub String);

impl std::fmt::Display for GltfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GltfError {}

impl From<gltf::Error> for GltfError {
    fn from(e: gltf::Error) -> Self {
        Self(e.to_string())
    }
}

/// One decoded RGBA8 image, shared by every primitive that references it.
pub struct CpuImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl CpuImage {
    pub fn mip_levels(&self) -> u32 {
        let max = self.width.max(self.height).max(1);
        32 - max.leading_zeros()
    }

    /// Level 0 first. Every level the GPU texture declares must be uploaded.
    pub fn mip_chain(&self) -> Vec<Vec<u8>> {
        let levels = self.mip_levels();
        let mut out = Vec::with_capacity(levels as usize);
        out.push(self.rgba.clone());
        let (mut w, mut h) = (self.width, self.height);
        for _ in 1..levels {
            let prev = out.last().expect("mip 0 pushed above");
            let (next, nw, nh) = downsample_srgb(prev, w, h);
            out.push(next);
            w = nw;
            h = nh;
        }
        out
    }
}

pub struct CpuPrimitive {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    /// Index into [`CpuScene::images`].
    pub albedo: usize,
    /// glTF `MASK`: the pixel shader discards below this. 0 = opaque.
    pub alpha_cutoff: f32,
    /// glTF `doubleSided`. Cutout foliage must not be back-face culled.
    pub double_sided: bool,
    pub world: Mat4,
}

impl CpuPrimitive {
    pub fn vertex_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self.vertices.as_ptr().cast::<u8>(),
                std::mem::size_of_val(self.vertices.as_slice()),
            )
        }
    }

    pub fn index_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self.indices.as_ptr().cast::<u8>(),
                std::mem::size_of_val(self.indices.as_slice()),
            )
        }
    }
}

pub struct CpuScene {
    pub images: Vec<CpuImage>,
    pub prims: Vec<CpuPrimitive>,
}

/// Triângulos por meshlet, quando se usam.
///
/// 128 é o número habitual da literatura — e **medido nesta árvore dá pior**: com
/// um draw indirecto por meshlet, o custo fixo por comando bate o ganho do culling
/// em todos os tamanhos que experimentei. O varrimento está em D58. O valor fica
/// aqui porque é o que um caminho de mesh shaders vai querer, e é por isso que a
/// função continua a existir.
pub const MESHLET_TRIS: usize = 128;

/// Vértices únicos por meshlet, no formato canónico.
///
/// 64 e 124 são os números que cabem em qualquer hardware com mesh shaders: o
/// RX 6700 permite 256 de cada, mas um mesh shader escrito para 64/124 corre em
/// todo o lado sem variantes.
pub const MESHLET_VERTS: usize = 64;
pub const MESHLET_PRIMS: usize = 124;

/// O formato canónico, para mesh shaders.
///
/// Um meshlet não guarda índices globais: guarda uma lista curta de vértices
/// únicos (no máximo 64) e os triângulos como índices **locais** de 8 bits nessa
/// lista. É isso que permite a um workgroup de mesh shader carregar os vértices
/// uma vez e emiti-los sem repetir — com índices globais, três triângulos
/// vizinhos buscariam o mesmo vértice três vezes.
#[derive(Clone, Debug, Default)]
pub struct MeshletData {
    /// Índices globais dos vértices, agrupados por meshlet.
    pub vertices: Vec<u32>,
    /// Três índices locais de 8 bits por triângulo, empacotados num `u32`.
    pub triangles: Vec<u32>,
    pub meshlets: Vec<MeshletRange>,
}

/// Onde um meshlet vive dentro de [`MeshletData`].
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct MeshletRange {
    pub vertex_offset: u32,
    pub vertex_count: u32,
    pub triangle_offset: u32,
    pub triangle_count: u32,
}

/// Um grupo de triângulos com a sua própria caixa.
#[derive(Clone, Copy, Debug)]
pub struct Meshlet {
    /// Índice da primitiva de origem: a matriz e o material vêm de lá.
    pub prim: u32,
    /// Onde começa no index buffer partilhado, e quantos índices tem.
    pub first_index: u32,
    pub index_count: u32,
    pub vertex_offset: i32,
    /// Em espaço do mundo, já com a transformação do nó aplicada.
    pub centre: [f32; 3],
    pub extents: [f32; 3],
}

/// Parte uma primitiva em meshlets e devolve os índices reordenados.
///
/// Os triângulos são ordenados por **código de Morton do centróide** antes de
/// serem cortados em grupos. Cortá-los pela ordem em que vêm no ficheiro daria
/// grupos com triângulos de sítios distantes da malha, e a caixa envolvente de um
/// grupo assim cobre meia primitiva — que é exactamente o que o culling não quer.
///
/// Devolve `(meshlets, índices reordenados)`, com os índices ainda relativos à
/// primitiva; quem chama soma o `vertex_offset`.
/// `max_verts` é o tecto de **vértices únicos** por grupo.
///
/// `usize::MAX` para o caminho clássico, onde não há limite nenhum; 64 para mesh
/// shaders, onde é do hardware. Tem de entrar aqui e não no empacotamento: 128
/// triângulos agrupados por vizinhança precisam de bem mais de 64 vértices, e
/// deixar a embalagem partir depois desalinhava as tabelas dos comandos.
pub fn build_meshlets(
    prim: &CpuPrimitive,
    prim_index: u32,
    tris_per_meshlet: usize,
    max_verts: usize,
) -> (Vec<Meshlet>, Vec<u32>) {
    let tris_per_meshlet = tris_per_meshlet.max(1);
    let max_verts = max_verts.max(3);
    let tri_count = prim.indices.len() / 3;
    if tri_count == 0 {
        return (Vec::new(), Vec::new());
    }

    // Caixa da primitiva, para normalizar os centróides antes do Morton.
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in &prim.vertices {
        for a in 0..3 {
            lo[a] = lo[a].min(v.pos[a]);
            hi[a] = hi[a].max(v.pos[a]);
        }
    }
    // **Uma** escala para os três eixos, não uma por eixo.
    //
    // Normalizar cada eixo à sua própria extensão faz um passo de Morton valer
    // distâncias diferentes em cada direcção: numa superfície de 32 × 6 × 32, os
    // 10 bits de Y cobrem seis unidades e os de X cobrem trinta e duas, portanto o
    // Y fica cinco vezes sobre-representado e a ordenação agrupa por faixas de
    // altura em vez de por vizinhança. Medido num heightfield de teste: o volume
    // somado das caixas dava **23 628** contra 6 090 de uma caixa só — os grupos
    // sobrepunham-se em vez de ladrilhar.
    let span = (hi[0] - lo[0])
        .max(hi[1] - lo[1])
        .max(hi[2] - lo[2])
        .max(1e-6);
    let span = [span, span, span];

    let centroid = |t: usize| {
        let mut c = [0.0f32; 3];
        for k in 0..3 {
            let v = &prim.vertices[prim.indices[t * 3 + k] as usize];
            for a in 0..3 {
                c[a] += v.pos[a] / 3.0;
            }
        }
        c
    };

    let mut order: Vec<(u32, usize)> = (0..tri_count)
        .map(|t| {
            let c = centroid(t);
            let q = |a: usize| (((c[a] - lo[a]) / span[a]).clamp(0.0, 1.0) * 1023.0) as u32;
            (morton3(q(0), q(1), q(2)), t)
        })
        .collect();
    // Só se vale a pena. Com um meshlet por primitiva a ordenação não agrupa
    // nada e só estraga a localidade que o ficheiro já tinha: medido, custava
    // 5% do frame da Sponza por nada.
    if tris_per_meshlet < tri_count {
        order.sort_unstable();
    }

    let mut out_idx: Vec<u32> = Vec::with_capacity(prim.indices.len());
    let mut meshlets = Vec::with_capacity(tri_count.div_ceil(tris_per_meshlet));

    let mut first = 0u32;
    let mut tris_here = 0usize;
    let mut unique: Vec<u32> = Vec::with_capacity(max_verts.min(256));
    let (mut mlo, mut mhi) = ([f32::MAX; 3], [f32::MIN; 3]);

    let close = |first: &mut u32,
                 tris_here: &mut usize,
                 unique: &mut Vec<u32>,
                 mlo: &mut [f32; 3],
                 mhi: &mut [f32; 3],
                 out_idx: &Vec<u32>,
                 meshlets: &mut Vec<Meshlet>| {
        if *tris_here == 0 {
            return;
        }
        meshlets.push(Meshlet {
            prim: prim_index,
            first_index: *first,
            index_count: (*tris_here * 3) as u32,
            vertex_offset: 0,
            centre: [
                (mlo[0] + mhi[0]) * 0.5,
                (mlo[1] + mhi[1]) * 0.5,
                (mlo[2] + mhi[2]) * 0.5,
            ],
            extents: [
                (mhi[0] - mlo[0]) * 0.5,
                (mhi[1] - mlo[1]) * 0.5,
                (mhi[2] - mlo[2]) * 0.5,
            ],
        });
        *first = out_idx.len() as u32;
        *tris_here = 0;
        unique.clear();
        *mlo = [f32::MAX; 3];
        *mhi = [f32::MIN; 3];
    };

    for &(_, t) in &order {
        let gi = [
            prim.indices[t * 3],
            prim.indices[t * 3 + 1],
            prim.indices[t * 3 + 2],
        ];
        // Quantos vértices **novos e distintos** este triângulo traz. O `gi[..k]`
        // evita contar duas vezes um vértice repetido dentro do próprio triângulo;
        // sem isso a conta subestimava e o empacotamento tinha de partir o grupo
        // à mesma, desalinhando as tabelas dos comandos.
        let mut missing = 0;
        for (k, g) in gi.iter().enumerate() {
            if unique.contains(g) || gi[..k].contains(g) {
                continue;
            }
            missing += 1;
        }
        // Fecha antes de estourar qualquer um dos dois tectos.
        if tris_here >= tris_per_meshlet || unique.len() + missing > max_verts {
            close(
                &mut first,
                &mut tris_here,
                &mut unique,
                &mut mlo,
                &mut mhi,
                &out_idx,
                &mut meshlets,
            );
        }
        for g in gi {
            out_idx.push(g);
            if !unique.contains(&g) {
                unique.push(g);
            }
            let v = &prim.vertices[g as usize];
            let w = prim
                .world
                .transform_point3(harpia_math::Vec3::new(v.pos[0], v.pos[1], v.pos[2]));
            let w = [w.x, w.y, w.z];
            for a in 0..3 {
                mlo[a] = mlo[a].min(w[a]);
                mhi[a] = mhi[a].max(w[a]);
            }
        }
        tris_here += 1;
    }
    close(
        &mut first,
        &mut tris_here,
        &mut unique,
        &mut mlo,
        &mut mhi,
        &out_idx,
        &mut meshlets,
    );
    (meshlets, out_idx)
}

/// Converte os meshlets de intervalos-de-índices para o formato canónico.
///
/// Recebe o que o [`build_meshlets`] devolveu — grupos de triângulos com índices
/// globais — e reescreve cada grupo como uma lista de vértices únicos mais
/// triângulos locais. Um grupo com mais de [`MESHLET_VERTS`] vértices únicos é
/// **partido**, porque o limite é do hardware e não uma preferência.
pub fn pack_meshlets(meshlets: &[Meshlet], indices: &[u32]) -> MeshletData {
    let mut out = MeshletData::default();
    for m in meshlets {
        let tris = m.index_count as usize / 3;
        let mut local: Vec<u32> = Vec::with_capacity(MESHLET_VERTS);
        let mut tri_buf: Vec<u32> = Vec::with_capacity(MESHLET_PRIMS);
        let flush = |local: &mut Vec<u32>, tri_buf: &mut Vec<u32>, out: &mut MeshletData| {
            if tri_buf.is_empty() {
                return;
            }
            out.meshlets.push(MeshletRange {
                vertex_offset: out.vertices.len() as u32,
                vertex_count: local.len() as u32,
                triangle_offset: out.triangles.len() as u32,
                triangle_count: tri_buf.len() as u32,
            });
            out.vertices.append(local);
            out.triangles.append(tri_buf);
        };
        for t in 0..tris {
            let gi = [
                indices[m.first_index as usize + t * 3],
                indices[m.first_index as usize + t * 3 + 1],
                indices[m.first_index as usize + t * 3 + 2],
            ];
            // Quantos destes três ainda não estão na lista local?
            let missing = gi
                .iter()
                .filter(|g| !local.contains(g))
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            if local.len() + missing > MESHLET_VERTS || tri_buf.len() >= MESHLET_PRIMS {
                flush(&mut local, &mut tri_buf, &mut out);
            }
            let mut packed = 0u32;
            for (k, g) in gi.iter().enumerate() {
                let li = match local.iter().position(|v| v == g) {
                    Some(i) => i,
                    None => {
                        local.push(*g);
                        local.len() - 1
                    }
                };
                packed |= (li as u32) << (k * 8);
            }
            tri_buf.push(packed);
        }
        flush(&mut local, &mut tri_buf, &mut out);
    }
    out
}

/// Entrelaça os bits de três valores de 10 bits.
fn morton3(x: u32, y: u32, z: u32) -> u32 {
    let part = |mut v: u32| {
        v &= 0x3ff;
        v = (v | (v << 16)) & 0x030000ff;
        v = (v | (v << 8)) & 0x0300f00f;
        v = (v | (v << 4)) & 0x030c30c3;
        v = (v | (v << 2)) & 0x09249249;
        v
    };
    part(x) | (part(y) << 1) | (part(z) << 2)
}

/// glTF image index (or a flat base-colour factor) → index into `images`.
#[derive(PartialEq, Eq, Hash)]
enum ImageKey {
    Source(usize),
    Factor([u8; 4]),
}

struct Loader<'a> {
    buffers: &'a [gltf::buffer::Data],
    images: &'a [gltf::image::Data],
    out_images: Vec<CpuImage>,
    keys: HashMap<ImageKey, usize>,
    prims: Vec<CpuPrimitive>,
}

pub fn load_path(path: &std::path::Path) -> Result<CpuScene, GltfError> {
    if !path.exists() {
        return Err(GltfError(format!(
            "glTF not found at {}. Run prog/tools/fetch_sponza.py",
            path.display()
        )));
    }
    let (doc, buffers, images) = gltf::import(path)?;
    let mut loader = Loader {
        buffers: &buffers,
        images: &images,
        out_images: Vec::new(),
        keys: HashMap::new(),
        prims: Vec::new(),
    };
    if let Some(scene) = doc.default_scene().or_else(|| doc.scenes().next()) {
        for node in scene.nodes() {
            loader.visit_node(&node, Mat4::IDENTITY)?;
        }
    }
    if loader.prims.is_empty() {
        return Err(GltfError("glTF scene has no triangle primitives".into()));
    }
    Ok(CpuScene {
        images: loader.out_images,
        prims: loader.prims,
    })
}

impl Loader<'_> {
    fn visit_node(&mut self, node: &gltf::Node<'_>, parent: Mat4) -> Result<(), GltfError> {
        let local = Mat4::from_cols_array_2d(&node.transform().matrix());
        let world = parent * local;
        if let Some(mesh) = node.mesh() {
            for prim in mesh.primitives() {
                if prim.mode() != Mode::Triangles {
                    continue;
                }
                let p = self.load_primitive(&prim, world)?;
                self.prims.push(p);
            }
        }
        for child in node.children() {
            self.visit_node(&child, world)?;
        }
        Ok(())
    }

    fn load_primitive(
        &mut self,
        prim: &gltf::Primitive<'_>,
        world: Mat4,
    ) -> Result<CpuPrimitive, GltfError> {
        let buffers = self.buffers;
        let reader = prim.reader(|b| buffers.get(b.index()).map(|d| d.0.as_slice()));
        let positions: Vec<[f32; 3]> = reader
            .read_positions()
            .ok_or_else(|| GltfError("primitive missing POSITION".into()))?
            .collect();
        let normals: Vec<[f32; 3]> = match reader.read_normals() {
            Some(n) => n.collect(),
            None => vec![[0.0, 1.0, 0.0]; positions.len()],
        };
        let uvs: Vec<[f32; 2]> = match reader.read_tex_coords(0).map(|t| t.into_f32()) {
            Some(t) => t.collect(),
            None => vec![[0.0, 0.0]; positions.len()],
        };
        let n = positions.len();
        let mut vertices = Vec::with_capacity(n);
        for i in 0..n {
            let nrm = normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
            let uv = uvs.get(i).copied().unwrap_or([0.0, 0.0]);
            vertices.push(MeshVertex {
                pos: positions[i],
                nrm: Vec3::from_array(nrm).normalize_or_zero().to_array(),
                uv,
            });
        }
        let indices: Vec<u32> = if let Some(idx) = reader.read_indices() {
            idx.into_u32().collect()
        } else {
            (0..n as u32).collect()
        };
        Ok(CpuPrimitive {
            vertices,
            indices,
            albedo: self.albedo_of(prim),
            alpha_cutoff: alpha_cutoff_of(prim),
            double_sided: double_sided_of(prim),
            world,
        })
    }

    fn albedo_of(&mut self, prim: &gltf::Primitive<'_>) -> usize {
        let mat = prim.material();
        let pbr = mat.pbr_metallic_roughness();
        let (key, build): (ImageKey, Box<dyn FnOnce(&[gltf::image::Data]) -> CpuImage>) =
            match pbr.base_color_texture() {
                Some(info) => {
                    let src = info.texture().source().index();
                    (
                        ImageKey::Source(src),
                        Box::new(move |images: &[gltf::image::Data]| to_rgba(&images[src])),
                    )
                }
                None => {
                    let f = pbr.base_color_factor();
                    let px = [
                        (f[0].clamp(0.0, 1.0) * 255.0) as u8,
                        (f[1].clamp(0.0, 1.0) * 255.0) as u8,
                        (f[2].clamp(0.0, 1.0) * 255.0) as u8,
                        (f[3].clamp(0.0, 1.0) * 255.0) as u8,
                    ];
                    (
                        ImageKey::Factor(px),
                        Box::new(move |_: &[gltf::image::Data]| CpuImage {
                            rgba: px.to_vec(),
                            width: 1,
                            height: 1,
                        }),
                    )
                }
            };
        if let Some(i) = self.keys.get(&key) {
            return *i;
        }
        let img = build(self.images);
        let i = self.out_images.len();
        self.out_images.push(img);
        self.keys.insert(key, i);
        i
    }
}

/// Sponza declares `MASK` + `doubleSided` on its foliage and chain. Anything
/// else is opaque and must not pay for a discard.
fn alpha_cutoff_of(prim: &gltf::Primitive<'_>) -> f32 {
    match prim.material().alpha_mode() {
        gltf::material::AlphaMode::Mask => prim.material().alpha_cutoff().unwrap_or(0.5),
        _ => 0.0,
    }
}

/// glTF `doubleSided`: the cutout foliage needs both faces.
fn double_sided_of(prim: &gltf::Primitive<'_>) -> bool {
    prim.material().double_sided()
}

fn to_rgba(img: &gltf::image::Data) -> CpuImage {
    let n = (img.width * img.height) as usize;
    let rgba = match img.format {
        gltf::image::Format::R8G8B8A8 => img.pixels.clone(),
        gltf::image::Format::R8G8B8 => {
            let mut out = Vec::with_capacity(n * 4);
            for c in img.pixels.chunks(3) {
                out.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
            out
        }
        gltf::image::Format::R8 => img.pixels.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        gltf::image::Format::R8G8 => {
            let mut out = Vec::with_capacity(n * 4);
            for c in img.pixels.chunks(2) {
                out.extend_from_slice(&[c[0], c[0], c[0], c[1]]);
            }
            out
        }
        _ => vec![200u8, 200, 200, 255].repeat(n),
    };
    CpuImage {
        rgba,
        width: img.width,
        height: img.height,
    }
}

fn srgb_to_linear_table() -> &'static [f32; 256] {
    static TABLE: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0.0f32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *v = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
        }
        t
    })
}

fn linear_to_srgb_u8(v: f32) -> u8 {
    let c = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// 2×2 box filter. RGB is averaged in linear light, alpha as-is.
fn downsample_srgb(src: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let table = srgb_to_linear_table();
    let dw = (w / 2).max(1);
    let dh = (h / 2).max(1);
    let mut out = vec![0u8; dw as usize * dh as usize * 4];
    for y in 0..dh {
        let y0 = (y * 2).min(h - 1) as usize;
        let y1 = (y * 2 + 1).min(h - 1) as usize;
        for x in 0..dw {
            let x0 = (x * 2).min(w - 1) as usize;
            let x1 = (x * 2 + 1).min(w - 1) as usize;
            let taps = [
                (y0 * w as usize + x0) * 4,
                (y0 * w as usize + x1) * 4,
                (y1 * w as usize + x0) * 4,
                (y1 * w as usize + x1) * 4,
            ];
            let mut rgb = [0.0f32; 3];
            let mut a = 0.0f32;
            for t in taps {
                rgb[0] += table[src[t] as usize];
                rgb[1] += table[src[t + 1] as usize];
                rgb[2] += table[src[t + 2] as usize];
                a += src[t + 3] as f32;
            }
            let d = (y as usize * dw as usize + x as usize) * 4;
            out[d] = linear_to_srgb_u8(rgb[0] * 0.25);
            out[d + 1] = linear_to_srgb_u8(rgb[1] * 0.25);
            out[d + 2] = linear_to_srgb_u8(rgb[2] * 0.25);
            out[d + 3] = (a * 0.25 + 0.5) as u8;
        }
    }
    (out, dw, dh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32) -> CpuImage {
        CpuImage {
            rgba: vec![255u8; (w * h * 4) as usize],
            width: w,
            height: h,
        }
    }

    #[test]
    fn mip_chain_reaches_one_by_one() {
        let i = img(256, 64);
        assert_eq!(i.mip_levels(), 9);
        let chain = i.mip_chain();
        assert_eq!(chain.len(), 9);
        assert_eq!(chain[0].len(), 256 * 64 * 4);
        // Last level is 1×1: max(256>>8,1) × max(64>>8,1).
        assert_eq!(chain[8].len(), 4);
        // White stays white through the linear round trip.
        assert_eq!(chain[8], vec![255u8; 4]);
    }

    #[test]
    fn one_by_one_has_a_single_level() {
        let i = img(1, 1);
        assert_eq!(i.mip_levels(), 1);
        assert_eq!(i.mip_chain().len(), 1);
    }
}

#[cfg(test)]
mod meshlet_tests {
    use super::*;
    use harpia_math::Mat4;

    /// Uma grelha de quads, para haver vértices partilhados a sério.
    fn grid(n: usize) -> CpuPrimitive {
        let mut vertices = Vec::new();
        for y in 0..=n {
            for x in 0..=n {
                // Relevo **lento**: uma grelha plana degenera o volume em zero dos
                // dois lados, e um relevo rápido faz qualquer patch cobrir a gama
                // toda de altura — nos dois casos o teste mede a função de teste em
                // vez da partição. A primeira versão tinha `sin(x * 0.7)` e foi
                // isso que aconteceu.
                let h = ((x as f32) * 0.1).sin() * ((y as f32) * 0.1).cos() * 3.0;
                vertices.push(MeshVertex {
                    pos: [x as f32, h, y as f32],
                    nrm: [0.0, 1.0, 0.0],
                    uv: [0.0, 0.0],
                });
            }
        }
        let mut indices = Vec::new();
        let w = (n + 1) as u32;
        for y in 0..n as u32 {
            for x in 0..n as u32 {
                let i = y * w + x;
                indices.extend_from_slice(&[i, i + w, i + 1, i + 1, i + w, i + w + 1]);
            }
        }
        CpuPrimitive {
            vertices,
            indices,
            albedo: 0,
            alpha_cutoff: 0.0,
            double_sided: false,
            world: Mat4::IDENTITY,
        }
    }

    /// O formato canónico tem de descrever **exactamente** os mesmos triângulos.
    ///
    /// É o único invariante que interessa: um meshlet que perca ou invente um
    /// triângulo é geometria que aparece ou desaparece, e num mesh shader isso não
    /// dá erro nenhum.
    #[test]
    fn packing_preserves_every_triangle() {
        let p = grid(24);
        let (ml, idx) = build_meshlets(&p, 0, 64, MESHLET_VERTS);
        let packed = pack_meshlets(&ml, &idx);

        let mut got: Vec<[u32; 3]> = Vec::new();
        for r in &packed.meshlets {
            for t in 0..r.triangle_count as usize {
                let tri = packed.triangles[r.triangle_offset as usize + t];
                let v = |k: u32| {
                    packed.vertices[r.vertex_offset as usize + ((tri >> (k * 8)) & 0xff) as usize]
                };
                got.push([v(0), v(1), v(2)]);
            }
        }
        let want: Vec<[u32; 3]> = idx.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
        assert_eq!(got.len(), want.len(), "número de triângulos mudou");
        let mut a = got.clone();
        let mut b = want.clone();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b, "os triângulos não são os mesmos");
    }

    /// Os limites são do hardware, não uma preferência: exceder um deles é um
    /// mesh shader que escreve fora do que declarou.
    #[test]
    fn packing_respects_the_hardware_limits() {
        let p = grid(40);
        let (ml, idx) = build_meshlets(&p, 0, 128, MESHLET_VERTS);
        let packed = pack_meshlets(&ml, &idx);
        assert!(!packed.meshlets.is_empty());
        for (i, r) in packed.meshlets.iter().enumerate() {
            assert!(
                r.vertex_count as usize <= MESHLET_VERTS,
                "meshlet {i} com {} vértices",
                r.vertex_count
            );
            assert!(
                r.triangle_count as usize <= MESHLET_PRIMS,
                "meshlet {i} com {} triângulos",
                r.triangle_count
            );
            // Os índices locais têm 8 bits: um que aponte para lá do fim da lista
            // lê outro vértice qualquer, sem erro nenhum.
            for t in 0..r.triangle_count as usize {
                let tri = packed.triangles[r.triangle_offset as usize + t];
                for k in 0..3 {
                    let li = (tri >> (k * 8)) & 0xff;
                    assert!(
                        li < r.vertex_count,
                        "índice local {li} fora de {}",
                        r.vertex_count
                    );
                }
            }
        }
    }

    /// Partir em meshlets não pode perder triângulos, seja qual for o tamanho.
    #[test]
    fn build_meshlets_keeps_every_triangle() {
        let p = grid(17);
        for n in [1, 7, 64, 128, usize::MAX] {
            let (ml, idx) = build_meshlets(&p, 0, n, usize::MAX);
            assert_eq!(
                idx.len(),
                p.indices.len(),
                "n={n}: índices a mais ou a menos"
            );
            let total: u32 = ml.iter().map(|m| m.index_count).sum();
            assert_eq!(total as usize, p.indices.len(), "n={n}");
        }
    }

    /// Sem tectos, o resultado tem de ser **exactamente** a primitiva.
    ///
    /// É o caminho por omissão de toda a árvore: um meshlet por primitiva, os
    /// índices pela ordem do ficheiro, e a caixa igual à da primitiva. Este teste
    /// existe porque a construção passou a acumular triângulo a triângulo em vez
    /// de cortar em blocos, e essa mudança apanha o caminho normal — que eu não
    /// podia verificar na GPU quando a escrevi.
    #[test]
    fn no_limits_reproduces_the_primitive_exactly() {
        let p = grid(9);
        let (ml, idx) = build_meshlets(&p, 7, usize::MAX, usize::MAX);
        assert_eq!(ml.len(), 1, "devia dar um meshlet só");
        assert_eq!(idx, p.indices, "os índices não podem ser reordenados");
        let m = ml[0];
        assert_eq!(m.prim, 7);
        assert_eq!(m.first_index, 0);
        assert_eq!(m.index_count as usize, p.indices.len());

        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in &p.vertices {
            for a in 0..3 {
                lo[a] = lo[a].min(v.pos[a]);
                hi[a] = hi[a].max(v.pos[a]);
            }
        }
        for a in 0..3 {
            let c = (lo[a] + hi[a]) * 0.5;
            let e = (hi[a] - lo[a]) * 0.5;
            assert!(
                (m.centre[a] - c).abs() < 1e-4,
                "centro em {a}: {} vs {c}",
                m.centre[a]
            );
            assert!((m.extents[a] - e).abs() < 1e-4, "extensão em {a}");
        }
    }

    /// Os intervalos têm de ladrilhar o index buffer sem buracos nem sobreposição.
    #[test]
    fn meshlet_ranges_tile_the_index_buffer() {
        let p = grid(19);
        for (t, v) in [
            (usize::MAX, usize::MAX),
            (64, 64),
            (124, 64),
            (7, usize::MAX),
        ] {
            let (ml, idx) = build_meshlets(&p, 0, t, v);
            let mut next = 0u32;
            for (i, m) in ml.iter().enumerate() {
                assert_eq!(
                    m.first_index, next,
                    "t={t} v={v}: buraco antes do meshlet {i}"
                );
                assert!(m.index_count > 0 && m.index_count % 3 == 0, "t={t} v={v}");
                next += m.index_count;
            }
            assert_eq!(
                next as usize,
                idx.len(),
                "t={t} v={v}: sobra índice por cobrir"
            );
        }
    }

    /// Meshlets mais pequenos têm de dar caixas mais apertadas — é a razão de eles
    /// existirem, e um agrupamento que não agrupe não serve de nada.
    #[test]
    fn smaller_meshlets_give_tighter_bounds() {
        let p = grid(32);
        let volume = |n: usize| -> f64 {
            let (ml, _) = build_meshlets(&p, 0, n, usize::MAX);
            ml.iter()
                .map(|m| {
                    (2.0 * m.extents[0] as f64).max(1e-4)
                        * (2.0 * m.extents[1] as f64).max(1e-4)
                        * (2.0 * m.extents[2] as f64).max(1e-4)
                })
                .sum()
        };
        let big = volume(usize::MAX);
        let small = volume(64);
        assert!(
            small < big * 0.25,
            "64 triângulos por meshlet dão {small:.1} de volume somado contra {big:.1} \
             de um só: a ordenação espacial não está a agrupar"
        );
    }
}
