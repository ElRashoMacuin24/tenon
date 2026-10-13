//! Primitives: a box, a cylinder, a sphere or a torus in a few clicks (3D Model > Primitives).
//!
//! As the established workflow has it: choose the shape, click a plane or a flat face, click
//! where its centre is, click how large it is, set its height and what it does to the part,
//! OK. What is made is not a special kind of feature: a dimensioned sketch and the extrusion or
//! revolution of it, by the same commands scripts and agents use (`model.box` and the rest,
//! DEC-084), in one undo step. The solid is shown as it will be while it is set up.

use egui::{Align2, Pos2, Rect, Stroke, Ui, vec2};
use serde_json::{Value, json};
use tenon_geom::{Frame, Vec2, Vec3};
use tenon_kernel::SurfaceKind;
use tenon_model::{Document, Operation, OriginPlane, PlaneRef, Session, WorkGeom, frame_on_face};

use crate::panels::Panel;
use crate::properties::{OPS, eq_field, flip_button, glyph_row, section};
use crate::theme::{self, Tokens};
use crate::viewport::Pick;
use crate::workbench::Workbench;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Box,
    Cylinder,
    Sphere,
    Torus,
}

impl Shape {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Shape::Box => "Box",
            Shape::Cylinder => "Cylinder",
            Shape::Sphere => "Sphere",
            Shape::Torus => "Torus",
        }
    }

    /// The registry command that makes it, which is also the ribbon's.
    pub(crate) fn command(self) -> &'static str {
        match self {
            Shape::Box => "model.box",
            Shape::Cylinder => "model.cylinder",
            Shape::Sphere => "model.sphere",
            Shape::Torus => "model.torus",
        }
    }

    pub(crate) fn of_command(id: &str) -> Option<Shape> {
        [Shape::Box, Shape::Cylinder, Shape::Sphere, Shape::Torus].into_iter().find(|s| s.command() == id)
    }

    /// How many clicks set its size once its centre is clicked.
    fn size_clicks(self) -> u8 {
        if self == Shape::Torus { 2 } else { 1 }
    }
}

/// The smallest size the pointer sets (mm): a click on the centre itself is not a solid.
const SMALLEST: f64 = 0.1;

/// Sizes and places set by the pointer are rounded to this (mm), so the dimensions that are
/// made read as someone would have typed them.
const STEP: f64 = 0.1;

fn rounded(v: f64) -> f64 {
    (v / STEP).round() * STEP
}

#[derive(Clone, Debug)]
pub(crate) struct PrimitivePanel {
    pub shape: Shape,
    /// The plane or flat face it stands on, and the frame of a sketch there.
    pub on: Option<(PlaneRef, Frame)>,
    /// Its centre on the plane (sketch coordinates), once clicked or typed.
    pub center: Option<Vec2>,
    /// How many clicks are still to set its size: while there are any, the pointer does.
    pub sizing: u8,
    /// A box's size along the sketch's x; of the others, the diameter (a torus's from the
    /// middle of its ring to the middle opposite).
    pub width: f64,
    /// A box's size along the sketch's y; how thick a torus's ring is.
    pub depth: f64,
    /// How far a box or a cylinder goes from its plane.
    pub height: f64,
    pub operation: Operation,
    pub reverse: bool,
}

impl PrimitivePanel {
    pub(crate) fn new(shape: Shape) -> PrimitivePanel {
        PrimitivePanel {
            shape,
            on: None,
            center: None,
            sizing: 0,
            width: 20.0,
            depth: if shape == Shape::Torus { 5.0 } else { 20.0 },
            height: 10.0,
            operation: Operation::Join,
            reverse: false,
        }
    }

    /// What the next click does, for the status bar.
    pub(crate) fn prompt(&self) -> String {
        let name = self.shape.title();
        match (&self.on, self.center, self.sizing, self.shape) {
            (None, ..) => format!("{name}: select a plane or a flat face of the part (origin planes are also in the browser)."),
            (_, None, ..) => format!("{name}: click where its centre is (the plane's origin is marked), or type it."),
            (_, _, 2, _) => "Torus: click where the middle of its ring is.".into(),
            (_, _, 1, Shape::Torus) => "Torus: click how thick its ring is.".into(),
            (_, _, 1, Shape::Box) => "Box: click a corner.".into(),
            (_, _, 1, _) => format!("{name}: click how large it is."),
            (_, _, _, Shape::Box | Shape::Cylinder) => format!("{name}: set its height, then OK."),
            _ => format!("{name}: OK makes it."),
        }
    }

