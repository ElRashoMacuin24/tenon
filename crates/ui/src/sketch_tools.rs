//! Sketch tools that reach beyond the sketch (DEC-055): the part's edges shown in it as
//! reference geometry, by the Project Geometry tool or from the start when a sketch is made on
//! a face; and the faces of a sheet metal part beyond a bend shown laid flat (Project Flat
//! Pattern, DEC-068). And the tools of the sketch's Modify and Pattern panels and its further shapes
//! (DEC-056): what a click does with each, what is shown before the click that commits it, and
//! the pattern window.

use std::collections::BTreeSet;

use egui::{Rect, Ui};
use serde_json::{Map, Value, json};
use tenon_geom::Vec2;
use tenon_model::project::{EdgeGeom, Projected, project};
use tenon_model::{EdgeRef, FaceRef, FeatureId};
use tenon_sketch::{EntityId, Motion, Sketch};

use crate::panels::Panel;
use crate::sketcher::{Click, Hud, SketchMode, Tool};
use crate::viewport::Pick;
use crate::workbench::{Mode, Workbench};

impl Workbench {
    /// What edge `edge` of shown body `body` is on the plane of `frame`, with its reference.
    fn edge_on(&self, body: usize, edge: u32, frame: &tenon_geom::Frame) -> Result<(EdgeRef, Projected), String> {
        let b = self.scene.bodies.get(body).ok_or("no such body")?;
        let i = edge as usize;
        let er = b.edge_ref(edge).ok_or("this edge cannot be referenced yet")?;
        let (Some(curve), Some(ends), Some(mid)) = (b.curves.get(i), b.ends.get(i), b.mids.get(i)) else { return Err("no such edge".into()) };
        Ok((er, project(&EdgeGeom { curve, ends: *ends, mid: *mid }, frame)?))
    }

    /// The edges round shown face `face` of `body` that can be shown on the plane of `frame`.
    fn face_edges_on(&self, body: usize, face: u32, frame: &tenon_geom::Frame) -> (Vec<EdgeRef>, Vec<Projected>) {
        let (mut edges, mut geometry) = (Vec::new(), Vec::new());
        let Some(b) = self.scene.bodies.get(body) else { return (edges, geometry) };
        let Some(Some(name)) = b.faces.get(face as usize).map(|f| f.0) else { return (edges, geometry) };
        for (i, (names, _)) in b.edges.iter().enumerate() {
            if !names.is_some_and(|n| n.contains(&name)) {
                continue;
            }
            if let Ok((er, p)) = self.edge_on(body, i as u32, frame) {
                edges.push(er);
                geometry.push(p);
            }
        }
        (edges, geometry)
    }

    /// For a new sketch on `face`: the face's edges and what each is on the face's own plane
    /// (Application Options: Autoproject edges for sketch creation and edit).
    pub(crate) fn face_outline(&self, face: &FaceRef) -> Option<(Vec<EdgeRef>, Vec<Projected>)> {
        for (bi, b) in self.scene.bodies.iter().enumerate() {
            // (Pieces of one named face lie in one plane: the first found gives it.)
            let Some(fi) = b.faces.iter().position(|(n, _)| n.as_ref() == Some(&face.origin)) else { continue };
            let frame = tenon_model::frame_on_face(&b.faces[fi].1.surface)?;
            return Some(self.face_edges_on(bi, fi as u32, &frame));
        }
        None
    }

