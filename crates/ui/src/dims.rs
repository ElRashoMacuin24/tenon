//! Sketch dimensions as they are drawn: extension lines out from the geometry, a dimension line
//! with arrowheads between them and the value on it; a leader for a radius or diameter; an arc
//! for an angle. Where the value sits is chosen by the user (a click when the dimension is
//! placed, a drag later) and the lines follow it.
//!
//! Positions are worked out in the sketch (so a dimension line stays parallel to what it
//! measures in any view) and sizes on screen (arrowheads and gaps do not grow with the zoom).

use egui::{Pos2, Stroke, vec2};
use tenon_geom::Vec2;
use tenon_sketch::{Constraint, EntityId, Geometry, Sketch};

/// Length and half-width of an arrowhead, in pixels.
const ARROW: f32 = 11.0;
const ARROW_HALF: f32 = 3.2;
/// An extension line starts this far from the geometry and runs this far past the dimension
/// line.
const EXT_GAP: f32 = 3.0;
const EXT_OVER: f32 = 6.0;
/// Clear space each side of the value on its line.
const TEXT_PAD: f32 = 4.0;
/// How far from its geometry a dimension sits until it is placed by hand, in pixels.
const DEFAULT_OFFSET: f64 = 30.0;

/// One dimension on screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DimShape {
    /// Straight strokes: extension lines, dimension lines and leaders.
    pub lines: Vec<[Pos2; 2]>,
    /// Curved strokes (an angle's arc), as polylines.
    pub curves: Vec<Vec<Pos2>>,
    /// Arrowheads: the tip and the unit direction it points in.
    pub arrows: Vec<(Pos2, egui::Vec2)>,
    /// The middle of the value's text.
    pub text: Pos2,
}

impl DimShape {
    /// Draws the lines and arrowheads (the value's text is the caller's).
    pub(crate) fn paint(&self, p: &egui::Painter, color: egui::Color32) {
        let stroke = Stroke::new(1.0, color);
        for l in &self.lines {
            p.line_segment(*l, stroke);
        }
        for c in &self.curves {
            p.add(egui::Shape::line(c.clone(), stroke));
        }
        for (tip, dir) in &self.arrows {
            let (back, side) = (*tip - *dir * ARROW, vec2(-dir.y, dir.x) * ARROW_HALF);
            p.add(egui::Shape::convex_polygon(vec![*tip, back + side, back - side], color, Stroke::NONE));
        }
    }
}

/// The two points a straight dimension measures between and the direction it measures along.
fn measured(s: &Sketch, c: &Constraint) -> Option<(Vec2, Vec2, Vec2)> {
    // A point's place, or (for a line given where a point is expected) its middle.
    let at = |id: EntityId| s.point(id).or_else(|| s.line(id).map(|(p, q)| p.mid(q)));
    match c {
        Constraint::Length { line, .. } => {
            let (a, b) = s.line(*line)?;
            Some((a, b, (b - a).normalized()))
        }
        Constraint::HorizontalDistance { a, b, .. } => Some((at(*a)?, at(*b)?, Vec2::new(1.0, 0.0))),
        Constraint::VerticalDistance { a, b, .. } => Some((at(*a)?, at(*b)?, Vec2::new(0.0, 1.0))),
        Constraint::Distance { a, b, .. } => {
            let pa = at(*a)?;
            match s.line(*b) {
                // From a point square on to a line.
                Some((p, q)) if s.point(*a).is_some() => {
                    let along = (q - p).normalized();
                    let foot = p + along * (pa - p).dot(along);
                    let d = if foot.dist(pa) > 1e-9 { (foot - pa).normalized() } else { along.perp() };
                    Some((pa, foot, d))
                }
                _ => {
                    let pb = at(*b)?;
                    Some((pa, pb, (pb - pa).normalized()))
                }
            }
        }
        _ => None,
    }
}

