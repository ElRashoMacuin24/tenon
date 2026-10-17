//! Text in a sketch (DEC-102): the outlines of its letters, from the font Tenon carries, as
//! closed contours of lines and Bézier curves in the sketch's coordinates. A text is one entity,
//! placed by its anchor point and edited as a whole; its letters, with their holes (the inside
//! of an O), are profiles for features.

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};
use tenon_geom::{Vec2, point_in_polygon, shoelace};
use tenon_kernel::{Curve2, Loop, Region, TaggedCurve2};

/// DejaVu Sans (Bitstream Vera Fonts licence; see ATTRIBUTION.md and assets/fonts).
static FONT: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans.ttf");

/// The font every sketch text is written in.
pub const FONT_NAME: &str = "DejaVu Sans";

/// Most characters in one text (hostile-input cap).
pub const MAX_CHARS: usize = 2000;

/// How tall capital letters are when nothing says (mm): the usual height of lettering on
/// drawings.
pub const DEFAULT_HEIGHT: f64 = 3.5;

/// Line spacing, in capital-letter heights.
const LINE_SPACING: f64 = 1.6;

/// A piece of a letter's outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Line(Vec2, Vec2),
    /// A quadratic Bézier: from, control, to.
    Quad(Vec2, Vec2, Vec2),
    /// A cubic Bézier: from, two controls, to.
    Cubic(Vec2, Vec2, Vec2, Vec2),
}

impl Seg {
    pub fn start(&self) -> Vec2 {
        match *self {
            Seg::Line(a, _) | Seg::Quad(a, _, _) | Seg::Cubic(a, _, _, _) => a,
        }
    }

    pub fn end(&self) -> Vec2 {
        match *self {
            Seg::Line(_, b) | Seg::Quad(_, _, b) | Seg::Cubic(_, _, _, b) => b,
        }
    }

    fn reversed(&self) -> Seg {
        match *self {
            Seg::Line(a, b) => Seg::Line(b, a),
            Seg::Quad(a, c, b) => Seg::Quad(b, c, a),
            Seg::Cubic(a, c, d, b) => Seg::Cubic(b, d, c, a),
        }
    }

    fn map(&self, f: impl Fn(Vec2) -> Vec2) -> Seg {
        match *self {
            Seg::Line(a, b) => Seg::Line(f(a), f(b)),
            Seg::Quad(a, c, b) => Seg::Quad(f(a), f(c), f(b)),
            Seg::Cubic(a, c, d, b) => Seg::Cubic(f(a), f(c), f(d), f(b)),
        }
    }

    /// Points along it, the start left out, `n` of them for a curve.
    fn sample(&self, n: usize, out: &mut Vec<Vec2>) {
        match *self {
            Seg::Line(_, b) => out.push(b),
            Seg::Quad(a, c, b) => {
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    let u = 1.0 - t;
                    out.push(a * (u * u) + c * (2.0 * u * t) + b * (t * t));
                }
            }
            Seg::Cubic(a, c, d, b) => {
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    let u = 1.0 - t;
                    out.push(a * (u * u * u) + c * (3.0 * u * u * t) + d * (3.0 * u * t * t) + b * (t * t * t));
                }
            }
        }
    }

    /// The piece as a kernel curve: a Bézier is a clamped uniform B-spline of one span.
    fn curve2(&self) -> Curve2 {
        match *self {
            Seg::Line(a, b) => Curve2::Line { start: a, end: b },
            Seg::Quad(a, c, b) => Curve2::BSpline { poles: vec![a, c, b], degree: 2 },
            Seg::Cubic(a, c, d, b) => Curve2::BSpline { poles: vec![a, c, d, b], degree: 3 },
        }
    }
}

/// A closed outline of a letter, or of a hole in one.
pub type Contour = Vec<Seg>;

/// Collects a glyph's outline in font units, moved along by `dx, dy`.
struct Pen {
    dx: f64,
    dy: f64,
    contours: Vec<Contour>,
    current: Contour,
    at: Vec2,
    start: Vec2,
}

impl Pen {
    fn p(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(f64::from(x) + self.dx, f64::from(y) + self.dy)
    }

    fn push(&mut self, s: Seg) {
        // (A piece of no length is no piece.)
        if (s.end() - s.start()).len() > 1e-9 {
            self.current.push(s);
        }
        self.at = s.end();
    }

