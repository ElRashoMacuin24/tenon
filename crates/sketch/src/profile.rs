//! Closed regions of a sketch and their conversion to kernel profiles.
//!
//! Non-construction curves form a planar graph. Its nodes are the sketch's points (points joined
//! by a coincident constraint or lying at the same place count as one node) and the places where
//! lines, arcs and circles cross or touch one another; its edges are the pieces of curve between
//! nodes. Dangling pieces and bridges are dropped, faces are traced by always turning to the next
//! piece clockwise, and bounded faces become regions. A component lying inside a face of another
//! component becomes a hole of that face.
//!
//! So a line drawn through a circle gives two regions, each half of the disc. A feature made
//! from both gets the whole disc back: pieces shared by two chosen regions are inside the profile
//! and left out, and pieces of one curve that follow one another are joined again
//! ([`profile`]).
//!
//! Splines join the graph at their ends only: where a spline crosses another curve midway,
//! neither is divided.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};
use tenon_geom::{Circle, Frame, Line, Vec2, circle_circle, line_circle, line_line_infinite, norm_angle, point_in_polygon, shoelace};
use tenon_kernel::{Curve2, Loop, Profile, Region, TaggedCurve2};

use crate::{Constraint, EntityId, Geometry, Sketch};

/// Chord tolerance used to approximate curves for face tracing (mm).
const TRACE_TOL: f64 = 0.01;
/// Points closer than this are one node (mm).
const MERGE_TOL: f64 = 1e-6;
/// Most bridge-removal passes.
const MAX_PASSES: usize = 32;
/// Most lines, arcs and circles compared pair by pair for crossings (hostile-input cap). A sketch
/// with more joins its curves at their ends only.
const MAX_CROSSING_CURVES: usize = 2000;
/// A face smaller than this is no region (mm^2).
const MIN_AREA: f64 = 1e-9;
/// Parameters closer than this are the same place on a curve.
const PARAM_EPS: f64 = 1e-12;

/// A stretch of one curve between two nodes, as a region's boundary runs along it.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub curve: EntityId,
    /// Where on the curve the stretch begins and ends, `t0 < t1`, in the curve's own parameter:
    /// a line or spline runs from 0 at its start to 1 at its end; an arc is measured in radians
    /// counter-clockwise from its start; a circle in radians counter-clockwise from +X.
    pub t0: f64,
    pub t1: f64,
    /// The stretch is the whole curve.
    pub whole: bool,
    /// The boundary runs the way the curve does (the region is on the curve's left), or against
    /// it (the region is on its right).
    pub forward: bool,
}

/// A closed region of the sketch.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchRegion {
    /// The sorted curves of the outer boundary. Survives dimension edits.
    pub key: Vec<EntityId>,
    /// Of `key`, the curves the region lies to the left of as they run from start to end. A
    /// circle or arc runs counter-clockwise, so its left is its inside. Sorted.
    pub left: Vec<EntityId>,
    /// Of `key`, the curves the region lies to the right (outside) of. Sorted. A curve that
    /// bounds the region from both sides is in both lists.
    pub right: Vec<EntityId>,
    /// Which one this is among regions with the same curves on the same sides (0 when it is the
    /// only one), counted along the lowest-numbered boundary curve.
    pub nth: u32,
    /// The outer boundary, in order around the region (counter-clockwise).
    pub outer: Vec<Piece>,
    /// Hole boundaries, each in order with the region on its left (clockwise).
    pub holes: Vec<Vec<Piece>>,
    /// Nesting depth: 0 for outermost regions, 1 inside a hole of a depth-0 region, ...
    pub depth: u32,
    /// Area of the region (holes subtracted), mm^2.
    pub area: f64,
    /// Outer boundary as a counter-clockwise polygon (for picking and display).
    pub outline: Vec<Vec2>,
    /// Hole boundaries as polygons.
    pub hole_outlines: Vec<Vec<Vec2>>,
    /// A point inside the region (not in a hole).
    pub inside: Vec2,
}

impl SketchRegion {
    /// True when `p` lies inside the region (inside the outer boundary, outside every hole).
    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(&self.outline, p) && !self.hole_outlines.iter().any(|h| point_in_polygon(h, p))
    }

    /// Every curve on the region's boundary, holes included.
    pub fn curves(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.outer.iter().chain(self.holes.iter().flatten()).map(|p| p.curve)
    }
}

/// How a feature names a region of its sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum RegionKey {
    /// The region these curves enclose on their own, whole, however other curves divide it.
    Curves(Vec<EntityId>),
    /// One region of those the sketch's curves divide it into: the one to the left of (inside)
    /// the curves in `left` and to the right of (outside) those in `right`.
    Sided {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        left: Vec<EntityId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        right: Vec<EntityId>,
        /// Which one, when several regions have these curves on these sides.
        #[serde(default, skip_serializing_if = "is_zero")]
        nth: u32,
    },
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn sorted(ids: &[EntityId]) -> Vec<EntityId> {
    let mut v = ids.to_vec();
    v.sort();
    v.dedup();
    v
}

