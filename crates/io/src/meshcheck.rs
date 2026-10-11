//! What a tessellated solid is like as something to print or to hand to a slicer: whether its
//! surface is closed, which of its faces overhang, and how thin its thinnest wall is. Also the
//! welded mesh (each position once) that 3MF wants.

use std::collections::HashMap;

use tenon_geom::Vec3;
use tenon_kernel::Mesh;

/// Triangles thinner than this (twice their area, mm^2) are left out: they are no surface.
const DEGENERATE: f64 = 1e-12;
/// How far inside the surface a wall-thickness ray starts (mm).
const INSET: f64 = 1e-5;
/// Triangles within this of the lowest point rest on the print bed (mm).
const ON_BED: f64 = 1e-3;
/// Most triangles in a leaf of the ray tree.
const LEAF: usize = 4;

/// A mesh with each position once: triangles that meet share their corners, so edges can be
/// matched. Triangles wind counter-clockwise seen from outside, as in the kernel's mesh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Welded {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    /// The kernel face each triangle is of.
    pub faces: Vec<u32>,
}

/// How closed a surface is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Closure {
    /// Edges with a triangle on one side only: holes in the surface.
    pub open_edges: usize,
    /// Edges where more than two triangles meet.
    pub crowded_edges: usize,
    /// Edges whose two triangles face opposite ways.
    pub flipped_edges: usize,
}

impl Closure {
    /// Every edge joins exactly two triangles that face the same way: a slicer can tell inside
    /// from outside everywhere.
    pub fn watertight(&self) -> bool {
        self.open_edges == 0 && self.crowded_edges == 0 && self.flipped_edges == 0
    }
}

/// A face that overhangs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overhang {
    pub face: u32,
    /// Area of the face's triangles past the limit, mm^2.
    pub area: f64,
    /// The flattest of them, in radians from vertical (a ceiling is a quarter turn).
    pub steepest: f64,
}

/// A face where the material behind it is thin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thin {
    pub face: u32,
    /// The least thickness found behind the face, mm.
    pub thinnest: f64,
    /// Area of the face's triangles with less than the wanted thickness behind them, mm^2.
    pub area: f64,
}

