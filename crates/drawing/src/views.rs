//! Views: which way each looks (frames), and their geometry from the model through the kernel:
//! hidden-line removal, section cuts with hatching, detail circles.
//!
//! View coordinates are model millimetres in the view plane: `x` right, `y` up. A view's frame
//! has its origin at the model origin and Z towards the viewer.

use std::collections::BTreeMap;
use std::sync::Arc;

use tenon_assembly::position::JointValue;
use tenon_assembly::{Assembly, ComponentId};
use tenon_geom::{Aabb3, Axis, Frame, Vec2, Vec3, tol};
use tenon_kernel::{BoolOp, FaceId, HlrCurve, HlrKind, Kernel, MeshTol, ShapeHandle, SurfaceKind};
use tenon_model::{Document, Scene};

use crate::annotate::{Resolved, edge_is_straight, resolve_pick};
use crate::model::{Boundary, Drawing, GeomPick, ModKind, Orientation, PickPoint, Side, View, ViewId, ViewKind};
use crate::modify::Clip;

fn frame(z: Vec3, x: Vec3) -> Frame {
    Frame::new(Vec3::ZERO, z, x).unwrap_or(Frame::WORLD)
}

/// The frame of a base view.
pub fn base_frame(o: Orientation) -> Frame {
    match o {
        Orientation::Front => frame(-Vec3::Y, Vec3::X),
        Orientation::Back => frame(Vec3::Y, -Vec3::X),
        Orientation::Top => frame(Vec3::Z, Vec3::X),
        Orientation::Bottom => frame(-Vec3::Z, Vec3::X),
        Orientation::Right => frame(Vec3::X, Vec3::Y),
        Orientation::Left => frame(-Vec3::X, -Vec3::Y),
        Orientation::Iso => frame(Vec3::new(1.0, -1.0, 1.0), Vec3::new(1.0, 1.0, 0.0)),
    }
}

/// The frame of a view placed on `side` of a view with frame `p`. Third-angle: a view to the
/// right shows the model from the right; first-angle: from the left.
pub fn projected_frame(p: &Frame, side: Side, third_angle: bool) -> Frame {
    let step = side.step();
    let (sx, sy) = if third_angle { (step.x, step.y) } else { (-step.x, -step.y) };
    let (px, py, pz) = (p.x(), p.y(), p.z());
    if sy == 0.0 {
        frame(px * sx, -(pz * sx))
    } else if sx == 0.0 {
        Frame::new(Vec3::ZERO, py * sy, px).unwrap_or(*p)
    } else {
        frame(px * sx + py * sy + pz, px - pz * sx)
    }
}

/// A section's frame, the plane's point, and the direction it is seen in (assembly or part
/// coordinates): the cut is along `a`-`b` of the parent view, seen towards the right of `a`-`b`
/// (`flip`: the left).
pub fn section_frame(p: &Frame, a: Vec2, b: Vec2, flip: bool) -> (Frame, Vec3, Vec3) {
    let d = (b - a).normalized();
    let look = if flip { Vec2::new(-d.y, d.x) } else { Vec2::new(d.y, -d.x) };
    let world = |v: Vec2| p.x() * v.x + p.y() * v.y;
    let l = world(look);
    // Unfolded from the parent like a projected view: the parent's frame turned a quarter turn
    // about the cutting line, so its Z comes round to the eye's side (against the look).
    let eye = -l;
    let axis = p.z().cross(eye);
    let turn = |v: Vec3| axis * axis.dot(v) + axis.cross(v);
    let f = Frame::new(Vec3::ZERO, eye, turn(p.x())).unwrap_or(*p);
    (f, world(a), l)
}

/// Every view's frame (parents before children), as far as the drawing alone can say: an
/// auxiliary view's edge is where it last was. See [`frames_in`].
pub fn frames(d: &Drawing) -> BTreeMap<ViewId, Frame> {
    frames_in(d, &BTreeMap::new())
}

/// Every view's frame (parents before children), with an auxiliary view's edge found in its model
/// as it is now.
pub fn frames_in(d: &Drawing, models: &BTreeMap<String, ModelGeometry>) -> BTreeMap<ViewId, Frame> {
    let mut out = BTreeMap::new();
    let third = d.standard == crate::model::Standard::Ansi;
    for v in &d.views {
        let f = match &v.kind {
            ViewKind::Base { orientation } => base_frame(*orientation),
            ViewKind::Projected { parent, side } => out.get(parent).map_or(Frame::WORLD, |p| projected_frame(p, *side, third)),
            ViewKind::Section { parent, a, b, flip } => out.get(parent).map_or(Frame::WORLD, |p| section_frame(p, *a, *b, *flip).0),
            ViewKind::Detail { parent, .. } | ViewKind::Overlay { parent, .. } => out.get(parent).copied().unwrap_or(Frame::WORLD),
            ViewKind::Auxiliary { parent, edge, a, b, flip } => out.get(parent).map_or(Frame::WORLD, |p| {
                let (a, b) = models.get(&v.model).and_then(|m| auxiliary_line(m, p, edge, *a, *b).ok()).unwrap_or((*a, *b));
                section_frame(p, a, b, *flip).0
            }),
        };
        out.insert(v.id, f);
    }
    out
}

/// The line of an auxiliary view's edge in its parent view (frame `pf`), as the model has it now,
/// the way round `a`-`b` is: or why it cannot be found. Seen from the right of that line, it gives
/// the view's frame, as a section's line does.
pub fn auxiliary_line(m: &ModelGeometry, pf: &Frame, edge: &GeomPick, a: Vec2, b: Vec2) -> Result<(Vec2, Vec2), String> {
    let whole = GeomPick { point: PickPoint::Whole, ..edge.clone() };
    let Resolved::Line(s, e) = resolve_pick(m, &whole)? else { return Err("the edge it looks square to is no longer straight".into()) };
    if !edge_is_straight(m, &whole)? {
        return Err("the edge it looks square to is no longer straight".into());
    }
    let (s, e) = (project(pf, s), project(pf, e));
    if s.dist(e) < tol::LINEAR {
        return Err("the edge it looks square to is seen end-on in the parent view".into());
    }
    Ok(if (e - s).dot(b - a) < 0.0 { (e, s) } else { (s, e) })
}

