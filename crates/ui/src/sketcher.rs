//! Sketch mode: drawing tools, constraints, dimensions, dragging and the sketch overlay. Every
//! edit is a registry command (`sketch.*`), so the UI and the CLI/MCP share one code path.

use std::collections::BTreeSet;

use egui::{Align2, Color32, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};
use serde_json::{Map, Value, json};
use tenon_geom::{Aabb3, Frame, Vec2};
use tenon_model::FeatureId;
use tenon_sketch::{Constraint, ConstraintId, EntityId, Geometry, Sketch};

use crate::panels::{Panel, ValueFor, ValuePanel};
use crate::theme::{self, Tokens};
use crate::workbench::{Mode, Workbench};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    Select,
    Line,
    Circle,
    Arc,
    Rectangle,
    Polygon,
    Spline,
    Point,
    Trim,
    Mirror,
    Fillet,
    Dimension,
    Coincident,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Tangent,
    Equal,
    Concentric,
    Fix,
    Collinear,
    Symmetric,
}

const TOOLS: &[(&str, Tool)] = &[
    ("sketch.collinear", Tool::Collinear),
    ("sketch.symmetric", Tool::Symmetric),
    ("sketch.line", Tool::Line),
    ("sketch.circle", Tool::Circle),
    ("sketch.arc", Tool::Arc),
    ("sketch.rectangle", Tool::Rectangle),
    ("sketch.polygon", Tool::Polygon),
    ("sketch.spline", Tool::Spline),
    ("sketch.point", Tool::Point),
    ("sketch.trim", Tool::Trim),
    ("sketch.mirror", Tool::Mirror),
    ("sketch.fillet", Tool::Fillet),
    ("sketch.dimension", Tool::Dimension),
    ("sketch.coincident", Tool::Coincident),
    ("sketch.horizontal", Tool::Horizontal),
    ("sketch.vertical", Tool::Vertical),
    ("sketch.parallel", Tool::Parallel),
    ("sketch.perpendicular", Tool::Perpendicular),
    ("sketch.tangent", Tool::Tangent),
    ("sketch.equal", Tool::Equal),
    ("sketch.concentric", Tool::Concentric),
    ("sketch.fix", Tool::Fix),
];

impl Tool {
    fn id(self) -> Option<&'static str> {
        TOOLS.iter().find(|(_, t)| *t == self).map(|(id, _)| *id)
    }
    fn hint(self) -> &'static str {
        match self {
            Tool::Select => "Click to select; drag points to move them; Delete removes the selection.",
            Tool::Line => "Click points to draw lines; Esc or right-click ends the chain.",
            Tool::Circle => "Click the centre, then a point on the circle.",
            Tool::Arc => "Click the start, the end, then a point on the arc.",
            Tool::Rectangle => "Click two opposite corners.",
            Tool::Polygon => "Click the centre, then a corner (6 sides).",
            Tool::Spline => "Click control points; Enter finishes.",
            Tool::Point => "Click to place points.",
            Tool::Trim => "Click the piece of a curve to remove.",
            Tool::Mirror => "Click the mirror line.",
            Tool::Fillet => "Click a corner where two lines meet.",
            Tool::Dimension => "Click a line, circle or arc; or two points/lines. Click empty space to place a line's length.",
            Tool::Coincident => "Click two points, or a point and a curve.",
            Tool::Horizontal | Tool::Vertical => "Click a line.",
            Tool::Parallel | Tool::Perpendicular => "Click two lines.",
            Tool::Tangent => "Click a line and a circle/arc, or two circles/arcs.",
            Tool::Equal => "Click two lines or two circles/arcs.",
            Tool::Concentric => "Click two circles/arcs.",
            Tool::Fix => "Click a point.",
            Tool::Collinear => "Click two lines.",
            Tool::Symmetric => "Click two points, then the line of symmetry.",
        }
    }
}

/// A constraint inferred while drawing (the pointer is nearly horizontal or vertical).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Infer {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Click {
    pub at: Vec2,
    pub point: Option<EntityId>,
    pub infer: Option<Infer>,
}

/// Inference tolerance: within this angle of horizontal or vertical, lines snap to it.
const INFER_ANGLE: f64 = 3.0 * std::f64::consts::PI / 180.0;

/// The value boxes beside the cursor while drawing (typed text per field).
#[derive(Clone, Debug, Default)]
pub(crate) struct Hud {
    pub fields: [String; 2],
    pub active: usize,
}

impl Hud {
    fn typed(&self, i: usize) -> Option<f64> {
        self.fields.get(i).and_then(|s| s.parse::<f64>().ok()).filter(|v| v.is_finite())
    }
    fn is_empty(&self) -> bool {
        self.fields.iter().all(String::is_empty)
    }
}

/// State of sketch mode.
pub(crate) struct SketchMode {
    pub feature: FeatureId,
    pub tool: Tool,
    pub clicks: Vec<Click>,
    pub picks: Vec<EntityId>,
    pub selection: Vec<EntityId>,
    pub hover: Option<EntityId>,
    pub drag: Option<(EntityId, Sketch)>,
    pub hud: Hud,
    dof: Option<(u64, Option<usize>, BTreeSet<EntityId>)>,
    /// The sketch as it was when opened. Until Finish Sketch the part is regenerated with this,
    /// so drawing and dragging never wait for the features that use the sketch.
    pub base: Option<Sketch>,
}

impl SketchMode {
    fn new(feature: FeatureId, base: Option<Sketch>) -> Self {
        SketchMode {
            feature,
            tool: Tool::Select,
            clicks: Vec::new(),
            picks: Vec::new(),
            selection: Vec::new(),
            hover: None,
            drag: None,
            hud: Hud::default(),
            dof: None,
            base,
        }
    }
}

/// The value boxes a drawing tool offers once it has its first point: labels and units.
fn hud_fields(tool: Tool) -> &'static [(&'static str, &'static str)] {
    match tool {
        Tool::Line => &[("Length", "mm"), ("Angle", "deg")],
        Tool::Circle => &[("Diameter", "mm")],
        Tool::Rectangle => &[("Width", "mm"), ("Height", "mm")],
        _ => &[],
    }
}

