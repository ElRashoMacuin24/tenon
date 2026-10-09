//! The assembly panels in the properties area: Constrain (mate, flush, angle, insert), Joint
//! (rigid, rotational, slider, cylindrical, planar, ball), editing a relationship's values, and
//! Tweak (an exploded-view step). Geometry is picked in the view, faces and edges of components;
//! while both picks are made the view shows where the relationship would put things.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use egui::{Color32, Rect, RichText, Ui, vec2};
use serde_json::json;
use tenon_assembly::session::{self as asm_session, relate};
use tenon_assembly::{ComponentId, Geom, JointKind, RelKind, RelationshipId, Target};
use tenon_geom::{Frame, Vec3};
use tenon_render::pick::{pick_edge, pick_face};

use crate::panels::Panel;
use crate::theme::{self, Tokens};
use crate::viewport::Pick;
use crate::workbench::Workbench;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsmTool {
    Constrain,
    Joint,
    Tweak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ConstraintType {
    Mate,
    Flush,
    Angle,
    Insert,
}

impl ConstraintType {
    pub(crate) const ALL: [ConstraintType; 4] = [ConstraintType::Mate, ConstraintType::Flush, ConstraintType::Angle, ConstraintType::Insert];
    pub(crate) fn label(self) -> &'static str {
        match self {
            ConstraintType::Mate => "Mate",
            ConstraintType::Flush => "Flush",
            ConstraintType::Angle => "Angle",
            ConstraintType::Insert => "Insert",
        }
    }
    fn id(self) -> &'static str {
        match self {
            ConstraintType::Mate => "mate",
            ConstraintType::Flush => "flush",
            ConstraintType::Angle => "angle",
            ConstraintType::Insert => "insert",
        }
    }
    fn hint(self) -> &'static str {
        match self {
            ConstraintType::Mate => "Faces face each other; axes line up; points meet",
            ConstraintType::Flush => "Planar faces side by side, facing the same way",
            ConstraintType::Angle => "The angle between two faces or edges",
            ConstraintType::Insert => "Two circular edges: axes in line, faces together",
        }
    }
}

/// An assembly panel's state.
#[derive(Clone, Debug)]
pub(crate) struct AsmPanel {
    pub tool: AsmTool,
    pub constraint: ConstraintType,
    pub joint: JointKind,
    /// The two picks, with what they are called.
    pub a: Option<(Target, String)>,
    pub b: Option<(Target, String)>,
    /// Which pick a click fills: 0 the first, 1 the second.
    pub slot: usize,
    pub offset: f64,
    pub degrees: f64,
    pub flip: bool,
    pub aligned: bool,
    /// The relationship whose values are being edited.
    pub editing: Option<RelationshipId>,
    /// Tweak: the components moved, the direction (0 X, 1 Y, 2 Z), reversed, and how far.
    pub components: Vec<ComponentId>,
    pub axis: usize,
    pub reverse: bool,
    pub distance: f64,
    /// Where the relationship would put the components: what it was worked out from, and the
    /// placements or why it cannot hold.
    pub preview: Option<(u64, Result<BTreeMap<ComponentId, Frame>, String>)>,
}

impl AsmPanel {
    pub(crate) fn new(tool: AsmTool) -> AsmPanel {
        AsmPanel {
            tool,
            constraint: ConstraintType::Mate,
            joint: JointKind::Rigid,
            a: None,
            b: None,
            slot: 0,
            offset: 0.0,
            degrees: 0.0,
            flip: false,
            aligned: false,
            editing: None,
            components: Vec::new(),
            axis: 2,
            reverse: false,
            distance: 20.0,
            preview: None,
        }
    }

