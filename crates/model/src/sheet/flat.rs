//! A flat pattern: the outline of a sheet metal part laid flat, with its bend lines.
//!
//! Plain 2D data in millimetres, seen from the side the pattern was laid out from (the A side),
//! with its lower left corner at the origin.

use std::collections::BTreeMap;
use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};
use tenon_geom::Vec2;

/// Two points of a flat pattern are the same point when they are this close (mm).
pub const JOIN: f64 = 1e-6;

/// How far from a point where outlines touch the sheet is looked for (mm).
const PINCH_PROBE: f64 = 0.01;

/// One curve of an outline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FlatCurve {
    Line {
        start: Vec2,
        end: Vec2,
    },
    /// Part of a circle from `start` to `end`, counter-clockwise when `ccw`.
    Arc {
        center: Vec2,
        radius: f64,
        start: Vec2,
        end: Vec2,
        ccw: bool,
    },
    Circle {
        center: Vec2,
        radius: f64,
    },
    /// A curve that is neither (a cut that runs across a bend), as points close together.
    Polyline {
        points: Vec<Vec2>,
    },
}

impl FlatCurve {
    /// Where the curve starts and ends; a circle has no ends.
    pub fn ends(&self) -> Option<(Vec2, Vec2)> {
        match self {
            FlatCurve::Line { start, end } | FlatCurve::Arc { start, end, .. } => Some((*start, *end)),
            FlatCurve::Circle { .. } => None,
            FlatCurve::Polyline { points } => Some((*points.first()?, *points.last()?)),
        }
    }

    /// The direction the curve sets off in.
    pub fn heading(&self) -> Vec2 {
        match self {
            FlatCurve::Line { start, end } => (*end - *start).normalized(),
            FlatCurve::Arc { center, start, ccw, .. } => {
                let out = (*start - *center).normalized();
                if *ccw { out.perp() } else { -out.perp() }
            }
            FlatCurve::Circle { .. } => Vec2::Y,
            FlatCurve::Polyline { points } => points.get(1).zip(points.first()).map_or(Vec2::X, |(b, a)| (*b - *a).normalized()),
        }
    }

    /// The same curve run the other way.
    pub fn reversed(&self) -> FlatCurve {
        match self {
            FlatCurve::Line { start, end } => FlatCurve::Line { start: *end, end: *start },
            FlatCurve::Arc { center, radius, start, end, ccw } => {
                FlatCurve::Arc { center: *center, radius: *radius, start: *end, end: *start, ccw: !ccw }
            }
            FlatCurve::Circle { .. } => self.clone(),
            FlatCurve::Polyline { points } => FlatCurve::Polyline { points: points.iter().rev().copied().collect() },
        }
    }

    /// The angle an arc turns through: positive counter-clockwise, never zero for a real arc.
    pub fn sweep(&self) -> f64 {
        match self {
            FlatCurve::Arc { center, start, end, ccw, .. } => {
                let (a0, a1) = ((*start - *center).angle(), (*end - *center).angle());
                let mut d = (a1 - a0).rem_euclid(TAU);
                if d < 1e-12 {
                    d = TAU;
                }
                if *ccw { d } else { d - TAU }
            }
            FlatCurve::Circle { .. } => TAU,
            _ => 0.0,
        }
    }

    pub fn length(&self) -> f64 {
        match self {
            FlatCurve::Line { start, end } => start.dist(*end),
            FlatCurve::Arc { radius, .. } => radius * self.sweep().abs(),
            FlatCurve::Circle { radius, .. } => radius * TAU,
            FlatCurve::Polyline { points } => points.windows(2).map(|w| w[0].dist(w[1])).sum(),
        }
    }

    /// What the curve adds to the area inside a loop run counter-clockwise (Green's theorem).
    fn area_term(&self) -> f64 {
        let chord = |a: Vec2, b: Vec2| a.cross(b) / 2.0;
        match self {
            FlatCurve::Line { start, end } => chord(*start, *end),
            FlatCurve::Arc { center, radius, start, .. } => {
                let (a0, d) = ((*start - *center).angle(), self.sweep());
                let a1 = a0 + d;
                0.5 * (center.x * radius * (a1.sin() - a0.sin()) - center.y * radius * (a1.cos() - a0.cos()) + radius * radius * d)
            }
            FlatCurve::Circle { radius, .. } => std::f64::consts::PI * radius * radius,
            FlatCurve::Polyline { points } => points.windows(2).map(|w| chord(w[0], w[1])).sum(),
        }
    }

