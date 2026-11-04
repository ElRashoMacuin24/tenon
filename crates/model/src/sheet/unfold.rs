//! Laying a sheet metal solid flat.
//!
//! The solid is read as it is, whatever features made it: one side of the sheet (the A side) is
//! followed from a flat face across every bend, each face is given its place in the plane, and
//! the edges left on the rim are the outline. A bend is as wide in the flat as its neutral
//! surface is long: angle x (inside radius + K x thickness).

use std::collections::{BTreeMap, VecDeque};
use std::f64::consts::{PI, TAU};

use tenon_geom::{Axis, Vec2, Vec3};
use tenon_kernel::{CurveKind, EdgeId, EdgeInfo, FaceInfo, Kernel, KernelError, ShapeHandle, SurfaceKind, Topology};

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
pub(super) enum Chart {
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
        /// How far that middle turns for each mm along the axis: a slit that runs round a tube at
        /// a slant has no one angle that is opposite it from end to end.
        slant: f64,
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
pub(super) fn radial(axis: &Axis, p: Vec3) -> Option<Vec3> {
    let d = p - axis.origin();
    let r = d - axis.dir() * d.dot(axis.dir());
    (r.len() > 1e-9).then(|| r.normalized())
}

impl Chart {
    /// How far round the axis `p` is from the chart's reference point, and how far along.
    pub(super) fn round(&self, p: Vec3) -> (f64, f64) {
        match self {
            Chart::Plane { .. } => (0.0, 0.0),
            Chart::Bend { q, axis, r0, mid, slant, .. } => {
                let a = axis.dir();
                let rp = radial(axis, p).unwrap_or(*r0);
                let raw = r0.cross(rp).dot(a).atan2(r0.dot(rp));
                let along = (p - *q).dot(a);
                (raw + TAU * ((mid + slant * along - raw) / TAU).round(), along)
            }
        }
    }

    pub(super) fn map(&self, p: Vec3) -> Vec2 {
        match self {
            Chart::Plane { o3, u3, v3, o2, u2, v2 } => *o2 + *u2 * (p - *o3).dot(*u3) + *v2 * (p - *o3).dot(*v3),
            Chart::Bend { rho, o2, ea, eg, .. } => {
                let (angle, along) = self.round(p);
                *o2 + *ea * along + *eg * (rho * angle)
            }
        }
    }