    pub(crate) fn title(&self) -> &'static str {
        match self.tool {
            AsmTool::Constrain => "Constrain",
            AsmTool::Joint => "Joint",
            AsmTool::Tweak => "Tweak Components",
        }
    }

    /// The relationship the panel describes, if both picks are made.
    pub(crate) fn kind(&self) -> Option<RelKind> {
        let (a, b) = (self.a.as_ref()?.0.clone(), self.b.as_ref()?.0.clone());
        let angle = self.degrees.to_radians();
        Some(match self.tool {
            AsmTool::Constrain => match self.constraint {
                ConstraintType::Mate => RelKind::Mate { a, b, offset: self.offset },
                ConstraintType::Flush => RelKind::Flush { a, b, offset: self.offset },
                ConstraintType::Angle => RelKind::Angle { a, b, angle, reference: Vec3::Z },
                ConstraintType::Insert => RelKind::Insert { a, b, offset: self.offset, aligned: self.aligned },
            },
            AsmTool::Joint => RelKind::Joint { joint: self.joint, a, b, flip: self.flip, offset: self.offset, angle },
            AsmTool::Tweak => return None,
        })
    }

    fn direction(&self) -> Vec3 {
        let d = [Vec3::X, Vec3::Y, Vec3::Z].get(self.axis).copied().unwrap_or(Vec3::Z);
        if self.reverse { -d } else { d }
    }

    /// What the preview depends on.
    fn preview_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}{:?}{:?}{:?}", self.tool, self.constraint, self.joint, self.kind()).hash(&mut h);
        (self.offset.to_bits(), self.degrees.to_bits(), self.flip, self.aligned).hash(&mut h);
        h.finish()
    }
}

/// A short description of a target.
fn describe(target: &Target, name: &str, kind: &str) -> String {
    match &target.geom {
        Geom::Face { .. } => format!("{name}: {kind} face"),
        Geom::Edge { .. } => format!("{name}: {} edge", if kind == "circle" { "circular" } else { "straight" }),
        Geom::Plane { plane } => format!("{name}: {plane:?} plane"),
        Geom::Axis { axis } => format!("{name}: {axis:?} axis"),
        Geom::Origin => format!("{name}: origin"),
        Geom::Work { feature } => format!("{name}: work feature {}", feature.0),
    }
}

impl Workbench {
    pub(crate) fn open_asm_panel(&mut self, tool: AsmTool) {
        self.panel = Some(Panel::Asm(Box::new(AsmPanel::new(tool))));
        self.view.hover = None;
        self.set_status(match tool {
            AsmTool::Constrain => "Constrain: click a face or edge of one component, then of another.",
            AsmTool::Joint => "Joint: click where the first component's joint origin is (a face, circular edge or axis), then the second's.",
            AsmTool::Tweak => "Tweak: click the components to move, choose a direction and distance.",
        });
    }

    /// Opens the panel on an existing relationship, to change its values.
    pub(crate) fn edit_relationship(&mut self, id: RelationshipId) -> Result<(), String> {
        let a = self.asm.as_ref().ok_or("no assembly")?;
        let r = a.session.assembly().relationship(id).ok_or_else(|| format!("{id} does not exist"))?.clone();
        let label = |t: &Target| {
            let name = t.component.and_then(|c| a.session.assembly().component(c)).map_or_else(|| "Assembly".to_string(), |c| c.name.clone());
            let kind = asm_session::resolve_target(a.session.assembly(), &a.session.parts, t).map_or("", |e| e.prim.kind());
            (t.clone(), describe(t, &name, kind))
        };
        let mut p = AsmPanel::new(if r.kind.is_joint() { AsmTool::Joint } else { AsmTool::Constrain });
        let [ta, tb] = r.kind.targets();
        (p.a, p.b) = (Some(label(ta)), Some(label(tb)));
        p.editing = Some(id);
        match &r.kind {
            RelKind::Mate { offset, .. } => (p.constraint, p.offset) = (ConstraintType::Mate, *offset),
            RelKind::Flush { offset, .. } => (p.constraint, p.offset) = (ConstraintType::Flush, *offset),
            RelKind::Angle { angle, .. } => (p.constraint, p.degrees) = (ConstraintType::Angle, angle.to_degrees()),
            RelKind::Insert { offset, aligned, .. } => (p.constraint, p.offset, p.aligned) = (ConstraintType::Insert, *offset, *aligned),
            RelKind::Joint { joint, flip, offset, angle, .. } => {
                (p.joint, p.flip, p.offset, p.degrees) = (*joint, *flip, *offset, angle.to_degrees());
            }
        }
        self.panel = Some(Panel::Asm(Box::new(p)));
        self.set_status(format!("Editing {}: change its values, then OK.", r.name));
        Ok(())
    }