impl RegionKey {
    /// Every curve the key names.
    pub fn curves(&self) -> Vec<EntityId> {
        match self {
            RegionKey::Curves(c) => sorted(c),
            RegionKey::Sided { left, right, .. } => sorted(&[left.as_slice(), right.as_slice()].concat()),
        }
    }
}

/// A line, arc or circle as something that can be cut.
#[derive(Clone, Copy, Debug)]
enum Carrier {
    Line(Vec2, Vec2),
    /// Centre, radius, start angle, sweep.
    Arc(Vec2, f64, f64, f64),
    Circle(Vec2, f64),
}

impl Carrier {
    fn of(sketch: &Sketch, id: EntityId) -> Option<Carrier> {
        match sketch.geometry(id)? {
            Geometry::Line { .. } => sketch.line(id).map(|(a, b)| Carrier::Line(a, b)),
            Geometry::Arc { .. } => sketch.arc(id).map(|a| Carrier::Arc(a.center, a.radius, a.start, a.sweep())),
            Geometry::Circle { .. } => sketch.circle(id).map(|(c, r)| Carrier::Circle(c, r)),
            _ => None,
        }
    }

    /// The parameter where the curve ends (it starts at 0); a circle has no ends.
    fn span(&self) -> Option<f64> {
        match *self {
            Carrier::Line(..) => Some(1.0),
            Carrier::Arc(_, _, _, sweep) => Some(sweep),
            Carrier::Circle(..) => None,
        }
    }

    /// The parameter step that is `MERGE_TOL` long.
    fn eps(&self) -> f64 {
        match *self {
            Carrier::Line(a, b) => MERGE_TOL / a.dist(b).max(MERGE_TOL),
            Carrier::Arc(_, r, ..) | Carrier::Circle(_, r) => MERGE_TOL / r.max(MERGE_TOL),
        }
    }

    /// The curve's parameter at `p`, when `p` is on the curve to within `MERGE_TOL`.
    fn param(&self, p: Vec2) -> Option<f64> {
        match *self {
            Carrier::Line(a, b) => {
                let len = a.dist(b);
                if len < MERGE_TOL {
                    return None;
                }
                let t = Line::new(a, b).param_of(p);
                let foot = a + (b - a) * t;
                (foot.dist(p) <= MERGE_TOL && t * len >= -MERGE_TOL && t * len <= len + MERGE_TOL).then_some(t.clamp(0.0, 1.0))
            }
            Carrier::Arc(c, r, start, sweep) => {
                if r < MERGE_TOL || (p.dist(c) - r).abs() > MERGE_TOL {
                    return None;
                }
                let th = norm_angle((p - c).angle() - start);
                if th <= sweep + MERGE_TOL / r {
                    Some(th.min(sweep))
                } else if (TAU - th) * r <= MERGE_TOL {
                    Some(0.0)
                } else {
                    None
                }
            }
            Carrier::Circle(c, r) => (r >= MERGE_TOL && (p.dist(c) - r).abs() <= MERGE_TOL).then(|| norm_angle((p - c).angle())),
        }
    }

    fn at(&self, t: f64) -> Vec2 {
        match *self {
            Carrier::Line(a, b) => a + (b - a) * t,
            Carrier::Arc(c, r, start, _) => Vec2::polar(c, r, start + t),
            Carrier::Circle(c, r) => Vec2::polar(c, r, t),
        }
    }

    /// A polyline from parameter `t0` to `t1`.
    fn poly(&self, t0: f64, t1: f64) -> Vec<Vec2> {
        match *self {
            Carrier::Line(..) => vec![self.at(t0), self.at(t1)],
            Carrier::Arc(_, r, ..) | Carrier::Circle(_, r) => {
                let step = if r > TRACE_TOL { 2.0 * (1.0 - TRACE_TOL / r).clamp(-1.0, 1.0).acos() } else { TAU / 8.0 };
                let n = (((t1 - t0) / step.max(1e-3)).ceil() as usize).clamp(2, 4096);
                (0..=n).map(|i| self.at(t0 + (t1 - t0) * i as f64 / n as f64)).collect()
            }
        }
    }

    /// A box round the whole curve: (min, max).
    fn bounds(&self) -> (Vec2, Vec2) {
        match *self {
            Carrier::Line(a, b) => (Vec2::new(a.x.min(b.x), a.y.min(b.y)), Vec2::new(a.x.max(b.x), a.y.max(b.y))),
            Carrier::Arc(c, r, ..) | Carrier::Circle(c, r) => (Vec2::new(c.x - r, c.y - r), Vec2::new(c.x + r, c.y + r)),
        }
    }

    fn circle(&self) -> Option<Circle> {
        match *self {
            Carrier::Arc(c, r, ..) | Carrier::Circle(c, r) => Some(Circle::new(c, r)),
            Carrier::Line(..) => None,
        }
    }