/// Where the two lines of an angle dimension cross, and the unit directions of the two rays the
/// angle is between, on the side of the crossing nearer `toward`.
fn angle_rays(s: &Sketch, a: EntityId, b: EntityId, toward: Option<Vec2>) -> Option<(Vec2, Vec2, Vec2)> {
    let ((a1, a2), (b1, b2)) = (s.line(a)?, s.line(b)?);
    let (da, db) = ((a2 - a1).normalized(), (b2 - b1).normalized());
    let cross = da.cross(db);
    if cross.abs() < 1e-9 {
        return None;
    }
    let vertex = a1 + da * ((b1 - a1).cross(db) / cross);
    // The angle is between the lines' own directions, or between both turned round: whichever
    // pair of rays faces `toward` (else the pair the lines themselves lie along).
    let bisector = da + db;
    let aim = toward.map(|t| t - vertex).filter(|v| v.len() > 1e-9).unwrap_or_else(|| a1.mid(a2).mid(b1.mid(b2)) - vertex);
    let side = if aim.dot(bisector) >= 0.0 { 1.0 } else { -1.0 };
    Some((vertex, da * side, db * side))
}

/// Where a dimension's value sits until it is placed by hand: beside its geometry, away from the
/// middle of the sketch. `mm_per_px` is the size of a pixel in the sketch.
pub(crate) fn default_place(s: &Sketch, c: &Constraint, mm_per_px: f64) -> Option<Vec2> {
    let off = DEFAULT_OFFSET * mm_per_px;
    let points: Vec<Vec2> = s.entities().filter_map(|(id, _)| s.point(id)).collect();
    let middle = if points.is_empty() { Vec2::ZERO } else { points.iter().fold(Vec2::ZERO, |a, b| a + *b) * (1.0 / points.len() as f64) };
    match c {
        Constraint::Radius { curve, .. } | Constraint::Diameter { curve, .. } => {
            let (centre, r) = s.circle(*curve).or_else(|| s.arc(*curve).map(|a| (a.center, a.radius)))?;
            // Up and to the right; for an arc, through its middle.
            let dir = match s.arc(*curve) {
                Some(a) => Vec2::polar(Vec2::ZERO, 1.0, a.start + a.sweep() / 2.0),
                None => Vec2::new(1.0, 1.0).normalized(),
            };
            Some(centre + dir * (r + off))
        }
        Constraint::Angle { a, b, .. } => {
            let (vertex, ra, rb) = angle_rays(s, *a, *b, None)?;
            let reach = s.line(*a).map_or(off, |(p, q)| p.dist(q)).min(s.line(*b).map_or(off, |(p, q)| p.dist(q)));
            Some(vertex + (ra + rb).normalized() * (0.6 * reach).max(off))
        }
        _ => {
            let (p1, p2, d) = measured(s, c)?;
            let mid = p1.mid(p2);
            let n = d.perp();
            let away = if (mid - middle).dot(n) >= 0.0 { n } else { -n };
            Some(mid + away * off)
        }
    }
}

/// The dimension `c` of sketch `s` with its value at `place`, on screen. `text` is the size of
/// the value's text in pixels: the dimension line leaves room for it.
pub(crate) fn shape(s: &Sketch, c: &Constraint, place: Vec2, text: egui::Vec2, project: &dyn Fn(Vec2) -> Option<Pos2>) -> Option<DimShape> {
    match c {
        Constraint::Radius { curve, .. } | Constraint::Diameter { curve, .. } => {
            let (centre, r) = s.circle(*curve).or_else(|| s.arc(*curve).map(|a| (a.center, a.radius)))?;
            let out = if place.dist(centre) > 1e-9 { (place - centre).normalized() } else { Vec2::new(1.0, 0.0) };
            round(centre, r, out, place, matches!(c, Constraint::Diameter { .. }), text, project)
        }
        Constraint::Angle { a, b, .. } => angle(s, *a, *b, place, text, project),
        _ => {
            let (p1, p2, d) = measured(s, c)?;
            straight(p1, p2, d, place, text, project)
        }
    }
}

/// Half the room the value's text takes along a line in direction `u` (the text is level).
fn text_half(text: egui::Vec2, u: egui::Vec2) -> f32 {
    (u.x.abs() * text.x + u.y.abs() * text.y) / 2.0 + TEXT_PAD
}