    /// Points along the curve, its start first and its end last, no further than `step` radians
    /// apart round an arc.
    pub fn points(&self, step: f64) -> Vec<Vec2> {
        let round = |center: Vec2, radius: f64, a0: f64, sweep: f64| -> Vec<Vec2> {
            let n = ((sweep.abs() / step).ceil() as usize).max(2);
            (0..=n).map(|i| Vec2::polar(center, radius, a0 + sweep * i as f64 / n as f64)).collect()
        };
        match self {
            FlatCurve::Line { start, end } => vec![*start, *end],
            FlatCurve::Arc { center, radius, start, end, .. } => {
                let mut pts = round(*center, *radius, (*start - *center).angle(), self.sweep());
                // The ends are the arc's own, to the last digit.
                if let Some(p) = pts.first_mut() {
                    *p = *start;
                }
                if let Some(p) = pts.last_mut() {
                    *p = *end;
                }
                pts
            }
            FlatCurve::Circle { center, radius } => round(*center, *radius, 0.0, TAU),
            FlatCurve::Polyline { points } => points.clone(),
        }
    }

    pub fn moved(&self, by: Vec2) -> FlatCurve {
        match self {
            FlatCurve::Line { start, end } => FlatCurve::Line { start: *start + by, end: *end + by },
            FlatCurve::Arc { center, radius, start, end, ccw } => {
                FlatCurve::Arc { center: *center + by, radius: *radius, start: *start + by, end: *end + by, ccw: *ccw }
            }
            FlatCurve::Circle { center, radius } => FlatCurve::Circle { center: *center + by, radius: *radius },
            FlatCurve::Polyline { points } => FlatCurve::Polyline { points: points.iter().map(|p| *p + by).collect() },
        }
    }

    /// The corners of a box that holds the curve.
    fn bounds(&self) -> (Vec2, Vec2) {
        let pts = self.points(0.02);
        let lo = pts.iter().fold(Vec2::new(f64::INFINITY, f64::INFINITY), |a, p| a.min(*p));
        let hi = pts.iter().fold(Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), |a, p| a.max(*p));
        match self {
            // A circle or an arc reaches its quadrant points exactly.
            FlatCurve::Circle { center, radius } => (*center - Vec2::new(*radius, *radius), *center + Vec2::new(*radius, *radius)),
            FlatCurve::Arc { center, radius, start, .. } => {
                let (a0, d) = ((*start - *center).angle(), self.sweep());
                let (mut lo, mut hi) = (lo, hi);
                for q in 0..4 {
                    let a = f64::from(q) * std::f64::consts::FRAC_PI_2;
                    let along = if d >= 0.0 { (a - a0).rem_euclid(TAU) } else { (a0 - a).rem_euclid(TAU) };
                    if along <= d.abs() {
                        let p = Vec2::polar(*center, *radius, a);
                        lo = lo.min(p);
                        hi = hi.max(p);
                    }
                }
                (lo, hi)
            }
            _ => (lo, hi),
        }
    }
}

/// A closed outline: its curves joined end to end, counter-clockwise.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FlatLoop {
    pub curves: Vec<FlatCurve>,
}

impl FlatLoop {
    /// The area inside, positive when the loop runs counter-clockwise.
    pub fn signed_area(&self) -> f64 {
        self.curves.iter().map(FlatCurve::area_term).sum()
    }
    pub fn length(&self) -> f64 {
        self.curves.iter().map(FlatCurve::length).sum()
    }
    pub fn reversed(&self) -> FlatLoop {
        FlatLoop { curves: self.curves.iter().rev().map(FlatCurve::reversed).collect() }
    }
    /// The loop as a polygon (its first point is not repeated at the end).
    pub fn polygon(&self, step: f64) -> Vec<Vec2> {
        let mut out: Vec<Vec2> = Vec::new();
        for c in &self.curves {
            let pts = c.points(step);
            let skip = usize::from(out.last().is_some_and(|l| pts.first().is_some_and(|f| l.dist(*f) < JOIN)));
            out.extend(pts.into_iter().skip(skip));
        }
        if out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| a.dist(*b) < JOIN) {
            out.pop();
        }
        out
    }
    /// Whether `p` is inside the loop.
    pub fn contains(&self, p: Vec2) -> bool {
        within(&self.polygon(0.05), p)
    }
}