/// A point in view coordinates.
pub fn project(f: &Frame, p: Vec3) -> Vec2 {
    Vec2::new(p.dot(f.x()), p.dot(f.y()))
}

/// A model's size seen in a view frame (model mm, from its bounding box): what a view of it at
/// scale 1 takes on the sheet, at most.
pub fn seen_size(m: &ModelGeometry, f: &Frame) -> Option<Vec2> {
    let pts = seen_corners(m, f)?;
    let lo = pts.iter().fold(Vec2::new(f64::MAX, f64::MAX), |a, p| a.min(*p));
    let hi = pts.iter().fold(Vec2::new(f64::MIN, f64::MIN), |a, p| a.max(*p));
    Some(hi - lo)
}

/// How far a model's box seen in a view frame reaches from its middle along `way`, a unit vector of
/// the view (view coordinates). Across a slanted edge a box's corners stand out far less than its
/// [`seen_size`] says.
pub fn seen_reach(m: &ModelGeometry, f: &Frame, way: Vec2) -> Option<f64> {
    let along: Vec<f64> = seen_corners(m, f)?.iter().map(|p| p.dot(way)).collect();
    let (lo, hi) = along.iter().fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
    Some((hi - lo) * 0.5)
}

/// The corners of a model's box seen in a view frame.
fn seen_corners(m: &ModelGeometry, f: &Frame) -> Option<Vec<Vec2>> {
    let b = m.bbox?;
    let (l, h) = (b.min, b.max);
    Some(
        (0..8)
            .map(|i| {
                project(f, Vec3::new(if i & 1 == 0 { l.x } else { h.x }, if i & 2 == 0 { l.y } else { h.y }, if i & 4 == 0 { l.z } else { h.z }))
            })
            .collect(),
    )
}

/// The middle of a model's box seen in a view frame (view coordinates).
pub fn seen_middle(m: &ModelGeometry, f: &Frame) -> Option<Vec2> {
    m.bbox.map(|b| box_center(f, &b))
}

/// What a model is made of, for the kernel to build: a part document, or an assembly and its
/// part documents (by part key).
#[derive(Clone, Debug)]
pub enum ModelSource {
    Part(Box<Document>),
    Assembly { asm: Assembly, parts: BTreeMap<String, Document> },
}

/// One placed occurrence of a part in a model (the part itself, for a part model).
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    pub component: Option<ComponentId>,
    pub part: String,
    pub frame: Frame,
}

/// A model as the drawing sees it: its parts' geometry and where each occurrence is.
#[derive(Clone, Debug, Default)]
pub struct ModelGeometry {
    pub instances: Vec<Instance>,
    pub scenes: BTreeMap<String, Arc<Scene>>,
    /// Part documents (hole tables read their holes).
    pub documents: BTreeMap<String, Document>,
    /// The assembly, for a model that is one (balloons, parts lists).
    pub assembly: Option<Assembly>,
    pub bbox: Option<Aabb3>,
    /// Why the model could not be built, if it could not.
    pub error: Option<String>,
}

impl ModelGeometry {
    /// The instance of a component (or the part's only one).
    pub fn instance(&self, component: Option<ComponentId>) -> Option<&Instance> {
        self.instances.iter().find(|i| i.component == component)
    }

    /// The assembly's parts as its solver wants them: their documents and geometry.
    pub fn parts(&self) -> tenon_assembly::Parts {
        self.documents
            .iter()
            .map(|(key, doc)| {
                let mut part = tenon_assembly::Part::new(doc.clone());
                part.scene = self.scenes.get(key).map(|s| (part.session.revision(), s.clone()));
                (key.clone(), part)
            })
            .collect()
    }

    /// Where each component is in the position `values` give (see
    /// [`tenon_assembly::position::position_frames`]).
    pub fn position_frames(&self, values: &[JointValue]) -> Result<BTreeMap<ComponentId, Frame>, String> {
        let asm = self.assembly.as_ref().ok_or("an overlay is for a view of an assembly")?;
        tenon_assembly::position::position_frames(asm, &self.parts(), values)
    }
}

/// A view's computed geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewGeometry {
    pub frame: Frame,
    pub curves: Vec<HlrCurve>,
    /// Hatch segments of a section's cut faces (view coordinates).
    pub hatch: Vec<[Vec2; 2]>,
    /// Where the view's middle is (view coordinates): it goes to the view's centre on the sheet.
    pub center: Vec2,
    /// Bounds of the visible geometry (view coordinates).
    pub bounds: Option<(Vec2, Vec2)>,
    pub error: Option<String>,
}

impl Default for ViewGeometry {
    fn default() -> Self {
        ViewGeometry { frame: Frame::WORLD, curves: Vec::new(), hatch: Vec::new(), center: Vec2::new(0.0, 0.0), bounds: None, error: None }
    }
}

/// Everything computed for a drawing: models and views.
#[derive(Clone, Debug, Default)]
pub struct Evaluation {
    pub models: BTreeMap<String, ModelGeometry>,
    pub views: BTreeMap<ViewId, ViewGeometry>,
}

impl Evaluation {
    pub fn view(&self, id: ViewId) -> Option<&ViewGeometry> {
        self.views.get(&id)
    }
}

fn placed_bbox(b: &Aabb3, f: &Frame) -> Aabb3 {
    tenon_assembly::session::placed_bbox(b, f)
}

/// A model's solids for the views: all placed where they stand, and of an assembly, each
/// component's in its part's coordinates, to be placed elsewhere. The caller releases them.
#[derive(Default)]
struct Solids {
    placed: Vec<ShapeHandle>,
    parts: Vec<(ComponentId, Vec<ShapeHandle>)>,
}

impl Solids {
    fn release(self, k: &mut dyn Kernel) {
        for s in self.placed.into_iter().chain(self.parts.into_iter().flat_map(|(_, shapes)| shapes)) {
            k.release(s);
        }
    }
}

