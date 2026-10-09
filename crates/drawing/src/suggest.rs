//! Suggested dimensions for a view: what a drafter would put on it first. They are ordinary
//! dimensions (persistent edge references, measured anew each time, so they follow the model);
//! the user reviews them and adds the ones wanted, as one undo step.
//!
//! For an orthographic view (base, projected or section):
//!
//! - its overall width and height, from straight edges that span the view (one edge across the
//!   whole width, or the two edges at its sides), placed outside the view;
//! - the diameter of each size of hole or boss seen end-on, once per size with a count ("4X Ø8");
//! - the radius of each size of round seen end-on ("2X R10");
//!
//! leaving out what the view's dimensions already give.

use tenon_assembly::ComponentId;
use tenon_geom::{Vec2, Vec3};
use tenon_kernel::CurveKind;
use tenon_model::EdgeRef;

use crate::annotate::{self, to_sheet};
use crate::model::{AnnotId, AnnotKind, Annotation, DimKind, Drawing, GeomPick, PickPoint, View, ViewId, ViewKind};
use crate::views::{Evaluation, project};

/// How far outside the view suggested dimensions go (sheet mm), and between rows of them.
const OUTSIDE: f64 = 10.0;

/// A suggested dimension.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub dim: DimKind,
    pub a: GeomPick,
    pub b: Option<GeomPick>,
    /// Where it goes on the sheet.
    pub at: Vec2,
    /// Its text when it stands for several features ("4X <>").
    pub text: Option<String>,
    /// What it measures now, and how it reads.
    pub value: f64,
    pub shown: String,
    /// Why it is suggested.
    pub why: String,
}

impl Suggestion {
    /// The annotation it becomes on `view`.
    pub fn kind(&self, view: ViewId, v: &View) -> AnnotKind {
        AnnotKind::Dimension {
            view,
            dim: self.dim,
            a: self.a.clone(),
            b: self.b.clone(),
            offset: self.at - v.center,
            text: self.text.clone(),
            precision: 2,
        }
    }
}

/// A model edge as the view sees it.
struct Seen {
    component: Option<ComponentId>,
    edge: EdgeRef,
    /// Its ends in the view (model mm).
    a: Vec2,
    b: Vec2,
    /// Towards the eye: of edges drawn on top of each other, the nearest is the one seen.
    depth: f64,
    straight: bool,
    /// Seen end-on: its centre in the view, radius, and whether it is a whole circle.
    round: Option<(Vec2, f64, bool)>,
}

impl Seen {
    fn pick(&self) -> GeomPick {
        GeomPick { component: self.component, edge: self.edge.clone(), point: PickPoint::Whole }
    }
}

fn seen_edges(ev: &Evaluation, v: &View) -> Result<Vec<Seen>, String> {
    let g = ev.view(v.id).ok_or("the view is not computed yet")?;
    let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
    let f = &g.frame;
    let cut = annotate::section_cut(v, ev);
    let kept = |p: Vec3| cut.is_none_or(|(point, look)| (p - point).dot(look) >= -1e-6);
    let mut out = Vec::new();
    for inst in &m.instances {
        let Some(scene) = m.scenes.get(&inst.part) else { continue };
        let fr = &inst.frame;
        let turn = |v: Vec3| fr.x() * v.x + fr.y() * v.y + fr.z() * v.z;
        for body in &scene.bodies {
            for e in 0..body.edges.len() {
                let Some(edge) = body.edge_ref(e as u32) else { continue };
                let (Some([s, t]), Some(curve)) = (body.ends.get(e), body.curves.get(e)) else { continue };
                let (ws, wt) = (fr.to_world(*s), fr.to_world(*t));
                if !kept(ws) || !kept(wt) {
                    continue;
                }
                let round = match curve {
                    CurveKind::Circle { axis, radius } if turn(axis.dir()).dot(f.z()).abs() > 1.0 - 1e-9 => {
                        Some((project(f, fr.to_world(axis.origin())), *radius, ws.dist(wt) < 1e-9))
                    }
                    _ => None,
                };
                out.push(Seen {
                    component: inst.component,
                    edge,
                    a: project(f, ws),
                    b: project(f, wt),
                    depth: ((ws + wt) * 0.5).dot(f.z()),
                    straight: matches!(curve, CurveKind::Line { .. }),
                    round,
                });
            }
        }
    }
    Ok(out)
}

