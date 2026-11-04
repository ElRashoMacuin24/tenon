//! Emboss and Derive (DEC-133): their panels, what a new one starts with, and what clicks in the
//! viewport put into them.

use egui::Ui;
use tenon_kernel::SurfaceKind;
use tenon_model::{Derive, Emboss, EmbossStyle, FaceRef, FeatureId, FeatureKind, Operation, PlaneRef, RegionSel};

use crate::Workbench;
use crate::modify::find_face;
use crate::panels::Panel;
use crate::properties::{Equations, WorkNames, eq_value_field, flip_button, plane_picker, section, slot_button};
use crate::sweeps::has_profile;
use crate::theme::Tokens;
use crate::viewport::Pick;

/// An emboss being made or edited.
#[derive(Clone, Debug)]
pub(crate) struct EmbossPanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub regions: RegionSel,
    pub style: EmbossStyle,
    pub depth: f64,
    pub reverse: bool,
    /// The feature being edited has a taper already (zero or not): its parameter stays.
    pub keep_taper: bool,
    /// Degrees its sides lean; zero is straight.
    pub taper: f64,
    /// Wrap to Face is ticked, and the cylinder or cone clicked (None until it is).
    pub wrapping: bool,
    pub face: Option<FaceRef>,
}

impl EmbossPanel {
    /// The feature as set up; None while Wrap to Face waits for its face.
    pub(crate) fn kind(&self) -> Option<FeatureKind> {
        let plain = !self.wrapping;
        let taper = (plain && (self.taper != 0.0 || self.keep_taper)).then(|| self.taper.to_radians());
        let wrap = if plain { None } else { Some(self.face.clone()?) };
        Some(FeatureKind::Emboss(Emboss {
            sketch: self.sketch,
            regions: self.regions.clone(),
            style: self.style,
            depth: self.depth,
            reverse: self.reverse,
            taper,
            wrap,
        }))
    }
}

/// A derived part being made or edited.
#[derive(Clone, Debug)]
pub(crate) struct DerivePanel {
    pub editing: Option<FeatureId>,
    /// The other part's file, in full.
    pub path: String,
    pub scale: f64,
    /// Mirror is ticked, and the plane (None until one is chosen).
    pub mirrored: bool,
    pub mirror: Option<PlaneRef>,
    pub new_solid: bool,
}

impl DerivePanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        FeatureKind::Derive(Derive {
            path: self.path.clone(),
            scale: self.scale,
            mirror: if self.mirrored { self.mirror.clone() } else { None },
            operation: if self.new_solid { Operation::NewBody } else { Operation::Join },
        })
    }
}

/// What the three styles are called.
fn style_name(s: EmbossStyle) -> &'static str {
    match s {
        EmbossStyle::Raise => "Raise from Face",
        EmbossStyle::Recess => "Recess into Face",
        EmbossStyle::FromPlane => "From Sketch Plane",
    }
}