    /// Where the two curves, carried on without end, meet. Curves that touch (to within the
    /// tolerance) meet at one place, not at two a hair apart with a sliver between them.
    fn meets(&self, other: &Carrier) -> Vec<Vec2> {
        match (self, other) {
            (Carrier::Line(p, q), Carrier::Line(r, s)) => line_line_infinite(*p, *q, *r, *s).map(|(x, _, _)| vec![x]).unwrap_or_default(),
            (Carrier::Line(p, q), round) | (round, Carrier::Line(p, q)) => {
                let Some(c) = round.circle() else { return Vec::new() };
                let line = Line::new(*p, *q);
                let foot = *p + (*q - *p) * line.param_of(c.center);
                if (foot.dist(c.center) - c.radius).abs() <= MERGE_TOL {
                    return vec![foot];
                }
                line_circle(&line, &c).into_iter().map(|(x, _)| x).collect()
            }
            (a, b) => {
                let (Some(c1), Some(c2)) = (a.circle(), b.circle()) else { return Vec::new() };
                let d = c1.center.dist(c2.center);
                if d > MERGE_TOL {
                    let dir = (c2.center - c1.center) / d;
                    if (d - (c1.radius + c2.radius)).abs() <= MERGE_TOL {
                        return vec![c1.center + dir * c1.radius];
                    }
                    if (d - (c1.radius - c2.radius).abs()).abs() <= MERGE_TOL {
                        return vec![if c1.radius >= c2.radius { c1.center + dir * c1.radius } else { c1.center - dir * c1.radius }];
                    }
                }
                circle_circle(&c1, &c2)
            }
        }
    }
}

struct Edge {
    curve: EntityId,
    a: usize,
    b: usize,
    /// Polyline from node `a` to node `b`.
    poly: Vec<Vec2>,
    t0: f64,
    t1: f64,
    whole: bool,
}

impl Edge {
    fn piece(&self, forward: bool) -> Piece {
        Piece { curve: self.curve, t0: self.t0, t1: self.t1, whole: self.whole, forward }
    }
}

/// Union-find root.
fn root(parent: &mut [usize], mut i: usize) -> usize {
    let mut guard = 0;
    while let Some(&p) = parent.get(i) {
        if p == i || guard > 100_000 {
            break;
        }
        let gp = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = gp;
        }
        i = p;
        guard += 1;
    }
    i
}

/// Nodes: points merged through coincident constraints and proximity. Also where each node is,
/// and the first node number that is free.
fn nodes(sketch: &Sketch) -> (HashMap<EntityId, usize>, Vec<(usize, Vec2)>, usize) {
    let points: Vec<(EntityId, Vec2)> = sketch.entities().filter_map(|(id, _)| sketch.point(id).map(|p| (id, p))).collect();
    let index: HashMap<EntityId, usize> = points.iter().enumerate().map(|(i, (id, _))| (*id, i)).collect();
    let mut parent: Vec<usize> = (0..points.len()).collect();
    let join = |a: usize, b: usize, parent: &mut Vec<usize>| {
        let (ra, rb) = (root(parent, a), root(parent, b));
        if ra != rb
            && let Some(s) = parent.get_mut(ra)
        {
            *s = rb;
        }
    };
    for (_, c) in sketch.constraints() {
        if let Constraint::Coincident { a, b } = c
            && let (Some(&ia), Some(&ib)) = (index.get(a), index.get(b))
        {
            join(ia, ib, &mut parent);
        }
    }
    // Proximity: sort by x so only neighbours are compared.
    let mut order: Vec<usize> = (0..points.len()).collect();
    order.sort_by(|&i, &j| points[i].1.x.total_cmp(&points[j].1.x));
    for (k, &i) in order.iter().enumerate() {
        for &j in order.iter().skip(k + 1) {
            if points[j].1.x - points[i].1.x > MERGE_TOL {
                break;
            }
            if points[i].1.near(points[j].1, MERGE_TOL) {
                join(i, j, &mut parent);
            }
        }
    }
    let mut out = HashMap::new();
    let mut places = Vec::with_capacity(points.len());
    for (i, (id, p)) in points.iter().enumerate() {
        let r = root(&mut parent, i);
        out.insert(*id, r);
        places.push((r, *p));
    }
    (out, places, points.len())
}

fn angle_out(poly: &[Vec2], from_end: bool) -> f64 {
    let (p0, p1) = if from_end {
        (poly.last().copied().unwrap_or_default(), poly.len().checked_sub(2).and_then(|i| poly.get(i)).copied().unwrap_or_default())
    } else {
        (poly.first().copied().unwrap_or_default(), poly.get(1).copied().unwrap_or_default())
    };
    (p1 - p0).y.atan2((p1 - p0).x)
}