    fn finish(&mut self) {
        if (self.at - self.start).len() > 1e-9 {
            let s = Seg::Line(self.at, self.start);
            self.current.push(s);
        }
        let done = std::mem::take(&mut self.current);
        if done.len() >= 2 || done.iter().any(|s| !matches!(s, Seg::Line(..))) {
            self.contours.push(done);
        }
        self.at = self.start;
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        if !self.current.is_empty() {
            self.finish();
        }
        self.start = self.p(x, y);
        self.at = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let s = Seg::Line(self.at, self.p(x, y));
        self.push(s);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let s = Seg::Quad(self.at, self.p(cx0, cy0), self.p(x, y));
        self.push(s);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let s = Seg::Cubic(self.at, self.p(cx0, cy0), self.p(cx1, cy1), self.p(x, y));
        self.push(s);
    }

    fn close(&mut self) {
        self.finish();
    }
}

/// The contours of `text` (one line or several), the left end of its first line's baseline at
/// `at`, its capital letters `height` tall, turned `angle` radians about `at`. Characters the
/// font does not have are drawn as its missing-letter box.
pub fn contours(text: &str, at: Vec2, height: f64, angle: f64) -> Vec<Contour> {
    let Ok(font) = FontRef::new(FONT) else { return Vec::new() };
    let location = LocationRef::default();
    let advances = font.glyph_metrics(Size::unscaled(), location);
    let charmap = font.charmap();
    let glyphs = font.outline_glyphs();
    let mut pen = Pen { dx: 0.0, dy: 0.0, contours: Vec::new(), current: Vec::new(), at: Vec2::ZERO, start: Vec2::ZERO };
    // How tall a capital is: the font's own H, measured (what the font says is a little off).
    if let Some(h) = charmap.map('H').and_then(|g| glyphs.get(g)) {
        let _ = h.draw(DrawSettings::unhinted(Size::unscaled(), location), &mut pen);
        pen.finish();
    }
    let measured = pen.contours.iter().flatten().map(|seg| seg.end().y).fold(0.0, f64::max);
    pen.contours.clear();
    let metrics = font.metrics(Size::unscaled(), location);
    let cap = if measured > 0.0 { measured } else { metrics.cap_height.map_or(0.72 * f64::from(metrics.units_per_em), f64::from) };
    if !(cap > 0.0 && height.is_finite() && height > 0.0) {
        return Vec::new();
    }
    let scale = height / cap;
    for (row, line) in text.split('\n').enumerate() {
        pen.dx = 0.0;
        pen.dy = -(row as f64) * LINE_SPACING * cap;
        for ch in line.chars().filter(|c| *c != '\r') {
            let gid = charmap.map(ch).unwrap_or_default();
            if let Some(glyph) = glyphs.get(gid) {
                let _ = glyph.draw(DrawSettings::unhinted(Size::unscaled(), location), &mut pen);
                if !pen.current.is_empty() {
                    pen.finish();
                }
            }
            pen.dx += advances.advance_width(gid).map_or(0.0, f64::from);
        }
    }
    let (s, c) = angle.sin_cos();
    let place = |p: Vec2| {
        let q = p * scale;
        at + Vec2::new(q.x * c - q.y * s, q.x * s + q.y * c)
    };
    pen.contours.into_iter().map(|k| k.iter().map(|seg| seg.map(place)).collect()).collect()
}

/// The contours as closed polylines, for drawing and picking.
pub fn polylines(contours: &[Contour]) -> Vec<Vec<Vec2>> {
    contours
        .iter()
        .map(|k| {
            let mut out = Vec::with_capacity(k.len() * 4 + 1);
            if let Some(first) = k.first() {
                out.push(first.start());
            }
            for s in k {
                s.sample(6, &mut out);
            }
            out
        })
        .collect()
}

/// The box the text takes, as its four corners (in order round it), for picking.
pub fn corners(text: &str, at: Vec2, height: f64, angle: f64) -> Option<[Vec2; 4]> {
    // (Measured unturned, then turned: the box turns with the text.)
    let flat = polylines(&contours(text, Vec2::ZERO, height, 0.0));
    let mut it = flat.iter().flatten();
    let first = *it.next()?;
    let (lo, hi) = it.fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let (s, c) = angle.sin_cos();
    let turn = |p: Vec2| at + Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c);
    Some([turn(lo), turn(Vec2::new(hi.x, lo.y)), turn(hi), turn(Vec2::new(lo.x, hi.y))])
}

