//! Feature and value dialogs. Extrude and revolve preview live: while their panel is open the
//! workbench regenerates the document with the pending feature; OK commits it as a command.

use egui::{Align2, Ui};
use serde_json::{Value, json};
use tenon_model::{
    AxisRef, Chamfer, ChamferSize, Document, EdgeRef, Extrude, ExtrudeExtent, FaceRef, FeatureId, FeatureKind, Fillet, Fingerprint, Operation,
    OriginAxis, RegionSel, Revolve, RevolveAngle, Shell,
};
use tenon_sketch::{Constraint, ConstraintId, EntityId};

use crate::viewport::Pick;
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

/// Fillet: rounds the picked edges.
#[derive(Clone, Debug)]
pub(crate) struct FilletPanel {
    pub editing: Option<FeatureId>,
    pub edges: Vec<EdgeRef>,
    pub radius: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChamferMethod {
    Distance,
    TwoDistances,
    DistanceAngle,
}

/// Chamfer: bevels the picked edges; the two-sided methods measure the first value on a picked
/// face.
#[derive(Clone, Debug)]
pub(crate) struct ChamferPanel {
    pub editing: Option<FeatureId>,
    pub edges: Vec<EdgeRef>,
    pub method: ChamferMethod,
    pub d1: f64,
    pub d2: f64,
    pub degrees: f64,
    pub reference: Option<FaceRef>,
}

/// Shell: hollows the body, opening the picked faces.
#[derive(Clone, Debug)]
pub(crate) struct ShellPanel {
    pub editing: Option<FeatureId>,
    pub faces: Vec<FaceRef>,
    pub thickness: f64,
    pub outside: bool,
}

impl FilletPanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        FeatureKind::Fillet(Fillet { edges: self.edges.clone(), radius: self.radius })
    }
}

impl ChamferPanel {
    pub(crate) fn kind(&self) -> Option<FeatureKind> {
        let size = match (self.method, self.reference.clone()) {
            (ChamferMethod::Distance, _) => ChamferSize::Equal(self.d1),
            (ChamferMethod::TwoDistances, Some(reference)) => ChamferSize::TwoDistances { d1: self.d1, d2: self.d2, reference },
            (ChamferMethod::DistanceAngle, Some(reference)) => {
                ChamferSize::DistanceAngle { distance: self.d1, angle: self.degrees.to_radians(), reference }
            }
            _ => return None,
        };
        Some(FeatureKind::Chamfer(Chamfer { edges: self.edges.clone(), size }))
    }
}