impl Welded {
    /// Welds a kernel mesh: positions that are the same become one, and triangles with no area
    /// are dropped. (The kernel meshes each face on its own but gives the faces of an edge the
    /// same points along it, so equal means equal.)
    pub fn of(mesh: &Mesh) -> Welded {
        let mut out = Welded::default();
        let mut at: HashMap<[u32; 3], u32> = HashMap::new();
        let mut index = Vec::with_capacity(mesh.positions.len());
        for p in &mesh.positions {
            // (Minus zero is zero.)
            let key = p.map(|c| (c + 0.0).to_bits());
            let next = u32::try_from(out.positions.len()).unwrap_or(u32::MAX);
            let i = *at.entry(key).or_insert(next);
            if i == next {
                out.positions.push(Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])));
            }
            index.push(i);
        }
        let (tris, _) = mesh.indices.as_chunks::<3>();
        for (t, tri) in tris.iter().enumerate() {
            let Some(v) = tri.iter().map(|i| index.get(*i as usize).copied()).collect::<Option<Vec<u32>>>() else { continue };
            let (a, b, c) = (v[0], v[1], v[2]);
            if a == b || b == c || a == c || out.normal_area(&[a, b, c]).1 <= DEGENERATE {
                continue;
            }
            out.triangles.push([a, b, c]);
            out.faces.push(u32::try_from(t).ok().and_then(|t| mesh.face_of_triangle(t)).unwrap_or(u32::MAX));
        }
        out
    }

    /// Several meshes as one (the bodies of a part), faces numbered on from one to the next.
    pub fn of_all(meshes: &[&Mesh]) -> Welded {
        let mut all = Welded::default();
        let mut face_base = 0u32;
        for m in meshes {
            let w = Welded::of(m);
            let base = u32::try_from(all.positions.len()).unwrap_or(u32::MAX);
            all.positions.extend(w.positions);
            all.triangles.extend(w.triangles.iter().map(|t| t.map(|i| i.saturating_add(base))));
            all.faces.extend(w.faces.iter().map(|f| f.saturating_add(face_base)));
            face_base = face_base.saturating_add(m.faces.iter().map(|f| f.face + 1).max().unwrap_or(0));
        }
        all
    }

    fn corner(&self, i: u32) -> Vec3 {
        self.positions.get(i as usize).copied().unwrap_or(Vec3::ZERO)
    }

    /// A triangle's unit normal and twice its area.
    fn normal_area(&self, t: &[u32; 3]) -> (Vec3, f64) {
        let (a, b, c) = (self.corner(t[0]), self.corner(t[1]), self.corner(t[2]));
        let n = (b - a).cross(c - a);
        let len = n.dot(n).sqrt();
        (if len > 0.0 { n * (1.0 / len) } else { Vec3::ZERO }, len)
    }

    /// Enclosed volume (positive when the triangles face outward), mm^3.
    pub fn volume(&self) -> f64 {
        self.triangles.iter().map(|t| self.corner(t[0]).dot(self.corner(t[1]).cross(self.corner(t[2]))) / 6.0).sum()
    }

    /// Surface area, mm^2.
    pub fn area(&self) -> f64 {
        self.triangles.iter().map(|t| self.normal_area(t).1 / 2.0).sum()
    }

    /// The box round the mesh: (min, max).
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let first = *self.positions.first()?;
        Some(self.positions.iter().fold((first, first), |(lo, hi), p| {
            (Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)), Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z)))
        }))
    }

    /// How closed the surface is.
    pub fn closure(&self) -> Closure {
        // Per edge (by its lower and higher corner): how many triangles run along it each way.
        let mut edges: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
        for t in &self.triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let e = edges.entry((a.min(b), a.max(b))).or_default();
                if a < b {
                    e.0 += 1;
                } else {
                    e.1 += 1;
                }
            }
        }
        let mut c = Closure::default();
        for (forward, backward) in edges.values() {
            match (forward + backward, forward == backward) {
                (1, _) => c.open_edges += 1,
                (2, true) => {}
                (2, false) => c.flipped_edges += 1,
                _ => c.crowded_edges += 1,
            }
        }
        c
    }

    /// The faces that lean out past `limit` (radians from vertical, `up` being up): they print
    /// over air and want support. What lies flat on the bed, at the lowest point, is not counted.
    pub fn overhangs(&self, up: Vec3, limit: f64) -> Vec<Overhang> {
        let up = up.normalized();
        let lowest = self.positions.iter().map(|p| p.dot(up)).fold(f64::INFINITY, f64::min);
        let mut by_face: HashMap<u32, Overhang> = HashMap::new();
        for (t, face) in self.triangles.iter().zip(&self.faces) {
            let (n, twice) = self.normal_area(t);
            // How far the triangle faces down: the sine of its angle from vertical.
            let down = -n.dot(up);
            if down <= limit.sin() + 1e-9 {
                continue;
            }
            if t.iter().all(|i| self.corner(*i).dot(up) - lowest <= ON_BED) {
                continue;
            }
            let o = by_face.entry(*face).or_insert(Overhang { face: *face, area: 0.0, steepest: 0.0 });
            o.area += twice / 2.0;
            o.steepest = o.steepest.max(down.clamp(-1.0, 1.0).asin());
        }
        let mut out: Vec<Overhang> = by_face.into_values().collect();
        out.sort_by_key(|o| o.face);
        out
    }

    /// The thickness of the material behind each triangle (straight in from its middle to the
    /// far side), as the least over all of them and, per face, where it is under `want`.
    pub fn walls(&self, want: f64) -> (Option<f64>, Vec<Thin>) {
        let tree = Tree::new(self);
        let mut thinnest: Option<f64> = None;
        let mut by_face: HashMap<u32, Thin> = HashMap::new();
        for (i, (t, face)) in self.triangles.iter().zip(&self.faces).enumerate() {
            let (n, twice) = self.normal_area(t);
            let middle = (self.corner(t[0]) + self.corner(t[1]) + self.corner(t[2])) * (1.0 / 3.0);
            let Some(d) = tree.nearest(self, middle - n * INSET, n * -1.0, i) else { continue };
            let thick = d + INSET;
            thinnest = Some(thinnest.map_or(thick, |x: f64| x.min(thick)));
            if thick < want {
                let e = by_face.entry(*face).or_insert(Thin { face: *face, thinnest: thick, area: 0.0 });
                e.thinnest = e.thinnest.min(thick);
                e.area += twice / 2.0;
            }
        }
        let mut out: Vec<Thin> = by_face.into_values().collect();
        out.sort_by_key(|t| t.face);
        (thinnest, out)
    }
}

