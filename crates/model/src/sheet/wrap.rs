//! A profile wrapped onto the sheet: what a cut across bends takes out.
//!
//! The sheet is laid flat from the face the profile's sketch is on, in the sketch's own
//! coordinates, so the profile is drawn on the flat pattern as it stands. Each flat face and
//! each bend of that side of the sheet then takes the part of the profile that lies on it: a
//! face as an outline in its own plane, a bend as an outline to be rolled round its axis
//! (`Kernel::roll`). In the flat pattern the cut is the profile, whatever it crosses.

use tenon_geom::{Frame, Vec2, Vec3};
use tenon_kernel::{CurveKind, Kernel, Region, ShapeHandle, SurfaceKind};

use super::flat::chain;
use super::outline::{Piece, polygon};
use super::region::{self, Shape};
use super::unfold::{Chart, Read, Unfold, flatten, follow, read};
use crate::project::Projected;

/// What takes the profile out of one face of the sheet.
#[derive(Clone, Debug)]
pub enum Tool {
    /// Out of a flat face: `shapes` in the XY plane of `frame` (the face's own side of the sheet,
    /// laid out as the profile's sketch has it), pushed `reach` along its Z into the sheet.
    Flat { face: u32, frame: Frame, reach: f64, shapes: Vec<Shape> },
    /// Out of a bend: `outline` rolled in `frame` (as `Kernel::roll` takes it) at the bend's
    /// neutral radius `rho`, between the radii `inner` and `outer`.
    Bend { face: u32, frame: Frame, rho: f64, inner: f64, outer: f64, outline: Vec<Piece> },
}

impl Tool {
    /// The face of the solid (kernel index) it cuts into.
    pub fn face(&self) -> u32 {
        match self {
            Tool::Flat { face, .. } | Tool::Bend { face, .. } => *face,
        }
    }
}

/// Whether the boxes of two sets of loops share anything.
fn near(a: &[Vec<Piece>], b: &[Vec<Piece>]) -> bool {
    let ((alo, ahi), (blo, bhi)) = (region::bounds(a), region::bounds(b));
    alo.x <= bhi.x && blo.x <= ahi.x && alo.y <= bhi.y && blo.y <= ahi.y
}

/// The profile as loops of pieces, region by region: each an outer loop and its holes.
fn drawn(profile: &[Region]) -> Result<Vec<Vec<Vec<Piece>>>, String> {
    let mut out = Vec::new();
    for r in profile {
        let loops: Result<Vec<Vec<Piece>>, String> = std::iter::once(&r.outer).chain(&r.holes).map(region::of_loop).collect();
        out.push(loops.map_err(|what| {
            format!("The profile has {what} in it, and a cut across bends takes profiles of lines, arcs and circles only: draw it with those, or cut it without Cut Across Bend.")
        })?);
    }
    Ok(out)
}

/// The outer boundary of flat face `f` as its chart lays it out. (A hole in the face is no
/// sheet to cut, and the floor of a pocket is sheet under the profile like any other.)
fn outline_of(k: &dyn Kernel, shape: ShapeHandle, solid: &Read, f: u32, chart: &Chart, opts: &Unfold) -> Result<Vec<Piece>, String> {
    let mut curves = Vec::new();
    for &e in solid.topo.face_edges.get(f as usize).map_or(&[][..], Vec::as_slice) {
        let Some(info) = solid.edges.get(e as usize).filter(|i| !i.degenerate) else { continue };
        curves.push(flatten(k, shape.edge(e), info, chart, opts.deflection)?);
    }
    let (outer, _) =
        chain(curves).map_err(|_| "A face of the sheet has an outline that cannot be followed, so the profile cannot be laid on it.".to_owned())?;
    Ok(region::of_flat(&outer, 0))
}

/// The chart of a face that lies in the plane of `sketch`: the sketch's own coordinates.
fn in_sketch(sketch: &Frame) -> Chart {
    Chart::Plane { o3: sketch.origin(), u3: sketch.x(), v3: sketch.y(), o2: Vec2::ZERO, u2: Vec2::X, v2: Vec2::Y }
}

/// What of the profile lies on the region `here`.
fn shared(drawn: &[Vec<Vec<Piece>>], here: &[Vec<Piece>]) -> Result<Vec<Shape>, String> {
    let mut shapes = Vec::new();
    for loops in drawn.iter().filter(|loops| near(loops, here)) {
        shapes.extend(region::common(loops, here).map_err(|why| {
            format!(
                "The profile and the sheet have {why} between them: the profile just touches an edge of the sheet or a bend line, or crosses itself. Move it a little, or draw it clear across."
            )
        })?);
    }
    Ok(shapes)
}