/// The line from `a` to `b` with a gap of `half` each side of `at` (a parameter along it, in
/// pixels from `a`).
fn gapped(a: Pos2, b: Pos2, at: f32, half: f32, out: &mut Vec<[Pos2; 2]>) {
    let len = a.distance(b);
    if len < 0.5 {
        return;
    }
    let u = (b - a) / len;
    let (lo, hi) = (at - half, at + half);
    if lo > 0.0 {
        out.push([a, a + u * lo.min(len)]);
    }
    if hi < len {
        out.push([a + u * hi.max(0.0), b]);
    }
}

fn straight(p1: Vec2, p2: Vec2, d: Vec2, place: Vec2, text: egui::Vec2, project: &dyn Fn(Vec2) -> Option<Pos2>) -> Option<DimShape> {
    // The dimension line runs through the value's place, along what is measured.
    let n = d.perp();
    let (q1, q2) = (p1 + n * (place - p1).dot(n), p2 + n * (place - p2).dot(n));
    let (s1, s2, e1, e2, t) = (project(p1)?, project(p2)?, project(q1)?, project(q2)?, project(place)?);
    let mut out = DimShape { text: t, ..Default::default() };
    for (from, to) in [(s1, e1), (s2, e2)] {
        let len = from.distance(to);
        if len > EXT_GAP + 1.0 {
            let u = (to - from) / len;
            out.lines.push([from + u * EXT_GAP, to + u * EXT_OVER]);
        }
    }
    let span = e1.distance(e2);
    if span < 0.5 {
        // Nothing between the extension lines to draw on: the value alone.
        return Some(out);
    }
    let u = (e2 - e1) / span;
    let half = text_half(text, u);
    let at = (t - e1).dot(u);
    // Arrowheads go between the extension lines when they and the value fit there, pointing
    // outwards; else outside, pointing in.
    let inside_text = at - half > 0.0 && at + half < span;
    let fits = span >= 2.0 * ARROW + if inside_text { 2.0 * half } else { 4.0 };
    let tail = ARROW + 8.0;
    let (mut lo, mut hi) = (0.0f32, span);
    if fits {
        out.arrows.push((e1, -u));
        out.arrows.push((e2, u));
    } else {
        out.arrows.push((e1, u));
        out.arrows.push((e2, -u));
        (lo, hi) = (-tail, span + tail);
    }
    // The line reaches the value when that is outside the extension lines.
    lo = lo.min(at - half);
    hi = hi.max(at + half);
    let (a, b) = (e1 + u * lo, e1 + u * hi);
    if fits || !inside_text {
        gapped(a, b, at - lo, half, &mut out.lines);
    } else {
        // Arrows outside and the value between them: only the tails are drawn.
        out.lines.push([a, e1]);
        out.lines.push([e2, b]);
    }
    Some(out)
}

fn round(
    centre: Vec2,
    r: f64,
    out_dir: Vec2,
    place: Vec2,
    diameter: bool,
    text: egui::Vec2,
    project: &dyn Fn(Vec2) -> Option<Pos2>,
) -> Option<DimShape> {
    // A leader along the line from the centre through the value's place.
    let (near, far) = (centre + out_dir * r, centre - out_dir * r);
    let (c, n, f, t) = (project(centre)?, project(near)?, project(far)?, project(place)?);
    let mut out = DimShape { text: t, ..Default::default() };
    let outside = place.dist(centre) > r;
    let start = if diameter { f } else { c };
    // From the far side of a circle (or the centre, for a radius) to the value or the near
    // side, whichever is further.
    let end = if outside { t } else { n };
    let len = start.distance(end);
    if len < 0.5 {
        return Some(out);
    }
    let u = (end - start) / len;
    let half = text_half(text, u);
    gapped(start, end, (t - start).dot(u), half, &mut out.lines);
    // Arrowheads touch the curve from inside it, pointing outwards.
    let n_len = n.distance(c);
    if n_len > 0.5 {
        out.arrows.push((n, (n - c) / n_len));
        if diameter {
            out.arrows.push((f, (f - c) / n_len.max(f.distance(c)).max(0.5)));
        }
    }
    Some(out)
}

