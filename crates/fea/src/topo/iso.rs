//! The kept material as a closed surface (DEC-123): densities at the corner nodes, the level
//! that keeps the share asked, and the surface where the density crosses it.
//!
//! CONTRACT (filled in by the surface's implementer):
//! - `node_density(mesh, x, passive)`: at each corner node the volume-weighted mean of the
//!   densities of the elements that share it; 1 at nodes of passive elements; mid-side nodes 0.
//! - `inside_volume(mesh, rho, t)`: the exact volume where a linear field (corner values on
//!   each straight tetrahedron) is at least t: one corner in, V a1 a2 a3; three in,
//!   V (1 - b1 b2 b3); two in, the prism split in three tetrahedra.
//! - `threshold_for(mesh, rho, share)`: bisection (60 passes on [1e-3, 1 - 1e-3]) for the t
//!   whose inside volume is `share` of the sum of the corner volumes.
//! - `extract(field, share, smooth)`: threshold for the share; islands (union-find over inside
//!   corners along tetrahedron sides) with no passive node dropped and counted; a load not
//!   joined to a support warned of; marching tetrahedra on the corner tetrahedra (edge
//!   vertices keyed by (a, b), a < b, worked out from a to b so neighbours weld bit for bit;
//!   triangles facing from inside to out); the boundary capped where it is inside (marching
//!   triangles on boundary triangles' corners, facing out; their vertices `fixed`); then
//!   `smooth::smooth` with `smooth` pass pairs. The share actually kept and the threshold are
//!   said.
//! - `crisp(field, t)`: an element is inside when the mean of its corner densities is >= t.
//! - `Skin::volume` (divergence theorem), `is_closed` (every directed side's reverse present
//!   exactly once), `to_mesh` (f32 positions, area-weighted normals, one face range).
//! - Deterministic: the same field gives the same skin bit for bit.

use tenon_geom::Vec3;

use crate::mesh::TetMesh;

/// The densities to make a surface from, on the mesh they belong to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Field {
    pub mesh: TetMesh,
    /// At each node (mid-side nodes 0).
    pub rho: Vec<f64>,
    pub passive_node: Vec<bool>,
    pub held_node: Vec<bool>,
    pub loaded_node: Vec<bool>,
}

/// A closed surface of triangles, each wound to face out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skin {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    /// Vertices on the part's own boundary, which smoothing leaves where they are.
    pub fixed: Vec<bool>,
}

/// A surface made for a share of the part.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extracted {
    pub skin: Skin,
    /// The share of the part's volume inside the threshold.
    pub share: f64,
    pub threshold: f64,
    /// Pieces cut off from everything that must stay, left out.
    pub islands_dropped: usize,
    /// A loaded place the kept material does not join to any support.
    pub load_not_joined: bool,
}

pub fn node_density(mesh: &TetMesh, x: &[f64], passive: &[bool]) -> Vec<f64> {
    let _ = (x, passive);
    vec![0.0; mesh.nodes.len()]
}

pub fn inside_volume(mesh: &TetMesh, rho: &[f64], t: f64) -> f64 {
    let _ = (mesh, rho, t);
    0.0
}

pub fn threshold_for(mesh: &TetMesh, rho: &[f64], share: f64) -> f64 {
    let _ = (mesh, rho, share);
    0.5
}

pub fn extract(field: &Field, share: f64, smooth: usize) -> Extracted {
    let _ = (field, share, smooth);
    Extracted::default()
}

pub fn crisp(field: &Field, t: f64) -> Vec<bool> {
    let _ = t;
    vec![false; field.mesh.tets.len()]
}

impl Skin {
    pub fn volume(&self) -> f64 {
        0.0
    }

    pub fn is_closed(&self) -> bool {
        false
    }

    pub fn to_mesh(&self) -> tenon_kernel::Mesh {
        tenon_kernel::Mesh::default()
    }
}
