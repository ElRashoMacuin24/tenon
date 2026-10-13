//! What a mesh keeps about itself so that a ray, or a point on the screen, need not be tried
//! against all of it: a box around the mesh, a tree of boxes over its triangles, and a box
//! around each of its edges. Made when first asked for, and kept with the mesh.
//!
//! Without it, the pointer moving over an assembly of 400 components (two million triangles)
//! took 80 ms to find the face under it and 700 ms to find the edge; with it, well under a
//! millisecond (docs/performance.md).

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use tenon_geom::Vec3;

use crate::Mesh;

/// A box with its sides along the axes, in a mesh's own single precision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Box3 {
    /// The box around nothing.
    pub const EMPTY: Box3 = Box3 { min: [f32::INFINITY; 3], max: [f32::NEG_INFINITY; 3] };

    pub fn is_empty(&self) -> bool {
        (0..3).any(|i| self.min[i] > self.max[i])
    }

    /// Grows the box to hold `p` (a point that is not a number is left out).
    pub fn add(&mut self, p: [f32; 3]) {
        for (i, c) in p.iter().enumerate() {
            self.min[i] = self.min[i].min(*c);
            self.max[i] = self.max[i].max(*c);
        }
    }

    pub fn of(points: impl IntoIterator<Item = [f32; 3]>) -> Box3 {
        let mut b = Box3::EMPTY;
        for p in points {
            b.add(p);
        }
        b
    }

    pub fn union(&self, other: &Box3) -> Box3 {
        if other.is_empty() {
            return *self;
        }
        let mut b = *self;
        b.add(other.min);
        b.add(other.max);
        b
    }

    pub fn center(&self) -> Vec3 {
        Vec3::new(
            (f64::from(self.min[0]) + f64::from(self.max[0])) / 2.0,
            (f64::from(self.min[1]) + f64::from(self.max[1])) / 2.0,
            (f64::from(self.min[2]) + f64::from(self.max[2])) / 2.0,
        )
    }

    /// Half the box's diagonal: the radius of the ball about its centre that holds it.
    pub fn radius(&self) -> f64 {
        if self.is_empty() {
            return 0.0;
        }
        let d = Vec3::new(
            f64::from(self.max[0]) - f64::from(self.min[0]),
            f64::from(self.max[1]) - f64::from(self.min[1]),
            f64::from(self.max[2]) - f64::from(self.min[2]),
        );
        d.len() / 2.0
    }

    pub fn corners(&self) -> [Vec3; 8] {
        let (l, h) = (self.min, self.max);
        let at = |k: usize| {
            Vec3::new(
                f64::from(if k & 1 == 0 { l[0] } else { h[0] }),
                f64::from(if k & 2 == 0 { l[1] } else { h[1] }),
                f64::from(if k & 4 == 0 { l[2] } else { h[2] }),
            )
        };
        [at(0), at(1), at(2), at(3), at(4), at(5), at(6), at(7)]
    }

    /// The stretch of the ray from `o` along `d` (from its start onward) that is inside the
    /// box grown by `pad` all round: the distances along the ray where it enters and leaves.
    pub fn ray(&self, o: Vec3, d: Vec3, pad: f64) -> Option<(f64, f64)> {
        let (o, d) = ([o.x, o.y, o.z], [d.x, d.y, d.z]);
        let (mut t0, mut t1) = (0.0_f64, f64::INFINITY);
        for i in 0..3 {
            let (lo, hi) = (f64::from(self.min[i]) - pad, f64::from(self.max[i]) + pad);
            if d[i].abs() < 1e-300 {
                if o[i] < lo || o[i] > hi {
                    return None;
                }
                continue;
            }
            let (a, b) = ((lo - o[i]) / d[i], (hi - o[i]) / d[i]);
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            t0 = t0.max(a);
            t1 = t1.min(b);
            if t0 > t1 {
                return None;
            }
        }
        Some((t0, t1))
    }
}

/// How many triangles a leaf of the tree holds at most.
const LEAF: usize = 8;

