//! Sets of dimensions measured from one place (DEC-132): how a baseline set and an ordinate set
//! lie on the sheet. Like every annotation they keep picks and never values, so each time they
//! are drawn they are worked out again from the model as it is.
//!
//! - **Baseline:** one dimension from the datum to each place, the shortest nearest the view and
//!   the rest evenly beyond it, so no dimension line crosses the extension line of a shorter one.
//!   The datum's one extension line reaches them all. The lines are `spacing` apart, or further
//!   when their text needs it, so they never overlap.
//! - **Ordinate:** the origin marked 0 and each place its position from it, signed, on its own
//!   extension line with no arrows. The values line up; where they would run into each other
//!   they are moved apart as little as can be, and the lines jog to reach them.
//!
//! Text, lines and arrows are those of every other dimension on the sheet (the constants in
//! [`crate::graphics`]), so a set looks as the dimensions beside it do.

use tenon_geom::Vec2;

use crate::annotate::{Resolved, format_number, pick_point, resolve_pick, to_sheet};
use crate::graphics::{ARROW_LENGTH, Align, EXT_GAP, EXT_OVER, Graphics, Owner, Pen, TEXT};
use crate::model::{AnnotKind, Annotation, Drawing, GeomPick, Orientation, PickPoint, SetAxis, View, ViewId, ViewKind};
use crate::stroke::text_width;
use crate::views::{Evaluation, ModelGeometry, ViewGeometry, project};

/// How far the text of a set stands clear of what is beside it (sheet mm).
const CLEAR: f64 = 1.0;
/// Of a text's height, how much it takes up in all (`Text::bounds`).
const TEXT_TALL: f64 = 1.35;
/// How far before its value an ordinate's extension line starts to jog (sheet mm).
const JOG: f64 = 4.0;
/// How far an arrow outside its extension lines runs on behind its head (sheet mm).
const TAIL: f64 = 1.5;
/// Two places are one when they are as near as this, along what is measured (model mm).
const SAME: f64 = 1e-6;

/// What a baseline set and an ordinate set have in common.
#[derive(Clone, Copy, Debug)]
pub struct SetParts<'a> {
    pub view: ViewId,
    pub axis: SetAxis,
    /// The place everything is measured from: a baseline's datum, an ordinate's origin.
    pub from: &'a GeomPick,
    pub targets: &'a [GeomPick],
    pub offset: Vec2,
    /// A baseline's spacing; an ordinate set has none.
    pub spacing: Option<f64>,
    pub precision: u8,
}

/// The parts of a set of dimensions, if it is one.
pub fn parts(kind: &AnnotKind) -> Option<SetParts<'_>> {
    match kind {
        AnnotKind::Baseline { view, axis, datum, targets, offset, spacing, precision } => {
            Some(SetParts { view: *view, axis: *axis, from: datum, targets, offset: *offset, spacing: Some(*spacing), precision: *precision })
        }
        AnnotKind::Ordinate { view, axis, origin, targets, offset, precision } => {
            Some(SetParts { view: *view, axis: *axis, from: origin, targets, offset: *offset, spacing: None, precision: *precision })
        }
        _ => None,
    }
}

/// A place of a set, as it is laid out.
#[derive(Clone, Debug, PartialEq)]
pub struct SetRow {
    /// Which of the set's targets it is; none for an ordinate set's origin.
    pub place: Option<usize>,
    /// Where it is from the datum or origin along what is measured, signed (model mm).
    pub position: f64,
    /// What it reads as a number: a baseline's distance, an ordinate's position.
    pub value: f64,
    pub shown: String,
    /// The middle of its text on the sheet.
    pub text: Vec2,
    /// A baseline's dimension line: how far across what is measured it is on the sheet.
    pub level: Option<f64>,
}

/// A set laid out on the sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct SetLayout {
    pub axis: SetAxis,
    /// Nearest the view first (baseline), or along the sheet (ordinate).
    pub rows: Vec<SetRow>,
    /// Between one dimension line and the next, as drawn.
    pub step: Option<f64>,
    /// The places that cannot be found now: their numbers and why.
    pub lost: Vec<(usize, String)>,
    lines: Vec<Vec<Vec2>>,
    arrows: Vec<(Vec2, Vec2)>,
    texts: Vec<(Vec2, String, Align)>,
}

impl SetLayout {
    fn new(axis: SetAxis) -> SetLayout {
        SetLayout { axis, rows: Vec::new(), step: None, lost: Vec::new(), lines: Vec::new(), arrows: Vec::new(), texts: Vec::new() }
    }

