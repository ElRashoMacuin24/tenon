//! Query results: topology, face and edge geometry, meshes, mass properties.

use serde::{Deserialize, Serialize};
use tenon_geom::{Axis, Vec2, Vec3, tol};

/// Top-level type of a shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    Compound,
    CompSolid,
    Solid,
    Shell,
    Face,
    Wire,
    Edge,
    Vertex,
}

/// Topology of one shape. Sub-shapes are numbered 0.. in a deterministic order that is stable
/// for this shape but **not** across operations (use [`History`](crate::History) for that).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Topology {
    pub kind: ShapeKind,
    pub solids: u32,
    pub shells: u32,
    pub faces: u32,
    pub edges: u32,
    pub vertices: u32,
    /// For each face, the indices of its edges.
    pub face_edges: Vec<Vec<u32>>,
    /// For each edge, the indices of the faces that contain it (two for a manifold solid).
    pub edge_faces: Vec<Vec<u32>>,
    /// For each edge, its vertex indices (one for a closed edge such as a full circle).
    pub edge_vertices: Vec<Vec<u32>>,
}

/// Geometry type of a face's underlying surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SurfaceKind {
    /// `normal` points out of the material (the face orientation is applied).
    Plane {
        origin: Vec3,
        normal: Vec3,
    },
    Cylinder {
        axis: Axis,
        radius: f64,
    },
    Cone {
        axis: Axis,
        half_angle: f64,
        ref_radius: f64,
    },
    Sphere {
        center: Vec3,
        radius: f64,
    },
    Torus {
        axis: Axis,
        major_radius: f64,
        minor_radius: f64,
    },
    BSpline,
    Bezier,
    Revolution,
    Extrusion,
    Offset,
    Other,
}

/// Geometry and measurements of one face.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceInfo {
    pub surface: SurfaceKind,
    pub area: f64,
    pub centroid: Vec3,
    /// The face looks the other way from its surface's outward normal. For a cylinder, cone,
    /// sphere or torus that means the material is outside it: the wall of a hole, not of a shaft.
    /// (A plane's `normal` has the orientation applied already.)
    pub reversed: bool,
}

/// Geometry type of an edge's underlying curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CurveKind {
    Line { origin: Vec3, dir: Vec3 },
    Circle { axis: Axis, radius: f64 },
    Ellipse,
    Hyperbola,
    Parabola,
    BSpline,
    Bezier,
    Offset,
    Other,
}

/// Geometry and measurements of one edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeInfo {
    pub curve: CurveKind,
    pub length: f64,
    pub start: Vec3,
    pub end: Vec3,
    /// A collapsed edge (for example at a cone apex); `curve` is `Other` and `length` 0.
    pub degenerate: bool,
}

/// Mass properties for a uniform density.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MassProps {
    /// mm^3 (0 for shapes without solids).
    pub volume: f64,
    /// mm^2.
    pub area: f64,
    /// `volume * density`.
    pub mass: f64,
    /// Centre of mass (centre of area for shapes without volume).
    pub center_of_mass: Vec3,
    /// Inertia tensor about the centre of mass, row-major, in mass units x mm^2.
    pub inertia: [[f64; 3]; 3],
}

/// Tessellation accuracy.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshTol {
    /// Maximum chord deviation (mm).
    pub linear: f64,
    /// Maximum angle between adjacent facet normals (radians).
    pub angular: f64,
}

impl Default for MeshTol {
    fn default() -> Self {
        MeshTol { linear: tol::MESH_LINEAR, angular: tol::MESH_ANGULAR }
    }
}

/// The triangles of one face inside a [`Mesh`]: `indices[first..first + count]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaceRange {
    pub face: u32,
    pub first: u32,
    pub count: u32,
}

/// The polyline of one edge, for edge display and picking.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgePolyline {
    pub edge: u32,
    pub points: Vec<[f32; 3]>,
}

/// A display mesh: triangles grouped by face (for face picking) plus edge polylines.
/// Triangles are wound counter-clockwise seen from outside the material.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub faces: Vec<FaceRange>,
    pub edges: Vec<EdgePolyline>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The face that triangle `tri` belongs to.
    pub fn face_of_triangle(&self, tri: u32) -> Option<u32> {
        let i = tri.checked_mul(3)?;
        self.faces.iter().find(|f| i >= f.first && i < f.first.saturating_add(f.count)).map(|f| f.face)
    }

    /// Enclosed volume by the divergence theorem. Positive for a closed mesh wound outward;
    /// a cheap check of orientation and completeness.
    pub fn signed_volume(&self) -> f64 {
        let p = |i: u32| self.positions.get(i as usize).map(|v| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2])));
        let (tris, _) = self.indices.as_chunks::<3>();
        tris.iter().filter_map(|&[a, b, c]| Some((p(a)?, p(b)?, p(c)?))).map(|(a, b, c)| a.dot(b.cross(c)) / 6.0).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit cube, 12 outward triangles, one face range per side.
    pub(crate) fn unit_cube() -> Mesh {
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
        let mut faces = Vec::new();
        for (f, q) in quads.iter().enumerate() {
            faces.push(FaceRange { face: f as u32, first: indices.len() as u32, count: 6 });
            indices.extend_from_slice(&[q[0], q[1], q[2], q[0], q[2], q[3]]);
        }
        Mesh { normals: vec![[0.0, 0.0, 1.0]; 8], positions, indices, faces, edges: vec![] }
    }

    #[test]
    fn signed_volume_of_cube() {
        let m = unit_cube();
        assert_eq!(m.triangle_count(), 12);
        assert!((m.signed_volume() - 1.0).abs() < 1e-12);
        let mut flipped = m.clone();
        for t in flipped.indices.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
        assert!((flipped.signed_volume() + 1.0).abs() < 1e-12);
    }

    #[test]
    fn triangle_to_face() {
        let m = unit_cube();
        assert_eq!(m.face_of_triangle(0), Some(0));
        assert_eq!(m.face_of_triangle(3), Some(1));
        assert_eq!(m.face_of_triangle(11), Some(5));
        assert_eq!(m.face_of_triangle(12), None);
        assert_eq!(m.face_of_triangle(u32::MAX), None);
    }

    #[test]
    fn signed_volume_ignores_bad_indices() {
        let mut m = unit_cube();
        m.indices.extend_from_slice(&[0, 1, 999]);
        assert!((m.signed_volume() - 1.0).abs() < 1e-12);
    }
}

/// A shape, or one face, edge or vertex of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubShape {
    Shape(crate::ShapeHandle),
    Face(crate::FaceId),
    Edge(crate::EdgeId),
    Vertex(crate::VertexId),
}

/// The shortest distance between two shapes and the nearest points on each.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Distance {
    pub value: f64,
    pub on_a: Vec3,
    pub on_b: Vec3,
}

/// What a projected edge is (hidden-line removal).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HlrKind {
    /// An edge where faces meet at an angle.
    Sharp,
    /// An edge between tangent faces (a fillet's boundary).
    Smooth,
    /// The silhouette of a curved face.
    Outline,
}

/// One edge seen in a view: a polyline in the view plane's coordinates (millimetres).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HlrCurve {
    pub kind: HlrKind,
    pub visible: bool,
    pub points: Vec<Vec2>,
}