/// Builds the models' solids: per model, its geometry and its solids. `overlaid`: an overlay view
/// wants the components' solids to place elsewhere.
fn build_model(k: &mut dyn Kernel, key: &str, src: &ModelSource, overlaid: bool) -> (ModelGeometry, Solids) {
    let mut g = ModelGeometry::default();
    let mut solids = Solids::default();
    let docs: Vec<(String, Document, Vec<(Option<ComponentId>, Frame)>)> = match src {
        ModelSource::Part(doc) => vec![(key.to_owned(), (**doc).clone(), vec![(None, Frame::WORLD)])],
        ModelSource::Assembly { asm, parts } => {
            g.assembly = Some(asm.clone());
            parts
                .iter()
                .map(|(pk, doc)| {
                    let at = asm.shown().filter(|c| c.key() == *pk).map(|c| (Some(c.id), c.placement)).collect();
                    (pk.clone(), doc.clone(), at)
                })
                .collect()
        }
    };
    for (pk, doc, at) in docs {
        // A part file every component of which uses a row of its design table is nowhere itself:
        // only its name is wanted (the parts list reads it). A part whose components are all hidden
        // is drawn nowhere, but the assembly's solver still needs its geometry to place what is shown
        // (an overlay's position): that is all it gives, so what is wrong with it is said only if
        // that comes to be wanted.
        let hidden = at.is_empty() && matches!(src, ModelSource::Assembly { asm, .. } if asm.components.iter().any(|c| c.key() == pk));
        if at.is_empty() && !hidden {
            g.documents.insert(pk, doc);
            continue;
        }
        let pk_shown = tenon_assembly::key_file(&pk).to_owned();
        let mut regen = tenon_model::regenerate(&doc, k);
        if !hidden && let Some((f, msg)) = regen.first_error() {
            let name = doc.feature(f).map_or_else(|| f.to_string(), |x| x.name.clone());
            g.error = Some(format!("{pk_shown} does not build: {name}: {msg}"));
        }
        match tenon_model::scene(&regen, k, &MeshTol::default()) {
            Ok(s) => {
                if let Some(b) = s.bbox() {
                    for (_, f) in &at {
                        let pb = placed_bbox(&b, f);
                        g.bbox = Some(g.bbox.map_or(pb, |x| x.union(&pb)));
                    }
                }
                g.scenes.insert(pk.clone(), Arc::new(s));
            }
            Err(e) if !hidden => g.error = Some(format!("{pk}: {e}")),
            Err(_) => {}
        }
        for (component, f) in &at {
            g.instances.push(Instance { component: *component, part: pk.clone(), frame: *f });
            for b in &regen.bodies {
                let placed = if component.is_some() { tenon_assembly::interfere::place(k, b.shape, f).ok() } else { k.duplicate(b.shape).ok() };
                solids.placed.extend(placed);
            }
            if let (Some(c), true) = (component, overlaid) {
                solids.parts.push((*c, regen.bodies.iter().filter_map(|b| k.duplicate(b.shape).ok()).collect()));
            }
        }
        g.documents.insert(pk, doc);
        regen.release(k);
    }
    (g, solids)
}

/// Distance from `p` to segment `a`-`b`.
fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + ab * t)
}

/// Hidden curves less the parts lying on visible ones (an edge behind a visible edge is not
/// drawn dashed).
pub fn drop_covered(curves: Vec<HlrCurve>, eps: f64) -> Vec<HlrCurve> {
    let visible: Vec<[Vec2; 2]> =
        curves.iter().filter(|c| c.visible).flat_map(|c| c.points.windows(2).map(|w| [w[0], w[1]]).collect::<Vec<_>>()).collect();
    let covered = |p: Vec2| visible.iter().any(|s| seg_dist(p, s[0], s[1]) < eps);
    let mut out = Vec::new();
    for c in curves {
        if c.visible {
            out.push(c);
            continue;
        }
        // Keep the runs of segments that are not covered.
        let mut run: Vec<Vec2> = Vec::new();
        for w in c.points.windows(2) {
            let (a, b) = (w[0], w[1]);
            let hidden_here = !(covered(a) && covered(b) && covered((a + b) * 0.5));
            if hidden_here {
                if run.is_empty() {
                    run.push(a);
                }
                run.push(b);
            } else if run.len() >= 2 {
                out.push(HlrCurve { kind: c.kind, visible: false, points: std::mem::take(&mut run) });
            } else {
                run.clear();
            }
        }
        if run.len() >= 2 {
            out.push(HlrCurve { kind: c.kind, visible: false, points: run });
        }
    }
    out
}

/// `curves` less what lies along the visible lines of `seen`: of a position drawn over a view, what
/// has not moved is drawn only once.
pub fn drop_seen(curves: Vec<HlrCurve>, seen: &[HlrCurve], eps: f64) -> Vec<HlrCurve> {
    let lines: Vec<[Vec2; 2]> =
        seen.iter().filter(|c| c.visible).flat_map(|c| c.points.windows(2).map(|w| [w[0], w[1]]).collect::<Vec<_>>()).collect();
    let on_a_line = |p: Vec2| lines.iter().any(|s| seg_dist(p, s[0], s[1]) < eps);
    let mut out = Vec::new();
    for c in curves {
        let mut run: Vec<Vec2> = Vec::new();
        let mut end_run = |run: &mut Vec<Vec2>| {
            if run.len() >= 2 {
                out.push(HlrCurve { kind: c.kind, visible: c.visible, points: std::mem::take(run) });
            }
            run.clear();
        };
        for w in c.points.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = a.dist(b);
            if len < 1e-12 {
                continue;
            }
            let u = (b - a) * (1.0 / len);
            let n = Vec2::new(-u.y, u.x);
            // What of it the lines along it cover (distances from `a`).
            let mut covered: Vec<(f64, f64)> = lines
                .iter()
                .filter(|s| (s[0] - a).dot(n).abs() < eps && (s[1] - a).dot(n).abs() < eps)
                .map(|s| {
                    let (p, q) = ((s[0] - a).dot(u), (s[1] - a).dot(u));
                    (p.min(q).max(0.0), p.max(q).min(len))
                })
                .filter(|(p, q)| q > p)
                .collect();
            covered.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut from = 0.0;
            let mut spans: Vec<(f64, f64)> = Vec::new();
            for (p, q) in covered {
                if p > from + eps {
                    spans.push((from, p));
                }
                from = q.max(from);
            }
            if len > from + eps {
                spans.push((from, len));
            }
            for (s, t) in spans {
                let (p, q) = (a + u * s, a + u * t);
                // (A chord of a curve lies along no line of the same curve chopped differently.)
                if on_a_line(p) && on_a_line(q) && on_a_line((p + q) * 0.5) {
                    end_run(&mut run);
                    continue;
                }
                if run.last().is_none_or(|l| l.dist(p) > 1e-9) {
                    end_run(&mut run);
                    run.push(p);
                }
                run.push(q);
            }
        }
        end_run(&mut run);
    }
    out
}

