//! Laying a sheet metal solid flat.
//!
//! The solid is read as it is, whatever features made it: one side of the sheet (the A side) is
//! followed from a flat face across every bend, each face is given its place in the plane, and
//! the edges left on the rim are the outline. A bend is as wide in the flat as its neutral
//! surface is long: angle x (inside radius + K x thickness).

use std::collections::{BTreeMap, VecDeque};
use std::f64::consts::TAU;

use tenon_geom::{Axis, Vec2, Vec3};
use tenon_kernel::{CurveKind, EdgeId, EdgeInfo, FaceInfo, Kernel, KernelError, ShapeHandle, SurfaceKind};

use super::flat::{Bend, FlatCurve, FlatLoop, FlatPattern, JOIN, bounds, chain, crossing};

/// How a solid is laid flat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unfold {
    pub thickness: f64,
    /// Where the neutral surface lies in the thickness, from the inside of a bend: 0 to 1.
    pub k_factor: f64,
    /// The face (kernel index) to lay the pattern out from; without one, the largest flat face.
    pub base: Option<u32>,
    /// How closely the points of a curve that is neither a line nor an arc follow it (mm).
    pub deflection: f64,
}

/// Faces are tangent where their normals differ by less than this (the sine of the angle).
const SMOOTH: f64 = 1e-6;

/// How far the volume of the solid may be from what its faces account for (see [`unfold`]).
const VOLUME_REL: f64 = 0.02;

/// Where a face of the A side lies in the flat.
#[derive(Clone, Debug)]
enum Chart {
    /// A rigid move: `o3` goes to `o2`, the directions `u3`, `v3` in the plane to `u2`, `v2`.
    Plane { o3: Vec3, u3: Vec3, v3: Vec3, o2: Vec2, u2: Vec2, v2: Vec2 },
    /// Unrolled: `q` goes to `o2`, a step along the axis to `ea`, and a turn by an angle about
    /// the axis to `eg` times the neutral radius `rho` times that angle.
    Bend {
        q: Vec3,
        axis: Axis,
        /// The direction from the axis to `q`.
        r0: Vec3,
        /// The angle of the middle of the face: angles are taken within half a turn of it.
        mid: f64,
        rho: f64,
        o2: Vec2,
        ea: Vec2,
        eg: Vec2,
        /// The A side is the inside of the bend.
        inner: bool,
        /// The radius of this face.
        radius: f64,
    },
}

/// The unit direction from an axis to `p`, unless `p` is on it.
fn radial(axis: &Axis, p: Vec3) -> Option<Vec3> {
    let d = p - axis.origin();
    let r = d - axis.dir() * d.dot(axis.dir());
    (r.len() > 1e-9).then(|| r.normalized())
}

impl Chart {
    /// How far round the axis `p` is from the chart's reference point, and how far along.
    fn round(&self, p: Vec3) -> (f64, f64) {
        match self {
            Chart::Plane { .. } => (0.0, 0.0),
            Chart::Bend { q, axis, r0, mid, .. } => {
                let a = axis.dir();
                let rp = radial(axis, p).unwrap_or(*r0);
                let raw = r0.cross(rp).dot(a).atan2(r0.dot(rp));
                (raw + TAU * ((mid - raw) / TAU).round(), (p - *q).dot(a))
            }
        }
    }

    fn map(&self, p: Vec3) -> Vec2 {
        match self {
            Chart::Plane { o3, u3, v3, o2, u2, v2 } => *o2 + *u2 * (p - *o3).dot(*u3) + *v2 * (p - *o3).dot(*v3),
            Chart::Bend { rho, o2, ea, eg, .. } => {
                let (angle, along) = self.round(p);
                *o2 + *ea * along + *eg * (rho * angle)
            }
        }
    }

    /// Where a point given by its angle and its distance along a bend lies in the flat.
    fn unrolled(&self, angle: f64, along: f64) -> Vec2 {
        match self {
            Chart::Plane { o2, .. } => *o2,
            Chart::Bend { rho, o2, ea, eg, .. } => *o2 + *ea * along + *eg * (rho * angle),
        }
    }