/// A box tree over a mesh's triangles, for finding what a ray meets first.
struct Tree {
    nodes: Vec<Node>,
    /// Triangle indices, grouped by leaf.
    order: Vec<u32>,
}

struct Node {
    lo: Vec3,
    hi: Vec3,
    /// A leaf holds `order[first..first + count]`; an inner node's children are `first` and
    /// `first + 1` (and `count` is 0).
    first: u32,
    count: u32,
}

impl Tree {
    fn new(w: &Welded) -> Tree {
        let middles: Vec<Vec3> = w.triangles.iter().map(|t| (w.corner(t[0]) + w.corner(t[1]) + w.corner(t[2])) * (1.0 / 3.0)).collect();
        let mut tree = Tree { nodes: Vec::new(), order: (0..u32::try_from(w.triangles.len()).unwrap_or(u32::MAX)).collect() };
        if !w.triangles.is_empty() {
            tree.nodes.push(Node { lo: Vec3::ZERO, hi: Vec3::ZERO, first: 0, count: 0 });
            // (An explicit stack: a long thin part must not overflow the real one.)
            let mut todo = vec![(0usize, 0usize, tree.order.len())];
            while let Some((node, from, to)) = todo.pop() {
                let (lo, hi) = bounds_of(w, &tree.order[from..to]);
                let axis = {
                    let d = hi - lo;
                    if d.x >= d.y && d.x >= d.z {
                        0
                    } else if d.y >= d.z {
                        1
                    } else {
                        2
                    }
                };
                let key = |i: &u32| {
                    let m = middles[*i as usize];
                    [m.x, m.y, m.z][axis]
                };
                if to - from <= LEAF {
                    tree.nodes[node] = Node { lo, hi, first: from as u32, count: (to - from) as u32 };
                    continue;
                }
                let mid = from + (to - from) / 2;
                tree.order[from..to].select_nth_unstable_by(mid - from, |a, b| key(a).total_cmp(&key(b)));
                let child = tree.nodes.len();
                tree.nodes[node] = Node { lo, hi, first: child as u32, count: 0 };
                tree.nodes.push(Node { lo: Vec3::ZERO, hi: Vec3::ZERO, first: 0, count: 0 });
                tree.nodes.push(Node { lo: Vec3::ZERO, hi: Vec3::ZERO, first: 0, count: 0 });
                todo.push((child, from, mid));
                todo.push((child + 1, mid, to));
            }
        }
        tree
    }

    /// How far along the ray from `origin` in direction `dir` (a unit vector) the nearest
    /// triangle other than `skip` is.
    fn nearest(&self, w: &Welded, origin: Vec3, dir: Vec3, skip: usize) -> Option<f64> {
        let mut best: Option<f64> = None;
        let mut todo = vec![0usize];
        while let Some(n) = todo.pop() {
            let Some(node) = self.nodes.get(n) else { continue };
            if !ray_meets_box(origin, dir, node.lo, node.hi, best) {
                continue;
            }
            if node.count == 0 {
                todo.push(node.first as usize);
                todo.push(node.first as usize + 1);
                continue;
            }
            for i in &self.order[node.first as usize..(node.first + node.count) as usize] {
                if *i as usize == skip {
                    continue;
                }
                let t = w.triangles[*i as usize];
                if let Some(d) = ray_meets_triangle(origin, dir, w.corner(t[0]), w.corner(t[1]), w.corner(t[2]))
                    && best.is_none_or(|b| d < b)
                {
                    best = Some(d);
                }
            }
        }
        best
    }
}