/// Faces of the graph as lists of half-edges `(edge index, forward)`.
fn trace(edges: &[Edge], alive: &[bool]) -> Vec<Vec<(usize, bool)>> {
    // Outgoing half-edges per node, sorted by angle.
    let mut out: BTreeMap<usize, Vec<(f64, usize, bool)>> = BTreeMap::new();
    for (i, e) in edges.iter().enumerate() {
        if !alive.get(i).copied().unwrap_or(false) {
            continue;
        }
        out.entry(e.a).or_default().push((angle_out(&e.poly, false), i, true));
        out.entry(e.b).or_default().push((angle_out(&e.poly, true), i, false));
    }
    for v in out.values_mut() {
        v.sort_by(|x, y| x.0.total_cmp(&y.0));
    }
    let mut used: BTreeSet<(usize, bool)> = BTreeSet::new();
    let mut faces = Vec::new();
    for (i, _) in edges.iter().enumerate().filter(|(i, _)| alive.get(*i).copied().unwrap_or(false)) {
        for fwd in [true, false] {
            if used.contains(&(i, fwd)) {
                continue;
            }
            let mut face = Vec::new();
            let (mut cur, mut dir) = (i, fwd);
            for _ in 0..=edges.len() * 2 {
                if !used.insert((cur, dir)) {
                    break;
                }
                face.push((cur, dir));
                let Some(e) = edges.get(cur) else { break };
                // Arrive at `v`; the twin leaves `v` back along this edge.
                let v = if dir { e.b } else { e.a };
                let Some(list) = out.get(&v) else { break };
                let Some(pos) = list.iter().position(|(_, j, f)| *j == cur && *f == !dir) else { break };
                // Next clockwise from the twin: the previous entry in counter-clockwise order.
                let next = if pos == 0 { list.len() - 1 } else { pos - 1 };
                let Some(&(_, j, f)) = list.get(next) else { break };
                (cur, dir) = (j, f);
            }
            faces.push(face);
        }
    }
    faces
}

fn face_polygon(edges: &[Edge], face: &[(usize, bool)]) -> Vec<Vec2> {
    let mut poly = Vec::new();
    for &(i, fwd) in face {
        if let Some(e) = edges.get(i) {
            let pts: Box<dyn Iterator<Item = &Vec2>> = if fwd { Box::new(e.poly.iter()) } else { Box::new(e.poly.iter().rev()) };
            for p in pts {
                if poly.last() != Some(p) {
                    poly.push(*p);
                }
            }
        }
    }
    if poly.len() > 1 && poly.first() == poly.last() {
        poly.pop();
    }
    poly
}

fn face_pieces(edges: &[Edge], face: &[(usize, bool)]) -> Vec<Piece> {
    face.iter().filter_map(|(i, fwd)| edges.get(*i).map(|e| e.piece(*fwd))).collect()
}

struct Face {
    pieces: Vec<Piece>,
    poly: Vec<Vec2>,
    area: f64,
    component: usize,
}

/// A point inside the counter-clockwise `outline` and outside every hole: just left of the
/// middle of one of its sides.
fn inner_point(outline: &[Vec2], holes: &[Vec<Vec2>]) -> Vec2 {
    let n = outline.len();
    for step in [1e-3_f64, 1e-5, 1e-7] {
        for i in 0..n {
            let (a, b) = (outline[i], outline[(i + 1) % n]);
            let d = b - a;
            let len = d.len();
            if len < 1e-9 {
                continue;
            }
            let p = (a + b) * 0.5 + d.perp() * (step.min(len / 4.0) / len);
            if point_in_polygon(outline, p) && !holes.iter().any(|h| point_in_polygon(h, p)) {
                return p;
            }
        }
    }
    outline.first().copied().unwrap_or_default()
}

/// The node at `p`: one already there, or a new one.
fn node_at(p: Vec2, places: &mut Vec<(usize, Vec2)>, next: &mut usize) -> usize {
    if let Some((n, _)) = places.iter().find(|(_, q)| q.near(p, MERGE_TOL)) {
        return *n;
    }
    let n = *next;
    *next += 1;
    places.push((n, p));
    n
}

/// Where each line, arc and circle is cut by the others: sorted parameters with their places.
fn cuts(curves: &[(EntityId, Carrier, Option<(Vec2, Vec2)>)]) -> Vec<Vec<(f64, Vec2)>> {
    let mut out: Vec<Vec<(f64, Vec2)>> = vec![Vec::new(); curves.len()];
    if curves.len() > MAX_CROSSING_CURVES {
        return out;
    }
    let boxes: Vec<(Vec2, Vec2)> = curves.iter().map(|c| c.1.bounds()).collect();
    let apart = |i: usize, j: usize| {
        let ((lo, hi), (lo2, hi2)) = (boxes[i], boxes[j]);
        lo.x > hi2.x + MERGE_TOL || lo2.x > hi.x + MERGE_TOL || lo.y > hi2.y + MERGE_TOL || lo2.y > hi.y + MERGE_TOL
    };
    // A cut at a curve's own end is no cut: the end is a node already.
    let add = |i: usize, t: f64, p: Vec2, out: &mut Vec<Vec<(f64, Vec2)>>| {
        let c = &curves[i].1;
        if c.span().is_none_or(|span| t > c.eps() && t < span - c.eps()) {
            out[i].push((t, p));
        }
    };
    for i in 0..curves.len() {
        for j in i + 1..curves.len() {
            if apart(i, j) {
                continue;
            }
            let (a, b) = (&curves[i].1, &curves[j].1);
            for p in a.meets(b) {
                if let (Some(ta), Some(tb)) = (a.param(p), b.param(p)) {
                    add(i, ta, p, &mut out);
                    add(j, tb, p, &mut out);
                }
            }
            // An end resting on the other curve (a T) is a cut there even when the curves, a
            // hair apart, do not cross.
            for (from, onto) in [(j, i), (i, j)] {
                if let Some((s, e)) = curves[from].2 {
                    for end in [s, e] {
                        if let Some(t) = curves[onto].1.param(end) {
                            add(onto, t, end, &mut out);
                        }
                    }
                }
            }
        }
    }
    for (i, list) in out.iter_mut().enumerate() {
        let c = &curves[i].1;
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        list.dedup_by(|later, earlier| later.0 - earlier.0 <= c.eps());
        // Round a circle the last cut may be the first again.
        if c.span().is_none()
            && list.len() > 1
            && let (Some(first), Some(last)) = (list.first(), list.last())
            && first.0 + TAU - last.0 <= c.eps()
        {
            list.pop();
        }
    }
    out
}