    /// Sets the size the pointer is setting, from where it is on the plane.
    fn size_from(&mut self, at: Vec2) {
        let Some(c) = self.center else { return };
        let away = (at - c).len();
        match (self.shape, self.sizing) {
            (Shape::Box, _) => {
                self.width = rounded(2.0 * (at.x - c.x).abs()).max(SMALLEST);
                self.depth = rounded(2.0 * (at.y - c.y).abs()).max(SMALLEST);
            }
            (Shape::Cylinder | Shape::Sphere, _) => self.width = rounded(2.0 * away).max(SMALLEST),
            (Shape::Torus, 2) => {
                self.width = rounded(2.0 * away).max(4.0 * SMALLEST);
                // (A ring a quarter as thick as it is wide, until its thickness is clicked.)
                self.depth = rounded(self.width / 4.0).max(SMALLEST);
            }
            (Shape::Torus, _) => {
                // As thick as twice the way from the middle of the ring to the pointer, and
                // always thinner than the ring is wide.
                let thick = 2.0 * (away - self.width / 2.0).abs();
                self.depth = rounded(thick).clamp(SMALLEST, (self.width - SMALLEST).max(SMALLEST));
            }
        }
    }

    /// The command's parameters, once there is a plane, a centre and a size.
    pub(crate) fn params(&self) -> Option<Value> {
        let ((plane, _), c) = (self.on.as_ref()?, self.center?);
        let mut p = match plane {
            PlaneRef::Origin(o) => json!({ "plane": format!("{o:?}").to_lowercase() }),
            PlaneRef::Face(f) => json!({ "face": f }),
            PlaneRef::Work(id) => json!({ "work_plane": id.0 }),
        };
        p["x"] = json!(c.x);
        p["y"] = json!(c.y);
        p["operation"] = json!(match self.operation {
            Operation::Join => "join",
            Operation::Cut => "cut",
            Operation::NewBody => "new_body",
            Operation::Intersect => "intersect",
        });
        let positive = |v: f64| v.is_finite() && v > 0.0;
        match self.shape {
            Shape::Box => {
                if !(positive(self.width) && positive(self.depth) && positive(self.height)) {
                    return None;
                }
                p["width"] = json!(self.width);
                p["depth"] = json!(self.depth);
                p["height"] = json!(self.height);
                p["centered"] = json!(true);
                p["reverse"] = json!(self.reverse);
            }
            Shape::Cylinder => {
                if !(positive(self.width) && positive(self.height)) {
                    return None;
                }
                p["diameter"] = json!(self.width);
                p["height"] = json!(self.height);
                p["reverse"] = json!(self.reverse);
            }
            Shape::Sphere => {
                if !positive(self.width) {
                    return None;
                }
                p["diameter"] = json!(self.width);
            }
            Shape::Torus => {
                if !(positive(self.depth) && self.depth < self.width) {
                    return None;
                }
                p["diameter"] = json!(self.width);
                p["section"] = json!(self.depth);
            }
        }
        Some(p)
    }
}

