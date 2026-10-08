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
    ThroughAll,
}

/// Which way a feature goes from its sketch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Default,
    Flipped,
    Symmetric,
    /// Two distances, one each way.
    Asymmetric,
}

#[derive(Clone, Debug)]
pub(crate) struct ExtrudePanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub extent: ExtentChoice,
    pub direction: Direction,
    pub distance: f64,
    /// The second distance (asymmetric).
    pub distance_b: f64,
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

/// A request to finish the open panel from outside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanelRequest {
    Ok,
    Cancel,
    /// OK, then start the same command again.
    Apply,
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

impl ExtrudePanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        let extent = match (self.extent, self.direction) {
            (ExtentChoice::ThroughAll, _) => ExtrudeExtent::ThroughAll,
            (ExtentChoice::Distance, Direction::Symmetric) => ExtrudeExtent::Symmetric(self.distance),
            (ExtentChoice::Distance, Direction::Asymmetric) => ExtrudeExtent::TwoSided { forward: self.distance, backward: self.distance_b },
            (ExtentChoice::Distance, _) => ExtrudeExtent::Distance(self.distance),
        };
        let reverse = self.direction == Direction::Flipped;
        FeatureKind::Extrude(Extrude { sketch: self.sketch, regions: self.regions.clone(), extent, reverse, operation: self.operation })
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
                    let way = if e.reverse { Direction::Flipped } else { Direction::Default };
                    let (extent, direction, distance, distance_b) = match e.extent {
                        ExtrudeExtent::Distance(d) => (ExtentChoice::Distance, way, d, d),
                        ExtrudeExtent::Symmetric(d) => (ExtentChoice::Distance, Direction::Symmetric, d, d),
                        ExtrudeExtent::TwoSided { forward, backward } => (ExtentChoice::Distance, Direction::Asymmetric, forward, backward),
                        ExtrudeExtent::ThroughAll => (ExtentChoice::ThroughAll, way, 10.0, 10.0),
                    };
                    ExtrudePanel {
                        editing,
                        sketch: e.sketch,
                        extent,
                        direction,
                        distance,
                        distance_b,
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
                    direction: Direction::Default,
                    distance: 10.0,
                    distance_b: 10.0,
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
                            ExtrudeExtent::TwoSided { forward, backward } => {
                                p["distance"] = json!(forward);
                                p["backward"] = json!(backward);
                            }
                            ExtrudeExtent::ThroughAll => p["through_all"] = json!(true),
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
        // OK, Cancel or Apply from the properties panel, the mini-toolbar or the radial menu.
        let request = self.panel_request.take();
        let mut keep = request != Some(PanelRequest::Cancel);
        let commit = matches!(request, Some(PanelRequest::Ok | PanelRequest::Apply));
        let again = request == Some(PanelRequest::Apply);
        let mut reopen: Option<&'static str> = None;
        let ctx = ui.ctx().clone();
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
                if commit {
                    let (editing, kind) = (p.editing, p.kind());
                    keep = !self.commit_feature(editing, kind);
                    if !keep && again {
                        reopen = Some("model.extrude");
                    }
                }
            }
            Panel::Revolve(p) => {
                if commit {
                    let (editing, kind) = (p.editing, p.kind());
                    keep = !self.commit_feature(editing, kind);
                    if !keep && again {
                        reopen = Some("model.revolve");
                    }
                }
            }
            Panel::Value(v) => {
                let mut ok = commit;
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
                let mut ok = commit;
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
                let mut ok = commit;
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
        if let Some(id) = reopen {
            self.command(id);
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
