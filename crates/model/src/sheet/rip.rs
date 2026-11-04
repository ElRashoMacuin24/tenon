//! A seam opened in a face of the sheet: what a rip takes out.
//!
//! The rip is laid out on the face it is on, flat or round, as the face lies in the flat: a slit
//! is a slot as wide as the gap there. On a bend its sides run square to the sheet, so a slit is
//! the same width through the thickness in the flat pattern. Each is one [`Tool`], as a cut
//! across bends is.

use std::f64::consts::PI;

use tenon_geom::{Frame, Vec2, Vec3};
use tenon_kernel::{CurveKind, Kernel, ShapeHandle, SurfaceKind};

use super::RipWay;
use super::flatten::rim_of;
use super::outline::{Piece, polygon};
use super::region::{self, Shape};
use super::unfold::{Chart, Read, Unfold, base_chart, bend_side, radial, read};
use super::wrap::{Tool, has_other_side, outline_of};

/// What names a slit's two sides and its two ends.
const SIDE_A: u64 = 1;
const END_A: u64 = 2;
const SIDE_B: u64 = 3;
const END_B: u64 = 4;

/// A point this close to the edge of a face is on it (mm).
const ON_EDGE: f64 = 1e-6;
/// How many thicknesses square to a face a point of a rip may be and still be on it: what was picked
/// on a face stays where it was when a change of the rule moves the face.
const OFF_THE_FACE: f64 = 10.0;

/// Why a point of a rip cannot be used.
fn off_face(what: &str, p: Vec3) -> String {
    format!(
        "The {what} of the rip, ({}, {}, {}), is not on the face it was picked on: pick it again.",
        crate::explain::mm(p.x),
        crate::explain::mm(p.y),
        crate::explain::mm(p.z)
    )
}

/// The slot `half` either side of the line from `p` to `q`, its sides and ends tagged.
fn slot(p: Vec2, q: Vec2, half: f64) -> Vec<Piece> {
    let side = (q - p).normalized().perp() * half;
    polygon(&[(p - side, SIDE_A), (q - side, END_A), (q + side, SIDE_B), (p + side, END_B)])
}

/// The tools that open `way` in face `face` of the sheet `shape`, with a slit `gap` wide.
/// `along` is the direction a slit through a point runs in, when it is told one.
pub fn tools(k: &dyn Kernel, shape: ShapeHandle, face: u32, way: &RipWay, along: Option<Vec3>, gap: f64, opts: &Unfold) -> Result<Vec<Tool>, String> {
    let solid = read(k, shape)?;
    let info = solid.faces.get(face as usize).ok_or("no such face")?;
    if !matches!(way, RipWay::Face) && gap < 1e-6 {
        return Err("A slit with no gap opens nothing: give the rip a gap, or set the part's Miter Gap in Sheet Metal Defaults.".into());
    }
    match &info.surface {
        SurfaceKind::Plane { .. } if has_other_side(&solid, face as usize, opts.thickness) => flat(k, shape, &solid, face, way, along, gap, opts),
        SurfaceKind::Cylinder { .. } if bend_side(face as usize, &solid.faces, opts.thickness).is_some() => {
            if along.is_some() {
                return Err("A slit in a round face runs along its axis, so it runs alongside no edge: take the edge out of the rip.".into());
            }
            round(k, shape, &solid, face, way, gap, opts)
        }
        _ => Err(
            "A rip goes in a side of the sheet, flat or round, and the face picked is neither: it is the rim of the sheet, or not one thickness from another side. Pick the face you would see looking at the sheet."
                .into(),
        ),
    }
}