    /// Two directions tangent to the face at `p`, at right angles, and where they point in the
    /// flat. A face tangent to this one at `p` is laid out to agree with them.
    fn tangent(&self, p: Vec3) -> ([Vec3; 2], [Vec2; 2]) {
        match self {
            Chart::Plane { u3, v3, u2, v2, .. } => ([*u3, *v3], [*u2, *v2]),
            Chart::Bend { axis, r0, ea, eg, .. } => {
                let a = axis.dir();
                ([a, a.cross(radial(axis, p).unwrap_or(*r0))], [*ea, *eg])
            }
        }
    }
}

/// The normal of a flat or round face at `p` (either way round), if it is one of those.
fn normal_at(face: &FaceInfo, p: Vec3) -> Option<Vec3> {
    match &face.surface {
        SurfaceKind::Plane { normal, .. } => Some(normal.normalized()),
        SurfaceKind::Cylinder { axis, .. } => radial(axis, p),
        _ => None,
    }
}

/// Whether round face `i` is one side of a bend: it has a partner about the same axis, one
/// thickness larger (then `i` is the inside of the bend, `Some(true)`) or smaller. The rounded
/// rim of a sheet and the wall of a hole have none.
fn bend_side(i: usize, faces: &[FaceInfo], t: f64) -> Option<bool> {
    let SurfaceKind::Cylinder { axis, radius } = &faces.get(i)?.surface else { return None };
    let tol = 1e-6 * (1.0 + radius + t);
    for (j, f) in faces.iter().enumerate() {
        let SurfaceKind::Cylinder { axis: b, radius: rb } = &f.surface else { continue };
        if j == i || axis.dir().cross(b.dir()).len() > SMOOTH {
            continue;
        }
        let d = b.origin() - axis.origin();
        if (d - axis.dir() * d.dot(axis.dir())).len() > tol {
            continue;
        }
        if (rb - radius - t).abs() < tol {
            return Some(true);
        }
        if (radius - rb - t).abs() < tol {
            return Some(false);
        }
    }
    None
}

/// The largest flat face; of equals (the two sides of a plate), the one that looks up most.
fn largest_flat(faces: &[FaceInfo]) -> Option<u32> {
    // How much a face looks up, then back, then right, to nine places (so that noise in the last
    // digits does not choose).
    let looks = |n: Vec3| [n.z, n.y, n.x].map(|v| (v * 1e9).round());
    let mut best: Option<(usize, f64, [f64; 3])> = None;
    for (i, f) in faces.iter().enumerate() {
        let SurfaceKind::Plane { normal, .. } = &f.surface else { continue };
        let better = match &best {
            None => true,
            Some((_, area, way)) if (f.area - area).abs() <= 1e-9 * area.max(1.0) => looks(*normal) > *way,
            Some((_, area, _)) => f.area > *area,
        };
        if better {
            best = Some((i, f.area, looks(*normal)));
        }
    }
    best.and_then(|b| u32::try_from(b.0).ok())
}

/// The chart of the face the pattern is laid out from: seen from outside, with the part's Z up
/// the page where the face is upright and the part's X across it where it lies flat.
fn base_chart(face: &FaceInfo) -> Option<Chart> {
    let SurfaceKind::Plane { normal, .. } = &face.surface else { return None };
    let n = normal.normalized();
    let (u3, v3) = if n.dot(Vec3::Z).abs() < 0.999 {
        let v = (Vec3::Z - n * n.dot(Vec3::Z)).normalized();
        (v.cross(n), v)
    } else {
        let u = (Vec3::X - n * n.dot(Vec3::X)).normalized();
        (u, n.cross(u))
    };
    Some(Chart::Plane { o3: face.centroid, u3, v3, o2: Vec2::ZERO, u2: Vec2::X, v2: Vec2::Y })
}

/// The chart of a face tangent to `parent` at `q`.
fn child_chart(parent: &Chart, q: Vec3, face: &FaceInfo, inner: Option<bool>, opts: &Unfold) -> Result<Chart, String> {
    let (b3, b2) = parent.tangent(q);
    let o2 = parent.map(q);
    match (&face.surface, inner) {
        (SurfaceKind::Cylinder { axis, radius }, Some(inner)) => {
            let a = axis.dir();
            let r0 = radial(axis, q).ok_or("a bend has a point on its own axis")?;
            let lin = |w: Vec3| b2[0] * w.dot(b3[0]) + b2[1] * w.dot(b3[1]);
            let (ea, eg) = (lin(a), lin(a.cross(r0)));
            if (ea.len() - 1.0).abs() > 1e-6 || (eg.len() - 1.0).abs() > 1e-6 {
                return Err("A bend does not run on smoothly from the face beside it, so the part cannot be laid flat.".into());
            }
            let rc =
                radial(axis, face.centroid).ok_or("A bend that goes all the way round (a tube) has no edge to lay it flat from: it needs a seam.")?;
            let mid = r0.cross(rc).dot(a).atan2(r0.dot(rc));
            let rho = if inner { radius + opts.k_factor * opts.thickness } else { radius - (1.0 - opts.k_factor) * opts.thickness };
            Ok(Chart::Bend { q, axis: *axis, r0, mid, rho, o2, ea, eg, inner, radius: *radius })
        }
        _ => Ok(Chart::Plane { o3: q, u3: b3[0], v3: b3[1], o2, u2: b2[0], v2: b2[1] }),
    }
}