fn angle(s: &Sketch, a: EntityId, b: EntityId, place: Vec2, text: egui::Vec2, project: &dyn Fn(Vec2) -> Option<Pos2>) -> Option<DimShape> {
    let (vertex, ra, rb) = angle_rays(s, a, b, Some(place))?;
    let r = place.dist(vertex);
    if r < 1e-9 {
        return None;
    }
    let t = project(place)?;
    let mut out = DimShape { text: t, ..Default::default() };
    // The arc from one ray to the other the short way, and on to the value if that is beyond
    // either ray.
    let a0 = ra.y.atan2(ra.x);
    let sweep = ra.cross(rb).atan2(ra.dot(rb));
    let to = (place - vertex).normalized();
    let at = ra.cross(to).atan2(ra.dot(to)) * sweep.signum();
    let (from, until) = (at.min(0.0), at.max(sweep.abs()));
    let steps = (((until - from) / 0.05).ceil() as usize).clamp(8, 256);
    let arc: Vec<Pos2> = (0..=steps)
        .filter_map(|i| project(Vec2::polar(vertex, r, a0 + sweep.signum() * (from + (until - from) * i as f64 / steps as f64))))
        .collect();
    // A gap for the value.
    let clear = text.x.max(text.y) / 2.0 + TEXT_PAD;
    let mut run: Vec<Pos2> = Vec::new();
    for p in arc {
        if p.distance(t) < clear {
            if run.len() > 1 {
                out.curves.push(std::mem::take(&mut run));
            }
            run.clear();
        } else {
            run.push(p);
        }
    }
    if run.len() > 1 {
        out.curves.push(run);
    }
    // Arrowheads where the arc meets each ray, pointing at the ray; and a line out to the arc
    // where a line stops short of it (or back to it, where the line starts beyond).
    for (ray, line, turn) in [(ra, a, -1.0), (rb, b, 1.0)] {
        let tip = vertex + ray * r;
        let tangent = ray.perp() * (turn * sweep.signum());
        if let (Some(p), Some(q)) = (project(tip), project(tip + tangent * r.max(1e-6) * 0.01)) {
            let len = p.distance(q);
            if len > 1e-4 {
                out.arrows.push((p, (q - p) / len));
            }
        }
        if let Some((p1, p2)) = s.line(line) {
            let (t1, t2) = ((p1 - vertex).dot(ray), (p2 - vertex).dot(ray));
            let (near, far) = (t1.min(t2), t1.max(t2));
            let from = if r > far {
                Some(far)
            } else if r < near {
                Some(near)
            } else {
                None
            };
            if let Some(from) = from
                && let (Some(p), Some(q)) = (project(vertex + ray * from), project(tip))
            {
                let len = p.distance(q);
                if len > EXT_GAP + 1.0 {
                    let u = (q - p) / len;
                    out.lines.push([p + u * EXT_GAP, q + u * EXT_OVER]);
                }
            }
        }
    }
    Some(out)
}