    /// The Project Geometry tool: the part's edge (or face) under the pointer lights up, and a
    /// click shows it in the sketch. With `flat` it is Project Flat Pattern: a face of the sheet
    /// is shown where it lies with the sheet laid flat from the sketch's face. Returns true
    /// while the tool has the pointer.
    pub(crate) fn project_tool(&mut self, ui: &Ui, resp: &egui::Response, rect: Rect, feature: FeatureId, flat: bool) -> bool {
        let Some(frame) = self.sketch_frame(feature) else { return true };
        let (meshes, tree) = (self.scene.placed(), self.body_tree());
        let under = resp.hover_pos().and_then(|p| {
            let (x, y, w, h) = (f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
            if !flat && let Some(e) = tenon_render::pick::pick_edge_in(tree.as_deref(), &meshes, &self.view.camera, w, h, x, y, 6.0) {
                return Some(Pick::Edge { body: e.body, edge: e.edge });
            }
            let (o, d) = self.view.camera.ray(x, y, w, h);
            tenon_render::pick::pick_face_in(tree.as_deref(), &meshes, o, d).map(|f| Pick::Face { body: f.body, face: f.face })
        });
        self.view.hover = under;
        if !resp.clicked_by(egui::PointerButton::Primary) {
            return true;
        }
        let _ = ui;
        if flat {
            match under {
                Some(Pick::Face { body, face }) => match self.face_ref_of(body, face) {
                    Some(face) => self.project_flat(feature, face),
                    None => self.set_error("That face cannot be referred to yet, so it cannot be shown laid flat."),
                },
                _ => self.set_error("Project Flat Pattern: click a face of the sheet, beyond a bend from the face the sketch is on."),
            }
            return true;
        }
        let found = match under {
            Some(Pick::Edge { body, edge }) => self.edge_on(body, edge, &frame).map(|(e, p)| (vec![e], vec![p])),
            Some(Pick::Face { body, face }) => match self.face_edges_on(body, face, &frame) {
                (edges, _) if edges.is_empty() => Err("none of that face's edges can be shown in this sketch".into()),
                found => Ok(found),
            },
            None => Err("click an edge of the part, or a face for all its edges".into()),
        };
        match found {
            Ok((edges, geometry)) => {
                let n = edges.len();
                if self.exec_status("sketch.project", json!({ "sketch": feature.0, "edges": edges, "geometry": geometry })).is_some() {
                    self.set_status(if n == 1 {
                        "Projected: the edge is reference geometry in the sketch, and follows the part.".to_owned()
                    } else {
                        format!("Projected {n} edges: they are reference geometry in the sketch, and follow the part.")
                    });
                }
            }
            Err(e) => self.set_error(format!("That cannot be projected: {e}.").replace("..", ".")),
        }
        true
    }

    /// Project Flat Pattern: the edges of `face` shown in sketch `sketch` where they lie with
    /// the sheet laid flat from the sketch's face. Worked out with the kernel, wherever that is.
    fn project_flat(&mut self, sketch: FeatureId, face: FaceRef) {
        let job = crate::print_dialog::PartJob::FlatEdges { sketch };
        let asked = self.part_job(job, move |k, _, doc| tenon_model::cmd::projected(doc, k, sketch, Vec::new(), Some(&face), true));
        if let Err(e) = asked {
            self.set_error(format!("Project Flat Pattern: {e}"));
        }
    }

    /// The edges of a face laid flat arrived: those that can be shown go into the sketch.
    pub(crate) fn flat_edges_done(&mut self, sketch: FeatureId, result: Box<dyn std::any::Any + Send>) {
        type Found = Vec<(EdgeRef, Result<Projected, String>)>;
        let found = result.downcast::<Result<Found, String>>().map_or_else(|_| Err("internal: unexpected result".into()), |b| *b);
        let found = match found {
            Ok(found) => found,
            Err(e) => return self.set_error(format!("That cannot be shown laid flat: {e}.").replace("..", ".")),
        };
        let why = found.iter().find_map(|(_, at)| at.as_ref().err().cloned());
        let (edges, geometry): (Vec<EdgeRef>, Vec<Projected>) = found.into_iter().filter_map(|(e, at)| Some((e, at.ok()?))).unzip();
        if edges.is_empty() {
            let why = why.unwrap_or_else(|| "it has no edges that can be referred to".into());
            return self.set_error(format!("That face cannot be shown laid flat: {why}.").replace("..", "."));
        }
        let n = edges.len();
        if self.exec_status("sketch.project", json!({ "sketch": sketch.0, "edges": edges, "geometry": geometry, "flat": true })).is_some() {
            self.set_status(format!(
                "Laid flat: {n} edges of the face are reference geometry in the sketch, where the sheet has them when it is unfolded. Draw on them for a cut across the bend."
            ));
        }
    }
}

/// The tools this file runs (the sketch's Modify and Pattern panels and its further shapes).
pub(crate) fn is_tool(t: Tool) -> bool {
    use Tool::*;
    matches!(
        t,
        Move | Copy
            | Rotate
            | Scale
            | Extend
            | Split
            | PatternRect
            | PatternCircular
            | Slot
            | SlotOverall
            | SlotCenter
            | RectCenter
            | Rect3
            | ArcCenter
            | ArcTangent
    )
}

/// The tools that work on what is selected.
pub(crate) fn needs_selection(t: Tool) -> Option<&'static str> {
    Some(match t {
        Tool::Move => "move",
        Tool::Copy => "copy",
        Tool::Rotate => "rotate",
        Tool::Scale => "scale",
        Tool::PatternRect | Tool::PatternCircular => "pattern",
        _ => return None,
    })
}

