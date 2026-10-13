//! Picking faces (ray against mesh triangles) and edges (screen distance to edge polylines).
//!
//! Neither looks at all of every mesh: each mesh keeps a box around itself, a tree of boxes
//! over its triangles and a box around each edge ([`Mesh::index`]), and what the ray or the
//! pointer is clear of is passed over. Nor at every body of a large scene: a tree of boxes
//! over the bodies ([`Bodies`]) leaves out whole groups of them. The answers are those of
//! trying everything.
//!
//! A mesh is taken with where it stands ([`Placed`]): the ray is brought to the mesh, not the
//! mesh to the ray, so the components of one part share one mesh and one index of it.

use std::sync::Arc;

use tenon_geom::Vec3;
use tenon_kernel::Placed;
use tenon_kernel::index::MeshIndex;

use crate::Camera;

/// A face under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceHit {
    pub body: usize,
    pub face: u32,
    pub point: Vec3,
    /// Distance along the ray.
    pub t: f64,
}

/// An edge near the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeHit {
    pub body: usize,
    pub edge: u32,
    pub point: Vec3,
    /// Distance from the pointer in pixels.
    pub pixels: f64,
}

/// A box with its sides along the axes, where the bodies stand (double precision).
#[derive(Clone, Copy, Debug)]
struct Span {
    min: Vec3,
    max: Vec3,
}