    /// Puts it on a sheet.
    pub fn draw(&self, gr: &mut Graphics, o: Owner) {
        for l in &self.lines {
            gr.polyline(o, Pen::Thin, l.clone());
        }
        for (tip, dir) in &self.arrows {
            gr.arrow(o, *tip, *dir);
        }
        for (at, text, align) in &self.texts {
            gr.text(o, *at, TEXT, text.clone(), *align);
        }
    }

    /// What to say of the places that cannot be found, if any.
    pub fn problem(&self) -> Option<String> {
        let (first, why) = self.lost.first()?;
        Some(match self.lost.len() {
            1 => format!("place {} of the set cannot be found: {why}", first + 1),
            n => format!("{n} places of the set cannot be found: {why}"),
        })
    }

    /// An extension line from just off the object to just past every level it reaches.
    fn extension(&mut self, along: f64, across: f64, levels: &[f64]) {
        let lo = levels.iter().copied().fold(across, f64::min);
        let hi = levels.iter().copied().fold(across, f64::max);
        let start = if across <= lo { across + EXT_GAP } else { lo - EXT_OVER };
        let end = if across >= hi { across - EXT_GAP } else { hi + EXT_OVER };
        if end > start {
            self.lines.push(vec![self.axis.point(along, start), self.axis.point(along, end)]);
        }
    }
}

/// A place on the sheet: how far along what is measured, where its extension line starts across
/// it, and where it is in the view along what is measured (model mm).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Place {
    along: f64,
    across: f64,
    model: f64,
}

/// Half of what a text takes up along its dimension line, with the room it wants round it.
fn half_along(axis: SetAxis, text: &str) -> f64 {
    let size = match axis {
        SetAxis::Horizontal => text_width(text, TEXT),
        SetAxis::Vertical => TEXT * TEXT_TALL,
    };
    size / 2.0 + CLEAR
}

/// What a text takes up across its dimension line, with the room it wants round it. Text is
/// always written level: a vertical line's text is as wide as it is across.
fn size_across(axis: SetAxis, text: &str) -> f64 {
    let size = match axis {
        SetAxis::Horizontal => TEXT * TEXT_TALL,
        SetAxis::Vertical => text_width(text, TEXT),
    };
    size + 2.0 * CLEAR
}

/// Where a text that does not fit between the ends of its line goes: past one end or the other,
/// clear of the extension lines that cross its level. The nearer of the two; when they are as near
/// as each other, past `hi` if `prefer_hi`. `crossing` is in order.
fn beside(lo: f64, hi: f64, half: f64, crossing: &[f64], prefer_hi: bool) -> f64 {
    let off = ARROW_LENGTH + TAIL + half;
    let mut up = hi + off;
    for c in crossing {
        if (c - up).abs() < half {
            up = c + half;
        }
    }
    let mut down = lo - off;
    for c in crossing.iter().rev() {
        if (c - down).abs() < half {
            down = c - half;
        }
    }
    let (above, below) = (up - hi, lo - down);
    let as_near = (above - below).abs() <= 1e-9;
    if if as_near { prefer_hi } else { above < below } { up } else { down }
}

