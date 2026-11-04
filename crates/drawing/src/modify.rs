//! What a view's modifiers do to what is drawn (DEC-131): which parts of its lines stay inside a
//! crop and outside the bands of its breaks, and where a point goes on the sheet when those bands
//! are closed up.
//!
//! A view's curves stay in true view coordinates: measuring, picking and the views made from it
//! go on as they were. Closing a band is a map from true coordinates to drawn ones, continuous and
//! rising, so it can be undone: the band shrinks to its gap and what is beyond it moves up.

use tenon_geom::Vec2;

use crate::model::{Boundary, BreakLines, ModKind, View};

/// A band taken out along one axis (true coordinates), and what is left of it (model mm).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Band {
    from: f64,
    to: f64,
    gap: f64,
}

/// The bands of a view along one axis, in order. A gap wider than its band, as a scale changed after
/// it was made can leave it, is as wide as the band: a break never spreads a view apart.
fn bands(v: &View, lines: BreakLines) -> Vec<Band> {
    let mut out: Vec<Band> = v
        .active_mods()
        .filter_map(|m| match &m.kind {
            ModKind::Break { lines: l, from, to, gap } if *l == lines => Some(Band { from: *from, to: *to, gap: (*gap / v.scale).min(*to - *from) }),
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| a.from.total_cmp(&b.from));
    out
}

fn close(bands: &[Band], t: f64) -> f64 {
    let mut shift = 0.0;
    for b in bands {
        if t >= b.to {
            shift += b.to - b.from - b.gap;
        } else if t > b.from {
            return b.from - shift + (t - b.from) * b.gap / (b.to - b.from);
        } else {
            break;
        }
    }
    t - shift
}

fn open(bands: &[Band], q: f64) -> f64 {
    let mut shift = 0.0;
    for b in bands {
        let at = b.from - shift;
        if q >= at + b.gap {
            shift += b.to - b.from - b.gap;
        } else if q > at {
            return b.from + (q - at) * (b.to - b.from) / b.gap;
        } else {
            break;
        }
    }
    q + shift
}

/// How a view's breaks move its points.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Folds {
    x: Vec<Band>,
    y: Vec<Band>,
}

impl Folds {
    pub(crate) fn of(v: &View) -> Folds {
        Folds { x: bands(v, BreakLines::Vertical), y: bands(v, BreakLines::Horizontal) }
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.x.is_empty() && self.y.is_empty()
    }
    /// A true point where it is drawn (still view coordinates, unscaled).
    pub(crate) fn close(&self, p: Vec2) -> Vec2 {
        Vec2::new(close(&self.x, p.x), close(&self.y, p.y))
    }
    /// Where a drawn point truly is.
    pub(crate) fn open(&self, q: Vec2) -> Vec2 {
        Vec2::new(open(&self.x, q.x), open(&self.y, q.y))
    }
    /// The true places of the bands' edges: across the sheet, and up it.
    pub(crate) fn edges(&self) -> (Vec<f64>, Vec<f64>) {
        let of = |bands: &[Band]| bands.iter().flat_map(|b| [b.from, b.to]).collect();
        (of(&self.x), of(&self.y))
    }
    /// The point drawn in the middle of a view whose true middle is `center`, whose true bounds
    /// are `lo`-`hi`: along an axis with a band the view stays centred on what is left of it.
    pub(crate) fn anchor(&self, center: Vec2, lo: Vec2, hi: Vec2) -> Vec2 {
        let mid = |bands: &[Band], lo: f64, hi: f64, c: f64| if bands.is_empty() { c } else { (close(bands, lo) + close(bands, hi)) * 0.5 };
        Vec2::new(mid(&self.x, lo.x, hi.x, center.x), mid(&self.y, lo.y, hi.y, center.y))
    }
}

/// The parameters along `a`-`b` where it crosses a boundary.
fn crossings(boundary: &Boundary, a: Vec2, b: Vec2, ts: &mut Vec<f64>) {
    let d = b - a;
    match boundary {
        Boundary::Rect { lo, hi } => {
            for x in [lo.x, hi.x] {
                if d.x != 0.0 {
                    ts.push((x - a.x) / d.x);
                }
            }
            for y in [lo.y, hi.y] {
                if d.y != 0.0 {
                    ts.push((y - a.y) / d.y);
                }
            }
        }
        Boundary::Circle { center, radius } => {
            let f = a - *center;
            let (qa, qb, qc) = (d.dot(d), 2.0 * f.dot(d), f.dot(f) - radius * radius);
            let disc = qb * qb - 4.0 * qa * qc;
            if qa > 0.0 && disc > 0.0 {
                ts.push((-qb - disc.sqrt()) / (2.0 * qa));
                ts.push((-qb + disc.sqrt()) / (2.0 * qa));
            }
        }
    }
}

