//! Laying a sheet metal solid flat.
//!
//! The solid is read as it is, whatever features made it: one side of the sheet (the A side) is
//! followed from a flat face across every bend, each face is given its place in the plane, and
//! the edges left on the rim are the outline. A bend is as wide in the flat as its neutral
//! surface is long: angle x (inside radius + K x thickness).
//!
//! A sheet rolled round an axis (a contour roll) is read the same way: a cylinder is one long
//! bend, and a cone unrolls about the point it comes to. The round parts of a rolled profile
//! (tori) cannot be laid flat at all without stretching: each is unrolled as the cone through
//! its two rims, as long as its neutral surface is, and the pattern says so ([`Chart::Turned`]).
//! A die-formed sheet is laid flat by triangles, which it is given ([`Extras`]): its smooth
//! faces say nothing of how, and nothing is followed out of them, so with anything but cuts
//! straight through it the solid is refused.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use tenon_geom::{Axis, Vec2, Vec3};
use tenon_kernel::{CurveKind, EdgeId, EdgeInfo, FaceInfo, Kernel, KernelError, ShapeHandle, SurfaceKind, Topology};

use super::flat::{Bend, FlatCurve, FlatLoop, FlatPattern, JOIN, PunchMark, bounds, chain, crossing};
use super::loft::Skin;

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
        /// The middle as it goes along the axis, where it is not one angle: (along, angle), in
        /// order. A slit that runs round a tube at a slant has no one angle opposite it.
        wander: Vec<(f64, f64)>,
        rho: f64,
        o2: Vec2,
        ea: Vec2,
        eg: Vec2,
        /// The A side is the inside of the bend.
        inner: bool,
        /// The radius of this face.
        radius: f64,
    },
    /// A cone or a round part of a rolled profile, turned round `axis`, unrolled a sector of a
    /// ring: `q` goes to `o2`; a step along the face at `q` to `es`, and a turn by an angle to
    /// `eg` times the neutral radius there. The neutral radius goes up by `slope` for each step
    /// along the face, so a turn by an angle turns the flat by that angle times `slope` about
    /// the point the sector comes to. (On a cone that is exact, its half angle's sine. A round
    /// part is the cone through its two rims, as long as its neutral surface is.)
    ///
    /// With `rows`, a profile of several pieces: each row round the axis is laid straight, as
    /// long as it is on the neutral surface, about one line up the page. Then pieces meet all
    /// along their rims, and nothing overlaps, and the shape is no longer each piece's own.
    Turned {
        axis: Axis,
        /// The direction from the axis to `q`.
        r0: Vec3,
        mid: f64,
        along: Along,
        /// The neutral radius at `q`.
        rho: f64,
        slope: f64,
        o2: Vec2,
        es: Vec2,
        eg: Vec2,
        /// The A side is the inside of the roll (a cone) or of the bend (a round part).
        inner: bool,
        rows: bool,
    },
    /// A die-formed side of the sheet: its place in the flat comes from the triangles it was
    /// laid flat by. `side` is the side of the skin this is; the flat is seen from it.
    Skin { skin: Arc<Skin>, side: usize },
}

/// What a step along a turned face is.
#[derive(Clone, Copy, Debug)]
pub(super) enum Along {
    /// Along a cone: `q` is the chart's reference point, and a step along it goes `cos` along
    /// the axis and `sin` away from it.
    Cone { q: Vec3, cos: f64, sin: f64 },
    /// Round a torus of radius `major` whose tube's neutral radius is `neutral`: from the angle
    /// `from` round the tube, angles being taken within half a turn of `middle`.
    Tube { major: f64, neutral: f64, from: f64, middle: f64 },
    /// Out across a flat ring square to the axis, from radius `from`.
    Ring { from: f64 },
}

impl Along {
    /// How far round the tube of a torus about `axis` of radius `major` the point `p` is.
    fn tube_angle(axis: &Axis, major: f64, p: Vec3) -> f64 {
        let d = p - axis.origin();
        let h = d.dot(axis.dir());
        let r = (d - axis.dir() * h).len();
        h.atan2(r - major)
    }

    /// How far along the face `p` is from the chart's reference point.
    fn distance(&self, axis: &Axis, p: Vec3) -> f64 {
        match self {
            Along::Cone { q, cos, .. } => (p - *q).dot(axis.dir()) / cos,
            Along::Tube { major, neutral, from, middle } => {
                let raw = Along::tube_angle(axis, *major, p);
                neutral * (raw + TAU * ((middle - raw) / TAU).round() - from)
            }
            Along::Ring { from } => {
                let d = p - axis.origin();
                (d - axis.dir() * d.dot(axis.dir())).len() - from
            }
        }
    }

    /// The way along the face at `p`, as a direction of space.
    fn way(&self, axis: &Axis, p: Vec3, r0: Vec3) -> Vec3 {
        let rp = radial(axis, p).unwrap_or(r0);
        match self {
            Along::Cone { cos, sin, .. } => rp * *sin + axis.dir() * *cos,
            Along::Tube { major, .. } => {
                let b = Along::tube_angle(axis, *major, p);
                axis.dir() * b.cos() - rp * b.sin()
            }
            Along::Ring { .. } => rp,
        }
    }
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
            Chart::Plane { .. } | Chart::Skin { .. } => (0.0, 0.0),
            Chart::Bend { q, axis, r0, mid, wander, .. } => {
                let a = axis.dir();
                let rp = radial(axis, p).unwrap_or(*r0);
                let raw = r0.cross(rp).dot(a).atan2(r0.dot(rp));
                let along = (p - *q).dot(a);
                let middle = middle_at(wander, along).unwrap_or(*mid);
                (raw + TAU * ((middle - raw) / TAU).round(), along)
            }
            Chart::Turned { axis, r0, mid, along, .. } => {
                let a = axis.dir();
                let rp = radial(axis, p).unwrap_or(*r0);
                let raw = r0.cross(rp).dot(a).atan2(r0.dot(rp));
                (raw + TAU * ((mid - raw) / TAU).round(), along.distance(axis, p))
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
            Chart::Turned { .. } => {
                let (angle, along) = self.round(p);
                self.unrolled(angle, along)
            }
            Chart::Skin { skin, side } => skin.map(p, *side),
        }
    }

    /// Where a point given by its angle and its distance along a bend lies in the flat.
    pub(super) fn unrolled(&self, angle: f64, along: f64) -> Vec2 {
        match self {
            Chart::Plane { o2, .. } => *o2,
            Chart::Skin { .. } => Vec2::ZERO,
            Chart::Bend { rho, o2, ea, eg, .. } => *o2 + *ea * along + *eg * (rho * angle),
            Chart::Turned { rho, slope, o2, es, eg, rows, .. } => {
                if *rows || slope.abs() < 1e-12 {
                    return *o2 + *es * along + *eg * ((rho + slope * along) * angle);
                }
                // About the point the sector comes to.
                let reach = rho / slope;
                let (sin, cos) = (angle * slope).sin_cos();
                *o2 - *es * reach + (*es * cos + *eg * sin) * (reach + along)
            }
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
            Chart::Turned { axis, r0, along, slope, es, eg, rows, .. } => {
                let (angle, _) = self.round(p);
                let (sin, cos) = if *rows { (0.0, 1.0) } else { (angle * slope).sin_cos() };
                let (way, round) = (along.way(axis, p, *r0), axis.dir().cross(radial(axis, p).unwrap_or(*r0)));
                ([way, round], [*es * cos + *eg * sin, *eg * cos - *es * sin])
            }
            // (Nothing is laid out from a die-formed side.)
            Chart::Skin { .. } => ([Vec3::X, Vec3::Y], [Vec2::X, Vec2::Y]),
        }
    }

    /// A point of the turned face `face` by how far round the axis it is from the chart's reference
    /// point and how far along. (What [`Chart::round`] gives of a point.)
    pub(super) fn point_of(&self, face: &FaceInfo, angle: f64, along: f64) -> Option<Vec3> {
        let Chart::Turned { axis, r0, along: how, .. } = self else { return None };
        let round = *r0 * angle.cos() + axis.dir().cross(*r0) * angle.sin();
        let (height, radius) = match (how, &face.surface) {
            (Along::Cone { q, cos, sin }, _) => {
                let d = *q - axis.origin();
                (d.dot(axis.dir()) + along * cos, (d - axis.dir() * d.dot(axis.dir())).len() + along * sin)
            }
            (Along::Tube { neutral, from, .. }, SurfaceKind::Torus { major_radius, minor_radius, .. }) => {
                let b = from + along / neutral;
                (minor_radius * b.sin(), major_radius + minor_radius * b.cos())
            }
            _ => return None,
        };
        Some(axis.origin() + axis.dir() * height + round * radius)
    }

    /// Whether this is a piece of a rolled sheet whose rims are circles of the flat, or straight
    /// lines where a neighbour has circles: where two meet they meet at one point only.
    pub(super) fn turned(&self) -> bool {
        matches!(self, Chart::Turned { .. })
    }
}

