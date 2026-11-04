//! 3D sketch mode (DEC-136): a sketch on no plane, drawn and held in the view. Every edit is a
//! registry command (`sketch3d.*`), as a 2D sketch's are, so the UI and the CLI share one path.
//!
//! A point comes from a click: on a point of the sketch, a vertex of the part, a work point or the
//! origin (those follow the part), on a face of the part (where the pointer is on it), or
//! elsewhere (in the plane through the last point that faces the viewer; near an axis through that
//! point, on that axis, and the line is held parallel to it). Or it is typed: X, Y, Z.

use std::collections::BTreeSet;

use egui::{Align2, Color32, Pos2, Rect, Shape, Stroke, Ui, vec2};
use serde_json::{Value, json};
use tenon_geom::Vec3;
use tenon_model::{EdgeRef, FeatureId, FeatureKind};
use tenon_render::pick::pick_edge_in;
use tenon_sketch::{Axis3, Constraint3, ConstraintId, EntityId, Geometry3, Sketch3d, Through};

use crate::icons::Icon;
use crate::panels::{Panel, ValueFor, ValuePanel};
use crate::theme::{self, Tokens};
use crate::workbench::{Mode, Workbench};

/// A 3D sketch's own colour, in the view and in the browser's sense of what it is.
pub(crate) fn colour(t: &Tokens) -> Color32 {
    if t.dark { Color32::from_rgb(64, 208, 196) } else { Color32::from_rgb(0, 128, 118) }
}