impl ShellPanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        FeatureKind::Shell(Shell { remove: self.faces.clone(), thickness: self.thickness, outside: self.outside })
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
    Extrude(ExtrudePanel),
    Revolve(RevolvePanel),
    Fillet(FilletPanel),
    Chamfer(ChamferPanel),
    Shell(ShellPanel),
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
    /// The document to show while a feature panel is open. Extrude and Revolve preview their
    /// result; Fillet, Chamfer and Shell show the part they act on (rolled back to before the
    /// feature when editing it), so edges and faces can be picked on it.
    pub(crate) fn preview_document(&self) -> Option<Document> {
        let rolled = |editing: Option<FeatureId>| editing.map(|id| self.document().rolled_back_to(id));
        match &self.panel {
            Some(Panel::Extrude(p)) => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Revolve(p)) => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Fillet(p)) => rolled(p.editing),
            Some(Panel::Chamfer(p)) => rolled(p.editing),
            Some(Panel::Shell(p)) => rolled(p.editing),
            _ => None,
        }
    }

    /// Edge references of the edges picked in the viewport.
    pub(crate) fn picked_edges(&self) -> Vec<EdgeRef> {
        self.view
            .selection
            .iter()
            .filter_map(|p| match p {
                Pick::Edge { body, edge } => self.scene.bodies.get(*body)?.edge_ref(*edge),
                Pick::Face { .. } => None,
            })
            .collect()
    }

    /// Face references of the faces picked in the viewport.
    pub(crate) fn picked_faces(&self) -> Vec<FaceRef> {
        self.view
            .selection
            .iter()
            .filter_map(|p| match p {
                Pick::Face { body, face } => {
                    let (name, info) = self.scene.bodies.get(*body)?.faces.get(*face as usize)?;
                    Some(FaceRef { origin: (*name)?, fingerprint: Fingerprint::of(info) })
                }
                Pick::Edge { .. } => None,
            })
            .collect()
    }

    /// Fillet, Chamfer or Shell: a new one starts from the edges or faces already selected.
    pub(crate) fn open_modify(&mut self, which: &str, editing: Option<FeatureId>) -> Result<(), String> {
        let existing = match editing {
            Some(id) => Some(self.document().feature(id).ok_or("no such feature")?.kind.clone()),
            None => None,
        };
        // The document, not the scene: the scene may still be regenerating.
        let solid = self.document().features().iter().any(|f| matches!(f.kind, FeatureKind::Extrude(_) | FeatureKind::Revolve(_)));
        if editing.is_none() && !solid {
            return Err("there is no solid yet: extrude or revolve a sketch first".into());
        }
        let panel = match (which, existing) {
            ("fillet", Some(FeatureKind::Fillet(f))) => Panel::Fillet(FilletPanel { editing, edges: f.edges, radius: f.radius }),
            ("fillet", None) => Panel::Fillet(FilletPanel { editing, edges: self.picked_edges(), radius: 2.0 }),
            ("chamfer", Some(FeatureKind::Chamfer(c))) => {
                let (method, d1, d2, degrees, reference) = match c.size {
                    ChamferSize::Equal(d) => (ChamferMethod::Distance, d, d, 45.0, None),
                    ChamferSize::TwoDistances { d1, d2, reference } => (ChamferMethod::TwoDistances, d1, d2, 45.0, Some(reference)),
                    ChamferSize::DistanceAngle { distance, angle, reference } => {
                        (ChamferMethod::DistanceAngle, distance, distance, angle.to_degrees(), Some(reference))
                    }
                };
                Panel::Chamfer(ChamferPanel { editing, edges: c.edges, method, d1, d2, degrees, reference })
            }
            ("chamfer", None) => Panel::Chamfer(ChamferPanel {
                editing,
                edges: self.picked_edges(),
                method: ChamferMethod::Distance,
                d1: 1.0,
                d2: 1.0,
                degrees: 45.0,
                reference: None,
            }),
            ("shell", Some(FeatureKind::Shell(s))) => {
                Panel::Shell(ShellPanel { editing, faces: s.remove, thickness: s.thickness, outside: s.outside })
            }
            ("shell", None) => Panel::Shell(ShellPanel { editing, faces: self.picked_faces(), thickness: 2.0, outside: false }),
            _ => return Err("not a feature of that type".into()),
        };
        self.panel = Some(panel);
        self.view.selection.clear();
        self.set_status(match which {
            "fillet" => "Fillet: click edges to round (click again to remove), set the radius, then OK.",
            "chamfer" => "Chamfer: click edges to bevel, set the distance, then OK.",
            _ => "Shell: click the faces to open, set the thickness, then OK.",
        });
        Ok(())
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
                    FeatureKind::Fillet(f) => ("model.fillet", json!({ "edges": f.edges, "radius": f.radius })),
                    FeatureKind::Chamfer(c) => {
                        let p = match &c.size {
                            ChamferSize::Equal(d) => json!({ "edges": c.edges, "distance": d }),
                            ChamferSize::TwoDistances { d1, d2, reference } => {
                                json!({ "edges": c.edges, "distance": d1, "distance2": d2, "reference": reference })
                            }
                            ChamferSize::DistanceAngle { distance, angle, reference } => {
                                json!({ "edges": c.edges, "distance": distance, "angle": angle, "reference": reference })
                            }
                        };
                        ("model.chamfer", p)
                    }
                    FeatureKind::Shell(s) => ("model.shell", json!({ "remove": s.remove, "thickness": s.thickness, "outside": s.outside })),
                    FeatureKind::Sketch { .. } => return false,
                };
                self.exec_status(params.0, params.1)
            }
        };
        r.is_some()
    }

    pub(crate) fn panels(&mut self, ui: &mut Ui) {
        let Some(mut panel) = self.panel.take() else { return };
        // OK, Cancel or Apply from the properties panel, the mini-toolbar or the radial menu.
        let request = self.panel_request.take();
        let mut keep = request != Some(PanelRequest::Cancel);
        let commit = matches!(request, Some(PanelRequest::Ok | PanelRequest::Apply));
        let again = request == Some(PanelRequest::Apply);
        let mut reopen: Option<&'static str> = None;
        let ctx = ui.ctx().clone();
        let t = crate::theme::Tokens::of(self.chrome.theme);
        match &mut panel {
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
            Panel::Fillet(p) => {
                if commit {
                    if p.edges.is_empty() {
                        self.set_error("Fillet: select at least one edge".to_string());
                    } else {
                        keep = !self.commit_feature(p.editing, p.kind());
                        if !keep && again {
                            reopen = Some("model.fillet");
                        }
                    }
                }
            }
            Panel::Chamfer(p) => {
                if commit {
                    match p.kind() {
                        _ if p.edges.is_empty() => self.set_error("Chamfer: select at least one edge".to_string()),
                        None => self.set_error("Chamfer: pick the face the first distance is measured on".to_string()),
                        Some(kind) => {
                            keep = !self.commit_feature(p.editing, kind);
                            if !keep && again {
                                reopen = Some("model.chamfer");
                            }
                        }
                    }
                }
            }
            Panel::Shell(p) => {
                if commit {
                    keep = !self.commit_feature(p.editing, p.kind());
                    if !keep && again {
                        reopen = Some("model.shell");
                    }
                }
            }
            Panel::Value(v) if matches!(v.what, ValueFor::Dimension { .. }) => {
                // A new dimension: the inline box sits on the dimension itself.
                let ValueFor::Dimension { sketch, constraint } = &v.what else { return };
                let (sketch, constraint) = (*sketch, constraint.clone());
                let unit = if constraint.is_angular() { "deg" } else { "mm" };
                let at = self.dimension_anchor(sketch, &constraint).unwrap_or(self.view.rect.center());
                let (ok, cancel) = inline_value(&ctx, at, &mut v.value, unit, &t);
                if cancel {
                    keep = false;
                } else if ok || commit {
                    let mut c = constraint;
                    let value = if c.is_angular() { v.value.to_radians() } else { v.value };
                    let signed = matches!(c, Constraint::HorizontalDistance { .. } | Constraint::VerticalDistance { .. });
                    c.set_value(if signed { value } else { value.abs() });
                    keep = self
                        .exec_status("sketch.constrain", json!({ "sketch": sketch.0, "constraint": serde_json::to_value(&c).unwrap_or(Value::Null) }))
                        .is_none();
                }
            }
            Panel::EditDimension { sketch, constraint, value, angular } => {
                // Double-clicked dimension: the same inline box.
                let c = self.document().sketch(*sketch).and_then(|s| s.constraint(*constraint).cloned());
                let at = c.as_ref().and_then(|c| self.dimension_anchor(*sketch, c)).unwrap_or(self.view.rect.center());
                let mut shown = if *angular { value.to_degrees() } else { *value };
                let (ok, cancel) = inline_value(&ctx, at, &mut shown, if *angular { "deg" } else { "mm" }, &t);
                *value = if *angular { shown.to_radians() } else { shown };
                if cancel {
                    keep = false;
                } else if ok || commit {
                    keep = self
                        .exec_status("sketch.set_dimension", json!({ "sketch": sketch.0, "constraint": constraint.0, "value": *value }))
                        .is_none();
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

/// The edit box placed on a dimension: the value (selected, so typing replaces it) and a check
/// mark. Enter or the check is OK, Esc cancels. Returns `(ok, cancel)`.
fn inline_value(ctx: &egui::Context, at: egui::Pos2, value: &mut f64, unit: &str, t: &crate::theme::Tokens) -> (bool, bool) {
    let (mut ok, mut cancel) = (false, false);
    let field = egui::Id::new("tn_inline_dimension");
    egui::Area::new(egui::Id::new("tn_inline_box")).order(egui::Order::Foreground).fixed_pos(at - egui::vec2(48.0, 13.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(t.panel).inner_margin(3.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                if ui.memory(|m| m.focused().is_none()) {
                    ui.memory_mut(|m| m.request_focus(field));
                }
                ok |= crate::properties::value_field(ui, field, value, unit, -1.0e6..=1.0e6, 72.0, t).entered;
                if ui.add(egui::Button::new(egui::RichText::new("✔").color(t.ok)).small()).on_hover_text("OK (Enter)").clicked() {
                    ok = true;
                }
            });
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cancel = true;
    }
    (ok, cancel)
}

fn op_name(o: Operation) -> &'static str {
    match o {
        Operation::Join => "join",
        Operation::Cut => "cut",
        Operation::NewBody => "new_body",
        Operation::Intersect => "intersect",
    }
}