/// The normal of a flat or round face at `p` (either way round), if it is one of those.
pub(super) fn normal_at(face: &FaceInfo, p: Vec3) -> Option<Vec3> {
    match &face.surface {
        SurfaceKind::Plane { normal, .. } => Some(normal.normalized()),
        SurfaceKind::Cylinder { axis, .. } => radial(axis, p),
        SurfaceKind::Cone { axis, half_angle, .. } => radial(axis, p).map(|r| r * half_angle.cos() - axis.dir() * half_angle.sin()),
        SurfaceKind::Torus { axis, major_radius, .. } => {
            let b = Along::tube_angle(axis, *major_radius, p);
            radial(axis, p).map(|r| r * b.cos() + axis.dir() * b.sin())
        }
        _ => None,
    }
}

/// Whether two axes are one line, and the second's direction relative to the first's.
fn same_line(a: &Axis, b: &Axis, slack: f64) -> Option<f64> {
    if a.dir().cross(b.dir()).len() > SMOOTH {
        return None;
    }
    let d = b.origin() - a.origin();
    ((d - a.dir() * d.dot(a.dir())).len() <= slack).then(|| a.dir().dot(b.dir()).signum())
}

/// The other side of the sheet from turned face `i`, a cone or a torus: its partner about the
/// same axis, one thickness away, and whether `i` is the inside of the two (nearer the axis, on
/// a cone; the smaller tube, on a torus).
pub(crate) fn turned_side(i: usize, faces: &[FaceInfo], t: f64) -> Option<(usize, bool)> {
    let face = faces.get(i)?;
    let tol = |r: f64| 1e-6 * (1.0 + r.abs() + t);
    for (j, f) in faces.iter().enumerate() {
        if j == i {
            continue;
        }
        match (&face.surface, &f.surface) {
            (SurfaceKind::Cone { axis, half_angle, ref_radius }, SurfaceKind::Cone { axis: b, half_angle: hb, ref_radius: rb }) => {
                let Some(way) = same_line(axis, b, tol(*ref_radius)) else { continue };
                if (half_angle - way * hb).abs() > SMOOTH {
                    continue;
                }
                // How far apart they are, square to the cone: radii compared at one height.
                let height = (b.origin() - axis.origin()).dot(axis.dir());
                let apart = (rb - height * half_angle.tan() - ref_radius) * half_angle.cos();
                if (apart.abs() - t).abs() < tol(*ref_radius) {
                    return Some((j, apart > 0.0));
                }
            }
            (SurfaceKind::Torus { axis, major_radius, minor_radius }, SurfaceKind::Torus { axis: b, major_radius: mb, minor_radius: nb }) => {
                if same_line(axis, b, tol(*major_radius)).is_none()
                    || (b.origin() - axis.origin()).dot(axis.dir()).abs() > tol(*major_radius)
                    || (major_radius - mb).abs() > tol(*major_radius)
                {
                    continue;
                }
                if (nb - minor_radius - t).abs() < tol(*minor_radius) {
                    return Some((j, true));
                }
                if (minor_radius - nb - t).abs() < tol(*minor_radius) {
                    return Some((j, false));
                }
            }
            _ => {}
        }
    }
    None
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

/// Where along a path of (along, angle) points the angle is at `along`: at its ends beyond them.
fn middle_at(path: &[(f64, f64)], along: f64) -> Option<f64> {
    let (first, last) = (path.first()?, path.last()?);
    if along <= first.0 {
        return Some(first.1);
    }
    let inside = path.windows(2).find(|w| w[0].0 < w[1].0 && (w[0].0..=w[1].0).contains(&along));
    Some(inside.map_or(last.1, |w| w[0].1 + (w[1].1 - w[0].1) * (along - w[0].0) / (w[1].0 - w[0].0)))
}

/// A straight stretch of a round face's rim as the angle round its axis and the distance along it
/// see it: its two ends, (angle, along), the angle not brought within a turn.
type Stretch = ((f64, f64), (f64, f64));

/// Whether (angle, along) is within the rim made of `rim`: looking back along the axis from there,
/// an odd number of stretches is passed.
fn within_rim(rim: &[Stretch], angle: f64, along: f64) -> bool {
    let mut odd = false;
    for ((t0, z0), (t1, z1)) in rim {
        let (lo, hi) = (t0.min(*t1), t0.max(*t1));
        let at = angle + TAU * ((lo - angle) / TAU).ceil();
        if at < hi && z0 + (z1 - z0) * (at - t0) / (t1 - t0) < along {
            odd = !odd;
        }
    }
    odd
}

/// The opening of a round face that covers every angle somewhere along its length: its middle, as
/// the (along, angle) points of a path from the one end of the face to the other that keeps within
/// the opening. None when there is no such path: the face closes round on itself somewhere.
fn opening_along(rim: &[Stretch]) -> Option<Vec<(f64, f64)>> {
    let mut stops: Vec<f64> = rim.iter().flat_map(|(a, b)| [a.1, b.1]).collect();
    stops.sort_by(f64::total_cmp);
    stops.dedup_by(|x, y| (*x - *y).abs() <= 1e-9);
    let angle_at = |s: &Stretch, along: f64| s.0.0 + (s.1.0 - s.0.0) * (along - s.0.1) / (s.1.1 - s.0.1);
    // In each band of the axis between two stops, what is not the face, by the angles of its two
    // sides at the band's two ends.
    type Opening = [(f64, f64); 2];
    let mut bands: Vec<Vec<Opening>> = Vec::with_capacity(stops.len());
    for w in stops.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        let z = (lo + hi) / 2.0;
        let mut sides: Vec<&Stretch> = rim.iter().filter(|s| s.0.1.min(s.1.1) < z && z < s.0.1.max(s.1.1)).collect();
        sides.sort_by(|p, q| angle_at(p, z).rem_euclid(TAU).total_cmp(&angle_at(q, z).rem_euclid(TAU)));
        let mut here = Vec::new();
        for (i, s) in sides.iter().enumerate() {
            let Some(t) = sides.get((i + 1) % sides.len()) else { continue };
            let (from, mut to) = (angle_at(s, z).rem_euclid(TAU), angle_at(t, z).rem_euclid(TAU));
            if to <= from {
                to += TAU;
            }
            if !within_rim(rim, (from + to) / 2.0, z) {
                let near = |angle: f64, by: f64| angle + TAU * ((by - angle) / TAU).round();
                let at = |along: f64| (near(angle_at(s, along), from), near(angle_at(t, along), to));
                here.push([at(lo), at(hi)]);
            }
        }
        bands.push(here);
    }
    // From the first band to the last, through openings that meet.
    let meet = |a: (f64, f64), b: (f64, f64)| {
        [-TAU, 0.0, TAU].iter().any(|k| a.0 <= b.1 + k + ROUND_GAP && b.0 + k <= a.1 + ROUND_GAP) && a.1 - a.0 < TAU && b.1 - b.0 < TAU
    };
    let mut came_from: Vec<Vec<Option<usize>>> = Vec::with_capacity(bands.len());
    for (j, band) in bands.iter().enumerate() {
        let row: Vec<Option<usize>> = band
            .iter()
            .enumerate()
            .map(|(i, open)| match j.checked_sub(1) {
                None => Some(i),
                Some(before) => bands.get(before).and_then(|b| {
                    b.iter()
                        .enumerate()
                        .position(|(p, was)| came_from.get(before).and_then(|r| r.get(p)).copied().flatten().is_some() && meet(was[1], open[0]))
                }),
            })
            .collect();
        came_from.push(row);
    }
    let mut at = came_from.last()?.iter().position(Option::is_some)?;
    let mut chosen: Vec<&Opening> = Vec::with_capacity(bands.len());
    for j in (0..bands.len()).rev() {
        chosen.push(bands.get(j)?.get(at)?);
        at = (*came_from.get(j)?.get(at)?)?;
    }
    chosen.reverse();
    let mut path: Vec<(f64, f64)> = Vec::with_capacity(chosen.len() * 2);
    for (open, w) in chosen.iter().zip(stops.windows(2)) {
        for (side, along) in open.iter().zip([w[0], w[1]]) {
            let middle = (side.0 + side.1) / 2.0;
            let onward = path.last().map_or(middle, |p| middle + TAU * ((p.1 - middle) / TAU).round());
            path.push((along, onward));
        }
    }
    Some(path)
}