/// A baseline set: one dimension from `from` to each place, the shortest at `level` and the rest
/// evenly on towards `side`.
fn baseline(axis: SetAxis, from: Place, places: &[(usize, Place)], level: f64, side: f64, spacing: f64, precision: u8) -> SetLayout {
    let mut out = SetLayout::new(axis);
    let mut rows: Vec<(usize, Place, f64, String)> = places
        .iter()
        .map(|(i, p)| {
            let position = p.model - from.model;
            (*i, *p, position, format_number(position.abs(), precision))
        })
        .collect();
    rows.sort_by(|a, b| a.2.abs().total_cmp(&b.2.abs()).then(a.0.cmp(&b.0)));
    let step = rows.iter().map(|r| size_across(axis, &r.3)).fold(spacing, f64::max);
    out.step = Some(step);
    let levels: Vec<f64> = (0..rows.len()).map(|k| level + side * step * k as f64).collect();
    out.extension(from.along, from.across, &levels);
    let (u, at) = (axis.point(1.0, 0.0), |t: f64, lv: f64| axis.point(t, lv));
    for (k, (i, p, position, shown)) in rows.iter().enumerate() {
        let lv = levels.get(k).copied().unwrap_or(level);
        out.extension(p.along, p.across, &[lv]);
        let (lo, hi) = (from.along.min(p.along), from.along.max(p.along));
        let span = hi - lo;
        let half = half_along(axis, shown);
        let c = if span >= 2.0 * half + 2.0 * ARROW_LENGTH {
            // Between the extension lines, the line broken for it.
            let mid = (lo + hi) / 2.0;
            out.lines.push(vec![at(lo, lv), at(mid - half, lv)]);
            out.lines.push(vec![at(mid + half, lv), at(hi, lv)]);
            out.arrows.push((at(lo, lv), -u));
            out.arrows.push((at(hi, lv), u));
            mid
        } else {
            // Beside them, the line run out to it, clear of the longer dimensions' extension lines.
            let mut crossing: Vec<f64> = rows.iter().skip(k + 1).map(|r| r.1.along).chain(std::iter::once(from.along)).collect();
            crossing.sort_by(f64::total_cmp);
            let c = beside(lo, hi, half, &crossing, p.along >= from.along);
            let mut tail = 0.0;
            if span >= 2.0 * ARROW_LENGTH {
                out.arrows.push((at(lo, lv), -u));
                out.arrows.push((at(hi, lv), u));
            } else if span > SAME {
                // No room for the arrows either: outside, pointing in.
                out.arrows.push((at(lo, lv), u));
                out.arrows.push((at(hi, lv), -u));
                tail = ARROW_LENGTH + TAIL;
            }
            if span > SAME {
                let (near, far) = if c > hi { (lo - tail, c - half) } else { (c + half, hi + tail) };
                out.lines.push(vec![at(near, lv), at(far, lv)]);
            }
            c
        };
        let middle = at(c, lv);
        out.texts.push((Vec2::new(middle.x, middle.y - TEXT / 2.0), shown.clone(), Align::Center));
        out.rows.push(SetRow { place: Some(*i), position: *position, value: position.abs(), shown: shown.clone(), text: middle, level: Some(lv) });
    }
    out
}

/// Moves things along a line as little as can be (least squares) so that none overlaps the next.
/// `at` is in order along the line, with each one's half size; the order stays.
fn spread(at: &[f64], half: &[f64]) -> Vec<f64> {
    // Less each one's room from the first, "not overlapping" is "in order": the nearest values in
    // order are runs of equal ones, each at its mean (pool adjacent violators).
    let mut room = Vec::with_capacity(at.len());
    let mut total = 0.0;
    for i in 0..at.len() {
        if let (Some(a), Some(b)) = (i.checked_sub(1).and_then(|p| half.get(p)), half.get(i)) {
            total += a + b;
        }
        room.push(total);
    }
    let mut runs: Vec<(f64, usize)> = Vec::new();
    for (x, r) in at.iter().zip(&room) {
        runs.push((x - r, 1));
        while let [.., (sum_a, n_a), (sum_b, n_b)] = runs[..] {
            if sum_a / n_a as f64 <= sum_b / n_b as f64 {
                break;
            }
            runs.pop();
            runs.pop();
            runs.push((sum_a + sum_b, n_a + n_b));
        }
    }
    let means = runs.iter().flat_map(|(sum, n)| std::iter::repeat_n(sum / *n as f64, *n));
    means.zip(&room).map(|(m, r)| m + r).collect()
}

/// An ordinate set: `from` marked 0 and each place its position, the values lined up at `level`.
fn ordinate(axis: SetAxis, from: Place, places: &[(usize, Place)], level: f64, side: f64, precision: u8) -> SetLayout {
    let mut out = SetLayout::new(axis);
    let mut rows: Vec<(Option<usize>, Place, f64, String)> = std::iter::once((None, from, 0.0, format_number(0.0, precision)))
        .chain(places.iter().map(|(i, p)| {
            let position = p.model - from.model;
            (Some(*i), *p, position, format_number(position, precision))
        }))
        .collect();
    rows.sort_by(|a, b| a.1.along.total_cmp(&b.1.along));
    let wanted: Vec<f64> = rows.iter().map(|r| r.1.along).collect();
    let halves: Vec<f64> = rows.iter().map(|r| half_along(axis, &r.3)).collect();
    let spot = spread(&wanted, &halves);
    for ((place, p, position, shown), label) in rows.iter().zip(spot) {
        // Its value is past the end of its line: on the way the line comes.
        let way = if (level - p.across).abs() > EXT_GAP { (level - p.across).signum() } else { side };
        let start = p.across + way * EXT_GAP;
        let mut line = Vec::new();
        if (level - start) * way > 0.0 {
            line.push(axis.point(p.along, start));
            let knee = level - way * JOG;
            if (label - p.along).abs() > SAME && (knee - start) * way > 0.0 {
                line.push(axis.point(p.along, knee));
            }
            line.push(axis.point(label, level));
        }
        out.lines.push(line);
        let width = text_width(shown, TEXT);
        let (baseline_at, align, middle) = match axis {
            SetAxis::Horizontal => {
                let y = if way > 0.0 { level + CLEAR } else { level - CLEAR - TEXT };
                (Vec2::new(label, y), Align::Center, Vec2::new(label, y + TEXT / 2.0))
            }
            SetAxis::Vertical => {
                let x = level + way * CLEAR;
                let (align, mid_x) = if way > 0.0 { (Align::Left, x + width / 2.0) } else { (Align::Right, x - width / 2.0) };
                (Vec2::new(x, label - TEXT / 2.0), align, Vec2::new(mid_x, label))
            }
        };
        out.texts.push((baseline_at, shown.clone(), align));
        out.rows.push(SetRow { place: *place, position: *position, value: *position, shown: shown.clone(), text: middle, level: None });
    }
    out
}

