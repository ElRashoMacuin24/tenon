//! A sheet as lines, fills and text in sheet millimetres (y up): what the screen draws and the
//! exports write. Each primitive says what it belongs to, for picking.

use tenon_geom::Vec2;

use crate::model::{AnnotId, SheetId, ViewId};

/// How a line is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pen {
    /// Visible edges.
    Visible,
    /// Hidden edges (dashed).
    Hidden,
    /// Dimension, extension and leader lines, tables.
    Thin,
    /// Centre marks and centrelines (long and short dashes).
    Center,
    /// Section lines on the parent view (chain, thick).
    Cutting,
    /// The border and the title block's frame.
    Border,
    /// Section hatching.
    Hatch,
}

impl Pen {
    /// Line width on paper (mm).
    pub fn width(self) -> f64 {
        match self {
            Pen::Visible => 0.5,
            Pen::Hidden => 0.35,
            Pen::Thin | Pen::Center => 0.25,
            Pen::Cutting => 0.6,
            Pen::Border => 0.7,
            Pen::Hatch => 0.18,
        }
    }
    /// Dash pattern on paper (mm): dash, gap, dash, gap...; empty for continuous.
    pub fn dashes(self) -> &'static [f64] {
        match self {
            Pen::Hidden => &[3.0, 1.5],
            Pen::Center | Pen::Cutting => &[12.0, 1.5, 2.0, 1.5],
            _ => &[],
        }
    }
    /// A name for layers and styles.
    pub fn name(self) -> &'static str {
        match self {
            Pen::Visible => "VISIBLE",
            Pen::Hidden => "HIDDEN",
            Pen::Thin => "THIN",
            Pen::Center => "CENTER",
            Pen::Cutting => "CUTTING",
            Pen::Border => "BORDER",
            Pen::Hatch => "HATCH",
        }
    }
    pub const ALL: [Pen; 7] = [Pen::Visible, Pen::Hidden, Pen::Thin, Pen::Center, Pen::Cutting, Pen::Border, Pen::Hatch];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Text: a single line, horizontal, its baseline at `at` (left end, middle or right end).
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub at: Vec2,
    pub height: f64,
    pub text: String,
    pub align: Align,
}

impl Text {
    /// The text's left end of the baseline.
    pub fn left(&self) -> Vec2 {
        let w = crate::stroke::text_width(&self.text, self.height);
        match self.align {
            Align::Left => self.at,
            Align::Center => Vec2::new(self.at.x - w / 2.0, self.at.y),
            Align::Right => Vec2::new(self.at.x - w, self.at.y),
        }
    }
    /// Its bounds on the sheet.
    pub fn bounds(&self) -> (Vec2, Vec2) {
        let l = self.left();
        (Vec2::new(l.x, l.y - self.height * 0.35), Vec2::new(l.x + crate::stroke::text_width(&self.text, self.height), l.y + self.height))
    }
}

/// What a primitive belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    Frame,
    View(ViewId),
    Annotation(AnnotId),
}

/// A sheet as primitives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graphics {
    pub sheet: Option<SheetId>,
    pub width: f64,
    pub height: f64,
    pub strokes: Vec<(Owner, Pen, Vec<Vec2>)>,
    /// Filled polygons (arrowheads, dots).
    pub fills: Vec<(Owner, Vec<Vec2>)>,
    pub texts: Vec<(Owner, Text)>,
}

impl Graphics {
    pub fn line(&mut self, owner: Owner, pen: Pen, a: Vec2, b: Vec2) {
        self.strokes.push((owner, pen, vec![a, b]));
    }
    pub fn polyline(&mut self, owner: Owner, pen: Pen, pts: Vec<Vec2>) {
        if pts.len() >= 2 {
            self.strokes.push((owner, pen, pts));
        }
    }
    pub fn circle(&mut self, owner: Owner, pen: Pen, c: Vec2, r: f64) {
        // Chords within 0.01 mm of the circle on paper (so outputs can write it as a circle).
        let step = if r > 0.01 { 2.0 * (1.0 - 0.01 / r).acos() } else { 1.0 };
        let n = ((std::f64::consts::TAU / step).ceil() as u32).clamp(24, 2000);
        let pts = (0..=n).map(|i| {
            let a = f64::from(i) / f64::from(n) * std::f64::consts::TAU;
            Vec2::new(c.x + r * a.cos(), c.y + r * a.sin())
        });
        self.polyline(owner, pen, pts.collect());
    }
    pub fn text(&mut self, owner: Owner, at: Vec2, height: f64, text: impl Into<String>, align: Align) {
        let text = text.into();
        if !text.is_empty() {
            self.texts.push((owner, Text { at, height, text, align }));
        }
    }
    /// A filled arrowhead with its tip at `tip`, pointing along `dir`.
    pub fn arrow(&mut self, owner: Owner, tip: Vec2, dir: Vec2) {
        let d = dir.normalized();
        if !d.is_finite() || d.len() < 0.5 {
            return;
        }
        let n = Vec2::new(-d.y, d.x);
        let base = tip - d * ARROW_LENGTH;
        self.fills.push((owner, vec![tip, base + n * (ARROW_WIDTH / 2.0), base - n * (ARROW_WIDTH / 2.0)]));
    }
    pub fn dot(&mut self, owner: Owner, c: Vec2, r: f64) {
        let pts = (0..12).map(|i| {
            let a = f64::from(i) / 12.0 * std::f64::consts::TAU;
            Vec2::new(c.x + r * a.cos(), c.y + r * a.sin())
        });
        self.fills.push((owner, pts.collect()));
    }

