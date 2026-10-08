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
}

const TOOLS: &[(&str, Tool)] = &[
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
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Click {
    pub at: Vec2,
    pub point: Option<EntityId>,
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
    dof: Option<(u64, Option<usize>, BTreeSet<EntityId>)>,
}

impl SketchMode {
    fn new(feature: FeatureId) -> Self {
        SketchMode { feature, tool: Tool::Select, clicks: Vec::new(), picks: Vec::new(), selection: Vec::new(), hover: None, drag: None, dof: None }
    }
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
        self.mode = Mode::Sketch(Box::new(SketchMode::new(feature)));
        self.chrome.tab = 1;
        if let Some(f) = self.sketch_frame(feature) {
            self.look_at_frame(&f);
            match self.sketch_box() {
                Some(b) if b.diagonal() > 1e-6 => self.view.camera.fit(&b),
                _ => self.view.camera.target = f.origin(),
            }
        }
        self.set_status(format!("Editing {}. {}", self.feature_name(feature), Tool::Select.hint()));
        Ok(())
    }

    pub(crate) fn finish_sketch(&mut self) {
        if let Mode::Sketch(s) = &self.mode {
            let name = self.feature_name(s.feature);
            self.mode = Mode::Model;
            self.chrome.tab = 0;
            self.set_status(format!("Finished {name}. Extrude or revolve it from the Model tab."));
        }
    }

    pub(crate) fn active_tool_id(&self) -> Option<&'static str> {
        match &self.mode {
            Mode::Sketch(s) => s.tool.id(),
            Mode::Model => None,
        }
    }

    pub(crate) fn sketch_tool(&mut self, id: &str) -> Result<(), String> {
        let Mode::Sketch(s) = &mut self.mode else {
            return Err("start a sketch (New Sketch) or double-click one in the browser first".into());
        };
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
            Some((_, Some(0), _)) => Some("Fully constrained".into()),
            Some((_, Some(n), _)) => Some(format!("{n} degrees of freedom")),
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
        let cursor = pointer.and_then(|p| plane.unproject(p));
        let hover = pointer.and_then(|p| hit_test(&doc_sketch, &plane, p));
        let snap = hover.filter(|h| doc_sketch.is_point(*h));
        let labels: Vec<(Rect, ConstraintId, Constraint)> = doc_sketch
            .constraints()
            .filter_map(|(cid, c)| {
                let (at, text) = dimension_label(&doc_sketch, c)?;
                let p = plane.project(at)?;
                let galley = ui.painter().layout_no_wrap(text, theme::small(), Color32::WHITE);
                Some((Rect::from_center_size(p, galley.size() + vec2(8.0, 4.0)), cid, c.clone()))
            })
            .collect();

        // ---- input ----
        if let Mode::Sketch(sm) = &mut self.mode {
            sm.hover = hover;
        }
        let (enter, esc, delete) =
            ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Delete)));
        let typing = ui.ctx().egui_wants_keyboard_input();
        if !typing && esc {
            self.sketch_escape();
        }
        if !typing && enter {
            self.finish_spline(feature);
        }
        if !typing && delete {
            self.delete_selection(feature);
        }
        if resp.secondary_clicked()
            && let Mode::Sketch(sm) = &mut self.mode
        {
            sm.clicks.clear();
            sm.picks.clear();
        }
        // Double-clicking a dimension edits it.
        if resp.double_clicked()
            && let Some(p) = pointer
            && let Some((_, cid, c)) = labels.iter().find(|(r, _, _)| r.contains(p))
        {
            self.panel = Some(Panel::EditDimension { sketch: feature, constraint: *cid, value: c.value().unwrap_or(0.0), angular: c.is_angular() });
        } else if resp.clicked_by(egui::PointerButton::Primary)
            && let Some(at) = cursor
        {
            let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
            self.sketch_click(feature, &doc_sketch, Click { at, point: snap }, hover, add);
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
                p.rect_filled(Rect::from_center_size(q, vec2(s * 2.0, s * 2.0)), 1.0, color_of(id));
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
        for (r, _, c) in &labels {
            if let Some((_, text)) = dimension_label(shown, c) {
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
                        && let Mode::Sketch(sm) = &mut self.mode
                    {
                        // Continue the chain from the new end point (stop on an existing point).
                        sm.clicks = if click.point.is_some() { vec![] } else { vec![Click { at: click.at, point: Some(EntityId(end)) }] };
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
                let (a, b) = (first.at, click.at);
                let _ = match tool {
                    Tool::Circle => {
                        let mut m = Map::new();
                        m.insert("sketch".into(), json!(f));
                        put_point(&mut m, "center", "cx", "cy", &first);
                        m.insert("r".into(), json!(a.dist(b)));
                        self.exec_status("sketch.circle", Value::Object(m))
                    }
                    Tool::Rectangle => self.exec_status("sketch.rectangle", json!({ "sketch": f, "x1": a.x, "y1": a.y, "x2": b.x, "y2": b.y })),
                    _ => self.exec_status("sketch.polygon", json!({ "sketch": f, "cx": a.x, "cy": a.y, "x": b.x, "y": b.y, "sides": 6 })),
                };
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