/// Where a pick is as a place of a set, from the model as it is now. A straight edge square to
/// what is measured starts its extension line at its end nearest `level`; anything else, at the
/// point the pick stands for (a circle's centre, a line's middle, or the point).
fn place_of(v: &View, g: &ViewGeometry, m: &ModelGeometry, pick: &GeomPick, axis: SetAxis, level: f64) -> Result<Place, String> {
    let f = &g.frame;
    let p = pick_point(f, &resolve_pick(m, pick)?);
    let model = axis.along(p);
    let mut start = to_sheet(v, g, p);
    if let Resolved::Line(s, e) = resolve_pick(m, &GeomPick { point: PickPoint::Whole, ..pick.clone() })? {
        let (s, e) = (project(f, s), project(f, e));
        if (axis.along(s) - axis.along(e)).abs() <= SAME && (axis.along(s) - model).abs() <= SAME {
            let (s, e) = (to_sheet(v, g, s), to_sheet(v, g, e));
            start = if (axis.across(s) - level).abs() <= (axis.across(e) - level).abs() { s } else { e };
        }
    }
    Ok(Place { along: axis.along(start), across: axis.across(start), model })
}

/// A set as it lies on the sheet now. The places that cannot be found are told in `lost` and the
/// rest are laid out; without its datum or origin nothing can be.
pub fn layout(d: &Drawing, ev: &Evaluation, a: &Annotation) -> Result<SetLayout, String> {
    let set = parts(&a.kind).ok_or("not a set of dimensions")?;
    let v = d.view(set.view).ok_or("the view is gone")?;
    let g = ev.view(set.view).ok_or("the view is not computed")?;
    let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
    let (axis, level) = (set.axis, set.axis.across(v.center + set.offset));
    let what = if set.spacing.is_some() { "datum" } else { "origin" };
    let from = place_of(v, g, m, set.from, axis, level).map_err(|e| format!("the {what} of the set cannot be found: {e}"))?;
    let mut places = Vec::with_capacity(set.targets.len());
    let mut lost = Vec::new();
    for (i, t) in set.targets.iter().enumerate() {
        match place_of(v, g, m, t, axis, level) {
            Ok(p) => places.push((i, p)),
            Err(e) => lost.push((i, e)),
        }
    }
    // Away from the middle of the view.
    let middle = g.bounds.map_or(v.center, |(lo, hi)| to_sheet(v, g, (lo + hi) * 0.5));
    let side = if level >= axis.across(middle) { 1.0 } else { -1.0 };
    let mut out = match set.spacing {
        Some(spacing) => baseline(axis, from, &places, level, side, spacing, set.precision),
        None => ordinate(axis, from, &places, level, side, set.precision),
    };
    out.lost = lost;
    Ok(out)
}

/// A set that can be made: in a view that is not isometric, every place can be found, none is where
/// the datum or origin is, and no two are in one place along what is measured. Returns how it lies.
pub fn check(d: &Drawing, ev: &Evaluation, a: &Annotation) -> Result<SetLayout, String> {
    let on_iso = a.kind.view().and_then(|v| d.view(v)).is_some_and(|v| match &v.kind {
        ViewKind::Base { orientation: Orientation::Iso } => true,
        ViewKind::Projected { side, .. } => side.diagonal(),
        _ => false,
    });
    if on_iso {
        return Err("an isometric view is not dimensioned: pick in a view that looks square on".into());
    }
    let l = layout(d, ev, a)?;
    if let Some(p) = l.problem() {
        return Err(p);
    }
    let what = if matches!(a.kind, AnnotKind::Baseline { .. }) { "datum" } else { "origin" };
    let mut seen: Vec<(usize, f64)> = Vec::new();
    for r in &l.rows {
        let Some(n) = r.place.map(|p| p + 1) else { continue };
        if r.position.abs() <= SAME {
            return Err(format!("place {n} is where the {what} is, along this direction: pick another"));
        }
        if let Some((other, _)) = seen.iter().find(|(_, x)| (x - r.position).abs() <= SAME) {
            return Err(format!("places {other} and {n} are in one place along this direction: leave one out"));
        }
        seen.push((n, r.position));
    }
    Ok(l)
}