/// A sketch pattern being set up: what to repeat, how, and how many.
#[derive(Clone, Debug)]
pub(crate) struct SketchPatternPanel {
    pub sketch: FeatureId,
    pub entities: Vec<EntityId>,
    /// Rectangular: the line the first direction runs along. Circular: none.
    pub along: Option<EntityId>,
    /// Circular: the point to pattern round.
    pub center: Vec2,
    pub count: u32,
    pub spacing: f64,
    pub count2: u32,
    pub spacing2: f64,
    /// Circular: the angle the pattern covers, degrees.
    pub angle: f64,
}

/// The angle from `a` round to `b`, counter-clockwise positive, in (-pi, pi].
fn turn(a: Vec2, b: Vec2) -> f64 {
    a.cross(b).atan2(a.dot(b))
}

/// The motion a Move, Copy, Rotate or Scale tool comes to with its clicks so far and the
/// pointer at `cursor`; values typed into the boxes beside the pointer come before the pointer's.
pub(crate) fn tool_motion(tool: Tool, clicks: &[Click], cursor: Vec2, hud: &Hud) -> Option<Motion> {
    let first = clicks.first()?.at;
    match tool {
        Tool::Move | Tool::Copy => {
            let d = cursor - first;
            Some(Motion::Shift(Vec2::new(hud.typed(0).unwrap_or(d.x), hud.typed(1).unwrap_or(d.y))))
        }
        Tool::Rotate => {
            let angle = match (hud.typed(0), clicks.get(1)) {
                (Some(deg), _) => deg.to_radians(),
                (None, Some(from)) => turn(from.at - first, cursor - first),
                _ => return None,
            };
            Some(Motion::Turn { about: first, angle })
        }
        Tool::Scale => {
            let factor = match (hud.typed(0), clicks.get(1)) {
                (Some(f), _) => f,
                (None, Some(from)) if from.at.dist(first) > 1e-9 => cursor.dist(first) / from.at.dist(first),
                _ => return None,
            };
            Some(Motion::Scale { about: first, factor })
        }
        _ => None,
    }
}

/// What the boxes beside the pointer read while nothing is typed in them.
pub(crate) fn tool_live(tool: Tool, clicks: &[Click], cursor: Vec2) -> [f64; 2] {
    match tool_motion(tool, clicks, cursor, &Hud::default()) {
        Some(Motion::Shift(d)) => [d.x, d.y],
        Some(Motion::Turn { angle, .. }) => [angle.to_degrees(), 0.0],
        Some(Motion::Scale { factor, .. }) => [factor, 0.0],
        None => [if tool == Tool::Scale { 1.0 } else { 0.0 }, 0.0],
    }
}

/// The two centres of a slot from the two points clicked for it and its width.
fn slot_centres(tool: Tool, a: Vec2, b: Vec2, width: f64) -> (Vec2, Vec2) {
    match tool {
        Tool::SlotOverall if a.dist(b) > width => {
            let d = (b - a).normalized();
            (a + d * (width / 2.0), b - d * (width / 2.0))
        }
        Tool::SlotCenter => (a * 2.0 - b, b),
        _ => (a, b),
    }
}

