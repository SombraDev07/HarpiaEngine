//! UV sphere, pos+normal tightly packed (stride 24).
//! Winding is CCW **seen from outside**, which is what `FRONT_FACE_COUNTER_CLOCKWISE`
//! plus [`harpia_math::perspective_vk`] expects. Do not re-wind to "fix" a mirrored image.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MeshVertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    pub uv: [f32; 2],
}

pub struct SphereMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl SphereMesh {
    pub fn uv(slices: u32, stacks: u32) -> Self {
        let slices = slices.max(3);
        let stacks = stacks.max(2);
        let mut vertices = Vec::new();
        for st in 0..=stacks {
            let v = st as f32 / stacks as f32;
            let theta = v * std::f32::consts::PI;
            let sin_t = theta.sin();
            let cos_t = theta.cos();
            for sl in 0..=slices {
                let u = sl as f32 / slices as f32;
                let phi = u * std::f32::consts::TAU;
                let x = sin_t * phi.cos();
                let y = cos_t;
                let z = sin_t * phi.sin();
                vertices.push(Vertex {
                    pos: [x, y, z],
                    nrm: [x, y, z],
                });
            }
        }
        let mut indices = Vec::new();
        let stride = slices + 1;
        for st in 0..stacks {
            for sl in 0..slices {
                let i0 = st * stride + sl;
                let i1 = i0 + 1;
                let i2 = i0 + stride;
                let i3 = i2 + 1;
                indices.extend_from_slice(&[i0, i1, i2, i1, i3, i2]);
            }
        }
        Self { vertices, indices }
    }

    /// Unit XZ quad, normal +Y, edge length 2. Instance `pos_scale.w` is half-extent.
    pub fn plane_xz() -> Self {
        let n = [0.0, 1.0, 0.0];
        Self {
            vertices: vec![
                Vertex {
                    pos: [-1.0, 0.0, -1.0],
                    nrm: n,
                },
                Vertex {
                    pos: [1.0, 0.0, -1.0],
                    nrm: n,
                },
                Vertex {
                    pos: [1.0, 0.0, 1.0],
                    nrm: n,
                },
                Vertex {
                    pos: [-1.0, 0.0, 1.0],
                    nrm: n,
                },
            ],
            indices: vec![0, 2, 1, 0, 3, 2],
        }
    }

    pub fn vertex_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self.vertices.as_ptr().cast::<u8>(),
                self.vertices.len() * std::mem::size_of::<Vertex>(),
            )
        }
    }

    pub fn index_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self.indices.as_ptr().cast::<u8>(),
                self.indices.len() * std::mem::size_of::<u32>(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Outward-facing CCW: every non-degenerate triangle's cross product must
    /// point the same way as its vertex normal. The poles are degenerate.
    #[test]
    fn winding_faces_outwards() {
        use harpia_math::Vec3;
        let s = SphereMesh::uv(16, 12);
        let mut checked = 0;
        for t in s.indices.chunks_exact(3) {
            let v = |i: u32| Vec3::from_array(s.vertices[i as usize].pos);
            let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
            let n = (b - a).cross(c - a);
            if n.length_squared() < 1e-8 {
                continue; // pole fan
            }
            let outward = (a + b + c) / 3.0;
            assert!(n.dot(outward) > 0.0, "sphere triangle faces inwards");
            checked += 1;
        }
        assert!(checked > 100, "expected real triangles, checked {checked}");

        let p = SphereMesh::plane_xz();
        for t in p.indices.chunks_exact(3) {
            let v = |i: u32| Vec3::from_array(p.vertices[i as usize].pos);
            let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
            assert!((b - a).cross(c - a).y > 0.0, "ground plane faces down");
        }
    }

    #[test]
    fn sphere_has_triangles() {
        let s = SphereMesh::uv(16, 12);
        assert!(!s.indices.is_empty());
        assert_eq!(s.indices.len() % 3, 0);
        assert_eq!(std::mem::size_of::<Vertex>(), 24);
        assert_eq!(std::mem::size_of::<MeshVertex>(), 32);
        let p = SphereMesh::plane_xz();
        assert_eq!(p.indices.len(), 6);
    }
}