/// Hatch lines at 45 degrees, `spacing` apart, over the union of `triangles`.
pub fn hatch(triangles: &[[Vec2; 3]], spacing: f64) -> Vec<[Vec2; 2]> {
    hatch_at(triangles, spacing, std::f64::consts::FRAC_PI_4)
}

/// The hatching of the `i`th solid cut by a section: 45 degrees, then 135, with wider spacing
/// for the next pair, and so on, so parts that touch read apart.
pub fn part_hatch(i: usize) -> (f64, f64) {
    let angle = if i.is_multiple_of(2) { std::f64::consts::FRAC_PI_4 } else { 3.0 * std::f64::consts::FRAC_PI_4 };
    let spacing = if (i / 2).is_multiple_of(2) { 1.0 } else { 1.6 };
    (angle, spacing)
}

/// Hatch lines at `angle` (radians from the view's X), `spacing` apart, over the union of
/// `triangles`.
pub fn hatch_at(triangles: &[[Vec2; 3]], spacing: f64, angle: f64) -> Vec<[Vec2; 2]> {
    if triangles.is_empty() || !(spacing.is_finite() && spacing > 0.0) || !angle.is_finite() {
        return Vec::new();
    }
    let u = Vec2::new(angle.cos(), angle.sin());
    let n = Vec2::new(-u.y, u.x);
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for t in triangles {
        for p in t {
            lo = lo.min(p.dot(n));
            hi = hi.max(p.dot(n));
        }
    }
    let mut out = Vec::new();
    let first = (lo / spacing).ceil() as i64;
    let last = (hi / spacing).floor() as i64;
    if last - first > 100_000 {
        return out;
    }
    for k in first..=last {
        let c = k as f64 * spacing;
        // Where the line enters and leaves each triangle (parameter along u).
        let mut spans: Vec<(f64, f64)> = Vec::new();
        for t in triangles {
            let mut ts = Vec::new();
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                let (da, db) = (a.dot(n) - c, b.dot(n) - c);
                if (da <= 0.0 && db > 0.0) || (da > 0.0 && db <= 0.0) {
                    let s = da / (da - db);
                    ts.push((a + (b - a) * s).dot(u));
                }
            }
            if ts.len() >= 2 {
                let (a, b) = (ts.iter().copied().fold(f64::MAX, f64::min), ts.iter().copied().fold(f64::MIN, f64::max));
                spans.push((a, b));
            }
        }
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for s in spans {
            match merged.last_mut() {
                Some(m) if s.0 <= m.1 + 1e-9 => m.1 = m.1.max(s.1),
                _ => merged.push(s),
            }
        }
        for (a, b) in merged {
            if b - a > 1e-9 {
                out.push([n * c + u * a, n * c + u * b]);
            }
        }
    }
    out
}

/// A hatch line less where it runs over `triangles`. What is left of it shorter than `shortest` is
/// the rounding of the triangles' own numbers, and goes.
fn hatch_less(line: [Vec2; 2], triangles: &[[Vec2; 3]], shortest: f64) -> Vec<[Vec2; 2]> {
    let (a, d) = (line[0], line[1] - line[0]);
    let cross = |u: Vec2, w: Vec2| u.x * w.y - u.y * w.x;
    // The stretches of it (parameters from 0 to 1) inside each triangle.
    let mut over: Vec<(f64, f64)> = Vec::new();
    for t in triangles {
        let area = cross(t[1] - t[0], t[2] - t[0]);
        if area == 0.0 {
            continue;
        }
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        for i in 0..3 {
            let (p, side) = (t[i], t[(i + 1) % 3] - t[i]);
            // Positive on the triangle's own side of this one of its sides.
            let (at, rate) = (cross(side, a - p) * area.signum(), cross(side, d) * area.signum());
            if rate > 0.0 {
                lo = lo.max(-at / rate);
            } else if rate < 0.0 {
                hi = hi.min(-at / rate);
            } else if at < 0.0 {
                hi = lo;
            }
        }
        if hi > lo {
            over.push((lo, hi));
        }
    }
    over.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out = Vec::new();
    let mut from = 0.0_f64;
    for (lo, hi) in over.into_iter().chain([(1.0, 1.0)]) {
        if lo > from && (lo - from) * d.len() >= shortest {
            out.push([a + d * from, a + d * lo]);
        }
        from = from.max(hi);
    }
    out
}

/// The parts of curves inside the circle (`center`, `radius`).
pub fn clip_to_circle(curves: &[HlrCurve], center: Vec2, radius: f64) -> Vec<HlrCurve> {
    let inside = |p: Vec2| p.dist(center) <= radius;
    let mut out = Vec::new();
    for c in curves {
        let mut run: Vec<Vec2> = Vec::new();
        for w in c.points.windows(2) {
            let (a, b) = (w[0], w[1]);
            // Where the segment crosses the circle.
            let d = b - a;
            let f = a - center;
            let (qa, qb, qc) = (d.dot(d), 2.0 * f.dot(d), f.dot(f) - radius * radius);
            let disc = qb * qb - 4.0 * qa * qc;
            let mut cuts = vec![0.0, 1.0];
            if qa > 0.0 && disc > 0.0 {
                for t in [(-qb - disc.sqrt()) / (2.0 * qa), (-qb + disc.sqrt()) / (2.0 * qa)] {
                    if t > 0.0 && t < 1.0 {
                        cuts.push(t);
                    }
                }
            }
            cuts.sort_by(f64::total_cmp);
            for s in cuts.windows(2) {
                let (p, q) = (a + d * s[0], a + d * s[1]);
                if inside((p + q) * 0.5) {
                    if run.last().is_none_or(|l| l.dist(p) > 1e-9) {
                        if run.len() >= 2 {
                            out.push(HlrCurve { kind: c.kind, visible: c.visible, points: std::mem::take(&mut run) });
                        }
                        run.clear();
                        run.push(p);
                    }
                    run.push(q);
                } else if run.len() >= 2 {
                    out.push(HlrCurve { kind: c.kind, visible: c.visible, points: std::mem::take(&mut run) });
                } else {
                    run.clear();
                }
            }
        }
        if run.len() >= 2 {
            out.push(HlrCurve { kind: c.kind, visible: c.visible, points: run });
        }
    }
    out
}