/// An edge of a face as a curve of the flat.
fn flatten(k: &dyn Kernel, edge: EdgeId, info: &EdgeInfo, chart: &Chart, deflection: f64) -> Result<FlatCurve, String> {
    let (start, end) = (chart.map(info.start), chart.map(info.end));
    let sampled = || -> Result<FlatCurve, String> {
        let pts = k.edge_points(edge, deflection).map_err(|e| e.to_string())?;
        let mut points: Vec<Vec2> = pts.iter().map(|p| chart.map(*p)).collect();
        if points.len() < 2 {
            points = vec![start, end];
        }
        // The ends are the edge's own, so that curves meet exactly.
        if let Some(p) = points.first_mut() {
            *p = start;
        }
        if let Some(p) = points.last_mut() {
            *p = end;
        }
        Ok(FlatCurve::Polyline { points })
    };
    match (&info.curve, chart) {
        (CurveKind::Line { .. }, _) => Ok(FlatCurve::Line { start, end }),
        (CurveKind::Circle { axis, radius }, Chart::Plane { .. }) => {
            let center = chart.map(axis.origin());
            if info.start.dist(info.end) < JOIN {
                return Ok(FlatCurve::Circle { center, radius: *radius });
            }
            let turn = |p: Vec2| ((p - center).angle() - (start - center).angle()).rem_euclid(TAU);
            Ok(FlatCurve::Arc { center, radius: *radius, start, end, ccw: turn(chart.map(info.mid)) < turn(end) })
        }
        // Round the bend, about its axis: straight in the flat.
        (CurveKind::Circle { axis, .. }, Chart::Bend { axis: bend, .. })
            if axis.dir().cross(bend.dir()).len() < SMOOTH && info.start.dist(info.end) >= JOIN =>
        {
            Ok(FlatCurve::Line { start, end })
        }
        _ => sampled(),
    }
}

fn kerr(e: KernelError) -> String {
    e.to_string()
}