/// The panel's fields. `plane` is what the plane it stands on is called. Returns true when
/// Enter was pressed in a field.
pub(crate) fn fields(ui: &mut Ui, p: &mut PrimitivePanel, plane: Option<&str>, t: &Tokens) -> bool {
    let mut enter = false;
    let field = |ui: &mut Ui, id: &str, v: &mut f64, low: f64| eq_field(ui, egui::Id::new(id), v, None, "mm", low..=100_000.0, 110.0, t);
    section(ui, "Placement", true, |ui| {
        egui::Grid::new("tn_props_prim_place").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Plane");
            // (Clicked, it takes the next click on a plane or face: the solid starts over.)
            let waiting = p.on.is_none();
            let text = egui::RichText::new(plane.unwrap_or("Select")).color(if waiting { t.accent_text } else { t.text });
            let button = egui::Button::new(text).fill(if waiting { t.accent } else { t.field }).min_size(vec2(96.0, 22.0));
            let r = ui.add(button).on_hover_text("Click, then pick a plane or a flat face in the part");
            crate::drawing::remember(ui, "tn_props_prim_plane", r.rect);
            if r.clicked() {
                p.on = None;
                p.center = None;
                p.sizing = 0;
            }
            ui.end_row();
            if p.on.is_some() {
                let mut c = p.center.unwrap_or(Vec2::new(0.0, 0.0));
                ui.label("Centre X").on_hover_text("Along the plane, from its origin (marked in the part)");
                let x = field(ui, "tn_props_prim_x", &mut c.x, -100_000.0);
                ui.end_row();
                ui.label("Centre Y");
                let y = field(ui, "tn_props_prim_y", &mut c.y, -100_000.0);
                ui.end_row();
                enter |= x.entered || y.entered;
                if x.changed || y.changed {
                    // Typed: there it is, and its sizes are as the fields below say.
                    p.center = Some(c);
                    p.sizing = 0;
                }
            }
        });
    });
    section(ui, "Size", true, |ui| {
        egui::Grid::new("tn_props_prim_size").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            let mut typed = false;
            let mut row = |ui: &mut Ui, label: &str, tip: &str, id: &str, v: &mut f64| {
                ui.label(label).on_hover_text(tip);
                let out = field(ui, id, v, 0.001);
                ui.end_row();
                enter |= out.entered;
                typed |= out.changed;
            };
            match p.shape {
                Shape::Box => {
                    row(ui, "Width", "Along the plane's first direction", "tn_props_prim_w", &mut p.width);
                    row(ui, "Depth", "Along the plane's second direction", "tn_props_prim_d", &mut p.depth);
                    row(ui, "Height", "Out of the plane", "tn_props_prim_h", &mut p.height);
                }
                Shape::Cylinder => {
                    row(ui, "Diameter", "Across the cylinder", "tn_props_prim_w", &mut p.width);
                    row(ui, "Height", "Out of the plane", "tn_props_prim_h", &mut p.height);
                }
                Shape::Sphere => row(ui, "Diameter", "Across the sphere", "tn_props_prim_w", &mut p.width),
                Shape::Torus => {
                    row(ui, "Diameter", "Across the ring, from the middle of it to the middle opposite", "tn_props_prim_w", &mut p.width);
                    row(ui, "Section", "How thick the ring is", "tn_props_prim_d", &mut p.depth);
                }
            }
            if typed {
                // A typed size stands: the pointer no longer sets it.
                p.sizing = 0;
            }
            if matches!(p.shape, Shape::Box | Shape::Cylinder) {
                ui.label("Direction");
                flip_button(ui, &mut p.reverse, t);
                ui.end_row();
            }
        });
    });
    section(ui, "Output", true, |ui| {
        egui::Grid::new("tn_props_prim_output").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Boolean");
            let items: Vec<(crate::properties::Glyph, &str, bool)> = OPS.iter().map(|o| (o.0, o.1, true)).collect();
            let sel = OPS.iter().position(|o| o.2 == p.operation).unwrap_or(0);
            if let Some(i) = glyph_row(ui, &items, sel, t)
                && let Some(op) = OPS.get(i)
            {
                p.operation = op.2;
            }
            ui.end_row();
        });
    });
    enter
}

impl Workbench {
    /// Box, Cylinder, Sphere or Torus: the panel opens, and the next click takes the plane or
    /// flat face it stands on (a flat face that is selected already is taken at once).
    pub(crate) fn open_primitive(&mut self, shape: Shape) -> Result<(), String> {
        let mut panel = PrimitivePanel::new(shape);
        let selected = std::mem::take(&mut self.view.selection);
        if let [Pick::Face { body, face }] = selected.as_slice()
            && let Some(plane) = self.flat_face(*body, *face)
        {
            panel.on = self.plane_frame(&plane).map(|f| (plane, f));
        }
        self.pick_plane = false;
        // (The origin planes are picked in the part or in the browser, as for a new sketch.)
        self.chrome.origin_open = true;
        self.set_status(panel.prompt());
        self.panel = Some(Panel::Primitive(panel));
        self.start_equations();
        Ok(())
    }