/// Which way round the axis a round face is opposite the opening in it, seen from `r0` at `q`: the
/// angle the chart's middle is to have, or the path it takes along the axis. Angles are then taken
/// within half a turn of that, so that its rim is in one piece in the flat wherever the sheet is
/// opened and whatever is cut out of it. None for a face that closes round on itself.
pub(super) fn opposite_opening(solid: &Read, f: u32, axis: &Axis, q: Vec3, r0: Vec3) -> Option<(f64, Vec<(f64, f64)>)> {
    let a = axis.dir();
    let angle = |p: Vec3| radial(axis, p).map(|rp| r0.cross(rp).dot(a).atan2(r0.dot(rp)));
    let onward = |from: f64, to: f64| from + (to - from + PI).rem_euclid(TAU) - PI;
    // Each edge of its rim, by where it starts, is half way and ends.
    let mut rim: Vec<Stretch> = Vec::new();
    for &e in solid.topo.face_edges.get(f as usize).map_or(&[][..], Vec::as_slice) {
        let Some(info) = solid.edges.get(e as usize).filter(|i| !i.degenerate) else { continue };
        let start = angle(info.start)?;
        // (A whole circle about the axis goes half a turn twice, and which way each half goes could
        // be told only by the last digits.)
        let whole_circle =
            info.start.dist(info.end) < JOIN && matches!(&info.curve, CurveKind::Circle { axis: about, .. } if about.dir().cross(a).len() < SMOOTH);
        let mid = if whole_circle { start + PI } else { onward(start, angle(info.mid)?) };
        let end = if whole_circle { start + TAU } else { onward(mid, angle(info.end)?) };
        let along = |p: Vec3| (p - q).dot(a);
        rim.push(((start, along(info.start)), (mid, along(info.mid))));
        rim.push(((mid, along(info.mid)), (end, along(info.end))));
    }
    // (Within half a turn of where the chart starts, which is at angle 0, so that it stays at 0.)
    let from_start = |angle: f64| (angle + PI).rem_euclid(TAU) - PI;
    let stretches: Vec<(f64, f64)> = rim.iter().map(|(p, q)| (p.0, q.0)).collect();
    if let Some((middle, _)) = widest_gap(&stretches) {
        return Some((from_start(middle + PI), Vec::new()));
    }
    let opposite: Vec<(f64, f64)> = opening_along(&rim)?.into_iter().map(|(along, middle)| (along, middle + PI)).collect();
    let here = middle_at(&opposite, 0.0)?;
    let by = from_start(here) - here;
    Some((0.0, opposite.into_iter().map(|(along, middle)| (along, middle + by)).collect()))
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
            let (mid, wander) = opposite_opening(solid, f, axis, q, r0).unwrap_or((r0.cross(rc).dot(a).atan2(r0.dot(rc)), Vec::new()));
            let rho = if inner { radius + opts.k_factor * opts.thickness } else { radius - (1.0 - opts.k_factor) * opts.thickness };
            Ok(Chart::Bend { q, axis: *axis, r0, mid, wander, rho, o2, ea, eg, inner, radius: *radius })
        }
        _ => Ok(Chart::Plane { o3: q, u3: b3[0], v3: b3[1], o2, u2: b2[0], v2: b2[1] }),
    }
}

/// The chart of a cone or a round part of a rolled profile: its reference point `q`, how it is
/// measured, and the directions in the flat of a step along it and a step round it, found by
/// `flat_of`. `extent`: the faces and edges to read a round part's reach from.
fn turned_chart(
    q: Vec3,
    face: &FaceInfo,
    rim: &[&EdgeInfo],
    inner: bool,
    o2: Vec2,
    flat_of: &dyn Fn(Vec3) -> Vec2,
    opts: &Unfold,
) -> Result<Chart, String> {
    let (axis, along, rho, slope) = turned_parts(q, face, rim, inner, None, opts)?;
    let r0 = radial(&axis, q).ok_or("a rolled face has a point on its own axis")?;
    let (es, eg) = (flat_of(along.way(&axis, q, r0)), flat_of(axis.dir().cross(r0)));
    if (es.len() - 1.0).abs() > 1e-6 || (eg.len() - 1.0).abs() > 1e-6 {
        return Err("A rolled face does not run on smoothly from the face beside it, so the part cannot be laid flat.".into());
    }
    let rc = radial(&axis, face.centroid).ok_or("A sheet rolled all the way round has no edge to lay it flat from: it needs a seam.")?;
    let mid = r0.cross(rc).dot(axis.dir()).atan2(r0.dot(rc));
    Ok(Chart::Turned { axis, r0, mid, along, rho, slope, o2, es, eg, inner, rows: false })
}

/// How a face turned round an axis is measured from its point `q`: the axis (turned the way of
/// `way`, when that is given), a step along the face, the neutral radius at `q` and how much it
/// goes up for each step along.
fn turned_parts(
    q: Vec3,
    face: &FaceInfo,
    rim: &[&EdgeInfo],
    inner: bool,
    way: Option<Vec3>,
    opts: &Unfold,
) -> Result<(Axis, Along, f64, f64), String> {
    let t = opts.thickness;
    let k = opts.k_factor;
    let turned = |axis: &Axis| -> (Axis, f64) {
        match way {
            Some(w) if w.dot(axis.dir()) < 0.0 => (Axis::new(axis.origin(), -axis.dir()).unwrap_or(*axis), -1.0),
            _ => (*axis, 1.0),
        }
    };
    let radius_of = |axis: &Axis| {
        let d = q - axis.origin();
        (d - axis.dir() * d.dot(axis.dir())).len()
    };
    match &face.surface {
        SurfaceKind::Cylinder { axis, radius } => {
            let (axis, _) = turned(axis);
            let rho = if inner { radius + k * t } else { radius - (1.0 - k) * t };
            Ok((axis, Along::Cone { q, cos: 1.0, sin: 0.0 }, rho, 0.0))
        }
        SurfaceKind::Cone { axis, half_angle, .. } => {
            let (axis, sign) = turned(axis);
            let (sin, cos) = (half_angle * sign).sin_cos();
            // The neutral surface is that far through the sheet from the side nearest the axis.
            let rho = if inner { radius_of(&axis) + k * t * cos } else { radius_of(&axis) - (1.0 - k) * t * cos };
            Ok((axis, Along::Cone { q, cos, sin }, rho, sin))
        }
        SurfaceKind::Torus { axis, major_radius, minor_radius } => {
            let (axis, _) = turned(axis);
            let neutral = if inner { minor_radius + k * t } else { minor_radius - (1.0 - k) * t };
            // How far round the tube it goes, from the ends of its edges.
            let angle = |p: Vec3| Along::tube_angle(&axis, *major_radius, p);
            let round_tube = rim.iter().find(|e| matches!(&e.curve, CurveKind::Circle { radius, .. } if (radius - minor_radius).abs() < 1e-7));
            let middle = round_tube.map_or(angle(face.centroid), |e| angle(e.mid));
            let unwrapped = |p: Vec3| {
                let raw = angle(p);
                raw + TAU * ((middle - raw) / TAU).round()
            };
            let (lo, hi) = rim
                .iter()
                .flat_map(|e| [e.start, e.mid, e.end])
                .map(unwrapped)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), b| (lo.min(b), hi.max(b)));
            if hi - lo <= 1e-9 {
                return Err("A round part of the sheet has no length, so it cannot be laid flat.".into());
            }
            // The cone through its two rims, as long as its neutral surface is.
            let at = |b: f64| major_radius + neutral * b.cos();
            let slope = (at(hi) - at(lo)) / (neutral * (hi - lo));
            let from = unwrapped(q);
            let rho = at(lo) + slope * neutral * (from - lo);
            Ok((axis, Along::Tube { major: *major_radius, neutral, from, middle }, rho, slope))
        }
        _ => Err("only a cylinder, a cone or a round part of a rolled profile is unrolled this way".into()),
    }
}