/// Lays `shape` flat.
///
/// The solid must be one sheet of `thickness`: flat faces joined by round bends, cut square
/// through. That is checked by volume (its A side's faces, each `thickness` deep, must account
/// for the solid to within 2 %), so a solid that is something else is refused in plain words
/// and never unfolded into nonsense.
pub fn unfold(k: &dyn Kernel, shape: ShapeHandle, opts: &Unfold) -> Result<FlatPattern, String> {
    let t = opts.thickness;
    if !(t.is_finite() && t > 0.0) {
        return Err("the sheet thickness must be more than 0".into());
    }
    if !(opts.k_factor.is_finite() && (0.0..=1.0).contains(&opts.k_factor)) {
        return Err("the K factor must be from 0 to 1".into());
    }
    let topo = k.topology(shape).map_err(kerr)?;
    if topo.solids != 1 {
        return Err(format!("A flat pattern is of one solid, and this part has {}. Combine them, or keep each in a part of its own.", topo.solids));
    }
    let faces: Vec<FaceInfo> = (0..topo.faces).map(|i| k.face_info(shape.face(i))).collect::<Result<_, _>>().map_err(kerr)?;
    let edges: Vec<EdgeInfo> = (0..topo.edges).map(|i| k.edge_info(shape.edge(i))).collect::<Result<_, _>>().map_err(kerr)?;

    let base = match opts.base {
        Some(b) => b,
        None => largest_flat(&faces).ok_or("The part has no flat face to lay the pattern out from.")?,
    };
    let base_info = faces.get(base as usize).ok_or("the face to lay the pattern out from does not exist")?;
    let first = base_chart(base_info).ok_or("The pattern is laid out from a flat face: the face chosen is not flat.")?;

    // Follow the A side from the base face across every edge where the next face runs on
    // smoothly: a flat face in the same plane, or one side of a bend.
    let mut charts: BTreeMap<u32, Chart> = BTreeMap::from([(base, first)]);
    let mut queue: VecDeque<u32> = VecDeque::from([base]);
    while let Some(f) = queue.pop_front() {
        let Some(chart) = charts.get(&f).cloned() else { continue };
        for &e in topo.face_edges.get(f as usize).map_or(&[][..], Vec::as_slice) {
            let Some(info) = edges.get(e as usize).filter(|i| !i.degenerate) else { continue };
            for &g in topo.edge_faces.get(e as usize).map_or(&[][..], Vec::as_slice) {
                if g == f || charts.contains_key(&g) {
                    continue;
                }
                let (Some(here), Some(next)) = (faces.get(f as usize), faces.get(g as usize)) else { continue };
                let (Some(nf), Some(ng)) = (normal_at(here, info.mid), normal_at(next, info.mid)) else { continue };
                if nf.cross(ng).len() > SMOOTH {
                    continue;
                }
                let inner = match &next.surface {
                    SurfaceKind::Plane { .. } => None,
                    // A round face that is no bend (the rounded rim of the sheet) is not part
                    // of the side: the outline stops at it.
                    _ => match bend_side(g as usize, &faces, t) {
                        Some(inner) => Some(inner),
                        None => continue,
                    },
                };
                charts.insert(g, child_chart(&chart, info.start, next, inner, opts)?);
                queue.push_back(g);
            }
        }
    }

    // The faces of the side, each one thickness deep, must be the solid.
    let volume = k.mass_properties(shape, 1.0).map_err(kerr)?.volume;
    let accounted: f64 = charts
        .iter()
        .map(|(f, c)| {
            let area = faces.get(*f as usize).map_or(0.0, |i| i.area);
            match c {
                Chart::Plane { .. } => area * t,
                // A bend's volume is its middle surface's area times the thickness.
                Chart::Bend { inner, radius, .. } => {
                    let inside = if *inner { *radius } else { radius - t };
                    area * t * (inside + t / 2.0) / radius
                }
            }
        })
        .sum();
    if (accounted - volume).abs() > VOLUME_REL * volume.abs() {
        return Err(format!(
            "This solid is not a sheet {} mm thick all over, so it cannot be laid flat: flat faces and round bends {} mm thick account for {:.0} mm³ of its {:.0} mm³. A flat pattern needs flat faces joined by round bends, cut straight through; check the thickness in Sheet Metal Defaults.",
            crate::explain::mm(t),
            crate::explain::mm(t),
            accounted,
            volume
        ));
    }

    // Edges with the side on one hand only are the outline; edges inside it must come out in
    // one place whichever face they are laid out with, and where a bend meets its neighbour
    // they are the bend's extent lines.
    let mut curves: Vec<FlatCurve> = Vec::new();
    let mut extents: BTreeMap<u32, Vec<[Vec2; 2]>> = BTreeMap::new();
    for (e, info) in edges.iter().enumerate() {
        if info.degenerate {
            continue;
        }
        let on: Vec<u32> = topo.edge_faces.get(e).map_or(&[][..], Vec::as_slice).iter().copied().filter(|f| charts.contains_key(f)).collect();
        match on.as_slice() {
            [f] => {
                let Some(chart) = charts.get(f) else { continue };
                curves.push(flatten(k, shape.edge(e as u32), info, chart, opts.deflection)?);
            }
            [f, g, ..] if f != g => {
                let (Some(cf), Some(cg)) = (charts.get(f), charts.get(g)) else { continue };
                for p in [info.start, info.mid, info.end] {
                    if cf.map(p).dist(cg.map(p)) > 1e-6 * (1.0 + t) {
                        return Err(
                            "The sheet closes on itself (a box section, or a tube), so it cannot be laid flat: it needs an open seam somewhere."
                                .into(),
                        );
                    }
                }
                if matches!(info.curve, CurveKind::Line { .. }) {
                    for (face, chart) in [(f, cf), (g, cg)] {
                        if matches!(chart, Chart::Bend { .. }) {
                            extents.entry(*face).or_default().push([chart.map(info.start), chart.map(info.end)]);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let (outer, inner) = chain(curves)?;
    let all: Vec<&FlatLoop> = std::iter::once(&outer).chain(&inner).collect();
    if let Some(at) = crossing(&all) {
        let (lo, _) = bounds(std::slice::from_ref(&outer));
        return Err(format!(
            "The flat pattern overlaps itself near ({:.1}, {:.1}): two pieces of the part would need the same piece of sheet. Shorten a flange or move a bend.",
            at.x - lo.x,
            at.y - lo.y
        ));
    }

    // The bends: how far each turns, and its centre line where the bend is whole.
    let mut bends: Vec<Bend> = Vec::new();
    for (f, chart) in &charts {
        let Chart::Bend { rho, inner, radius, .. } = chart else { continue };
        // The face's rim as runs of (angle, along) points.
        let mut rim: Vec<Vec<(f64, f64)>> = Vec::new();
        for &e in topo.face_edges.get(*f as usize).map_or(&[][..], Vec::as_slice) {
            let Some(info) = edges.get(e as usize).filter(|i| !i.degenerate) else { continue };
            let straight = match &info.curve {
                CurveKind::Line { .. } => true,
                CurveKind::Circle { axis, .. } => matches!(chart, Chart::Bend { axis: bend, .. } if axis.dir().cross(bend.dir()).len() < SMOOTH),
                _ => false,
            };
            let pts = if straight { vec![info.start, info.end] } else { k.edge_points(shape.edge(e), opts.deflection).map_err(kerr)? };
            rim.push(pts.iter().map(|p| chart.round(*p)).collect());
        }
        let angles = || rim.iter().flatten().map(|p| p.0);
        let (lo, hi) = (angles().fold(f64::INFINITY, f64::min), angles().fold(f64::NEG_INFINITY, f64::max));
        // (Also when there is no rim to measure: the comparison is false for a number that is none.)
        if (hi - lo).partial_cmp(&1e-9) != Some(std::cmp::Ordering::Greater) {
            continue;
        }
        let middle = (lo + hi) / 2.0;
        let mut cuts: Vec<f64> = Vec::new();
        for w in rim.iter().flat_map(|run| run.windows(2)) {
            let (a, b) = (w[0], w[1]);
            if (a.0 - middle) * (b.0 - middle) < 0.0 {
                cuts.push(a.1 + (b.1 - a.1) * (middle - a.0) / (b.0 - a.0));
            }
        }
        cuts.sort_by(f64::total_cmp);
        let lines = cuts
            .as_chunks::<2>()
            .0
            .iter()
            .filter(|[a, b]| b - a > JOIN)
            .map(|[a, b]| [chart.unrolled(middle, *a), chart.unrolled(middle, *b)])
            .collect();
        bends.push(Bend {
            lines,
            extents: extents.remove(f).unwrap_or_default(),
            angle: hi - lo,
            radius: if *inner { *radius } else { radius - t },
            up: *inner,
            allowance: (hi - lo) * rho,
            face: *f,
        });
    }

    // With its lower left corner at the origin.
    let (lo, hi) = bounds(std::slice::from_ref(&outer));
    let shift = -lo;
    let moved = |l: &FlatLoop| FlatLoop { curves: l.curves.iter().map(|c| c.moved(shift)).collect() };
    let (outer, inner): (FlatLoop, Vec<FlatLoop>) = (moved(&outer), inner.iter().map(moved).collect());
    for b in &mut bends {
        for seg in b.lines.iter_mut().chain(b.extents.iter_mut()) {
            *seg = [seg[0] + shift, seg[1] + shift];
        }
    }
    // A steady order: by where each bend's centre line is.
    let place = |b: &Bend| b.lines.first().map_or((f64::INFINITY, f64::INFINITY), |l| (l[0].x.min(l[1].x), l[0].y.min(l[1].y)));
    bends.sort_by(|a, b| {
        let (pa, pb) = (place(a), place(b));
        pa.0.total_cmp(&pb.0).then(pa.1.total_cmp(&pb.1)).then(a.face.cmp(&b.face))
    });
    let area = outer.signed_area() - inner.iter().map(|l| l.signed_area().abs()).sum::<f64>();
    let cut_length = outer.length() + inner.iter().map(FlatLoop::length).sum::<f64>();
    Ok(FlatPattern { outer, inner, bends, thickness: t, k_factor: opts.k_factor, length: hi.x - lo.x, width: hi.y - lo.y, area, cut_length })
}