/// Snaps a line end to horizontal or vertical from `start` when it is nearly so.
fn infer_line(start: Vec2, cursor: Vec2) -> (Vec2, Option<Infer>) {
    let d = cursor - start;
    let len = d.len();
    if len < 1e-9 {
        return (cursor, None);
    }
    if (d.y / len).abs() < INFER_ANGLE.sin() {
        (Vec2::new(cursor.x, start.y), Some(Infer::Horizontal))
    } else if (d.x / len).abs() < INFER_ANGLE.sin() {
        (Vec2::new(start.x, cursor.y), Some(Infer::Vertical))
    } else {
        (cursor, None)
    }
}

/// A sketch grid step: 1, 2 or 5 times a power of ten, at least `min_px` apart on screen.
pub(crate) fn grid_step(px_per_mm: f64, min_px: f64) -> f64 {
    if !(px_per_mm.is_finite() && px_per_mm > 0.0) {
        return 10.0;
    }
    let raw = min_px / px_per_mm;
    let p = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * p).find(|s| *s >= raw).unwrap_or(10.0 * p)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Point,
    Line,
    Round,
    Other,
}

fn kind(s: &Sketch, id: EntityId) -> Kind {
    match s.geometry(id) {
        Some(Geometry::Point { .. }) => Kind::Point,
        Some(Geometry::Line { .. }) => Kind::Line,
        Some(Geometry::Circle { .. } | Geometry::Arc { .. }) => Kind::Round,
        _ => Kind::Other,
    }
}

/// Point parameter of a command: an existing point id, else coordinates.
fn put_point(m: &mut Map<String, Value>, id_key: &str, x: &str, y: &str, c: &Click) {
    match c.point {
        Some(id) => {
            m.insert(id_key.into(), json!(id.0));
        }
        None => {
            m.insert(x.into(), json!(c.at.x));
            m.insert(y.into(), json!(c.at.y));
        }
    }
}

pub(crate) fn fmt_len(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Screen mapping of the sketch plane.
#[derive(Clone, Copy)]
struct Plane {
    frame: Frame,
    cam: tenon_render::Camera,
    rect: Rect,
}

impl Plane {
    fn project(self, p: Vec2) -> Option<Pos2> {
        self.cam
            .project(self.frame.plane_point(p), f64::from(self.rect.width()), f64::from(self.rect.height()))
            .map(|(x, y, _)| pos2(self.rect.left() + x as f32, self.rect.top() + y as f32))
    }
    fn unproject(self, p: Pos2) -> Option<Vec2> {
        let (o, d) = self.cam.ray(
            f64::from(p.x - self.rect.left()),
            f64::from(p.y - self.rect.top()),
            f64::from(self.rect.width()),
            f64::from(self.rect.height()),
        );
        let n = self.frame.z();
        let den = d.dot(n);
        if den.abs() < 1e-9 {
            return None;
        }
        let t = (self.frame.origin() - o).dot(n) / den;
        let local = self.frame.to_local(o + d * t);
        let v = Vec2::new(local.x, local.y);
        v.is_finite().then_some(v)
    }
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t).distance(p)
}

/// Nearest entity under the pointer: points within 9 px first, then curves within 6 px.
fn hit_test(s: &Sketch, plane: &Plane, p: Pos2) -> Option<EntityId> {
    let mut best_point: Option<(f32, EntityId)> = None;
    let mut best_curve: Option<(f32, EntityId)> = None;
    for (id, e) in s.entities() {
        if let Geometry::Point { pos } = e.geometry {
            if let Some(q) = plane.project(pos) {
                let d = q.distance(p);
                if d < 9.0 && best_point.is_none_or(|b| d < b.0) {
                    best_point = Some((d, id));
                }
            }
            continue;
        }
        let pts: Vec<Pos2> = s.tessellate(id, 0.05).into_iter().filter_map(|v| plane.project(v)).collect();
        for w in pts.windows(2) {
            let d = seg_dist(p, w[0], w[1]);
            if d < 6.0 && best_curve.is_none_or(|b| d < b.0) {
                best_curve = Some((d, id));
            }
        }
    }
    best_point.or(best_curve).map(|(_, id)| id)
}

/// The sketch grid: lines every grid step, stronger every fifth, the sketch axes coloured. Only
/// drawn when the whole viewport maps onto the plane (looking at it, not edge-on).
fn draw_grid(ui: &Ui, plane: &Plane, rect: Rect, t: &Tokens) {
    let (Some(o), Some(x1)) = (plane.project(Vec2::new(0.0, 0.0)), plane.project(Vec2::new(1.0, 0.0))) else { return };
    let step = grid_step(f64::from(o.distance(x1)), 18.0);
    let corners: Option<Vec<Vec2>> =
        [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()].iter().map(|c| plane.unproject(*c)).collect();
    let Some(c) = corners else { return };
    let (lo, hi) = c.iter().fold((c[0], c[0]), |(a, b), p| (a.min(*p), b.max(*p)));
    let (i0, i1) = ((lo.x / step).floor() as i64, (hi.x / step).ceil() as i64);
    let (j0, j1) = ((lo.y / step).floor() as i64, (hi.y / step).ceil() as i64);
    if i1 - i0 > 400 || j1 - j0 > 400 {
        return;
    }
    let p = ui.painter();
    let line = |a: Vec2, b: Vec2, color: Color32| {
        if let (Some(a), Some(b)) = (plane.project(a), plane.project(b)) {
            p.line_segment([a, b], Stroke::new(1.0, color));
        }
    };
    for i in i0..=i1 {
        let x = i as f64 * step;
        let color = if i == 0 { t.axis_y.gamma_multiply(0.55) } else { t.text_dim.gamma_multiply(if i % 5 == 0 { 0.22 } else { 0.09 }) };
        line(Vec2::new(x, lo.y), Vec2::new(x, hi.y), color);
    }
    for j in j0..=j1 {
        let y = j as f64 * step;
        let color = if j == 0 { t.axis_x.gamma_multiply(0.55) } else { t.text_dim.gamma_multiply(if j % 5 == 0 { 0.22 } else { 0.09 }) };
        line(Vec2::new(lo.x, y), Vec2::new(hi.x, y), color);
    }
}

