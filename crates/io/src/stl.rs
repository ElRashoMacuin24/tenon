//! STL output (binary and ASCII) from a kernel [`Mesh`]. Units are millimetres; STL itself has no
//! unit field.

use std::fmt::Write as _;

use tenon_geom::Vec3;
use tenon_kernel::Mesh;

/// Size of the binary STL header.
const HEADER_LEN: usize = 80;
/// Bytes per binary STL facet: normal + 3 vertices (12 f32) + attribute count (u16).
const FACET_LEN: usize = 50;

fn triangles(mesh: &Mesh) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
    let (tris, _) = mesh.indices.as_chunks::<3>();
    tris.iter().filter_map(|&[a, b, c]| Some([*mesh.positions.get(a as usize)?, *mesh.positions.get(b as usize)?, *mesh.positions.get(c as usize)?]))
}

fn facet_normal(t: &[[f32; 3]; 3]) -> [f32; 3] {
    let p = t.map(|v| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2])));
    let n = (p[1] - p[0]).cross(p[2] - p[0]).normalized();
    [n.x as f32, n.y as f32, n.z as f32]
}

/// Binary STL. `header` is truncated to fit the 80-byte header and never starts with `solid`
/// (which would make readers take the file for ASCII STL).
pub fn write_binary(mesh: &Mesh, header: &str) -> Vec<u8> {
    let tris: Vec<[[f32; 3]; 3]> = triangles(mesh).collect();
    let mut out = Vec::with_capacity(HEADER_LEN + 4 + tris.len() * FACET_LEN);
    let mut head = header.as_bytes().to_vec();
    if head.starts_with(b"solid") {
        head.insert(0, b'_');
    }
    head.resize(HEADER_LEN, b' ');
    out.extend_from_slice(&head);
    let count = u32::try_from(tris.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&count.to_le_bytes());
    for t in tris.iter().take(count as usize) {
        for v in std::iter::once(facet_normal(t)).chain(t.iter().copied()) {
            for c in v {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

/// ASCII STL.
pub fn write_ascii(mesh: &Mesh, name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    let mut out = format!("solid {name}\n");
    for t in triangles(mesh) {
        let n = facet_normal(&t);
        let _ = writeln!(out, "  facet normal {:e} {:e} {:e}\n    outer loop", n[0], n[1], n[2]);
        for v in t {
            let _ = writeln!(out, "      vertex {:e} {:e} {:e}", v[0], v[1], v[2]);
        }
        out.push_str("    endloop\n  endfacet\n");
    }
    let _ = writeln!(out, "endsolid {name}");
    out
}

/// Triangle count of binary STL data, if the data is well formed (length matches the count).
pub fn binary_triangle_count(data: &[u8]) -> Option<u32> {
    let count = u32::from_le_bytes(data.get(HEADER_LEN..HEADER_LEN + 4)?.try_into().ok()?);
    (data.len() == HEADER_LEN + 4 + count as usize * FACET_LEN).then_some(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tenon_kernel::FaceRange;

    fn cube() -> Mesh {
        let positions = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let quads: [[u32; 4]; 6] = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [3, 0, 4, 7]];
        let mut indices = Vec::new();
        for q in quads {
            indices.extend_from_slice(&[q[0], q[1], q[2], q[0], q[2], q[3]]);
        }
        Mesh { normals: vec![[0.0; 3]; 8], positions, faces: vec![FaceRange { face: 0, first: 0, count: 36 }], indices, edges: vec![] }
    }

    #[test]
    fn binary_layout() {
        let data = write_binary(&cube(), "Tenon test");
        assert_eq!(data.len(), 80 + 4 + 12 * 50);
        assert_eq!(binary_triangle_count(&data), Some(12));
        assert!(data.starts_with(b"Tenon test"));
        // First facet: bottom face, normal -Z.
        let f = |o: usize| f32::from_le_bytes(data[o..o + 4].try_into().unwrap());
        assert_eq!((f(84), f(88), f(92)), (0.0, 0.0, -1.0));
        assert_eq!(binary_triangle_count(&data[..data.len() - 1]), None);
        assert_eq!(binary_triangle_count(b"short"), None);
    }

    #[test]
    fn header_never_looks_ascii() {
        let data = write_binary(&cube(), "solid trouble");
        assert!(!data.starts_with(b"solid"));
        assert_eq!(write_binary(&cube(), &"x".repeat(200)).len(), 684);
    }

    #[test]
    fn ascii_layout() {
        let s = write_ascii(&cube(), "cube\n");
        assert!(s.starts_with("solid cube\n"));
        assert_eq!(s.matches("facet normal").count(), 12);
        assert_eq!(s.matches("vertex").count(), 36);
        assert!(s.trim_end().ends_with("endsolid cube"));
    }

    #[test]
    fn bad_indices_are_skipped() {
        let mut m = cube();
        m.indices.extend_from_slice(&[0, 1, 99]);
        assert_eq!(binary_triangle_count(&write_binary(&m, "")), Some(12));
    }
}