/// A node of the tree: its box, and either its two children or (a leaf) a run of triangles.
#[derive(Clone, Copy, Debug)]
struct Node {
    bounds: Box3,
    /// The first child, or the start of the leaf's run in `Tree::order`.
    first: u32,
    /// How many triangles the leaf holds; 0 for a node with children (`first`, `first + 1`).
    count: u32,
}

#[derive(Debug)]
struct Tree {
    nodes: Vec<Node>,
    /// Triangle numbers, leaf by leaf.
    order: Vec<u32>,
}

/// Enough of a mesh to tell that it has changed since its index was made (meshes are plain
/// data that anyone may change): its sizes, where its buffers are, and its first and last
/// point, corner and edge point. A mesh that was moved, added to or given other buffers no
/// longer matches. One whose middle alone was changed in place would, and nothing does that:
/// looking at more of the mesh was most of the cost of finding what is under the pointer in
/// an assembly of a thousand bodies, each look a part of memory not otherwise touched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    positions: usize,
    indices: usize,
    edges: usize,
    buffers: [usize; 3],
    sample: u64,
}

impl Stamp {
    fn of(m: &Mesh) -> Stamp {
        let mut sample = 0xcbf2_9ce4_8422_2325_u64;
        let mut mix = |v: u32| sample = (sample ^ u64::from(v)).wrapping_mul(0x0000_0100_0000_01b3);
        for p in m.positions.first().into_iter().chain(m.positions.last()) {
            p.iter().for_each(|c| mix(c.to_bits()));
        }
        m.indices.first().into_iter().chain(m.indices.last()).for_each(|i| mix(*i));
        let ends = m.edges.first().and_then(|e| e.points.first()).into_iter().chain(m.edges.last().and_then(|e| e.points.last()));
        ends.flatten().for_each(|c| mix(c.to_bits()));
        Stamp {
            positions: m.positions.len(),
            indices: m.indices.len(),
            edges: m.edges.len(),
            buffers: [m.positions.as_ptr() as usize, m.indices.as_ptr() as usize, m.edges.as_ptr() as usize],
            sample,
        }
    }
}

/// The index of a mesh ([`Mesh::index`]).
#[derive(Debug)]
pub struct MeshIndex {
    stamp: Stamp,
    bounds: Box3,
    /// How far the boxes are grown when a ray is tried against them: a hit worked out in
    /// double precision may lie a rounding outside the box of single-precision corners.
    pad: f64,
    tree: OnceLock<Tree>,
    edges: OnceLock<Vec<Box3>>,
}

fn triangle(m: &Mesh, t: u32) -> Option<[[f32; 3]; 3]> {
    let i = (t as usize).checked_mul(3)?;
    let c = m.indices.get(i..i.checked_add(3)?)?;
    Some([*m.positions.get(*c.first()? as usize)?, *m.positions.get(*c.get(1)? as usize)?, *m.positions.get(*c.get(2)? as usize)?])
}

fn v3(p: [f32; 3]) -> Vec3 {
    Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
}

