//! Feature and value dialogs. Extrude and revolve preview live: while their panel is open the
//! workbench regenerates the document with the pending feature; OK commits it as a command.

use egui::{Align2, Ui};
use serde_json::{Value, json};
use tenon_model::{
    AxisRef, Document, Extrude, ExtrudeExtent, FeatureId, FeatureKind, Operation, OriginAxis, OriginPlane, RegionSel, Revolve, RevolveAngle,
};
use tenon_sketch::{Constraint, ConstraintId, EntityId};

use crate::workbench::{Mode, Workbench};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtentChoice {
    Distance,
    Symmetric,
    ThroughAll,
}

#[derive(Clone, Debug)]
pub(crate) struct ExtrudePanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub extent: ExtentChoice,
    pub distance: f64,
    pub reverse: bool,
    pub operation: Operation,
    pub regions: RegionSel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AxisChoice {
    Origin(OriginAxis),
    Line(EntityId),
}

#[derive(Clone, Debug)]
pub(crate) struct RevolvePanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub axis: AxisChoice,
    pub full: bool,
    pub degrees: f64,
    pub symmetric: bool,
    pub operation: Operation,
    pub regions: RegionSel,
}

#[derive(Clone, Debug)]
pub(crate) enum ValueFor {
    Fillet {
        sketch: FeatureId,
        point: EntityId,
    },
    Offset {
        sketch: FeatureId,
        curves: Vec<EntityId>,
    },
    /// A new dimension (its value is replaced by the entered one).
    Dimension {
        sketch: FeatureId,
        constraint: Constraint,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ValuePanel {
    pub title: &'static str,
    pub label: &'static str,
    pub value: f64,
    pub what: ValueFor,
}

impl ValuePanel {
    pub(crate) fn new(title: &'static str, label: &'static str, value: f64, what: ValueFor) -> Self {
        ValuePanel { title, label, value, what }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Panel {
    NewSketch,
    Extrude(ExtrudePanel),
    Revolve(RevolvePanel),
    Value(ValuePanel),
    EditDimension { sketch: FeatureId, constraint: ConstraintId, value: f64, angular: bool },
    Rename { feature: FeatureId, name: String },
}

fn operation_ui(ui: &mut Ui, op: &mut Operation) {
    ui.horizontal(|ui| {
        ui.label("Output");
        ui.radio_value(op, Operation::Join, "Join");
        ui.radio_value(op, Operation::Cut, "Cut");
        ui.radio_value(op, Operation::Intersect, "Intersect");
        ui.radio_value(op, Operation::NewBody, "New body");
    });
}

impl ExtrudePanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        let extent = match self.extent {
            ExtentChoice::Distance => ExtrudeExtent::Distance(self.distance),
            ExtentChoice::Symmetric => ExtrudeExtent::Symmetric(self.distance),
            ExtentChoice::ThroughAll => ExtrudeExtent::ThroughAll,
        };
        FeatureKind::Extrude(Extrude { sketch: self.sketch, regions: self.regions.clone(), extent, reverse: self.reverse, operation: self.operation })
    }
}

impl RevolvePanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        let angle = if self.full {
            RevolveAngle::Full
        } else if self.symmetric {
            RevolveAngle::Symmetric(self.degrees.to_radians())
        } else {
            RevolveAngle::Angle(self.degrees.to_radians())
        };
        let axis = match self.axis {
            AxisChoice::Origin(a) => AxisRef::Origin(a),
            AxisChoice::Line(l) => AxisRef::SketchLine(l),
        };
        FeatureKind::Revolve(Revolve { sketch: self.sketch, regions: self.regions.clone(), axis, angle, operation: self.operation })
    }
}

/// The document with `kind` added (or replacing feature `editing`).
fn with_feature(doc: &Document, editing: Option<FeatureId>, kind: FeatureKind) -> Option<Document> {
    let mut d = doc.clone();
    match editing {
        Some(id) => d.feature_mut(id)?.kind = kind,
        None => {
            d.add(kind).ok()?;
        }
    }
    Some(d)
}