/// Whether `p` is inside the polygon.
fn within(poly: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// One bend of the part, as the flat pattern shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bend {
    /// The bend's centre line: one piece, or more where a cut runs across the bend.
    pub lines: Vec<[Vec2; 2]>,
    /// Where the bend begins and ends in the flat sheet (its tangent lines).
    pub extents: Vec<[Vec2; 2]>,
    /// How far the sheet turns (radians).
    pub angle: f64,
    /// The radius of the inside of the bend.
    pub radius: f64,
    /// Bent towards whoever looks at the pattern (the flange comes up off the table).
    pub up: bool,
    /// The width of the bend in the flat: angle x (radius + K x thickness).
    pub allowance: f64,
    /// The face of the solid this is, as the kernel numbers it.
    pub face: u32,
}

/// A sheet metal part laid flat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlatPattern {
    pub outer: FlatLoop,
    /// Holes and cut-outs, each clockwise (a hole that is one whole circle has no direction).
    pub inner: Vec<FlatLoop>,
    pub bends: Vec<Bend>,
    pub thickness: f64,
    pub k_factor: f64,
    /// The size of the smallest upright rectangle that holds the pattern: along X, along Y.
    pub length: f64,
    pub width: f64,
    /// The sheet's area, holes taken off (mm^2).
    pub area: f64,
    /// How far a cutter travels round the outline and every hole (mm).
    pub cut_length: f64,
}

/// Two curves that run on from one another as one curve, when they are pieces of the same
/// line, of the same circle (the same way round) or both runs of points.
fn joined(a: &FlatCurve, b: &FlatCurve) -> Option<FlatCurve> {
    match (a, b) {
        (FlatCurve::Line { start, end: mid }, FlatCurve::Line { start: _, end }) => {
            let (d1, d2) = (*mid - *start, *end - *mid);
            (d1.cross(d2).abs() <= 1e-9 * d1.len() * d2.len() && d1.dot(d2) > 0.0).then_some(FlatCurve::Line { start: *start, end: *end })
        }
        (FlatCurve::Arc { center, radius, start, ccw, .. }, FlatCurve::Arc { center: c2, radius: r2, end, ccw: ccw2, .. }) => {
            if center.dist(*c2) >= JOIN || (radius - r2).abs() >= JOIN || ccw != ccw2 {
                return None;
            }
            let turned = a.sweep().abs() + b.sweep().abs();
            if turned > TAU + 1e-9 {
                return None;
            }
            // All the way round, it is the circle.
            Some(if (turned - TAU).abs() <= 1e-9 {
                FlatCurve::Circle { center: *center, radius: *radius }
            } else {
                FlatCurve::Arc { center: *center, radius: *radius, start: *start, end: *end, ccw: *ccw }
            })
        }
        (FlatCurve::Polyline { points: p }, FlatCurve::Polyline { points: q }) => {
            Some(FlatCurve::Polyline { points: p.iter().chain(q.iter().skip(1)).copied().collect() })
        }
        _ => None,
    }
}

/// A loop with the pieces of one line, one arc or one run of points made one curve each: the
/// kernel splits edges where faces meet, and a cutter has no use for the joints.
fn simplified(l: FlatLoop) -> FlatLoop {
    let mut out: Vec<FlatCurve> = Vec::with_capacity(l.curves.len());
    for c in l.curves {
        match out.last().and_then(|last| joined(last, &c)) {
            Some(one) => {
                out.pop();
                out.push(one);
            }
            None => out.push(c),
        }
    }
    // The loop may have started in the middle of a curve.
    while out.len() > 1 {
        let Some(one) = out.last().zip(out.first()).and_then(|(last, first)| joined(last, first)) else { break };
        out.pop();
        out[0] = one;
    }
    FlatLoop { curves: out }
}