    /// Works out where a new relationship would put things (each time its picks or values
    /// change), so the view shows it before OK.
    pub(crate) fn asm_panel_preview(&mut self) {
        let Some(Panel::Asm(p)) = &self.panel else { return };
        if p.editing.is_none() && p.kind().is_none() {
            if p.preview.is_some()
                && let Some(Panel::Asm(p)) = &mut self.panel
            {
                p.preview = None;
            }
            return;
        }
        let key = p.preview_key();
        if p.preview.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let Some(a) = self.asm.as_ref() else { return };
        let mut asm = a.session.assembly().clone();
        let result = match (p.editing, p.kind()) {
            (Some(id), Some(kind)) => {
                // The edited relationship with its new values.
                if let Some(r) = asm.relationship_mut(id) {
                    r.kind = match (&r.kind, kind) {
                        (RelKind::Angle { reference, .. }, RelKind::Angle { a, b, angle, .. }) => {
                            RelKind::Angle { a, b, angle, reference: *reference }
                        }
                        (_, k) => k,
                    };
                }
                let s = asm_session::solve_assembly(&mut asm, &a.session.parts, None);
                if s.converged { Ok(()) } else { Err("the relationships cannot all hold with these values".to_string()) }
            }
            (None, Some(kind)) => relate(&mut asm, &a.session.parts, kind).map(|_| ()).map_err(|e| e.0),
            _ => Ok(()),
        };
        let frames = result.map(|()| asm.components.iter().map(|c| (c.id, c.placement)).collect());
        if let Some(Panel::Asm(p)) = &mut self.panel {
            p.preview = Some((key, frames));
        }
        if let Some(a) = self.asm.as_mut() {
            a.shown = None;
        }
    }

    /// The target a pick in the shown scene means, and its description.
    fn target_of_pick(&self, pick: Pick) -> Result<(Target, String), String> {
        let a = self.asm.as_ref().ok_or("no assembly")?;
        let body = match pick {
            Pick::Face { body, .. } | Pick::Edge { body, .. } => body,
        };
        let (c, part_body) = *a.map.get(body).ok_or("nothing there")?;
        let comp = a.session.assembly().component(c).ok_or("no such component")?;
        let scene = asm_session::scene_of(&a.session.parts, comp).ok_or("the part's geometry is not ready")?;
        let target = match pick {
            Pick::Face { face, .. } => asm_session::face_target(scene, c, part_body, face as usize)?,
            Pick::Edge { edge, .. } => asm_session::edge_target(scene, c, part_body, edge as usize)?,
        };
        let kind = asm_session::resolve_target(a.session.assembly(), &a.session.parts, &target)?.prim.kind();
        let text = describe(&target, &comp.name, kind);
        Ok((target, text))
    }