fn bounds(curves: &[HlrCurve]) -> Option<(Vec2, Vec2)> {
    let mut it = curves.iter().filter(|c| c.visible).flat_map(|c| c.points.iter().copied());
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
}

/// The middle of a model's box seen in a view.
fn box_center(f: &Frame, b: &Aabb3) -> Vec2 {
    let (l, h) = (b.min, b.max);
    let pts: Vec<Vec2> = (0..8)
        .map(|i| project(f, Vec3::new(if i & 1 == 0 { l.x } else { h.x }, if i & 2 == 0 { l.y } else { h.y }, if i & 4 == 0 { l.z } else { h.z })))
        .collect();
    let lo = pts.iter().fold(Vec2::new(f64::MAX, f64::MAX), |a, p| a.min(*p));
    let hi = pts.iter().fold(Vec2::new(f64::MIN, f64::MIN), |a, p| a.max(*p));
    (lo + hi) * 0.5
}

/// Chord tolerance on paper (mm): view curves are this close to the true ones when printed.
const PAPER_DEFLECTION: f64 = 0.02;
/// Spacing of section hatching on paper (mm).
pub const HATCH_SPACING: f64 = 3.0;
/// Single-precision numbers (a mesh's) are good to about this at the size of anything drawn (mm).
const MESH_ROUNDING: f64 = 1e-3;

/// One view's geometry from the placed solids of its model.
fn compute_view(
    k: &mut dyn Kernel,
    v: &View,
    f: &Frame,
    built: &Solids,
    model: &ModelGeometry,
    parent: Option<&ViewGeometry>,
    parent_frame: Option<&Frame>,
) -> ViewGeometry {
    let solids = &built.placed;
    let mut g = ViewGeometry { frame: *f, ..ViewGeometry::default() };
    let deflection = (PAPER_DEFLECTION / v.scale).clamp(1e-4, 5.0);
    if let ViewKind::Detail { center, radius, .. } = &v.kind {
        let Some(p) = parent else {
            g.error = Some("the parent view has no geometry".into());
            return g;
        };
        g.curves = clip_to_circle(&p.curves, *center, *radius);
        g.hatch = p
            .hatch
            .iter()
            .flat_map(|s| clip_to_circle(&[HlrCurve { kind: HlrKind::Sharp, visible: true, points: s.to_vec() }], *center, *radius))
            .filter_map(|c| (c.points.len() >= 2).then(|| [c.points[0], c.points[c.points.len() - 1]]))
            .collect();
        g.center = *center;
        g.bounds = bounds(&g.curves);
        return g;
    }
    if let ViewKind::Overlay { values, .. } = &v.kind {
        return overlay_view(k, f, deflection, model, built, parent, values);
    }
    let outs = cut_outs(v, f, model);
    let mut cut: Vec<ShapeHandle> = Vec::new();
    let mut plane: Option<(Vec3, Vec3)> = None;
    let shapes: Vec<ShapeHandle> = match (&v.kind, parent_frame) {
        _ if !outs.is_empty() => {
            cut = cut_out(k, solids, f, model, &outs, &mut g);
            cut.clone()
        }
        (ViewKind::Section { a, b, flip, .. }, Some(pf)) => {
            let (_, point, look) = section_frame(pf, *a, *b, *flip);
            plane = Some((point, look));
            // A box filling the viewer's side of the plane removes it.
            let size = model.bbox.map_or(1000.0, |b| b.diagonal() * 4.0 + 10.0);
            // The box's own axes: Z along the look, X across.
            let bx = f.x();
            let by = look.cross(bx);
            let corner = point - look * size - bx * (size / 2.0) - by * (size / 2.0);
            let tool = Frame::new(corner, look, bx).and_then(|fr| k.make_box(&fr, Vec3::new(size, size, size)).ok());
            if let Some(tool) = tool {
                for s in solids {
                    if let Ok(op) = k.boolean(BoolOp::Cut, *s, &[tool.shape]) {
                        cut.push(op.shape);
                    }
                }
                k.release(tool.shape);
            }
            cut.clone()
        }
        _ => solids.to_vec(),
    };
    match k.project_edges(&shapes, f, deflection, v.hidden) {
        Ok(curves) => {
            let curves = if v.tangent { curves } else { curves.into_iter().filter(|c| c.kind != HlrKind::Smooth).collect() };
            g.curves = drop_covered(curves, deflection * 2.0);
        }
        Err(e) => g.error = Some(e.to_string()),
    }
    // Section faces: on the cut plane, facing the viewer; each solid hatched its own way.
    if let Some((point, look)) = plane {
        for (i, s) in cut.iter().enumerate() {
            let tris = faces_on(k, *s, f, point, look);
            let (angle, spacing) = part_hatch(i);
            g.hatch.extend(hatch_at(&tris, HATCH_SPACING * spacing / v.scale, angle));
        }
    }
    // The floor of each break-out: the same, within its boundary, less what was on that plane before
    // it was cut (the top of a step it comes level with is no cut face).
    for out in &outs {
        let Some(floor) = out.floor else { continue };
        let plane = f.z() * floor;
        let inside = Clip::inside(out.boundary);
        let mut was: Option<Vec<[Vec2; 3]>> = None;
        for (i, s) in cut.iter().enumerate() {
            let tris = faces_on(k, *s, f, plane, -f.z());
            if tris.is_empty() {
                continue;
            }
            let was = was.get_or_insert_with(|| solids.iter().flat_map(|u| faces_on(k, *u, f, plane, -f.z())).collect());
            let (angle, spacing) = part_hatch(i);
            let lines = hatch_at(&tris, HATCH_SPACING * spacing / v.scale, angle);
            let runs = lines.iter().flat_map(|l| inside.pieces(l));
            let cut_lines = runs.filter_map(|r| Some([*r.first()?, *r.last()?])).flat_map(|l| hatch_less(l, was, MESH_ROUNDING));
            g.hatch.extend(cut_lines);
        }
    }
    for s in cut {
        k.release(s);
    }
    g.center = model.bbox.map_or(Vec2::new(0.0, 0.0), |b| box_center(f, &b));
    // An auxiliary view lines up with its parent along the edge it looks square to: a model point
    // is as far along that edge in both, so the two middles are made as far along it too.
    if let (ViewKind::Auxiliary { edge, a, b, .. }, Some(p), Some(pf)) = (&v.kind, parent, parent_frame) {
        let line = auxiliary_line(model, pf, edge, *a, *b);
        if let Err(e) = &line {
            g.error = Some(format!("{e}: pick another edge, or delete the view"));
        }
        let (a, b) = line.unwrap_or((*a, *b));
        let u = (b - a).normalized();
        g.center += u * (p.center - g.center).dot(u);
    }
    g.bounds = bounds(&g.curves);
    g
}