impl Span {
    const EMPTY: Span =
        Span { min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY) };

    fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    fn add(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    fn union(&self, other: &Span) -> Span {
        if other.is_empty() {
            return *self;
        }
        Span { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    fn corners(&self) -> [Vec3; 8] {
        let (l, h) = (self.min, self.max);
        let at = |k: usize| Vec3::new(if k & 1 == 0 { l.x } else { h.x }, if k & 2 == 0 { l.y } else { h.y }, if k & 4 == 0 { l.z } else { h.z });
        [at(0), at(1), at(2), at(3), at(4), at(5), at(6), at(7)]
    }

    /// How far along the ray from `o` along `d` the box (grown by `pad`) is entered, if it is.
    fn ray(&self, o: Vec3, d: Vec3, pad: f64) -> Option<f64> {
        let (o, d) = ([o.x, o.y, o.z], [d.x, d.y, d.z]);
        let (lo, hi) = ([self.min.x, self.min.y, self.min.z], [self.max.x, self.max.y, self.max.z]);
        let (mut t0, mut t1) = (0.0_f64, f64::INFINITY);
        for i in 0..3 {
            let (lo, hi) = (lo[i] - pad, hi[i] + pad);
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
        Some(t0)
    }
}

/// How many bodies a leaf of the tree over bodies holds at most.
const LEAF: usize = 4;

/// Scenes of fewer bodies than this are gone through one by one: a box to try each is as
/// little work as a tree would be.
const WORTH_A_TREE: usize = 16;

#[derive(Clone, Copy, Debug)]
struct Node {
    span: Span,
    /// The first child, or the start of the leaf's run in `Bodies::order`.
    first: u32,
    /// How many bodies the leaf holds; 0 for a node with children (`first`, `first + 1`).
    count: u32,
}

/// A tree of boxes over the bodies of a scene, each box round a body where it stands: what
/// lets a ray or the pointer pass over most of a large assembly without a look at each body.
///
/// It is of the bodies it was made from, in their order and where they stood: made again when
/// the scene changes.
#[derive(Debug)]
pub struct Bodies {
    /// A box per body (empty for a body with nothing in it).
    spans: Vec<Span>,
    nodes: Vec<Node>,
    order: Vec<u32>,
    /// What the boxes are grown by when a ray is tried: the placing of a body is worked out
    /// in double precision and its box may be a rounding short.
    pad: f64,
}

impl Bodies {
    pub fn of(meshes: &[Placed]) -> Bodies {
        let spans: Vec<Span> = meshes
            .iter()
            .map(|m| {
                let b = m.mesh.index().bounds();
                let mut span = Span::EMPTY;
                if !b.is_empty() {
                    m.corners(&b).into_iter().for_each(|p| span.add(p));
                }
                span
            })
            .collect();
        let all = spans.iter().fold(Span::EMPTY, |a, b| a.union(b));
        let size = if all.is_empty() { 0.0 } else { (all.max - all.min).len() + all.center().len() };
        let mut order: Vec<u32> =
            (0..u32::try_from(spans.len()).unwrap_or(u32::MAX)).filter(|i| spans.get(*i as usize).is_some_and(|s| !s.is_empty())).collect();
        let blank = Node { span: Span::EMPTY, first: 0, count: 0 };
        let mut nodes = vec![blank];
        let mut todo = vec![(0_usize, 0_usize, order.len())];
        while let Some((at, start, end)) = todo.pop() {
            let Some(run) = order.get_mut(start..end) else { continue };
            let (mut span, mut spread) = (Span::EMPTY, Span::EMPTY);
            for b in run.iter() {
                if let Some(s) = spans.get(*b as usize) {
                    span = span.union(s);
                    spread.add(s.center());
                }
            }
            let (first, count) = if run.len() <= LEAF {
                (start, run.len())
            } else {
                // Halved where the bodies' middles are spread furthest.
                let wide = spread.max - spread.min;
                let axis = if wide.x >= wide.y && wide.x >= wide.z {
                    0
                } else if wide.y >= wide.z {
                    1
                } else {
                    2
                };
                let half = run.len() / 2;
                let key = |b: &u32| {
                    spans.get(*b as usize).map_or(0.0, |s| {
                        let c = s.center();
                        [c.x, c.y, c.z][axis]
                    })
                };
                run.select_nth_unstable_by(half, |a, b| key(a).total_cmp(&key(b)).then(a.cmp(b)));
                let children = nodes.len();
                nodes.push(blank);
                nodes.push(blank);
                todo.push((children, start, start + half));
                todo.push((children + 1, start + half, end));
                (children, 0)
            };
            if let Some(node) = nodes.get_mut(at) {
                *node = Node { span, first: u32::try_from(first).unwrap_or(u32::MAX), count: u32::try_from(count).unwrap_or(0) };
            }
        }
        Bodies { spans, nodes, order, pad: 1e-9 * size + 1e-9 }
    }

    /// How many bodies the tree is of.
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Calls `each` with every body whose box `wanted` says may matter, passing over whole
    /// groups whose box it says cannot. `wanted` may tighten as bodies are looked at.
    fn visit(&self, mut wanted: impl FnMut(&Span) -> bool, mut each: impl FnMut(usize)) {
        let mut todo: Vec<u32> = vec![0];
        while let Some(at) = todo.pop() {
            let Some(node) = self.nodes.get(at as usize) else { continue };
            if node.span.is_empty() || !wanted(&node.span) {
                continue;
            }
            if node.count == 0 {
                todo.push(node.first);
                todo.push(node.first.saturating_add(1));
                continue;
            }
            for b in self.order.get(node.first as usize..(node.first as usize).saturating_add(node.count as usize)).unwrap_or(&[]) {
                if self.spans.get(*b as usize).is_some_and(&mut wanted) {
                    each(*b as usize);
                }
            }
        }
    }
}

/// Nearest face hit by the ray `(o, d)` (unit direction) among `meshes`. Of two hit at the
/// same distance, the one in the earlier mesh, and in it the earlier triangle.
pub fn pick_face(meshes: &[Placed], o: Vec3, d: Vec3) -> Option<FaceHit> {
    pick_face_in(None, meshes, o, d)
}

/// [`pick_face`], with the tree over `meshes` when there is one (made from these meshes, as
/// they stand).
pub fn pick_face_in(bodies: Option<&Bodies>, meshes: &[Placed], o: Vec3, d: Vec3) -> Option<FaceHit> {
    let indices: Vec<Arc<MeshIndex>> = meshes.iter().map(|m| m.mesh.index()).collect();
    face_along(of_these(bodies, meshes), meshes, &indices, o, d)
}

/// The tree, if it is of these meshes and there are enough of them for it to pay.
fn of_these<'a>(bodies: Option<&'a Bodies>, meshes: &[Placed]) -> Option<&'a Bodies> {
    bodies.filter(|b| b.len() == meshes.len() && meshes.len() >= WORTH_A_TREE)
}

fn face_along(bodies: Option<&Bodies>, meshes: &[Placed], indices: &[Arc<MeshIndex>], o: Vec3, d: Vec3) -> Option<FaceHit> {
    let best = std::cell::Cell::new(None::<FaceHit>);
    let mut try_body = |bi: usize| {
        let (Some(m), Some(index)) = (meshes.get(bi), indices.get(bi)) else { return };
        let was = best.get();
        let within = was.map_or(f64::INFINITY, |h| h.t);
        let (lo, ld) = m.ray_in(o, d);
        if let Some((triangle, dist)) = index.ray(m.mesh, lo, ld, within)
            && was.is_none_or(|h| dist < h.t || (dist == h.t && bi < h.body))
            && let Some(face) = m.mesh.face_of_triangle(triangle)
        {
            best.set(Some(FaceHit { body: bi, face, point: o + d * dist, t: dist }));
        }
    };
    match bodies {
        Some(tree) => {
            tree.visit(|span| span.ray(o, d, tree.pad).is_some_and(|near| near <= best.get().map_or(f64::INFINITY, |h| h.t)), &mut try_body)
        }
        None => (0..meshes.len()).for_each(&mut try_body),
    }
    best.get()
}

fn seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (cx, cy) = (a.0 + dx * t, a.1 + dy * t);
    (((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt(), t)
}

/// The point of segment `ab` nearest to the line through `o` along `d`.
fn nearest_on_segment(o: Vec3, d: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let u = b - a;
    let w0 = a - o;
    let (uu, ud, dd, uw, dw) = (u.dot(u), u.dot(d), d.dot(d), u.dot(w0), d.dot(w0));
    let den = uu * dd - ud * ud;
    let s = if uu <= 0.0 || den.abs() < 1e-12 * uu * dd { 0.0 } else { ((ud * dw - dd * uw) / den).clamp(0.0, 1.0) };
    a + u * s
}

/// Nearest edge within `radius` pixels of `(x, y)` that is not hidden behind a face.
pub fn pick_edge(meshes: &[Placed], camera: &Camera, w: f64, h: f64, x: f64, y: f64, radius: f64) -> Option<EdgeHit> {
    pick_edge_in(None, meshes, camera, w, h, x, y, radius)
}

/// [`pick_edge`], with the tree over `meshes` when there is one (made from these meshes, as
/// they stand).
#[allow(clippy::too_many_arguments)]
pub fn pick_edge_in(bodies: Option<&Bodies>, meshes: &[Placed], camera: &Camera, w: f64, h: f64, x: f64, y: f64, radius: f64) -> Option<EdgeHit> {
    let (o, d) = camera.ray(x, y, w, h);
    let bodies = of_these(bodies, meshes);
    let indices: Vec<Arc<MeshIndex>> = meshes.iter().map(|m| m.mesh.index()).collect();
    // Whether anything among these corners of a box can be within reach of the pointer: not
    // when the pointer is clear of where they are on the screen. (A box that is partly behind
    // the eye is not ruled out.)
    let reach = radius + 1.0;
    let near_corners = |corners: [Vec3; 8]| {
        let (mut low, mut high) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
        for corner in corners {
            let Some((px, py, _)) = camera.project(corner, w, h) else { return true };
            low = (low.0.min(px), low.1.min(py));
            high = (high.0.max(px), high.1.max(py));
        }
        x >= low.0 - reach && x <= high.0 + reach && y >= low.1 - reach && y <= high.1 + reach
    };
    // Every segment near the pointer (cheap: projection only), nearest first.
    let mut near: Vec<EdgeHit> = Vec::new();
    let mut try_body = |bi: usize| {
        let (Some(m), Some(index)) = (meshes.get(bi), indices.get(bi)) else { return };
        let bounds = index.bounds();
        if bounds.is_empty() || !near_corners(m.corners(&bounds)) {
            return;
        }
        for (e, bounds) in m.mesh.edges.iter().zip(index.edge_bounds(m.mesh)) {
            if e.points.len() > 4 && (bounds.is_empty() || !near_corners(m.corners(bounds))) {
                continue;
            }
            for seg in e.points.windows(2) {
                let (pa, pb) = (m.point(seg[0]), m.point(seg[1]));
                let (Some(a), Some(b)) = (camera.project(pa, w, h), camera.project(pb, w, h)) else { continue };
                let (dist, _) = seg_dist((x, y), (a.0, a.1), (b.0, b.1));
                if dist <= radius {
                    // The 3D point under the pointer (a screen fraction is not a 3D fraction in
                    // perspective).
                    near.push(EdgeHit { body: bi, edge: e.edge, point: nearest_on_segment(o, d, pa, pb), pixels: dist });
                }
            }
        }
    };
    match bodies {
        Some(tree) => {
            // (In the order of the bodies, as going through them one by one gives.)
            let mut which = Vec::new();
            tree.visit(|span| near_corners(span.corners()), |bi| which.push(bi));
            which.sort_unstable();
            which.into_iter().for_each(&mut try_body);
        }
        None => (0..meshes.len()).for_each(&mut try_body),
    }
    near.sort_by(|a, b| a.pixels.total_cmp(&b.pixels));
    // The nearest one not hidden by a face clearly in front of it along its own ray.
    let slack = 1e-3 * camera.distance.max(1.0);
    near.into_iter().find(|hit| match camera.project(hit.point, w, h) {
        Some((px, py, _)) => {
            let (o2, d2) = camera.ray(px, py, w, h);
            !face_along(bodies, meshes, &indices, o2, d2).is_some_and(|f| f.t + slack < (hit.point - o2).dot(d2))
        }
        None => true,
    })
}