    /// Bounds of what belongs to `owner`.
    pub fn bounds_of(&self, owner: Owner) -> Option<(Vec2, Vec2)> {
        let pts = self
            .strokes
            .iter()
            .filter(|s| s.0 == owner)
            .flat_map(|s| s.2.iter().copied())
            .chain(self.fills.iter().filter(|f| f.0 == owner).flat_map(|f| f.1.iter().copied()))
            .chain(self.texts.iter().filter(|t| t.0 == owner).flat_map(|t| {
                let (a, b) = t.1.bounds();
                [a, b]
            }));
        let mut it = pts;
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// What is under `p` (within `tol` mm), the topmost: annotations, then views.
    pub fn pick(&self, p: Vec2, tol: f64) -> Option<Owner> {
        let seg = |a: Vec2, b: Vec2| {
            let ab = b - a;
            let l2 = ab.dot(ab);
            let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
            p.dist(a + ab * t)
        };
        let mut best: Option<(f64, Owner)> = None;
        let mut consider = |d: f64, o: Owner| {
            let rank = if matches!(o, Owner::Annotation(_)) { d } else { d + tol * 0.5 };
            if d <= tol && best.is_none_or(|b| rank < b.0) {
                best = Some((rank, o));
            }
        };
        for (o, _, pts) in &self.strokes {
            if *o == Owner::Frame {
                continue;
            }
            for w in pts.windows(2) {
                consider(seg(w[0], w[1]), *o);
            }
        }
        for (o, t) in &self.texts {
            if *o == Owner::Frame {
                continue;
            }
            let (a, b) = t.bounds();
            if p.x >= a.x - tol && p.x <= b.x + tol && p.y >= a.y - tol && p.y <= b.y + tol {
                consider(0.0, *o);
            }
        }
        best.map(|b| b.1)
    }
}

/// Arrowhead size on paper (mm).
pub const ARROW_LENGTH: f64 = 3.0;
pub const ARROW_WIDTH: f64 = 1.0;
/// Annotation text height on paper (mm).
pub const TEXT: f64 = 3.5;
/// Gap between an extension line and the object, and its overshoot past the dimension line.
pub const EXT_GAP: f64 = 1.5;
pub const EXT_OVER: f64 = 2.0;

/// Splits a polyline into dashes of `pattern` (on paper mm): the runs to draw.
pub fn dashed(pts: &[Vec2], pattern: &[f64]) -> Vec<Vec<Vec2>> {
    let total: f64 = pattern.iter().sum();
    if pts.len() < 2 || pattern.is_empty() || total <= 0.0 {
        return vec![pts.to_vec()];
    }
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    if len / total > 20_000.0 {
        return vec![pts.to_vec()];
    }
    let mut out: Vec<Vec<Vec2>> = Vec::new();
    let mut cur: Vec<Vec2> = Vec::new();
    // Start half way into the first dash, so lines start and end with a dash.
    let (mut idx, mut left, mut on) = (0usize, pattern[0] / 2.0, true);
    if on {
        cur.push(pts[0]);
    }
    for w in pts.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut seg = a.dist(b);
        while seg > 1e-12 {
            if left >= seg {
                left -= seg;
                if on {
                    cur.push(b);
                }
                break;
            }
            let p = a + (b - a) * (left / seg);
            if on {
                cur.push(p);
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
            } else {
                cur = vec![p];
            }
            seg -= left;
            a = p;
            on = !on;
            idx = (idx + 1) % pattern.len();
            left = pattern[idx];
        }
    }
    if on && cur.len() >= 2 {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashes_cover_the_line_with_gaps() {
        let line = [Vec2::new(0.0, 0.0), Vec2::new(30.0, 0.0)];
        let d = dashed(&line, &[3.0, 1.5]);
        let drawn: f64 = d.iter().map(|r| r.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>()).sum();
        // Two thirds of the length is dash.
        assert!((drawn - 20.0).abs() < 2.0, "{drawn}");
        assert!(d.len() >= 6);
        assert_eq!(dashed(&line, &[]).len(), 1);
    }
}