fn bounds_of(w: &Welded, triangles: &[u32]) -> (Vec3, Vec3) {
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for t in triangles {
        for i in w.triangles[*t as usize] {
            let p = w.corner(i);
            lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
    }
    (lo, hi)
}

/// The ray reaches the box before `within` (when that is set).
fn ray_meets_box(origin: Vec3, dir: Vec3, lo: Vec3, hi: Vec3, within: Option<f64>) -> bool {
    let (mut near, mut far) = (0.0_f64, within.unwrap_or(f64::INFINITY));
    for (o, d, l, h) in [(origin.x, dir.x, lo.x, hi.x), (origin.y, dir.y, lo.y, hi.y), (origin.z, dir.z, lo.z, hi.z)] {
        if d.abs() < 1e-300 {
            if o < l - 1e-9 || o > h + 1e-9 {
                return false;
            }
            continue;
        }
        let (a, b) = ((l - o) / d, (h - o) / d);
        near = near.max(a.min(b) - 1e-9);
        far = far.min(a.max(b) + 1e-9);
        if near > far {
            return false;
        }
    }
    true
}

/// Where along the ray it passes through the triangle, if it does (Moeller and Trumbore).
fn ray_meets_triangle(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f64> {
    let (e1, e2) = (b - a, c - a);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let s = origin - a;
    let u = s.dot(p) / det;
    let q = s.cross(e1);
    let v = dir.dot(q) / det;
    if u < -1e-9 || v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    let t = e2.dot(q) / det;
    (t > 0.0).then_some(t)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use std::f64::consts::FRAC_PI_4;

    use tenon_kernel::FaceRange;

    use super::*;

    /// A box from the origin to (`x`, `y`, `z`), each side a face of its own with corners of
    /// its own, as the kernel meshes one: bottom 0, top 1, front 2, back 3, right 4, left 5.
    fn block(x: f32, y: f32, z: f32) -> Mesh {
        let c = [[0.0, 0.0, 0.0], [x, 0.0, 0.0], [x, y, 0.0], [0.0, y, 0.0], [0.0, 0.0, z], [x, 0.0, z], [x, y, z], [0.0, y, z]];
        let quads: [[usize; 4]; 6] = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [3, 0, 4, 7]];
        let mut m = Mesh::default();
        for (f, q) in quads.iter().enumerate() {
            let base = m.positions.len() as u32;
            m.positions.extend(q.iter().map(|i| c[*i]));
            m.normals.extend([[0.0; 3]; 4]);
            m.faces.push(FaceRange { face: f as u32, first: m.indices.len() as u32, count: 6 });
            m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        m
    }

    #[test]
    fn a_block_is_welded_closed_and_measured() {
        let w = Welded::of(&block(40.0, 20.0, 10.0));
        assert_eq!((w.positions.len(), w.triangles.len()), (8, 12), "24 corners become 8");
        assert_eq!(w.faces, [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5]);
        assert!(w.closure().watertight(), "{:?}", w.closure());
        assert!((w.volume() - 8000.0).abs() < 1e-9 && (w.area() - 2800.0).abs() < 1e-9);
        assert_eq!(w.bounds(), Some((Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0))));
        // A triangle with no area, a repeated corner and an index out of range are dropped.
        let mut m = block(40.0, 20.0, 10.0);
        let n = m.positions.len() as u32;
        m.positions.extend([[1.0, 1.0, 20.0], [2.0, 2.0, 20.0], [3.0, 3.0, 20.0]]);
        m.indices.extend([n, n + 1, n + 2, 0, 0, 1, 0, 1, 999]);
        assert_eq!(Welded::of(&m).triangles.len(), 12);
    }

    #[test]
    fn holes_and_turned_triangles_are_counted() {
        // The top taken off: its four edges are open.
        let mut m = block(40.0, 20.0, 10.0);
        m.indices.drain(6..12);
        let c = Welded::of(&m).closure();
        assert_eq!((c.open_edges, c.crowded_edges, c.flipped_edges, c.watertight()), (4, 0, 0, false));
        // One triangle of the top turned over: its three edges no longer agree with their neighbours.
        let mut m = block(40.0, 20.0, 10.0);
        m.indices.swap(6, 7);
        let c = Welded::of(&m).closure();
        assert_eq!((c.open_edges, c.flipped_edges, c.watertight()), (0, 3, false));
        // A fin on an edge: three triangles meet there.
        let mut m = block(40.0, 20.0, 10.0);
        let n = m.positions.len() as u32;
        m.positions.extend([[0.0, 0.0, 0.0], [40.0, 0.0, 0.0], [20.0, -5.0, -5.0]]);
        m.indices.extend([n, n + 1, n + 2]);
        let c = Welded::of(&m).closure();
        assert_eq!((c.crowded_edges, c.open_edges), (1, 2));
    }

    #[test]
    fn what_faces_down_overhangs_unless_it_lies_on_the_bed() {
        let up = Vec3::new(0.0, 0.0, 1.0);
        // A block on the bed: nothing overhangs (its bottom is on the bed).
        let w = Welded::of(&block(40.0, 20.0, 10.0));
        assert!(w.overhangs(up, FRAC_PI_4).is_empty());
        // The same block seen with X up stands on its left side; with -Z up it lies on its top.
        assert!(w.overhangs(Vec3::new(1.0, 0.0, 0.0), FRAC_PI_4).is_empty());
        assert!(w.overhangs(Vec3::new(0.0, 0.0, -1.0), FRAC_PI_4).is_empty());
        // Lifted off the bed by a foot under one end, its bottom is a ceiling over air.
        let mut m = block(40.0, 20.0, 10.0);
        for p in &mut m.positions {
            p[2] += 5.0;
        }
        let foot = block(5.0, 20.0, 5.0);
        let (base, faces) = (m.positions.len() as u32, 6);
        m.positions.extend(&foot.positions);
        m.faces.extend(foot.faces.iter().map(|f| FaceRange { face: f.face + faces, first: f.first + m.indices.len() as u32, count: f.count }));
        m.indices.extend(foot.indices.iter().map(|i| i + base));
        let found = Welded::of(&m).overhangs(up, FRAC_PI_4);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].face, 0);
        assert!((found[0].area - 800.0).abs() < 1e-9 && (found[0].steepest - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        // A wall leaning out 30 degrees from vertical is within 45 and past 20.
        let lean = (30f32).to_radians().tan() * 10.0;
        let mut m = block(40.0, 20.0, 10.0);
        for p in &mut m.positions {
            if p[2] > 5.0 {
                p[0] += lean;
            }
        }
        let w = Welded::of(&m);
        assert!(w.overhangs(up, FRAC_PI_4).is_empty());
        let found = w.overhangs(up, 20f64.to_radians());
        assert_eq!(found.iter().map(|o| o.face).collect::<Vec<_>>(), [4], "the right side, which leans out over the bed");
        assert!((found[0].steepest - 30f64.to_radians()).abs() < 1e-6, "{}", found[0].steepest.to_degrees());
    }

    #[test]
    fn the_thinnest_wall_is_measured_through_the_material() {
        // A plate 0.5 thick: thin from above and below, not through its width or length.
        let w = Welded::of(&block(40.0, 20.0, 0.5));
        let (thinnest, thin) = w.walls(0.8);
        assert!((thinnest.unwrap() - 0.5).abs() < 1e-6, "{thinnest:?}");
        assert_eq!(thin.iter().map(|t| t.face).collect::<Vec<_>>(), [0, 1]);
        assert!(thin.iter().all(|t| (t.thinnest - 0.5).abs() < 1e-6 && (t.area - 800.0).abs() < 1e-9), "{thin:?}");
        // A block 10 thick has nothing under 0.8; its thinnest way through is its height.
        let (thinnest, thin) = Welded::of(&block(40.0, 20.0, 10.0)).walls(0.8);
        assert!(thin.is_empty() && (thinnest.unwrap() - 10.0).abs() < 1e-6, "{thinnest:?}");
        // An empty mesh has no walls.
        assert_eq!(Welded::default().walls(0.8), (None, Vec::new()));
    }
}
