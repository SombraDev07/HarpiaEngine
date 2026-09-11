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
pub fn build_meshlets(
    prim: &CpuPrimitive,
    prim_index: u32,
    tris_per_meshlet: usize,
) -> (Vec<Meshlet>, Vec<u32>) {
    let tris_per_meshlet = tris_per_meshlet.max(1);
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
    let span = [
        (hi[0] - lo[0]).max(1e-6),
        (hi[1] - lo[1]).max(1e-6),
        (hi[2] - lo[2]).max(1e-6),
    ];

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
    for chunk in order.chunks(tris_per_meshlet) {
        let first = out_idx.len() as u32;
        let (mut mlo, mut mhi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for &(_, t) in chunk {
            for k in 0..3 {
                let i = prim.indices[t * 3 + k];
                out_idx.push(i);
                let v = &prim.vertices[i as usize];
                let w = prim
                    .world
                    .transform_point3(harpia_math::Vec3::new(v.pos[0], v.pos[1], v.pos[2]));
                let w = [w.x, w.y, w.z];
                for a in 0..3 {
                    mlo[a] = mlo[a].min(w[a]);
                    mhi[a] = mhi[a].max(w[a]);
                }
            }
        }
        meshlets.push(Meshlet {
            prim: prim_index,
            first_index: first,
            index_count: (chunk.len() * 3) as u32,
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
    }
    (meshlets, out_idx)
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