impl Workbench {
    /// The document to show while a feature panel previews.
    pub(crate) fn preview_document(&self) -> Option<Document> {
        match &self.panel {
            Some(Panel::Extrude(p)) => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Revolve(p)) => with_feature(self.document(), p.editing, p.kind()),
            _ => None,
        }
    }

    /// The sketch a new feature should use: the one being edited, else the newest.
    fn default_sketch(&mut self) -> Result<FeatureId, String> {
        if let Mode::Sketch(s) = &self.mode {
            let id = s.feature;
            self.finish_sketch();
            return Ok(id);
        }
        self.sketches().last().map(|s| s.0).ok_or_else(|| "draw a sketch first".to_string())
    }

    pub(crate) fn open_extrude(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Extrude(e) => {
                    let (extent, distance) = match e.extent {
                        ExtrudeExtent::Distance(d) => (ExtentChoice::Distance, d),
                        ExtrudeExtent::Symmetric(d) => (ExtentChoice::Symmetric, d),
                        ExtrudeExtent::TwoSided { forward, .. } => (ExtentChoice::Distance, forward),
                        ExtrudeExtent::ThroughAll => (ExtentChoice::ThroughAll, 10.0),
                    };
                    ExtrudePanel {
                        editing,
                        sketch: e.sketch,
                        extent,
                        distance,
                        reverse: e.reverse,
                        operation: e.operation,
                        regions: e.regions.clone(),
                    }
                }
                _ => return Err("not an extrusion".into()),
            },
            None => {
                // Join makes a new body when there is none yet.
                let sketch = self.default_sketch()?;
                ExtrudePanel {
                    editing: None,
                    sketch,
                    extent: ExtentChoice::Distance,
                    distance: 10.0,
                    reverse: false,
                    operation: Operation::Join,
                    regions: RegionSel::Default,
                }
            }
        };
        self.panel = Some(Panel::Extrude(panel));
        self.set_status("Extrude: set the distance and output, then OK.");
        Ok(())
    }

    pub(crate) fn open_revolve(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Revolve(r) => {
                    let (full, degrees, symmetric) = match r.angle {
                        RevolveAngle::Full => (true, 360.0, false),
                        RevolveAngle::Angle(a) => (false, a.to_degrees(), false),
                        RevolveAngle::Symmetric(a) => (false, a.to_degrees(), true),
                    };
                    let axis = match r.axis {
                        AxisRef::Origin(a) => AxisChoice::Origin(a),
                        AxisRef::SketchLine(l) => AxisChoice::Line(l),
                    };
                    RevolvePanel { editing, sketch: r.sketch, axis, full, degrees, symmetric, operation: r.operation, regions: r.regions.clone() }
                }
                _ => return Err("not a revolution".into()),
            },
            None => {
                let sketch = self.default_sketch()?;
                // Default axis: a construction line of the sketch if there is one.
                let axis = self
                    .document()
                    .sketch(sketch)
                    .and_then(|s| s.entities().find(|(id, e)| e.construction && s.is_line(*id)).map(|(id, _)| AxisChoice::Line(id)))
                    .unwrap_or(AxisChoice::Origin(OriginAxis::Y));
                RevolvePanel {
                    editing: None,
                    sketch,
                    axis,
                    full: true,
                    degrees: 360.0,
                    symmetric: false,
                    operation: Operation::Join,
                    regions: RegionSel::Default,
                }
            }
        };
        self.panel = Some(Panel::Revolve(panel));
        self.set_status("Revolve: choose the axis and angle, then OK.");
        Ok(())
    }

    pub(crate) fn commit_feature(&mut self, editing: Option<FeatureId>, kind: FeatureKind) -> bool {
        let r = match editing {
            Some(id) => self.exec_status("feature.update", json!({ "feature": id.0, "kind": serde_json::to_value(&kind).unwrap_or(Value::Null) })),
            None => {
                // Through the registry, like every other edit.
                let params = match &kind {
                    FeatureKind::Extrude(e) => {
                        let mut p = json!({ "sketch": e.sketch.0, "reverse": e.reverse, "operation": op_name(e.operation) });
                        match e.extent {
                            ExtrudeExtent::Distance(d) => p["distance"] = json!(d),
                            ExtrudeExtent::Symmetric(d) => p["symmetric"] = json!(d),
                            ExtrudeExtent::ThroughAll | ExtrudeExtent::TwoSided { .. } => p["through_all"] = json!(true),
                        }
                        ("model.extrude", p)
                    }
                    FeatureKind::Revolve(r) => {
                        let mut p = json!({ "sketch": r.sketch.0, "operation": op_name(r.operation) });
                        p["axis"] = match r.axis {
                            AxisRef::Origin(a) => json!(format!("{a:?}").to_lowercase()),
                            AxisRef::SketchLine(l) => json!(l.0),
                        };
                        match r.angle {
                            RevolveAngle::Full => {}
                            RevolveAngle::Angle(a) => p["angle"] = json!(a),
                            RevolveAngle::Symmetric(a) => {
                                p["angle"] = json!(a);
                                p["symmetric"] = json!(true);
                            }
                        }
                        ("model.revolve", p)
                    }
                    FeatureKind::Sketch { .. } => return false,
                };
                self.exec_status(params.0, params.1)
            }
        };
        r.is_some()
    }

    pub(crate) fn panels(&mut self, ui: &mut Ui) {
        let Some(mut panel) = self.panel.take() else { return };
        let at = self.view.rect.left_top() + egui::vec2(12.0, 44.0);
        let mut keep = true;
        let mut commit = false;
        let ctx = ui.ctx().clone();
        let sketches = self.sketches();
        match &mut panel {
            Panel::NewSketch => {
                egui::Window::new("New Sketch").collapsible(false).resizable(false).default_pos(at).show(&ctx, |ui| {
                    ui.label("Choose a plane (or select a planar face of the part first):");
                    ui.horizontal(|ui| {
                        for (label, plane) in [("XY (top)", OriginPlane::XY), ("XZ (front)", OriginPlane::XZ), ("YZ (right)", OriginPlane::YZ)] {
                            if ui.button(label).clicked() {
                                let name = format!("{plane:?}").to_lowercase();
                                if let Err(e) = self.create_sketch(json!({ "plane": name })) {
                                    self.set_error(e);
                                }
                                keep = false;
                            }
                        }
                    });
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            }
            Panel::Extrude(p) => {
                egui::Window::new(if p.editing.is_some() { "Edit Extrude" } else { "Extrude" })
                    .collapsible(false)
                    .resizable(false)
                    .default_pos(at)
                    .show(&ctx, |ui| {
                        egui::ComboBox::from_label("Profile sketch")
                            .selected_text(sketches.iter().find(|s| s.0 == p.sketch).map_or("?".into(), |s| s.1.clone()))
                            .show_ui(ui, |ui| {
                                for (id, name) in &sketches {
                                    ui.selectable_value(&mut p.sketch, *id, name);
                                }
                            });
                        ui.horizontal(|ui| {
                            ui.label("Extent");
                            ui.radio_value(&mut p.extent, ExtentChoice::Distance, "Distance");
                            ui.radio_value(&mut p.extent, ExtentChoice::Symmetric, "Symmetric");
                            ui.radio_value(&mut p.extent, ExtentChoice::ThroughAll, "Through all");
                        });
                        if p.extent != ExtentChoice::ThroughAll {
                            ui.horizontal(|ui| {
                                ui.label("Distance");
                                ui.add(egui::DragValue::new(&mut p.distance).speed(0.5).range(0.001..=100_000.0).suffix(" mm"));
                            });
                        }
                        ui.checkbox(&mut p.reverse, "Reverse direction");
                        operation_ui(ui, &mut p.operation);
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                commit = true;
                            }
                            if ui.button("Cancel").clicked() {
                                keep = false;
                            }
                        });
                    });
                if commit {
                    let (editing, kind) = (p.editing, p.kind());
                    keep = !self.commit_feature(editing, kind);
                }
            }
            Panel::Revolve(p) => {
                let lines: Vec<EntityId> = self
                    .document()
                    .sketch(p.sketch)
                    .map(|s| s.entities().filter(|(id, _)| s.is_line(*id)).map(|(id, _)| id).collect())
                    .unwrap_or_default();
                egui::Window::new(if p.editing.is_some() { "Edit Revolve" } else { "Revolve" })
                    .collapsible(false)
                    .resizable(false)
                    .default_pos(at)
                    .show(&ctx, |ui| {
                        egui::ComboBox::from_label("Profile sketch")
                            .selected_text(sketches.iter().find(|s| s.0 == p.sketch).map_or("?".into(), |s| s.1.clone()))
                            .show_ui(ui, |ui| {
                                for (id, name) in &sketches {
                                    ui.selectable_value(&mut p.sketch, *id, name);
                                }
                            });
                        let axis_name = |a: &AxisChoice| match a {
                            AxisChoice::Origin(o) => format!("{o:?} axis"),
                            AxisChoice::Line(l) => format!("Sketch line {}", l.0),
                        };
                        egui::ComboBox::from_label("Axis").selected_text(axis_name(&p.axis)).show_ui(ui, |ui| {
                            for o in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
                                ui.selectable_value(&mut p.axis, AxisChoice::Origin(o), format!("{o:?} axis"));
                            }
                            for l in &lines {
                                ui.selectable_value(&mut p.axis, AxisChoice::Line(*l), format!("Sketch line {}", l.0));
                            }
                        });
                        ui.checkbox(&mut p.full, "Full turn");
                        if !p.full {
                            ui.horizontal(|ui| {
                                ui.label("Angle");
                                ui.add(egui::DragValue::new(&mut p.degrees).speed(1.0).range(0.1..=360.0).suffix(" deg"));
                                ui.checkbox(&mut p.symmetric, "Symmetric");
                            });
                        }
                        operation_ui(ui, &mut p.operation);
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                commit = true;
                            }
                            if ui.button("Cancel").clicked() {
                                keep = false;
                            }
                        });
                    });
                if commit {
                    let (editing, kind) = (p.editing, p.kind());
                    keep = !self.commit_feature(editing, kind);
                }
            }
            Panel::Value(v) => {
                let mut ok = false;
                egui::Window::new(v.title).collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, [0.0, 160.0]).show(&ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(v.label);
                        let r = ui.add(egui::DragValue::new(&mut v.value).speed(0.1));
                        r.request_focus();
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            ok = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            ok = true;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if ok {
                    let done = match &v.what {
                        ValueFor::Fillet { sketch, point } => {
                            self.exec_status("sketch.fillet", json!({ "sketch": sketch.0, "point": point.0, "radius": v.value }))
                        }
                        ValueFor::Offset { sketch, curves } => {
                            let ids: Vec<u32> = curves.iter().map(|c| c.0).collect();
                            self.exec_status("sketch.offset", json!({ "sketch": sketch.0, "curves": ids, "distance": v.value }))
                        }
                        ValueFor::Dimension { sketch, constraint } => {
                            let mut c = constraint.clone();
                            let value = if c.is_angular() { v.value.to_radians() } else { v.value };
                            let signed = matches!(c, Constraint::HorizontalDistance { .. } | Constraint::VerticalDistance { .. });
                            c.set_value(if signed { value } else { value.abs() });
                            self.exec_status(
                                "sketch.constrain",
                                json!({ "sketch": sketch.0, "constraint": serde_json::to_value(&c).unwrap_or(Value::Null) }),
                            )
                        }
                    };
                    keep = done.is_none();
                }
            }
            Panel::EditDimension { sketch, constraint, value, angular } => {
                let mut ok = false;
                egui::Window::new("Edit Dimension").collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, [0.0, 160.0]).show(&ctx, |ui| {
                    let mut shown = if *angular { value.to_degrees() } else { *value };
                    ui.horizontal(|ui| {
                        ui.label(if *angular { "Degrees" } else { "Millimetres" });
                        let r = ui.add(egui::DragValue::new(&mut shown).speed(0.1));
                        r.request_focus();
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            ok = true;
                        }
                    });
                    *value = if *angular { shown.to_radians() } else { shown };
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            ok = true;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if ok {
                    keep = self
                        .exec_status("sketch.set_dimension", json!({ "sketch": sketch.0, "constraint": constraint.0, "value": *value }))
                        .is_none();
                }
            }
            Panel::Rename { feature, name } => {
                let mut ok = false;
                egui::Window::new("Rename").collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, [0.0, 160.0]).show(&ctx, |ui| {
                    let r = ui.text_edit_singleline(name);
                    r.request_focus();
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            ok = true;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if ok {
                    keep = self.exec_status("feature.rename", json!({ "feature": feature.0, "name": name.trim() })).is_none();
                }
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = false;
        }
        if keep && self.panel.is_none() {
            self.panel = Some(panel);
        }
    }
}

fn op_name(o: Operation) -> &'static str {
    match o {
        Operation::Join => "join",
        Operation::Cut => "cut",
        Operation::NewBody => "new_body",
        Operation::Intersect => "intersect",
    }
}