impl Workbench {
    pub(crate) fn open_emboss(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Emboss(e) => EmbossPanel {
                    editing,
                    sketch: e.sketch,
                    regions: e.regions.clone(),
                    style: e.style,
                    depth: e.depth,
                    reverse: e.reverse,
                    keep_taper: e.taper.is_some(),
                    taper: e.taper.unwrap_or(0.0).to_degrees(),
                    wrapping: e.wrap.is_some(),
                    face: e.wrap.clone(),
                },
                _ => return Err("not an emboss".into()),
            },
            None => {
                if !self.has_solid() {
                    return Err("there is nothing to emboss yet: make a solid first".into());
                }
                let sketch = self.profile_sketch()?;
                // A round face selected beforehand is the one to wrap round.
                let face = self.view.selection.iter().find_map(|p| match p {
                    Pick::Face { body, face } if self.round_for_wrap(*body, *face) => self.face_ref_of(*body, *face),
                    _ => None,
                });
                EmbossPanel {
                    editing: None,
                    sketch,
                    regions: RegionSel::Default,
                    style: EmbossStyle::Raise,
                    depth: 1.0,
                    reverse: false,
                    keep_taper: false,
                    taper: 0.0,
                    wrapping: face.is_some(),
                    face,
                }
            }
        };
        self.panel = Some(Panel::Emboss(panel));
        self.start_equations();
        self.view.selection.clear();
        self.set_status("Emboss: choose raise, recess or from the sketch plane, set the depth, then OK. Tick Wrap to Face to lay the profile round a cylinder or cone.");
        Ok(())
    }

    pub(crate) fn open_derive(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Derive(d) => DerivePanel {
                    editing,
                    path: d.path.clone(),
                    scale: d.scale,
                    mirrored: d.mirror.is_some(),
                    mirror: d.mirror.clone(),
                    new_solid: d.operation == Operation::NewBody,
                },
                _ => return Err("not a derived part".into()),
            },
            None => {
                let path = self.choose_derive_file()?;
                DerivePanel { editing: None, path, scale: 1.0, mirrored: false, mirror: None, new_solid: false }
            }
        };
        self.panel = Some(Panel::Derive(panel));
        self.start_equations();
        self.view.selection.clear();
        self.set_status("Derive: the other part's solid comes in at its own place. Set a scale or a mirror plane if it needs them, then OK.");
        Ok(())
    }

    /// Asks which part file to bring in, and reads it so that the panel can show it. An error says
    /// why it cannot be used.
    pub(crate) fn choose_derive_file(&mut self) -> Result<String, String> {
        let chosen = match (&self.services.pick_open_ext, &self.services.pick_open) {
            (Some(f), _) => f("tenon"),
            (None, Some(f)) => f(),
            _ => None,
        };
        let path = chosen.ok_or("no file chosen")?;
        tenon_io::derive::read_for_panel(&mut self.session, &path)
    }

    /// The face may be wrapped round.
    pub(crate) fn round_for_wrap(&self, body: usize, face: u32) -> bool {
        let surface = self.scene.bodies.get(body).and_then(|b| b.faces.get(face as usize)).map(|f| &f.1.surface);
        matches!(surface, Some(SurfaceKind::Cylinder { .. } | SurfaceKind::Cone { .. })) && self.face_ref_of(body, face).is_some()
    }

    /// True when Wrap to Face is waiting for, or can take, a face.
    pub(crate) fn wrap_wanted(&self) -> bool {
        matches!(&self.panel, Some(Panel::Emboss(p)) if p.wrapping)
    }

    /// A click on a face while Wrap to Face is ticked. Returns false when it cannot be wrapped.
    pub(crate) fn wrap_pick(&mut self, p: Pick) -> bool {
        let Pick::Face { body, face } = p else { return false };
        if !self.round_for_wrap(body, face) {
            return false;
        }
        let r = self.face_ref_of(body, face);
        match &mut self.panel {
            Some(Panel::Emboss(e)) if e.wrapping => {
                e.face = r;
                true
            }
            _ => false,
        }
    }

    /// The face the panel wraps round, picked out on the part.
    pub(crate) fn wrap_picks(&self) -> Vec<Pick> {
        match &self.panel {
            Some(Panel::Emboss(e)) if e.wrapping => {
                e.face.as_ref().and_then(|r| find_face(&self.scene, r)).map(|(body, face)| Pick::Face { body, face }).into_iter().collect()
            }
            _ => Vec::new(),
        }
    }

    /// The fields of an emboss or a derived part. Returns whether Enter was pressed in a value.
    pub(crate) fn emboss_properties(
        &mut self,
        ui: &mut Ui,
        panel: &mut Panel,
        eqs: &mut Equations,
        sketches: &[(FeatureId, String)],
        work_names: &WorkNames,
        t: &Tokens,
    ) -> bool {
        let mut enter = false;
        match panel {
            Panel::Emboss(p) => {
                let doc = self.document();
                let name = |id: FeatureId| sketches.iter().find(|s| s.0 == id).map_or_else(|| "?".to_string(), |s| s.1.clone());
                section(ui, "Input Geometry", true, |ui| {
                    egui::Grid::new("tn_props_emboss_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Profiles");
                        ui.label(format!("{} selected", self.profile_count(p.sketch, &p.regions)));
                        ui.end_row();
                        ui.label("Sketch");
                        egui::ComboBox::from_id_salt("tn_props_emboss_sketch").selected_text(name(p.sketch)).show_ui(ui, |ui| {
                            for (id, n) in sketches.iter().filter(|s| has_profile(doc, s.0)) {
                                if ui.selectable_value(&mut p.sketch, *id, n).changed() {
                                    p.regions = RegionSel::Default;
                                }
                            }
                        });
                        ui.end_row();
                    });
                });
                section(ui, "Behavior", true, |ui| {
                    egui::Grid::new("tn_props_emboss_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Type");
                        let styles = [EmbossStyle::Raise, EmbossStyle::Recess, EmbossStyle::FromPlane];
                        let picked = egui::ComboBox::from_id_salt("tn_props_emboss_style").selected_text(style_name(p.style)).show_ui(ui, |ui| {
                            for s in styles {
                                let r = ui.selectable_value(&mut p.style, s, style_name(s));
                                crate::drawing::remember(ui, &format!("tn_emboss_style_{s:?}"), r.rect);
                            }
                        });
                        crate::drawing::remember(ui, "tn_emboss_style", picked.response.rect);
                        ui.end_row();
                        ui.label("Depth").on_hover_text(if p.style == EmbossStyle::FromPlane {
                            "How far from the sketch plane it reaches, each way"
                        } else {
                            "How far from the faces, along their normals"
                        });
                        enter |=
                            eq_value_field(ui, egui::Id::new("tn_props_emboss_depth"), &mut p.depth, eqs, "depth", "mm", 0.001..=100_000.0, 110.0, t)
                                .entered;
                        ui.end_row();
                        ui.label("Direction").on_hover_text(if p.style == EmbossStyle::FromPlane {
                            "Which side of the sketch plane is taken away and which is added"
                        } else {
                            "Which way the faces it goes on look"
                        });
                        flip_button(ui, &mut p.reverse, t);
                        ui.end_row();
                        if !p.wrapping {
                            ui.label("Taper").on_hover_text("How far its sides lean: what is raised narrows going up, what is recessed going down");
                            enter |=
                                eq_value_field(ui, egui::Id::new("tn_props_emboss_taper"), &mut p.taper, eqs, "taper", "deg", -85.0..=85.0, 110.0, t)
                                    .entered;
                            ui.end_row();
                        }
                    });
                    if p.style != EmbossStyle::FromPlane {
                        let tick = ui.checkbox(&mut p.wrapping, "Wrap to Face");
                        crate::drawing::remember(ui, "tn_emboss_wrap", tick.rect);
                        if p.wrapping {
                            ui.horizontal(|ui| {
                                let label = if p.face.is_some() { "1 face" } else { "click a cylinder or cone" };
                                slot_button(ui, true, label, t);
                            });
                            ui.label(
                                egui::RichText::new("The profile is laid round the face, its lengths kept along the round.")
                                    .small()
                                    .color(t.text_dim),
                            );
                        }
                    } else {
                        p.wrapping = false;
                    }
                });
            }
            Panel::Derive(p) => {
                let mut change = false;
                section(ui, "Input Geometry", true, |ui| {
                    egui::Grid::new("tn_props_derive_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Part");
                        let name = std::path::Path::new(&p.path).file_name().map_or_else(|| p.path.clone(), |n| n.to_string_lossy().into_owned());
                        ui.label(egui::RichText::new(name).strong()).on_hover_text(p.path.clone());
                        ui.end_row();
                        ui.label("");
                        change = crate::drawing::button(ui, "Choose...", "tn_derive_choose").clicked();
                        ui.end_row();
                    });
                });
                if change {
                    match self.choose_derive_file() {
                        Ok(path) => p.path = path,
                        Err(e) => self.set_error(e),
                    }
                }
                section(ui, "Behavior", true, |ui| {
                    egui::Grid::new("tn_props_derive_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Scale").on_hover_text("About the part's origin");
                        enter |= eq_value_field(ui, egui::Id::new("tn_props_derive_scale"), &mut p.scale, eqs, "scale", "", 0.001..=1000.0, 110.0, t)
                            .entered;
                        ui.end_row();
                    });
                    let tick = ui.checkbox(&mut p.mirrored, "Mirror");
                    crate::drawing::remember(ui, "tn_derive_mirror", tick.rect);
                    if p.mirrored {
                        ui.horizontal(|ui| {
                            ui.label("Plane");
                            plane_picker(ui, "tn_props_derive_plane", &mut p.mirror, true, work_names, t);
                        });
                    }
                });
                section(ui, "Output", true, |ui| {
                    let tick = ui.checkbox(&mut p.new_solid, "New solid");
                    crate::drawing::remember(ui, "tn_derive_new_solid", tick.rect);
                    ui.label(
                        egui::RichText::new(if p.new_solid {
                            "It stays a solid of its own."
                        } else {
                            "It is joined to the part's last solid; with none, it is the part's first."
                        })
                        .small()
                        .color(t.text_dim),
                    );
                });
            }
            _ => {}
        }
        enter
    }
}
