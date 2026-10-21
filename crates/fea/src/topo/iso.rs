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
//!   said. A corner whose density is within a ten-thousandth of the threshold is taken as that
//!   far off it (on its own side), so that the surface never runs through a corner, where it
//!   would make triangles of no area; the share said is of the densities so taken.
//! - `crisp(field, t)`: an element is inside when the mean of its corner densities is >= t.
//! - `Skin::volume` (divergence theorem), `is_closed` (every directed side's reverse present
//!   exactly once), `to_mesh` (f32 positions, area-weighted normals, one face range).
//! - Deterministic: the same field gives the same skin bit for bit.

use std::collections::HashMap;

use tenon_geom::Vec3;
use tenon_kernel::{FaceRange, Mesh};

use crate::mesh::TetMesh;

use super::smooth;

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

/// The least and most threshold looked for, and how many halvings find it.
const LOWEST: f64 = 1e-3;
const HIGHEST: f64 = 1.0 - 1e-3;
const PASSES: usize = 60;

/// How far from the threshold a corner's density is taken to be at least: a ten-thousandth,
/// finer than the thousandths the solver is given, and enough that no two points of the surface
/// fall on one another (as they would where it runs through a corner).
const NEAR: f64 = 1e-4;

/// The six sides of a tetrahedron, by its corners.
const SIDES: [(usize, usize); 6] = [(0, 1), (1, 2), (2, 0), (0, 3), (2, 3), (1, 3)];

/// The density at node `n` (nothing where it is missing or not a number).
fn value(rho: &[f64], n: u32) -> f64 {
    rho.get(n as usize).copied().filter(|v| v.is_finite()).unwrap_or(0.0)
}

/// Which nodes are corners of a tetrahedron (the others are in the middles of sides).
fn corners(mesh: &TetMesh) -> Vec<bool> {
    let mut corner = vec![false; mesh.nodes.len()];
    for t in &mesh.tets {
        for c in &t[..4] {
            if let Some(k) = corner.get_mut(*c as usize) {
                *k = true;
            }
        }
    }
    corner
}

/// The densities at the nodes, from the elements' densities `x` (in the mesh's order): at each
/// corner the mean of the elements it is a corner of, each counted as much as its volume; 1 at
/// the corners of passive elements; 0 in the middles of sides.
pub fn node_density(mesh: &TetMesh, x: &[f64], passive: &[bool]) -> Vec<f64> {
    let n = mesh.nodes.len();
    let (mut sum, mut weight) = (vec![0.0; n], vec![0.0; n]);
    for (e, t) in mesh.tets.iter().enumerate() {
        let (v, xe) = (mesh.volume(t), x.get(e).copied().filter(|x| x.is_finite()).unwrap_or(0.0));
        for c in &t[..4] {
            if let (Some(s), Some(w)) = (sum.get_mut(*c as usize), weight.get_mut(*c as usize)) {
                (*s, *w) = (*s + v * xe, *w + v);
            }
        }
    }
    let mut rho: Vec<f64> = sum.iter().zip(&weight).map(|(s, w)| if *w > 0.0 { s / w } else { 0.0 }).collect();
    for (t, _) in mesh.tets.iter().zip(passive).filter(|(_, p)| **p) {
        for c in &t[..4] {
            if let Some(r) = rho.get_mut(*c as usize) {
                *r = 1.0;
            }
        }
    }
    rho
}

