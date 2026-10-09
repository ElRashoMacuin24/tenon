//! Strokes as they are written to files: duplicate strokes dropped, points on straight runs
//! merged, and runs of points on a circle written as true arcs and circles. Exported drawings are
//! then small, and edit well in other CAD programs (a hole is a CIRCLE, not 72 lines).
//!
//! All tolerances are on paper (sheet millimetres): what is written differs from what is drawn on
//! screen by less than a printer can show.

use std::collections::HashSet;
use std::f64::consts::{PI, TAU};

use tenon_geom::Vec2;

use crate::graphics::{Graphics, Pen};

/// How far a point may be from a circle and still be on it (mm). View curves put their points on
/// the true curve, so this only absorbs rounding.
const ON_CIRCLE: f64 = 0.002;
/// How far a chord may sag from the circle (mm): the views' chord tolerance on paper, with room.
const CHORD_SAG: f64 = 0.03;
/// Points closer than this are one point (mm).
const SAME_POINT: f64 = 1e-6;

/// A stroke as written to a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Polyline(Vec<Vec2>),
    /// Counter-clockwise from `a0` to `a1` (radians, `a0 < a1 < a0 + 2π`).
    Arc {
        c: Vec2,
        r: f64,
        a0: f64,
        a1: f64,
    },
    Circle {
        c: Vec2,
        r: f64,
    },
}

impl Item {
    /// Points along it, `step` radians apart at most for arcs (for checks and rasters).
    pub fn points(&self, step: f64) -> Vec<Vec2> {
        let around = |c: Vec2, r: f64, a0: f64, a1: f64| {
            let n = (((a1 - a0) / step.max(1e-3)).ceil() as usize).clamp(2, 10_000);
            (0..=n).map(|i| a0 + (a1 - a0) * i as f64 / n as f64).map(|a| Vec2::new(c.x + r * a.cos(), c.y + r * a.sin())).collect()
        };
        match self {
            Item::Polyline(p) => p.clone(),
            Item::Arc { c, r, a0, a1 } => around(*c, *r, *a0, *a1),
            Item::Circle { c, r } => around(*c, *r, 0.0, TAU),
        }
    }
}

/// A sheet's strokes ready to write, with their pens, in the order drawn.
pub fn items(g: &Graphics) -> Vec<(Pen, Item)> {
    let mut seen: HashSet<(&'static str, Vec<i64>)> = HashSet::new();
    let mut out = Vec::new();
    for (_, pen, pts) in &g.strokes {
        let Some(item) = simplify(pts) else { continue };
        if seen.insert((pen.name(), key(&item))) {
            out.push((*pen, item));
        }
    }
    out
}

/// What makes two items the same stroke (to the micrometre); a polyline drawn backwards too.
fn key(item: &Item) -> Vec<i64> {
    let q = |v: f64| (v * 1000.0).round() as i64;
    match item {
        Item::Polyline(p) => {
            let fwd: Vec<i64> = p.iter().flat_map(|v| [q(v.x), q(v.y)]).collect();
            let back: Vec<i64> = p.iter().rev().flat_map(|v| [q(v.x), q(v.y)]).collect();
            let mut k = vec![0];
            k.extend(if back < fwd { back } else { fwd });
            k
        }
        Item::Arc { c, r, a0, a1 } => vec![1, q(c.x), q(c.y), q(*r), q(*a0), q(*a1)],
        Item::Circle { c, r } => vec![2, q(c.x), q(c.y), q(*r)],
    }
}

/// One stroke written as simply as it can be: a circle, an arc, or a polyline without points
/// that add nothing.
pub fn simplify(pts: &[Vec2]) -> Option<Item> {
    let mut p: Vec<Vec2> = Vec::with_capacity(pts.len());
    for q in pts {
        if !(q.x.is_finite() && q.y.is_finite()) {
            return None;
        }
        if p.last().is_none_or(|l| l.dist(*q) > SAME_POINT) {
            p.push(*q);
        }
    }
    if p.len() < 2 {
        return None;
    }
    if let Some(c) = as_circle(&p) {
        return Some(c);
    }
    Some(Item::Polyline(straighten(&p)))
}

/// Drops points that lie on the straight line between their neighbours.
fn straighten(p: &[Vec2]) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(p.len());
    for (i, q) in p.iter().enumerate() {
        if let (Some(a), Some(b)) = (out.last(), p.get(i + 1)) {
            let ab = *b - *a;
            let len = ab.len();
            let off = if len > 0.0 { ((*q - *a).x * ab.y - (*q - *a).y * ab.x).abs() / len } else { 0.0 };
            // On the line and between the ends: adds nothing.
            if off < SAME_POINT * 10.0 && (*q - *a).dot(ab) > 0.0 && (*b - *q).dot(ab) > 0.0 {
                continue;
            }
        }
        out.push(*q);
    }
    out
}