/// Whether face `i` of the solid is a side of the sheet that lies in the plane of `sketch`: a
/// flat face in that plane with the sheet's other side one thickness behind it (the rim of the
/// sheet has none).
fn side_in(solid: &Read, i: usize, sketch: &Frame, t: f64) -> bool {
    let Some(face) = solid.faces.get(i) else { return false };
    let SurfaceKind::Plane { normal, .. } = &face.surface else { return false };
    let n = normal.normalized();
    if n.cross(sketch.z()).len() > 1e-6 || (face.centroid - sketch.origin()).dot(sketch.z()).abs() > 1e-6 {
        return false;
    }
    solid.faces.iter().any(|g| {
        matches!(&g.surface, SurfaceKind::Plane { normal: m, .. } if m.normalized().dot(n) < -1.0 + 1e-9 && ((g.centroid - face.centroid).dot(n) + t).abs() < 1e-6 * (1.0 + t))
    })
}

/// Whether face `face` of `shape` is a side of the sheet in the plane of `sketch`: the face a
/// sketch put on it lies on.
pub fn is_seat(k: &dyn Kernel, shape: ShapeHandle, face: u32, sketch: &Frame, opts: &Unfold) -> Result<bool, String> {
    Ok(side_in(&read(k, shape)?, face as usize, sketch, opts.thickness))
}

/// The side of the sheet `shape` that the sketch lies on: of its sides in the plane of `sketch`,
/// the one with most of the profile on it (and how much of it, mm²), or the one nearest the
/// profile (the sketch's origin, without one) when none has any. None when no side of the sheet
/// is in that plane.
pub fn seat(k: &dyn Kernel, shape: ShapeHandle, sketch: &Frame, profile: &[Region], opts: &Unfold) -> Result<Option<(u32, f64)>, String> {
    let solid = read(k, shape)?;
    let drawn = drawn(profile)?;
    let all: Vec<Vec<Piece>> = drawn.iter().flatten().cloned().collect();
    let middle = if all.is_empty() {
        Vec2::ZERO
    } else {
        let (lo, hi) = region::bounds(&all);
        (lo + hi) / 2.0
    };
    let chart = in_sketch(sketch);
    let mut best: Option<(u32, f64, f64)> = None;
    for (i, face) in solid.faces.iter().enumerate() {
        if !side_in(&solid, i, sketch, opts.thickness) {
            continue;
        }
        let Ok(f) = u32::try_from(i) else { continue };
        let here = [outline_of(k, shape, &solid, f, &chart, opts)?];
        let share: f64 = shared(&drawn, &here).map_or(0.0, |shapes| shapes.iter().map(Shape::area).sum());
        let away = chart.map(face.centroid).dist(middle);
        if best.is_none_or(|(_, most, nearest)| share > most + 1e-9 || ((share - most).abs() <= 1e-9 && away < nearest)) {
            best = Some((f, share, away));
        }
    }
    Ok(best.map(|(f, share, _)| (f, share)))
}

/// Where edge `e` of `shape` lies in the plane of `sketch` when the sheet is laid flat from its
/// face `base` (the side the sketch lies on): what Project Flat Pattern shows of it. An edge
/// that is not on that side of the sheet, or would be neither a line nor an arc, is an error.
pub fn flat_edge(k: &dyn Kernel, shape: ShapeHandle, base: u32, sketch: &Frame, e: u32, opts: &Unfold) -> Result<Projected, String> {
    let solid = read(k, shape)?;
    let charts = follow(&solid, base, in_sketch(sketch), opts)?;
    let info = solid.edges.get(e as usize).ok_or("no such edge")?;
    let on = solid.topo.edge_faces.get(e as usize).into_iter().flatten().find_map(|f| charts.get(f));
    let chart = on.ok_or("it is not on the side of the sheet the sketch is on, so it has no place on that side laid flat")?;
    let (start, end) = (chart.map(info.start), chart.map(info.end));
    let line = || if start.dist(end) < 1e-7 { Projected::Point { at: start } } else { Projected::Line { start, end } };
    match (&info.curve, chart) {
        (CurveKind::Line { .. }, _) => Ok(line()),
        // Round a bend, about its axis: straight in the flat.
        (CurveKind::Circle { axis, .. }, Chart::Bend { axis: bend, .. })
            if axis.dir().cross(bend.dir()).len() < 1e-6 && info.start.dist(info.end) >= 1e-7 =>
        {
            Ok(line())
        }
        (CurveKind::Circle { axis, radius }, Chart::Plane { u3, v3, .. }) if axis.dir().dot(u3.cross(*v3)).abs() > 1.0 - 1e-9 => {
            let center = chart.map(axis.origin());
            if info.start.dist(info.end) < 1e-7 {
                return Ok(Projected::Circle { center, radius: *radius });
            }
            // Counter-clockwise from one end to the other, whichever way the chart turns it.
            let turn = |p: Vec2| ((p - center).angle() - (start - center).angle()).rem_euclid(std::f64::consts::TAU);
            Ok(if turn(chart.map(info.mid)) < turn(end) {
                Projected::Arc { center, start, end }
            } else {
                Projected::Arc { center, start: end, end: start }
            })
        }
        _ => Err("laid flat it would be neither a line nor an arc, and sketches have no other curves to show it by yet".into()),
    }
}