/// How wide a slot through `a` and `b` is for the pointer to be on its side.
fn slot_width(a: Vec2, b: Vec2, at: Vec2) -> f64 {
    let d = (b - a).normalized();
    (2.0 * d.cross(at - a).abs()).max(0.01)
}

/// The curves of `s` as polylines.
fn polylines(s: &Sketch) -> Vec<Vec<Vec2>> {
    s.entities().filter(|(_, e)| e.geometry.is_curve() && !e.construction).map(|(id, _)| s.tessellate(id, 0.05)).collect()
}

impl Workbench {
    /// A click with one of this file's tools.
    pub(crate) fn modify_click(&mut self, feature: FeatureId, sketch: &Sketch, click: Click, hover: Option<EntityId>) {
        let Mode::Sketch(sm) = &mut self.mode else { return };
        let (tool, f) = (sm.tool, feature.0);
        match tool {
            Tool::Move | Tool::Copy | Tool::Rotate | Tool::Scale => {
                let before = if matches!(tool, Tool::Move | Tool::Copy) { 1 } else { 2 };
                if sm.clicks.len() < before {
                    sm.clicks.push(click);
                    let next = match (tool, sm.clicks.len()) {
                        (Tool::Move | Tool::Copy, _) => "Click where it goes, or type X and Y and press Enter.",
                        (Tool::Rotate, 1) => "Click a point to turn from, or type the angle and press Enter.",
                        (Tool::Rotate, _) => "Click the point to turn to.",
                        (_, 1) => "Click a point to scale from, or type the factor and press Enter.",
                        _ => "Click the point to scale to.",
                    };
                    self.set_status(next);
                    return;
                }
                let motion = tool_motion(tool, &sm.clicks, click.at, &Hud::default());
                self.apply_motion(feature, tool, motion);
            }
            Tool::Extend | Tool::Split => match hover.filter(|h| !sketch.is_point(*h)) {
                Some(c) => {
                    let id = if tool == Tool::Extend { "sketch.extend" } else { "sketch.split" };
                    let _ = self.exec_status(id, json!({ "sketch": f, "curve": c.0, "x": click.at.x, "y": click.at.y }));
                }
                None => self.set_error(tool_hint(tool)),
            },
            Tool::PatternRect => match hover.filter(|h| sketch.is_line(*h)) {
                Some(line) => {
                    let entities = sm.selection.clone();
                    sm.tool = Tool::Select;
                    self.panel = Some(Panel::SketchPattern(SketchPatternPanel {
                        sketch: feature,
                        entities,
                        along: Some(line),
                        center: Vec2::ZERO,
                        count: 2,
                        spacing: 10.0,
                        count2: 1,
                        spacing2: 10.0,
                        angle: 360.0,
                    }));
                }
                None => self.set_error(tool_hint(tool)),
            },
            Tool::PatternCircular => {
                let entities = sm.selection.clone();
                sm.tool = Tool::Select;
                self.panel = Some(Panel::SketchPattern(SketchPatternPanel {
                    sketch: feature,
                    entities,
                    along: None,
                    center: click.at,
                    count: 6,
                    spacing: 10.0,
                    count2: 1,
                    spacing2: 10.0,
                    angle: 360.0,
                }));
            }
            Tool::Slot | Tool::SlotOverall | Tool::SlotCenter | Tool::Rect3 | Tool::ArcCenter => {
                if sm.clicks.len() < 2 {
                    sm.clicks.push(click);
                    return;
                }
                let (a, b) = (sm.clicks[0], sm.clicks[1]);
                sm.clicks.clear();
                match tool {
                    Tool::Rect3 => {
                        let p = json!({ "sketch": f, "x1": a.at.x, "y1": a.at.y, "x2": b.at.x, "y2": b.at.y, "x3": click.at.x, "y3": click.at.y });
                        let _ = self.exec_status("sketch.rectangle3", p);
                    }
                    Tool::ArcCenter => {
                        // The shorter way round from the start to where the pointer is.
                        let r = b.at.dist(a.at);
                        let end = a.at + (click.at - a.at).normalized() * r;
                        let (start, end, start_id, end_id) =
                            if turn(b.at - a.at, end - a.at) >= 0.0 { (b.at, end, b.point, click.point) } else { (end, b.at, click.point, b.point) };
                        let mut m = Map::new();
                        m.insert("sketch".into(), json!(f));
                        crate::sketcher::put_point(&mut m, "center", "cx", "cy", &a);
                        crate::sketcher::put_point(&mut m, "start", "x1", "y1", &Click { at: start, point: start_id, infer: None });
                        // (An existing point is only an end when it is on the circle already.)
                        let on_circle = end_id.filter(|_| (click.at.dist(a.at) - r).abs() < 1e-6 || end_id == b.point);
                        crate::sketcher::put_point(&mut m, "end", "x2", "y2", &Click { at: end, point: on_circle, infer: None });
                        let _ = self.exec_status("sketch.arc", Value::Object(m));
                    }
                    _ => {
                        let width = slot_width(a.at, b.at, click.at);
                        let by = match tool {
                            Tool::SlotOverall => "overall",
                            Tool::SlotCenter => "center",
                            _ => "centers",
                        };
                        let p = json!({ "sketch": f, "x1": a.at.x, "y1": a.at.y, "x2": b.at.x, "y2": b.at.y, "width": width, "by": by });
                        let _ = self.exec_status("sketch.slot", p);
                    }
                }
            }
            Tool::RectCenter => {
                let Some(first) = sm.clicks.first().copied() else {
                    sm.clicks.push(click);
                    return;
                };
                sm.clicks.clear();
                let p = json!({ "sketch": f, "cx": first.at.x, "cy": first.at.y, "x": click.at.x, "y": click.at.y });
                // Drawn round an existing point (the origin, say), its centre is that point.
                if let Some(r) = self.exec_status("sketch.rectangle_center", p)
                    && let (Some(on), Some(c)) = (first.point, r["center"].as_u64())
                {
                    let _ = self.exec("sketch.constrain", json!({ "sketch": f, "constraint": { "type": "coincident", "a": on.0, "b": c } }));
                }
            }
            Tool::ArcTangent => {
                let Some(first) = sm.clicks.first().copied() else {
                    match click.point {
                        Some(_) => sm.clicks.push(click),
                        None => self.set_error("click the end point of a line or an arc for the arc to carry on from"),
                    }
                    return;
                };
                sm.clicks.clear();
                if let Some(from) = first.point {
                    let _ = self.exec_status("sketch.arc_tangent", json!({ "sketch": f, "from": from.0, "x": click.at.x, "y": click.at.y }));
                }
            }
            _ => {}
        }
    }