/// The way a slit through a point runs when it is not told: alongside a bend beside the face, so
/// that it goes from rim to rim and leaves the bend whole; without one, across the face's short
/// way, square to its longest straight edge or along it.
fn slit_way(solid: &Read, face: u32, outline: &[Piece], u3: Vec3, v3: Vec3, t: f64) -> Vec2 {
    let bent = solid.topo.face_edges.get(face as usize).map_or(&[][..], Vec::as_slice).iter().find_map(|&e| {
        let info = solid.edges.get(e as usize)?;
        if !matches!(info.curve, CurveKind::Line { .. }) {
            return None;
        }
        let beside = solid.topo.edge_faces.get(e as usize)?.iter().find(|g| **g != face)?;
        let SurfaceKind::Cylinder { axis, .. } = &solid.faces.get(*beside as usize)?.surface else { return None };
        (axis.dir().cross(info.end - info.start).len() < 1e-6 * info.length.max(1.0) && bend_side(*beside as usize, &solid.faces, t).is_some())
            .then_some(info.end - info.start)
    });
    if let Some(d) = bent {
        return Vec2::new(d.dot(u3), d.dot(v3)).normalized();
    }
    let longest = outline.iter().filter(|p| p.arc.is_none()).max_by(|x, y| x.a.dist(x.b).total_cmp(&y.a.dist(y.b)));
    let Some(longest) = longest else { return Vec2::X };
    let d = (longest.b - longest.a).normalized();
    let reach = |w: Vec2| {
        let at = outline.iter().flat_map(|p| [p.a, p.b]).map(|p| p.dot(w));
        at.clone().fold(f64::NEG_INFINITY, f64::max) - at.fold(f64::INFINITY, f64::min)
    };
    if reach(d.perp()) <= reach(d) { d.perp() } else { d }
}

/// A rip on a flat side of the sheet.
#[allow(clippy::too_many_arguments)]
fn flat(
    k: &dyn Kernel,
    shape: ShapeHandle,
    solid: &Read,
    face: u32,
    way: &RipWay,
    along: Option<Vec3>,
    gap: f64,
    opts: &Unfold,
) -> Result<Vec<Tool>, String> {
    let info = solid.faces.get(face as usize).ok_or("no such face")?;
    let SurfaceKind::Plane { normal, .. } = &info.surface else { return Err("the face of a rip is not flat".into()) };
    let chart = base_chart(info).ok_or("the face of a rip is not flat")?;
    let Chart::Plane { o3, u3, v3, .. } = &chart else { return Err("the face of a rip is not flat".into()) };
    let outline = outline_of(k, shape, solid, face, &chart, opts)?;
    let loops = [outline.clone()];
    let is_on = |p: Vec2| region::inside(&loops, p) || region::distance_to(&loops, p) <= ON_EDGE;
    let in_reach = |at: Vec3| (at - *o3).dot(normal.normalized()).abs() <= OFF_THE_FACE * opts.thickness;
    let shapes: Vec<Shape> = match way {
        RipWay::Face => vec![Shape { outer: outline, holes: Vec::new() }],
        RipWay::Point { at, .. } => {
            let p = chart.map(*at);
            if !is_on(p) || !in_reach(*at) {
                return Err(off_face("point", *at));
            }
            let d = match along {
                Some(d) => {
                    let seen = Vec2::new(d.dot(*u3), d.dot(*v3));
                    if seen.len() < 1e-6 * d.len() {
                        return Err("The edge a rip runs alongside stands square to the face, so a slit cannot run that way: pick an edge in the plane of the face.".into());
                    }
                    seen.normalized()
                }
                None => slit_way(solid, face, &outline, *u3, *v3, opts.thickness),
            };
            let (lo, hi) = region::bounds(&loops);
            let far = lo.dist(hi) + 1.0;
            let mut through = region::common(&[slot(p - d * far, p + d * far, gap / 2.0)], &loops).map_err(|why| touches(&why))?;
            // The stretch of it the point is on: the slit does not jump across a gap in the face.
            through.sort_by(|x, y| x.away(p).total_cmp(&y.away(p)));
            through.truncate(1);
            through
        }
        RipWay::Between { from, to } => {
            let (p, q) = (chart.map(*from), chart.map(*to));
            for (what, at, seen) in [("first point", *from, p), ("second point", *to, q)] {
                if !is_on(seen) || !in_reach(at) {
                    return Err(off_face(what, at));
                }
            }
            if p.dist(q) < 1e-6 {
                return Err("The two points of the rip are one point: a slit runs from one to another.".into());
            }
            let d = (q - p).normalized();
            // A point on the edge of the face takes the slit through that edge.
            let past = |at: Vec2| if region::distance_to(&loops, at) <= ON_EDGE { gap + opts.thickness } else { 0.0 };
            region::common(&[slot(p - d * past(p), q + d * past(q), gap / 2.0)], &loops).map_err(|why| touches(&why))?
        }
    };
    if shapes.is_empty() {
        return Err("A slit there takes nothing of the face: move the point a little.".into());
    }
    let frame = Frame::new(*o3, u3.cross(*v3), *u3).ok_or("the face of a rip has no direction")?;
    // Into the sheet: against the way the face looks.
    let reach = if frame.z().dot(*normal) > 0.0 { -opts.thickness } else { opts.thickness };
    Ok(vec![Tool::Flat { face, frame, reach, shapes }])
}

