//! Closed regions of a sketch and their conversion to kernel profiles.
//!
//! Non-construction curves form a planar graph whose nodes are points (points joined by a
//! coincident constraint or lying at the same place count as one node). Dangling curves and
//! bridges are dropped, faces are traced by always turning to the next curve clockwise, and
//! bounded faces become regions. A component lying inside a face of another component becomes a
//! hole of that face.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use tenon_geom::{Frame, Vec2, point_in_polygon, shoelace};
use tenon_kernel::{Loop, Profile, Region, TaggedCurve2};

use crate::{Constraint, EntityId, Geometry, Sketch};

/// Chord tolerance used to approximate curves for face tracing (mm).
const TRACE_TOL: f64 = 0.01;
/// Points closer than this are one node (mm).
const MERGE_TOL: f64 = 1e-6;
/// Most bridge-removal passes.
const MAX_PASSES: usize = 32;

/// A closed region of the sketch.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchRegion {
    /// Stable identity: the sorted curves of the outer boundary. Survives dimension edits.
    pub key: Vec<EntityId>,
    /// Outer boundary curves, in order around the region.
    pub outer: Vec<EntityId>,
    /// Hole boundaries.
    pub holes: Vec<Vec<EntityId>>,
    /// Nesting depth: 0 for outermost regions, 1 inside a hole of a depth-0 region, ...
    pub depth: u32,
    /// Area of the region (holes subtracted), mm^2.
    pub area: f64,
    /// Outer boundary as a counter-clockwise polygon (for picking and display).
    pub outline: Vec<Vec2>,
    /// Hole boundaries as polygons.
    pub hole_outlines: Vec<Vec<Vec2>>,
}

impl SketchRegion {
    /// True when `p` lies inside the region (inside the outer boundary, outside every hole).
    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(&self.outline, p) && !self.hole_outlines.iter().any(|h| point_in_polygon(h, p))
    }
}

struct Edge {
    curve: EntityId,
    a: usize,
    b: usize,
    /// Polyline from node `a` to node `b`.
    poly: Vec<Vec2>,
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

/// Nodes: points merged through coincident constraints and proximity.
fn nodes(sketch: &Sketch) -> HashMap<EntityId, usize> {
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
    for (id, i) in &index {
        out.insert(*id, root(&mut parent, *i));
    }
    out
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

struct Face {
    curves: Vec<EntityId>,
    poly: Vec<Vec2>,
    area: f64,
    component: usize,
}

/// All closed regions of the sketch (construction geometry excluded).
pub fn regions(sketch: &Sketch) -> Vec<SketchRegion> {
    let node_of = nodes(sketch);
    let mut edges = Vec::new();
    let mut loops: Vec<Face> = Vec::new(); // circles: each is its own component
    for (id, e) in sketch.entities() {
        if e.construction {
            continue;
        }
        let ends = match &e.geometry {
            Geometry::Line { start, end } => Some((*start, *end)),
            Geometry::Arc { start, end, .. } => Some((*start, *end)),
            Geometry::Spline { poles, .. } => poles.first().zip(poles.last()).map(|(a, b)| (*a, *b)),
            Geometry::Circle { .. } => {
                let mut poly = sketch.tessellate(id, TRACE_TOL);
                if poly.len() > 1 && poly.first().zip(poly.last()).is_some_and(|(a, b)| a.near(*b, MERGE_TOL)) {
                    poly.pop();
                }
                let area = shoelace(&poly).abs();
                loops.push(Face { curves: vec![id], poly, area, component: 0 });
                None
            }
            Geometry::Point { .. } => None,
        };
        let Some((s, t)) = ends else { continue };
        let (Some(&a), Some(&b)) = (node_of.get(&s), node_of.get(&t)) else { continue };
        let poly = sketch.tessellate(id, TRACE_TOL);
        if poly.len() < 2 {
            continue;
        }
        edges.push(Edge { curve: id, a, b, poly });
    }

    // Drop dangling edges (degree-1 nodes) repeatedly, then bridges.
    let mut alive = vec![true; edges.len()];
    let mut faces: Vec<Vec<(usize, bool)>> = Vec::new();
    for _ in 0..MAX_PASSES {
        loop {
            let mut degree: HashMap<usize, usize> = HashMap::new();
            for (i, e) in edges.iter().enumerate().filter(|(i, _)| alive[*i]) {
                *degree.entry(e.a).or_default() += 1;
                *degree.entry(e.b).or_default() += 1;
                let _ = i;
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

    // Bounded faces (positive area) and each component's outer outline (negative area).
    let mut bounded: Vec<Face> = Vec::new();
    let mut outlines: BTreeMap<usize, (Vec<Vec2>, Vec<EntityId>)> = BTreeMap::new();
    for f in &faces {
        let poly = face_polygon(&edges, f);
        let area = shoelace(&poly);
        let Some(&(first, _)) = f.first() else { continue };
        let component = root(&mut parent, first);
        let curves: Vec<EntityId> = f.iter().filter_map(|(i, _)| edges.get(*i).map(|e| e.curve)).collect();
        if area > 0.0 {
            bounded.push(Face { curves, poly, area, component });
        } else if area < 0.0 {
            let mut p = poly;
            p.reverse();
            outlines.insert(component, (p, curves));
        }
    }
    // Circles are components of their own.
    let base = edges.len();
    for (k, mut c) in loops.into_iter().enumerate() {
        c.component = base + k;
        outlines.insert(c.component, (c.poly.clone(), c.curves.clone()));
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
                && let Some((poly, curves)) = outlines.get(comp)
            {
                area -= shoelace(poly).abs();
                holes.push(curves.clone());
                hole_outlines.push(poly.clone());
            }
        }
        let mut key = f.curves.clone();
        key.sort();
        key.dedup();
        out.push(SketchRegion { key, outer: f.curves.clone(), holes, depth: depth_of(i), area, outline: f.poly.clone(), hole_outlines });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

/// The regions extruded when the user picks nothing: every region at even nesting depth (a
/// plate with holes, but not the discs filling the holes).
pub fn default_regions(all: &[SketchRegion]) -> Vec<&SketchRegion> {
    all.iter().filter(|r| r.depth % 2 == 0).collect()
}

/// Kernel profile of the given regions, in `frame`. Curve tags are entity ids.
pub fn profile(sketch: &Sketch, frame: Frame, regions: &[&SketchRegion]) -> Profile {
    let lp = |curves: &[EntityId]| Loop {
        curves: curves.iter().filter_map(|id| sketch.curve2(*id).map(|curve| TaggedCurve2 { tag: u64::from(id.0), curve })).collect(),
    };
    Profile { frame, regions: regions.iter().map(|r| Region { outer: lp(&r.outer), holes: r.holes.iter().map(|h| lp(h)).collect() }).collect() }
}