/// Where the ray from `o` along `d` crosses the triangle `abc`, as a distance along the ray
/// (Moller and Trumbore's method).
pub fn ray_triangle(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f64> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

impl Tree {
    fn of(m: &Mesh) -> Tree {
        let n = m.indices.len() / 3;
        let boxes: Vec<Box3> = (0..n).map(|t| u32::try_from(t).ok().and_then(|t| triangle(m, t)).map_or(Box3::EMPTY, Box3::of)).collect();
        // (A triangle that is not all there has no box and is never tried; it sorts as if at
        // the origin.)
        let mids: Vec<[f32; 3]> = boxes
            .iter()
            .map(|b| if b.is_empty() { [0.0; 3] } else { [(b.min[0] + b.max[0]) / 2.0, (b.min[1] + b.max[1]) / 2.0, (b.min[2] + b.max[2]) / 2.0] })
            .collect();
        let mut order: Vec<u32> = (0..u32::try_from(n).unwrap_or(u32::MAX)).collect();
        let blank = Node { bounds: Box3::EMPTY, first: 0, count: 0 };
        let mut nodes = vec![blank];
        let mut todo = vec![(0_usize, 0_usize, order.len())];
        while let Some((at, start, end)) = todo.pop() {
            let Some(run) = order.get_mut(start..end) else { continue };
            let mut bounds = Box3::EMPTY;
            let mut spread = Box3::EMPTY;
            for t in run.iter() {
                if let (Some(b), Some(c)) = (boxes.get(*t as usize), mids.get(*t as usize)) {
                    if !b.is_empty() {
                        bounds = bounds.union(b);
                    }
                    spread.add(*c);
                }
            }
            let (first, count) = if run.len() <= LEAF {
                (start, run.len())
            } else {
                // Halved where the triangles' middles are spread furthest.
                let wide = |i: usize| spread.max[i] - spread.min[i];
                let axis = if wide(0) >= wide(1) && wide(0) >= wide(2) {
                    0
                } else if wide(1) >= wide(2) {
                    1
                } else {
                    2
                };
                let half = run.len() / 2;
                let key = |t: &u32| mids.get(*t as usize).map_or(0.0, |c| c[axis]);
                run.select_nth_unstable_by(half, |a, b| key(a).total_cmp(&key(b)).then(a.cmp(b)));
                let children = nodes.len();
                nodes.push(blank);
                nodes.push(blank);
                todo.push((children, start, start + half));
                todo.push((children + 1, start + half, end));
                (children, 0)
            };
            if let Some(node) = nodes.get_mut(at) {
                *node = Node { bounds, first: u32::try_from(first).unwrap_or(u32::MAX), count: u32::try_from(count).unwrap_or(0) };
            }
        }
        Tree { nodes, order }
    }
}

impl MeshIndex {
    fn new(m: &Mesh, stamp: Stamp) -> MeshIndex {
        let bounds = Box3::of(m.positions.iter().copied()).union(&Box3::of(m.edges.iter().flat_map(|e| e.points.iter().copied())));
        let size = if bounds.is_empty() { 0.0 } else { bounds.radius() + bounds.center().len() };
        MeshIndex { stamp, bounds, pad: 1e-6 * size + 1e-9, tree: OnceLock::new(), edges: OnceLock::new() }
    }

    /// The box around the mesh: its triangles and its edges.
    pub fn bounds(&self) -> Box3 {
        self.bounds
    }

    /// Whether the ray from `o` along `d` comes within the mesh's box before `within`.
    pub fn reaches(&self, o: Vec3, d: Vec3, within: f64) -> bool {
        !self.bounds.is_empty() && self.bounds.ray(o, d, self.pad).is_some_and(|(near, _)| near <= within)
    }

    /// The triangle of `m` (the mesh this is the index of) that the ray from `o` along `d`
    /// (a unit vector) hits first, no further along than `within`: its number and the
    /// distance. Of two hit at the same distance, the one with the lower number: exactly what
    /// trying every triangle in turn would give.
    pub fn ray(&self, m: &Mesh, o: Vec3, d: Vec3, within: f64) -> Option<(u32, f64)> {
        if !self.reaches(o, d, within) {
            return None;
        }
        let tree = self.tree.get_or_init(|| Tree::of(m));
        let mut best: Option<(u32, f64)> = None;
        let mut limit = within;
        let mut todo: Vec<u32> = vec![0];
        while let Some(at) = todo.pop() {
            let Some(node) = tree.nodes.get(at as usize) else { continue };
            if node.bounds.is_empty() || !node.bounds.ray(o, d, self.pad).is_some_and(|(near, _)| near <= limit) {
                continue;
            }
            if node.count == 0 {
                todo.push(node.first);
                todo.push(node.first.saturating_add(1));
                continue;
            }
            let run = tree.order.get(node.first as usize..(node.first as usize).saturating_add(node.count as usize)).unwrap_or(&[]);
            for t in run {
                let Some([a, b, c]) = triangle(m, *t) else { continue };
                if let Some(dist) = ray_triangle(o, d, v3(a), v3(b), v3(c))
                    && dist <= limit
                    && best.is_none_or(|(bt, bd)| dist < bd || (dist == bd && *t < bt))
                {
                    best = Some((*t, dist));
                    limit = dist;
                }
            }
        }
        best
    }

    /// The box around each edge's polyline, in the order of the mesh's edges.
    pub fn edge_bounds(&self, m: &Mesh) -> &[Box3] {
        self.edges.get_or_init(|| m.edges.iter().map(|e| Box3::of(e.points.iter().copied())).collect())
    }
}

/// Where a mesh keeps its index. Not part of what the mesh is: two meshes are the same
/// whether or not they have one, a copy starts without, and it is not written out.
#[derive(Debug, Default)]
pub struct IndexSlot(Mutex<Option<Arc<MeshIndex>>>);

impl Clone for IndexSlot {
    fn clone(&self) -> Self {
        IndexSlot::default()
    }
}

impl PartialEq for IndexSlot {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Mesh {
    /// The mesh's index: made on first use, and made again if the mesh has changed since.
    pub fn index(&self) -> Arc<MeshIndex> {
        let stamp = Stamp::of(self);
        let mut slot = self.index.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(index) = slot.as_ref().filter(|i| i.stamp == stamp) {
            return Arc::clone(index);
        }
        let made = Arc::new(MeshIndex::new(self, stamp));
        *slot = Some(Arc::clone(&made));
        made
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EdgePolyline, FaceRange};

    /// A bumpy sheet of `n` by `n` squares, each two triangles, with a polyline along one side.
    fn sheet(n: u32) -> Mesh {
        let at = |i: u32, j: u32| [i as f32, j as f32, ((i * 7 + j * 13) % 5) as f32 * 0.3];
        let mut positions = Vec::new();
        for j in 0..=n {
            for i in 0..=n {
                positions.push(at(i, j));
            }
        }
        let mut indices = Vec::new();
        for j in 0..n {
            for i in 0..n {
                let a = j * (n + 1) + i;
                indices.extend_from_slice(&[a, a + 1, a + n + 2, a, a + n + 2, a + n + 1]);
            }
        }
        let count = indices.len() as u32;
        Mesh {
            normals: vec![[0.0, 0.0, 1.0]; positions.len()],
            edges: vec![EdgePolyline { edge: 0, points: (0..=n).map(|i| at(i, 0)).collect() }],
            positions,
            indices,
            faces: vec![FaceRange { face: 0, first: 0, count }],
            ..Mesh::default()
        }
    }

    /// Every triangle in turn: what the index must agree with.
    fn slow(m: &Mesh, o: Vec3, d: Vec3) -> Option<(u32, f64)> {
        let mut best: Option<(u32, f64)> = None;
        for t in 0..(m.indices.len() / 3) as u32 {
            let [a, b, c] = triangle(m, t).unwrap();
            if let Some(dist) = ray_triangle(o, d, v3(a), v3(b), v3(c))
                && best.is_none_or(|(_, bd)| dist < bd)
            {
                best = Some((t, dist));
            }
        }
        best
    }

    #[test]
    fn the_index_finds_what_trying_every_triangle_finds() {
        let m = sheet(40);
        let index = m.index();
        let mut hits = 0;
        // Rays from all round, many of them through corners and along the seams between
        // triangles, where two are hit at the same distance.
        for i in 0..60 {
            for j in 0..60 {
                let target = Vec3::new(f64::from(i) * 0.7 - 1.0, f64::from(j) * 0.7 - 1.0, 0.5);
                for from in [Vec3::new(20.0, 20.0, 90.0), Vec3::new(-30.0, 15.0, 12.0), Vec3::new(20.0, -50.0, 3.0), target + Vec3::Z * 5.0] {
                    let d = (target - from).normalized();
                    let (fast, slow) = (index.ray(&m, from, d, f64::INFINITY), slow(&m, from, d));
                    assert_eq!(fast, slow, "from {from:?} to {target:?}");
                    hits += usize::from(fast.is_some());
                }
            }
        }
        assert!(hits > 8000, "{hits}");
        // Nothing beyond the limit, and nothing behind the start.
        let (o, d) = (Vec3::new(10.2, 10.3, 50.0), -Vec3::Z);
        let (_, far) = index.ray(&m, o, d, f64::INFINITY).unwrap();
        assert!(index.ray(&m, o, d, far - 0.01).is_none());
        assert_eq!(index.ray(&m, o, d, far).map(|h| h.1), Some(far));
        assert!(index.ray(&m, o, Vec3::Z, f64::INFINITY).is_none());
        assert!(!index.reaches(Vec3::new(500.0, 0.0, 50.0), -Vec3::Z, f64::INFINITY));
    }

    #[test]
    fn boxes_hold_the_mesh_and_its_edges() {
        let m = sheet(10);
        let index = m.index();
        let b = index.bounds();
        assert_eq!((b.min, b.max), ([0.0, 0.0, 0.0], [10.0, 10.0, 1.2]));
        assert!((b.radius() - (100.0_f64 + 100.0 + 1.44).sqrt() / 2.0).abs() < 1e-6);
        assert_eq!(index.edge_bounds(&m).len(), 1);
        assert_eq!(index.edge_bounds(&m)[0].max, [10.0, 0.0, 1.2]);
        assert!(Box3::EMPTY.is_empty() && Box3::EMPTY.radius() == 0.0);
        // A ray that only grazes the padding is still let through; one clear of it is not.
        let unit = Box3 { min: [0.0; 3], max: [1.0; 3] };
        assert!(unit.ray(Vec3::new(-1.0, 1.0 + 1e-7, 0.5), Vec3::X, 1e-6).is_some());
        assert!(unit.ray(Vec3::new(-1.0, 1.1, 0.5), Vec3::X, 1e-6).is_none());
        assert_eq!(unit.ray(Vec3::new(-1.0, 0.5, 0.5), Vec3::X, 0.0), Some((1.0, 2.0)));
        assert!(unit.ray(Vec3::new(3.0, 0.5, 0.5), Vec3::X, 0.0).is_none(), "behind the start");
    }

    #[test]
    fn a_changed_mesh_gets_a_new_index_and_a_copy_starts_without() {
        let mut m = sheet(10);
        let first = m.index();
        assert!(Arc::ptr_eq(&first, &m.index()), "kept");
        // A copy is the same mesh, and has an index of its own.
        let copy = m.clone();
        assert_eq!(copy, m);
        assert!(!Arc::ptr_eq(&first, &copy.index()));
        // Moved in place: the old index would send rays to where the mesh was.
        for p in &mut m.positions {
            p[0] += 100.0;
        }
        let moved = m.index();
        assert!(!Arc::ptr_eq(&first, &moved));
        assert_eq!(moved.bounds().max[0], 110.0);
        let d = -Vec3::Z;
        assert!(moved.ray(&m, Vec3::new(105.2, 5.3, 50.0), d, f64::INFINITY).is_some());
        assert!(moved.ray(&m, Vec3::new(5.2, 5.3, 50.0), d, f64::INFINITY).is_none());
        // Added to: likewise.
        let n = m.positions.len() as u32;
        m.positions.extend([[0.0, 0.0, 9.0], [1.0, 0.0, 9.0], [0.0, 1.0, 9.0]]);
        m.indices.extend([n, n + 1, n + 2]);
        assert!(m.index().ray(&m, Vec3::new(0.2, 0.2, 50.0), d, f64::INFINITY).is_some());
    }

    #[test]
    fn a_mesh_with_bad_corners_or_none_is_indexed_without_harm() {
        let empty = Mesh::default();
        assert!(empty.index().bounds().is_empty());
        assert!(empty.index().ray(&empty, Vec3::ZERO, Vec3::Z, f64::INFINITY).is_none());
        let mut m = sheet(4);
        m.indices.extend([0, 1, 999]);
        m.positions.push([f32::NAN, 0.0, 0.0]);
        let index = m.index();
        assert_eq!(index.ray(&m, Vec3::new(1.2, 1.3, 50.0), -Vec3::Z, f64::INFINITY), slow_ok(&m, Vec3::new(1.2, 1.3, 50.0), -Vec3::Z));
    }

    fn slow_ok(m: &Mesh, o: Vec3, d: Vec3) -> Option<(u32, f64)> {
        let mut best: Option<(u32, f64)> = None;
        for t in 0..(m.indices.len() / 3) as u32 {
            let Some([a, b, c]) = triangle(m, t) else { continue };
            if let Some(dist) = ray_triangle(o, d, v3(a), v3(b), v3(c))
                && best.is_none_or(|(_, bd)| dist < bd)
            {
                best = Some((t, dist));
            }
        }
        best
    }
}