/// Where a pick is on the sheet now: the point it stands for (for marking it while a set is made).
pub fn pick_on_sheet(d: &Drawing, ev: &Evaluation, view: ViewId, pick: &GeomPick) -> Option<Vec2> {
    let v = d.view(view)?;
    let g = ev.view(view)?;
    let m = ev.models.get(&v.model)?;
    Some(to_sheet(v, g, pick_point(&g.frame, &resolve_pick(m, pick).ok()?)))
}

/// The sheet box of a view, from its geometry.
fn view_box(v: &View, g: &ViewGeometry) -> Option<(Vec2, Vec2)> {
    g.bounds.map(|(lo, hi)| (to_sheet(v, g, lo), to_sheet(v, g, hi)))
}

/// The way a set is to measure when not told: by where it is put, when that says (above or below
/// the view, horizontal; to its left or right, vertical), else the way its places are most spread.
pub fn axis_for(d: &Drawing, ev: &Evaluation, view: ViewId, at: Option<Vec2>, from: &GeomPick, targets: &[GeomPick]) -> Result<SetAxis, String> {
    let v = d.view(view).ok_or("the view is gone")?;
    let g = ev.view(view).ok_or("the view is not computed")?;
    let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
    if let (Some(at), Some((lo, hi))) = (at, view_box(v, g)) {
        let (beside, over) = (at.x < lo.x || at.x > hi.x, at.y < lo.y || at.y > hi.y);
        if over != beside {
            return Ok(if over { SetAxis::Horizontal } else { SetAxis::Vertical });
        }
    }
    let mut points = Vec::with_capacity(targets.len() + 1);
    for pick in std::iter::once(from).chain(targets) {
        points.push(pick_point(&g.frame, &resolve_pick(m, pick)?));
    }
    let range = |axis: SetAxis| {
        let along = points.iter().map(|p| axis.along(*p));
        along.clone().fold(f64::MIN, f64::max) - along.fold(f64::MAX, f64::min)
    };
    Ok(if range(SetAxis::Vertical) > range(SetAxis::Horizontal) { SetAxis::Vertical } else { SetAxis::Horizontal })
}

/// How far outside a view, or outside what is already beside it, a set goes unless told (sheet mm).
const OUTSIDE: f64 = 10.0;

