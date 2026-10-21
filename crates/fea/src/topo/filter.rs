//! The density filter (DEC-123): each element's density seen as the weighted mean of the
//! densities around it, so that no member is thinner than about twice the radius and the
//! design does not break into a checkerboard. With planes of symmetry, an element's
//! neighbours include the mirror images of the others, so a design that starts the same on
//! both sides stays so.
//!
//! CONTRACT (filled in by the filter's implementer):
//! - `Filter::new(centroids, volumes, radius, mirrors)`: a sparse matrix A with rows sorted by
//!   column, a_ei = w_ei V_i / S_e, w_ei = sum over the mirror group g of
//!   max(0, r - |c_e - g(c_i)|), S_e = sum_i w_ei V_i (so A keeps a constant field constant).
//!   Neighbours are found on a uniform grid of cell `radius`, one query per image; pairs that
//!   appear under several images add into one entry. A radius not above zero, lengths that
//!   differ, or a table that would take more than 512 MB, are refused in words.
//! - `apply(x)`: A x. `back(g)`: A^T g (the chain rule back through the filter).
//! - `mirror_group(mirrors)`: the reflections, the identity first: 1, 2, 4 or 8 members for 0
//!   to 3 planes, each the planes to reflect in, in turn; planes not at right angles
//!   (|n_i . n_j| >= 1e-6) or more than three are refused in words.
//! - Deterministic: the same inputs give bit-identical outputs.

use tenon_geom::Vec3;

use super::Mirror;

/// The filter as a sparse matrix, row by row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    start: Vec<u32>,
    near: Vec<u32>,
    coef: Vec<f64>,
}

impl Filter {
    pub fn new(centroids: &[Vec3], volumes: &[f64], radius: f64, mirrors: &[Mirror]) -> Result<Filter, String> {
        let _ = (centroids, volumes, radius, mirrors);
        Err("the filter is not written yet".into())
    }

    pub fn apply(&self, x: &[f64]) -> Vec<f64> {
        let _ = (&self.start, &self.near, &self.coef);
        x.to_vec()
    }

    pub fn back(&self, g: &[f64]) -> Vec<f64> {
        g.to_vec()
    }
}

/// The reflections of a group of planes, each the planes to reflect in, in turn.
pub fn mirror_group(mirrors: &[Mirror]) -> Result<Vec<Vec<Mirror>>, String> {
    let _ = mirrors;
    Ok(vec![Vec::new()])
}