/// The one axis every face of a rolled sheet is turned about, when they all are: with flat rings
/// square to it among them.
fn common_axis(charts: &BTreeMap<u32, Chart>, faces: &[FaceInfo]) -> Option<Axis> {
    let axis = charts.values().find_map(|c| match c {
        Chart::Bend { axis, .. } | Chart::Turned { axis, .. } => Some(*axis),
        _ => None,
    })?;
    let slack = 1e-6;
    let all = charts.iter().all(|(f, c)| match c {
        Chart::Bend { axis: a, .. } | Chart::Turned { axis: a, .. } => same_line(&axis, a, slack).is_some(),
        Chart::Plane { .. } => matches!(faces.get(*f as usize).map(|i| &i.surface), Some(SurfaceKind::Plane { normal, .. }) if normal.normalized().cross(axis.dir()).len() < SMOOTH),
        Chart::Skin { .. } => false,
    });
    all.then_some(axis)
}

/// The rolled sheet of `charts`, laid out row by row (see [`Chart::Turned`]): from face `base`,
/// the way along it up the page and seen from outside; and what the pieces come to: how far a
/// straight row is from where it would lie on that piece alone.
fn rows_of(solid: &Read, charts: &BTreeMap<u32, Chart>, base: u32, axis: &Axis, opts: &Unfold) -> Result<(BTreeMap<u32, Chart>, f64), String> {
    let t = opts.thickness;
    let Read { topo, faces, edges, .. } = solid;
    let inner_of = |f: u32| -> bool {
        match faces.get(f as usize).map(|i| &i.surface) {
            Some(SurfaceKind::Cylinder { .. }) => bend_side(f as usize, faces, t).unwrap_or(true),
            _ => turned_side(f as usize, faces, t).is_some_and(|s| s.1),
        }
    };
    let parts = |f: u32, q: Vec3| -> Result<(Axis, Along, f64, f64), String> {
        let face = faces.get(f as usize).ok_or("no such face")?;
        if let SurfaceKind::Plane { .. } = face.surface {
            // A flat ring: no bend round the axis, so its middle is as good as any place in it.
            let d = q - axis.origin();
            let from = (d - axis.dir() * d.dot(axis.dir())).len();
            return Ok((*axis, Along::Ring { from }, from, 1.0));
        }
        turned_parts(q, face, &edges_of(solid, f), inner_of(f), Some(axis.dir()), opts)
    };
    let centre = faces.get(base as usize).ok_or("no such face")?.centroid;
    let r0 = radial(axis, centre).ok_or("A sheet rolled all the way round has no edge to lay it flat from: it needs a seam.")?;
    // The point of the base face in the middle of it.
    let q = match faces.get(base as usize).map(|i| &i.surface) {
        Some(SurfaceKind::Cylinder { radius, axis: own }) => own.origin() + own.dir() * (centre - own.origin()).dot(own.dir()) + r0 * *radius,
        Some(SurfaceKind::Cone { axis: own, half_angle, ref_radius }) => {
            let h = (centre - own.origin()).dot(own.dir());
            own.origin() + own.dir() * h + r0 * (ref_radius + h * half_angle.tan())
        }
        Some(SurfaceKind::Torus { axis: own, major_radius, minor_radius }) => {
            let middle = edges_of(solid, base)
                .iter()
                .find(|e| matches!(&e.curve, CurveKind::Circle { radius, .. } if (radius - minor_radius).abs() < 1e-7))
                .map_or(Along::tube_angle(own, *major_radius, centre), |e| Along::tube_angle(own, *major_radius, e.mid));
            own.origin() + own.dir() * (minor_radius * middle.sin()) + r0 * (major_radius + minor_radius * middle.cos())
        }
        _ => centre,
    };
    let (own, along, rho, slope) = parts(base, q)?;
    let way = along.way(&own, q, r0);
    let up = if way.dot(Vec3::Z).abs() > 1e-9 { way.dot(Vec3::Z) > 0.0 } else { way.dot(Vec3::Y) >= 0.0 };
    let es = Vec2::Y * if up { 1.0 } else { -1.0 };
    let round = axis.dir().cross(r0);
    // Seen from outside the side of the sheet this is.
    let inner = inner_of(base);
    let outward = match faces.get(base as usize).map(|i| &i.surface) {
        Some(SurfaceKind::Plane { normal, .. }) => normal.normalized(),
        Some(surface) => {
            let out = normal_at(&FaceInfo { surface: surface.clone(), area: 0.0, centroid: q, reversed: false }, q).unwrap_or(r0);
            if inner { -out } else { out }
        }
        None => r0,
    };
    let eg = Vec2::X * if round.cross(way).dot(outward) * if up { 1.0 } else { -1.0 } >= 0.0 { 1.0 } else { -1.0 };
    let first = Chart::Turned { axis: own, r0, mid: 0.0, along, rho, slope, o2: Vec2::ZERO, es, eg, inner, rows: true };
    let mut out: BTreeMap<u32, Chart> = BTreeMap::from([(base, first)]);
    let mut queue: VecDeque<u32> = VecDeque::from([base]);
    while let Some(f) = queue.pop_front() {
        for &e in topo.face_edges.get(f as usize).map_or(&[][..], Vec::as_slice) {
            let Some(info) = edges.get(e as usize).filter(|i| !i.degenerate) else { continue };
            for &g in topo.edge_faces.get(e as usize).map_or(&[][..], Vec::as_slice) {
                if g == f || out.contains_key(&g) || !charts.contains_key(&g) {
                    continue;
                }
                let Some(Chart::Turned { axis: pa, along: pal, rho: prho, slope: pslope, o2: po2, es: pes, .. }) = out.get(&f).cloned() else {
                    continue;
                };
                let q = info.mid;
                let (at, was) = (pal.distance(&pa, q), pal.way(&pa, q, r0));
                let (own, along, mut rho, mut slope) = parts(g, q)?;
                let her = along.way(&own, q, r0);
                let here = prho + pslope * at;
                // A round part runs on from what it comes from, and to what it goes to: each line's
                // neutral surface is its own.
                if let Along::Tube { .. } = along {
                    let reach = tube_reach(&along, &own, &edges_of(solid, g), q);
                    let far = rho + slope * reach;
                    rho = here;
                    if reach.abs() > 1e-9 {
                        slope = (far - here) / reach;
                    }
                } else if at.abs() > 1e-9
                    && matches!(pal, Along::Tube { .. })
                    && f != base
                    && let Some(Chart::Turned { slope: ps, .. }) = out.get_mut(&f)
                {
                    *ps = (rho - prho) / at;
                }
                let flip = if her.dot(was) >= 0.0 { 1.0 } else { -1.0 };
                out.insert(
                    g,
                    Chart::Turned {
                        axis: own,
                        r0,
                        mid: 0.0,
                        along,
                        rho,
                        slope,
                        o2: po2 + pes * at,
                        es: pes * flip,
                        eg,
                        inner: inner_of(g),
                        rows: true,
                    },
                );
                queue.push_back(g);
            }
        }
    }
    // How far a piece's rim would stray from straight, unrolled on its own.
    let mut bow = 0.0f64;
    for (f, c) in &out {
        let Chart::Turned { rho, slope, .. } = c else { continue };
        let angles = edges_of(solid, *f).iter().flat_map(|e| [e.start, e.end]).map(|p| c.round(p).0).collect::<Vec<f64>>();
        let (lo, hi) = angles.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), a| (l.min(*a), h.max(*a)));
        let reach = edges_of(solid, *f).iter().flat_map(|e| [e.start, e.end]).map(|p| c.round(p).1).fold(0.0f64, |m, s| m.max(s.abs()));
        let radius = (rho.abs() + slope.abs() * reach).max(0.0);
        let half = (hi - lo) / 2.0 * slope.abs();
        if slope.abs() > 1e-9 && half.is_finite() {
            bow = bow.max((radius / slope.abs()) * (1.0 - half.cos()));
        }
    }
    Ok((out, bow))
}