/// Why a slit and a face could not be told apart.
fn touches(why: &str) -> String {
    format!("The slit and the edge of the face have {why} between them: the slit just touches an edge of the face. Move it a little.")
}

/// A rip on one side of a bend, or of a tube. It goes right through both sides of it.
fn round(k: &dyn Kernel, shape: ShapeHandle, solid: &Read, face: u32, way: &RipWay, gap: f64, opts: &Unfold) -> Result<Vec<Tool>, String> {
    let t = opts.thickness;
    let info = solid.faces.get(face as usize).ok_or("no such face")?;
    let SurfaceKind::Cylinder { axis, radius } = &info.surface else { return Err("the face of a rip is not round".into()) };
    let inner = bend_side(face as usize, &solid.faces, t).ok_or("the face of a rip is no side of the sheet")?;
    let inside = if inner { *radius } else { radius - t };
    let rho = inside + opts.k_factor * t;
    let a = axis.dir();
    // How far round and along the face reaches, from the middle of it; all the way round, a tube
    // has no middle, and is measured from the first point of the rip.
    let in_reach = |what: &str, at: Vec3| -> Result<(), String> {
        let Some(out) = radial(axis, at) else {
            return Err(format!(
                "The {what} of the rip is on the axis of the round face it was picked on, not on the face, so it has no place round it: pick it again."
            ));
        };
        if ((at - axis.origin()).dot(out) - radius).abs() > OFF_THE_FACE * t {
            return Err(off_face(what, at));
        }
        Ok(())
    };
    let picked = match way {
        RipWay::Point { at, .. } => {
            in_reach("point", *at)?;
            radial(axis, *at)
        }
        RipWay::Between { from, to } => {
            in_reach("first point", *from)?;
            in_reach("second point", *to)?;
            radial(axis, *from)
        }
        RipWay::Face => None,
    };
    let middle = radial(axis, info.centroid);
    let Some(r0) = middle.or(picked) else {
        return Err("Taking out the whole of a tube leaves nothing of it. Open it with a slit: pick a point on it.".into());
    };
    let chart = |r0: Vec3| Chart::Bend {
        q: axis.origin() + r0 * *radius,
        axis: *axis,
        r0,
        mid: 0.0,
        slant: 0.0,
        rho,
        o2: Vec2::ZERO,
        ea: Vec2::Y,
        eg: Vec2::X,
        inner,
        radius: *radius,
    };
    let here = chart(r0);
    let (mut turn, run) = rim_of(k, shape, solid, face, &here, axis, opts)?;
    if run.1 - run.0 <= 1e-9 || !run.0.is_finite() {
        return Err("This face of the sheet could not be read: it has no length along its axis.".into());
    }
    // A face with no middle goes all the way round.
    let whole = middle.is_none();
    if whole {
        turn = (-PI, PI);
    }
    let margin = inside.min(t) / 2.0;
    let on_face =
        |p: Vec2| (whole || (turn.0 * rho - ON_EDGE..=turn.1 * rho + ON_EDGE).contains(&p.x)) && (run.0 - ON_EDGE..=run.1 + ON_EDGE).contains(&p.y);
    // A tool runs past the ends of the face, but stops short of the next piece of the bend on the
    // same axis, at the middle of the slot between them.
    let (mut below, mut above) = (margin, margin);
    for (i, g) in solid.faces.iter().enumerate() {
        let SurfaceKind::Cylinder { axis: b, .. } = &g.surface else { continue };
        let apart = b.origin() - axis.origin();
        let Ok(other) = u32::try_from(i) else { continue };
        if other == face
            || a.cross(b.dir()).len() > 1e-6
            || (apart - a * apart.dot(a)).len() > 1e-6 * (1.0 + inside + t)
            || bend_side(i, &solid.faces, t).is_none()
        {
            continue;
        }
        let (_, there) = rim_of(k, shape, solid, other, &here, axis, opts)?;
        if there.0 >= run.1 - 1e-9 {
            above = above.min(((there.0 - run.1) / 2.0).max(0.0));
        }
        if there.1 <= run.0 + 1e-9 {
            below = below.min(((run.0 - there.1) / 2.0).max(0.0));
        }
    }
    let (from_y, to_y) = (run.0 - below, run.1 + above);
    let frame = Frame::new(axis.origin(), a, r0).ok_or("the bend of a rip has no direction")?;
    // The slit, in the flat of the face: past its ends along the bend, and no further round than
    // the bend goes.
    let tool = |outline: Vec<Piece>| Tool::Bend { face, frame, rho, inner: inside - margin, outer: inside + t + margin, outline };
    match way {
        RipWay::Face => {
            let c = |x: f64, y: f64| (Vec2::new(x * rho, y), SIDE_A);
            Ok(vec![tool(polygon(&[c(turn.0, from_y), c(turn.1, from_y), c(turn.1, to_y), c(turn.0, to_y)]))])
        }
        RipWay::Point { at, .. } => {
            let p = here.map(*at);
            if !on_face(p) {
                return Err(off_face("point", *at));
            }
            let half = gap / 2.0;
            let (from, to) = if whole { (p.x - half, p.x + half) } else { ((p.x - half).max(turn.0 * rho), (p.x + half).min(turn.1 * rho)) };
            if to - from < 1e-6 {
                return Err("A slit there takes nothing of the bend: move the point a little.".into());
            }
            let c = |x: f64, y: f64, tag: u64| (Vec2::new(x, y), tag);
            Ok(vec![tool(polygon(&[c(from, from_y, END_B), c(to, from_y, SIDE_B), c(to, to_y, END_A), c(from, to_y, SIDE_A)]))])
        }
        RipWay::Between { from, to } => {
            let (p, q) = (here.map(*from), here.map(*to));
            for (what, at, seen) in [("first point", *from, p), ("second point", *to, q)] {
                if !on_face(seen) {
                    return Err(off_face(what, at));
                }
            }
            if p.dist(q) < 1e-6 {
                return Err("The two points of the rip are one point: a slit runs from one to another.".into());
            }
            let d = (q - p).normalized();
            let on_rim = |at: Vec2| {
                (at.y - run.0).abs() <= ON_EDGE
                    || (at.y - run.1).abs() <= ON_EDGE
                    || (!whole && ((at.x - turn.0 * rho).abs() <= ON_EDGE || (at.x - turn.1 * rho).abs() <= ON_EDGE))
            };
            let past = |at: Vec2| if on_rim(at) { gap + t } else { 0.0 };
            let c = |x: f64, y: f64| (Vec2::new(x, y), SIDE_A);
            let bound = [polygon(&[c(turn.0 * rho, from_y), c(turn.1 * rho, from_y), c(turn.1 * rho, to_y), c(turn.0 * rho, to_y)])];
            let slit = [slot(p - d * past(p), q + d * past(q), gap / 2.0)];
            let shapes = region::common(&slit, &bound).map_err(|why| touches(&why))?;
            if shapes.is_empty() {
                return Err("A slit there takes nothing of the bend: move the points a little.".into());
            }
            Ok(shapes.into_iter().map(|s| tool(s.outer)).collect())
        }
    }
}
