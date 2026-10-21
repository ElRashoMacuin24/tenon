//! What must stay solid (DEC-123), and whether a part is the same on both sides of a plane.
//!
//! CONTRACT (filled in by the regions' implementer):
//! - `design(mesh, passive)`: each tetrahedron's volume (`TetMesh::volume`) and middle
//!   (`TetMesh::centroid`), with `passive` as given.
//! - `passive(problem, kept)`: true for every element with any of its ten nodes held or
//!   loaded (`Problem::held`, `Problem::forces`); for every element with a node on a boundary
//!   triangle whose mesh surface is in a kept set; and, for a kept set with a depth d, every
//!   element whose middle is within d of one of that set's boundary triangles (their corner
//!   triangles; a grid of cell max(d, element size) to find them).
//! - `asymmetry(mesh, mirror)`: how far the boundary is from its mirror image: the largest,
//!   over boundary triangle corners, of the distance from the corner's image to the nearest
//!   boundary triangle (0 for a symmetric part).

use crate::mesh::TetMesh;
use crate::problem::Problem;

use super::{Design, Mirror};

pub fn design(mesh: &TetMesh, passive: Vec<bool>) -> Design {
    let _ = (mesh, passive);
    Design::default()
}

pub fn passive(problem: &Problem, kept: &[(Vec<u32>, Option<f64>)]) -> Vec<bool> {
    let _ = kept;
    vec![false; problem.mesh.tets.len()]
}

pub fn asymmetry(mesh: &TetMesh, mirror: &Mirror) -> f64 {
    let _ = (mesh, mirror);
    0.0
}