    /// The flat face `face` of `body` as a plane to stand something on, if it is flat and can
    /// be referred to.
    fn flat_face(&self, body: usize, face: u32) -> Option<PlaneRef> {
        let (_, info) = self.scene.bodies.get(body)?.faces.get(face as usize)?;
        matches!(info.surface, SurfaceKind::Plane { .. }).then(|| self.face_ref_of(body, face)).flatten().map(PlaneRef::Face)
    }

    /// The frame a sketch on `plane` has: where the regeneration will put it.
    pub(crate) fn plane_frame(&self, plane: &PlaneRef) -> Option<Frame> {
        match plane {
            PlaneRef::Origin(o) => Some(o.frame()),
            PlaneRef::Face(f) => {
                let (body, face) = crate::modify::find_face(&self.scene, f)?;
                frame_on_face(&self.scene.bodies.get(body)?.faces.get(face as usize)?.1.surface)
            }
            PlaneRef::Work(id) => self.scene.work.iter().find_map(|(w, g)| match g {
                WorkGeom::Plane(f) if w == id => Some(*f),
                _ => None,
            }),
        }
    }

    /// True while the open panel waits for a plane to be clicked (the origin planes are shown).
    pub(crate) fn primitive_wants_plane(&self) -> bool {
        matches!(&self.panel, Some(Panel::Primitive(p)) if p.on.is_none())
    }

    /// Puts the primitive being set up on `plane`. False when no primitive is waiting for one.
    pub(crate) fn primitive_on(&mut self, plane: PlaneRef) -> bool {
        if !self.primitive_wants_plane() {
            return false;
        }
        let Some(frame) = self.plane_frame(&plane) else {
            self.set_error("That plane cannot hold a sketch.".to_string());
            return true;
        };
        if let Some(Panel::Primitive(p)) = &mut self.panel {
            p.on = Some((plane, frame));
            p.center = None;
            p.sizing = 0;
            let prompt = p.prompt();
            self.set_status(prompt);
        }
        self.view.hover = None;
        self.view.plane_hover = None;
        self.view.work_hover = None;
        true
    }