/// Which parts of a view's lines are drawn: inside its crop, outside the bands of its breaks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clip {
    crop: Option<Boundary>,
    /// Bands of `x`, and of `y`.
    x: Vec<(f64, f64)>,
    y: Vec<(f64, f64)>,
}

impl Clip {
    pub fn of(v: &View) -> Clip {
        let mut c = Clip::default();
        for m in v.active_mods() {
            match &m.kind {
                ModKind::Crop { boundary, .. } => c.crop = Some(*boundary),
                ModKind::Break { lines: BreakLines::Vertical, from, to, .. } => c.x.push((*from, *to)),
                ModKind::Break { lines: BreakLines::Horizontal, from, to, .. } => c.y.push((*from, *to)),
                ModKind::BreakOut { .. } => {}
            }
        }
        c
    }

    /// What is inside a boundary.
    pub(crate) fn inside(boundary: Boundary) -> Clip {
        Clip { crop: Some(boundary), ..Clip::default() }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.crop.is_none() && self.x.is_empty() && self.y.is_empty()
    }

    /// Whether a true point of the view is drawn.
    pub fn keeps(&self, p: Vec2) -> bool {
        let outside = |bands: &[(f64, f64)], t: f64| !bands.iter().any(|(a, b)| t > *a && t < *b);
        self.crop.is_none_or(|c| c.contains(p)) && outside(&self.x, p.x) && outside(&self.y, p.y)
    }

    /// The parts of a polyline that are drawn.
    pub fn pieces(&self, pts: &[Vec2]) -> Vec<Vec<Vec2>> {
        if self.is_empty() {
            return vec![pts.to_vec()];
        }
        let mut out = Vec::new();
        let mut run: Vec<Vec2> = Vec::new();
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let d = b - a;
            let mut ts = vec![0.0, 1.0];
            if let Some(c) = &self.crop {
                crossings(c, a, b, &mut ts);
            }
            if d.x != 0.0 {
                ts.extend(self.x.iter().flat_map(|(p, q)| [(p - a.x) / d.x, (q - a.x) / d.x]));
            }
            if d.y != 0.0 {
                ts.extend(self.y.iter().flat_map(|(p, q)| [(p - a.y) / d.y, (q - a.y) / d.y]));
            }
            ts.retain(|t| (0.0..=1.0).contains(t));
            ts.sort_by(f64::total_cmp);
            for s in ts.windows(2) {
                if s[1] - s[0] <= 1e-12 {
                    continue;
                }
                let (p, q) = (a + d * s[0], a + d * s[1]);
                if self.keeps((p + q) * 0.5) {
                    if run.last().is_none_or(|l| l.dist(p) > 1e-9) {
                        if run.len() >= 2 {
                            out.push(std::mem::take(&mut run));
                        }
                        run.clear();
                        run.push(p);
                    }
                    run.push(q);
                } else {
                    if run.len() >= 2 {
                        out.push(std::mem::take(&mut run));
                    }
                    run.clear();
                }
            }
        }
        if run.len() >= 2 {
            out.push(run);
        }
        out
    }
}