/// Where a set goes unless told: outside the view (above it or, measuring up the sheet, to its
/// right), past the annotations already on that side. Where `room` of paper there would run off the
/// sheet or into another view, the other side.
pub fn default_at(d: &Drawing, ev: &Evaluation, view: ViewId, axis: SetAxis, room: f64) -> Result<Vec2, String> {
    let v = d.view(view).ok_or("the view is gone")?;
    let g = ev.view(view).ok_or("the view is not computed")?;
    let (lo, hi) = view_box(v, g).ok_or("the view shows nothing")?;
    let sheet = crate::annotate::build(d, v.sheet, ev).0;
    let others: Vec<(Vec2, Vec2)> = d
        .annotations
        .iter()
        .filter(|a| a.kind.view() == Some(view))
        .filter(|a| {
            matches!(a.kind, AnnotKind::Dimension { .. } | AnnotKind::Baseline { .. } | AnnotKind::Ordinate { .. } | AnnotKind::Balloon { .. })
        })
        .filter_map(|a| sheet.bounds_of(Owner::Annotation(a.id)))
        .collect();
    let views: Vec<(Vec2, Vec2)> = d.views_on(v.sheet).filter(|o| o.id != view).filter_map(|o| sheet.bounds_of(Owner::View(o.id))).collect();
    let (from, to) = (axis.along(lo), axis.along(hi));
    let free = |near: f64, far: f64| {
        let (a, b) = (near.min(far), near.max(far));
        let inside = a >= crate::sheets::BORDER && b <= axis.across(Vec2::new(sheet.width, sheet.height)) - crate::sheets::BORDER;
        inside && !views.iter().any(|(l, h)| axis.along(*l) < to && axis.along(*h) > from && axis.across(*l) < b && axis.across(*h) > a)
    };
    let past_high = others.iter().map(|b| axis.across(b.1)).filter(|y| *y > axis.across(hi)).fold(axis.across(hi), f64::max) + OUTSIDE;
    let past_low = others.iter().map(|b| axis.across(b.0)).filter(|y| *y < axis.across(lo)).fold(axis.across(lo), f64::min) - OUTSIDE;
    let level = if free(past_high, past_high + room) || !free(past_low, past_low - room) { past_high } else { past_low };
    Ok(axis.point((from + to) / 2.0, level))
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: SetAxis = SetAxis::Horizontal;
    const V: SetAxis = SetAxis::Vertical;

    /// At 1:1 from a view whose model coordinates are its sheet coordinates less 100.
    fn place(along: f64) -> Place {
        Place { along: 100.0 + along, across: 100.0, model: along }
    }

    fn places(at: &[f64]) -> Vec<(usize, Place)> {
        at.iter().enumerate().map(|(i, x)| (i, place(*x))).collect()
    }

    /// The box of the ink of each text, as it would be drawn.
    fn boxes(l: &SetLayout) -> Vec<(Vec2, Vec2, String)> {
        let mut g = Graphics::default();
        l.draw(&mut g, Owner::Frame);
        g.texts
            .iter()
            .map(|(_, t)| {
                let ink = crate::stroke::text_strokes(&t.text, t.left(), t.height).concat();
                let (mut lo, mut hi) = (ink[0], ink[0]);
                for p in &ink {
                    (lo, hi) = (lo.min(*p), hi.max(*p));
                }
                (lo, hi, t.text.clone())
            })
            .collect()
    }

    /// No two texts touch, and no line runs through a text but the one it was broken for.
    fn nothing_runs_into_anything(l: &SetLayout) {
        let bs = boxes(l);
        for (i, a) in bs.iter().enumerate() {
            for b in &bs[i + 1..] {
                let apart = a.1.x < b.0.x || b.1.x < a.0.x || a.1.y < b.0.y || b.1.y < a.0.y;
                assert!(apart, "the texts {} and {} overlap: {l:#?}", a.2, b.2);
            }
        }
        for (lo, hi, text) in &bs {
            for line in &l.lines {
                for w in line.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    // Straight lines only (with the box a little smaller, for lines that end at it).
                    let (x0, x1, y0, y1) = (a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y));
                    let square = x0 == x1 || y0 == y1;
                    let hits = square && x1 > lo.x + 0.01 && x0 < hi.x - 0.01 && y1 > lo.y + 0.01 && y0 < hi.y - 0.01;
                    assert!(!hits, "a line from {a:?} to {b:?} runs through the text {text}: {l:#?}");
                }
            }
        }
    }

    #[test]
    fn a_horizontal_baseline_stacks_from_the_shortest_out() {
        // From x = 10 to 40 (30), 110 (100), 70 (60): the shortest at the level asked, then 8 on.
        let l = baseline(H, place(10.0), &places(&[40.0, 110.0, 70.0]), 150.0, 1.0, 8.0, 2);
        assert_eq!(l.step, Some(8.0));
        let rows: Vec<(Option<usize>, f64, &str, Option<f64>)> = l.rows.iter().map(|r| (r.place, r.value, r.shown.as_str(), r.level)).collect();
        assert_eq!(rows, [(Some(0), 30.0, "30", Some(150.0)), (Some(2), 60.0, "60", Some(158.0)), (Some(1), 100.0, "100", Some(166.0))], "{l:#?}");
        // Each text in the middle of its span, on its line.
        assert_eq!(l.rows[0].text, Vec2::new(125.0, 150.0));
        assert_eq!(l.rows[1].text, Vec2::new(140.0, 158.0));
        assert_eq!(l.rows[2].text, Vec2::new(160.0, 166.0));
        // The datum's line: 1.5 off the object to 2 past the last level. Each place's, to its own.
        assert!(l.lines.contains(&vec![Vec2::new(110.0, 101.5), Vec2::new(110.0, 168.0)]), "{:?}", l.lines);
        assert!(l.lines.contains(&vec![Vec2::new(140.0, 101.5), Vec2::new(140.0, 152.0)]), "{:?}", l.lines);
        assert!(l.lines.contains(&vec![Vec2::new(170.0, 101.5), Vec2::new(170.0, 160.0)]), "{:?}", l.lines);
        assert!(l.lines.contains(&vec![Vec2::new(210.0, 101.5), Vec2::new(210.0, 168.0)]), "{:?}", l.lines);
        // Two arrows a row, pointing out at the ends of the span.
        assert_eq!(l.arrows.len(), 6);
        assert!(
            l.arrows.contains(&(Vec2::new(110.0, 150.0), Vec2::new(-1.0, 0.0))) && l.arrows.contains(&(Vec2::new(140.0, 150.0), Vec2::new(1.0, 0.0)))
        );
        nothing_runs_into_anything(&l);
    }

    #[test]
    fn a_baseline_below_the_view_stacks_downwards_with_places_on_both_sides_of_the_datum() {
        // From x = 50 to 20 (30, on the left) and to 90 (40, on the right).
        let l = baseline(H, place(50.0), &places(&[90.0, 20.0]), 60.0, -1.0, 10.0, 2);
        let rows: Vec<(f64, f64, Option<f64>)> = l.rows.iter().map(|r| (r.position, r.value, r.level)).collect();
        assert_eq!(rows, [(-30.0, 30.0, Some(60.0)), (40.0, 40.0, Some(50.0))]);
        assert!(l.lines.contains(&vec![Vec2::new(150.0, 48.0), Vec2::new(150.0, 98.5)]), "{:?}", l.lines);
        nothing_runs_into_anything(&l);
    }

    #[test]
    fn up_the_sheet_the_lines_stand_as_far_apart_as_their_level_text_needs() {
        // "120.25" is wider than 8 mm can take: the columns are all that far apart, evenly.
        let l = baseline(V, place(0.0), &places(&[40.25, 120.25, 80.25]), 300.0, 1.0, 8.0, 2);
        let wide = text_width("120.25", TEXT);
        assert!(wide > 8.0, "{wide}");
        let step = wide + 2.0;
        assert_eq!(l.step, Some(step));
        let levels: Vec<f64> = l.rows.iter().map(|r| r.level.unwrap()).collect();
        assert_eq!(levels, [300.0, 300.0 + step, 300.0 + 2.0 * step]);
        // A vertical line's text sits on the line at the middle of the span.
        assert_eq!(l.rows[0].text, Vec2::new(300.0, 100.0 + 40.25 / 2.0));
        nothing_runs_into_anything(&l);
    }

    #[test]
    fn asked_for_less_than_text_takes_the_lines_stay_clear() {
        let l = baseline(H, place(0.0), &places(&[30.0, 60.0, 90.0]), 150.0, 1.0, 1.0, 2);
        // A text's height (3.5 and its tail) and 1 clear above and below it: 6.725.
        assert!((l.step.unwrap() - 6.725).abs() < 1e-12, "{:?}", l.step);
        nothing_runs_into_anything(&l);
    }

    #[test]
    fn a_dimension_too_short_for_its_text_has_it_beside_clear_of_longer_ones() {
        // 4 from the datum: too short for "4" and two arrows, so it goes past the far end.
        let l = baseline(H, place(0.0), &places(&[4.0, 6.0, 60.0]), 150.0, 1.0, 8.0, 2);
        nothing_runs_into_anything(&l);
        let half = half_along(H, "4");
        // Past the near end by an arrow and its tail and half the text.
        let want = 104.0 + ARROW_LENGTH + TAIL + half;
        assert!((l.rows[0].text.x - want).abs() < 1e-9, "{:?} {want}", l.rows[0]);
        // Arrows point in from outside.
        assert!(
            l.arrows.contains(&(Vec2::new(100.0, 150.0), Vec2::new(1.0, 0.0))) && l.arrows.contains(&(Vec2::new(104.0, 150.0), Vec2::new(-1.0, 0.0)))
        );
        // Where an extension line would run through the text, it is moved past it.
        let beyond = baseline(H, place(0.0), &places(&[4.0, 12.0, 60.0]), 150.0, 1.0, 8.0, 2);
        nothing_runs_into_anything(&beyond);
        let t = beyond.rows[0].text.x;
        assert!(t - half >= 112.0 - 1e-9 || t + half <= 112.0 + 1e-9, "{t}");
        // A dozen close together, every which way.
        let close: Vec<f64> = (0..12).map(|i| f64::from(i) * 1.5 + 1.0).collect();
        nothing_runs_into_anything(&baseline(H, place(0.0), &places(&close), 150.0, 1.0, 8.0, 2));
        nothing_runs_into_anything(&baseline(V, place(0.0), &places(&close), 150.0, 1.0, 8.0, 2));
    }

    #[test]
    fn values_move_apart_as_little_as_can_be_and_stay_in_order() {
        // Worked out by hand: 0 and 1 and 2 with half sizes 5 close up about their mean, 1.
        assert_eq!(spread(&[0.0, 1.0, 2.0], &[5.0, 5.0, 5.0]), [-9.0, 1.0, 11.0]);
        // Already clear: nothing moves.
        assert_eq!(spread(&[0.0, 20.0, 40.0], &[5.0, 5.0, 5.0]), [0.0, 20.0, 40.0]);
        // Only those too close move: the first two by 4 each way, the third not at all.
        assert_eq!(spread(&[0.0, 2.0, 40.0], &[5.0, 5.0, 5.0]), [-4.0, 6.0, 40.0]);
        // Moving two pushes into a third: all three as one, about (0 + 2 + 12) / 3 - ...
        let p = spread(&[0.0, 2.0, 12.0], &[5.0, 5.0, 5.0]);
        assert!((p[1] - p[0] - 10.0).abs() < 1e-12 && (p[2] - p[1] - 10.0).abs() < 1e-12, "{p:?}");
        assert!(((p[0] + p[1] + p[2]) - 14.0).abs() < 1e-12, "the mean stays: {p:?}");
        assert!(spread(&[], &[]).is_empty());
    }

    #[test]
    fn ordinates_read_their_signed_positions_on_lines_with_no_arrows() {
        // Origin at x = 20; places at 80 (+60), 10 (-10); values at y = 150, the view below them.
        let l = ordinate(H, place(20.0), &places(&[80.0, 10.0]), 150.0, 1.0, 2);
        let rows: Vec<(Option<usize>, f64, &str)> = l.rows.iter().map(|r| (r.place, r.value, r.shown.as_str())).collect();
        assert_eq!(rows, [(Some(1), -10.0, "-10"), (None, 0.0, "0"), (Some(0), 60.0, "60")], "along the sheet");
        assert!(l.arrows.is_empty());
        // Clear of each other: straight up from 1.5 off the object, the value just above the line.
        assert_eq!(l.lines[1], [Vec2::new(120.0, 101.5), Vec2::new(120.0, 150.0)]);
        assert_eq!(l.rows[1].text, Vec2::new(120.0, 151.0 + TEXT / 2.0));
        nothing_runs_into_anything(&l);
    }

    #[test]
    fn ordinates_too_close_jog_apart_and_their_lines_reach_them() {
        // Places 1 apart with values wider than that: each value's half width and 1 clear.
        let l = ordinate(H, place(0.0), &places(&[40.0, 41.0, 42.0]), 150.0, 1.0, 2);
        nothing_runs_into_anything(&l);
        let half = |s: &str| half_along(H, s);
        for w in l.rows.windows(2) {
            assert!(w[1].text.x - w[0].text.x >= half(&w[0].shown) + half(&w[1].shown) - 1e-9, "{w:?}");
        }
        // Of three as wide as each other the middle one keeps its place, and the others move by
        // the same; they are clear of the origin, which does not move at all.
        let width = 2.0 * half("40");
        assert!((l.rows[2].text.x - 141.0).abs() < 1e-9 && (l.rows[1].text.x - (141.0 - width)).abs() < 1e-9, "{l:#?}");
        assert_eq!(l.lines[0], [Vec2::new(100.0, 101.5), Vec2::new(100.0, 150.0)]);
        // A line that jogs goes up from 1.5 off the object to 4 below the values, then to its value.
        let jog = &l.lines[1];
        assert_eq!(jog.len(), 3, "{jog:?}");
        assert_eq!(jog[0], Vec2::new(140.0, 101.5));
        assert_eq!(jog[1], Vec2::new(140.0, 146.0));
        assert_eq!(jog[2], Vec2::new(l.rows[1].text.x, 150.0));
        // Up the sheet, the same, with values to the right of the line's end, letter height apart.
        let up = ordinate(V, place(0.0), &places(&[1.0, 2.0, 3.0]), 300.0, 1.0, 2);
        for w in up.rows.windows(2) {
            assert!(w[1].text.y - w[0].text.y >= 2.0 * half_along(V, "1") - 1e-9, "{w:?}");
        }
        assert_eq!(up.texts[0].1, "0");
        assert_eq!(up.texts[0].2, Align::Left);
        assert_eq!(up.texts[0].0.x, 301.0);
        nothing_runs_into_anything(&up);
        // To the left of the view they end at the line's end the other way.
        let left =
            ordinate(V, Place { along: 100.0, across: 100.0, model: 0.0 }, &[(0, Place { along: 140.0, across: 100.0, model: 40.0 })], 50.0, -1.0, 2);
        assert_eq!(left.texts[1].2, Align::Right);
        assert_eq!(left.texts[1].0, Vec2::new(49.0, 140.0 - TEXT / 2.0));
        assert_eq!(left.lines[1], [Vec2::new(98.5, 140.0), Vec2::new(50.0, 140.0)]);
    }
}