/// The letters as regions for a feature: each outline that is not inside another an outer
/// boundary, each one inside it a hole, as deep as they go; all counter-clockwise, as circles
/// are (the kernel turns holes round). The
/// kernel tags every curve with `tag`, the text's entity id.
pub fn regions(contours: &[Contour], tag: u64) -> Vec<Region> {
    let outlines = polylines(contours);
    let n = contours.len();
    // Which contours hold each one: its first point inside them.
    let holders: Vec<Vec<usize>> =
        (0..n).map(|i| (0..n).filter(|j| *j != i && outlines[i].first().is_some_and(|p| point_in_polygon(&outlines[*j], *p))).collect()).collect();
    let area = |i: usize| shoelace(&outlines[i]).abs();
    let as_loop = |i: usize, ccw: bool| {
        let turning = shoelace(&outlines[i]) > 0.0;
        let segs: Vec<Seg> = if turning == ccw { contours[i].clone() } else { contours[i].iter().rev().map(Seg::reversed).collect() };
        Loop { curves: segs.iter().map(|s| TaggedCurve2 { tag, curve: s.curve2() }).collect() }
    };
    let mut out = Vec::new();
    for i in (0..n).filter(|i| holders[*i].len().is_multiple_of(2)) {
        let depth = holders[i].len();
        // Its holes: one level deeper, and held by it nearest (by it, and by nothing smaller).
        let holes: Vec<Loop> = (0..n)
            .filter(|h| holders[*h].len() == depth + 1 && holders[*h].contains(&i))
            .filter(|h| holders[*h].iter().filter(|o| **o != i && holders[**o].len() == depth).all(|o| area(*o) >= area(i)))
            // (Holes go to the kernel the way circles do, counter-clockwise: it turns them round.)
            .map(|h| as_loop(h, true))
            .collect();
        out.push(Region { outer: as_loop(i, true), holes });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_are_outlined_at_their_height_and_place() {
        // A capital H, 10 tall, at (5, 7): one outline, from the baseline up 10.
        let h = contours("H", Vec2::new(5.0, 7.0), 10.0, 0.0);
        assert_eq!(h.len(), 1, "an H is one outline");
        let pts: Vec<Vec2> = polylines(&h).concat();
        let (lo, hi) = pts.iter().fold((pts[0], pts[0]), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        assert!((lo.y - 7.0).abs() < 1e-6 && (hi.y - 17.0).abs() < 1e-6, "{lo:?} {hi:?}");
        assert!(lo.x >= 5.0 && hi.x < 5.0 + 10.0, "{lo:?} {hi:?}");
        // An O is an outline with a hole: one region with one hole.
        let o = regions(&contours("O", Vec2::ZERO, 10.0, 0.0), 3);
        assert_eq!((o.len(), o[0].holes.len()), (1, 1));
        // "AB": the A has one hole, the B two.
        let ab = regions(&contours("AB", Vec2::ZERO, 10.0, 0.0), 3);
        let mut holes: Vec<usize> = ab.iter().map(|r| r.holes.len()).collect();
        holes.sort_unstable();
        assert_eq!(holes, [1, 2]);
        // Spaces and a second line: the second line is lower.
        let two = polylines(&contours("I\nI", Vec2::ZERO, 10.0, 0.0));
        assert_eq!(two.len(), 2);
        let low = |k: &Vec<Vec2>| k.iter().map(|p| p.y).fold(f64::MAX, f64::min);
        assert!(low(&two[1]) < low(&two[0]) - 10.0, "{} {}", low(&two[0]), low(&two[1]));
        // Turned a quarter: the H now stands to the left of its place.
        let turned = polylines(&contours("H", Vec2::ZERO, 10.0, std::f64::consts::FRAC_PI_2)).concat();
        assert!(turned.iter().all(|p| p.x <= 1e-6) && turned.iter().any(|p| p.x < -9.0), "{turned:?}");
        // Nothing to draw, nothing drawn.
        assert!(contours("", Vec2::ZERO, 10.0, 0.0).is_empty());
        assert!(contours("H", Vec2::ZERO, 0.0, 0.0).is_empty());
    }

    #[test]
    fn a_text_box_turns_with_its_text() {
        let [a, b, c, d] = corners("TEXT", Vec2::new(1.0, 2.0), 5.0, 0.0).unwrap();
        assert!((a.y - 2.0).abs() < 1e-6 && (c.y - 7.0).abs() < 1e-6 && b.x > a.x + 10.0 && (d.x - a.x).abs() < 1e-9, "{a:?} {b:?} {c:?} {d:?}");
    }
}