/// How far along a round part its far rim is from `q`, in steps along it.
fn tube_reach(along: &Along, axis: &Axis, rim: &[&EdgeInfo], q: Vec3) -> f64 {
    let at_q = along.distance(axis, q);
    rim.iter()
        .flat_map(|e| [e.start, e.mid, e.end])
        .map(|p| along.distance(axis, p) - at_q)
        .fold(0.0, |far: f64, s| if s.abs() > far.abs() { s } else { far })
}

/// The chart of a round side of the sheet that a pattern is laid out from, when it has no flat
/// side to start from: seen from outside, with the way along the face up the page.
fn round_base(solid: &Read, f: u32, t: f64, opts: &Unfold) -> Result<Option<Chart>, String> {
    let Some(face) = solid.faces.get(f as usize) else { return Ok(None) };
    let Some(rc) = (match &face.surface {
        SurfaceKind::Cylinder { axis, .. } | SurfaceKind::Cone { axis, .. } | SurfaceKind::Torus { axis, .. } => radial(axis, face.centroid),
        _ => return Ok(None),
    }) else {
        return Err(
            "The sheet goes all the way round (a tube), so it cannot be laid flat: it needs an open seam somewhere. Open one with Rip.".into()
        );
    };
    // Which side of the sheet the viewer is on decides which way round the flat is.
    let up = |w: Vec3| {
        if w.z.abs() > 1e-9 {
            w.z > 0.0
        } else if w.y.abs() > 1e-9 {
            w.y > 0.0
        } else {
            w.x >= 0.0
        }
    };
    let turned_q = |axis: &Axis| axis.origin() + axis.dir() * (face.centroid - axis.origin()).dot(axis.dir());
    match &face.surface {
        SurfaceKind::Cylinder { axis, radius } => {
            let Some(inner) = bend_side(f as usize, &solid.faces, t) else { return Ok(None) };
            let q = turned_q(axis) + rc * *radius;
            let rho = if inner { radius + opts.k_factor * t } else { radius - (1.0 - opts.k_factor) * t };
            let way = if up(axis.dir()) { 1.0 } else { -1.0 };
            let eg = Vec2::X * if inner { -way } else { way };
            // (Opposite the opening in it, which is what a tube that was ripped has.)
            let (mid, wander) = opposite_opening(solid, f, axis, q, rc).unwrap_or((0.0, Vec::new()));
            Ok(Some(Chart::Bend { q, axis: *axis, r0: rc, mid, wander, rho, o2: Vec2::ZERO, ea: Vec2::Y * way, eg, inner, radius: *radius }))
        }
        SurfaceKind::Cone { axis, half_angle, ref_radius } => {
            let Some((_, inner)) = turned_side(f as usize, &solid.faces, t) else { return Ok(None) };
            let height = (face.centroid - axis.origin()).dot(axis.dir());
            let q = turned_q(axis) + rc * (ref_radius + height * half_angle.tan());
            let way = if up(axis.dir()) { 1.0 } else { -1.0 };
            let (es, eg) = (Vec2::Y * way, Vec2::X * if inner { -way } else { way });
            let (s, c) = half_angle.sin_cos();
            let round = axis.dir().cross(rc);
            let along = rc * s + axis.dir() * c;
            let flat_of = |w: Vec3| es * w.dot(along) + eg * w.dot(round);
            turned_chart(q, face, &[], inner, Vec2::ZERO, &flat_of, opts).map(Some)
        }
        SurfaceKind::Torus { axis, major_radius, minor_radius } => {
            let Some((_, inner)) = turned_side(f as usize, &solid.faces, t) else { return Ok(None) };
            let rim: Vec<&EdgeInfo> = solid
                .topo
                .face_edges
                .get(f as usize)
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .filter_map(|e| solid.edges.get(*e as usize))
                .filter(|e| !e.degenerate)
                .collect();
            // In the middle of the tube's reach.
            let angle = |p: Vec3| Along::tube_angle(axis, *major_radius, p);
            let middle = rim
                .iter()
                .find(|e| matches!(&e.curve, CurveKind::Circle { radius, .. } if (radius - minor_radius).abs() < 1e-7))
                .map_or(angle(face.centroid), |e| angle(e.mid));
            let q = turned_q(axis)
                + axis.dir() * (minor_radius * middle.sin() - (turned_q(axis) - axis.origin()).dot(axis.dir()))
                + rc * (major_radius + minor_radius * middle.cos());
            let way = if up(axis.dir()) { 1.0 } else { -1.0 };
            let (es, eg) = (Vec2::Y * way, Vec2::X * if inner { -way } else { way });
            let round = axis.dir().cross(rc);
            let along = axis.dir() * middle.cos() - rc * middle.sin();
            let flat_of = |w: Vec3| es * w.dot(along) + eg * w.dot(round);
            turned_chart(q, face, &rim, inner, Vec2::ZERO, &flat_of, opts).map(Some)
        }
        _ => Ok(None),
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
        // A whole rim of a die-formed side is as its triangles laid it out.
        (_, Chart::Skin { skin, side }) if skin.rim(info.start, info.end, *side).is_some() => {
            Ok(FlatCurve::Polyline { points: skin.rim(info.start, info.end, *side).unwrap_or_default() })
        }
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
        // Round a turned face, about its axis: a circle about the point it comes to, or straight
        // where its neutral radius does not change along it.
        (CurveKind::Circle { axis, .. }, Chart::Turned { axis: about, rho, slope, o2, es, rows, .. })
            if axis.dir().cross(about.dir()).len() < SMOOTH && info.start.dist(info.end) >= JOIN =>
        {
            if *rows || slope.abs() < 1e-12 {
                return Ok(FlatCurve::Line { start, end });
            }
            let center = *o2 - *es * (rho / slope);
            let turn = |p: Vec2| ((p - center).angle() - (start - center).angle()).rem_euclid(TAU);
            Ok(FlatCurve::Arc { center, radius: center.dist(start), start, end, ccw: turn(chart.map(info.mid)) < turn(end) })
        }
        _ => sampled(),
    }
}

fn kerr(e: KernelError) -> String {
    e.to_string()
}

/// A solid as the unfolding reads it: what its faces and edges are, and which meet, and the
/// faces it is not to follow onto (what a punch has formed).
pub(super) struct Read {
    pub topo: Topology,
    pub faces: Vec<FaceInfo>,
    pub edges: Vec<EdgeInfo>,
    pub skip: BTreeSet<u32>,
}

pub(super) fn read(k: &dyn Kernel, shape: ShapeHandle) -> Result<Read, String> {
    let topo = k.topology(shape).map_err(kerr)?;
    let faces: Vec<FaceInfo> = (0..topo.faces).map(|i| k.face_info(shape.face(i))).collect::<Result<_, _>>().map_err(kerr)?;
    let edges: Vec<EdgeInfo> = (0..topo.edges).map(|i| k.edge_info(shape.edge(i))).collect::<Result<_, _>>().map_err(kerr)?;
    Ok(Read { topo, faces, edges, skip: BTreeSet::new() })
}

/// The edges of face `f`, as they are.
fn edges_of(solid: &Read, f: u32) -> Vec<&EdgeInfo> {
    solid
        .topo
        .face_edges
        .get(f as usize)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(|e| solid.edges.get(*e as usize))
        .filter(|e| !e.degenerate)
        .collect()
}