/// The tools that take `profile` (regions in the plane of `sketch`) out of the sheet `shape`,
/// `depth` deep from the side its face `base` is on: the face the sketch lies on.
///
/// A depth of the whole thickness (or more) goes through. Nothing of the profile on the sheet is
/// an error, said in plain words, as is a profile this cannot wrap.
pub fn wrap(
    k: &dyn Kernel,
    shape: ShapeHandle,
    base: u32,
    sketch: &Frame,
    profile: &[Region],
    depth: f64,
    opts: &Unfold,
) -> Result<Vec<Tool>, String> {
    let t = opts.thickness;
    if !(depth.is_finite() && depth > 0.0) {
        return Err("the depth of a cut must be more than 0".into());
    }
    let through = depth >= t * (1.0 - 1e-9);
    let solid = read(k, shape)?;
    let charts = follow(&solid, base, in_sketch(sketch), opts)?;
    let drawn = drawn(profile)?;

    let mut tools = Vec::new();
    for (&f, chart) in &charts {
        let Some(face) = solid.faces.get(f as usize) else { continue };
        match chart {
            Chart::Plane { o3, u3, v3, o2, u2, v2 } => {
                let SurfaceKind::Plane { normal, .. } = &face.surface else { continue };
                let shapes = shared(&drawn, &[outline_of(k, shape, &solid, f, chart, opts)?])?;
                if shapes.is_empty() {
                    continue;
                }
                // Flat coordinates are this frame's: the flat's x and y as directions of the face.
                let (x, y) = (*u3 * u2.x + *v3 * v2.x, *u3 * u2.y + *v3 * v2.y);
                let origin = *o3 - *u3 * o2.dot(*u2) - *v3 * o2.dot(*v2);
                let z = x.cross(y);
                let frame = Frame::new(origin, z, x).ok_or("a face of the sheet has no direction")?;
                // Into the sheet: against the way the face looks.
                let reach = if z.dot(*normal) > 0.0 { -depth.min(t) } else { depth.min(t) };
                tools.push(Tool::Flat { face: f, frame, reach, shapes });
            }
            Chart::Bend { q, axis, r0, rho, o2, ea, eg, inner, radius, .. } => {
                // The bend in the flat: as far round as it turns, as far along as it reaches.
                let (mut turn, mut run) = ((f64::INFINITY, f64::NEG_INFINITY), (f64::INFINITY, f64::NEG_INFINITY));
                for &e in solid.topo.face_edges.get(f as usize).map_or(&[][..], Vec::as_slice) {
                    let Some(info) = solid.edges.get(e as usize).filter(|i| !i.degenerate) else { continue };
                    let straight = match &info.curve {
                        CurveKind::Line { .. } => true,
                        CurveKind::Circle { axis: about, .. } => about.dir().cross(axis.dir()).len() < 1e-6,
                        _ => false,
                    };
                    let points: Vec<Vec3> = if straight {
                        vec![info.start, info.mid, info.end]
                    } else {
                        k.edge_points(shape.edge(e), opts.deflection).map_err(|e| e.to_string())?
                    };
                    for p in points {
                        let (angle, along) = chart.round(p);
                        turn = (turn.0.min(angle), turn.1.max(angle));
                        run = (run.0.min(along), run.1.max(along));
                    }
                }
                if (turn.1 - turn.0).partial_cmp(&1e-9) != Some(std::cmp::Ordering::Greater) || run.1 - run.0 <= 1e-9 {
                    continue;
                }
                let corner = |angle: f64, along: f64| (chart.unrolled(angle, along), 0);
                let here = [polygon(&[corner(turn.0, run.0), corner(turn.1, run.0), corner(turn.1, run.1), corner(turn.0, run.1)])];
                let shapes = shared(&drawn, &here)?;
                if shapes.is_empty() {
                    continue;
                }
                if shapes.iter().any(|s| !s.holes.is_empty()) {
                    return Err("The profile has an island that lies on a bend, and a cut across bends cannot leave one there yet: keep the island on a flat face, or cut round it with two profiles.".into());
                }
                // Into the coordinates the bend is rolled in: round the axis from where the
                // bend's chart starts, and along it.
                let a = axis.dir();
                let origin = axis.origin() + a * (*q - axis.origin()).dot(a);
                let frame = Frame::new(origin, a, *r0).ok_or("a bend of the sheet has no direction")?;
                let rolled = |p: Vec2| Vec2::new((p - *o2).dot(*eg), (p - *o2).dot(*ea));
                let mirrored = eg.cross(*ea) < 0.0;
                // Clear of the bend's own faces by a margin, on the sides it goes right through.
                let inside = if *inner { *radius } else { radius - t };
                let margin = inside.min(t) / 2.0;
                let (lo, hi) = match (through, *inner) {
                    (true, _) => (inside - margin, inside + t + margin),
                    (false, true) => (inside - margin, inside + depth),
                    (false, false) => (inside + t - depth, inside + t + margin),
                };
                for s in shapes {
                    tools.push(Tool::Bend { face: f, frame, rho: *rho, inner: lo, outer: hi, outline: region::moved(&s.outer, rolled, mirrored) });
                }
            }
        }
    }
    if tools.is_empty() {
        return Err("The profile is not on the sheet: laid flat from the face its sketch is on, no face and no bend lies under it.".into());
    }
    Ok(tools)
}