/// The circle through three points.
fn circumcenter(a: Vec2, b: Vec2, c: Vec2) -> Option<Vec2> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a.dot(a), b.dot(b), c.dot(c));
    Some(Vec2::new((a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d, (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d))
}

/// The circle or arc a run of points lies on, if it does: every point on one circle, every chord
/// sagging less than [`CHORD_SAG`], turning one way.
fn as_circle(p: &[Vec2]) -> Option<Item> {
    let n = p.len();
    if n < 5 {
        return None;
    }
    let closed = p[0].dist(p[n - 1]) < SAME_POINT * 10.0;
    let (a, b, c) = if closed { (p[0], p[n / 3], p[2 * n / 3]) } else { (p[0], p[n / 2], p[n - 1]) };
    let center = circumcenter(a, b, c)?;
    let r = center.dist(a);
    if !(r > 0.05 && r < 10_000.0) || p.iter().any(|q| (q.dist(center) - r).abs() > ON_CIRCLE) {
        return None;
    }
    let mut sweep = 0.0;
    for w in p.windows(2) {
        let (u, v) = (w[0] - center, w[1] - center);
        let step = (u.x * v.y - u.y * v.x).atan2(u.dot(v));
        // A chord's sag from the circle: r (1 - cos(step / 2)).
        if r * (1.0 - (step / 2.0).cos()) > CHORD_SAG || step.abs() > PI / 2.0 {
            return None;
        }
        if sweep != 0.0 && step != 0.0 && step.signum() != f64::signum(sweep) {
            return None;
        }
        sweep += step;
    }
    if closed {
        return ((sweep.abs() - TAU).abs() < 1e-6).then_some(Item::Circle { c: center, r });
    }
    if sweep.abs() >= TAU - 1e-9 || sweep.abs() < 1e-9 {
        return None;
    }
    let start = (a - center).y.atan2((a - center).x);
    let (a0, a1) = if sweep > 0.0 { (start, start + sweep) } else { (start + sweep, start) };
    Some(Item::Arc { c: center, r, a0, a1 })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::graphics::Owner;

    #[test]
    fn circles_arcs_lines_and_duplicates() {
        let mut g = Graphics { width: 200.0, height: 100.0, ..Graphics::default() };
        g.circle(Owner::Frame, Pen::Visible, Vec2::new(50.0, 50.0), 4.0);
        // A quarter arc, clockwise, from (80, 50) to (70, 40) about (70, 50).
        let arc: Vec<Vec2> =
            (0..=20).map(|i| -(f64::from(i) / 20.0) * PI / 2.0).map(|a| Vec2::new(70.0 + 10.0 * a.cos(), 50.0 + 10.0 * a.sin())).collect();
        g.polyline(Owner::Frame, Pen::Hidden, arc.clone());
        // A straight line through many points, then the same line backwards.
        let line: Vec<Vec2> = (0..=10).map(|i| Vec2::new(10.0 + f64::from(i), 10.0)).collect();
        g.polyline(Owner::Frame, Pen::Visible, line.clone());
        g.polyline(Owner::Frame, Pen::Visible, line.iter().rev().copied().collect());
        // A hexagon is not a circle; an ellipse is not either.
        let hex: Vec<Vec2> = (0..=6).map(|i| f64::from(i) * TAU / 6.0).map(|a| Vec2::new(150.0 + 10.0 * a.cos(), 50.0 + 10.0 * a.sin())).collect();
        g.polyline(Owner::Frame, Pen::Visible, hex);
        let ellipse: Vec<Vec2> =
            (0..=90).map(|i| f64::from(i) * TAU / 90.0).map(|a| Vec2::new(150.0 + 20.0 * a.cos(), 20.0 + 8.0 * a.sin())).collect();
        g.polyline(Owner::Frame, Pen::Visible, ellipse);
        let items = items(&g);
        assert_eq!(items.len(), 5, "{items:?}");
        assert!(matches!(items[0], (Pen::Visible, Item::Circle { c, r }) if c.dist(Vec2::new(50.0, 50.0)) < 1e-9 && (r - 4.0).abs() < 1e-9));
        match &items[1] {
            (Pen::Hidden, Item::Arc { c, r, a0, a1 }) => {
                assert!(c.dist(Vec2::new(70.0, 50.0)) < 1e-9 && (r - 10.0).abs() < 1e-9);
                assert!((a0 + PI / 2.0).abs() < 1e-9 && a1.abs() < 1e-9, "counter-clockwise from -90 to 0 degrees: {a0} {a1}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(items[2].1, Item::Polyline(vec![Vec2::new(10.0, 10.0), Vec2::new(20.0, 10.0)]), "one segment, drawn once");
        assert!(matches!(&items[3].1, Item::Polyline(p) if p.len() == 7), "the hexagon keeps its corners");
        assert!(matches!(&items[4].1, Item::Polyline(p) if p.len() > 80), "the ellipse keeps its points");
    }
}