/// A closed run of curves as loops that do not pass through one point twice: where an outline
/// pinches to a point (a relief that reaches the corner between two bends), it is the outline
/// and a hole that touches it, whichever way round the curves were followed.
fn unpinched(mut run: Vec<FlatCurve>) -> Vec<Vec<FlatCurve>> {
    let mut out = Vec::new();
    'again: loop {
        let ends: Vec<Vec2> = run.iter().map(|c| c.ends().map_or(Vec2::ZERO, |e| e.1)).collect();
        for i in 0..ends.len() {
            for j in i + 1..ends.len() {
                if ends[i].dist(ends[j]) < JOIN {
                    // The curves after `i` up to `j` leave that point and come back to it.
                    out.push(run.drain(i + 1..=j).collect());
                    continue 'again;
                }
            }
        }
        out.push(run);
        return out;
    }
}

/// Finds the loops a set of curves makes. Every end must meet exactly one other end; the result
/// has each loop's curves in order, running counter-clockwise for the largest (the outline) and
/// clockwise for the rest.
pub fn chain(curves: Vec<FlatCurve>) -> Result<(FlatLoop, Vec<FlatLoop>), String> {
    let all = curves.clone();
    // Every curve as short straight pieces, made when outlines are found to touch.
    let segments: std::cell::OnceCell<Vec<(Vec2, Vec2)>> = std::cell::OnceCell::new();
    let mut loops: Vec<FlatLoop> = Vec::new();
    let mut open: Vec<FlatCurve> = Vec::new();
    for c in curves {
        match c.ends() {
            None => loops.push(FlatLoop { curves: vec![c] }),
            // A curve that ends where it starts is a loop of its own.
            Some((a, b)) if a.dist(b) < JOIN && c.length() > JOIN => loops.push(FlatLoop { curves: vec![c] }),
            Some((a, b)) if a.dist(b) < JOIN => {}
            Some(_) => open.push(c),
        }
    }
    // Ends by grid cell, so that meeting ends are found without comparing every pair.
    let cell = |p: Vec2| ((p.x / (JOIN * 16.0)).floor() as i64, (p.y / (JOIN * 16.0)).floor() as i64);
    let mut at: BTreeMap<(i64, i64), Vec<(usize, bool)>> = BTreeMap::new();
    for (i, c) in open.iter().enumerate() {
        if let Some((a, b)) = c.ends() {
            at.entry(cell(a)).or_default().push((i, true));
            at.entry(cell(b)).or_default().push((i, false));
        }
    }
    let end_of = |i: usize, start: bool| -> Vec2 { open[i].ends().map_or(Vec2::ZERO, |(a, b)| if start { a } else { b }) };
    let meeting = |p: Vec2, skip: usize, used: &[bool]| -> Vec<(usize, bool)> {
        let (cx, cy) = cell(p);
        let mut found = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &(j, start) in at.get(&(cx + dx, cy + dy)).map_or(&[][..], Vec::as_slice) {
                    if j != skip && !used[j] && end_of(j, start).dist(p) < JOIN {
                        found.push((j, start));
                    }
                }
            }
        }
        found
    };
    let mut used = vec![false; open.len()];
    for first in 0..open.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut run = vec![open[first].clone()];
        let home = end_of(first, true);
        let mut here = end_of(first, false);
        let mut last = first;
        while here.dist(home) >= JOIN {
            let next = meeting(here, last, &used);
            let Some(&(j, start)) = next.first() else {
                return Err(format!(
                    "The flat pattern's outline does not close near ({:.3}, {:.3}): the part is not a sheet of one thickness there.",
                    here.x, here.y
                ));
            };
            // Where more than two ends meet, outlines touch at a point: a relief that reaches the
            // corner between two bends, say. The way on is the curve next to the one arrived by,
            // across a gap in the sheet: so an outline goes round a gap, never through the point
            // into another one, whichever way round it is followed.
            let (j, start) = if next.len() > 1 {
                let segments =
                    segments.get_or_init(|| all.iter().flat_map(|c| c.points(0.01).windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>()).collect());
                let sheet_at = |p: Vec2| -> bool {
                    // Crossings of a ray to the right of `p` with everything: odd inside the sheet.
                    segments.iter().filter(|(a, b)| (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x)).count() % 2
                        == 1
                };
                let back = run.last().map_or(Vec2::X, |c| c.reversed().heading());
                // Each way on, by how far round it is from the way back (counter-clockwise).
                let mut ways: Vec<(f64, (usize, bool))> = next
                    .iter()
                    .map(|&(j, start)| {
                        let h = if start { open[j].heading() } else { open[j].reversed().heading() };
                        (back.cross(h).atan2(back.dot(h)).rem_euclid(TAU), (j, start))
                    })
                    .collect();
                ways.sort_by(|a, b| a.0.total_cmp(&b.0));
                let gap = |from: f64, to: f64| !sheet_at(here + back.rotate((from + to) / 2.0) * PINCH_PROBE);
                match (ways.first(), ways.last()) {
                    (Some(&(angle, way)), _) if gap(0.0, angle) => way,
                    (_, Some(&(angle, way))) if gap(angle, TAU) => way,
                    _ => ways.first().map_or((j, start), |w| w.1),
                }
            } else {
                (j, start)
            };
            used[j] = true;
            run.push(if start { open[j].clone() } else { open[j].reversed() });
            here = end_of(j, !start);
            last = j;
        }
        loops.extend(unpinched(run).into_iter().map(|curves| simplified(FlatLoop { curves })));
    }
    if loops.is_empty() {
        return Err("The flat pattern has no outline.".into());
    }
    let largest = (0..loops.len()).max_by(|a, b| loops[*a].signed_area().abs().total_cmp(&loops[*b].signed_area().abs())).unwrap_or(0);
    let outer = loops.swap_remove(largest);
    let outer = if outer.signed_area() < 0.0 { outer.reversed() } else { outer };
    let mut inner: Vec<FlatLoop> = loops.into_iter().map(|l| if l.signed_area() > 0.0 { l.reversed() } else { l }).collect();
    // A steady order: by where each hole is.
    let key = |l: &FlatLoop| {
        let (lo, _) = bounds(std::slice::from_ref(l));
        (lo.x, lo.y)
    };
    inner.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
    });
    let rim = outer.polygon(0.05);
    for hole in &inner {
        // (A hole may touch the outline at a point: most of it is inside all the same.)
        let points = hole.polygon(0.2);
        if points.iter().filter(|p| within(&rim, **p)).count() * 2 <= points.len() {
            return Err("The flat pattern comes apart into separate pieces: the part is not one sheet.".into());
        }
    }
    Ok((outer, inner))
}