/// Whether a face is flat, on the plane through `point` square to `look`, and faces against `look`.
fn on_plane(k: &dyn Kernel, face: FaceId, point: Vec3, look: Vec3) -> bool {
    match k.face_info(face).map(|info| info.surface) {
        Ok(SurfaceKind::Plane { normal, origin }) => normal.dot(-look) > 1.0 - 1e-6 && (origin - point).dot(look).abs() < 1e-6,
        _ => false,
    }
}

/// `s` with its faces on a floor's plane made one. Where a floor comes level with a face that was
/// there, the cut leaves a line between the two that is no edge of anything, and it would be drawn.
/// (If they will not join, as they are.)
fn floor_joined(k: &mut dyn Kernel, s: ShapeHandle, f: &Frame, floor: f64) -> ShapeHandle {
    let Ok(topo) = k.topology(s) else { return s };
    let faces: Vec<FaceId> = (0..topo.faces).map(|i| s.face(i)).filter(|face| on_plane(k, *face, f.z() * floor, -f.z())).collect();
    if faces.len() < 2 {
        return s;
    }
    match k.unify(s, &faces) {
        Ok(op) => {
            k.release(s);
            op.shape
        }
        Err(_) => s,
    }
}

/// The triangles (view coordinates) of a solid's flat faces on the plane through `point` square to
/// `look`, facing against `look`: what a cut leaves there to hatch.
fn faces_on(k: &mut dyn Kernel, s: ShapeHandle, f: &Frame, point: Vec3, look: Vec3) -> Vec<[Vec2; 3]> {
    let mut tris: Vec<[Vec2; 3]> = Vec::new();
    let Ok(mesh) = k.tessellate(s, &MeshTol::default()) else { return tris };
    let Ok(topo) = k.topology(s) else { return tris };
    for fi in 0..topo.faces {
        if !on_plane(k, s.face(fi), point, look) {
            continue;
        }
        for range in mesh.faces.iter().filter(|r| r.face == fi) {
            let idx = mesh.indices.get(range.first as usize..(range.first + range.count) as usize).unwrap_or(&[]);
            for t in idx.as_chunks::<3>().0 {
                let p = |i: u32| mesh.positions.get(i as usize).map(|q| project(f, Vec3::new(f64::from(q[0]), f64::from(q[1]), f64::from(q[2]))));
                if let (Some(a), Some(b), Some(c)) = (p(t[0]), p(t[1]), p(t[2])) {
                    tris.push([a, b, c]);
                }
            }
        }
    }
    tris
}

/// How far a model reaches along a view's Z, towards the eye and away from it.
fn z_range(f: &Frame, model: &ModelGeometry) -> Option<(f64, f64)> {
    let b = model.bbox?;
    let (l, h) = (b.min, b.max);
    let zs = (0..8)
        .map(|i| Vec3::new(if i & 1 == 0 { l.x } else { h.x }, if i & 2 == 0 { l.y } else { h.y }, if i & 4 == 0 { l.z } else { h.z }).dot(f.z()));
    Some(zs.fold((f64::MAX, f64::MIN), |(lo, hi), z| (lo.min(z), hi.max(z))))
}

/// How far a model truly reaches towards the eye along `z`: from its geometry, not its box, whose
/// corner may stand well clear of it when the view looks at it aslant. The ends of its edges are
/// exact; a curved face can stand beyond them, and then only its mesh says how far, to within the
/// mesh's tolerance.
fn nearest(model: &ModelGeometry, z: Vec3) -> Option<f64> {
    let mut near: Option<f64> = None;
    for inst in &model.instances {
        let Some(scene) = model.scenes.get(&inst.part) else { continue };
        let along = |p: Vec3| inst.frame.to_world(p).dot(z);
        for b in &scene.bodies {
            let at = b.at.unwrap_or(Frame::WORLD);
            let ends = b.ends.iter().flatten().map(|p| along(*p)).fold(f64::NEG_INFINITY, f64::max);
            let mesh = b
                .mesh
                .positions
                .iter()
                .map(|p| along(at.to_world(Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])))))
                .fold(f64::NEG_INFINITY, f64::max);
            let here = if mesh > ends + MESH_ROUNDING { mesh } else { ends };
            if here.is_finite() {
                near = Some(near.map_or(here, |n| n.max(here)));
            }
        }
    }
    near
}

/// A break-out at work on a view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CutOut {
    /// Its number among the view's modifiers.
    id: u32,
    pub(crate) boundary: Boundary,
    /// The height of its floor along the view's Z. None: it goes through the model.
    pub(crate) floor: Option<f64>,
}

/// A view's break-outs at work. Each goes down from the model's nearest point as the view sees it.
pub(crate) fn cut_outs(v: &View, f: &Frame, model: &ModelGeometry) -> Vec<CutOut> {
    let mut wanted = v
        .active_mods()
        .filter_map(|m| match &m.kind {
            ModKind::BreakOut { boundary, depth } => Some((m.id, *boundary, *depth)),
            _ => None,
        })
        .peekable();
    if wanted.peek().is_none() {
        return Vec::new();
    }
    let Some(near) = nearest(model, f.z()) else { return Vec::new() };
    wanted.map(|(id, boundary, depth)| CutOut { id, boundary, floor: depth.map(|d| near - d) }).collect()
}

