//! Draft, Split and Combine (M6): features that change the bodies already in the part. Their
//! panels, what a new one starts with, and what clicks in the viewport put into them.

use egui::Ui;
use tenon_kernel::SurfaceKind;
use tenon_model::{Combine, Draft, FaceRef, FeatureId, FeatureKind, Operation, OriginPlane, PlaneRef, Split, SplitKeep};

use crate::Workbench;
use crate::modify::find_face;
use crate::panels::Panel;
use crate::properties::{Equations, OPS, WorkNames, eq_value_field, flip_button, glyph_row, picked_row, plane_picker, section, slot_button};
use crate::theme::Tokens;
use crate::viewport::Pick;

/// Which selector of a draft panel takes clicks in the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DraftSlot {
    Faces,
    Plane,
}

/// A draft being made or edited.
#[derive(Clone, Debug)]
pub(crate) struct DraftPanel {
    pub editing: Option<FeatureId>,
    pub faces: Vec<FaceRef>,
    pub plane: PlaneRef,
    pub degrees: f64,
    pub reverse: bool,
    pub slot: DraftSlot,
}

impl DraftPanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        FeatureKind::Draft(Draft { faces: self.faces.clone(), plane: self.plane.clone(), angle: self.degrees.to_radians(), reverse: self.reverse })
    }
}

/// A split being made or edited.
#[derive(Clone, Debug)]
pub(crate) struct SplitPanel {
    pub editing: Option<FeatureId>,
    /// None until a plane is chosen.
    pub plane: Option<PlaneRef>,
    pub keep: SplitKeep,
    pub body: Option<FaceRef>,
}

impl SplitPanel {
    pub(crate) fn kind(&self) -> Option<FeatureKind> {
        Some(FeatureKind::Split(Split { plane: self.plane.clone()?, keep: self.keep, body: self.body.clone() }))
    }
}

/// Which selector of a combine panel takes clicks in the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CombineSlot {
    Base,
    Tools,
}

/// A combine being made or edited. Bodies are chosen by a face of each.
#[derive(Clone, Debug)]
pub(crate) struct CombinePanel {
    pub editing: Option<FeatureId>,
    pub base: Option<FaceRef>,
    pub tools: Vec<FaceRef>,
    pub operation: Operation,
    pub keep_tools: bool,
    pub slot: CombineSlot,
}

impl CombinePanel {
    pub(crate) fn kind(&self) -> Option<FeatureKind> {
        let base = self.base.clone()?;
        (!self.tools.is_empty())
            .then(|| FeatureKind::Combine(Combine { base, tools: self.tools.clone(), operation: self.operation, keep_tools: self.keep_tools }))
    }
}

/// Join, Cut and Intersect: what bodies can do to each other.
const BODY_OPS: [Operation; 3] = [Operation::Join, Operation::Cut, Operation::Intersect];

impl Workbench {
    /// The part as shown has a solid (the scene may still be regenerating: ask the document).
    fn has_solid(&self) -> bool {
        self.document().features().iter().any(|f| {
            matches!(f.kind, FeatureKind::Extrude(_) | FeatureKind::Revolve(_) | FeatureKind::Sweep(_) | FeatureKind::Coil(_) | FeatureKind::Loft(_))
        })
    }