/// Where every face of one side of the sheet lies in the flat, when face `base` lies at `first`:
/// the side is followed from the base face across every edge where the next face runs on
/// smoothly, a flat face in the same plane, one side of a bend or of a rolled sheet.
pub(super) fn follow(solid: &Read, base: u32, first: Chart, opts: &Unfold) -> Result<BTreeMap<u32, Chart>, String> {
    let Read { topo, faces, edges, skip } = solid;
    let mut charts: BTreeMap<u32, Chart> = BTreeMap::from([(base, first)]);
    let mut queue: VecDeque<u32> = VecDeque::from([base]);
    while let Some(f) = queue.pop_front() {
        let Some(chart) = charts.get(&f).cloned() else { continue };
        for &e in topo.face_edges.get(f as usize).map_or(&[][..], Vec::as_slice) {
            let Some(info) = edges.get(e as usize).filter(|i| !i.degenerate) else { continue };
            for &g in topo.edge_faces.get(e as usize).map_or(&[][..], Vec::as_slice) {
                if g == f || charts.contains_key(&g) || skip.contains(&g) {
                    continue;
                }
                let (Some(here), Some(next)) = (faces.get(f as usize), faces.get(g as usize)) else { continue };
                let (Some(nf), Some(ng)) = (normal_at(here, info.mid), normal_at(next, info.mid)) else { continue };
                if nf.cross(ng).len() > SMOOTH {
                    continue;
                }
                let made = match &next.surface {
                    // Where two pieces of a rolled sheet meet round the axis they touch at one
                    // point only: in the middle of the edge they meet along.
                    SurfaceKind::Plane { .. } => {
                        let q = if chart.turned() { info.mid } else { info.start };
                        child_chart(&chart, q, solid, g, None, opts)?
                    }
                    SurfaceKind::Cone { .. } | SurfaceKind::Torus { .. } => {
                        // One that has no other side is no sheet: the outline stops at it.
                        let Some((_, inner)) = turned_side(g as usize, faces, opts.thickness) else { continue };
                        let q = if matches!(info.curve, CurveKind::Circle { .. }) { info.mid } else { info.start };
                        let (b3, b2) = chart.tangent(q);
                        let flat_of = |w: Vec3| b2[0] * w.dot(b3[0]) + b2[1] * w.dot(b3[1]);
                        turned_chart(q, next, &edges_of(solid, g), inner, chart.map(q), &flat_of, opts)?
                    }
                    // A round face that is no bend (the rounded rim of the sheet) is not part
                    // of the side: the outline stops at it.
                    _ => {
                        let Some(inner) = bend_side(g as usize, faces, opts.thickness) else { continue };
                        let q = if chart.turned() { info.mid } else { info.start };
                        child_chart(&chart, q, solid, g, Some(inner), opts)?
                    }
                };
                charts.insert(g, made);
                queue.push_back(g);
            }
        }
    }
    Ok(charts)
}

/// The largest side of a rolled sheet: a cylinder, a cone or a round part with the sheet's other
/// side one thickness from it.
fn largest_round(faces: &[FaceInfo], t: f64) -> Option<u32> {
    let side = |i: usize, f: &FaceInfo| match &f.surface {
        SurfaceKind::Cylinder { .. } => bend_side(i, faces, t).is_some(),
        SurfaceKind::Cone { .. } | SurfaceKind::Torus { .. } => turned_side(i, faces, t).is_some(),
        _ => false,
    };
    let best = faces.iter().enumerate().filter(|(i, f)| side(*i, f)).max_by(|a, b| a.1.area.total_cmp(&b.1.area));
    best.and_then(|(i, _)| u32::try_from(i).ok())
}

/// What the features that made a part know of it and its solid does not say.
#[derive(Clone, Debug, Default)]
pub struct Extras {
    /// Die-formed sheets: laid flat by the triangles each comes with.
    pub skins: Vec<Arc<Skin>>,
    /// What punches have made of the sheet.
    pub punches: Vec<PunchFaces>,
    /// How much the shapes punches have formed add to what the sheet's own faces account for.
    pub formed_volume: f64,
}

/// The faces one stamp of a punch made, as the flat pattern shows them.
#[derive(Clone, Debug, Default)]
pub struct PunchFaces {
    /// What it is called.
    pub name: String,
    /// A formed shape: nothing of it is an outline of the flat. Otherwise a cut.
    pub formed: bool,
    pub faces: BTreeSet<u32>,
    /// A formed shape: those of its faces the sheet bends at, where the sheet meets them.
    pub bends: BTreeSet<u32>,
    /// The faces of the solid made by features after this one.
    pub later: BTreeSet<u32>,
    /// Where its point was when it was stamped, on the sheet's side.
    pub at: Option<Vec3>,
}

/// Whether face `f` touches both sides of die-formed sheet `skin`, as a wall of a cut through it does.
fn reaches_across(topo: &Topology, f: u32, skin: &Skin) -> bool {
    let touches = |side: &Vec<u32>| {
        topo.face_edges
            .get(f as usize)
            .into_iter()
            .flatten()
            .any(|e| topo.edge_faces.get(*e as usize).into_iter().flatten().any(|g| side.contains(g)))
    };
    skin.faces.iter().all(touches)
}

/// How many pieces a curve of the flat that is not straight is drawn in where it is marked.
const MARK_STEP: f64 = 0.2;

/// Lays `shape` flat.
///
/// The solid must be one sheet of `thickness`: flat faces joined by round bends, cut square
/// through. That is checked by volume (its A side's faces, each `thickness` deep, must account
/// for the solid to within 2 %), so a solid that is something else is refused in plain words
/// and never unfolded into nonsense. A die-formed sheet is held to that too, and every other face
/// must reach from one of its sides to the other: a volume does not see what it leaves out.
pub fn unfold(k: &dyn Kernel, shape: ShapeHandle, opts: &Unfold) -> Result<FlatPattern, String> {
    unfold_with(k, shape, opts, &Extras::default())
}