/// The closed regions bounded by the sketch's curves, or by those in `only`.
fn arrangement(sketch: &Sketch, only: Option<&BTreeSet<EntityId>>) -> Vec<SketchRegion> {
    let (node_of, mut places, mut next_node) = nodes(sketch);
    let used = |id: EntityId, construction: bool| !construction && only.is_none_or(|o| o.contains(&id));

    // Lines, arcs and circles cut one another where they cross or touch.
    let cuttable: Vec<(EntityId, Carrier, Option<(Vec2, Vec2)>)> = sketch
        .entities()
        .filter(|(id, e)| used(*id, e.construction))
        .filter_map(|(id, _)| {
            let c = Carrier::of(sketch, id)?;
            Some((id, c, c.span().map(|span| (c.at(0.0), c.at(span)))))
        })
        .collect();
    let cut_at: HashMap<EntityId, Vec<(f64, Vec2)>> = cuttable.iter().map(|c| c.0).zip(cuts(&cuttable)).collect();

    let mut edges = Vec::new();
    let mut loops: Vec<Face> = Vec::new(); // uncut circles: each is its own component
    for (id, e) in sketch.entities() {
        if !used(id, e.construction) {
            continue;
        }
        let ends = match &e.geometry {
            Geometry::Line { start, end } => Some((*start, *end)),
            Geometry::Arc { start, end, .. } => Some((*start, *end)),
            Geometry::Spline { poles, .. } => poles.first().zip(poles.last()).map(|(a, b)| (*a, *b)),
            Geometry::Circle { .. } => None,
            Geometry::Point { .. } => continue,
        };
        let carrier = Carrier::of(sketch, id);
        let cuts = cut_at.get(&id).map(Vec::as_slice).unwrap_or_default();
        let end_nodes = ends.and_then(|(s, t)| node_of.get(&s).copied().zip(node_of.get(&t).copied()));
        match (carrier, cuts.is_empty()) {
            // Cut: a piece from each cut to the next.
            (Some(c), false) => {
                let mut stops: Vec<(f64, usize)> = cuts.iter().map(|(t, p)| (*t, node_at(*p, &mut places, &mut next_node))).collect();
                match (c.span(), end_nodes) {
                    (Some(span), Some((a, b))) => {
                        stops.insert(0, (0.0, a));
                        stops.push((span, b));
                    }
                    (Some(_), None) => continue,
                    // Round the circle and back to the first cut.
                    (None, _) => stops.push((stops[0].0 + TAU, stops[0].1)),
                }
                let whole = c.span().is_none() && cuts.len() == 1;
                for w in stops.windows(2) {
                    let ((t0, a), (t1, b)) = (w[0], w[1]);
                    edges.push(Edge { curve: id, a, b, poly: c.poly(t0, t1), t0, t1, whole });
                }
            }
            // An uncut circle.
            (Some(c), true) if c.span().is_none() => {
                let mut poly = sketch.tessellate(id, TRACE_TOL);
                if poly.len() > 1 && poly.first().zip(poly.last()).is_some_and(|(a, b)| a.near(*b, MERGE_TOL)) {
                    poly.pop();
                }
                let area = shoelace(&poly).abs();
                loops.push(Face { pieces: vec![Piece { curve: id, t0: 0.0, t1: TAU, whole: true, forward: true }], poly, area, component: 0 });
            }
            // An uncut line or arc, or a spline: one edge between its ends.
            (c, _) => {
                let Some((a, b)) = end_nodes else { continue };
                let poly = sketch.tessellate(id, TRACE_TOL);
                if poly.len() < 2 {
                    continue;
                }
                let t1 = c.and_then(|c| c.span()).unwrap_or(1.0);
                edges.push(Edge { curve: id, a, b, poly, t0: 0.0, t1, whole: true });
            }
        }
    }

    // Drop dangling edges (degree-1 nodes) repeatedly, then bridges.
    let mut alive = vec![true; edges.len()];
    let mut faces: Vec<Vec<(usize, bool)>> = Vec::new();
    for _ in 0..MAX_PASSES {
        loop {
            let mut degree: HashMap<usize, usize> = HashMap::new();
            for (_, e) in edges.iter().enumerate().filter(|(i, _)| alive[*i]) {
                *degree.entry(e.a).or_default() += 1;
                *degree.entry(e.b).or_default() += 1;
            }
            let mut changed = false;
            for (i, e) in edges.iter().enumerate() {
                if alive[i] && e.a != e.b && (degree.get(&e.a) == Some(&1) || degree.get(&e.b) == Some(&1)) {
                    alive[i] = false;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        faces = trace(&edges, &alive);
        let mut bridge = false;
        for f in &faces {
            let mut seen = BTreeSet::new();
            for (i, _) in f {
                if !seen.insert(*i) {
                    alive[*i] = false;
                    bridge = true;
                }
            }
        }
        if !bridge {
            break;
        }
    }

    // Components of the edge graph (by node).
    let mut parent: Vec<usize> = (0..edges.len()).collect();
    let mut by_node: HashMap<usize, usize> = HashMap::new();
    for (i, e) in edges.iter().enumerate().filter(|(i, _)| alive[*i]) {
        for n in [e.a, e.b] {
            match by_node.get(&n) {
                Some(&j) => {
                    let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                    if ri != rj
                        && let Some(s) = parent.get_mut(ri)
                    {
                        *s = rj;
                    }
                }
                None => {
                    by_node.insert(n, i);
                }
            }
        }
    }

    // Bounded faces (positive area) and each component's outer outline (negative area): the
    // outline as a counter-clockwise polygon, and its pieces as a region round it sees them.
    let mut bounded: Vec<Face> = Vec::new();
    let mut outlines: BTreeMap<usize, (Vec<Vec2>, Vec<Piece>)> = BTreeMap::new();
    for f in &faces {
        let poly = face_polygon(&edges, f);
        let area = shoelace(&poly);
        let Some(&(first, _)) = f.first() else { continue };
        let component = root(&mut parent, first);
        let pieces = face_pieces(&edges, f);
        if area > MIN_AREA {
            bounded.push(Face { pieces, poly, area, component });
        } else if area < -MIN_AREA {
            let mut p = poly;
            p.reverse();
            outlines.insert(component, (p, pieces));
        }
    }
    // Circles are components of their own.
    let base = edges.len();
    for (k, mut c) in loops.into_iter().enumerate() {
        c.component = base + k;
        let round: Vec<Piece> = c.pieces.iter().map(|p| Piece { forward: false, ..p.clone() }).collect();
        outlines.insert(c.component, (c.poly.clone(), round));
        bounded.push(c);
    }

    // Parent face of each component: the smallest bounded face of another component containing it.
    let mut parent_face: BTreeMap<usize, usize> = BTreeMap::new();
    for (comp, (outline, _)) in &outlines {
        let Some(sample) = outline.first() else { continue };
        let best = bounded
            .iter()
            .enumerate()
            .filter(|(_, f)| f.component != *comp && point_in_polygon(&f.poly, *sample))
            .min_by(|a, b| a.1.area.total_cmp(&b.1.area))
            .map(|(i, _)| i);
        if let Some(i) = best {
            parent_face.insert(*comp, i);
        }
    }
    let depth_of = |face: usize| -> u32 {
        let mut d = 0;
        let mut comp = bounded.get(face).map(|f| f.component);
        while let Some(c) = comp {
            match parent_face.get(&c) {
                Some(&pf) if d < 64 => {
                    d += 1;
                    comp = bounded.get(pf).map(|f| f.component);
                }
                _ => break,
            }
        }
        d
    };

    let mut out = Vec::new();
    for (i, f) in bounded.iter().enumerate() {
        let mut holes = Vec::new();
        let mut hole_outlines = Vec::new();
        let mut area = f.area;
        for (comp, pf) in &parent_face {
            if *pf == i
                && let Some((poly, pieces)) = outlines.get(comp)
            {
                area -= shoelace(poly).abs();
                holes.push(pieces.clone());
                hole_outlines.push(poly.clone());
            }
        }
        let side = |forward: bool| sorted(&f.pieces.iter().filter(|p| p.forward == forward).map(|p| p.curve).collect::<Vec<_>>());
        let inside = inner_point(&f.poly, &hole_outlines);
        out.push(SketchRegion {
            key: sorted(&f.pieces.iter().map(|p| p.curve).collect::<Vec<_>>()),
            left: side(true),
            right: side(false),
            nth: 0,
            outer: f.pieces.clone(),
            holes,
            depth: depth_of(i),
            area,
            outline: f.poly.clone(),
            hole_outlines,
            inside,
        });
    }
    // Regions with the same curves on the same sides are told apart by where they come along
    // the lowest-numbered curve of their boundary.
    let anchor = |r: &SketchRegion| r.outer.iter().map(|p| (p.curve, p.t0)).min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    out.sort_by(|a, b| {
        (&a.key, &a.left, &a.right).cmp(&(&b.key, &b.left, &b.right)).then_with(|| match (anchor(a), anchor(b)) {
            (Some(x), Some(y)) => x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)),
            _ => std::cmp::Ordering::Equal,
        })
    });
    for i in 1..out.len() {
        let (before, rest) = out.split_at_mut(i);
        if let (Some(prev), Some(r)) = (before.last(), rest.first_mut())
            && (&prev.key, &prev.left, &prev.right) == (&r.key, &r.left, &r.right)
        {
            r.nth = prev.nth + 1;
        }
    }
    out
}

/// All closed regions of the sketch (construction geometry excluded).
pub fn regions(sketch: &Sketch) -> Vec<SketchRegion> {
    arrangement(sketch, None)
}

/// The regions extruded when the user picks nothing: every region at even nesting depth (a
/// plate with holes, but not the discs filling the holes).
pub fn default_regions(all: &[SketchRegion]) -> Vec<&SketchRegion> {
    all.iter().filter(|r| r.depth % 2 == 0).collect()
}

/// The regions of `all` (every region of `sketch`) that `key` names: none when the sketch no
/// longer has such a region.
pub fn find<'r>(sketch: &Sketch, all: &'r [SketchRegion], key: &RegionKey) -> Vec<&'r SketchRegion> {
    match key {
        RegionKey::Sided { left, right, nth } => {
            let (l, r) = (sorted(left), sorted(right));
            all.iter().filter(|x| x.left == l && x.right == r && x.nth == *nth).collect()
        }
        RegionKey::Curves(curves) => {
            // What the curves enclose by themselves, and of the regions in it the outermost:
            // deeper ones belong to islands inside, which are its holes.
            let k = sorted(curves);
            let only: BTreeSet<EntityId> = k.iter().copied().collect();
            let alone: Vec<SketchRegion> = arrangement(sketch, Some(&only)).into_iter().filter(|r| r.key == k).collect();
            let within: Vec<&SketchRegion> = all.iter().filter(|r| alone.iter().any(|a| a.contains(r.inside))).collect();
            let top = within.iter().map(|r| r.depth).min();
            within.into_iter().filter(|r| Some(r.depth) == top).collect()
        }
    }
}