/// The corners of the upright box that holds the loops.
pub fn bounds(loops: &[FlatLoop]) -> (Vec2, Vec2) {
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in loops.iter().flat_map(|l| &l.curves) {
        let (a, b) = c.bounds();
        lo = lo.min(a);
        hi = hi.max(b);
    }
    (lo, hi)
}

/// Where two outlines cross, if they do: the sheet would be needed twice there.
pub fn crossing(loops: &[&FlatLoop]) -> Option<Vec2> {
    // Segments with the loop and place they came from, so neighbours are not compared.
    let mut segs: Vec<(Vec2, Vec2, usize, usize, usize)> = Vec::new();
    for (li, l) in loops.iter().enumerate() {
        let poly = l.polygon(0.1);
        let n = poly.len();
        for i in 0..n {
            segs.push((poly[i], poly[(i + 1) % n], li, i, n));
        }
    }
    // Sorted by their left end, a segment is compared only with those that start before it ends.
    segs.sort_by(|a, b| a.0.x.min(a.1.x).total_cmp(&b.0.x.min(b.1.x)));
    for i in 0..segs.len() {
        let (a, b, la, ia, na) = segs[i];
        let right = a.x.max(b.x);
        for &(c, d, lb, ib, _) in segs[i + 1..].iter().take_while(|s| s.0.x.min(s.1.x) <= right) {
            if la == lb && (ia == ib || (ia + 1) % na == ib || (ib + 1) % na == ia) {
                continue;
            }
            let (r, s) = (b - a, d - c);
            let denom = r.cross(s);
            if denom.abs() < 1e-12 {
                continue;
            }
            let t = (c - a).cross(s) / denom;
            let u = (c - a).cross(r) / denom;
            // A real crossing, clear of the ends: outlines that only touch do not cross.
            let eps = 1e-7;
            if t > eps && t < 1.0 - eps && u > eps && u < 1.0 - eps {
                return Some(a + r * t);
            }
        }
    }
    None
}