/// The points of a boundary's outline, closed.
pub fn outline(b: &Boundary) -> Vec<Vec2> {
    match b {
        Boundary::Rect { lo, hi } => vec![*lo, Vec2::new(hi.x, lo.y), *hi, Vec2::new(lo.x, hi.y), *lo],
        Boundary::Circle { center, radius } => (0..=96)
            .map(|i| {
                let a = f64::from(i) / 96.0 * std::f64::consts::TAU;
                *center + Vec2::new(a.cos(), a.sin()) * *radius
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Orientation, SheetId, ViewId, ViewKind, ViewMod};

    fn view(mods: Vec<ModKind>, scale: f64) -> View {
        View {
            id: ViewId(1),
            sheet: SheetId(1),
            name: "VIEW1".into(),
            model: "m".into(),
            kind: ViewKind::Base { orientation: Orientation::Front },
            scale,
            center: Vec2::new(0.0, 0.0),
            hidden: false,
            tangent: false,
            centerlines: false,
            label: false,
            mods: mods.into_iter().enumerate().map(|(i, kind)| ViewMod { id: i as u32 + 1, suppressed: false, kind }).collect(),
        }
    }

    fn band(from: f64, to: f64, gap: f64) -> ModKind {
        ModKind::Break { lines: BreakLines::Vertical, from, to, gap }
    }

    #[test]
    fn a_band_closes_to_its_gap_and_opens_again() {
        // 40 to 80 taken out and 100 to 110, each leaving 3 of paper, at 1:2: 6 of the model.
        let v = view(vec![band(100.0, 110.0, 3.0), band(40.0, 80.0, 3.0)], 0.5);
        let f = Folds::of(&v);
        for (t, q) in [(0.0, 0.0), (40.0, 40.0), (80.0, 46.0), (100.0, 66.0), (110.0, 72.0), (120.0, 82.0), (60.0, 43.0)] {
            let p = Vec2::new(t, 7.0);
            assert!(f.close(p).near(Vec2::new(q, 7.0), 1e-12), "{t}");
            assert!(f.open(Vec2::new(q, 7.0)).near(p, 1e-12), "{q}");
        }
        // The view stays centred on what is left: of 0 to 120, 0 to 82, so at 41.
        let a = f.anchor(Vec2::new(60.0, 9.0), Vec2::new(0.0, 0.0), Vec2::new(120.0, 18.0));
        assert!(a.near(Vec2::new(41.0, 9.0), 1e-12));
        // A suppressed break does nothing.
        let mut w = v.clone();
        w.mods[0].suppressed = true;
        w.mods[1].suppressed = true;
        assert!(Folds::of(&w).is_empty());
    }

    #[test]
    fn a_band_with_no_gap_closes_right_up() {
        let f = Folds::of(&view(vec![band(40.0, 80.0, 0.0)], 1.0));
        assert!(f.close(Vec2::new(90.0, 0.0)).near(Vec2::new(50.0, 0.0), 1e-12));
        assert!(f.open(Vec2::new(50.0, 0.0)).near(Vec2::new(90.0, 0.0), 1e-12));
        assert!(f.close(Vec2::new(60.0, 0.0)).near(Vec2::new(40.0, 0.0), 1e-12));
    }

    #[test]
    fn lines_are_clipped_to_a_crop_and_outside_bands() {
        let line = [Vec2::new(-10.0, 5.0), Vec2::new(110.0, 5.0)];
        // Inside a rectangle: 0 to 50.
        let v = view(vec![ModKind::Crop { boundary: Boundary::rect(Vec2::new(50.0, 10.0), Vec2::new(0.0, 0.0)), outline: true }], 1.0);
        let p = Clip::of(&v).pieces(&line);
        assert_eq!(p.len(), 1);
        assert!(p[0][0].near(Vec2::new(0.0, 5.0), 1e-9) && p[0][1].near(Vec2::new(50.0, 5.0), 1e-9), "{p:?}");
        // Inside a circle of radius 5 about (20, 5): 15 to 25.
        let v = view(vec![ModKind::Crop { boundary: Boundary::Circle { center: Vec2::new(20.0, 5.0), radius: 5.0 }, outline: true }], 1.0);
        let p = Clip::of(&v).pieces(&line);
        assert!(p[0][0].near(Vec2::new(15.0, 5.0), 1e-9) && p[0][1].near(Vec2::new(25.0, 5.0), 1e-9), "{p:?}");
        // Outside a band: two pieces; a polyline stays joined up to it.
        let v = view(vec![band(40.0, 80.0, 6.0)], 1.0);
        let c = Clip::of(&v);
        let p = c.pieces(&line);
        assert_eq!(p.len(), 2);
        assert!(p[0][1].near(Vec2::new(40.0, 5.0), 1e-9) && p[1][0].near(Vec2::new(80.0, 5.0), 1e-9), "{p:?}");
        assert!(!c.keeps(Vec2::new(60.0, 0.0)) && c.keeps(Vec2::new(40.0, 0.0)) && c.keeps(Vec2::new(81.0, 0.0)));
        let bent = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)];
        assert_eq!(c.pieces(&bent).len(), 1);
        assert_eq!(c.pieces(&bent)[0].len(), 3);
        // Nothing in the way: the polyline as it is.
        assert_eq!(Clip::default().pieces(&bent), vec![bent.to_vec()]);
    }
}