/// Whether cutting `tool` from a solid, which left `left`, took anything away. By their volumes when
/// that is plain; when the two come to much the same it may be that the tool only touches the
/// solid or misses it, or that it takes little from much, so what the two share is asked for.
fn took_away(k: &mut dyn Kernel, solid: ShapeHandle, left: ShapeHandle, tool: ShapeHandle) -> bool {
    let volume = |k: &dyn Kernel, s: ShapeHandle| k.mass_properties(s, 1.0).map(|m| m.mass);
    let Ok(before) = volume(k, solid) else { return true };
    let after = volume(k, left).unwrap_or(0.0);
    if before - after > before * tol::MEASURE_REL {
        return true;
    }
    let Ok(shared) = k.boolean(BoolOp::Intersect, solid, &[tool]) else { return true };
    let some = volume(k, shared.shape).is_ok_and(|v| v > tol::CLASH_VOLUME);
    k.release(shared.shape);
    some
}

/// A prism along the view's Z over `boundary`, from `from` towards the eye for `height`.
fn prism(k: &mut dyn Kernel, f: &Frame, boundary: &Boundary, from: f64, height: f64) -> Option<ShapeHandle> {
    match boundary {
        Boundary::Rect { lo, hi } => {
            let corner = f.x() * lo.x + f.y() * lo.y + f.z() * from;
            let at = Frame::new(corner, f.z(), f.x())?;
            k.make_box(&at, Vec3::new(hi.x - lo.x, hi.y - lo.y, height)).ok().map(|op| op.shape)
        }
        Boundary::Circle { center, radius } => {
            let axis = Axis::new(f.x() * center.x + f.y() * center.y + f.z() * from, f.z())?;
            k.make_cylinder(&axis, *radius, height).ok().map(|op| op.shape)
        }
    }
}

/// `solids` with each break-out cut from them (new shapes, for the caller to release). What is cut
/// away whole is gone; a cut that fails leaves the solid as it was and says so on the view.
fn cut_out(k: &mut dyn Kernel, solids: &[ShapeHandle], f: &Frame, model: &ModelGeometry, outs: &[CutOut], g: &mut ViewGeometry) -> Vec<ShapeHandle> {
    let mut shapes: Vec<ShapeHandle> = solids.iter().filter_map(|s| k.duplicate(*s).ok()).collect();
    let Some((far, near)) = z_range(f, model) else { return shapes };
    let over = (near - far).abs() + 10.0;
    for out in outs {
        // (A depth past the far side of the model goes through it, no farther. How far the tool goes
        // comes from the model's box: it is enough to be clear of it all.)
        let from = out.floor.map_or(far - 1.0, |z| z.max(far - 1.0));
        let Some(tool) = prism(k, f, &out.boundary, from, near + over - from) else {
            g.error = Some(format!("Break Out {}: its boundary could not be made into a cut", out.id));
            continue;
        };
        let mut next = Vec::with_capacity(shapes.len());
        let mut cut_any = shapes.is_empty();
        for s in shapes {
            match k.boolean(BoolOp::Cut, s, &[tool]) {
                Ok(op) => {
                    cut_any = cut_any || took_away(k, s, op.shape, tool);
                    k.release(s);
                    let left = out.floor.map_or(op.shape, |z| floor_joined(k, op.shape, f, z));
                    if k.topology(left).is_ok_and(|t| t.faces > 0) {
                        next.push(left);
                    } else {
                        k.release(left);
                    }
                }
                Err(e) => {
                    cut_any = true;
                    g.error = Some(format!("Break Out {} could not be cut: {e}", out.id));
                    next.push(s);
                }
            }
        }
        if !cut_any {
            g.error = Some(format!(
                "Break Out {} cuts nothing: its boundary misses the part, or its floor is above it there: go deeper or move it",
                out.id
            ));
        }
        shapes = next;
        k.release(tool);
    }
    shapes
}

/// An overlay: the model in another position, seen in the frame of its parent, its visible lines
/// less those its parent draws already.
fn overlay_view(
    k: &mut dyn Kernel,
    f: &Frame,
    deflection: f64,
    model: &ModelGeometry,
    built: &Solids,
    parent: Option<&ViewGeometry>,
    values: &[JointValue],
) -> ViewGeometry {
    let mut g = ViewGeometry { frame: *f, ..ViewGeometry::default() };
    let Some(p) = parent else {
        g.error = Some("the parent view has no geometry".into());
        return g;
    };
    g.center = p.center;
    let frames = match model.position_frames(values) {
        Ok(x) => x,
        Err(e) => {
            g.error = Some(e);
            return g;
        }
    };
    let mut placed = Vec::new();
    for (c, shapes) in &built.parts {
        let Some(at) = frames.get(c) else { continue };
        placed.extend(shapes.iter().filter_map(|s| tenon_assembly::interfere::place(k, *s, at).ok()));
    }
    match k.project_edges(&placed, f, deflection, false) {
        Ok(curves) => {
            let curves: Vec<HlrCurve> = curves.into_iter().filter(|c| c.visible && c.kind != HlrKind::Smooth).collect();
            g.curves = drop_seen(curves, &p.curves, deflection * 2.0);
        }
        Err(e) => g.error = Some(e.to_string()),
    }
    for s in placed {
        k.release(s);
    }
    g.bounds = bounds(&g.curves);
    g
}

/// The file name of a model key (a path), for messages.
pub fn file_name(key: &str) -> &str {
    key.rsplit(['/', '\\']).next().unwrap_or(key)
}