/// The key to store for `region`, one of `all` (every region of `sketch`): its curves alone
/// when they name it and nothing else, else its curves by side.
pub fn key_of(sketch: &Sketch, all: &[SketchRegion], region: &SketchRegion) -> RegionKey {
    let plain = RegionKey::Curves(region.key.clone());
    match find(sketch, all, &plain).as_slice() {
        [only] if *only == region => plain,
        _ => RegionKey::Sided { left: region.left.clone(), right: region.right.clone(), nth: region.nth },
    }
}

/// A polyline along `piece` the way its boundary runs.
fn piece_poly(sketch: &Sketch, piece: &Piece) -> Vec<Vec2> {
    let mut poly = match Carrier::of(sketch, piece.curve) {
        Some(c) if !piece.whole || c.span().is_none() => c.poly(piece.t0, piece.t1),
        _ => sketch.tessellate(piece.curve, TRACE_TOL),
    };
    if !piece.forward {
        poly.reverse();
    }
    poly
}

/// The boundary of `regions` taken together: each outer loop with its holes. A piece that two
/// of the regions share lies inside and is left out, so neighbours become one region.
fn union_loops(sketch: &Sketch, regions: &[&SketchRegion]) -> Vec<(Vec<Piece>, Vec<Vec<Piece>>)> {
    let mut distinct: Vec<&SketchRegion> = Vec::new();
    for r in regions {
        if !distinct.iter().any(|d| (&d.key, &d.left, &d.right, d.nth) == (&r.key, &r.left, &r.right, r.nth)) {
            distinct.push(r);
        }
    }
    let id = |p: &Piece| (p.curve, p.t0.to_bits(), p.t1.to_bits(), p.forward);
    let all: Vec<&Piece> = distinct.iter().flat_map(|r| r.outer.iter().chain(r.holes.iter().flatten())).collect();
    let present: BTreeSet<(EntityId, u64, u64, bool)> = all.iter().map(|p| id(p)).collect();
    let shared = |p: &Piece| present.contains(&(p.curve, p.t0.to_bits(), p.t1.to_bits(), !p.forward));
    if !all.iter().any(|p| shared(p)) {
        return distinct.iter().map(|r| (r.outer.clone(), r.holes.clone())).collect();
    }

    // What is left bounds the union. Each piece is an edge running the way its boundary does;
    // the faces traced along the edges' own direction are the union's loops.
    let kept: Vec<&Piece> = all.into_iter().filter(|p| !shared(p)).collect();
    let (mut places, mut next) = (Vec::new(), 0);
    let edges: Vec<Edge> = kept
        .iter()
        .filter_map(|p| {
            let poly = piece_poly(sketch, p);
            let (a, b) = (node_at(*poly.first()?, &mut places, &mut next), node_at(*poly.last()?, &mut places, &mut next));
            Some(Edge { curve: p.curve, a, b, poly, t0: p.t0, t1: p.t1, whole: p.whole })
        })
        .collect();
    let mut outers: Vec<(Vec<Piece>, Vec<Vec2>, f64, Vec<Vec<Piece>>)> = Vec::new();
    let mut holes: Vec<(Vec<Piece>, Vec<Vec2>)> = Vec::new();
    for face in trace(&edges, &vec![true; edges.len()]) {
        if !face.first().is_some_and(|(_, forward)| *forward) {
            continue;
        }
        let pieces: Vec<Piece> = face.iter().filter_map(|(i, _)| kept.get(*i).map(|p| (*p).clone())).collect();
        let poly = face_polygon(&edges, &face);
        let area = shoelace(&poly);
        if area > MIN_AREA {
            outers.push((pieces, poly, area, Vec::new()));
        } else if area < -MIN_AREA {
            holes.push((pieces, poly));
        }
    }
    for (pieces, poly) in holes {
        let Some(sample) = poly.first() else { continue };
        if let Some(o) = outers.iter_mut().filter(|o| point_in_polygon(&o.1, *sample)).min_by(|a, b| a.2.total_cmp(&b.2)) {
            o.3.push(pieces);
        }
    }
    outers.into_iter().map(|o| (o.0, o.3)).collect()
}