/// Where a dimension label sits (sketch coordinates) and its text.
fn dimension_label(s: &Sketch, c: &Constraint) -> Option<(Vec2, String)> {
    use Constraint::*;
    let value = c.value()?;
    let text = if c.is_angular() { format!("{}deg", fmt_len(value.to_degrees())) } else { fmt_len(value.abs()) };
    let at = match c {
        Length { line, .. } => {
            let (a, b) = s.line(*line)?;
            a.mid(b) + (b - a).normalized().perp() * 4.0
        }
        Radius { curve, .. } | Diameter { curve, .. } => {
            let (center, r) = s.circle(*curve)?;
            center + Vec2::new(r * 0.707, r * 0.707) * 1.15
        }
        Distance { a, b, .. } | HorizontalDistance { a, b, .. } | VerticalDistance { a, b, .. } => {
            let pa = s.point(*a).or_else(|| s.line(*a).map(|(p, q)| p.mid(q)))?;
            let pb = s.point(*b).or_else(|| s.line(*b).map(|(p, q)| p.mid(q)))?;
            pa.mid(pb) + Vec2::new(0.0, 3.0)
        }
        Angle { a, b, .. } => {
            let (a1, b1) = s.line(*a)?;
            let (a2, b2) = s.line(*b)?;
            a1.mid(b1).mid(a2.mid(b2))
        }
        _ => return None,
    };
    let prefix = match c {
        Diameter { .. } => "D",
        Radius { .. } => "R",
        _ => "",
    };
    Some((at, format!("{prefix}{text}")))
}

fn glyph(c: &Constraint) -> Option<(&'static str, EntityId)> {
    use Constraint::*;
    Some(match c {
        Horizontal { line } => ("H", *line),
        Vertical { line } => ("V", *line),
        Parallel { a, .. } => ("//", *a),
        Perpendicular { a, .. } => ("L", *a),
        Tangent { a, .. } => ("T", *a),
        Equal { a, .. } => ("=", *a),
        Concentric { a, .. } => ("O", *a),
        Collinear { a, .. } => ("C", *a),
        Fix { point } => ("F", *point),
        Midpoint { point, .. } => ("M", *point),
        Symmetric { a, .. } => ("S", *a),
        _ => return None,
    })
}

impl Workbench {
    pub(crate) fn enter_sketch(&mut self, feature: FeatureId) -> Result<(), String> {
        if self.document().sketch(feature).is_none() {
            return Err(format!("{feature} is not a sketch"));
        }
        self.panel = None;
        let was_sketching = matches!(self.mode, Mode::Sketch(_));
        let base = self.document().sketch(feature).cloned();
        self.mode = Mode::Sketch(Box::new(SketchMode::new(feature, base)));
        self.chrome.tab = crate::commands::SKETCH_TAB;
        let before = self.view.anim.map_or(self.view.camera, |a| a.to);
        if let Some(f) = self.sketch_frame(feature) {
            // Look square at the sketch plane, X to the right, framing what is drawn.
            let mut to = self.square_to(&f);
            match self.sketch_box() {
                Some(b) if b.diagonal() > 1e-6 => to.fit(&b),
                _ => to.target = f.origin(),
            }
            self.animate_to(to);
        }
        if !was_sketching {
            self.view.sketch_return = Some(before);
        }
        self.set_status(format!("Editing {}. {}", self.feature_name(feature), Tool::Select.hint()));
        Ok(())
    }

    pub(crate) fn finish_sketch(&mut self) {
        if let Mode::Sketch(s) = &self.mode {
            let name = self.feature_name(s.feature);
            self.mode = Mode::Model;
            self.chrome.tab = crate::commands::MODEL_TAB;
            self.set_status(format!("Finished {name}. Extrude or revolve it from the 3D Model tab."));
            // Back to the view from before the sketch.
            if let Some(c) = self.view.sketch_return.take() {
                self.animate_to(c);
            }
        }
    }