/// [`unfold`], with what the part's features know of the solid.
pub fn unfold_with(k: &dyn Kernel, shape: ShapeHandle, opts: &Unfold, extras: &Extras) -> Result<FlatPattern, String> {
    let t = opts.thickness;
    if !(t.is_finite() && t > 0.0) {
        return Err("the sheet thickness must be more than 0".into());
    }
    if !(opts.k_factor.is_finite() && (0.0..=1.0).contains(&opts.k_factor)) {
        return Err("the K factor must be from 0 to 1".into());
    }
    let mut solid = read(k, shape)?;
    solid.skip = extras.punches.iter().filter(|p| p.formed).flat_map(|p| p.faces.iter().copied()).collect();
    if solid.topo.solids != 1 {
        return Err(format!(
            "A flat pattern is of one solid, and this part has {}. Combine them, or keep each in a part of its own.",
            solid.topo.solids
        ));
    }
    // A formed shape is one stroke of the press, and a mark in the flat: a cut through it has no
    // place there.
    for p in extras.punches.iter().filter(|p| p.formed) {
        let cut = p.faces.iter().any(|f| {
            let beside = solid
                .topo
                .face_edges
                .get(*f as usize)
                .into_iter()
                .flatten()
                .flat_map(|e| solid.topo.edge_faces.get(*e as usize).into_iter().flatten());
            beside.into_iter().any(|g| !p.faces.contains(g) && p.later.contains(g) && !super::wrap::has_other_side(&solid, *g as usize, t))
        });
        if cut {
            return Err(format!(
                "{} has been cut since it was stamped: in a flat pattern a formed shape is a mark, and what cuts it has no place there. Move the cut clear of it, or take it off, and the flat pattern can be laid out.",
                p.name
            ));
        }
    }
    let volume = k.mass_properties(shape, 1.0).map_err(kerr)?.volume;
    // The faces of the side, each one thickness deep, must be the solid.
    let accounted_by = |charts: &BTreeMap<u32, Chart>| -> f64 {
        charts
            .iter()
            .map(|(f, c)| {
                let area = solid.faces.get(*f as usize).map_or(0.0, |i| i.area);
                // (A turned face's middle surface: halfway between it and its other side.)
                let other = || turned_side(*f as usize, &solid.faces, t).and_then(|(j, _)| solid.faces.get(j)).map_or(area, |g| g.area);
                match c {
                    Chart::Plane { .. } => area * t,
                    // A bend's volume is its middle surface's area times the thickness.
                    Chart::Bend { inner, radius, .. } => {
                        let inside = if *inner { *radius } else { radius - t };
                        area * t * (inside + t / 2.0) / radius
                    }
                    Chart::Turned { .. } => (area + other()) / 2.0 * t,
                    Chart::Skin { skin, side } => {
                        let far =
                            skin.faces.get(1 - *side).map_or(0.0, |fs| fs.iter().filter_map(|i| solid.faces.get(*i as usize)).map(|g| g.area).sum());
                        (area + far) / 2.0 * t
                    }
                }
            })
            .sum::<f64>()
            + extras.formed_volume
    };
    let account = |charts: &BTreeMap<u32, Chart>| (accounted_by(charts) - volume).abs() <= VOLUME_REL * volume.abs();
    let refusal = |charts: &BTreeMap<u32, Chart>| {
        format!(
            "This solid is not a sheet {} mm thick all over, so it cannot be laid flat: flat faces and round bends {} mm thick account for {:.0} mm³ of its {:.0} mm³. A flat pattern needs flat faces joined by round bends, cut straight through; check the thickness in Sheet Metal Defaults.",
            crate::explain::mm(t),
            crate::explain::mm(t),
            accounted_by(charts),
            volume
        )
    };
    // A die-formed side is laid out from itself; a flat face, when it is one the whole sheet
    // follows from; else the largest round side.
    let skin_base = extras.skins.iter().find_map(|skin| {
        let side = match opts.base {
            Some(b) => skin.faces.iter().position(|fs| fs.contains(&b))?,
            None => 0,
        };
        (!skin.faces.get(side)?.is_empty()).then_some((skin, side))
    });
    let charts = if let Some((skin, side)) = skin_base {
        let charts: BTreeMap<u32, Chart> = skin.faces[side].iter().map(|f| (*f, Chart::Skin { skin: skin.clone(), side })).collect();
        // Nothing is followed out of a die-formed side (its triangles are all it has), so whatever
        // else is on the solid would be left out. Every other face must be a wall of a cut
        // straight through: it reaches from one side of the sheet to the other.
        let stray = (0..solid.faces.len() as u32)
            .filter(|f| !skin.faces.iter().any(|fs| fs.contains(f)))
            .filter(|f| !reaches_across(&solid.topo, *f, skin))
            .count();
        if stray > 0 || !account(&charts) {
            return Err(format!(
                "A die-formed sheet is laid flat by its own triangles, and only what is cut straight through it can be shown on them. Something else is joined to this one or cut part way into it: {stray} of the solid's faces do not reach from one side of the sheet to the other, and the two sides {} mm thick account for {:.0} mm³ of its {:.0} mm³. Take that off, or make the sheet as Press Brake facets, which unfold with what is joined to them.",
                crate::explain::mm(t),
                accounted_by(&charts),
                volume
            ));
        }
        charts
    } else {
        let plain = match opts.base {
            Some(b) => {
                let info = solid.faces.get(b as usize).ok_or("the face to lay the pattern out from does not exist")?;
                base_chart(info).map(|c| (b, c))
            }
            None => largest_flat(&solid.faces).and_then(|b| solid.faces.get(b as usize).and_then(base_chart).map(|c| (b, c))),
        };
        // (A tube's rims and the sides of a slit are flat, and no side of the sheet.)
        let flat_is_a_side = plain.as_ref().is_some_and(|(b, _)| super::wrap::has_other_side(&solid, *b as usize, t));
        let from_flat = plain.map(|(b, c)| follow(&solid, b, c, opts)).transpose();
        let round = match opts.base {
            Some(b) => Some(b),
            None => largest_round(&solid.faces, t),
        };
        match from_flat {
            Ok(Some(charts)) if account(&charts) => charts,
            flat => {
                let rolled = match round {
                    Some(b) => match round_base(&solid, b, t, opts) {
                        Ok(Some(c)) => Some(follow(&solid, b, c, opts).map(|charts| (account(&charts), charts))),
                        Ok(None) => None,
                        Err(e) => Some(Err(e)),
                    },
                    None => None,
                };
                match (flat, rolled) {
                    (_, Some(Ok((true, charts)))) => charts,
                    (Err(e), _) => return Err(e),
                    (Ok(Some(_)), Some(Err(e))) if !flat_is_a_side => return Err(e),
                    (Ok(Some(charts)), _) => return Err(refusal(&charts)),
                    // Nothing to lay it out from.
                    (Ok(None), Some(Ok((false, charts)))) => return Err(refusal(&charts)),
                    (Ok(None), Some(Err(e))) => return Err(e),
                    (Ok(None), None) if opts.base.is_some() => {
                        return Err("The pattern is laid out from a flat face: the face chosen is not flat.".into());
                    }
                    (Ok(None), None) => return Err("The part has no flat face to lay the pattern out from.".into()),
                }
            }
        }
    };
    // Pieces of a rolled sheet that do not lie flat together (they have rims of different
    // curves, or a round part stretches between them) are laid out row by row, when the whole
    // sheet is turned round one axis.
    let in_rows = {
        let bad_fit = charts.values().any(|c| matches!(c, Chart::Turned { along: Along::Tube { .. }, .. }))
            || solid.edges.iter().enumerate().any(|(e, info)| {
                let at = solid.topo.edge_faces.get(e).map_or(&[][..], Vec::as_slice);
                let on: Vec<&Chart> = at.iter().filter_map(|f| charts.get(f)).collect();
                match on.as_slice() {
                    [a, b, ..] if a.turned() || b.turned() => !info.degenerate && a.map(info.start).dist(b.map(info.start)) >= JOIN,
                    _ => false,
                }
            });
        match (bad_fit, common_axis(&charts, &solid.faces)) {
            (true, Some(axis)) => {
                let area = |f: u32| solid.faces.get(f as usize).map_or(0.0, |i| i.area);
                let start = opts.base.filter(|b| charts.contains_key(b)).or(charts.keys().copied().max_by(|a, b| area(*a).total_cmp(&area(*b))));
                match start {
                    Some(b) => Some(rows_of(&solid, &charts, b, &axis, opts)?),
                    None => None,
                }
            }
            _ => None,
        }
    };
    let (charts, bow) = match in_rows {
        Some((rows, bow)) => (rows, Some(bow)),
        None => (charts, None),
    };
    let Read { topo, edges, .. } = solid;
    let formed_face = |f: &u32| extras.punches.iter().any(|p| p.formed && p.faces.contains(f));

    // Edges with the side on one hand only are the outline; edges inside it must come out in
    // one place whichever face they are laid out with, and where a bend meets its neighbour
    // they are the bend's extent lines. Pieces of a rolled sheet meet at one point only, in the
    // middle of the edge between them: the outline goes across to the next at each end, and
    // how far is what the pattern says of how close it is.
    let mut curves: Vec<FlatCurve> = Vec::new();
    let mut extents: BTreeMap<u32, Vec<[Vec2; 2]>> = BTreeMap::new();
    // Round faces that run on from one another about one axis: one bend, that the kernel has in
    // two where it has the seam of its cylinder (a tube opened elsewhere than there).
    let mut one_bend: Vec<(u32, u32)> = Vec::new();
    let mut apart = 0.0f64;
    // At each end of such an edge that needs it: the ends of the short line across.
    let spot = |p: Vec3| [(p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64];
    let mut across: BTreeMap<[i64; 3], (Vec2, Vec2)> = BTreeMap::new();
    for (e, info) in edges.iter().enumerate() {
        if info.degenerate {
            continue;
        }
        let at = topo.edge_faces.get(e).map_or(&[][..], Vec::as_slice);
        let on: Vec<u32> = at.iter().copied().filter(|f| charts.contains_key(f)).collect();
        // The line where a round face meets itself: it goes all the way round there.
        if let ([_], [f]) = (at, on.as_slice())
            && matches!(charts.get(f), Some(Chart::Bend { .. }))
        {
            return Err("The sheet closes on itself (a box section, or a tube), so it cannot be laid flat: it needs an open seam somewhere.".into());
        }
        match on.as_slice() {
            [f] => {
                let Some(chart) = charts.get(f) else { continue };
                // What a punch has formed is marked in the flat, and its rim is no outline.
                if at.iter().any(&formed_face) {
                    continue;
                }
                curves.push(flatten(k, shape.edge(e as u32), info, chart, opts.deflection)?);
            }
            [f, g, ..] if f != g => {
                let (Some(cf), Some(cg)) = (charts.get(f), charts.get(g)) else { continue };
                if (cf.turned() || cg.turned()) && matches!(info.curve, CurveKind::Circle { .. }) {
                    for p in [info.start, info.end] {
                        let (a, b) = (cf.map(p), cg.map(p));
                        apart = apart.max(a.dist(b));
                        if a.dist(b) >= JOIN {
                            across.insert(spot(p), (a, b));
                        }
                    }
                    for (face, chart) in [(f, cf), (g, cg)] {
                        if chart.turned() {
                            let line = flatten(k, shape.edge(e as u32), info, chart, opts.deflection)?;
                            extents.entry(*face).or_default().extend(line.points(MARK_STEP).windows(2).map(|w| [w[0], w[1]]));
                        }
                    }
                    continue;
                }
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
    curves.extend(across.values().map(|(a, b)| FlatCurve::Line { start: *a, end: *b }));
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
        let (rho, inner, radius, slope, tube) = match chart {
            Chart::Bend { rho, inner, radius, .. } => (*rho, *inner, *radius, 0.0, false),
            // (A flat ring is not bent at all.)
            Chart::Turned { along: Along::Ring { .. }, .. } => continue,
            Chart::Turned { rho, inner, slope, along, .. } => (*rho, *inner, 0.0, *slope, matches!(along, Along::Tube { .. })),
            _ => continue,
        };
        // The face's rim as runs of (angle, along) points.
        let mut rim: Vec<Vec<(f64, f64)>> = Vec::new();
        for &e in topo.face_edges.get(*f as usize).map_or(&[][..], Vec::as_slice) {
            let Some(info) = edges.get(e as usize).filter(|i| !i.degenerate) else { continue };
            let straight = match &info.curve {
                CurveKind::Line { .. } => true,
                CurveKind::Circle { axis, .. } => match chart {
                    Chart::Bend { axis: about, .. } | Chart::Turned { axis: about, .. } => axis.dir().cross(about.dir()).len() < SMOOTH,
                    _ => false,
                },
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
        // A round part of a rolled profile is a bend the other way: round the tube, along the
        // sheet, its centre line going round the axis.
        if let (true, Chart::Turned { along: Along::Tube { neutral, .. }, .. }) = (tube, chart) {
            let reach = || rim.iter().flatten().map(|p| p.1);
            let (near, far) = (reach().fold(f64::INFINITY, f64::min), reach().fold(f64::NEG_INFINITY, f64::max));
            let halfway = (near + far) / 2.0;
            let pieces = ((hi - lo) / MARK_STEP).ceil().max(1.0) as usize;
            let on = |i: usize| chart.unrolled(lo + (hi - lo) * i as f64 / pieces as f64, halfway);
            bends.push(Bend {
                lines: (0..pieces).map(|i| [on(i), on(i + 1)]).collect(),
                extents: extents.remove(f).unwrap_or_default(),
                angle: (far - near) / neutral,
                radius: neutral - opts.k_factor * t,
                up: inner,
                allowance: far - near,
                face: *f,
            });
            continue;
        }
        // A cone's neutral radius changes along it: what it comes to halfway along.
        let (rho, radius) = if let Chart::Turned { along: Along::Cone { cos, .. }, .. } = chart {
            let reach = || rim.iter().flatten().map(|p| p.1);
            let halfway = (reach().fold(f64::INFINITY, f64::min) + reach().fold(f64::NEG_INFINITY, f64::max)) / 2.0;
            let there = rho + slope * halfway;
            (there, there - opts.k_factor * t * cos)
        } else {
            (rho, if inner { radius } else { radius - t })
        };
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
            radius,
            up: inner,
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

    // What each stamp of a punch shows: where it is, and for a formed shape where it bends.
    let mut marks: Vec<PunchMark> = Vec::new();
    for p in &extras.punches {
        let mut pieces: Vec<FlatCurve> = Vec::new();
        let mut lines: Vec<[Vec2; 2]> = Vec::new();
        // Its own point, when the face it is on is still where it was stamped (a fold made after
        // it moves the face, and not the sketch).
        let mut own: Option<Vec2> = None;
        for (e, info) in edges.iter().enumerate().filter(|(_, i)| !i.degenerate) {
            let at = topo.edge_faces.get(e).map_or(&[][..], Vec::as_slice);
            let Some(chart) = at.iter().find_map(|f| charts.get(f)).filter(|_| at.iter().any(|f| p.faces.contains(f))) else { continue };
            if let (None, Some(point), Chart::Plane { o3, u3, v3, .. }) = (own, p.at, chart)
                && (point - *o3).dot(u3.cross(*v3)).abs() <= t * (1.0 + 1e-6)
            {
                own = Some(chart.map(point));
            }
            let curve = flatten(k, shape.edge(e as u32), info, chart, opts.deflection)?;
            if p.formed && at.iter().any(|f| p.bends.contains(f)) {
                lines.extend(curve.points(MARK_STEP).windows(2).map(|w| [w[0], w[1]]));
            }
            pieces.push(curve);
        }
        if pieces.is_empty() {
            continue;
        }
        // Its own point; else the middle of what it is: a round shape's centre, however much of
        // it is left, or else the middle of the box round it (every other shape is even about its
        // centre).
        let round = own.or(pieces.iter().find_map(|c| match c {
            FlatCurve::Circle { center, .. } => Some(*center),
            FlatCurve::Arc { center, .. } if c.sweep().abs() > std::f64::consts::PI + 0.01 => Some(*center),
            _ => None,
        }));
        let center = round.unwrap_or_else(|| {
            let points: Vec<Vec2> = pieces.iter().flat_map(|c| c.points(0.2)).collect();
            let least = points.iter().fold(Vec2::new(f64::INFINITY, f64::INFINITY), |a, q| a.min(*q));
            let most = points.iter().fold(Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), |a, q| a.max(*q));
            (least + most) / 2.0
        });
        marks.push(PunchMark { name: p.name.clone(), center, formed: p.formed, lines });
    }

    // What the pattern is not exact about.
    let mut notes: Vec<String> = Vec::new();
    let stretched = charts.values().any(|c| matches!(c, Chart::Turned { along: Along::Tube { .. }, .. }));
    if let Some(bow) = bow {
        notes.push(format!(
            "This shape does not unroll exactly: only one cylinder, cone or flat ring can be laid flat without stretching. Each line and arc of its profile is unrolled at its neutral surface, row by row, each row as long as it is round the axis, so its lengths across and round and its area are right. The rims of the pieces are drawn straight: up to {} mm from where they would lie on each piece alone. The sheet stretches to make up the difference.",
            crate::explain::mm((bow * 100.0).round() / 100.0)
        ));
    } else if stretched || apart >= JOIN {
        notes.push(format!(
            "This shape does not unroll exactly: only one cylinder, cone or flat ring can be laid flat without stretching. Each line and arc of its profile is unrolled on its own at its neutral surface; the pieces touch in their middles and are up to {} mm apart at their ends, and the round parts stretch to fit.",
            crate::explain::mm(apart)
        ));
    }
    let used: BTreeSet<*const Skin> =
        charts.values().filter_map(|c| if let Chart::Skin { skin, .. } = c { Some(Arc::as_ptr(skin)) } else { None }).collect();
    notes.extend(extras.skins.iter().filter(|s| used.contains(&Arc::as_ptr(s))).map(|s| s.words.clone()));

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
    for m in &mut marks {
        m.center += shift;
        for seg in &mut m.lines {
            *seg = [seg[0] + shift, seg[1] + shift];
        }
    }
    marks.sort_by(|a, b| a.center.x.total_cmp(&b.center.x).then(a.center.y.total_cmp(&b.center.y)));
    // A steady order: by where each bend's centre line is.
    let place = |b: &Bend| b.lines.first().map_or((f64::INFINITY, f64::INFINITY), |l| (l[0].x.min(l[1].x), l[0].y.min(l[1].y)));
    bends.sort_by(|a, b| {
        let (pa, pb) = (place(a), place(b));
        pa.0.total_cmp(&pb.0).then(pa.1.total_cmp(&pb.1)).then(a.face.cmp(&b.face))
    });
    let area = outer.signed_area() - inner.iter().map(|l| l.signed_area().abs()).sum::<f64>();
    let cut_length = outer.length() + inner.iter().map(FlatLoop::length).sum::<f64>();
    Ok(FlatPattern {
        outer,
        inner,
        bends,
        thickness: t,
        k_factor: opts.k_factor,
        length: hi.x - lo.x,
        width: hi.y - lo.y,
        area,
        cut_length,
        notes,
        marks,
    })
}