    /// Runs the Move, Copy, Rotate or Scale the tool has come to.
    fn apply_motion(&mut self, feature: FeatureId, tool: Tool, motion: Option<Motion>) {
        let Mode::Sketch(sm) = &mut self.mode else { return };
        let ids: Vec<u32> = sm.selection.iter().map(|e| e.0).collect();
        let copy = tool == Tool::Copy;
        let (id, mut p) = match motion {
            Some(Motion::Shift(d)) => ("sketch.move", json!({ "dx": d.x, "dy": d.y })),
            Some(Motion::Turn { about, angle }) => ("sketch.rotate", json!({ "cx": about.x, "cy": about.y, "angle": angle })),
            Some(Motion::Scale { about, factor }) => ("sketch.scale", json!({ "cx": about.x, "cy": about.y, "factor": factor })),
            None => return,
        };
        p["sketch"] = json!(feature.0);
        p["entities"] = json!(ids);
        p["copy"] = json!(copy);
        // A copy is put down and the tool waits for where the next one goes; the others are done.
        if copy {
            sm.clicks.truncate(1);
        } else {
            sm.clicks.clear();
            sm.tool = Tool::Select;
        }
        sm.hud = Hud::default();
        if self.exec_status(id, p).is_some() {
            self.set_status(match tool {
                Tool::Copy => "Copied. Click where the next copy goes, or press Esc.",
                Tool::Rotate => "Rotated.",
                Tool::Scale => "Scaled.",
                _ => "Moved.",
            });
        }
    }