    pub(crate) fn active_tool_id(&self) -> Option<&'static str> {
        if self.in_drawing() {
            return self.drw.as_ref().and_then(|d| d.tool.as_ref()).map(crate::drawing::DrwTool::id);
        }
        match &self.mode {
            Mode::Sketch(s) => s.tool.id(),
            Mode::Model => None,
        }
    }

    pub(crate) fn sketch_tool(&mut self, id: &str) -> Result<(), String> {
        if !matches!(self.mode, Mode::Sketch(_)) {
            // A sketch tool outside a sketch starts one first, then the tool runs in it.
            self.pending_tool = TOOLS.iter().find(|(t, _)| *t == id).map(|(t, _)| *t);
            return self.run_ui("sketch.new");
        }
        let Mode::Sketch(s) = &mut self.mode else { return Ok(()) };
        if id == "sketch.construction" {
            let (feature, picked) = (s.feature, s.selection.clone());
            if picked.is_empty() {
                return Err("select the sketch geometry to change first".into());
            }
            let sk = self.document().sketch(feature).ok_or("the sketch is gone")?;
            let on = !picked.iter().all(|e| sk.entity(*e).is_some_and(|x| x.construction));
            for e in picked {
                self.exec("sketch.construction", json!({ "sketch": feature.0, "entity": e.0, "on": on }))?;
            }
            self.set_status(if on { "Construction geometry (not part of profiles)." } else { "Normal geometry." });
            return Ok(());
        }
        if id == "sketch.offset" {
            if s.selection.is_empty() {
                return Err("select the curves to offset first".into());
            }
            let (sketch, curves) = (s.feature, s.selection.clone());
            self.panel =
                Some(Panel::Value(ValuePanel::new("Offset", "Distance (mm, negative goes inward)", 2.0, ValueFor::Offset { sketch, curves })));
            return Ok(());
        }
        let tool = TOOLS.iter().find(|(t, _)| *t == id).map(|(_, tool)| *tool).ok_or_else(|| format!("unknown command `{id}`"))?;
        if tool == Tool::Mirror && s.selection.is_empty() {
            return Err("select the geometry to mirror first".into());
        }
        s.tool = if s.tool == tool { Tool::Select } else { tool };
        s.clicks.clear();
        s.picks.clear();
        let hint = s.tool.hint();
        self.set_status(hint);
        Ok(())
    }

    pub(crate) fn sketch_dof_text(&mut self) -> Option<String> {
        let rev = self.session.revision();
        let Mode::Sketch(s) = &mut self.mode else { return None };
        if s.dof.as_ref().is_none_or(|(r, _, _)| *r != rev) {
            let d = self.session.document().sketch(s.feature).map(Sketch::dof);
            s.dof = Some(match d {
                Some(Ok(d)) => (rev, Some(d.dof), d.fully_constrained),
                _ => (rev, None, BTreeSet::new()),
            });
        }
        match s.dof.as_ref() {
            Some((_, Some(0), _)) => Some("Fully Constrained".into()),
            Some((_, Some(1), _)) => Some("1 dimension needed".into()),
            Some((_, Some(n), _)) => Some(format!("{n} dimensions needed")),
            _ => Some("Over-constrained or inconsistent".into()),
        }
    }

    /// Bounding box of the active sketch in 3D.
    pub(crate) fn sketch_box(&self) -> Option<Aabb3> {
        let Mode::Sketch(s) = &self.mode else { return None };
        let frame = self.sketch_frame(s.feature)?;
        let sk = self.document().sketch(s.feature)?;
        Aabb3::from_points(sk.entities().filter_map(|(id, _)| sk.point(id)).map(|p| frame.plane_point(p)))
    }

    pub(crate) fn sketch_ui(&mut self, ui: &Ui, resp: &egui::Response, rect: Rect, t: &Tokens) {
        let Mode::Sketch(sm) = &self.mode else { return };
        let feature = sm.feature;
        let Some(frame) = self.sketch_frame(feature) else {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Waiting for the sketch plane (the face it sits on must regenerate).",
                theme::body(),
                t.text_dim,
            );
            return;
        };
        let Some(doc_sketch) = self.document().sketch(feature).cloned() else {
            self.mode = Mode::Model;
            return;
        };
        let plane = Plane { frame, cam: self.view.camera, rect };
        let pointer = resp.hover_pos().or_else(|| resp.interact_pointer_pos());
        let raw_cursor = pointer.and_then(|p| plane.unproject(p));
        let hover = pointer.and_then(|p| hit_test(&doc_sketch, &plane, p));
        let snap = hover.filter(|h| doc_sketch.is_point(*h));
        // Lines snap to horizontal or vertical near those directions (unless on a point).
        let (cursor, infer) = match (&self.mode, raw_cursor) {
            (Mode::Sketch(sm), Some(c)) if sm.tool == Tool::Line && snap.is_none() => match sm.clicks.last() {
                Some(start) => {
                    let (c, i) = infer_line(start.at, c);
                    (Some(c), i)
                }
                None => (Some(c), None),
            },
            _ => (raw_cursor, None),
        };
        // Dimensions driven by an equation read "fx: 20".
        let fx: std::collections::BTreeSet<ConstraintId> =
            doc_sketch.constraints().map(|(cid, _)| cid).filter(|cid| self.dimension_equation(feature, *cid).is_some()).collect();
        let label_text = |cid: ConstraintId, text: String| if fx.contains(&cid) { format!("fx: {text}") } else { text };
        let labels: Vec<(Rect, ConstraintId, Constraint)> = doc_sketch
            .constraints()
            .filter_map(|(cid, c)| {
                let (at, text) = dimension_label(&doc_sketch, c)?;
                let text = label_text(cid, text);
                let p = plane.project(at)?;
                let galley = ui.painter().layout_no_wrap(text, theme::small(), Color32::WHITE);
                Some((Rect::from_center_size(p, galley.size() + vec2(8.0, 4.0)), cid, c.clone()))
            })
            .collect();

        // ---- input ----
        if let Mode::Sketch(sm) = &mut self.mode {
            sm.hover = hover;
        }
        let (mut enter, mut esc, delete) =
            ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Delete)));
        let typing = ui.ctx().egui_wants_keyboard_input();
        // Value boxes: once a drawing tool has its first point, digits go into the boxes beside
        // the cursor; Tab moves between them, Enter places the geometry.
        if !typing
            && let Mode::Sketch(sm) = &mut self.mode
            && !sm.clicks.is_empty()
            && !hud_fields(sm.tool).is_empty()
        {
            let n = hud_fields(sm.tool).len();
            let (text, tab, back) = ui.input(|i| {
                let text: String = i
                    .events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::Text(t) => Some(t.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect::<String>()),
                        _ => None,
                    })
                    .collect();
                (text, i.key_pressed(egui::Key::Tab), i.key_pressed(egui::Key::Backspace))
            });
            let field = sm.hud.active.min(n - 1);
            if let Some(s) = sm.hud.fields.get_mut(field) {
                s.push_str(&text);
                if back {
                    s.pop();
                }
            }
            if tab {
                sm.hud.active = (field + 1) % n;
            }
            if esc && !sm.hud.is_empty() {
                // Esc first clears what was typed, then ends the tool as usual.
                sm.hud = Hud::default();
                esc = false;
            }
            if enter && !sm.hud.is_empty() {
                enter = false;
                if let Some(c) = cursor {
                    self.hud_commit(feature, c);
                }
            }
        }
        if !typing && esc {
            self.sketch_escape();
        }
        if !typing && enter {
            self.finish_spline(feature);
        }
        if !typing && delete {
            self.delete_selection(feature);
        }
        // Double-clicking a dimension edits it.
        if resp.double_clicked()
            && let Some(p) = pointer
            && let Some((_, cid, c)) = labels.iter().find(|(r, _, _)| r.contains(p))
        {
            let equation = self.dimension_equation(feature, *cid);
            self.panel =
                Some(Panel::EditDimension { sketch: feature, constraint: *cid, value: c.value().unwrap_or(0.0), angular: c.is_angular(), equation });
        } else if resp.clicked_by(egui::PointerButton::Primary)
            && let Some(at) = cursor
        {
            let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
            self.sketch_click(feature, &doc_sketch, Click { at, point: snap, infer }, hover, add);
        }
        // Dragging a point (select tool): preview on a copy, one command on release.
        let tool = match &self.mode {
            Mode::Sketch(sm) => sm.tool,
            Mode::Model => Tool::Select,
        };
        if tool == Tool::Select {
            // Pick the point where the button went down: a drag is only recognised once the
            // pointer has moved, and a quick flick can already be outside the snap radius.
            if resp.drag_started_by(egui::PointerButton::Primary)
                && let Some(origin) = ui.input(|i| i.pointer.press_origin())
                && let Some(p) = hit_test(&doc_sketch, &plane, origin).filter(|h| doc_sketch.is_point(*h))
                && let Mode::Sketch(sm) = &mut self.mode
            {
                sm.drag = Some((p, doc_sketch.clone()));
            }
            if let (Some(at), Mode::Sketch(sm)) = (cursor, &mut self.mode)
                && let Some((p, preview)) = &mut sm.drag
            {
                let mut next = doc_sketch.clone();
                if next.drag(*p, at).is_ok() {
                    *preview = next;
                }
            }
            if resp.drag_stopped()
                && let Mode::Sketch(sm) = &mut self.mode
                && let Some((p, _)) = sm.drag.take()
                && let Some(at) = cursor
            {
                let _ = self.exec_status("sketch.drag", json!({ "sketch": feature.0, "point": p.0, "x": at.x, "y": at.y }));
            }
        }

        // ---- drawing ----
        let Mode::Sketch(sm) = &self.mode else { return };
        let shown = sm.drag.as_ref().map_or(&doc_sketch, |(_, s)| s);
        let fully = sm.dof.as_ref().map(|d| d.2.clone()).unwrap_or_default();
        let p = ui.painter();
        let under = Color32::from_rgb(0x5c, 0xd0, 0xc8);
        let done = Color32::from_rgb(0xe8, 0xec, 0xf0);
        let picked = Color32::from_rgb(0xf0, 0xa0, 0x3c);
        let hot = Color32::from_rgb(0xff, 0xe0, 0x80);
        let color_of = |id: EntityId| {
            if sm.selection.contains(&id) || sm.picks.contains(&id) {
                picked
            } else if sm.hover == Some(id) {
                hot
            } else if fully.contains(&id) {
                done
            } else {
                under
            }
        };
        draw_grid(ui, &plane, rect, t);
        for (id, e) in shown.entities() {
            if e.geometry.is_point() {
                continue;
            }
            let pts: Vec<Pos2> = shown.tessellate(id, 0.02).into_iter().filter_map(|v| plane.project(v)).collect();
            if pts.len() < 2 {
                continue;
            }
            if e.construction {
                p.extend(Shape::dashed_line(&pts, Stroke::new(1.0, t.text_dim), 6.0, 4.0));
            } else {
                p.add(Shape::line(pts, Stroke::new(if sm.hover == Some(id) { 2.5 } else { 1.8 }, color_of(id))));
            }
        }
        for (id, e) in shown.entities() {
            if let Geometry::Point { pos } = e.geometry
                && let Some(q) = plane.project(pos)
            {
                let s = if sm.hover == Some(id) { 4.0 } else { 2.5 };
                if e.construction {
                    // Projected geometry (the part origin): a round amber dot.
                    p.circle_filled(q, s + 1.0, if sm.hover == Some(id) { hot } else { t.tint_work });
                } else {
                    p.rect_filled(Rect::from_center_size(q, vec2(s * 2.0, s * 2.0)), 1.0, color_of(id));
                }
            }
        }
        // Constraint glyphs next to their first entity.
        for (_, c) in shown.constraints() {
            if let Some((g, on)) = glyph(c) {
                let anchor = shown
                    .line(on)
                    .map(|(a, b)| a.mid(b))
                    .or_else(|| shown.circle(on).map(|(c, r)| c + Vec2::new(0.0, r)))
                    .or_else(|| shown.point(on));
                if let Some(q) = anchor.and_then(|a| plane.project(a)) {
                    let r = Rect::from_center_size(q + vec2(10.0, -10.0), vec2(16.0, 13.0));
                    p.rect_filled(r, 2.0, t.panel.gamma_multiply(0.9));
                    p.text(r.center(), Align2::CENTER_CENTER, g, theme::small(), t.text_dim);
                }
            }
        }
        for (r, cid, c) in &labels {
            if let Some((_, text)) = dimension_label(shown, c) {
                let text = label_text(*cid, text);
                p.rect_filled(*r, 2.0, t.panel.gamma_multiply(0.92));
                p.text(r.center(), Align2::CENTER_CENTER, text, theme::small(), done);
            }
        }
        // Tool preview from the pending clicks to the pointer.
        if let (Some(cur), Some(last)) = (cursor, sm.clicks.last()) {
            let pv = |a: Vec2, b: Vec2| -> Option<[Pos2; 2]> { Some([plane.project(a)?, plane.project(b)?]) };
            let stroke = Stroke::new(1.2, under.gamma_multiply(0.8));
            match sm.tool {
                Tool::Line | Tool::Spline => {
                    if let Some(seg) = pv(last.at, cur) {
                        p.line_segment(seg, stroke);
                    }
                }
                Tool::Circle | Tool::Polygon => {
                    let r = last.at.dist(cur);
                    let ring: Vec<Pos2> =
                        (0..=64).filter_map(|i| plane.project(Vec2::polar(last.at, r, std::f64::consts::TAU * f64::from(i) / 64.0))).collect();
                    p.add(Shape::line(ring, stroke));
                }
                Tool::Rectangle => {
                    let (a, b) = (last.at, cur);
                    let corners = [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y), a];
                    let pts: Vec<Pos2> = corners.iter().filter_map(|c| plane.project(*c)).collect();
                    p.add(Shape::line(pts, stroke));
                }
                Tool::Arc => {
                    if let [s, e] = sm.clicks.as_slice()
                        && let Some(arc) = tenon_geom::Arc::from_3_points(s.at, cur, e.at)
                    {
                        let mut v = Vec::new();
                        arc.tessellate(0.05, &mut v);
                        p.add(Shape::line(v.into_iter().filter_map(|q| plane.project(q)).collect(), stroke));
                    } else if let Some(seg) = pv(last.at, cur) {
                        p.line_segment(seg, stroke);
                    }
                }
                _ => {}
            }
        }
        if let Some(q) = snap.and_then(|s| shown.point(s)).and_then(|v| plane.project(v)) {
            p.circle_stroke(q, 7.0, Stroke::new(1.5, hot));
        }
        // The inferred constraint, beside the cursor.
        if let (Some(i), Some(q)) = (infer.filter(|_| !sm.clicks.is_empty()), cursor.and_then(|c| plane.project(c))) {
            let r = Rect::from_min_size(q + vec2(10.0, -26.0), vec2(16.0, 16.0));
            p.rect_filled(r, 2.0, t.panel.gamma_multiply(0.9));
            crate::icons::paint(
                p,
                r.shrink(2.0),
                if i == Infer::Horizontal { crate::icons::Icon::Horizontal } else { crate::icons::Icon::Vertical },
                hot,
            );
        }
        // Value boxes beside the cursor: typed values, else the live ones.
        let fields = hud_fields(sm.tool);
        if let (Some(first), Some(cur), Some(q)) = (sm.clicks.last(), cursor, cursor.and_then(|c| plane.project(c)))
            && !fields.is_empty()
        {
            let d = cur - first.at;
            let live = match sm.tool {
                Tool::Line => [d.len(), d.y.atan2(d.x).to_degrees()],
                Tool::Circle => [2.0 * d.len(), 0.0],
                _ => [d.x.abs(), d.y.abs()],
            };
            let mut at = q + vec2(18.0, 14.0);
            for (i, (name, unit)) in fields.iter().enumerate() {
                let typed = sm.hud.fields.get(i).filter(|s| !s.is_empty());
                let text = match typed {
                    Some(s) => format!("{s} {unit}"),
                    None => format!("{} {unit}", fmt_len(live[i])),
                };
                let galley = p.layout_no_wrap(text, theme::small(), if typed.is_some() { t.text } else { t.text_dim });
                let r = Rect::from_min_size(at, vec2(galley.size().x.max(46.0) + 10.0, 18.0));
                p.rect_filled(r, 2.0, t.field);
                let active = i == sm.hud.active.min(fields.len() - 1);
                p.rect_stroke(r, 2.0, Stroke::new(1.0, if active { t.accent } else { t.border }), egui::StrokeKind::Inside);
                p.galley(r.min + vec2(5.0, 3.0), galley, t.text);
                let _ = name;
                at.x = r.right() + 4.0;
            }
        }
    }

    /// The equation driving a dimension, if any.
    pub(crate) fn dimension_equation(&self, sketch: FeatureId, constraint: ConstraintId) -> Option<String> {
        let path = tenon_model::ValuePath::Dimension { sketch, constraint };
        let name = self.document().name_of(&path)?;
        self.document().parameters().model.iter().find(|m| m.name == name)?.equation.clone()
    }

    /// Where a dimension's label is on screen (for the inline edit box).
    pub(crate) fn dimension_anchor(&self, sketch: FeatureId, c: &Constraint) -> Option<Pos2> {
        let frame = self.sketch_frame(sketch)?;
        let sk = self.document().sketch(sketch)?;
        let (at, _) = dimension_label(sk, c)?;
        Plane { frame, cam: self.view.camera, rect: self.view.rect }.project(at)
    }

    /// Adds the horizontal or vertical constraint inferred while drawing a line. Inference is a
    /// convenience: if the sketch refuses it (redundant, conflicting), the line stays as drawn.
    fn add_inferred(&mut self, sketch: u32, line: u64, infer: Option<Infer>) {
        let c = match infer {
            Some(Infer::Horizontal) => json!({ "type": "horizontal", "line": line }),
            Some(Infer::Vertical) => json!({ "type": "vertical", "line": line }),
            None => return,
        };
        let _ = self.exec("sketch.constrain", json!({ "sketch": sketch, "constraint": c }));
    }

    /// Ties the rectangle corner at `at` to the existing point `to` (a rectangle started on the
    /// projected origin stays on it).
    fn attach_corner(&mut self, feature: FeatureId, lines: &[u64], at: Vec2, to: EntityId) {
        let Some(sk) = self.document().sketch(feature) else { return };
        let corner = lines
            .iter()
            .filter_map(|l| u32::try_from(*l).ok())
            .filter_map(|l| match sk.geometry(EntityId(l)) {
                Some(Geometry::Line { start, .. }) => Some(*start),
                _ => None,
            })
            .find(|p| sk.point(*p).is_some_and(|q| q.dist(at) < 1e-6));
        if let Some(c) = corner {
            let _ = self.exec("sketch.constrain", json!({ "sketch": feature.0, "constraint": { "type": "coincident", "a": to.0, "b": c.0 } }));
        }
    }

    /// Places what the value boxes describe, from the tool's first point towards the cursor.
    fn hud_commit(&mut self, feature: FeatureId, cursor: Vec2) {
        let Mode::Sketch(sm) = &mut self.mode else { return };
        let hud = std::mem::take(&mut sm.hud);
        let (tool, Some(first)) = (sm.tool, sm.clicks.last().copied()) else { return };
        let f = feature.0;
        let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
        match tool {
            Tool::Line => {
                let d = cursor - first.at;
                let len = hud.typed(0).unwrap_or(d.len());
                let angle = hud.typed(1).map_or(d.y.atan2(d.x), f64::to_radians);
                if len.is_nan() || len <= 0.0 {
                    self.set_error("a line needs a length above zero");
                    return;
                }
                let end = first.at + Vec2::from_angle(angle) * len;
                let mut m = Map::new();
                m.insert("sketch".into(), json!(f));
                put_point(&mut m, "start", "x1", "y1", &first);
                m.insert("x2".into(), json!(end.x));
                m.insert("y2".into(), json!(end.y));
                let Some(r) = self.exec_status("sketch.line", Value::Object(m)) else { return };
                let (line, end_id) = (r["line"].as_u64(), r["end"].as_u64().and_then(|v| u32::try_from(v).ok()));
                if let Some(l) = line {
                    if hud.typed(0).is_some() {
                        let _ = self.exec("sketch.constrain", json!({ "sketch": f, "constraint": { "type": "length", "line": l, "value": len } }));
                    }
                    if let Some(a) = hud.typed(1) {
                        let a = a.rem_euclid(180.0);
                        let infer = if a.abs() < 1e-9 || (a - 180.0).abs() < 1e-9 {
                            Some(Infer::Horizontal)
                        } else if (a - 90.0).abs() < 1e-9 {
                            Some(Infer::Vertical)
                        } else {
                            None
                        };
                        self.add_inferred(f, l, infer);
                    }
                }
                if let Mode::Sketch(sm) = &mut self.mode {
                    sm.clicks = end_id.map(|e| vec![Click { at: end, point: Some(EntityId(e)), infer: None }]).unwrap_or_default();
                }
            }
            Tool::Circle => {
                let r = hud.typed(0).map_or(first.at.dist(cursor), |d| d / 2.0);
                let mut m = Map::new();
                m.insert("sketch".into(), json!(f));
                put_point(&mut m, "center", "cx", "cy", &first);
                m.insert("r".into(), json!(r));
                if let Some(res) = self.exec_status("sketch.circle", Value::Object(m))
                    && let (Some(c), Some(d)) = (res["circle"].as_u64(), hud.typed(0))
                {
                    let _ = self.exec("sketch.constrain", json!({ "sketch": f, "constraint": { "type": "diameter", "curve": c, "value": d } }));
                }
                if let Mode::Sketch(sm) = &mut self.mode {
                    sm.clicks.clear();
                }
            }
            Tool::Rectangle => {
                let d = cursor - first.at;
                let w = hud.typed(0).map_or(d.x, |w| w.abs() * sign(d.x));
                let h = hud.typed(1).map_or(d.y, |h| h.abs() * sign(d.y));
                let b = first.at + Vec2::new(w, h);
                if let Some(res) =
                    self.exec_status("sketch.rectangle", json!({ "sketch": f, "x1": first.at.x, "y1": first.at.y, "x2": b.x, "y2": b.y }))
                {
                    let lines: Vec<u64> = res["lines"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
                    if let (Some(l), true) = (lines.first(), hud.typed(0).is_some()) {
                        let _ =
                            self.exec("sketch.constrain", json!({ "sketch": f, "constraint": { "type": "length", "line": l, "value": w.abs() } }));
                    }
                    if let (Some(l), true) = (lines.get(1), hud.typed(1).is_some()) {
                        let _ =
                            self.exec("sketch.constrain", json!({ "sketch": f, "constraint": { "type": "length", "line": l, "value": h.abs() } }));
                    }
                    if let Some(p) = first.point {
                        self.attach_corner(feature, &lines, first.at, p);
                    }
                }
                if let Mode::Sketch(sm) = &mut self.mode {
                    sm.clicks.clear();
                }
            }
            _ => {}
        }
    }

    fn sketch_escape(&mut self) {
        if let Mode::Sketch(sm) = &mut self.mode {
            if !sm.clicks.is_empty() || !sm.picks.is_empty() {
                sm.clicks.clear();
                sm.picks.clear();
            } else if sm.tool != Tool::Select {
                sm.tool = Tool::Select;
                self.set_status(Tool::Select.hint());
            } else {
                sm.selection.clear();
            }
        }
    }

    fn finish_spline(&mut self, feature: FeatureId) {
        let Mode::Sketch(sm) = &mut self.mode else { return };
        if sm.tool != Tool::Spline || sm.clicks.len() < 2 {
            return;
        }
        let pts: Vec<[f64; 2]> = sm.clicks.iter().map(|c| [c.at.x, c.at.y]).collect();
        sm.clicks.clear();
        let degree = (pts.len() - 1).min(3);
        let _ = self.exec_status("sketch.spline", json!({ "sketch": feature.0, "points": pts, "degree": degree }));
    }

    fn delete_selection(&mut self, feature: FeatureId) {
        let Mode::Sketch(sm) = &mut self.mode else { return };
        if sm.selection.is_empty() {
            return;
        }
        let ids: Vec<u32> = sm.selection.drain(..).map(|e| e.0).collect();
        let _ = self.exec_status("sketch.delete", json!({ "sketch": feature.0, "entities": ids }));
    }

    pub(crate) fn sketch_click(&mut self, feature: FeatureId, sketch: &Sketch, click: Click, hover: Option<EntityId>, add: bool) {
        let Mode::Sketch(sm) = &mut self.mode else { return };
        let f = feature.0;
        match sm.tool {
            Tool::Select => match hover {
                Some(h) if add => {
                    if let Some(i) = sm.selection.iter().position(|x| *x == h) {
                        sm.selection.remove(i);
                    } else {
                        sm.selection.push(h);
                    }
                }
                Some(h) => sm.selection = vec![h],
                None => sm.selection.clear(),
            },
            Tool::Point => {
                let _ = self.exec_status("sketch.point", json!({ "sketch": f, "x": click.at.x, "y": click.at.y }));
            }
            Tool::Line => {
                if let Some(start) = sm.clicks.last().copied() {
                    let mut m = Map::new();
                    m.insert("sketch".into(), json!(f));
                    put_point(&mut m, "start", "x1", "y1", &start);
                    put_point(&mut m, "end", "x2", "y2", &click);
                    if let Some(r) = self.exec_status("sketch.line", Value::Object(m))
                        && let Some(end) = r["end"].as_u64().and_then(|v| u32::try_from(v).ok())
                    {
                        if let Some(line) = r["line"].as_u64() {
                            self.add_inferred(f, line, click.infer);
                        }
                        if let Mode::Sketch(sm) = &mut self.mode {
                            // Continue the chain from the new end point (stop on an existing point).
                            sm.clicks =
                                if click.point.is_some() { vec![] } else { vec![Click { at: click.at, point: Some(EntityId(end)), infer: None }] };
                            sm.hud = Hud::default();
                        }
                    }
                } else {
                    sm.clicks.push(click);
                }
            }
            Tool::Circle | Tool::Rectangle | Tool::Polygon => {
                let Some(first) = sm.clicks.first().copied() else {
                    sm.clicks.push(click);
                    return;
                };
                let tool = sm.tool;
                sm.clicks.clear();
                sm.hud = Hud::default();
                let (a, b) = (first.at, click.at);
                match tool {
                    Tool::Circle => {
                        let mut m = Map::new();
                        m.insert("sketch".into(), json!(f));
                        put_point(&mut m, "center", "cx", "cy", &first);
                        m.insert("r".into(), json!(a.dist(b)));
                        let _ = self.exec_status("sketch.circle", Value::Object(m));
                    }
                    Tool::Rectangle => {
                        if let Some(res) = self.exec_status("sketch.rectangle", json!({ "sketch": f, "x1": a.x, "y1": a.y, "x2": b.x, "y2": b.y }))
                            && let Some(p) = first.point
                        {
                            let lines: Vec<u64> = res["lines"].as_array().map(|x| x.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
                            self.attach_corner(feature, &lines, a, p);
                        }
                    }
                    _ => {
                        let _ = self.exec_status("sketch.polygon", json!({ "sketch": f, "cx": a.x, "cy": a.y, "x": b.x, "y": b.y, "sides": 6 }));
                    }
                }
            }
            Tool::Arc => {
                if sm.clicks.len() < 2 {
                    sm.clicks.push(click);
                    return;
                }
                let (s, e) = (sm.clicks[0].at, sm.clicks[1].at);
                sm.clicks.clear();
                let _ = self.exec_status(
                    "sketch.arc3",
                    json!({ "sketch": f, "x1": s.x, "y1": s.y, "x2": click.at.x, "y2": click.at.y, "x3": e.x, "y3": e.y }),
                );
            }
            Tool::Spline => {
                sm.clicks.push(click);
                self.set_status(format!(
                    "{} control points; Enter finishes.",
                    match &self.mode {
                        Mode::Sketch(sm) => sm.clicks.len(),
                        Mode::Model => 0,
                    }
                ));
            }
            Tool::Trim => match hover.filter(|h| !sketch.is_point(*h)) {
                Some(c) => {
                    let _ = self.exec_status("sketch.trim", json!({ "sketch": f, "curve": c.0, "x": click.at.x, "y": click.at.y }));
                }
                None => self.set_error("click a curve to trim"),
            },
            Tool::Fillet => match hover.filter(|h| sketch.is_point(*h)) {
                Some(pt) => {
                    self.panel =
                        Some(Panel::Value(ValuePanel::new("Sketch Fillet", "Radius (mm)", 2.0, ValueFor::Fillet { sketch: feature, point: pt })))
                }
                None => self.set_error("click the corner point"),
            },
            Tool::Mirror => match hover.filter(|h| sketch.is_line(*h)) {
                Some(axis) => {
                    let ids: Vec<u32> = sm.selection.iter().map(|e| e.0).collect();
                    sm.tool = Tool::Select;
                    let _ = self.exec_status("sketch.mirror", json!({ "sketch": f, "entities": ids, "axis": axis.0 }));
                }
                None => self.set_error("click the mirror line"),
            },
            Tool::Dimension => self.dimension_click(feature, sketch, hover),
            t => self.constraint_click(feature, sketch, t, hover),
        }
    }

    pub(crate) fn dimension_click(&mut self, feature: FeatureId, s: &Sketch, hover: Option<EntityId>) {
        let Mode::Sketch(sm) = &mut self.mode else { return };
        let picks = sm.picks.clone();
        let c = match (picks.as_slice(), hover) {
            ([l], None) if kind(s, *l) == Kind::Line => Some(Constraint::Length { line: *l, value: 0.0 }),
            ([], Some(h)) if kind(s, h) == Kind::Round => Some(match s.geometry(h) {
                Some(Geometry::Circle { .. }) => Constraint::Diameter { curve: h, value: 0.0 },
                _ => Constraint::Radius { curve: h, value: 0.0 },
            }),
            ([], Some(h)) if matches!(kind(s, h), Kind::Line | Kind::Point) => {
                sm.picks.push(h);
                None
            }
            ([l], Some(h)) if *l == h && kind(s, h) == Kind::Line => Some(Constraint::Length { line: h, value: 0.0 }),
            ([a], Some(b)) => match (kind(s, *a), kind(s, b)) {
                (Kind::Point, Kind::Point) | (Kind::Point, Kind::Line) => Some(Constraint::Distance { a: *a, b, value: 0.0 }),
                (Kind::Line, Kind::Point) => Some(Constraint::Distance { a: b, b: *a, value: 0.0 }),
                (Kind::Line, Kind::Line) => {
                    let parallel = match (s.line(*a), s.line(b)) {
                        (Some((p, q)), Some((r, t))) => (q - p).normalized().cross((t - r).normalized()).abs() < 1e-6,
                        _ => false,
                    };
                    if parallel {
                        s.geometry(b).and_then(|g| g.points().first().copied()).map(|pt| Constraint::Distance { a: pt, b: *a, value: 0.0 })
                    } else {
                        Some(Constraint::Angle { a: *a, b, value: 0.0 })
                    }
                }
                _ => None,
            },
            _ => None,
        };
        let Some(mut c) = c else { return };
        if let Mode::Sketch(sm) = &mut self.mode {
            sm.picks.clear();
        }
        let measured = s.measure(&c).unwrap_or(1.0);
        let value = if matches!(c, Constraint::HorizontalDistance { .. } | Constraint::VerticalDistance { .. }) { measured } else { measured.abs() };
        c.set_value(value);
        let (title, label) = if c.is_angular() { ("Angle", "Degrees") } else { ("Dimension", "Millimetres") };
        let shown = if c.is_angular() { value.to_degrees() } else { value };
        self.panel = Some(Panel::Value(ValuePanel::new(title, label, shown, ValueFor::Dimension { sketch: feature, constraint: c })));
    }

    fn constraint_click(&mut self, feature: FeatureId, s: &Sketch, tool: Tool, hover: Option<EntityId>) {
        let Some(h) = hover else {
            self.set_error(tool.hint());
            return;
        };
        let Mode::Sketch(sm) = &mut self.mode else { return };
        sm.picks.push(h);
        let picks = sm.picks.clone();
        let c = match (tool, picks.as_slice()) {
            (Tool::Horizontal, [l]) => Some(Constraint::Horizontal { line: *l }),
            (Tool::Vertical, [l]) => Some(Constraint::Vertical { line: *l }),
            (Tool::Fix, [p]) => Some(Constraint::Fix { point: *p }),
            (Tool::Coincident, [a, b]) => Some(match (kind(s, *a), kind(s, *b)) {
                (Kind::Point, Kind::Point) => Constraint::Coincident { a: *a, b: *b },
                (Kind::Point, _) => Constraint::PointOnCurve { point: *a, curve: *b },
                _ => Constraint::PointOnCurve { point: *b, curve: *a },
            }),
            (Tool::Parallel, [a, b]) => Some(Constraint::Parallel { a: *a, b: *b }),
            (Tool::Perpendicular, [a, b]) => Some(Constraint::Perpendicular { a: *a, b: *b }),
            (Tool::Tangent, [a, b]) => Some(Constraint::Tangent { a: *a, b: *b }),
            (Tool::Equal, [a, b]) => Some(Constraint::Equal { a: *a, b: *b }),
            (Tool::Concentric, [a, b]) => Some(Constraint::Concentric { a: *a, b: *b }),
            (Tool::Collinear, [a, b]) => Some(Constraint::Collinear { a: *a, b: *b }),
            (Tool::Symmetric, [a, b, axis]) => Some(Constraint::Symmetric { a: *a, b: *b, axis: *axis }),
            _ => None,
        };
        let Some(c) = c else { return };
        sm.picks.clear();
        let value = serde_json::to_value(&c).unwrap_or(Value::Null);
        if self.exec_status("sketch.constrain", json!({ "sketch": feature.0, "constraint": value })).is_some() {
            self.set_status(format!("Added {}.", c.name().replace('_', " ")));
        }
    }
}