    /// Where a point given by its angle and its distance along a bend lies in the flat.
    pub(super) fn unrolled(&self, angle: f64, along: f64) -> Vec2 {
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
pub(crate) fn bend_side(i: usize, faces: &[FaceInfo], t: f64) -> Option<bool> {
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
pub(super) fn base_chart(face: &FaceInfo) -> Option<Chart> {
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

/// A gap in a round face less wide than this is none (radians).
const ROUND_GAP: f64 = 1e-9;

/// What stretches of angle leave of the way round, each given by its two ends in either order: the
/// middle of the widest gap, and how wide it is. Nothing when they go all the way round.
fn widest_gap(stretches: &[(f64, f64)]) -> Option<(f64, f64)> {
    let mut covered: Vec<(f64, f64)> = Vec::with_capacity(stretches.len() + 1);
    for &(a, b) in stretches {
        let (lo, long) = (a.min(b), (a - b).abs());
        if long >= TAU {
            return None;
        }
        let from = lo.rem_euclid(TAU);
        covered.push((from, from + long));
        // (What runs past a turn comes round to the start again.)
        covered.push((from - TAU, from + long - TAU));
    }
    covered.sort_by(|x, y| x.0.total_cmp(&y.0));
    let &(first, _) = covered.first()?;
    let (mut reach, mut best) = (first, None::<(f64, f64)>);
    for (lo, hi) in &covered {
        if *lo > reach && best.is_none_or(|b| *lo - reach > b.1) {
            best = Some(((reach + *lo) / 2.0, *lo - reach));
        }
        reach = reach.max(*hi);
    }
    // From the furthest reached round to where the first starts, a turn on.
    if first + TAU > reach && best.is_none_or(|b| first + TAU - reach > b.1) {
        best = Some(((reach + first + TAU) / 2.0, first + TAU - reach));
    }
    best.filter(|b| b.1 > ROUND_GAP)
}

/// Which way round the axis a round face is opposite the opening in it, seen from `r0` at `q`: the
/// angle the chart's middle is to have and how far it turns for each mm along. Angles are then
/// taken within half a turn of that, so that its rim is in one piece in the flat wherever the
/// sheet is opened and whatever is cut out of it. A face that covers every angle somewhere along
/// its length may yet be opened at a slant, along one of its edges. None for one that closes.
fn opposite_opening(solid: &Read, f: u32, axis: &Axis, q: Vec3, r0: Vec3) -> Option<(f64, f64)> {
    let a = axis.dir();
    let angle = |p: Vec3| radial(axis, p).map(|rp| r0.cross(rp).dot(a).atan2(r0.dot(rp)));
    let onward = |from: f64, to: f64| from + (to - from + PI).rem_euclid(TAU) - PI;
    // Each edge of its rim: where it starts, is half way and ends, at (angle, along).
    let mut rim: Vec<(bool, [(f64, f64); 3])> = Vec::new();
    for &e in solid.topo.face_edges.get(f as usize).map_or(&[][..], Vec::as_slice) {
        let Some(info) = solid.edges.get(e as usize).filter(|i| !i.degenerate) else { continue };
        let (start, mid) = (angle(info.start)?, onward(angle(info.start)?, angle(info.mid)?));
        let end = onward(mid, angle(info.end)?);
        let along = |p: Vec3| (p - q).dot(a);
        let round = matches!(&info.curve, CurveKind::Circle { axis: about, .. } if about.dir().cross(a).len() < SMOOTH);
        rim.push((round, [(start, along(info.start)), (mid, along(info.mid)), (end, along(info.end))]));
    }
    // (Within half a turn of where the chart starts, which is at angle 0, so that it stays at 0.)
    let round_from_start = |angle: f64| (angle + PI).rem_euclid(TAU) - PI;
    let gap_at = |slant: f64| {
        let flat = |at: (f64, f64)| at.0 - slant * at.1;
        let stretches: Vec<(f64, f64)> = rim.iter().flat_map(|(_, r)| [(flat(r[0]), flat(r[1])), (flat(r[1]), flat(r[2]))]).collect();
        widest_gap(&stretches)
    };
    if let Some((middle, _)) = gap_at(0.0) {
        return Some((round_from_start(middle + PI), 0.0));
    }
    rim.iter()
        .filter(|(round, r)| !round && (r[2].1 - r[0].1).abs() > 1e-9)
        .map(|(_, r)| (r[2].0 - r[0].0) / (r[2].1 - r[0].1))
        .find_map(|slant| gap_at(slant).map(|(middle, _)| (round_from_start(middle + PI), slant)))
}

/// The chart of a round side of the sheet the pattern is laid out from, as a tube that was ripped
/// open has no flat one: seen from outside, its axis up the page, opposite the opening in it.
fn round_chart(solid: &Read, f: u32, inner: bool, opts: &Unfold) -> Option<Chart> {
    let face = solid.faces.get(f as usize)?;
    let SurfaceKind::Cylinder { axis, radius } = &face.surface else { return None };
    let r0 = radial(axis, face.centroid)?;
    let rho = if inner { radius + opts.k_factor * opts.thickness } else { radius - (1.0 - opts.k_factor) * opts.thickness };
    let eg = if inner { -Vec2::X } else { Vec2::X };
    let q = axis.origin() + r0 * *radius;
    let (mid, slant) = opposite_opening(solid, f, axis, q, r0).unwrap_or((0.0, 0.0));
    Some(Chart::Bend { q, axis: *axis, r0, mid, slant, rho, o2: Vec2::ZERO, ea: Vec2::Y, eg, inner, radius: *radius })
}

/// The face a pattern is laid out from when it is not told: the largest flat face, unless that
/// is no side of the sheet and a round side is to be had (a tube that was ripped open, whose
/// flat faces are its rims and the sides of the slit): then the largest round side.
fn side_to_lay_out_from(solid: &Read, t: f64) -> Option<u32> {
    let flat = largest_flat(&solid.faces);
    if flat.is_some_and(|f| super::wrap::has_other_side(solid, f as usize, t)) {
        return flat;
    }
    let round = (0..solid.faces.len())
        .filter(|i| bend_side(*i, &solid.faces, t).is_some())
        .max_by(|x, y| solid.faces[*x].area.total_cmp(&solid.faces[*y].area))
        .and_then(|i| u32::try_from(i).ok());
    round.or(flat)
}

/// The chart of a face tangent to `parent` at `q`.
fn child_chart(parent: &Chart, q: Vec3, solid: &Read, f: u32, inner: Option<bool>, opts: &Unfold) -> Result<Chart, String> {
    let (b3, b2) = parent.tangent(q);
    let o2 = parent.map(q);
    let face = solid.faces.get(f as usize).ok_or("no such face")?;
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
            let (mid, slant) = opposite_opening(solid, f, axis, q, r0).unwrap_or((r0.cross(rc).dot(a).atan2(r0.dot(rc)), 0.0));
            let rho = if inner { radius + opts.k_factor * opts.thickness } else { radius - (1.0 - opts.k_factor) * opts.thickness };
            Ok(Chart::Bend { q, axis: *axis, r0, mid, slant, rho, o2, ea, eg, inner, radius: *radius })
        }
        _ => Ok(Chart::Plane { o3: q, u3: b3[0], v3: b3[1], o2, u2: b2[0], v2: b2[1] }),
    }
}

/// An edge of a face as a curve of the flat.
pub(super) fn flatten(k: &dyn Kernel, edge: EdgeId, info: &EdgeInfo, chart: &Chart, deflection: f64) -> Result<FlatCurve, String> {
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

/// A solid as the unfolding reads it: what its faces and edges are, and which meet.
pub(super) struct Read {
    pub topo: Topology,
    pub faces: Vec<FaceInfo>,
    pub edges: Vec<EdgeInfo>,
}

pub(super) fn read(k: &dyn Kernel, shape: ShapeHandle) -> Result<Read, String> {
    let topo = k.topology(shape).map_err(kerr)?;
    let faces: Vec<FaceInfo> = (0..topo.faces).map(|i| k.face_info(shape.face(i))).collect::<Result<_, _>>().map_err(kerr)?;
    let edges: Vec<EdgeInfo> = (0..topo.edges).map(|i| k.edge_info(shape.edge(i))).collect::<Result<_, _>>().map_err(kerr)?;
    Ok(Read { topo, faces, edges })
}

/// Where every face of one side of the sheet lies in the flat, when face `base` lies at `first`:
/// the side is followed from the base face across every edge where the next face runs on
/// smoothly, a flat face in the same plane or one side of a bend.
pub(super) fn follow(solid: &Read, base: u32, first: Chart, opts: &Unfold) -> Result<BTreeMap<u32, Chart>, String> {
    let Read { topo, faces, edges } = solid;
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
                    _ => match bend_side(g as usize, faces, opts.thickness) {
                        Some(inner) => Some(inner),
                        None => continue,
                    },
                };
                charts.insert(g, child_chart(&chart, info.start, solid, g, inner, opts)?);
                queue.push_back(g);
            }
        }
    }
    Ok(charts)
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
    let solid = read(k, shape)?;
    if solid.topo.solids != 1 {
        return Err(format!(
            "A flat pattern is of one solid, and this part has {}. Combine them, or keep each in a part of its own.",
            solid.topo.solids
        ));
    }
    let base = match opts.base {
        Some(b) => b,
        None => side_to_lay_out_from(&solid, t).ok_or("The part has no flat face to lay the pattern out from.")?,
    };
    let base_info = solid.faces.get(base as usize).ok_or("the face to lay the pattern out from does not exist")?;
    if let SurfaceKind::Cylinder { axis, .. } = &base_info.surface
        && radial(axis, base_info.centroid).is_none()
    {
        return Err(
            "The sheet goes all the way round (a tube), so it cannot be laid flat: it needs an open seam somewhere. Open one with Rip.".into()
        );
    }
    let first = base_chart(base_info)
        .or_else(|| round_chart(&solid, base, bend_side(base as usize, &solid.faces, t)?, opts))
        .ok_or("The pattern is laid out from a flat face or a side of a bend: the face chosen is neither.")?;
    let charts = follow(&solid, base, first, opts)?;
    let Read { topo, faces, edges } = solid;

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
    // Round faces that run on from one another about one axis: one bend, that the kernel has in
    // two where it has the seam of its cylinder (a tube opened elsewhere than there).
    let mut one_bend: Vec<(u32, u32)> = Vec::new();
    for (e, info) in edges.iter().enumerate() {
        if info.degenerate {
            continue;
        }
        let around = topo.edge_faces.get(e).map_or(&[][..], Vec::as_slice);
        let on: Vec<u32> = around.iter().copied().filter(|f| charts.contains_key(f)).collect();
        // The line where a round face meets itself: it goes all the way round there.
        if let ([_], [f]) = (around, on.as_slice())
            && matches!(charts.get(f), Some(Chart::Bend { .. }))
        {
            return Err("The sheet closes on itself (a box section, or a tube), so it cannot be laid flat: it needs an open seam somewhere.".into());
        }
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
                if let (Chart::Bend { axis: a, radius: ra, .. }, Chart::Bend { axis: b, radius: rb, .. }) = (cf, cg)
                    && a.dir().cross(b.dir()).len() < SMOOTH
                    && (ra - rb).abs() < 1e-6 * (1.0 + t)
                    && radial(a, b.origin()).is_none_or(|_| (b.origin() - a.origin()).cross(a.dir()).len() < 1e-6 * (1.0 + t))
                {
                    one_bend.push((*f, *g));
                    continue;
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
            // (A slit at a slant has the sheet reach further round than a turn from end to end.)
            angle: (hi - lo).min(TAU),
            radius: if *inner { *radius } else { radius - t },
            up: *inner,
            allowance: (hi - lo) * rho,
            face: *f,
        });
    }

    for (f, g) in one_bend {
        let (Some(i), Some(j)) = (bends.iter().position(|b| b.face == f), bends.iter().position(|b| b.face == g)) else { continue };
        if i == j {
            continue;
        }
        let other = bends.remove(j);
        let Some(first) = bends.iter_mut().find(|b| b.face == f) else { continue };
        // The centre line of the whole: the longer's, moved to the middle of the two.
        let middle = |b: &Bend| b.lines.first().map(|l| (l[0] + l[1]) / 2.0);
        if let (Some(a), Some(b)) = (middle(first), middle(&other)) {
            let (mine, theirs) = (first.allowance, other.allowance);
            let at = (a * mine + b * theirs) / (mine + theirs);
            let (from, base) = if mine >= theirs { (a, first.lines.clone()) } else { (b, other.lines.clone()) };
            let across = (b - a).normalized();
            let by = across * (at - from).dot(across);
            first.lines = base.iter().map(|l| [l[0] + by, l[1] + by]).collect();
        }
        first.angle = (first.angle + other.angle).min(TAU);
        first.allowance += other.allowance;
        first.extents.extend(other.extents);
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