/// What the Dimension tool would make of the geometry picked so far with the pointer at
/// `cursor`, which decides between a level, an upright and an aligned dimension: over or under
/// what is measured gives its width, beside it its height, anywhere else the straight distance.
pub(crate) fn pending(s: &Sketch, picks: &[EntityId], cursor: Vec2) -> Option<Constraint> {
    let is_line = |id: EntityId| s.is_line(id);
    let is_point = |id: EntityId| s.is_point(id);
    let ends = |id: EntityId| match s.geometry(id) {
        Some(Geometry::Line { start, end }) => Some((*start, *end)),
        _ => None,
    };
    // Between two points: level, upright or aligned, by where the pointer is.
    let between = |a: EntityId, b: EntityId, aligned: Constraint| -> Option<Constraint> {
        let (pa, pb) = (s.point(a)?, s.point(b)?);
        let (lo, hi) = (Vec2::new(pa.x.min(pb.x), pa.y.min(pb.y)), Vec2::new(pa.x.max(pb.x), pa.y.max(pb.y)));
        // (A level or upright pair has only the one distance.)
        if (hi.x - lo.x) < 1e-9 || (hi.y - lo.y) < 1e-9 {
            return Some(aligned);
        }
        let (in_x, in_y) = ((lo.x..=hi.x).contains(&cursor.x), (lo.y..=hi.y).contains(&cursor.y));
        Some(match (in_x, in_y) {
            (true, false) => Constraint::HorizontalDistance { a, b, value: 0.0 },
            (false, true) => Constraint::VerticalDistance { a, b, value: 0.0 },
            _ => aligned,
        })
    };
    match picks {
        [one] if is_line(*one) => {
            let (a, b) = ends(*one)?;
            between(a, b, Constraint::Length { line: *one, value: 0.0 })
        }
        [one] => match s.geometry(*one) {
            Some(Geometry::Circle { .. }) => Some(Constraint::Diameter { curve: *one, value: 0.0 }),
            Some(Geometry::Arc { .. }) => Some(Constraint::Radius { curve: *one, value: 0.0 }),
            _ => None,
        },
        [a, b] if is_point(*a) && is_point(*b) => between(*a, *b, Constraint::Distance { a: *a, b: *b, value: 0.0 }),
        [a, b] if is_point(*a) && is_line(*b) => Some(Constraint::Distance { a: *a, b: *b, value: 0.0 }),
        [a, b] if is_line(*a) && is_point(*b) => Some(Constraint::Distance { a: *b, b: *a, value: 0.0 }),
        [a, b] if is_line(*a) && is_line(*b) => {
            let ((p, q), (r, t)) = (s.line(*a)?, s.line(*b)?);
            if (q - p).normalized().cross((t - r).normalized()).abs() < 1e-6 {
                // Parallel: how far apart.
                let (start, _) = ends(*b)?;
                Some(Constraint::Distance { a: start, b: *a, value: 0.0 })
            } else {
                Some(Constraint::Angle { a: *a, b: *b, value: 0.0 })
            }
        }
        _ => None,
    }
}