/// The suggestions for a view, minus what its dimensions already give.
pub fn suggest(d: &Drawing, ev: &Evaluation, view: ViewId) -> Result<Vec<Suggestion>, String> {
    let v = d.view(view).ok_or("no such view")?;
    match &v.kind {
        ViewKind::Detail { .. } => return Err("suggestions are for whole views; dimension a detail by hand".into()),
        ViewKind::Base { orientation: crate::model::Orientation::Iso } => return Err("an isometric view is not dimensioned".into()),
        ViewKind::Projected { side, .. } if side.diagonal() => return Err("an isometric view is not dimensioned".into()),
        _ => {}
    }
    let g = ev.view(view).ok_or("the view is not computed yet")?;
    let (lo, hi) = g.bounds.ok_or("the view shows nothing")?;
    let edges = seen_edges(ev, v)?;
    let tol = 1e-6 * (hi - lo).len().max(1.0);
    let (slo, shi) = (to_sheet(v, g, lo), to_sheet(v, g, hi));
    let mut out: Vec<Suggestion> = Vec::new();

    // The nearest to the eye of the edges that pass a test, preferring the lowest `order`.
    let best = |test: &dyn Fn(&Seen) -> bool, order: &dyn Fn(&Seen) -> f64| -> Option<&Seen> {
        edges.iter().filter(|e| e.straight && test(e)).min_by(|x, y| order(x).total_cmp(&order(y)).then(y.depth.total_cmp(&x.depth)))
    };
    let near = |a: f64, b: f64| (a - b).abs() <= tol;
    let spans = |e: &Seen, along_x: bool| {
        let (a, b) = if along_x { (e.a.x.min(e.b.x), e.a.x.max(e.b.x)) } else { (e.a.y.min(e.b.y), e.a.y.max(e.b.y)) };
        let (l, h) = if along_x { (lo.x, hi.x) } else { (lo.y, hi.y) };
        near(a, l) && near(b, h)
    };
    // Width: one horizontal edge across the whole view (the lowest), else the two vertical edges
    // at its sides.
    let horizontal = |e: &Seen| near(e.a.y, e.b.y) && spans(e, true);
    let width = match best(&horizontal, &|e| e.a.y) {
        Some(e) => Some((e.pick(), None)),
        None => {
            let side = |x: f64| move |e: &Seen| near(e.a.x, e.b.x) && near(e.a.x, x);
            match (best(&side(lo.x), &|e| e.a.y.min(e.b.y)), best(&side(hi.x), &|e| e.a.y.min(e.b.y))) {
                (Some(l), Some(r)) => Some((l.pick(), Some(r.pick()))),
                _ => None,
            }
        }
    };
    if let Some((a, b)) = width {
        let at = Vec2::new((slo.x + shi.x) / 2.0, slo.y - OUTSIDE);
        out.push(Suggestion { dim: DimKind::Horizontal, a, b, at, text: None, value: 0.0, shown: String::new(), why: "overall width".into() });
    }
    // Height: one vertical edge up the whole view (the rightmost), else the edges at top and bottom.
    let vertical = |e: &Seen| near(e.a.x, e.b.x) && spans(e, false);
    let height = match best(&vertical, &|e| -e.a.x) {
        Some(e) => Some((e.pick(), None)),
        None => {
            let side = |y: f64| move |e: &Seen| near(e.a.y, e.b.y) && near(e.a.y, y);
            match (best(&side(lo.y), &|e| -e.a.x.max(e.b.x)), best(&side(hi.y), &|e| -e.a.x.max(e.b.x))) {
                (Some(b), Some(t)) => Some((b.pick(), Some(t.pick()))),
                _ => None,
            }
        }
    };
    if let Some((a, b)) = height {
        let at = Vec2::new(shi.x + OUTSIDE, (slo.y + shi.y) / 2.0);
        out.push(Suggestion { dim: DimKind::Vertical, a, b, at, text: None, value: 0.0, shown: String::new(), why: "overall height".into() });
    }

    // Circles and arcs seen end-on, by size: one dimension per size, on the one nearest the eye
    // at the top right, counting the places it stands for.
    let mut sizes: Vec<(bool, f64, Vec<&Seen>)> = Vec::new();
    for e in &edges {
        let Some((_, r, whole)) = e.round else { continue };
        match sizes.iter_mut().find(|(w, s, _)| *w == whole && (s - r).abs() <= tol) {
            Some(group) => group.2.push(e),
            None => sizes.push((whole, r, vec![e])),
        }
    }
    sizes.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)));
    let directions = [Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0), Vec2::new(1.0, -1.0), Vec2::new(-1.0, -1.0)];
    for (k, (whole, r, group)) in sizes.iter().enumerate() {
        let centre = |e: &Seen| e.round.map_or(Vec2::new(0.0, 0.0), |(c, _, _)| c);
        let mut places: Vec<Vec2> = Vec::new();
        for e in group {
            let c = if *whole { centre(e) } else { (e.a + e.b) * 0.5 };
            if !places.iter().any(|p| p.dist(c) <= tol.max(1e-6)) {
                places.push(c);
            }
        }
        let Some(rep) = group.iter().max_by(|x, y| {
            let key = |e: &Seen| centre(e).x + centre(e).y;
            key(x).total_cmp(&key(y)).then(x.depth.total_cmp(&y.depth))
        }) else {
            continue;
        };
        let dir = directions[k % directions.len()].normalized();
        let c = to_sheet(v, g, centre(rep));
        let (dim, why, outward) = if *whole {
            (DimKind::Diameter, if places.len() > 1 { format!("{} holes or bosses this size", places.len()) } else { "hole or boss".into() }, dir)
        } else {
            // Out through the middle of the arc.
            let mid = (rep.a + rep.b) * 0.5 - centre(rep);
            let out = if mid.len() > 1e-9 { mid.normalized() } else { dir };
            (DimKind::Radius, if places.len() > 1 { format!("{} rounds this size", places.len()) } else { "round".into() }, out)
        };
        let at = c + outward * (r * v.scale + 8.0 + 4.0 * (k / directions.len()) as f64);
        let text = (places.len() > 1).then(|| format!("{}X <>", places.len()));
        out.push(Suggestion { dim, a: rep.pick(), b: None, at, text, value: 0.0, shown: String::new(), why });
    }

    // Values as they measure now; drop what cannot be measured and what is already there.
    let existing: Vec<(DimKind, f64)> = d
        .annotations
        .iter()
        .filter(|a| matches!(a.kind, AnnotKind::Dimension { view: vv, .. } if vv == view))
        .filter_map(|a| match &a.kind {
            AnnotKind::Dimension { dim, .. } => annotate::dimension(d, ev, a).ok().map(|(value, _)| (*dim, value)),
            _ => None,
        })
        .collect();
    let mut kept: Vec<Suggestion> = Vec::new();
    for mut s in out {
        let probe = Annotation { id: AnnotId(u32::MAX), kind: s.kind(view, v) };
        let Ok((value, shown)) = annotate::dimension(d, ev, &probe) else { continue };
        if existing.iter().any(|(k, x)| *k == s.dim && (x - value).abs() <= 1e-6) {
            continue;
        }
        (s.value, s.shown) = (value, shown);
        kept.push(s);
    }
    Ok(kept)
}