    /// The shown-scene picks of the panel's targets (to highlight them).
    pub(crate) fn asm_panel_picks(&self) -> Vec<Pick> {
        let (Some(Panel::Asm(p)), Some(a)) = (&self.panel, self.asm.as_ref()) else { return Vec::new() };
        if p.tool == AsmTool::Tweak {
            return a
                .map
                .iter()
                .enumerate()
                .filter(|(_, (c, _))| p.components.contains(c))
                .flat_map(|(i, _)| {
                    let n = self.scene.bodies.get(i).map_or(0, |b| b.faces.len());
                    (0..n).map(move |f| Pick::Face { body: i, face: f as u32 })
                })
                .collect();
        }
        let mut out = Vec::new();
        for (t, _) in [&p.a, &p.b].into_iter().flatten() {
            let Some(c) = t.component else { continue };
            let Some(comp) = a.session.assembly().component(c) else { continue };
            let Some(scene) = asm_session::scene_of(&a.session.parts, comp) else { continue };
            let flat = |part_body: usize| a.map.iter().position(|m| *m == (c, part_body));
            match &t.geom {
                Geom::Face { face } => {
                    if let Ok((b, f)) = tenon_assembly::geometry::find_face(scene, face)
                        && let Some(i) = flat(b)
                    {
                        out.push(Pick::Face { body: i, face: f as u32 });
                    }
                }
                Geom::Edge { edge } => {
                    if let Ok((b, e)) = tenon_assembly::geometry::find_edge(scene, edge)
                        && let Some(i) = flat(b)
                    {
                        out.push(Pick::Edge { body: i, edge: e as u32 });
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Picking for an assembly panel: faces and edges of components (or whole components for
    /// Tweak).
    pub(crate) fn asm_panel_pointer(&mut self, _ui: &Ui, resp: &egui::Response, rect: Rect) {
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let meshes: Vec<&tenon_kernel::Mesh> = self.scene.bodies.iter().map(|b| &b.mesh).collect();
        let tweak = matches!(&self.panel, Some(Panel::Asm(p)) if p.tool == AsmTool::Tweak);
        self.view.hover = resp.hover_pos().and_then(|p| {
            let (x, y) = (f64::from(p.x - rect.left()), f64::from(p.y - rect.top()));
            if !tweak && let Some(e) = pick_edge(&meshes, &self.view.camera, w, h, x, y, 5.0) {
                return Some(Pick::Edge { body: e.body, edge: e.edge });
            }
            let (o, d) = self.view.camera.ray(x, y, w, h);
            pick_face(&meshes, o, d).map(|f| Pick::Face { body: f.body, face: f.face })
        });
        if !resp.clicked() {
            return;
        }
        let Some(pick) = self.view.hover else { return };
        if tweak {
            let body = match pick {
                Pick::Face { body, .. } | Pick::Edge { body, .. } => body,
            };
            if let (Some(c), Some(Panel::Asm(p))) = (self.component_of_body(body), &mut self.panel) {
                if let Some(i) = p.components.iter().position(|x| *x == c) {
                    p.components.remove(i);
                } else {
                    p.components.push(c);
                }
            }
            return;
        }
        match self.target_of_pick(pick) {
            Ok(t) => {
                let other = match &self.panel {
                    Some(Panel::Asm(p)) => if p.slot == 0 { &p.b } else { &p.a }.as_ref().and_then(|(o, _)| o.component),
                    _ => None,
                };
                if other.is_some() && other == t.0.component {
                    self.set_error("Pick the second geometry on another component.");
                    return;
                }
                if let Some(Panel::Asm(p)) = &mut self.panel {
                    if p.editing.is_some() {
                        return;
                    }
                    if p.slot == 0 {
                        p.a = Some(t);
                        p.slot = 1;
                    } else {
                        p.b = Some(t);
                    }
                }
            }
            Err(e) => self.set_error(e),
        }
    }

    /// The panel's controls.
    pub(crate) fn asm_panel_ui(&self, ui: &mut Ui, p: &mut AsmPanel, t: &Tokens) {
        let section = |ui: &mut Ui, title: &str, body: &mut dyn FnMut(&mut Ui)| {
            egui::CollapsingHeader::new(RichText::new(title).font(theme::body()).strong()).default_open(true).show(ui, |ui| body(ui));
        };
        let slot_row = |ui: &mut Ui, label: &str, pick: &Option<(Target, String)>, active: bool, hint: &str| -> bool {
            let mut clicked = false;
            ui.horizontal(|ui| {
                ui.label(label);
                let text = pick.as_ref().map_or_else(|| hint.to_string(), |(_, s)| s.clone());
                let b = egui::Button::new(RichText::new(text).color(if active { t.accent_text } else { t.text })).fill(if active {
                    t.accent
                } else {
                    t.field
                });
                clicked = ui.add(b.min_size(vec2(170.0, 22.0))).on_hover_text("Click, then pick in the view").clicked();
            });
            clicked
        };
        match p.tool {
            AsmTool::Constrain => {
                section(ui, "Type", &mut |ui| {
                    ui.horizontal(|ui| {
                        for c in ConstraintType::ALL {
                            let on = p.constraint == c;
                            let b = egui::Button::new(RichText::new(c.label()).color(if on { t.accent_text } else { t.text })).fill(if on {
                                t.accent
                            } else {
                                t.field
                            });
                            if ui.add_enabled(p.editing.is_none(), b).on_hover_text(c.hint()).clicked() {
                                p.constraint = c;
                            }
                        }
                    });
                });
            }
            AsmTool::Joint => {
                section(ui, "Type", &mut |ui| {
                    egui::ComboBox::from_id_salt("tn_joint_type").selected_text(p.joint.label()).show_ui(ui, |ui| {
                        for k in JointKind::ALL {
                            let text = format!("{}  ({} free)", k.label(), k.freedom());
                            ui.selectable_value(&mut p.joint, k, text);
                        }
                    });
                });
            }
            AsmTool::Tweak => {}
        }
        if p.tool != AsmTool::Tweak {
            section(ui, "Selections", &mut |ui| {
                let hint = match (p.tool, p.constraint) {
                    (AsmTool::Constrain, ConstraintType::Insert) => "click a circular edge",
                    (AsmTool::Joint, _) => "click a face, circular edge or axis",
                    _ => "click a face or edge",
                };
                if slot_row(ui, "1", &p.a, p.slot == 0 && p.editing.is_none(), hint) && p.editing.is_none() {
                    p.slot = 0;
                }
                if slot_row(ui, "2", &p.b, p.slot == 1 && p.editing.is_none(), hint) && p.editing.is_none() {
                    p.slot = 1;
                }
                if p.editing.is_none() && (p.a.is_some() || p.b.is_some()) && ui.small_button("Clear").clicked() {
                    p.a = None;
                    p.b = None;
                    p.slot = 0;
                }
            });
            section(ui, "Values", &mut |ui| {
                egui::Grid::new("tn_asm_values").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    let angle = matches!((p.tool, p.constraint), (AsmTool::Constrain, ConstraintType::Angle))
                        || matches!((p.tool, p.joint), (AsmTool::Joint, JointKind::Rigid | JointKind::Slider));
                    let offset = !matches!((p.tool, p.constraint), (AsmTool::Constrain, ConstraintType::Angle))
                        && !matches!((p.tool, p.joint), (AsmTool::Joint, JointKind::Slider | JointKind::Cylindrical | JointKind::Ball));
                    if offset {
                        ui.label("Offset");
                        ui.add(egui::DragValue::new(&mut p.offset).speed(0.5).suffix(" mm").range(-100_000.0..=100_000.0));
                        ui.end_row();
                    }
                    if angle {
                        ui.label("Angle");
                        ui.add(egui::DragValue::new(&mut p.degrees).speed(1.0).suffix(" deg").range(-360.0..=360.0));
                        ui.end_row();
                    }
                    if p.tool == AsmTool::Constrain && p.constraint == ConstraintType::Insert {
                        ui.label("Solution");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut p.aligned, false, "Opposed");
                            ui.selectable_value(&mut p.aligned, true, "Aligned");
                        });
                        ui.end_row();
                    }
                    if p.tool == AsmTool::Joint && p.joint != JointKind::Ball {
                        ui.label("Flip");
                        ui.checkbox(&mut p.flip, "the second origin's Z axis");
                        ui.end_row();
                    }
                });
            });
            match &p.preview {
                Some((_, Err(e))) => {
                    ui.label(RichText::new(e).color(Color32::from_rgb(0xd8, 0x44, 0x38)));
                }
                Some((_, Ok(_))) if p.editing.is_none() => {
                    ui.label(RichText::new("The view shows where it puts the components.").color(t.text_dim));
                }
                _ => {}
            }
        } else {
            section(ui, "Components", &mut |ui| {
                let names: Vec<String> = p.components.iter().map(|c| self.asm.as_ref().map_or_else(String::new, |a| a.name(*c))).collect();
                ui.label(if names.is_empty() { "click the components to move".to_string() } else { names.join(", ") });
                if !p.components.is_empty() && ui.small_button("Clear").clicked() {
                    p.components.clear();
                }
            });
            section(ui, "Move", &mut |ui| {
                egui::Grid::new("tn_tweak").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Direction");
                    ui.horizontal(|ui| {
                        for (i, l) in ["X", "Y", "Z"].iter().enumerate() {
                            ui.selectable_value(&mut p.axis, i, *l);
                        }
                        ui.checkbox(&mut p.reverse, "reverse");
                    });
                    ui.end_row();
                    ui.label("Distance");
                    ui.add(egui::DragValue::new(&mut p.distance).speed(0.5).suffix(" mm").range(-100_000.0..=100_000.0));
                    ui.end_row();
                });
            });
        }
    }

    /// OK on an assembly panel: makes (or changes) the relationship, or adds the exploded-view
    /// step. True when done.
    pub(crate) fn commit_asm_panel(&mut self, p: &AsmPanel) -> bool {
        let result = match (p.tool, p.editing) {
            (AsmTool::Tweak, _) => {
                if p.components.is_empty() {
                    Err("click at least one component to move".to_string())
                } else {
                    let d = p.direction();
                    let ids: Vec<u32> = p.components.iter().map(|c| c.0).collect();
                    let r = self.asm_exec("asm.explode.add", json!({ "components": ids, "direction": [d.x, d.y, d.z], "distance": p.distance }));
                    if r.is_ok()
                        && let Some(a) = self.asm.as_mut()
                    {
                        a.exploded = true;
                    }
                    r
                }
            }
            (tool, Some(id)) => self.asm_exec(
                "asm.edit_relationship",
                json!({
                    "relationship": id.0,
                    "offset": p.offset,
                    "angle": p.degrees.to_radians(),
                    "flip": p.flip,
                    "aligned": p.aligned,
                    "type": if tool == AsmTool::Joint { Some(p.joint.id()) } else { None },
                }),
            ),
            (tool, None) => match (&p.a, &p.b) {
                (Some((a, _)), Some((b, _))) => {
                    let (id, params) = if tool == AsmTool::Joint {
                        (
                            "asm.joint",
                            json!({ "type": p.joint.id(), "a": a, "b": b, "flip": p.flip, "offset": p.offset, "angle": p.degrees.to_radians() }),
                        )
                    } else {
                        (
                            "asm.constrain",
                            json!({ "type": p.constraint.id(), "a": a, "b": b, "offset": p.offset, "angle": p.degrees.to_radians(), "aligned": p.aligned }),
                        )
                    };
                    self.asm_exec(id, params)
                }
                _ => Err("pick geometry on two components first".to_string()),
            },
        };
        match result {
            Ok(v) => {
                if let Some(name) = v["name"].as_str() {
                    self.set_status(format!("Added {name}; {} degrees of freedom left in the assembly.", v["dof"]));
                }
                if let Some(a) = self.asm.as_mut() {
                    a.shown = None;
                }
                true
            }
            Err(e) => {
                self.set_error(e);
                false
            }
        }
    }
}