/// The size of a pixel in the sketch, near `at`.
pub(crate) fn mm_per_px(at: Vec2, project: &dyn Fn(Vec2) -> Option<Pos2>) -> f64 {
    match (project(at), project(at + Vec2::new(1.0, 0.0)), project(at + Vec2::new(0.0, 1.0))) {
        (Some(o), Some(x), Some(y)) => {
            let px = f64::from(o.distance(x).max(o.distance(y)));
            if px > 1e-6 { 1.0 / px } else { 1.0 }
        }
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use egui::pos2;

    use super::*;

    /// One pixel per millimetre, y up in the sketch and down on screen.
    fn flat(p: Vec2) -> Option<Pos2> {
        Some(pos2(p.x as f32, -p.y as f32))
    }

    fn rectangle() -> (Sketch, [EntityId; 4]) {
        let mut s = Sketch::new();
        let lines = s.add_rectangle(Vec2::new(0.0, 0.0), Vec2::new(100.0, 50.0)).unwrap();
        (s, lines)
    }

    #[test]
    fn a_length_has_extension_lines_a_dimension_line_and_two_arrowheads() {
        let (s, lines) = rectangle();
        // The bottom side, its value placed 20 below the middle.
        let c = Constraint::Length { line: lines[0], value: 100.0 };
        let d = shape(&s, &c, Vec2::new(50.0, -20.0), vec2(30.0, 12.0), &flat).unwrap();
        assert_eq!(d.text, pos2(50.0, 20.0));
        // Two extension lines, from just off the corners down past the dimension line.
        let ext: Vec<_> = d.lines.iter().filter(|l| (l[0].x - l[1].x).abs() < 1e-3).collect();
        assert_eq!(ext.len(), 2);
        for (l, x) in ext.iter().zip([0.0, 100.0]) {
            assert!((l[0].x - x).abs() < 1e-3 && (l[0].y - EXT_GAP).abs() < 1e-3 && (l[1].y - (20.0 + EXT_OVER)).abs() < 1e-3, "{l:?}");
        }
        // The dimension line in two parts, level, with room for the value between them.
        let dim: Vec<_> = d.lines.iter().filter(|l| (l[0].y - 20.0).abs() < 1e-3 && (l[1].y - 20.0).abs() < 1e-3).collect();
        assert_eq!(dim.len(), 2);
        assert!((dim[0][0].x - 0.0).abs() < 1e-3 && (dim[0][1].x - (50.0 - 15.0 - TEXT_PAD)).abs() < 1e-3, "{:?}", dim[0]);
        assert!((dim[1][0].x - (50.0 + 15.0 + TEXT_PAD)).abs() < 1e-3 && (dim[1][1].x - 100.0).abs() < 1e-3, "{:?}", dim[1]);
        // Arrowheads at both ends, pointing out at the extension lines.
        assert_eq!(d.arrows, vec![(pos2(0.0, 20.0), vec2(-1.0, 0.0)), (pos2(100.0, 20.0), vec2(1.0, 0.0))]);
    }

    #[test]
    fn a_short_dimension_puts_its_arrowheads_outside_and_reaches_a_value_placed_beyond() {
        let mut s = Sketch::new();
        let l = s.add_line(Vec2::new(0.0, 0.0), Vec2::new(12.0, 0.0)).unwrap();
        let c = Constraint::Length { line: l, value: 12.0 };
        // The value 40 to the right of the line's end, 20 above it.
        let d = shape(&s, &c, Vec2::new(52.0, 20.0), vec2(30.0, 12.0), &flat).unwrap();
        assert_eq!(d.arrows, vec![(pos2(0.0, -20.0), vec2(1.0, 0.0)), (pos2(12.0, -20.0), vec2(-1.0, 0.0))], "pointing in");
        let reach = d.lines.iter().flat_map(|l| [l[0].x, l[1].x]).fold(f32::NEG_INFINITY, f32::max);
        assert!((reach - (52.0 - 15.0 - TEXT_PAD)).abs() < 1e-3, "the line runs out to the value: {reach}");
    }

    #[test]
    fn the_pointer_chooses_between_level_upright_and_aligned() {
        let mut s = Sketch::new();
        let l = s.add_line(Vec2::new(0.0, 0.0), Vec2::new(40.0, 30.0)).unwrap();
        let Some(Geometry::Line { start, end }) = s.geometry(l).cloned() else { panic!() };
        // Under the line: how wide. Beside it: how high. Off its corner: its length.
        assert_eq!(pending(&s, &[l], Vec2::new(20.0, -10.0)), Some(Constraint::HorizontalDistance { a: start, b: end, value: 0.0 }));
        assert_eq!(pending(&s, &[l], Vec2::new(55.0, 15.0)), Some(Constraint::VerticalDistance { a: start, b: end, value: 0.0 }));
        assert_eq!(pending(&s, &[l], Vec2::new(-10.0, 40.0)), Some(Constraint::Length { line: l, value: 0.0 }));
        // A level line has one length whichever side the pointer is.
        let flat_line = s.add_line(Vec2::new(0.0, 50.0), Vec2::new(40.0, 50.0)).unwrap();
        assert_eq!(pending(&s, &[flat_line], Vec2::new(20.0, 70.0)), Some(Constraint::Length { line: flat_line, value: 0.0 }));
        // A circle is its diameter; two lines that cross, their angle; two that do not, how far apart.
        let ring = s.add_circle(Vec2::new(100.0, 0.0), 10.0).unwrap();
        assert_eq!(pending(&s, &[ring], Vec2::ZERO), Some(Constraint::Diameter { curve: ring, value: 0.0 }));
        assert_eq!(pending(&s, &[l, flat_line], Vec2::ZERO), Some(Constraint::Angle { a: l, b: flat_line, value: 0.0 }));
        let other = s.add_line(Vec2::new(0.0, 80.0), Vec2::new(40.0, 80.0)).unwrap();
        assert!(matches!(pending(&s, &[flat_line, other], Vec2::ZERO), Some(Constraint::Distance { b, .. }) if b == flat_line));
        assert_eq!(pending(&s, &[start], Vec2::ZERO), None, "a point alone measures nothing yet");
    }

    #[test]
    fn a_diameter_runs_through_the_centre_and_a_radius_from_it() {
        let mut s = Sketch::new();
        let ring = s.add_circle(Vec2::new(0.0, 0.0), 20.0).unwrap();
        // Placed outside, to the right: the line crosses the circle and runs on to the value.
        let d = shape(&s, &Constraint::Diameter { curve: ring, value: 40.0 }, Vec2::new(60.0, 0.0), vec2(30.0, 12.0), &flat).unwrap();
        assert_eq!(d.arrows, vec![(pos2(20.0, 0.0), vec2(1.0, 0.0)), (pos2(-20.0, 0.0), vec2(-1.0, 0.0))]);
        let (lo, hi) = d.lines.iter().flat_map(|l| [l[0].x, l[1].x]).fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)));
        assert!((lo + 20.0).abs() < 1e-3 && (hi - (60.0 - 15.0 - TEXT_PAD)).abs() < 1e-3, "{lo} to {hi}");
        // A radius starts at the centre and has one arrowhead.
        let r = shape(&s, &Constraint::Radius { curve: ring, value: 20.0 }, Vec2::new(0.0, 60.0), vec2(30.0, 12.0), &flat).unwrap();
        assert_eq!(r.arrows, vec![(pos2(0.0, -20.0), vec2(0.0, -1.0))]);
        assert!(r.lines.iter().any(|l| l[0] == pos2(0.0, 0.0)), "{:?}", r.lines);
    }

    #[test]
    fn an_angle_is_an_arc_between_its_lines_with_an_arrowhead_on_each() {
        let mut s = Sketch::new();
        let a = s.add_line(Vec2::new(0.0, 0.0), Vec2::new(50.0, 0.0)).unwrap();
        let b = s.add_line(Vec2::new(0.0, 0.0), Vec2::new(0.0, 50.0)).unwrap();
        let c = Constraint::Angle { a, b, value: std::f64::consts::FRAC_PI_2 };
        // The value on the bisector, 30 from the corner.
        let at = Vec2::polar(Vec2::ZERO, 30.0, std::f64::consts::FRAC_PI_4);
        let d = shape(&s, &c, at, vec2(20.0, 12.0), &flat).unwrap();
        // The arc is 30 from the corner everywhere, in two runs either side of the value.
        assert_eq!(d.curves.len(), 2);
        assert!(d.curves.iter().flatten().all(|p| (p.distance(pos2(0.0, 0.0)) - 30.0).abs() < 1e-2));
        // One arrowhead on each line, pointing at it along the arc.
        assert_eq!(d.arrows.len(), 2);
        assert!(d.arrows[0].0.distance(pos2(30.0, 0.0)) < 1e-3 && d.arrows[0].1.y > 0.99, "{:?}", d.arrows[0]);
        assert!(d.arrows[1].0.distance(pos2(0.0, -30.0)) < 1e-3 && d.arrows[1].1.x < -0.99, "{:?}", d.arrows[1]);
        assert!(d.lines.is_empty(), "the lines reach the arc themselves");
        // Further out than the lines reach, a line runs on from each to the arc.
        let far = shape(&s, &c, Vec2::polar(Vec2::ZERO, 80.0, std::f64::consts::FRAC_PI_4), vec2(20.0, 12.0), &flat).unwrap();
        assert_eq!(far.lines.len(), 2);
        // Parallel lines have no angle to show.
        let level = s.add_line(Vec2::new(0.0, 10.0), Vec2::new(50.0, 10.0)).unwrap();
        assert!(shape(&s, &Constraint::Angle { a, b: level, value: 0.0 }, at, vec2(20.0, 12.0), &flat).is_none());
    }

    #[test]
    fn an_unplaced_dimension_sits_beside_its_geometry_away_from_the_sketch() {
        let (s, lines) = rectangle();
        // 30 px at 0.5 mm per px: 15 mm below the bottom side, above the top one.
        let bottom = default_place(&s, &Constraint::Length { line: lines[0], value: 100.0 }, 0.5).unwrap();
        assert!((bottom.x - 50.0).abs() < 1e-9 && (bottom.y + 15.0).abs() < 1e-9, "{bottom:?}");
        let top = default_place(&s, &Constraint::Length { line: lines[2], value: 100.0 }, 0.5).unwrap();
        assert!((top.y - 65.0).abs() < 1e-9, "{top:?}");
        assert!((mm_per_px(Vec2::ZERO, &flat) - 1.0).abs() < 1e-9);
    }
}