/// Builds every model and every view of a drawing.
pub fn evaluate(k: &mut dyn Kernel, sources: &BTreeMap<String, ModelSource>, d: &Drawing) -> Evaluation {
    let mut ev = Evaluation::default();
    let mut solids: BTreeMap<String, Solids> = BTreeMap::new();
    for key in d.models() {
        match sources.get(&key) {
            Some(src) => {
                let overlaid = d.views.iter().any(|v| v.model == key && matches!(v.kind, ViewKind::Overlay { .. }));
                let (g, s) = build_model(k, &key, src, overlaid);
                ev.models.insert(key.clone(), g);
                solids.insert(key, s);
            }
            None => {
                ev.models
                    .insert(key.clone(), ModelGeometry { error: Some(format!("{} is not loaded", file_name(&key))), ..ModelGeometry::default() });
            }
        }
    }
    let frames = frames_in(d, &ev.models);
    for v in &d.views {
        let f = frames.get(&v.id).copied().unwrap_or(Frame::WORLD);
        let empty = Solids::default();
        let model_solids = solids.get(&v.model).unwrap_or(&empty);
        let parent = v.kind.parent();
        let g = match ev.models.get(&v.model) {
            // A model that is not there: the view is empty and says why.
            Some(ModelGeometry { error: Some(e), instances, .. }) if instances.is_empty() => {
                ViewGeometry { frame: f, error: Some(e.clone()), ..ViewGeometry::default() }
            }
            // A model that is there in part: the view shows what there is, and says what is not.
            Some(m) => {
                let mut g = compute_view(k, v, &f, model_solids, m, parent.and_then(|p| ev.views.get(&p)), parent.and_then(|p| frames.get(&p)));
                if g.error.is_none() {
                    g.error.clone_from(&m.error);
                }
                g
            }
            None => ViewGeometry { frame: f, error: Some("the model is not loaded".into()), ..ViewGeometry::default() },
        };
        ev.views.insert(v.id, g);
    }
    for s in solids.into_values() {
        s.release(k);
    }
    ev
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn frames_follow_the_projection_angle() {
        let front = base_frame(Orientation::Front);
        // Third-angle: the view to the right looks from the right (+X), the one above from above.
        let right = projected_frame(&front, Side::Right, true);
        assert!(right.z().near(Vec3::X, 1e-12) && right.y().near(Vec3::Z, 1e-12) && right.x().near(Vec3::Y, 1e-12));
        let above = projected_frame(&front, Side::Above, true);
        assert!(above.z().near(Vec3::Z, 1e-12) && above.x().near(Vec3::X, 1e-12) && above.y().near(Vec3::Y, 1e-12));
        // First-angle: the other way round.
        assert!(projected_frame(&front, Side::Right, false).z().near(-Vec3::X, 1e-12));
        assert!(projected_frame(&front, Side::Above, false).z().near(-Vec3::Z, 1e-12));
        // Diagonal: an isometric view, up still mostly up.
        let iso = projected_frame(&front, Side::AboveRight, true);
        assert!(iso.z().near(Vec3::new(1.0, -1.0, 1.0).normalized(), 1e-12));
        assert!(iso.y().z > 0.7);
        assert!(base_frame(Orientation::Iso).z().near(iso.z(), 1e-12));
        // A horizontal section line in the front view, seen downwards: like a top view.
        let (s, point, look) = section_frame(&front, Vec2::new(0.0, 5.0), Vec2::new(10.0, 5.0), false);
        assert!(s.z().near(Vec3::Z, 1e-12) && s.x().near(Vec3::X, 1e-12) && s.y().near(Vec3::Y, 1e-12));
        assert!(point.near(Vec3::new(0.0, 0.0, 5.0), 1e-12) && look.near(-Vec3::Z, 1e-12));
        // In a top view, a horizontal line seen upwards (towards the back) gives a section like
        // the front view, upright; a vertical one seen leftwards, like the right view.
        let top = base_frame(Orientation::Top);
        let (s, _, _) = section_frame(&top, Vec2::new(0.0, 40.0), Vec2::new(100.0, 40.0), true);
        let fr = base_frame(Orientation::Front);
        assert!(s.z().near(fr.z(), 1e-12) && s.x().near(fr.x(), 1e-12) && s.y().near(fr.y(), 1e-12), "{s:?}");
        let (s, _, _) = section_frame(&front, Vec2::new(5.0, -10.0), Vec2::new(5.0, 50.0), true);
        let right = base_frame(Orientation::Right);
        assert!(s.z().near(right.z(), 1e-12) && s.y().near(right.y(), 1e-12), "{s:?}");
        // The same as projecting to the eye's side.
        assert!(s.x().near(projected_frame(&front, Side::Right, true).x(), 1e-12));
    }

    #[test]
    fn hatching_clipping_and_covered_hidden_lines() {
        // A 10 x 10 square in two triangles, hatched 1 apart: 13 or 14 lines, each inside.
        let sq =
            [[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)], [Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]];
        let h = hatch(&sq, 1.0);
        assert!((13..=15).contains(&h.len()), "{}", h.len());
        let total: f64 = h.iter().map(|s| s[0].dist(s[1])).sum();
        // Hatch length is the area over the spacing.
        assert!((total - 100.0).abs() < 1.0, "{total}");
        for s in &h {
            for p in s {
                assert!(p.x > -1e-9 && p.x < 10.0 + 1e-9 && p.y > -1e-9 && p.y < 10.0 + 1e-9);
            }
        }
        // A line through a circle of radius 2 keeps its middle 4.
        let line = HlrCurve { kind: HlrKind::Sharp, visible: true, points: vec![Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0)] };
        let c = clip_to_circle(std::slice::from_ref(&line), Vec2::new(0.0, 0.0), 2.0);
        assert_eq!(c.len(), 1);
        assert!(c[0].points[0].near(Vec2::new(-2.0, 0.0), 1e-9) && c[0].points[1].near(Vec2::new(2.0, 0.0), 1e-9));
        // A hidden line behind the visible one goes; one beside it stays.
        let behind = HlrCurve { kind: HlrKind::Sharp, visible: false, points: vec![Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0)] };
        let beside = HlrCurve { kind: HlrKind::Sharp, visible: false, points: vec![Vec2::new(-5.0, 1.0), Vec2::new(5.0, 1.0)] };
        let out = drop_covered(vec![line, behind, beside], 1e-6);
        assert_eq!(out.len(), 2);
        assert!(out.iter().any(|c| !c.visible && (c.points[0].y - 1.0).abs() < 1e-12));
    }
}