    /// Where the pointer is on the plane of `frame`, in the plane's coordinates.
    fn on_plane(&self, frame: &Frame, p: Pos2, rect: Rect) -> Option<Vec2> {
        let (o, d) =
            self.view.camera.ray(f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
        let den = d.dot(frame.z());
        if den.abs() < 1e-9 {
            return None;
        }
        let t = (frame.origin() - o).dot(frame.z()) / den;
        let l = frame.to_local(o + d * t);
        (t > 0.0 && l.x.is_finite() && l.y.is_finite()).then_some(Vec2::new(l.x, l.y))
    }

    fn plane_to_screen(&self, frame: &Frame, at: Vec2, rect: Rect) -> Option<Pos2> {
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        self.view.camera.project(frame.plane_point(at), w, h).map(|(x, y, _)| rect.min + vec2(x as f32, y as f32))
    }

    /// The pointer while a primitive is set up: the plane, then the centre, then the size.
    /// Returns false when no primitive is.
    pub(crate) fn primitive_pointer(&mut self, resp: &egui::Response, rect: Rect) -> bool {
        let Some(Panel::Primitive(p)) = &self.panel else { return false };
        let (on, center, sizing) = (p.on.clone(), p.center, p.sizing);
        self.view.hover = None;
        self.view.plane_hover = None;
        self.view.work_hover = None;
        let Some(pos) = resp.hover_pos() else { return true };
        let Some((_, frame)) = on else {
            // (What is under the pointer is shown as it is for a new sketch.)
            let plane = self.plane_under(pos, rect);
            if resp.clicked()
                && let Some(plane) = plane
            {
                self.primitive_on(plane);
            }
            return true;
        };
        let Some(mut at) = self.on_plane(&frame, pos, rect) else { return true };
        // Near the plane's origin on the screen, the origin is what is meant.
        if center.is_none() && self.plane_to_screen(&frame, Vec2::new(0.0, 0.0), rect).is_some_and(|o| o.distance(pos) <= 8.0) {
            at = Vec2::new(0.0, 0.0);
        }
        let clicked = resp.clicked();
        let Some(Panel::Primitive(p)) = &mut self.panel else { return true };
        match center {
            None if clicked => {
                p.center = Some(Vec2::new(rounded(at.x), rounded(at.y)));
                p.sizing = p.shape.size_clicks();
            }
            Some(_) if sizing > 0 => {
                p.size_from(at);
                if clicked {
                    p.sizing -= 1;
                }
            }
            _ => return true,
        }
        if clicked {
            let prompt = p.prompt();
            self.set_status(prompt);
        }
        true
    }

    /// The solid as it will be, for the preview: the document with the primitive's command run
    /// on a copy of it. None until it has a plane, a centre and a size, or when it cannot be
    /// made (OK then says why).
    pub(crate) fn primitive_preview(&self, p: &PrimitivePanel) -> Option<Document> {
        let params = p.params()?;
        let mut s = Session::new(self.document().clone());
        tenon_io::cmd::run(&mut s, p.shape.command(), &params, None).ok()?;
        Some(s.document().clone())
    }

    /// Marks, while a primitive is set up: the origin of its plane (where a click snaps), its
    /// centre, and the picked origin plane.
    pub(crate) fn primitive_overlay(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        let Some(Panel::Primitive(p)) = &self.panel else { return };
        let Some((plane, frame)) = &p.on else { return };
        let painter = ui.painter().with_clip_rect(rect);
        if let PlaneRef::Origin(o) = plane
            && p.center.is_none()
        {
            // The plane it stands on, as it was shown when it was picked.
            let half = self.origin_plane_half();
            let corners: Option<Vec<Pos2>> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .iter()
                .map(|(a, b)| self.plane_to_screen(frame, Vec2::new(a * half, b * half), rect))
                .collect();
            if let Some(c) = corners {
                painter.add(egui::Shape::convex_polygon(c.clone(), t.tint_work.gamma_multiply(0.12), Stroke::NONE));
                painter.add(egui::Shape::closed_line(c.clone(), Stroke::new(1.2, t.tint_work)));
                let label = match o {
                    OriginPlane::XY => "XY Plane",
                    OriginPlane::XZ => "XZ Plane",
                    OriginPlane::YZ => "YZ Plane",
                };
                if let Some(at) = c.get(3) {
                    painter.text(*at + vec2(4.0, 4.0), Align2::LEFT_TOP, label, theme::small(), t.viewport_text);
                }
            }
        }
        let cross = |at: Pos2, r: f32, stroke: Stroke| {
            painter.line_segment([at - vec2(r, 0.0), at + vec2(r, 0.0)], stroke);
            painter.line_segment([at - vec2(0.0, r), at + vec2(0.0, r)], stroke);
        };
        if p.center.is_none()
            && let Some(o) = self.plane_to_screen(frame, Vec2::new(0.0, 0.0), rect)
        {
            cross(o, 7.0, Stroke::new(1.5, t.tint_work));
            painter.circle_stroke(o, 4.0, Stroke::new(1.5, t.tint_work));
        }
        if let Some(c) = p.center.and_then(|c| self.plane_to_screen(frame, c, rect)) {
            cross(c, 6.0, Stroke::new(1.5, t.accent));
        }
    }

    /// What the plane a primitive stands on is called, for its panel.
    pub(crate) fn plane_name(&self, plane: &PlaneRef) -> String {
        match plane {
            PlaneRef::Origin(OriginPlane::XY) => "XY Plane".into(),
            PlaneRef::Origin(OriginPlane::XZ) => "XZ Plane".into(),
            PlaneRef::Origin(OriginPlane::YZ) => "YZ Plane".into(),
            PlaneRef::Face(_) => "Face".into(),
            PlaneRef::Work(id) => self.feature_name(*id),
        }
    }

    /// The world point at the middle of a primitive being set up, for the toolbar beside it:
    /// once its size is clicked, so that the toolbar is not in the way of the pointer setting it.
    pub(crate) fn primitive_anchor(&self) -> Option<Vec3> {
        let Some(Panel::Primitive(p)) = &self.panel else { return None };
        let (_, frame) = p.on.as_ref()?;
        (p.sizing == 0).then_some(frame.plane_point(p.center?))
    }
}