/// The same when the constraints hold it whole: as a 2D sketch goes from one colour to another.
fn colour_done(t: &Tokens) -> Color32 {
    if t.dark { Color32::from_rgb(190, 245, 240) } else { Color32::from_rgb(0, 72, 66) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool3 {
    Select,
    Line,
    Spline,
    Point,
    Bend,
    Include,
    Dimension,
    Coordinate(Axis3),
    Coincident,
    Fix,
    Along(Axis3),
    Parallel,
    Perpendicular,
    Equal,
    Tangent,
}

const TOOLS: &[(&str, Tool3)] = &[
    ("sketch3d.line", Tool3::Line),
    ("sketch3d.spline", Tool3::Spline),
    ("sketch3d.point", Tool3::Point),
    ("sketch3d.bend", Tool3::Bend),
    ("sketch3d.include", Tool3::Include),
    ("sketch3d.dimension", Tool3::Dimension),
    ("sketch3d.coord_x", Tool3::Coordinate(Axis3::X)),
    ("sketch3d.coord_y", Tool3::Coordinate(Axis3::Y)),
    ("sketch3d.coord_z", Tool3::Coordinate(Axis3::Z)),
    ("sketch3d.coincident", Tool3::Coincident),
    ("sketch3d.fix", Tool3::Fix),
    ("sketch3d.along_x", Tool3::Along(Axis3::X)),
    ("sketch3d.along_y", Tool3::Along(Axis3::Y)),
    ("sketch3d.along_z", Tool3::Along(Axis3::Z)),
    ("sketch3d.parallel", Tool3::Parallel),
    ("sketch3d.perpendicular", Tool3::Perpendicular),
    ("sketch3d.equal", Tool3::Equal),
    ("sketch3d.tangent", Tool3::Tangent),
];

impl Tool3 {
    fn id(self) -> Option<&'static str> {
        TOOLS.iter().find(|(_, t)| *t == self).map(|(id, _)| *id)
    }
    fn hint(self) -> &'static str {
        match self {
            Tool3::Select => "Click to select (a curve's length is said here); drag a point to move it; Delete removes the selection.",
            Tool3::Line => {
                "Click points to draw lines: on a vertex, a work point or a point of the sketch, or type X, Y, Z. Near an axis it stays on the axis. Esc ends the chain."
            }
            Tool3::Spline => "Click the points it goes through, or type X, Y, Z. Enter makes the spline; Esc leaves it.",
            Tool3::Point => "Click where a point goes: on a vertex or a work point (it follows the part), or type X, Y, Z. Esc ends.",
            Tool3::Bend => "Click a corner where two lines meet, then say the radius.",
            Tool3::Include => "Click edges of the part to bring them in: they follow the part. Esc ends.",
            Tool3::Dimension => "Click a line for its length, a bend for its radius, or two points for how far apart they are.",
            Tool3::Coordinate(_) => "Click a point, then say where it is along this axis.",
            Tool3::Coincident => "Click two points.",
            Tool3::Fix => "Click a point that must not move.",
            Tool3::Along(_) => "Click a line to hold it parallel to this axis.",
            Tool3::Parallel | Tool3::Perpendicular => "Click two lines.",
            Tool3::Equal => "Click two lines, or two bends.",
            Tool3::Tangent => "Click two curves that meet at an end (a line and a bend or a spline, or two of those).",
        }
    }
    /// The geometry it picks, and how many.
    fn wants(self) -> (Wants, usize) {
        match self {
            Tool3::Coincident => (Wants::Point, 2),
            Tool3::Fix | Tool3::Coordinate(_) => (Wants::Point, 1),
            Tool3::Along(_) => (Wants::Line, 1),
            Tool3::Parallel | Tool3::Perpendicular => (Wants::Line, 2),
            Tool3::Equal => (Wants::LineOrArc, 2),
            Tool3::Tangent => (Wants::Curve, 2),
            _ => (Wants::Any, 0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wants {
    Any,
    Point,
    Line,
    LineOrArc,
    Curve,
}

/// What a click on a place stands for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    /// A point of the sketch.
    Existing(EntityId),
    /// A vertex of the part: which end of the edge, and which way it lies from the other.
    Vertex {
        edge: EdgeRef,
        end: u8,
        towards: Option<Vec3>,
    },
    Work(FeatureId),
    Origin,
    /// Just a place.
    Free,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Click3 {
    pub at: Vec3,
    pub target: Target,
    /// Held to this axis from the click before it.
    pub axis: Option<Axis3>,
}

impl Click3 {
    /// As a command takes a point.
    fn spec(&self) -> Value {
        let at = json!([self.at.x, self.at.y, self.at.z]);
        match &self.target {
            Target::Existing(id) => json!(id.0),
            Target::Vertex { edge, end, towards } => {
                let mut spec = json!({ "vertex": edge, "end": end, "at": at });
                if let Some(t) = towards {
                    spec["towards"] = json!([t.x, t.y, t.z]);
                }
                spec
            }
            Target::Work(id) => json!({ "work": id.0, "at": at }),
            Target::Origin => json!("origin"),
            Target::Free => at,
        }
    }
}

/// State of 3D sketch mode.
pub(crate) struct Sketch3dMode {
    pub feature: FeatureId,
    pub tool: Tool3,
    pub clicks: Vec<Click3>,
    pub picks: Vec<EntityId>,
    pub selection: Vec<EntityId>,
    /// Constraints selected by clicking their symbols (Delete removes them).
    pub constraints: Vec<ConstraintId>,
    pub hover: Option<EntityId>,
    /// What a click would be now, in a tool that places points.
    pub under: Option<Click3>,
    /// The point being dragged, the sketch as it would be, and the plane it moves in: through the
    /// point as it was, facing the viewer.
    pub drag: Option<(EntityId, Sketch3d, Vec3)>,
    /// What the coordinates' box says.
    pub xyz: [f64; 3],
    /// The radius the last bend had.
    pub radius: f64,
    /// The sketch as it was when opened. Until Finish the part is regenerated with this, so
    /// drawing never waits for the features that use the sketch.
    pub base: Option<Sketch3d>,
    /// The features that use it, as they were when opened: an edit can change what they name.
    users: Vec<(FeatureId, FeatureKind)>,
    dof: Option<(u64, Option<usize>, BTreeSet<EntityId>)>,
    /// Constraint symbols and dimension values as last drawn, and where.
    badges: Vec<(ConstraintId, Rect)>,
    dims: Vec<(ConstraintId, Rect)>,
}

impl Sketch3dMode {
    fn new(feature: FeatureId, base: Option<Sketch3d>, users: Vec<(FeatureId, FeatureKind)>) -> Self {
        Sketch3dMode {
            feature,
            tool: Tool3::Select,
            clicks: Vec::new(),
            picks: Vec::new(),
            selection: Vec::new(),
            constraints: Vec::new(),
            hover: None,
            under: None,
            drag: None,
            xyz: [0.0; 3],
            radius: 5.0,
            base,
            users,
            dof: None,
            badges: Vec::new(),
            dims: Vec::new(),
        }
    }

    /// `doc` with the sketch, what ties it to the part and what uses it as they were when it was
    /// opened: what the part is regenerated from until Finish.
    pub(crate) fn as_opened(&self, doc: &mut tenon_model::Document) {
        let Some(base) = &self.base else { return };
        if let Some(sk) = doc.sketch3d_mut(self.feature) {
            *sk = base.clone();
        }
        // (What was brought in since is no part of the sketch as it was: nothing is lost of it.)
        if let Some(FeatureKind::Sketch3d { links, .. }) = doc.feature_mut(self.feature).map(|f| &mut f.kind) {
            links.retain(|l| base.is_reference(l.entity));
        }
        for (id, kind) in &self.users {
            if let Some(f) = doc.feature_mut(*id) {
                f.kind = kind.clone();
            }
        }
    }

    /// Where the value of a dimension was last drawn (tests).
    #[cfg(test)]
    pub(crate) fn dim_of(&self, c: ConstraintId) -> Option<Pos2> {
        self.dims.iter().find(|b| b.0 == c).map(|b| b.1.center())
    }

    /// Where the symbol of a constraint was last drawn (tests).
    #[cfg(test)]
    pub(crate) fn badges_of(&self, c: ConstraintId) -> Option<Pos2> {
        self.badges.iter().find(|b| b.0 == c).map(|b| b.1.center())
    }
}

/// A dimension's value as it reads: "40", "R6", "X 12".
fn dimension_text(c: &Constraint3) -> Option<String> {
    let v = crate::sketcher::fmt_len(c.value()?);
    Some(match c {
        Constraint3::Radius { .. } => format!("R{v}"),
        Constraint3::Coordinate { axis, .. } => format!("{} {v}", axis.name()),
        _ => v,
    })
}

/// The picture a constraint is shown as, or its letter, and the entities it is shown beside.
fn glyph(c: &Constraint3) -> Option<(Result<Icon, &'static str>, Vec<EntityId>)> {
    use Constraint3::*;
    Some(match c {
        Coincident { a, .. } => (Ok(Icon::Coincident), vec![*a]),
        Fix { point } => (Ok(Icon::Fix), vec![*point]),
        Along { line, axis } => (Err(axis.name()), vec![*line]),
        Parallel { a, b } => (Ok(Icon::Parallel), vec![*a, *b]),
        Perpendicular { a, b } => (Ok(Icon::Perpendicular), vec![*a, *b]),
        Equal { a, b } => (Ok(Icon::Equal), vec![*a, *b]),
        Tangent { a, b } => (Ok(Icon::Tangent), vec![*a, *b]),
        _ => return None,
    })
}

/// About the middle of an entity.
fn middle_of(s: &Sketch3d, id: EntityId) -> Option<Vec3> {
    let line = s.polyline(id);
    match line.as_slice() {
        [] => None,
        [one] => Some(*one),
        [a, b] => Some(a.lerp(*b, 0.5)),
        many => many.get(many.len() / 2).copied(),
    }
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t).distance(p)
}

impl Workbench {
    fn screen3(&self, p: Vec3, rect: Rect) -> Option<Pos2> {
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        self.view.camera.project(p, w, h).map(|(x, y, _)| rect.min + vec2(x as f32, y as f32))
    }

    /// Start 3D Sketch: a new one, and into it.
    pub(crate) fn new_sketch3d(&mut self) -> Result<(), String> {
        self.finish_sketch3d();
        if matches!(self.mode, Mode::Sketch(_)) {
            self.finish_sketch();
        }
        let r = self.exec("sketch.new_3d", json!({}))?;
        let id = r["feature"].as_u64().and_then(|v| u32::try_from(v).ok()).ok_or("internal: no feature id")?;
        self.panel = None;
        self.pick_plane = false;
        self.enter_sketch3d(FeatureId(id))
    }

    /// Opens a 3D sketch for editing.
    pub(crate) fn enter_sketch3d(&mut self, feature: FeatureId) -> Result<(), String> {
        let Some(base) = self.document().sketch3d(feature).cloned() else {
            return Err(format!("{feature} is not a 3D sketch"));
        };
        self.panel = None;
        let drawn = !base.is_empty();
        let users = self.document().features().iter().filter(|f| f.kind.depends_on().contains(&feature)).map(|f| (f.id, f.kind.clone())).collect();
        self.mode = Mode::Sketch3d(Box::new(Sketch3dMode::new(feature, Some(base), users)));
        self.chrome.tab = crate::commands::SKETCH3D_TAB;
        // (It is on no plane: the view stays as it was, and takes in what is drawn.)
        if drawn {
            self.zoom_all();
        }
        self.set_status(format!("Editing {}. {}", self.feature_name(feature), Tool3::Select.hint()));
        Ok(())
    }

    /// Leaves the 3D sketch, if one is open.
    pub(crate) fn finish_sketch3d(&mut self) {
        if let Mode::Sketch3d(s) = &self.mode {
            let name = self.feature_name(s.feature);
            self.mode = Mode::Model;
            self.chrome.tab = self.home_tab();
            self.view.hover = None;
            self.set_status(format!("Finished {name}. Sweep a profile along it from the 3D Model tab."));
        }
    }

    pub(crate) fn is_sketching3d(&self) -> bool {
        matches!(self.mode, Mode::Sketch3d(_))
    }

    /// The id of the tool running in the 3D sketch.
    pub(crate) fn active_tool3(&self) -> Option<&'static str> {
        match &self.mode {
            Mode::Sketch3d(s) => s.tool.id(),
            _ => None,
        }
    }

    /// A button of the 3D Sketch tab.
    pub(crate) fn sketch3d_tool(&mut self, id: &str) -> Result<(), String> {
        let Mode::Sketch3d(s) = &mut self.mode else {
            return Err("that works in a 3D sketch: Start 3D Sketch first".into());
        };
        if id == "sketch3d.finish" {
            self.finish_sketch3d();
            return Ok(());
        }
        if id == "sketch3d.construction" {
            let (feature, picked) = (s.feature, s.selection.clone());
            if picked.is_empty() {
                return Err("select the geometry to change first".into());
            }
            let sk = self.document().sketch3d(feature).ok_or("the sketch is gone")?;
            let on = !picked.iter().all(|e| sk.entity(*e).is_some_and(|x| x.construction));
            for e in picked {
                self.exec("sketch3d.construction", json!({ "sketch": feature.0, "entity": e.0, "on": on }))?;
            }
            self.set_status(if on { "Construction geometry (no part of a path unless it is chosen)." } else { "Normal geometry." });
            return Ok(());
        }
        if id == "sketch3d.break_link" {
            let (feature, picked) = (s.feature, s.selection.clone());
            let sk = self.document().sketch3d(feature).ok_or("the sketch is gone")?;
            let linked: Vec<u32> = picked.iter().filter(|e| sk.is_reference(**e)).map(|e| e.0).collect();
            if linked.is_empty() {
                return Err("select what follows the part (it is drawn in amber) to break its link".into());
            }
            self.exec("sketch3d.break_link", json!({ "sketch": feature.0, "entities": linked }))?;
            self.set_status("Link broken: it is the sketch's own now, where it was, and no longer follows the part.");
            return Ok(());
        }
        let tool = TOOLS.iter().find(|(t, _)| *t == id).map(|(_, t)| *t).ok_or_else(|| format!("unknown command `{id}`"))?;
        s.tool = if s.tool == tool { Tool3::Select } else { tool };
        s.clicks.clear();
        s.picks.clear();
        let hint = s.tool.hint();
        self.set_status(hint);
        Ok(())
    }

    /// Esc, or Cancel from the radial menu: the tool's clicks go, then the tool.
    pub(crate) fn sketch3d_escape(&mut self) {
        let Mode::Sketch3d(s) = &mut self.mode else { return };
        if !s.clicks.is_empty() || !s.picks.is_empty() {
            s.clicks.clear();
            s.picks.clear();
            let hint = s.tool.hint();
            self.set_status(hint);
        } else {
            s.tool = Tool3::Select;
            s.selection.clear();
            s.constraints.clear();
            self.view.hover = None;
            self.set_status("Ready");
        }
    }

    /// What the status bar says of a tool once what it was asked for is done: its hint.
    pub(crate) fn sketch3d_hint(&mut self) {
        if let Mode::Sketch3d(s) = &self.mode {
            let hint = s.tool.hint();
            self.set_status(hint);
        }
    }

    pub(crate) fn sketch3d_dof_text(&mut self) -> Option<String> {
        let rev = self.session.revision();
        let Mode::Sketch3d(s) = &mut self.mode else { return None };
        if self.session.document().sketch3d(s.feature).is_some_and(Sketch3d::is_empty) {
            return Some("Nothing drawn yet".into());
        }
        if s.dof.as_ref().is_none_or(|(r, _, _)| *r != rev) {
            let d = self.session.document().sketch3d(s.feature).map(Sketch3d::dof);
            s.dof = Some(match d {
                Some(Ok(d)) => (rev, Some(d.dof), d.fully_constrained),
                _ => (rev, None, BTreeSet::new()),
            });
        }
        match s.dof.as_ref() {
            Some((_, Some(0), _)) => Some("Fully Constrained".into()),
            Some((_, Some(1), _)) => Some("1 degree of freedom".into()),
            Some((_, Some(n), _)) => Some(format!("{n} degrees of freedom")),
            _ => Some("Over-constrained or inconsistent".into()),
        }
    }

    /// The 3D sketches that show when none is being worked in, each with whether the open panel
    /// uses it: as [`Workbench::shown_sketches`] has it of sketches.
    pub(crate) fn shown_sketches3d(&self) -> Vec<(FeatureId, bool)> {
        let doc = self.document();
        let live: Vec<&tenon_model::Feature> = doc.features().iter().take(doc.end_of_part()).filter(|f| !f.suppressed).collect();
        let used: BTreeSet<FeatureId> = live.iter().flat_map(|f| f.kind.depends_on()).collect();
        let chosen: Option<FeatureId> = match &self.panel {
            Some(Panel::Sweep(p)) => p.path.as_ref().map(|c| c.sketch),
            Some(Panel::Work(w)) => w.curve.map(|c| c.0),
            _ => None,
        };
        // (Picking a point or a line of one, or measuring, all that are not hidden by hand show.)
        let picking = matches!(&self.panel, Some(Panel::Work(_) | Panel::Measure(_) | Panel::Sweep(_)));
        live.iter()
            .filter(|f| matches!(f.kind, FeatureKind::Sketch3d { .. }))
            .filter(|f| chosen == Some(f.id) || f.visible.unwrap_or(picking || !used.contains(&f.id)))
            .map(|f| (f.id, chosen == Some(f.id)))
            .collect()
    }

    /// What the 3D sketches shown take up, the open one with them.
    pub(crate) fn sketch3d_box(&self) -> Option<tenon_geom::Aabb3> {
        let open = match &self.mode {
            Mode::Sketch3d(s) => Some((s.feature, false)),
            _ => None,
        };
        let doc = self.document();
        tenon_geom::Aabb3::from_points(
            self.shown_sketches3d().into_iter().chain(open).filter_map(|(id, _)| doc.sketch3d(id)).flat_map(Sketch3d::extent),
        )
    }

    /// Frames what there is with `at` in the view: a typed point can be anywhere.
    fn frame_with(&mut self, at: Vec3) {
        let Mode::Sketch3d(sm) = &self.mode else { return };
        let extra = sm.clicks.iter().map(|c| c.at).chain([at]);
        let Some(mut b) = tenon_geom::Aabb3::from_points(extra) else { return };
        if let Some(there) = self.scene_box() {
            b = b.union(&there);
        }
        let mut to = self.view.anim.map_or(self.view.camera, |a| a.to);
        // (A point alone has no size to fit: it goes in the middle.)
        if b.diagonal() > 1e-6 {
            to.fit(&b);
        } else {
            to.target = at;
        }
        self.animate_to(to);
    }

    /// Draws the 3D sketches that show, but for the one being worked in.
    pub(crate) fn sketch3d_wires(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        let doc = self.document();
        let painter = ui.painter().with_clip_rect(rect);
        let open = match &self.mode {
            Mode::Sketch3d(s) => Some(s.feature),
            _ => None,
        };
        let own = colour(t);
        for (id, on) in self.shown_sketches3d() {
            if open == Some(id) {
                continue;
            }
            let Some(sk) = doc.sketch3d(id) else { continue };
            let stroke = if on { Stroke::new(2.4, own) } else { Stroke::new(1.5, own.gamma_multiply(0.85)) };
            for (e, entity) in sk.entities() {
                if entity.geometry.is_point() {
                    continue;
                }
                let pts: Vec<Pos2> = sk.polyline(e).into_iter().filter_map(|q| self.screen3(q, rect)).collect();
                if pts.len() < 2 {
                    continue;
                }
                if entity.construction || entity.reference {
                    painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, stroke.color), 6.0, 4.0));
                } else {
                    painter.add(Shape::line(pts, stroke));
                }
            }
        }
    }

    /// The curve of a 3D sketch shown under a screen point (the open one too).
    pub(crate) fn sketch3d_curve_at(&self, p: Pos2, rect: Rect) -> Option<(FeatureId, EntityId)> {
        let mut best: Option<(f32, FeatureId, EntityId)> = None;
        let open = match &self.mode {
            Mode::Sketch3d(s) => Some((s.feature, false)),
            _ => None,
        };
        for (id, _) in self.shown_sketches3d().into_iter().chain(open) {
            let Some(sk) = self.document().sketch3d(id) else { continue };
            for (e, entity) in sk.entities() {
                if !entity.geometry.is_curve() {
                    continue;
                }
                let pts: Vec<Pos2> = sk.polyline(e).into_iter().filter_map(|q| self.screen3(q, rect)).collect();
                for w in pts.windows(2) {
                    let d = seg_dist(p, w[0], w[1]);
                    if d <= 6.0 && best.is_none_or(|b| d < b.0) {
                        best = Some((d, id, e));
                    }
                }
            }
        }
        best.map(|(_, id, e)| (id, e))
    }

    /// The point of a 3D sketch shown under a screen point.
    pub(crate) fn sketch3d_point_at(&self, p: Pos2, rect: Rect) -> Option<(FeatureId, EntityId)> {
        let mut best: Option<(f32, FeatureId, EntityId)> = None;
        for (id, _) in self.shown_sketches3d() {
            let Some(sk) = self.document().sketch3d(id) else { continue };
            for (e, entity) in sk.entities() {
                let Geometry3::Point { pos } = entity.geometry else { continue };
                let Some(s) = self.screen3(pos, rect) else { continue };
                let d = s.distance(p);
                if d <= 8.0 && best.is_none_or(|b| d < b.0) {
                    best = Some((d, id, e));
                }
            }
        }
        best.map(|(_, id, e)| (id, e))
    }

    /// The vertex of the part under a screen point, not behind a face: an end of an edge that can
    /// be named, which way it lies from the edge's other end, and where it is.
    pub(crate) fn vertex_at(&self, p: Pos2, rect: Rect) -> Option<(EdgeRef, u8, Option<Vec3>, Vec3)> {
        let (o, d) =
            self.view.camera.ray(f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
        let in_front = self.point_under(p, rect).map(|q| (q - o).dot(d));
        let mut best: Option<(f32, usize, usize, u8, Option<Vec3>, Vec3)> = None;
        for (b, body) in self.scene.bodies.iter().enumerate() {
            for (e, ends) in body.ends.iter().enumerate() {
                for (i, at) in ends.iter().enumerate() {
                    let Some(s) = self.screen3(*at, rect) else { continue };
                    let dist = s.distance(p);
                    if dist > 9.0 || best.as_ref().is_some_and(|x| dist >= x.0) {
                        continue;
                    }
                    // (A little in front of what the pointer is over counts: it is on that face.)
                    let along = (*at - o).dot(d);
                    if in_front.is_some_and(|f| along > f + 1e-3 * (1.0 + f.abs())) {
                        continue;
                    }
                    if body.edge_ref(u32::try_from(e).ok()?).is_some() {
                        let towards = Some((*at - ends[1 - i]).normalized()).filter(|t| t.len() > 0.5);
                        best = Some((dist, b, e, u8::from(i != 0), towards, *at));
                    }
                }
            }
        }
        let (_, b, e, end, towards, at) = best?;
        Some((self.scene.bodies.get(b)?.edge_ref(u32::try_from(e).ok()?)?, end, towards, at))
    }

    /// The nearest place on an axis through `from` to the pointer, within a few pixels on the
    /// screen: what a line drawn from `from` is held to.
    fn axis_snap(&self, from: Vec3, p: Pos2, rect: Rect) -> Option<(Vec3, Axis3)> {
        let (o, d) =
            self.view.camera.ray(f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
        let w = from - o;
        let mut best: Option<(f32, Vec3, Axis3)> = None;
        for axis in Axis3::ALL {
            let a = axis.dir();
            let b = a.dot(d);
            let den = 1.0 - b * b;
            if den < 1e-6 {
                continue;
            }
            let t = (b * w.dot(d) - w.dot(a)) / den;
            let at = from + a * t;
            let Some(s) = self.screen3(at, rect) else { continue };
            let dist = s.distance(p);
            if dist <= 8.0 && best.as_ref().is_none_or(|x| dist < x.0) {
                best = Some((dist, at, axis));
            }
        }
        best.map(|(_, at, axis)| (at, axis))
    }

    /// What a click at `p` would place: see the module's text.
    fn place_at(&self, s: &Sketch3d, sm: &Sketch3dMode, p: Pos2, rect: Rect) -> Option<Click3> {
        if let Some((_, id)) = self.own_point_at(s, p, rect)
            && let Some(at) = s.point(id)
        {
            return Some(Click3 { at, target: Target::Existing(id), axis: None });
        }
        if let Some((edge, end, towards, at)) = self.vertex_at(p, rect) {
            return Some(Click3 { at, target: Target::Vertex { edge, end, towards }, axis: None });
        }
        if let Some(id) = self.work_point_at(p, rect)
            && let Some((_, tenon_model::WorkGeom::Point(at))) = self.scene.work.iter().find(|(w, _)| *w == id)
        {
            return Some(Click3 { at: *at, target: Target::Work(id), axis: None });
        }
        if self.screen3(Vec3::ZERO, rect).is_some_and(|o| o.distance(p) <= 8.0) {
            return Some(Click3 { at: Vec3::ZERO, target: Target::Origin, axis: None });
        }
        let last = sm.clicks.last().map(|c| c.at);
        // A line goes along an axis when it is near one, even over the part.
        if sm.tool == Tool3::Line
            && let Some(from) = last
            && let Some((at, axis)) = self.axis_snap(from, p, rect)
        {
            return Some(Click3 { at, target: Target::Free, axis: Some(axis) });
        }
        if let Some(at) = self.point_under(p, rect) {
            return Some(Click3 { at, target: Target::Free, axis: None });
        }
        // In the plane through the last point (else the origin) that faces the viewer.
        let (o, d) =
            self.view.camera.ray(f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
        let from = last.unwrap_or(Vec3::ZERO);
        let at = o + d * ((from - o).dot(d) / d.dot(d));
        at.is_finite().then_some(Click3 { at, target: Target::Free, axis: None })
    }

    fn own_point_at(&self, s: &Sketch3d, p: Pos2, rect: Rect) -> Option<(f32, EntityId)> {
        let mut best: Option<(f32, EntityId)> = None;
        for (id, e) in s.entities() {
            let Geometry3::Point { pos } = e.geometry else { continue };
            let Some(q) = self.screen3(pos, rect) else { continue };
            let d = q.distance(p);
            if d < 9.0 && best.is_none_or(|b| d < b.0) {
                best = Some((d, id));
            }
        }
        best
    }

    /// Nearest entity under the pointer: points first, then curves.
    fn hit3(&self, s: &Sketch3d, p: Pos2, rect: Rect) -> Option<EntityId> {
        if let Some((_, id)) = self.own_point_at(s, p, rect) {
            return Some(id);
        }
        let mut best: Option<(f32, EntityId)> = None;
        for (id, e) in s.entities() {
            if !e.geometry.is_curve() {
                continue;
            }
            let pts: Vec<Pos2> = s.polyline(id).into_iter().filter_map(|q| self.screen3(q, rect)).collect();
            for w in pts.windows(2) {
                let d = seg_dist(p, w[0], w[1]);
                if d < 6.0 && best.is_none_or(|b| d < b.0) {
                    best = Some((d, id));
                }
            }
        }
        best.map(|(_, id)| id)
    }

    /// A place clicked, or typed, in a tool that places points.
    fn place3(&mut self, feature: FeatureId, click: Click3) {
        let Mode::Sketch3d(sm) = &mut self.mode else { return };
        match sm.tool {
            Tool3::Point => {
                if matches!(click.target, Target::Existing(_)) {
                    self.set_status("There is a point there already.");
                    return;
                }
                if self.exec_status("sketch3d.point", json!({ "sketch": feature.0, "at": click.spec() })).is_some() {
                    let hint = Tool3::Point.hint();
                    self.set_status(hint);
                }
            }
            Tool3::Spline => {
                sm.clicks.push(click);
                let n = sm.clicks.len();
                self.set_status(format!(
                    "{n} point{} of the spline. Click the next, or Enter to make it; Esc leaves it.",
                    if n == 1 { "" } else { "s" }
                ));
            }
            Tool3::Line => {
                let Some(first) = sm.clicks.last().cloned() else {
                    sm.clicks.push(click);
                    return;
                };
                let mut p = json!({ "sketch": feature.0, "start": first.spec(), "end": click.spec() });
                if let Some(axis) = click.axis {
                    p["along"] = json!(axis.name().to_lowercase());
                }
                if let Some(r) = self.exec_status("sketch3d.line", p)
                    && let Mode::Sketch3d(sm) = &mut self.mode
                    && let Some(end) = r["end"].as_u64().and_then(|v| u32::try_from(v).ok())
                {
                    // The chain goes on from the end of this one.
                    let at = self.session.document().sketch3d(feature).and_then(|s| s.point(EntityId(end))).unwrap_or(click.at);
                    sm.clicks = vec![Click3 { at, target: Target::Existing(EntityId(end)), axis: None }];
                }
            }
            _ => {}
        }
    }

    /// The axis a line typed to `at` goes along from the last point, as one clicked near an axis is
    /// held to it: a path typed axis by axis stays so when it is dimensioned.
    fn typed_along(&self, at: Vec3) -> Option<Axis3> {
        let Mode::Sketch3d(sm) = &self.mode else { return None };
        if sm.tool != Tool3::Line {
            return None;
        }
        let way = at - sm.clicks.last()?.at;
        let off = |v: f64| v.abs() > tenon_geom::tol::LINEAR;
        match (off(way.x), off(way.y), off(way.z)) {
            (true, false, false) => Some(Axis3::X),
            (false, true, false) => Some(Axis3::Y),
            (false, false, true) => Some(Axis3::Z),
            _ => None,
        }
    }

    /// Makes the spline clicked out so far.
    fn finish_spline3(&mut self, feature: FeatureId) {
        let Mode::Sketch3d(sm) = &mut self.mode else { return };
        if sm.tool != Tool3::Spline {
            return;
        }
        if sm.clicks.len() < 2 {
            self.set_error("A spline goes through two points or more: click another first.");
            return;
        }
        let points: Vec<Value> = sm.clicks.iter().map(Click3::spec).collect();
        if self.exec_status("sketch3d.spline", json!({ "sketch": feature.0, "points": points })).is_some()
            && let Mode::Sketch3d(sm) = &mut self.mode
        {
            sm.clicks.clear();
            self.set_status("Made the spline. Click the points of another; Esc ends.");
        }
    }

    /// A click on `hover` in a tool that picks geometry.
    fn pick3(&mut self, feature: FeatureId, s: &Sketch3d, hover: Option<EntityId>) {
        let Mode::Sketch3d(sm) = &mut self.mode else { return };
        let tool = sm.tool;
        let (wants, n) = tool.wants();
        let Some(id) = hover else {
            self.set_status(format!("Nothing there. {}", tool.hint()));
            return;
        };
        let fits = match wants {
            Wants::Point => s.is_point(id),
            Wants::Line => s.is_line(id),
            Wants::LineOrArc => s.is_line(id) || s.is_arc(id),
            Wants::Curve => s.is_curve(id),
            Wants::Any => true,
        };
        if !fits {
            self.set_error(format!("That is a {}. {}", s.geometry(id).map_or("nothing", Geometry3::kind_name), tool.hint()));
            return;
        }
        if sm.picks.contains(&id) {
            return;
        }
        sm.picks.push(id);
        if sm.picks.len() < n {
            return;
        }
        let picks = std::mem::take(&mut sm.picks);
        let c = match (tool, picks.as_slice()) {
            (Tool3::Coincident, [a, b]) => Constraint3::Coincident { a: *a, b: *b },
            (Tool3::Fix, [point]) => Constraint3::Fix { point: *point },
            (Tool3::Along(axis), [line]) => Constraint3::Along { line: *line, axis },
            (Tool3::Parallel, [a, b]) => Constraint3::Parallel { a: *a, b: *b },
            (Tool3::Perpendicular, [a, b]) => Constraint3::Perpendicular { a: *a, b: *b },
            (Tool3::Equal, [a, b]) => Constraint3::Equal { a: *a, b: *b },
            (Tool3::Tangent, [a, b]) => Constraint3::Tangent { a: *a, b: *b },
            (Tool3::Coordinate(axis), [point]) => {
                let value = s.point(*point).map_or(0.0, |q| axis.of(q));
                self.panel = Some(Panel::Value(ValuePanel::new(
                    "Coordinate",
                    axis.name(),
                    value,
                    ValueFor::Dimension3 { sketch: feature, constraint: Constraint3::Coordinate { point: *point, axis, value } },
                )));
                return;
            }
            _ => return,
        };
        if self.exec_status("sketch3d.constrain", json!({ "sketch": feature.0, "constraint": c })).is_some() {
            let hint = tool.hint();
            self.set_status(hint);
        }
    }

    /// A click in the Dimension tool.
    fn dimension3(&mut self, feature: FeatureId, s: &Sketch3d, hover: Option<EntityId>) {
        let Mode::Sketch3d(sm) = &mut self.mode else { return };
        let Some(id) = hover else {
            self.set_status(format!("Nothing there. {}", Tool3::Dimension.hint()));
            return;
        };
        let what = match (s.geometry(id), sm.picks.first().copied()) {
            (Some(Geometry3::Point { .. }), None) => {
                sm.picks.push(id);
                self.set_status("Click the other point.");
                return;
            }
            (Some(Geometry3::Point { .. }), Some(a)) if a != id => {
                sm.picks.clear();
                let value = s.point(a).zip(s.point(id)).map_or(0.0, |(p, q)| p.dist(q));
                Some(("Distance", value, Constraint3::Distance { a, b: id, value }))
            }
            (Some(Geometry3::Line { .. }), _) => {
                sm.picks.clear();
                let value = s.length(id).unwrap_or(0.0);
                Some(("Length", value, Constraint3::Length { line: id, value }))
            }
            (Some(Geometry3::Arc { .. }), _) => {
                sm.picks.clear();
                let value = s.arc(id).map_or(0.0, |a| a.radius);
                Some(("Radius", value, Constraint3::Radius { arc: id, value }))
            }
            _ => None,
        };
        match what {
            Some((title, value, constraint)) => {
                self.panel = Some(Panel::Value(ValuePanel::new(title, title, value, ValueFor::Dimension3 { sketch: feature, constraint })));
            }
            None => self.set_error(format!("A dimension cannot be of that. {}", Tool3::Dimension.hint())),
        }
    }

    /// The box of the coordinates: X, Y, Z and a button that places them; for a spline, a button
    /// that makes it. Returns the place to put, when the button or Enter says so.
    fn coordinates_box(&mut self, ui: &Ui, rect: Rect, t: &Tokens) -> (Option<[f64; 3]>, bool) {
        let Mode::Sketch3d(sm) = &mut self.mode else { return (None, false) };
        if !matches!(sm.tool, Tool3::Line | Tool3::Spline | Tool3::Point) {
            return (None, false);
        }
        let (mut add, mut make) = (false, false);
        let spline = sm.tool == Tool3::Spline;
        let enough = sm.clicks.len() >= 2;
        let xyz = &mut sm.xyz;
        egui::Area::new(egui::Id::new("tn_s3_box")).order(egui::Order::Foreground).fixed_pos(rect.min + vec2(12.0, 12.0)).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).fill(t.panel).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (i, (name, key)) in [("X", "tn_s3_x"), ("Y", "tn_s3_y"), ("Z", "tn_s3_z")].iter().enumerate() {
                        ui.label(*name).on_hover_text("Where the next point goes, in the part's own axes");
                        add |= crate::properties::eq_field(ui, egui::Id::new(*key), &mut xyz[i], None, "mm", -1.0e6..=1.0e6, 64.0, t).entered;
                    }
                    // (A button clicked gives the keys back: to the sketch's shortcuts, and Enter.)
                    let press = crate::drawing::button(ui, "Add", "tn_s3_add").on_hover_text("Puts a point at these coordinates");
                    if press.clicked() {
                        press.surrender_focus();
                        add = true;
                    }
                    if spline && enough {
                        let press = crate::drawing::button(ui, "Make Spline", "tn_s3_make");
                        if press.clicked() {
                            press.surrender_focus();
                            make = true;
                        }
                    }
                });
            });
        });
        (add.then_some(*xyz), make)
    }

    /// Sketch mode's pointer, keys and drawing for a 3D sketch.
    pub(crate) fn sketch3d_ui(&mut self, ui: &Ui, resp: &egui::Response, rect: Rect, t: &Tokens) {
        let (typed, make) = self.coordinates_box(ui, rect, t);
        let Mode::Sketch3d(sm) = &self.mode else { return };
        let feature = sm.feature;
        if let Some([x, y, z]) = typed {
            let at = Vec3::new(x, y, z);
            let axis = self.typed_along(at);
            // (Typed exactly where it is, the origin is what is meant, as a click on it means: a path
            // from it stays there when it is dimensioned, and on what was sketched there.)
            let target = if at == Vec3::ZERO { Target::Origin } else { Target::Free };
            self.place3(feature, Click3 { at, target, axis });
            if !self.screen3(at, rect).is_some_and(|q| rect.contains(q)) {
                self.frame_with(at);
            }
        }
        if make {
            self.finish_spline3(feature);
        }
        let Some(doc_sketch) = self.document().sketch3d(feature).cloned() else {
            self.mode = Mode::Model;
            return;
        };
        // Measure works here as anywhere.
        if matches!(self.panel, Some(Panel::Measure(_))) && self.panel_pointer(resp, rect) {
            self.draw_sketch3d(ui, &doc_sketch, None, rect, t);
            return;
        }
        let pointer = resp.hover_pos().or_else(|| resp.interact_pointer_pos());
        let hover = pointer.and_then(|p| self.hit3(&doc_sketch, p, rect));
        let Mode::Sketch3d(sm) = &self.mode else { return };
        let tool = sm.tool;
        let places = matches!(tool, Tool3::Line | Tool3::Spline | Tool3::Point);
        let under = if places { pointer.and_then(|p| self.place_at(&doc_sketch, sm, p, rect)) } else { None };
        if let Mode::Sketch3d(sm) = &mut self.mode {
            sm.hover = hover;
            sm.under = under.clone();
            sm.constraints.retain(|c| doc_sketch.constraint(*c).is_some());
            sm.selection.retain(|e| doc_sketch.entity(*e).is_some());
        }
        // Keys.
        let typing = ui.ctx().egui_wants_keyboard_input();
        let (enter, esc, delete) =
            ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Delete)));
        if !typing && esc {
            self.sketch3d_escape();
        }
        // (An Enter the box took put a point there: the next is for the spline.)
        if !typing && enter && typed.is_none() {
            self.finish_spline3(feature);
        }
        if !typing && delete {
            self.delete3(feature);
        }
        // Include Geometry works on the part behind the sketch.
        if tool == Tool3::Include {
            let (meshes, tree) = (self.scene.placed(), self.body_tree());
            let hit = pointer.and_then(|p| {
                let (x, y, w, h) = (f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
                pick_edge_in(tree.as_deref(), &meshes, &self.view.camera, w, h, x, y, 5.0)
            });
            self.view.hover = hit.as_ref().map(|e| crate::viewport::Pick::Edge { body: e.body, edge: e.edge });
            if resp.clicked_by(egui::PointerButton::Primary) {
                match hit {
                    Some(e) => self.include3(feature, e.body, e.edge),
                    None => self.set_status(format!("No edge there. {}", tool.hint())),
                }
            }
        } else {
            self.view.hover = None;
        }
        let on_badge = {
            let Mode::Sketch3d(sm) = &self.mode else { return };
            pointer.filter(|_| tool == Tool3::Select).and_then(|p| sm.badges.iter().find(|b| b.1.contains(p))).map(|b| b.0)
        };
        let on_dim = {
            let Mode::Sketch3d(sm) = &self.mode else { return };
            pointer.and_then(|p| sm.dims.iter().find(|b| b.1.contains(p))).map(|b| b.0)
        };
        // A dimension's value, in the Select tool, is selected as a symbol is.
        let on_held = on_badge.or(on_dim.filter(|_| tool == Tool3::Select));
        // Clicks.
        let primary = resp.clicked_by(egui::PointerButton::Primary);
        if resp.double_clicked()
            && let Some(cid) = on_dim
        {
            // A dimension's value, double-clicked: to change it.
            let value = doc_sketch.constraint(cid).and_then(Constraint3::value).unwrap_or(0.0);
            self.panel =
                Some(Panel::Value(ValuePanel::new("Edit Dimension", "Value", value, ValueFor::EditDimension3 { sketch: feature, constraint: cid })));
        } else if primary && let (Some(cid), Mode::Sketch3d(sm)) = (on_held, &mut self.mode) {
            let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
            match (sm.constraints.iter().position(|c| *c == cid), add) {
                (Some(i), true) => {
                    sm.constraints.remove(i);
                }
                (None, true) => sm.constraints.push(cid),
                _ => {
                    sm.constraints = vec![cid];
                    sm.selection.clear();
                }
            }
            let name = doc_sketch.constraint(cid).map_or_else(|| "constraint".to_owned(), |c| doc_sketch.constraint_words(c, &c.refs()));
            let change = if on_dim == Some(cid) { "Double-click to change it; Delete" } else { "Delete" };
            self.set_status(format!("Selected: {name}. {change} removes it."));
        } else if primary {
            match tool {
                Tool3::Select => self.select3(&doc_sketch, hover, ui.input(|i| i.modifiers.command || i.modifiers.shift)),
                Tool3::Line | Tool3::Spline | Tool3::Point => {
                    if let Some(c) = under {
                        self.place3(feature, c);
                    }
                }
                Tool3::Bend => {
                    let corner = hover.filter(|h| doc_sketch.is_point(*h));
                    match corner.map(|point| (point, doc_sketch.bend_refused(point))) {
                        Some((_, Some(why))) => self.set_error(why),
                        Some((point, None)) => {
                            let radius = match &self.mode {
                                Mode::Sketch3d(sm) => sm.radius,
                                _ => 5.0,
                            };
                            self.panel = Some(Panel::Value(ValuePanel::new("Bend", "Radius", radius, ValueFor::Bend3 { sketch: feature, point })));
                        }
                        None => self.set_status(format!("That is no corner. {}", tool.hint())),
                    }
                }
                Tool3::Include => {}
                Tool3::Dimension => self.dimension3(feature, &doc_sketch, hover),
                _ => self.pick3(feature, &doc_sketch, hover),
            }
        }
        // Dragging a point: a preview on a copy, one command on release.
        if tool == Tool3::Select {
            if resp.drag_started_by(egui::PointerButton::Primary)
                && self.left_drag_tool(ui) == crate::viewport::Nav::Select
                && let Some(origin) = ui.input(|i| i.pointer.press_origin())
                && let Some((_, held)) = self.own_point_at(&doc_sketch, origin, rect)
            {
                // What stays where it is whatever the pointer does says why.
                if let Some(why) = doc_sketch.held_why(held) {
                    self.set_status(why);
                } else if let (Some(at), Mode::Sketch3d(sm)) = (doc_sketch.point(held), &mut self.mode) {
                    sm.drag = Some((held, doc_sketch.clone(), at));
                }
            }
            let target = {
                let Mode::Sketch3d(sm) = &self.mode else { return };
                match (&sm.drag, pointer) {
                    (Some((_, _, through)), Some(p)) => {
                        let (o, d) = self.view.camera.ray(
                            f64::from(p.x - rect.left()),
                            f64::from(p.y - rect.top()),
                            f64::from(rect.width()),
                            f64::from(rect.height()),
                        );
                        Some(o + d * ((*through - o).dot(d) / d.dot(d)))
                    }
                    _ => None,
                }
            };
            if let (Some(at), Mode::Sketch3d(sm)) = (target, &mut self.mode)
                && let Some((p, preview, _)) = &mut sm.drag
            {
                let mut next = doc_sketch.clone();
                if next.drag(*p, at).is_ok() {
                    *preview = next;
                }
            }
            if resp.drag_stopped()
                && let Mode::Sketch3d(sm) = &mut self.mode
                && let Some((p, _, _)) = sm.drag.take()
                && let Some(at) = target
            {
                let moved = self.exec_status("sketch3d.move", json!({ "sketch": feature.0, "point": p.0, "at": [at.x, at.y, at.z] }));
                // (Nothing wrong, and nothing done: its constraints leave it no way to go.)
                if moved.is_some() && self.document().sketch3d(feature).and_then(|s| s.point(p)) == doc_sketch.point(p) {
                    self.set_status("What holds that point leaves it nowhere to go: delete a constraint on it first to move it.");
                }
            }
        }
        let preview = match &self.mode {
            Mode::Sketch3d(sm) => sm.drag.as_ref().map(|d| d.1.clone()),
            _ => None,
        };
        self.draw_sketch3d(ui, preview.as_ref().unwrap_or(&doc_sketch), pointer, rect, t);
    }

    /// Selects what was clicked, and says the length of what is selected.
    fn select3(&mut self, s: &Sketch3d, hover: Option<EntityId>, add: bool) {
        let Mode::Sketch3d(sm) = &mut self.mode else { return };
        sm.constraints.clear();
        match (hover, add) {
            (Some(id), true) => {
                if let Some(i) = sm.selection.iter().position(|e| *e == id) {
                    sm.selection.remove(i);
                } else {
                    sm.selection.push(id);
                }
            }
            (Some(id), false) => sm.selection = vec![id],
            (None, _) => sm.selection.clear(),
        }
        let picked = sm.selection.clone();
        let lengths: Vec<f64> = picked.iter().filter_map(|e| s.length(*e)).collect();
        match lengths.as_slice() {
            [] => {
                let hint = Tool3::Select.hint();
                self.set_status(hint);
            }
            [one] => self.set_status(format!("Length {} mm.", crate::sketcher::fmt_len(*one))),
            many => self.set_status(format!("{} curves, {} mm long in all.", many.len(), crate::sketcher::fmt_len(many.iter().sum::<f64>()))),
        }
    }

    /// Delete: the constraints selected, else the geometry.
    fn delete3(&mut self, feature: FeatureId) {
        let Mode::Sketch3d(sm) = &mut self.mode else { return };
        let (entities, constraints) = (std::mem::take(&mut sm.selection), std::mem::take(&mut sm.constraints));
        if entities.is_empty() && constraints.is_empty() {
            return;
        }
        let p = json!({
            "sketch": feature.0,
            "entities": entities.iter().map(|e| e.0).collect::<Vec<_>>(),
            "constraints": constraints.iter().map(|c| c.0).collect::<Vec<_>>(),
        });
        let _ = self.exec_status("sketch3d.delete", p);
    }

    /// Brings an edge of the part in.
    fn include3(&mut self, feature: FeatureId, body: usize, edge: u32) {
        let Some(view) = self.scene.bodies.get(body) else { return };
        let Some(eref) = view.edge_ref(edge) else {
            self.set_error("That edge cannot be referred to: choose another.");
            return;
        };
        let i = edge as usize;
        let (Some(curve), Some(ends), Some(mid), Some(named)) = (view.curves.get(i), view.ends.get(i), view.mids.get(i), view.edges.get(i)) else {
            return;
        };
        // What the kernel knows of an edge, from the scene.
        let line = view.mesh.edges.iter().find(|m| m.edge == edge).map_or(&[][..], |m| m.points.as_slice());
        let along: Vec<Vec3> = line.iter().map(|q| Vec3::new(f64::from(q[0]), f64::from(q[1]), f64::from(q[2]))).collect();
        let info =
            tenon_kernel::EdgeInfo { curve: curve.clone(), length: named.1.length, start: ends[0], end: ends[1], mid: *mid, degenerate: false };
        let found = tenon_model::sketch3d::edge_found(&info, || Ok(along));
        match found {
            Ok(found) => {
                if self.exec_status("sketch3d.include", json!({ "sketch": feature.0, "edge": eref, "geometry": found })).is_some() {
                    self.set_status("Brought in: it follows the part. Click another, or Esc ends.");
                }
            }
            Err(e) => self.set_error(format!("That edge cannot be brought in: {e}")),
        }
    }

    /// The Measure panel: a curve of a 3D sketch clicked, on its own or with those before it.
    pub(crate) fn measure_curve3(&mut self, curve: (FeatureId, EntityId), add: bool) {
        let Some(Panel::Measure(m)) = &mut self.panel else { return };
        if !add {
            m.curves.clear();
        }
        if !m.curves.contains(&curve) {
            m.curves.push(curve);
        }
        (m.a, m.b, m.pending) = (None, None, None);
        let curves = m.curves.clone();
        let doc = self.session.document();
        let lengths: Vec<f64> = curves.iter().filter_map(|(s, c)| doc.sketch3d(*s).and_then(|sk| sk.length(*c))).collect();
        let mut values = vec![("Length".to_owned(), lengths.iter().sum::<f64>(), "mm".to_owned())];
        if lengths.len() > 1 {
            values.push(("Curves".to_owned(), lengths.len() as f64, String::new()));
        }
        if let Some(Panel::Measure(m)) = &mut self.panel {
            m.result = Some(Ok(tenon_model::measure::Measurement { values, nearest: None }));
        }
    }

    /// Draws the sketch being worked in.
    fn draw_sketch3d(&mut self, ui: &Ui, shown: &Sketch3d, pointer: Option<Pos2>, rect: Rect, t: &Tokens) {
        let Mode::Sketch3d(sm) = &self.mode else { return };
        let feature = sm.feature;
        let p = ui.painter().with_clip_rect(rect);
        let fully = sm.dof.as_ref().map(|d| d.2.clone()).unwrap_or_default();
        let (picked, hot) = (t.sketch_picked, t.sketch_hot);
        let (reference, orphan) = (t.sketch_ref, t.sketch_lost);
        let (own, done) = (colour(t), colour_done(t));
        let chosen = sm.constraints.clone();
        let lost: BTreeSet<EntityId> = self.scene.lost.get(&feature).map(|l| l.iter().map(|(e, _)| *e).collect()).unwrap_or_default();
        let lit: BTreeSet<EntityId> = sm.constraints.iter().filter_map(|c| shown.constraint(*c)).flat_map(Constraint3::refs).collect();
        let color_of = |id: EntityId| {
            if sm.selection.contains(&id) || sm.picks.contains(&id) {
                picked
            } else if sm.hover == Some(id) || lit.contains(&id) {
                hot
            } else if lost.contains(&id) {
                orphan
            } else if shown.is_reference(id) {
                reference
            } else if fully.contains(&id) {
                done
            } else {
                own
            }
        };
        for (id, e) in shown.entities() {
            if e.geometry.is_point() {
                continue;
            }
            let pts: Vec<Pos2> = shown.polyline(id).into_iter().filter_map(|q| self.screen3(q, rect)).collect();
            if pts.len() < 2 {
                continue;
            }
            if e.construction {
                p.extend(Shape::dashed_line(&pts, Stroke::new(1.0, t.text_dim), 6.0, 4.0));
            } else {
                p.add(Shape::line(pts, Stroke::new(if sm.hover == Some(id) { 2.6 } else { 2.0 }, color_of(id))));
            }
        }
        for (id, e) in shown.entities() {
            let Geometry3::Point { pos } = e.geometry else { continue };
            let Some(q) = self.screen3(pos, rect) else { continue };
            let s = if sm.hover == Some(id) { 4.0 } else { 2.5 };
            if e.reference {
                p.circle_filled(q, s + 0.5, color_of(id));
            } else if e.construction {
                // (A bend's corner and centre.)
                p.circle_stroke(q, s, Stroke::new(1.0, if sm.hover == Some(id) { hot } else { t.text_dim }));
            } else {
                p.rect_filled(Rect::from_center_size(q, vec2(s * 2.0, s * 2.0)), 1.0, color_of(id));
            }
        }
        // Constraint symbols beside what they hold.
        let mut badges = Vec::new();
        let mut taken: std::collections::HashMap<EntityId, usize> = std::collections::HashMap::new();
        for (cid, c) in shown.constraints() {
            let Some((pic, beside)) = glyph(c) else { continue };
            if !self.view.show_constraints {
                continue;
            }
            for on in beside {
                let Some(q) = middle_of(shown, on).and_then(|m| self.screen3(m, rect)) else { continue };
                let slot = taken.entry(on).or_insert(0);
                let r = Rect::from_center_size(q + vec2(13.0 + 18.0 * *slot as f32, -13.0), vec2(16.0, 16.0));
                *slot += 1;
                let chosen = sm.constraints.contains(&cid);
                p.rect_filled(r, 3.0, if chosen { picked.gamma_multiply(0.35) } else { t.panel.gamma_multiply(0.92) });
                p.rect_stroke(r, 3.0, Stroke::new(1.0, if chosen { picked } else { t.border }), egui::StrokeKind::Inside);
                match pic {
                    Ok(icon) => crate::icons::paint(&p, r.shrink(2.5), icon, if chosen { hot } else { t.text_dim }),
                    Err(letter) => {
                        let color = match letter {
                            "X" => t.axis_x,
                            "Y" => t.axis_y,
                            _ => t.axis_z,
                        };
                        p.text(r.center(), Align2::CENTER_CENTER, letter, theme::small(), color);
                    }
                }
                badges.push((cid, r));
            }
        }
        // Dimensions: the value beside what it measures, with a line to it.
        let mut dims = Vec::new();
        for (cid, c) in shown.constraints() {
            let Some(text) = dimension_text(c) else { continue };
            let anchor = match c {
                Constraint3::Distance { a, b, .. } => shown.point(*a).zip(shown.point(*b)).map(|(x, y)| x.lerp(y, 0.5)),
                _ => c.refs().first().and_then(|e| middle_of(shown, *e)),
            };
            let Some(q) = anchor.and_then(|m| self.screen3(m, rect)) else { continue };
            let at = q + vec2(10.0, 14.0);
            let size = p.layout_no_wrap(text.clone(), theme::small(), t.text).size();
            let r = Rect::from_min_size(at, size + vec2(8.0, 4.0));
            let over = pointer.is_some_and(|m| r.contains(m));
            let on = chosen.contains(&cid);
            p.line_segment([q, r.left_top()], Stroke::new(1.0, t.dim_line));
            p.rect_filled(r, 2.0, if on { picked.gamma_multiply(0.35) } else { t.panel.gamma_multiply(0.85) });
            p.text(r.center(), Align2::CENTER_CENTER, text, theme::small(), if over || on { hot } else { done });
            dims.push((cid, r));
        }
        // What a click would do.
        let stroke = Stroke::new(1.3, own.gamma_multiply(0.8));
        if let Some(u) = &sm.under {
            if sm.tool == Tool3::Line
                && let Some(from) = sm.clicks.last()
                && let (Some(a), Some(b)) = (self.screen3(from.at, rect), self.screen3(u.at, rect))
            {
                let along = u.axis.map_or(stroke, |axis| {
                    Stroke::new(
                        1.6,
                        match axis {
                            Axis3::X => t.axis_x,
                            Axis3::Y => t.axis_y,
                            Axis3::Z => t.axis_z,
                        },
                    )
                });
                p.line_segment([a, b], along);
                if let Some(axis) = u.axis {
                    p.text(b + vec2(10.0, -14.0), Align2::LEFT_BOTTOM, axis.name(), theme::small(), along.color);
                }
            }
            if sm.tool == Tool3::Spline && !sm.clicks.is_empty() {
                let mut through: Vec<Vec3> = sm.clicks.iter().map(|c| c.at).collect();
                through.push(u.at);
                if let Some(s) = Through::new(&through) {
                    let pts: Vec<Pos2> = s.polyline().into_iter().filter_map(|q| self.screen3(q, rect)).collect();
                    p.add(Shape::line(pts, stroke));
                }
            }
            if let Some(q) = self.screen3(u.at, rect) {
                match u.target {
                    Target::Free => {}
                    Target::Existing(_) => {
                        p.circle_stroke(q, 7.0, Stroke::new(1.5, hot));
                    }
                    _ => {
                        p.circle_stroke(q, 7.0, Stroke::new(1.5, reference));
                    }
                }
            }
        }
        for c in &sm.clicks {
            if let Some(q) = self.screen3(c.at, rect) {
                p.circle_filled(q, 3.0, hot);
            }
        }
        if let Mode::Sketch3d(sm) = &mut self.mode {
            sm.badges = badges;
            sm.dims = dims;
        }
    }
}