/// A loop of pieces as kernel curves: pieces of one curve that follow one another are one curve
/// again, and a curve used whole is the sketch's own.
fn loop_curves(sketch: &Sketch, pieces: &[Piece]) -> Vec<TaggedCurve2> {
    struct Run {
        curve: EntityId,
        start: f64,
        span: f64,
        forward: bool,
        whole: bool,
    }
    let round = |curve: EntityId| matches!(sketch.geometry(curve), Some(Geometry::Circle { .. }));
    // `a` and `b` are the same place on the curve.
    let same = |curve: EntityId, a: f64, b: f64| {
        let d = (a - b).abs();
        d <= PARAM_EPS || (round(curve) && (d - TAU).abs() <= PARAM_EPS)
    };
    // `next` carries on where `run` stops.
    let follows = |run: &Run, next: &Piece| {
        run.curve == next.curve
            && run.forward == next.forward
            && !run.whole
            && !next.whole
            && if run.forward { same(run.curve, run.start + run.span, next.t0) } else { same(run.curve, run.start, next.t1) }
    };
    let mut runs: Vec<Run> = Vec::new();
    for p in pieces {
        match runs.last_mut() {
            Some(run) if follows(run, p) => {
                run.span += p.t1 - p.t0;
                if !run.forward {
                    run.start = p.t0;
                }
            }
            _ => runs.push(Run { curve: p.curve, start: p.t0, span: p.t1 - p.t0, forward: p.forward, whole: p.whole }),
        }
    }
    // The loop may begin in the middle of a run.
    if runs.len() > 1
        && let (Some(last), Some(first)) = (runs.last(), runs.first())
        && follows(last, &Piece { curve: first.curve, t0: first.start, t1: first.start + first.span, whole: first.whole, forward: first.forward })
    {
        let (span, start) = (first.span, first.start);
        runs.remove(0);
        if let Some(last) = runs.last_mut() {
            last.span += span;
            if !last.forward {
                last.start = start;
            }
        }
    }
    runs.iter()
        .filter_map(|run| {
            let carrier = Carrier::of(sketch, run.curve);
            let all = carrier.is_none_or(|c| (run.span - c.span().unwrap_or(TAU)).abs() <= PARAM_EPS.max(c.eps()));
            let curve = match carrier {
                Some(c) if !run.whole && !all => match c {
                    Carrier::Line(..) => Curve2::Line { start: c.at(run.start), end: c.at(run.start + run.span) },
                    Carrier::Arc(center, radius, from, _) => {
                        let start_angle = norm_angle(from + run.start);
                        Curve2::Arc { center, radius, start_angle, end_angle: start_angle + run.span }
                    }
                    Carrier::Circle(center, radius) => {
                        let start_angle = norm_angle(run.start);
                        Curve2::Arc { center, radius, start_angle, end_angle: start_angle + run.span }
                    }
                },
                _ => sketch.curve2(run.curve)?,
            };
            Some(TaggedCurve2 { tag: u64::from(run.curve.0), curve })
        })
        .collect()
}