    /// Enter with values typed beside the pointer for Move, Copy, Rotate or Scale.
    pub(crate) fn modify_hud_commit(&mut self, feature: FeatureId, tool: Tool, hud: &Hud, cursor: Vec2) {
        let Mode::Sketch(sm) = &self.mode else { return };
        let motion = tool_motion(tool, &sm.clicks, cursor, hud);
        if motion.is_none() {
            self.set_error("type a value, or click the next point");
            return;
        }
        self.apply_motion(feature, tool, motion);
    }

    /// What the running tool would make with the pointer at `cursor`, as polylines in the sketch
    /// (drawn faintly until the click that makes it).
    pub(crate) fn tool_ghost(&self, sketch: &Sketch, sm: &SketchMode, cursor: Vec2) -> Vec<Vec<Vec2>> {
        let moved = |motion: Motion| -> Vec<Vec<Vec2>> {
            let mut points: BTreeSet<EntityId> = BTreeSet::new();
            for id in &sm.selection {
                match sketch.geometry(*id) {
                    Some(g) if g.is_point() => {
                        points.insert(*id);
                    }
                    Some(g) => points.extend(g.points()),
                    None => {}
                }
            }
            sketch
                .entities()
                .filter(|(id, e)| e.geometry.is_curve() && (sm.selection.contains(id) || e.geometry.points().iter().all(|p| points.contains(p))))
                .map(|(id, _)| sketch.tessellate(id, 0.05).into_iter().map(|q| motion.apply(q)).collect())
                .collect()
        };
        let made = |build: &dyn Fn(&mut Sketch) -> bool| -> Vec<Vec<Vec2>> {
            let mut s = Sketch::new();
            if build(&mut s) { polylines(&s) } else { Vec::new() }
        };
        match (sm.tool, sm.clicks.as_slice()) {
            (Tool::Move | Tool::Copy | Tool::Rotate | Tool::Scale, clicks) => {
                tool_motion(sm.tool, clicks, cursor, &sm.hud).map(moved).unwrap_or_default()
            }
            (Tool::Slot | Tool::SlotOverall | Tool::SlotCenter, [a]) => vec![vec![a.at, cursor]],
            (Tool::Slot | Tool::SlotOverall | Tool::SlotCenter, [a, b]) => {
                let width = slot_width(a.at, b.at, cursor);
                let (ca, cb) = slot_centres(sm.tool, a.at, b.at, width);
                made(&|s| s.add_slot(ca, cb, width).is_ok())
            }
            (Tool::RectCenter, [c]) => made(&|s| s.add_rectangle(c.at * 2.0 - cursor, cursor).is_ok()),
            (Tool::Rect3 | Tool::ArcCenter, [a]) => vec![vec![a.at, cursor]],
            (Tool::Rect3, [a, b]) => made(&|s| s.add_rectangle_3(a.at, b.at, cursor).is_ok()),
            (Tool::ArcCenter, [c, start]) => {
                let r = start.at.dist(c.at);
                let end = c.at + (cursor - c.at).normalized() * r;
                let (from, to) = if turn(start.at - c.at, end - c.at) >= 0.0 { (start.at, end) } else { (end, start.at) };
                made(&|s| s.add_arc(c.at, from, to).is_ok())
            }
            (Tool::ArcTangent, [from]) => match from.point {
                // Worked out on a copy of the sketch: the arc alone is what is shown.
                Some(p) => {
                    let mut s = sketch.clone();
                    s.add_arc_tangent(p, cursor).ok().map(|arc| vec![s.tessellate(arc, 0.05)]).unwrap_or_default()
                }
                None => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// The copies a sketch pattern being set up would make, as polylines in the sketch.
    pub(crate) fn pattern_ghost(&self, p: &SketchPatternPanel) -> Vec<Vec<Vec2>> {
        let Some(sketch) = self.document().sketch(p.sketch) else { return Vec::new() };
        let mut s = sketch.clone();
        let before: BTreeSet<EntityId> = s.entities().map(|(id, _)| id).collect();
        let made = match p.along.and_then(|l| sketch.line(l)) {
            Some((a, b)) => s.pattern_rect(&p.entities, b - a, p.count, p.spacing, (p.count2 > 1).then(|| ((b - a).perp(), p.count2, p.spacing2))),
            None => s.pattern_circular(&p.entities, p.center, p.count, p.angle.to_radians()),
        };
        if made.is_err() {
            return Vec::new();
        }
        s.entities().filter(|(id, e)| !before.contains(id) && e.geometry.is_curve()).map(|(id, _)| s.tessellate(id, 0.05)).collect()
    }

    /// The Rectangular or Circular Pattern window of a sketch. False once it is done with.
    pub(crate) fn sketch_pattern_window(&mut self, ctx: &egui::Context, p: &mut SketchPatternPanel, commit: bool) -> bool {
        let (mut ok, mut keep) = (commit, true);
        let rect = p.along.is_some();
        let title = if rect { "Rectangular Pattern" } else { "Circular Pattern" };
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 160.0]).show(ctx, |ui| {
            ui.set_width(230.0);
            let whole = |ui: &mut Ui, label: &str, key: &str, v: &mut u32| {
                ui.label(label);
                let r = ui.add(egui::DragValue::new(v).range(1..=tenon_sketch::MAX_PATTERN as u32).speed(0.1));
                crate::drawing::remember(ui, key, r.rect);
                ui.end_row();
            };
            let size = |ui: &mut Ui, label: &str, key: &str, v: &mut f64, unit: &str| {
                ui.label(label);
                ui.horizontal(|ui| {
                    let r = ui.add(egui::DragValue::new(v).speed(0.1));
                    crate::drawing::remember(ui, key, r.rect);
                    ui.label(unit);
                });
                ui.end_row();
            };
            egui::Grid::new("tn_skpattern").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                if rect {
                    whole(ui, "Count", "tn_skpattern_count", &mut p.count);
                    size(ui, "Spacing", "tn_skpattern_spacing", &mut p.spacing, "mm");
                    whole(ui, "Count, second direction", "tn_skpattern_count2", &mut p.count2);
                    size(ui, "Spacing, second direction", "tn_skpattern_spacing2", &mut p.spacing2, "mm");
                } else {
                    whole(ui, "Count", "tn_skpattern_count", &mut p.count);
                    size(ui, "Angle", "tn_skpattern_angle", &mut p.angle, "deg");
                }
            });
            ui.label(
                egui::RichText::new("The copies are geometry of their own: they do not follow the original afterwards.").font(crate::theme::small()),
            );
            ui.horizontal(|ui| {
                if crate::drawing::button(ui, "OK", "tn_skpattern_ok").clicked() {
                    ok = true;
                }
                if crate::drawing::button(ui, "Cancel", "tn_skpattern_cancel").clicked() {
                    keep = false;
                }
            });
        });
        if !ok {
            return keep;
        }
        let ids: Vec<u32> = p.entities.iter().map(|e| e.0).collect();
        let done = match p.along {
            Some(line) => {
                let mut c = json!({ "sketch": p.sketch.0, "entities": ids, "along": line.0, "count": p.count, "spacing": p.spacing });
                if p.count2 > 1 {
                    c["count2"] = json!(p.count2);
                    c["spacing2"] = json!(p.spacing2);
                }
                self.exec_status("sketch.pattern.rect", c)
            }
            None => self.exec_status(
                "sketch.pattern.circular",
                json!({ "sketch": p.sketch.0, "entities": ids, "cx": p.center.x, "cy": p.center.y, "count": p.count, "angle": p.angle.to_radians() }),
            ),
        };
        match done {
            Some(r) => {
                self.set_status(format!("Patterned: {} copies.", r["entities"].as_array().map_or(0, Vec::len)));
                false
            }
            None => true,
        }
    }
}

fn tool_hint(tool: Tool) -> &'static str {
    match tool {
        Tool::Extend => "click a line or an arc near the end to make longer",
        Tool::Split => "click a curve near where another crosses it",
        Tool::PatternRect => "click a line for the direction of the pattern",
        _ => "click in the sketch",
    }
}