/// The volume of the straight tetrahedron `a`, `b`, `c`, `d`.
fn tet_volume(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> f64 {
    ((b - a).dot((c - a).cross(d - a)) / 6.0).abs()
}

/// The volume of a straight tetrahedron (corners `p`, densities `v`, volume `whole`) where the
/// density, linear between the corners, is at least `t`.
fn cut(p: [Vec3; 4], v: [f64; 4], t: f64, whole: f64) -> f64 {
    let (mut ins, mut outs, mut ni, mut no) = ([0usize; 4], [0usize; 4], 0, 0);
    for (i, vi) in v.iter().enumerate() {
        if *vi >= t {
            ins[ni] = i;
            ni += 1;
        } else {
            outs[no] = i;
            no += 1;
        }
    }
    // (The share of the way along the side from inside corner a to outside corner b to where
    // the density is t.)
    let along = |a: usize, b: usize| (v[a] - t) / (v[a] - v[b]);
    match ni {
        0 => 0.0,
        1 => whole * along(ins[0], outs[0]) * along(ins[0], outs[1]) * along(ins[0], outs[2]),
        // (The outside corner's own small tetrahedron taken away.)
        3 => {
            let d = outs[0];
            let back = |a: usize| (t - v[d]) / (v[a] - v[d]);
            whole * (1.0 - back(ins[0]) * back(ins[1]) * back(ins[2]))
        }
        // (A prism from the side between the two inside corners to the cut: its ends are the
        // inside corners with the points where their sides to the outside ones are cut.)
        2 => {
            let at = |a: usize, b: usize| p[a] + (p[b] - p[a]) * along(a, b);
            let (a, b, c, d) = (ins[0], ins[1], outs[0], outs[1]);
            let (ac, ad, bc, bd) = (at(a, c), at(a, d), at(b, c), at(b, d));
            tet_volume(p[a], ac, ad, p[b]) + tet_volume(ac, ad, p[b], bc) + tet_volume(ad, p[b], bc, bd)
        }
        _ => whole,
    }
}

/// The volume (mm^3) where the density, linear on each element between its corners (the
/// elements taken straight), is at least `t`: exact for such a field.
pub fn inside_volume(mesh: &TetMesh, rho: &[f64], t: f64) -> f64 {
    mesh.tets
        .iter()
        .map(|tet| {
            let p = [0, 1, 2, 3].map(|i| mesh.nodes.get(tet[i] as usize).copied().unwrap_or(Vec3::ZERO));
            cut(p, [0, 1, 2, 3].map(|i| value(rho, tet[i])), t, mesh.corner_volume(tet))
        })
        .sum()
}

/// The volume of the mesh with its elements taken straight.
fn corner_total(mesh: &TetMesh) -> f64 {
    mesh.tets.iter().map(|t| mesh.corner_volume(t)).sum()
}

/// The threshold that keeps `share` of the part (of its elements taken straight): found by
/// halving, between a thousandth and a thousandth short of one.
pub fn threshold_for(mesh: &TetMesh, rho: &[f64], share: f64) -> f64 {
    let want = share * corner_total(mesh);
    let (mut lo, mut hi) = (LOWEST, HIGHEST);
    for _ in 0..PASSES {
        let mid = 0.5 * (lo + hi);
        if inside_volume(mesh, rho, mid) > want {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// The surface of what is kept of `field` for `share` of the part, smoothed `smooth` pass pairs.
pub fn extract(field: &Field, share: f64, smooth: usize) -> Extracted {
    surface(field, threshold_for(&field.mesh, &field.rho, share), smooth)
}

/// Each element's being kept or not at threshold `t`: kept when the mean of its corners'
/// densities is at least it.
pub fn crisp(field: &Field, t: f64) -> Vec<bool> {
    field.mesh.tets.iter().map(|tet| (0..4).map(|i| value(&field.rho, tet[i])).sum::<f64>() / 4.0 >= t).collect()
}

/// The root of the piece node `a` is in (halving the way up as it goes).
fn root(up: &mut [u32], mut a: u32) -> u32 {
    while up[a as usize] != a {
        let next = up[up[a as usize] as usize];
        up[a as usize] = next;
        a = next;
    }
    a
}

/// The surface at threshold `t`: what [`extract`] does once the threshold is found.
fn surface(field: &Field, t: f64, pairs: usize) -> Extracted {
    let mesh = &field.mesh;
    let n = mesh.nodes.len();
    let corner = corners(mesh);
    let flag = |v: &[bool], i: u32| v.get(i as usize) == Some(&true);
    // The corners' densities, none nearer the threshold than NEAR.
    let mut rho: Vec<f64> = (0..n)
        .map(|i| {
            let v = value(&field.rho, i as u32);
            if !corner[i] {
                0.0
            } else if v >= t {
                v.max(t + NEAR)
            } else {
                v.min(t - NEAR)
            }
        })
        .collect();
    let mut inside: Vec<bool> = (0..n).map(|i| corner[i] && rho[i] >= t).collect();

    // The pieces of what is inside: corners inside joined along the sides between them (the
    // inside of an element is all one piece, so this is exact).
    let mut up: Vec<u32> = (0..n as u32).collect();
    for tet in &mesh.tets {
        for (i, j) in SIDES {
            let (a, b) = (tet[i], tet[j]);
            if inside[a as usize] && inside[b as usize] {
                let (ra, rb) = (root(&mut up, a), root(&mut up, b));
                if ra != rb {
                    up[ra.max(rb) as usize] = ra.min(rb);
                }
            }
        }
    }
    let piece: Vec<u32> = (0..n as u32).map(|i| root(&mut up, i)).collect();
    // Each piece's being kept (it has a node that must stay) and held (it has a support), from
    // the nodes of the elements it is in.
    let in_piece = |tet: &[u32; 10]| tet[..4].iter().find(|c| inside[**c as usize]).map(|c| piece[*c as usize] as usize);
    let (mut kept, mut held) = (vec![false; n], vec![false; n]);
    for tet in &mesh.tets {
        let Some(r) = in_piece(tet) else { continue };
        for node in tet {
            kept[r] |= flag(&field.passive_node, *node);
            held[r] |= flag(&field.held_node, *node);
        }
    }
    // A loaded node is joined to a support when a kept piece with a support has it.
    let mut joined = vec![false; n];
    for tet in &mesh.tets {
        if in_piece(tet).is_some_and(|r| kept[r] && held[r]) {
            tet.iter().for_each(|node| joined[*node as usize] = true);
        }
    }
    let load_not_joined = field.loaded_node.iter().enumerate().any(|(i, l)| *l && !joined.get(i).copied().unwrap_or(false));
    // Pieces with nothing that must stay are left out.
    let mut dropped = vec![false; n];
    let mut islands_dropped = 0;
    for c in 0..n {
        if inside[c] && !kept[piece[c] as usize] {
            let r = piece[c] as usize;
            if !dropped[r] {
                dropped[r] = true;
                islands_dropped += 1;
            }
            (rho[c], inside[c]) = (0.0, false);
        }
    }

    let mut maker = Maker { nodes: &mesh.nodes, rho: &rho, t, skin: Skin::default(), crossings: HashMap::new(), corners: HashMap::new() };
    // Where the density crosses the threshold inside each element: a triangle, or a four-sided
    // piece as two.
    for tet in &mesh.tets {
        let (mut ins, mut outs, mut ni, mut no) = ([0u32; 4], [0u32; 4], 0, 0);
        for c in &tet[..4] {
            if inside[*c as usize] {
                ins[ni] = *c;
                ni += 1;
            } else {
                outs[no] = *c;
                no += 1;
            }
        }
        if ni == 0 || no == 0 {
            continue;
        }
        let mean = |s: &[u32]| s.iter().fold(Vec3::ZERO, |m, c| m + mesh.nodes[*c as usize]) * (1.0 / s.len() as f64);
        let out = mean(&outs[..no]) - mean(&ins[..ni]);
        match ni {
            1 => {
                let v = [0, 1, 2].map(|k| maker.crossing(ins[0], outs[k]));
                maker.face(v, out);
            }
            3 => {
                let v = [0, 1, 2].map(|k| maker.crossing(ins[k], outs[0]));
                maker.face(v, out);
            }
            _ => {
                // (Round the piece: each point and the next share a face of the element.)
                let q = [
                    maker.crossing(ins[0], outs[0]),
                    maker.crossing(ins[0], outs[1]),
                    maker.crossing(ins[1], outs[1]),
                    maker.crossing(ins[1], outs[0]),
                ];
                let at = |i: u32| maker.skin.positions[i as usize];
                let (first, second) = if at(q[0]).dist(at(q[2])) <= at(q[1]).dist(at(q[3])) {
                    ([q[0], q[1], q[2]], [q[0], q[2], q[3]])
                } else {
                    ([q[1], q[2], q[3]], [q[1], q[3], q[0]])
                };
                maker.face(first, out);
                maker.face(second, out);
            }
        }
    }
    // The part's own boundary where it is inside: each boundary triangle cut to its inside
    // corners, facing out as the triangle does.
    let outward = mesh.outward().unwrap_or_default();
    for (i, tri) in mesh.tris.iter().enumerate() {
        let [a, mut b, mut c] = [tri.nodes[0], tri.nodes[1], tri.nodes[2]];
        if outward.get(i).is_some_and(|s| *s < 0.0) {
            std::mem::swap(&mut b, &mut c);
        }
        let ring = [a, b, c];
        let (mut cap, mut k) = ([0u32; 4], 0);
        for j in 0..3 {
            let (u, w) = (ring[j], ring[(j + 1) % 3]);
            if inside[u as usize] {
                cap[k] = maker.corner(u);
                k += 1;
            }
            if inside[u as usize] != inside[w as usize] {
                cap[k] = maker.crossing(u, w);
                k += 1;
            }
        }
        for v in &cap[..k] {
            maker.skin.fixed[*v as usize] = true;
        }
        for j in 1..k.saturating_sub(1) {
            maker.skin.triangles.push([cap[0], cap[j], cap[j + 1]]);
        }
    }

    let mut skin = maker.skin;
    smooth::smooth(&mut skin, pairs);
    let total = corner_total(mesh);
    let share = if total > 0.0 { inside_volume(mesh, &rho, t) / total } else { 0.0 };
    Extracted { skin, share, threshold: t, islands_dropped, load_not_joined }
}

/// The skin as it is made: each vertex made once, by the side or the corner it is on.
struct Maker<'a> {
    nodes: &'a [Vec3],
    rho: &'a [f64],
    t: f64,
    skin: Skin,
    crossings: HashMap<(u32, u32), u32>,
    corners: HashMap<u32, u32>,
}

impl Maker<'_> {
    fn add(&mut self, p: Vec3) -> u32 {
        self.skin.positions.push(p);
        self.skin.fixed.push(false);
        (self.skin.positions.len() - 1) as u32
    }

    /// The vertex where the density crosses the threshold on the side between nodes `a` and
    /// `b` (one inside, one not): worked out from the lower-numbered one, so that every element
    /// with that side would find the same point to the bit.
    fn crossing(&mut self, a: u32, b: u32) -> u32 {
        let key = (a.min(b), a.max(b));
        if let Some(v) = self.crossings.get(&key) {
            return *v;
        }
        let (pa, pb) = (self.nodes[key.0 as usize], self.nodes[key.1 as usize]);
        let (ra, rb) = (self.rho[key.0 as usize], self.rho[key.1 as usize]);
        let v = self.add(pa + (pb - pa) * ((self.t - ra) / (rb - ra)));
        self.crossings.insert(key, v);
        v
    }

    /// The vertex at corner node `c`.
    fn corner(&mut self, c: u32) -> u32 {
        if let Some(v) = self.corners.get(&c) {
            return *v;
        }
        let v = self.add(self.nodes[c as usize]);
        self.corners.insert(c, v);
        v
    }

    /// Adds a triangle, wound to face along `out`.
    fn face(&mut self, [a, b, c]: [u32; 3], out: Vec3) {
        let at = |i: u32| self.skin.positions[i as usize];
        let normal = (at(b) - at(a)).cross(at(c) - at(a));
        self.skin.triangles.push(if normal.dot(out) < 0.0 { [a, c, b] } else { [a, b, c] });
    }
}

impl Skin {
    /// The volume it encloses (mm^3), by the divergence theorem: more than nothing when it is
    /// closed and faces out.
    pub fn volume(&self) -> f64 {
        let Some(o) = self.positions.first().copied() else { return 0.0 };
        let at = |i: u32| self.positions.get(i as usize).map(|p| *p - o);
        let sum: f64 = self.triangles.iter().filter_map(|[a, b, c]| Some(at(*a)?.dot(at(*b)?.cross(at(*c)?)))).sum();
        sum / 6.0
    }

    /// Whether it is closed: each side of each triangle is a side of exactly one other, the
    /// other way round.
    pub fn is_closed(&self) -> bool {
        let mut sides: Vec<(u32, u32)> = self.triangles.iter().flat_map(|[a, b, c]| [(*a, *b), (*b, *c), (*c, *a)]).collect();
        sides.sort_unstable();
        sides.windows(2).all(|w| w[0] != w[1]) && sides.iter().all(|(a, b)| a != b && sides.binary_search(&(*b, *a)).is_ok())
    }

    /// As a mesh to draw and to write out: one face, each vertex's normal the mean of its
    /// triangles' as much as their areas.
    pub fn to_mesh(&self) -> Mesh {
        let mut normals = vec![Vec3::ZERO; self.positions.len()];
        for t in &self.triangles {
            let [Some(a), Some(b), Some(c)] = t.map(|i| self.positions.get(i as usize).copied()) else { continue };
            let n = (b - a).cross(c - a);
            for i in t {
                normals[*i as usize] = normals[*i as usize] + n;
            }
        }
        let f32s = |v: Vec3| [v.x as f32, v.y as f32, v.z as f32];
        let normals = normals
            .iter()
            .map(|n| {
                let l = n.len();
                f32s(if l > 0.0 && l.is_finite() { *n * (1.0 / l) } else { Vec3::Z })
            })
            .collect();
        let indices: Vec<u32> = self.triangles.iter().flatten().copied().collect();
        let count = u32::try_from(indices.len()).unwrap_or(u32::MAX);
        Mesh {
            positions: self.positions.iter().map(|p| f32s(*p)).collect(),
            normals,
            indices,
            faces: vec![FaceRange { face: 0, first: 0, count }],
            ..Mesh::default()
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fixed run of numbers from 0 to 1, the same each time.
    pub(crate) struct Lcg(pub u64);

    impl Lcg {
        pub(crate) fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    /// A field on `mesh` with `rho(p)` at each corner node (mid-side nodes 0), the nodes for
    /// which `passive(p)` must stay, and none held or loaded.
    pub(crate) fn field(mesh: TetMesh, rho: impl Fn(Vec3) -> f64, passive: impl Fn(Vec3) -> bool) -> Field {
        let corner = corners(&mesh);
        let n = mesh.nodes.len();
        Field {
            rho: (0..n).map(|i| if corner[i] { rho(mesh.nodes[i]) } else { 0.0 }).collect(),
            passive_node: mesh.nodes.iter().map(|p| passive(*p)).collect(),
            held_node: vec![false; n],
            loaded_node: vec![false; n],
            mesh,
        }
    }

    /// A field of random densities on a box, every node to stay (so no piece is dropped).
    pub(crate) fn random_field(cells: [usize; 3], seed: u64) -> Field {
        let mesh = TetMesh::block(cells, Vec3::new(cells[0] as f64, cells[1] as f64, cells[2] as f64));
        let mut lcg = Lcg(seed);
        let values: Vec<f64> = (0..mesh.nodes.len()).map(|_| lcg.next()).collect();
        let mut f = field(mesh, |_| 0.0, |_| true);
        let corner = corners(&f.mesh);
        f.rho = values.iter().zip(&corner).map(|(v, c)| if *c { *v } else { 0.0 }).collect();
        f
    }

    fn area(skin: &Skin, t: &[u32; 3]) -> f64 {
        let [a, b, c] = t.map(|i| skin.positions[i as usize]);
        (b - a).cross(c - a).len() / 2.0
    }

    #[test]
    fn node_densities_are_means_by_volume() {
        let mut mesh = TetMesh::block([2, 1, 1], Vec3::new(2.0, 1.0, 1.0));
        // A middle node moved, so that the elements about it are of different volumes.
        let mid = mesh.tets[3][6] as usize;
        mesh.nodes[mid] = mesh.nodes[mid] + Vec3::new(0.05, 0.1, -0.08);
        let x: Vec<f64> = (0..mesh.tets.len()).map(|e| (e as f64 + 1.0) / 13.0).collect();
        let mut passive = vec![false; mesh.tets.len()];
        passive[8] = true;
        let rho = node_density(&mesh, &x, &passive);
        let corner = corners(&mesh);
        let volumes: Vec<f64> = mesh.tets.iter().map(|t| mesh.volume(t)).collect();
        assert!(volumes.iter().any(|v| (v - 1.0 / 6.0).abs() > 1e-3));
        for (n, r) in rho.iter().enumerate() {
            let of: Vec<usize> = (0..mesh.tets.len()).filter(|e| mesh.tets[*e][..4].contains(&(n as u32))).collect();
            let want = if !corner[n] {
                0.0
            } else if of.iter().any(|e| passive[*e]) {
                1.0
            } else {
                of.iter().map(|e| volumes[*e] * x[*e]).sum::<f64>() / of.iter().map(|e| volumes[*e]).sum::<f64>()
            };
            assert!((r - want).abs() < 1e-14, "node {n}: {r} against {want}");
        }
        assert!(rho.iter().zip(&corner).any(|(r, c)| *c && *r != 1.0), "not every corner is passive");
        // The same density everywhere is that density at every corner.
        let rho = node_density(&mesh, &vec![0.37; mesh.tets.len()], &[]);
        assert!(rho.iter().zip(&corner).all(|(r, c)| if *c { (r - 0.37).abs() < 1e-15 } else { *r == 0.0 }));
    }

    /// One straight tetrahedron, its middle nodes half way along its sides.
    fn one_tet() -> TetMesh {
        let p = [Vec3::new(0.2, -0.1, 0.3), Vec3::new(2.1, 0.4, -0.2), Vec3::new(0.5, 1.7, 0.1), Vec3::new(0.3, 0.6, 1.9)];
        let mut nodes = p.to_vec();
        nodes.extend(SIDES.map(|(a, b)| (p[a] + p[b]) * 0.5));
        TetMesh { nodes, tets: vec![[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]], tris: Vec::new(), lines: Vec::new() }
    }

    #[test]
    fn one_cut_tet_has_the_closed_form_volume() {
        let mesh = one_tet();
        let whole = mesh.corner_volume(&mesh.tets[0]);
        let v = [0.9, 0.2, 0.55, 0.35];
        let mut rho = vec![0.0; 10];
        rho[..4].copy_from_slice(&v);
        // The share of a simplex where a linear field is at least t: the sum over corners of
        // (v_i - t)^3 over the product of (v_i - v_j), corners at or below t left out.
        let closed = |t: f64| -> f64 {
            whole
                * (0..4)
                    .filter(|i| v[*i] > t)
                    .map(|i| (v[i] - t).powi(3) / (0..4).filter(|j| *j != i).map(|j| v[i] - v[j]).product::<f64>())
                    .sum::<f64>()
        };
        for (t, inside) in [(0.8, 1), (0.6, 1), (0.5, 2), (0.4, 2), (0.3, 3), (0.21, 3)] {
            assert_eq!(v.iter().filter(|x| **x >= t).count(), inside);
            let got = inside_volume(&mesh, &rho, t);
            assert!((got - closed(t)).abs() <= 1e-12 * whole, "t {t}: {got} against {}", closed(t));
            assert!(got > 0.0 && got < whole);
        }
        assert_eq!(inside_volume(&mesh, &rho, 0.1), whole);
        assert_eq!(inside_volume(&mesh, &rho, 0.95), 0.0);
        // The corners in any order: the same.
        let mut turned = mesh.clone();
        turned.tets[0] = [2, 0, 3, 1, 6, 7, 4, 5, 8, 9];
        for t in [0.8, 0.5, 0.3] {
            assert!((inside_volume(&turned, &rho, t) - closed(t)).abs() <= 1e-12 * whole);
        }
    }

    #[test]
    fn the_kept_volume_never_grows_with_the_threshold() {
        let f = random_field([4, 3, 3], 7);
        let total = corner_total(&f.mesh);
        let mut last = inside_volume(&f.mesh, &f.rho, -0.1);
        assert_eq!(last, total, "below every density, all of it");
        for k in 0..=240 {
            let t = -0.1 + 1.2 * f64::from(k) / 240.0;
            let now = inside_volume(&f.mesh, &f.rho, t);
            assert!(now <= last + 1e-12 * total, "t {t}: {now} after {last}");
            last = now;
        }
        assert_eq!(last, 0.0, "above every density, none");
    }

    #[test]
    fn the_matched_threshold_keeps_the_share_asked() {
        let f = random_field([4, 3, 3], 11);
        let total = corner_total(&f.mesh);
        for share in [0.05, 0.2, 0.5, 0.75, 0.95] {
            let t = threshold_for(&f.mesh, &f.rho, share);
            assert!((LOWEST..=HIGHEST).contains(&t));
            let kept = inside_volume(&f.mesh, &f.rho, t) / total;
            assert!((kept - share).abs() <= 1e-6, "share {share}: {kept} at {t}");
        }
        // More than can be kept, or less: the ends of the range.
        assert!(threshold_for(&f.mesh, &f.rho, 1.5) - LOWEST < 1e-15);
        assert!(HIGHEST - threshold_for(&f.mesh, &f.rho, -0.5) < 1e-15);
    }

    #[test]
    fn a_linear_field_cuts_a_block_flat() {
        let (l, w, h) = (8.0, 3.0, 2.0);
        let f = field(TetMesh::block([4, 3, 2], Vec3::new(l, w, h)), |p| p.x / l, |p| p.x == l);
        let e = extract(&f, 0.37, 0);
        assert!((e.threshold - 0.63).abs() < 1e-12, "{}", e.threshold);
        assert!((e.share - 0.37).abs() < 1e-12, "{}", e.share);
        assert_eq!((e.islands_dropped, e.load_not_joined), (0, false));
        let skin = &e.skin;
        assert!(skin.is_closed());
        assert!((skin.volume() - 0.37 * l * w * h).abs() < 1e-12 * l * w * h, "{}", skin.volume());
        // The cut: flat at x = 0.63 l, all of the box's section, facing toward x = 0.
        let at = 0.63 * l;
        let flat: Vec<&[u32; 3]> =
            skin.triangles.iter().filter(|t| t.iter().all(|i| (skin.positions[*i as usize].x - at).abs() < 1e-12 * l)).collect();
        assert!(!flat.is_empty());
        assert!((flat.iter().map(|t| area(skin, t)).sum::<f64>() - w * h).abs() < 1e-12 * w * h);
        for t in &flat {
            let [a, b, c] = t.map(|i| skin.positions[i as usize]);
            let n = (b - a).cross(c - a);
            assert!(n.x < 0.0 && (n.x.abs() - n.len()).abs() < 1e-12 * n.len(), "{n:?}");
        }
        // Every other triangle is on the box's sides (the caps), and every vertex is at the cut
        // or beyond it.
        let on_side = |p: Vec3| p.x == l || p.y == 0.0 || (p.y - w).abs() < 1e-12 || p.z == 0.0 || (p.z - h).abs() < 1e-12;
        assert!(skin.triangles.iter().filter(|t| !flat.contains(t)).all(|t| t.iter().all(|i| on_side(skin.positions[*i as usize]))));
        assert!(skin.positions.iter().all(|p| p.x >= at - 1e-12 * l));
        // The vertices of the caps are fixed, and only they.
        for (i, p) in skin.positions.iter().enumerate() {
            let in_cap = skin.triangles.iter().filter(|t| !flat.contains(t)).any(|t| t.contains(&(i as u32)));
            assert_eq!(skin.fixed[i], in_cap, "{p:?}");
        }
    }

    /// The density at `p` as the surface was made from it (each corner kept off the threshold
    /// `t` as [`surface`] keeps it), or None outside the mesh.
    fn density_at(f: &Field, t: f64, p: Vec3) -> Option<f64> {
        let off = |v: f64| if v >= t { v.max(t + NEAR) } else { v.min(t - NEAR) };
        let vol = |a: Vec3, b: Vec3, c: Vec3, d: Vec3| (b - a).dot((c - a).cross(d - a));
        f.mesh.tets.iter().find_map(|tet| {
            let c = [0, 1, 2, 3].map(|i| f.mesh.nodes[tet[i] as usize]);
            let whole = vol(c[0], c[1], c[2], c[3]);
            let w = [vol(p, c[1], c[2], c[3]), vol(c[0], p, c[2], c[3]), vol(c[0], c[1], p, c[3]), vol(c[0], c[1], c[2], p)].map(|v| v / whole);
            w.iter().all(|x| *x >= -1e-12).then(|| (0..4).map(|i| w[i] * off(f.rho[tet[i] as usize])).sum())
        })
    }

    #[test]
    fn the_skin_is_closed_and_faces_out() {
        for seed in [1, 2, 3] {
            // Only what is joined to the side at x = 0 stays.
            let mut f = random_field([5, 4, 3], seed);
            f.passive_node = f.mesh.nodes.iter().map(|p| p.x == 0.0).collect();
            for share in [0.2, 0.5, 0.8] {
                let e = extract(&f, share, 0);
                let skin = &e.skin;
                assert!(skin.triangles.len() > 50 && skin.is_closed(), "seed {seed}, share {share}");
                assert!(skin.volume() > 0.0, "{}", skin.volume());
                // Each triangle inside the box faces out: just behind its middle the density is
                // at least the threshold, just in front of it less (or the box has ended).
                let inner = skin.triangles.iter().filter(|t| !t.iter().all(|i| skin.fixed[*i as usize]));
                let mut looked = 0;
                for t in inner {
                    let [a, b, c] = t.map(|i| skin.positions[i as usize]);
                    let (n, m) = ((b - a).cross(c - a).normalized(), (a + b + c) * (1.0 / 3.0));
                    let (behind, ahead) = (density_at(&f, e.threshold, m - n * 1e-6), density_at(&f, e.threshold, m + n * 1e-6));
                    assert!(behind.is_some_and(|v| v >= e.threshold) && ahead.is_none_or(|v| v < e.threshold), "{behind:?} {ahead:?} at {m:?}");
                    looked += 1;
                }
                assert!(looked > 20, "{looked}");
            }
        }
    }

    #[test]
    fn the_skin_encloses_the_volume_it_says() {
        for seed in [4, 5] {
            let f = random_field([5, 4, 3], seed);
            let total = corner_total(&f.mesh);
            for share in [0.15, 0.5, 0.85] {
                let e = extract(&f, share, 0);
                assert_eq!(e.islands_dropped, 0);
                assert!((e.skin.volume() - e.share * total).abs() <= 1e-9 * total, "{} against {}", e.skin.volume(), e.share * total);
                // No corner is near the threshold here, so the share said is the field's own at
                // the threshold, and the one asked.
                assert!(f.rho.iter().all(|v| (v - e.threshold).abs() > NEAR), "seed {seed}, share {share}");
                assert!((e.share - inside_volume(&f.mesh, &f.rho, e.threshold) / total).abs() < 1e-12);
                assert!((e.share - share).abs() < 1e-6, "{} against {share}", e.share);
                // The mesher may wind a boundary triangle either way: every other one turned,
                // the skin is the same but for the order of the caps' corners.
                let mut turned = f.clone();
                for t in turned.mesh.tris.iter_mut().step_by(2) {
                    let [a, b, c, ab, bc, ca] = t.nodes;
                    t.nodes = [a, c, b, ca, bc, ab];
                }
                assert!(turned.mesh.outward().unwrap().contains(&-1.0));
                let te = extract(&turned, share, 0);
                assert!(te.skin.is_closed());
                assert_eq!((te.share, te.skin.triangles.len()), (e.share, e.skin.triangles.len()));
                assert!((te.skin.volume() - e.skin.volume()).abs() <= 1e-9 * total, "{} against {}", te.skin.volume(), e.skin.volume());
            }
        }
    }

    /// A bar 8 long in two pieces: solid to x = 2 and from x = 6, nothing from 3 to 5, and
    /// between, the density running from the one to the other.
    fn two_pieces(passive: impl Fn(Vec3) -> bool) -> Field {
        let rho = |p: Vec3| if p.x <= 2.0 || p.x >= 6.0 { 1.0 } else { 0.0 };
        field(TetMesh::block([8, 2, 2], Vec3::new(8.0, 2.0, 2.0)), rho, passive)
    }

    #[test]
    fn islands_away_from_kept_elements_are_dropped() {
        let total = 32.0;
        let mut f = two_pieces(|p| p.x == 0.0);
        f.held_node = f.passive_node.clone();
        // For 0.6 of the part the threshold is 0.6: each piece reaches 0.4 past its solid end.
        let e = extract(&f, 0.6, 0);
        assert!((e.threshold - 0.6).abs() < 1e-12, "{}", e.threshold);
        assert_eq!((e.islands_dropped, e.load_not_joined), (1, false));
        assert!(e.skin.is_closed());
        assert!((e.share - 0.3).abs() < 1e-12, "{}", e.share);
        assert!((e.skin.volume() - 0.3 * total).abs() < 1e-9 * total);
        assert!(e.skin.positions.iter().all(|p| p.x <= 2.4 + 1e-12));
        // With something to stay in each piece, neither is dropped.
        let e = extract(&two_pieces(|p| p.x == 0.0 || p.x == 8.0), 0.6, 0);
        assert_eq!(e.islands_dropped, 0);
        assert!((e.share - 0.6).abs() < 1e-12 && e.skin.is_closed());
        assert!((e.skin.volume() - 0.6 * total).abs() < 1e-9 * total);
        // Nothing to stay anywhere: both go.
        let e = extract(&two_pieces(|_| false), 0.6, 0);
        assert_eq!((e.islands_dropped, e.share, e.skin.triangles.len()), (2, 0.0, 0));
    }

    #[test]
    fn a_load_the_shape_does_not_join_to_a_support_is_warned() {
        let with = |held: fn(Vec3) -> bool, loaded: fn(Vec3) -> bool| {
            let mut f = two_pieces(|p| p.x == 0.0 || p.x == 8.0);
            f.held_node = f.mesh.nodes.iter().map(|p| held(*p)).collect();
            f.loaded_node = f.mesh.nodes.iter().map(|p| loaded(*p)).collect();
            extract(&f, 0.6, 0)
        };
        // Held at one end, loaded at the other, nothing between: warned.
        let e = with(|p| p.x == 0.0, |p| p.x == 8.0);
        assert_eq!((e.load_not_joined, e.islands_dropped), (true, 0));
        assert!(e.skin.is_closed());
        // Held and loaded on the same piece: not.
        assert!(!with(|p| p.x == 0.0, |p| p.x == 0.0 && p.y == 0.0).load_not_joined);
        // Held at both ends: not.
        assert!(!with(|p| p.x == 0.0 || p.x == 8.0, |p| p.x == 8.0).load_not_joined);
        // A load in the gap, where nothing is kept: warned.
        assert!(with(|p| p.x == 0.0, |p| p.x == 4.0 && p.y == 1.0 && p.z == 1.0).load_not_joined);
        // Nothing loaded: not.
        assert!(!with(|p| p.x == 0.0, |_| false).load_not_joined);
        // The ends joined by what is kept between them: not.
        let mut f = field(TetMesh::block([8, 2, 2], Vec3::new(8.0, 2.0, 2.0)), |_| 1.0, |p| p.x == 0.0 || p.x == 8.0);
        f.held_node = f.mesh.nodes.iter().map(|p| p.x == 0.0).collect();
        f.loaded_node = f.mesh.nodes.iter().map(|p| p.x == 8.0).collect();
        let e = extract(&f, 0.5, 0);
        assert!(!e.load_not_joined && e.skin.is_closed());
    }

    #[test]
    fn corners_at_the_threshold_make_no_flat_triangles() {
        let mesh = TetMesh::block([4, 4, 4], Vec3::new(4.0, 4.0, 4.0));
        // Densities of nothing, a half and one only, cut at a half: many corners are at it.
        let mut lcg = Lcg(99);
        let picks: Vec<f64> = mesh.nodes.iter().map(|_| [0.0, 0.5, 1.0][((lcg.next() * 3.0) as usize).min(2)]).collect();
        let mut lattice = field(mesh.clone(), |_| 0.0, |_| true);
        let corner = corners(&mesh);
        lattice.rho = picks.iter().zip(&corner).map(|(v, c)| if *c { *v } else { 0.0 }).collect();
        // And a whole layer of them: solid to x = 2, a half there, nothing beyond.
        let layer = field(
            mesh,
            |p| {
                if p.x < 2.0 {
                    1.0
                } else if p.x == 2.0 {
                    0.5
                } else {
                    0.0
                }
            },
            |_| true,
        );
        for f in [lattice, layer] {
            assert!(f.rho.iter().filter(|v| **v == 0.5).count() >= 25);
            let e = surface(&f, 0.5, 0);
            let skin = &e.skin;
            assert!(skin.triangles.len() > 20 && skin.is_closed());
            for t in &skin.triangles {
                let [a, b, c] = t.map(|i| skin.positions[i as usize]);
                let shortest = a.dist(b).min(b.dist(c)).min(c.dist(a));
                assert!(shortest > 1e-5 && area(skin, t) > 1e-10, "{a:?} {b:?} {c:?}");
            }
            let total = corner_total(&f.mesh);
            assert!((skin.volume() - e.share * total).abs() <= 1e-9 * total, "{} against {}", skin.volume(), e.share * total);
            // The share said is of the corners at the threshold taken a little inside it.
            let asked = inside_volume(&f.mesh, &f.rho, 0.5) / total;
            assert!(e.share > asked && e.share - asked < 1e-3, "{} against {asked}", e.share);
        }
    }

    #[test]
    fn the_same_field_gives_the_same_skin_bit_for_bit() {
        let f = random_field([5, 4, 3], 21);
        let (a, b) = (extract(&f, 0.45, 10), extract(&f.clone(), 0.45, 10));
        assert_eq!(a, b);
        let bits = |e: &Extracted| e.skin.positions.iter().flat_map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]).collect::<Vec<u64>>();
        assert_eq!(bits(&a), bits(&b));
        assert!(a.skin.triangles.len() > 50 && a.skin.is_closed());
        // Smoothed: the same triangles, not all where they were made.
        let raw = extract(&f, 0.45, 0);
        assert_eq!((&raw.skin.triangles, &raw.skin.fixed), (&a.skin.triangles, &a.skin.fixed));
        assert_ne!(raw.skin.positions, a.skin.positions);
        // (The caps' vertices, on the part's own boundary, stay where they were.)
        assert!(raw.skin.positions.iter().zip(&a.skin.positions).zip(&a.skin.fixed).all(|((p, q), fixed)| !fixed || p == q));
        assert!(a.skin.fixed.iter().any(|f| *f));
    }

    #[test]
    fn an_element_is_kept_when_its_corners_mean_is_at_the_threshold() {
        // Densities of x on a unit cube: each element has one, two or three corners at x = 1.
        let f = field(TetMesh::block([1, 1, 1], Vec3::new(1.0, 1.0, 1.0)), |p| p.x, |_| false);
        let means: Vec<f64> = f.mesh.tets.iter().map(|t| (0..4).map(|i| f.mesh.nodes[t[i] as usize].x).sum::<f64>() / 4.0).collect();
        let mut sorted = means.clone();
        sorted.sort_by(f64::total_cmp);
        assert_eq!(sorted, [0.25, 0.25, 0.5, 0.5, 0.75, 0.75]);
        assert_eq!(crisp(&f, 0.5), means.iter().map(|m| *m >= 0.5).collect::<Vec<_>>());
        assert_eq!(crisp(&f, 0.5).iter().filter(|k| **k).count(), 4);
        assert!(crisp(&f, 0.25).iter().all(|k| *k) && crisp(&f, 0.76).iter().all(|k| !k));
    }

    /// A box from `lo` to `hi` as twelve triangles facing out.
    fn cube(lo: Vec3, hi: Vec3) -> Skin {
        let pick = |i: u32, bit: u32, a: f64, b: f64| if i & bit == 0 { a } else { b };
        let positions = (0..8).map(|i| Vec3::new(pick(i, 1, lo.x, hi.x), pick(i, 2, lo.y, hi.y), pick(i, 4, lo.z, hi.z))).collect();
        let quads: [[u32; 4]; 6] = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let triangles = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
        Skin { positions, triangles, fixed: vec![false; 8] }
    }

    #[test]
    fn a_skin_says_its_volume_and_whether_it_is_closed() {
        let c = cube(Vec3::new(1.0, 2.0, 3.0), Vec3::new(3.0, 3.0, 7.0));
        assert!(c.is_closed());
        assert!((c.volume() - 8.0).abs() < 1e-12, "{}", c.volume());
        // Far from the origin: the same.
        let lo = Vec3::new(1e4 + 0.1, 2e4 + 0.3, 3e4 + 0.7);
        let far = cube(lo, lo + Vec3::new(2.0, 1.0, 4.0));
        let size = (far.positions[7] - far.positions[0]).x * (far.positions[7] - far.positions[0]).y * (far.positions[7] - far.positions[0]).z;
        assert!((far.volume() - size).abs() < 1e-9, "{} against {size}", far.volume());
        // Turned inside out: as much less than nothing.
        let mut inside_out = c.clone();
        inside_out.triangles.iter_mut().for_each(|t| t.swap(1, 2));
        assert!(inside_out.is_closed() && (inside_out.volume() + 8.0).abs() < 1e-12);
        // A triangle missing, one twice, one turned, one with a corner twice: not closed.
        let mut open = c.clone();
        open.triangles.pop();
        let mut twice = c.clone();
        twice.triangles.push(c.triangles[0]);
        let mut turned = c.clone();
        turned.triangles[3].swap(0, 1);
        let mut pinched = c.clone();
        pinched.triangles[0] = [0, 0, 2];
        for s in [&open, &twice, &turned, &pinched] {
            assert!(!s.is_closed(), "{:?}", s.triangles);
        }
        // Nothing: closed, and enclosing nothing.
        assert!(Skin::default().is_closed() && Skin::default().volume() == 0.0);
    }

    #[test]
    fn a_skin_becomes_one_face_of_a_mesh() {
        let mut c = cube(Vec3::ZERO, Vec3::new(2.0, 1.0, 1.0));
        // (A vertex no triangle has.)
        c.positions.push(Vec3::new(5.0, 5.0, 5.0));
        let m = c.to_mesh();
        assert_eq!((m.triangle_count(), m.positions.len(), m.normals.len()), (12, 9, 9));
        assert_eq!(m.faces, [FaceRange { face: 0, first: 0, count: 36 }]);
        assert_eq!((m.face_of_triangle(0), m.face_of_triangle(11), m.face_of_triangle(12)), (Some(0), Some(0), None));
        assert!((m.signed_volume() - 2.0).abs() < 1e-6, "{}", m.signed_volume());
        // At the first corner: the bottom and the front (2 by 1) count twice what the end does.
        let want = [-1.0 / 3.0, -2.0 / 3.0, -2.0 / 3.0];
        assert!(m.normals[0].iter().zip(want).all(|(a, b)| (f64::from(*a) - b).abs() < 1e-6), "{:?}", m.normals[0]);
        assert_eq!(m.normals[8], [0.0, 0.0, 1.0]);
        // A made skin: every triangle, every normal of length one, the positions as they are.
        let e = extract(&random_field([4, 3, 3], 5), 0.5, 3);
        let m = e.skin.to_mesh();
        assert_eq!(m.triangle_count(), e.skin.triangles.len());
        assert_eq!(m.faces, [FaceRange { face: 0, first: 0, count: 3 * e.skin.triangles.len() as u32 }]);
        assert_eq!(m.indices, e.skin.triangles.iter().flatten().copied().collect::<Vec<_>>());
        assert!(m.normals.iter().all(|n| (n.iter().map(|x| x * x).sum::<f32>().sqrt() - 1.0).abs() < 1e-6));
        assert!(m.positions.iter().zip(&e.skin.positions).all(|(a, b)| *a == [b.x as f32, b.y as f32, b.z as f32]));
        assert!((m.signed_volume() - e.skin.volume()).abs() < 1e-5 * e.skin.volume(), "{} {}", m.signed_volume(), e.skin.volume());
    }
}