/// Kernel profile of the given regions taken together, in `frame`. Curve tags are entity ids.
pub fn profile(sketch: &Sketch, frame: Frame, regions: &[&SketchRegion]) -> Profile {
    let lp = |pieces: &[Piece]| Loop { curves: loop_curves(sketch, pieces) };
    Profile {
        frame,
        regions: union_loops(sketch, regions)
            .iter()
            .map(|(outer, holes)| Region { outer: lp(outer), holes: holes.iter().map(|h| lp(h)).collect() })
            .collect(),
    }
}

/// The boundary of the given regions taken together, as closed polylines (outer loops and
/// holes alike), for showing what a feature is made from.
pub fn outlines(sketch: &Sketch, regions: &[&SketchRegion]) -> Vec<Vec<Vec2>> {
    let poly = |pieces: &[Piece]| -> Vec<Vec2> {
        let mut out: Vec<Vec2> = Vec::new();
        for p in pieces {
            for q in piece_poly(sketch, p) {
                if out.last().is_none_or(|l| !l.near(q, MERGE_TOL)) {
                    out.push(q);
                }
            }
        }
        if out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| a.near(*b, MERGE_TOL)) {
            out.pop();
        }
        out
    };
    union_loops(sketch, regions).iter().flat_map(|(outer, holes)| std::iter::once(poly(outer)).chain(holes.iter().map(|h| poly(h)))).collect()
}