impl FlatPattern {
    /// Every loop, the outline first.
    pub fn loops(&self) -> impl Iterator<Item = &FlatLoop> {
        std::iter::once(&self.outer).chain(&self.inner)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::f64::consts::PI;

    fn line(a: (f64, f64), b: (f64, f64)) -> FlatCurve {
        FlatCurve::Line { start: Vec2::new(a.0, a.1), end: Vec2::new(b.0, b.1) }
    }

    #[test]
    fn curves_in_any_order_and_direction_are_chained_into_an_outline_and_its_holes() {
        // A 40 x 20 plate with a round hole and a square one, its sides given out of order and
        // two of them backwards.
        let curves = vec![
            line((40.0, 20.0), (0.0, 20.0)),
            line((0.0, 0.0), (40.0, 0.0)),
            FlatCurve::Circle { center: Vec2::new(10.0, 10.0), radius: 3.0 },
            line((40.0, 20.0), (40.0, 0.0)),
            line((0.0, 20.0), (0.0, 0.0)),
            line((30.0, 8.0), (34.0, 8.0)),
            line((34.0, 12.0), (34.0, 8.0)),
            line((34.0, 12.0), (30.0, 12.0)),
            line((30.0, 12.0), (30.0, 8.0)),
        ];
        let (outer, inner) = chain(curves).unwrap();
        assert!((outer.signed_area() - 800.0).abs() < 1e-9, "counter-clockwise: {}", outer.signed_area());
        assert_eq!(inner.len(), 2);
        assert!((inner[0].signed_area().abs() - 9.0 * PI).abs() < 1e-9, "the round hole: {}", inner[0].signed_area());
        assert!((inner[1].signed_area() + 16.0).abs() < 1e-9);
        assert!((outer.length() - 120.0).abs() < 1e-9 && (inner[0].length() - 6.0 * PI).abs() < 1e-9);
        // A side missing: it says where the outline stops.
        let open = vec![line((0.0, 0.0), (40.0, 0.0)), line((40.0, 0.0), (40.0, 20.0)), line((40.0, 20.0), (0.0, 20.0))];
        let e = chain(open).unwrap_err();
        assert!(e.contains("does not close near (0.000, 20.000)"), "{e}");
        // A hole outside the outline is a second piece of sheet.
        let apart = vec![
            line((0.0, 0.0), (10.0, 0.0)),
            line((10.0, 0.0), (10.0, 10.0)),
            line((10.0, 10.0), (0.0, 10.0)),
            line((0.0, 10.0), (0.0, 0.0)),
            FlatCurve::Circle { center: Vec2::new(30.0, 5.0), radius: 2.0 },
        ];
        assert!(chain(apart).unwrap_err().contains("separate pieces"));
    }

    #[test]
    fn arcs_measure_their_area_length_and_reach_exactly() {
        // A 20 x 10 slot: two half circles of radius 5 joined by two lines.
        let slot = vec![
            line((0.0, -5.0), (10.0, -5.0)),
            FlatCurve::Arc { center: Vec2::new(10.0, 0.0), radius: 5.0, start: Vec2::new(10.0, -5.0), end: Vec2::new(10.0, 5.0), ccw: true },
            line((10.0, 5.0), (0.0, 5.0)),
            FlatCurve::Arc { center: Vec2::new(0.0, 0.0), radius: 5.0, start: Vec2::new(0.0, 5.0), end: Vec2::new(0.0, -5.0), ccw: true },
        ];
        let (outer, inner) = chain(slot.clone()).unwrap();
        assert!(inner.is_empty());
        assert!((outer.signed_area() - (100.0 + 25.0 * PI)).abs() < 1e-9, "{}", outer.signed_area());
        assert!((outer.length() - (20.0 + 10.0 * PI)).abs() < 1e-9);
        let (lo, hi) = bounds(std::slice::from_ref(&outer));
        assert!(lo.near(Vec2::new(-5.0, -5.0), 1e-12) && hi.near(Vec2::new(15.0, 5.0), 1e-12), "{lo:?} {hi:?}");
        // Given clockwise it is turned round, arcs and all.
        let back: Vec<FlatCurve> = slot.iter().rev().map(FlatCurve::reversed).collect();
        let (again, _) = chain(back).unwrap();
        assert!((again.signed_area() - outer.signed_area()).abs() < 1e-9);
        // A quarter arc run clockwise sweeps a negative quarter turn.
        let q = FlatCurve::Arc { center: Vec2::ZERO, radius: 2.0, start: Vec2::new(0.0, 2.0), end: Vec2::new(2.0, 0.0), ccw: false };
        assert!((q.sweep() + PI / 2.0).abs() < 1e-12 && (q.length() - PI).abs() < 1e-12);
        assert!(outer.contains(Vec2::new(14.0, 0.0)) && !outer.contains(Vec2::new(14.0, 4.5)));
    }

    #[test]
    fn outlines_that_cross_are_found_and_ones_that_touch_are_not() {
        let square = |x: f64, y: f64, s: f64| {
            chain(vec![line((x, y), (x + s, y)), line((x + s, y), (x + s, y + s)), line((x + s, y + s), (x, y + s)), line((x, y + s), (x, y))])
                .unwrap()
                .0
        };
        let (a, b, c) = (square(0.0, 0.0, 10.0), square(5.0, 5.0, 10.0), square(10.0, 0.0, 10.0));
        let at = crossing(&[&a, &b]).unwrap();
        assert!(at.near(Vec2::new(10.0, 5.0), 1e-9) || at.near(Vec2::new(5.0, 10.0), 1e-9), "{at:?}");
        assert_eq!(crossing(&[&a, &c]), None, "sharing a side is not crossing");
        assert_eq!(crossing(&[&a]), None);
    }

    #[test]
    fn pieces_of_one_line_or_circle_are_one_curve_and_touching_outlines_are_told_apart() {
        // A square whose every side comes in two pieces, with a hole given as two half circles.
        let half = |a: f64, b: f64| FlatCurve::Arc {
            center: Vec2::new(5.0, 5.0),
            radius: 2.0,
            start: Vec2::polar(Vec2::new(5.0, 5.0), 2.0, a),
            end: Vec2::polar(Vec2::new(5.0, 5.0), 2.0, b),
            ccw: true,
        };
        let curves = vec![
            line((0.0, 0.0), (4.0, 0.0)),
            line((4.0, 0.0), (10.0, 0.0)),
            line((10.0, 0.0), (10.0, 3.0)),
            line((10.0, 10.0), (10.0, 3.0)),
            line((10.0, 10.0), (2.0, 10.0)),
            line((2.0, 10.0), (0.0, 10.0)),
            line((0.0, 10.0), (0.0, 6.0)),
            line((0.0, 6.0), (0.0, 0.0)),
            half(0.0, PI),
            half(PI, 0.0),
        ];
        let (outer, inner) = chain(curves).unwrap();
        assert_eq!(outer.curves.len(), 4, "{outer:?}");
        assert_eq!(inner.len(), 1);
        assert!(matches!(inner[0].curves.as_slice(), [FlatCurve::Circle { radius, .. }] if *radius == 2.0), "{inner:?}");

        // A cross whose arms meet only at the corners of a square notch in the middle piece:
        // the plate of a tray with a square relief at one corner, in small. At the point where
        // the notch touches the outline four curves meet; the notch comes out as a hole and the
        // rest as the outline, whichever way and in whatever order the curves are given.
        let cross = vec![
            // The outline: an L of two arms round the corner at (0, 0), which is where the
            // notch touches it.
            line((0.0, 0.0), (0.0, -5.0)),
            line((0.0, -5.0), (20.0, -5.0)),
            line((20.0, -5.0), (20.0, 20.0)),
            line((20.0, 20.0), (-5.0, 20.0)),
            line((-5.0, 20.0), (-5.0, 0.0)),
            line((-5.0, 0.0), (0.0, 0.0)),
            // The notch: 4 square, one corner at (0, 0), inside the sheet.
            line((0.0, 0.0), (4.0, 0.0)),
            line((4.0, 0.0), (4.0, 4.0)),
            line((4.0, 4.0), (0.0, 4.0)),
            line((0.0, 4.0), (0.0, 0.0)),
        ];
        let sheet = 25.0 * 25.0 - 25.0;
        let mut orders: Vec<Vec<FlatCurve>> =
            vec![cross.clone(), cross.iter().rev().cloned().collect(), cross.iter().map(FlatCurve::reversed).collect()];
        let mut turned = cross.clone();
        turned.rotate_left(7);
        orders.push(turned);
        for curves in orders {
            let (outer, inner) = chain(curves).unwrap();
            assert!((outer.signed_area() - sheet).abs() < 1e-9 && outer.curves.len() == 6, "{outer:?}");
            assert!(inner.len() == 1 && (inner[0].signed_area() + 16.0).abs() < 1e-9 && inner[0].curves.len() == 4, "{inner:?}");
        }
    }
}