    pub(crate) fn open_draft(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Draft(d) => DraftPanel {
                    editing,
                    faces: d.faces.clone(),
                    plane: d.plane.clone(),
                    degrees: d.angle.to_degrees(),
                    reverse: d.reverse,
                    slot: DraftSlot::Faces,
                },
                _ => return Err("not a draft".into()),
            },
            None => {
                if !self.has_solid() {
                    return Err("there is no solid to draft yet: extrude or revolve a sketch first".into());
                }
                // The faces already selected; the part is pulled from the XY plane unless another
                // is chosen.
                DraftPanel {
                    editing: None,
                    faces: self.picked_faces(),
                    plane: PlaneRef::Origin(OriginPlane::XY),
                    degrees: 3.0,
                    reverse: false,
                    slot: DraftSlot::Faces,
                }
            }
        };
        self.panel = Some(Panel::Draft(panel));
        self.start_equations();
        self.view.selection.clear();
        self.set_status("Draft: click the faces to tilt, choose the plane they stay put on, set the angle, then OK.");
        Ok(())
    }

    pub(crate) fn open_split(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Split(s) => SplitPanel { editing, plane: Some(s.plane.clone()), keep: s.keep, body: s.body.clone() },
                _ => return Err("not a split".into()),
            },
            None => {
                if !self.has_solid() {
                    return Err("there is no solid to split yet: extrude or revolve a sketch first".into());
                }
                // The last work plane, if there is one: it was most likely made for this.
                let plane =
                    self.document().features().iter().rev().find(|f| matches!(f.kind, FeatureKind::WorkPlane(_))).map(|f| PlaneRef::Work(f.id));
                SplitPanel { editing: None, plane, keep: SplitKeep::Both, body: None }
            }
        };
        self.panel = Some(Panel::Split(panel));
        self.start_equations();
        self.view.selection.clear();
        self.set_status("Split: click the plane that cuts the part (a work plane, an origin plane or a flat face), then OK.");
        Ok(())
    }

    pub(crate) fn open_combine(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Combine(c) => CombinePanel {
                    editing,
                    base: Some(c.base.clone()),
                    tools: c.tools.clone(),
                    operation: c.operation,
                    keep_tools: c.keep_tools,
                    slot: CombineSlot::Tools,
                },
                _ => return Err("not a combine".into()),
            },
            None => {
                // Only when the part shown is the part as it stands: just after opening a file or an
                // edit it is still being rebuilt, and its bodies are not known yet.
                let current = !self.waiting && self.shown.as_ref() == Some(self.document());
                if current && self.scene.bodies.len() < 2 {
                    return Err(
                        "Combine needs two or more solid bodies: make another with New Solid (the last Boolean choice of Extrude, Revolve, Sweep, Coil and Loft), or Split"
                            .into(),
                    );
                }
                CombinePanel { editing: None, base: None, tools: Vec::new(), operation: Operation::Join, keep_tools: false, slot: CombineSlot::Base }
            }
        };
        self.panel = Some(Panel::Combine(panel));
        self.start_equations();
        self.view.selection.clear();
        self.set_status("Combine: click the body that stays, then the bodies to join to it, cut from it or intersect with it.");
        Ok(())
    }

    /// The body a face reference is on, in the part as shown.
    fn body_of(&self, r: &FaceRef) -> Option<usize> {
        find_face(&self.scene, r).map(|f| f.0)
    }

    /// True when the face may go into the open Draft, Split or Combine panel's active selector.
    pub(crate) fn bodies_referable(&self, body: usize, face: u32) -> bool {
        let planar =
            || matches!(self.scene.bodies.get(body).and_then(|b| b.faces.get(face as usize)).map(|f| &f.1.surface), Some(SurfaceKind::Plane { .. }));
        self.face_ref_of(body, face).is_some()
            && match &self.panel {
                Some(Panel::Draft(d)) => d.slot == DraftSlot::Faces || planar(),
                Some(Panel::Split(_)) => planar(),
                Some(Panel::Combine(_)) => true,
                _ => false,
            }
    }

    /// A click on a face while a Draft, Split or Combine panel is open. Returns false when the
    /// panel does not take it.
    pub(crate) fn bodies_pick(&mut self, p: Pick, toggle: bool) -> bool {
        let Pick::Face { body, face } = p else { return false };
        if !self.bodies_referable(body, face) {
            return false;
        }
        let Some(r) = self.face_ref_of(body, face) else { return false };
        // What the panel holds already, as bodies and faces of the part shown.
        let same_face = |me: &Self, x: &FaceRef| find_face(&me.scene, x) == Some((body, face));
        let base_body = match &self.panel {
            Some(Panel::Combine(c)) => c.base.as_ref().and_then(|b| self.body_of(b)),
            _ => None,
        };
        let held_tool = match &self.panel {
            Some(Panel::Combine(c)) => c.tools.iter().position(|t| self.body_of(t) == Some(body)),
            _ => None,
        };
        let held_face = match &self.panel {
            Some(Panel::Draft(d)) => d.faces.iter().position(|f| same_face(self, f)),
            _ => None,
        };
        let mut note = None;
        match &mut self.panel {
            Some(Panel::Draft(d)) if d.slot == DraftSlot::Plane => {
                d.plane = PlaneRef::Face(r);
                d.slot = DraftSlot::Faces;
            }
            Some(Panel::Draft(d)) => match held_face {
                Some(i) if toggle => {
                    d.faces.remove(i);
                }
                Some(_) => {}
                None => d.faces.push(r),
            },
            Some(Panel::Split(s)) => s.plane = Some(PlaneRef::Face(r)),
            Some(Panel::Combine(c)) if c.slot == CombineSlot::Base => {
                // The body that stays cannot also be one of the others.
                if let Some(i) = held_tool {
                    c.tools.remove(i);
                }
                c.base = Some(r);
                c.slot = CombineSlot::Tools;
            }
            Some(Panel::Combine(c)) => match held_tool {
                _ if base_body == Some(body) => note = Some("That is the body that stays: click another body."),
                Some(i) if toggle => {
                    c.tools.remove(i);
                }
                Some(_) => {}
                None => c.tools.push(r),
            },
            _ => return false,
        }
        if let Some(n) = note {
            self.set_error(n.to_string());
        }
        true
    }

    /// The open Draft, Split or Combine panel's references, as picks on the part shown.
    pub(crate) fn bodies_picks(&self) -> Vec<Pick> {
        let face = |r: &FaceRef| find_face(&self.scene, r).map(|(body, face)| Pick::Face { body, face });
        match &self.panel {
            Some(Panel::Draft(d)) => {
                let mut v: Vec<Pick> = d.faces.iter().filter_map(face).collect();
                if let PlaneRef::Face(f) = &d.plane {
                    v.extend(face(f));
                }
                v
            }
            Some(Panel::Split(s)) => match &s.plane {
                Some(PlaneRef::Face(f)) => face(f).into_iter().collect(),
                _ => Vec::new(),
            },
            // Every face of each chosen body: a body is what is picked.
            Some(Panel::Combine(c)) => {
                let bodies: Vec<usize> = c.base.iter().chain(&c.tools).filter_map(|r| self.body_of(r)).collect();
                let mut v = Vec::new();
                for b in bodies {
                    let n = self.scene.bodies.get(b).map_or(0, |x| x.faces.len());
                    v.extend((0..n).filter_map(|f| u32::try_from(f).ok()).map(|face| Pick::Face { body: b, face }));
                }
                v
            }
            _ => Vec::new(),
        }
    }

    /// A work plane or an origin plane chosen for the open Draft or Split panel (in the viewport,
    /// the browser or a list). Returns false when the panel does not take it.
    pub(crate) fn bodies_plane(&mut self, plane: PlaneRef) -> bool {
        match &mut self.panel {
            Some(Panel::Draft(d)) if d.slot == DraftSlot::Plane => {
                d.plane = plane;
                d.slot = DraftSlot::Faces;
                true
            }
            Some(Panel::Split(s)) => {
                s.plane = Some(plane);
                true
            }
            _ => false,
        }
    }

    /// The fields of a draft, split or combine panel. Returns whether Enter was pressed in a
    /// value, and whether Clear was pressed on the picked faces.
    pub(crate) fn bodies_properties(&self, ui: &mut Ui, panel: &mut Panel, eqs: &mut Equations, work_names: &WorkNames, t: &Tokens) -> (bool, bool) {
        let (mut enter, mut clear) = (false, false);
        match panel {
            Panel::Draft(p) => {
                section(ui, "Input Geometry", true, |ui| {
                    egui::Grid::new("tn_props_draft_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Faces");
                        ui.horizontal(|ui| {
                            if slot_button(ui, p.slot == DraftSlot::Faces, "Faces", t) {
                                p.slot = DraftSlot::Faces;
                            }
                            clear |= picked_row(ui, p.faces.len(), "click faces");
                        });
                        ui.end_row();
                        ui.label("Fixed Plane");
                        ui.horizontal(|ui| {
                            let mut pl = Some(p.plane.clone());
                            if plane_picker(ui, "tn_props_draft_plane", &mut pl, p.slot == DraftSlot::Plane, work_names, t) {
                                p.slot = DraftSlot::Plane;
                            }
                            if let Some(pl) = pl {
                                p.plane = pl;
                            }
                        });
                        ui.end_row();
                    });
                });
                section(ui, "Behavior", true, |ui| {
                    egui::Grid::new("tn_props_draft_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Draft Angle");
                        enter |=
                            eq_value_field(ui, egui::Id::new("tn_props_draft_angle"), &mut p.degrees, eqs, "degrees", "deg", 0.01..=85.0, 110.0, t)
                                .entered;
                        ui.end_row();
                        ui.label("Direction");
                        flip_button(ui, &mut p.reverse, t);
                        ui.end_row();
                    });
                });
            }
            Panel::Split(p) => {
                section(ui, "Input Geometry", true, |ui| {
                    egui::Grid::new("tn_props_split_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Split Tool");
                        ui.horizontal(|ui| {
                            plane_picker(ui, "tn_props_split_plane", &mut p.plane, true, work_names, t);
                        });
                        ui.end_row();
                    });
                });
                section(ui, "Behavior", true, |ui| {
                    egui::Grid::new("tn_props_split_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Keep");
                        let name = |k: SplitKeep| match k {
                            SplitKeep::Both => "Both sides, as two solids",
                            SplitKeep::Front => "The side the plane faces",
                            SplitKeep::Back => "The side behind the plane",
                        };
                        egui::ComboBox::from_id_salt("tn_props_split_keep").selected_text(name(p.keep)).show_ui(ui, |ui| {
                            for k in [SplitKeep::Both, SplitKeep::Front, SplitKeep::Back] {
                                ui.selectable_value(&mut p.keep, k, name(k));
                            }
                        });
                        ui.end_row();
                    });
                });
            }
            Panel::Combine(p) => {
                section(ui, "Input Geometry", true, |ui| {
                    egui::Grid::new("tn_props_combine_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Base");
                        if slot_button(ui, p.slot == CombineSlot::Base, if p.base.is_some() { "1 solid" } else { "click a solid" }, t) {
                            p.slot = CombineSlot::Base;
                        }
                        ui.end_row();
                        ui.label("Toolbody");
                        ui.horizontal(|ui| {
                            let label = match p.tools.len() {
                                0 => "click solids".to_string(),
                                1 => "1 solid".to_string(),
                                n => format!("{n} solids"),
                            };
                            if slot_button(ui, p.slot == CombineSlot::Tools, &label, t) {
                                p.slot = CombineSlot::Tools;
                            }
                            if !p.tools.is_empty() && ui.small_button("Clear").clicked() {
                                clear = true;
                            }
                        });
                        ui.end_row();
                    });
                });
                section(ui, "Output", true, |ui| {
                    egui::Grid::new("tn_props_combine_output").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Boolean");
                        let items: Vec<_> = OPS.iter().filter(|o| BODY_OPS.contains(&o.2)).map(|o| (o.0, o.1, true)).collect();
                        let sel = BODY_OPS.iter().position(|o| *o == p.operation).unwrap_or(0);
                        if let Some(i) = glyph_row(ui, &items, sel, t) {
                            p.operation = BODY_OPS[i];
                        }
                        ui.end_row();
                    });
                    ui.checkbox(&mut p.keep_tools, "Keep Toolbody");
                });
            }
            _ => {}
        }
        (enter, clear)
    }
}
