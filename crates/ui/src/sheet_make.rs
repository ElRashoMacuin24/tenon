//! Sheet metal that makes sheet of its own or stamps it, in the app: the Contour Roll, Lofted Flange
//! and Punch Tool panels, the punch library's window, and what is clicked in the view for them.

use egui::{Pos2, Rect, RichText, Stroke, Ui, vec2};
use serde_json::json;
use tenon_geom::{Frame, Vec2};
use tenon_kernel::{Curve2, SurfaceKind};
use tenon_model::sheet::{self, Forming, PunchShape, Sides};
use tenon_model::{AxisRef, Document, FaceRef, FeatureId, FeatureKind, Operation, OriginAxis, PlaneRef};
use tenon_render::pick::pick_face_in;
use tenon_sketch::{EntityId, Geometry};

use crate::panels::{AxisChoice, Panel};
use crate::properties::{Equations, WorkNames, eq_value_field, flip_button, fmt_value, section};
use crate::sheet::{NO_SHEET, NOT_SHEET, choice};
use crate::theme::{self, Tokens};
use crate::viewport::Pick;
use crate::workbench::Workbench;

fn three(a: &'static str, b: &'static str, c: &'static str) -> [(Sides, &'static str, &'static str, &'static str); 3] {
    [(Sides::Default, "default", "Default", a), (Sides::Flipped, "flipped", "Flipped", b), (Sides::Symmetric, "symmetric", "Symmetric", c)]
}

/// Which side of a profile the sheet's thickness goes.
fn positions() -> [(Sides, &'static str, &'static str, &'static str); 3] {
    three(
        "Inside the profile's first corner: the profile is the outside of the part there",
        "Outside the profile",
        "Half each side: the profile is the middle of the sheet",
    )
}

/// A strip of sheet rolled round an axis.
#[derive(Clone, Debug)]
pub(crate) struct RollPanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub axis: AxisChoice,
    pub degrees: f64,
    pub direction: Sides,
    pub position: Sides,
    /// A bend radius of its own, in place of the rule's.
    pub own_radius: bool,
    pub radius: f64,
    pub new_solid: bool,
}

impl RollPanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        FeatureKind::ContourRoll(sheet::ContourRoll {
            sketch: self.sketch,
            axis: match self.axis {
                AxisChoice::Origin(a) => AxisRef::Origin(a),
                AxisChoice::Line(l) => AxisRef::SketchLine(l),
                AxisChoice::Work(w) => AxisRef::Work(w),
            },
            angle: self.degrees.to_radians(),
            direction: self.direction,
            position: self.position,
            radius: self.own_radius.then_some(self.radius),
            operation: if self.new_solid { Operation::NewBody } else { Operation::Join },
        })
    }

    pub(crate) fn eq_pointers(&self) -> Vec<(&'static str, &'static str)> {
        let mut v = vec![("degrees", "/angle")];
        if self.own_radius {
            v.push(("radius", "/radius"));
        }
        v
    }
}

/// A sheet between two open profiles.
#[derive(Clone, Debug)]
pub(crate) struct LoftedPanel {
    pub editing: Option<FeatureId>,
    pub sketches: [Option<FeatureId>; 2],
    /// Flat facets with bends between them, not one smooth surface.
    pub press_brake: bool,
    pub chord: f64,
    pub position: Sides,
    pub own_radius: bool,
    pub radius: f64,
    pub new_solid: bool,
}

impl LoftedPanel {
    /// The feature, once both profiles are picked.
    pub(crate) fn kind(&self) -> Option<FeatureKind> {
        let (Some(a), Some(b)) = (self.sketches[0], self.sketches[1]) else { return None };
        Some(FeatureKind::LoftedFlange(sheet::LoftedFlange {
            sketches: [a, b],
            forming: if self.press_brake { Forming::PressBrake { chord: self.chord } } else { Forming::DieFormed },
            position: self.position,
            radius: self.own_radius.then_some(self.radius),
            operation: if self.new_solid { Operation::NewBody } else { Operation::Join },
        }))
    }

    pub(crate) fn eq_pointers(&self) -> Vec<(&'static str, &'static str)> {
        let mut v = Vec::new();
        if self.press_brake {
            v.push(("chord", "/forming/press_brake/chord"));
        }
        if self.own_radius {
            v.push(("radius", "/radius"));
        }
        v
    }
}

/// Shapes from the punch library stamped into a face of the sheet.
#[derive(Clone, Debug)]
pub(crate) struct PunchPanel {
    pub editing: Option<FeatureId>,
    /// The sketch of its centre points; with none, they are clicked on a face.
    pub sketch: Option<FeatureId>,
    /// Which of the sketch's points; none: every centre point.
    pub points: Vec<EntityId>,
    pub shape: PunchShape,
    pub degrees: f64,
    pub flip: bool,
    /// Stamps placed by clicking a flat face: the face, its sketch frame, and the centres clicked
    /// on it (in that frame). On OK a sketch on the face with these points is made with the
    /// punch, in one step.
    pub face: Option<(FaceRef, Frame)>,
    pub face_points: Vec<Vec2>,
    /// The library's window is open beside the panel.
    pub library: bool,
}

impl PunchPanel {
    /// The feature, once it has a sketch.
    pub(crate) fn kind(&self) -> Option<FeatureKind> {
        let sketch = self.sketch?;
        Some(FeatureKind::Punch(sheet::Punch {
            sketch,
            points: self.points.clone(),
            shape: self.shape,
            rotation: self.degrees.to_radians(),
            flip: self.flip,
        }))
    }

    pub(crate) fn eq_pointers(&self) -> Vec<(&'static str, &'static str)> {
        let mut v = vec![("degrees", "/rotation")];
        v.extend(match self.shape {
            PunchShape::Round { .. } => vec![("diameter", "/shape/round/diameter")],
            PunchShape::Square { .. } => vec![("side", "/shape/square/side")],
            PunchShape::Rectangle { .. } => vec![("length", "/shape/rectangle/length"), ("width", "/shape/rectangle/width")],
            PunchShape::Obround { .. } => vec![("length", "/shape/obround/length"), ("width", "/shape/obround/width")],
            PunchShape::DShape { .. } => vec![("diameter", "/shape/d_shape/diameter"), ("across", "/shape/d_shape/across")],
            PunchShape::Dimple { .. } => vec![("diameter", "/shape/dimple/diameter"), ("depth", "/shape/dimple/depth")],
            PunchShape::Louver { .. } => {
                vec![("length", "/shape/louver/length"), ("width", "/shape/louver/width"), ("height", "/shape/louver/height")]
            }
        });
        v
    }

    /// Its stamps are placed by clicking a face.
    pub(crate) fn on_face(&self) -> bool {
        self.editing.is_none() && self.sketch.is_none()
    }

    /// The document with the sketch its face clicks make, and the punch on it: the punch as it
    /// would be made.
    pub(crate) fn on_face_document(&self, doc: &Document) -> Option<(Document, FeatureKind)> {
        let ((face, _), points) = (self.face.as_ref()?, &self.face_points);
        if points.is_empty() {
            return None;
        }
        let mut d = doc.clone();
        let mut sk = tenon_sketch::Sketch::new();
        let ids: Vec<EntityId> = points.iter().filter_map(|p| sk.add_point(*p).ok()).collect();
        let sketch = d.add(FeatureKind::Sketch { plane: PlaneRef::Face(face.clone()), sketch: sk, projections: Vec::new() }).ok()?;
        let kind = FeatureKind::Punch(sheet::Punch { sketch, points: ids, shape: self.shape, rotation: self.degrees.to_radians(), flip: self.flip });
        d.add(kind.clone()).ok()?;
        Some((d, kind))
    }
}

/// The sizes of a shape, each with what the panel calls it, its tip and its equation key.
fn sizes_of(shape: &mut PunchShape) -> Vec<(&'static str, &'static str, &'static str, &mut f64)> {
    let tips = sheet::punch::family_of(shape).map_or(&[][..], |f| f.sizes);
    let found = |key: &str| tips.iter().find(|s| s.0 == key).map_or(("", ""), |s| (s.1, s.2));
    let row = |key: &'static str, v| {
        let (label, tip) = found(key);
        (label, tip, key, v)
    };
    match shape {
        PunchShape::Round { diameter } => vec![row("diameter", diameter)],
        PunchShape::Square { side } => vec![row("side", side)],
        PunchShape::Rectangle { length, width } | PunchShape::Obround { length, width } => vec![row("length", length), row("width", width)],
        PunchShape::DShape { diameter, across } => vec![row("diameter", diameter), row("across", across)],
        PunchShape::Dimple { diameter, depth } => vec![row("diameter", diameter), row("depth", depth)],
        PunchShape::Louver { length, width, height } => vec![row("length", length), row("width", width), row("height", height)],
    }
}

/// A bend radius by the rule or of its own. Returns whether Enter was pressed in it.
#[allow(clippy::too_many_arguments)]
fn radius_section(ui: &mut Ui, key: &str, own: &mut bool, radius: &mut f64, eqs: &mut Equations, rule: Option<f64>, tip: &str, t: &Tokens) -> bool {
    let mut enter = false;
    section(ui, "Bend", true, |ui| {
        egui::Grid::new(format!("tn_props_{key}_bend")).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Bend Radius").on_hover_text(tip);
            let mut by_rule = !*own;
            let r = ui.checkbox(&mut by_rule, "By rule");
            crate::drawing::remember(ui, &format!("tn_{key}_by_rule"), r.rect);
            r.on_hover_text("The part's BendRadius, from Sheet Metal Defaults");
            if by_rule == *own {
                *own = !by_rule;
                if let (false, Some(s)) = (*own, rule) {
                    *radius = s;
                }
            }
            ui.end_row();
            ui.label("");
            if *own {
                enter |= eq_value_field(ui, egui::Id::new(format!("tn_props_{key}_r")), radius, eqs, "radius", "mm", 0.01..=1000.0, 110.0, t).entered;
            } else {
                ui.label(RichText::new(rule.map_or("?".into(), |s| format!("{} mm", fmt_value(s)))).color(t.text_dim));
            }
            ui.end_row();
        });
    });
    enter
}

/// The note under a panel: what it comes to, in words.
fn note(ui: &mut Ui, key: &str, words: &str, t: &Tokens) {
    let r = ui.add(egui::Label::new(RichText::new(words).font(theme::small()).color(t.text_dim)).wrap());
    crate::drawing::remember(ui, key, r.rect);
}

impl Workbench {
    /// What a lofted flange or a contour roll says of itself: how it is laid flat, and how
    /// close that is. For the browser, and its panel.
    pub(crate) fn feature_words(&self, kind: &FeatureKind) -> Option<Result<String, String>> {
        let doc = self.document();
        match kind {
            FeatureKind::ContourRoll(c) => Some(tenon_model::regen::roll_words(c, doc.sketch(c.sketch)?)),
            FeatureKind::LoftedFlange(l) => {
                let sizes = doc.sheet_sizes().ok()?;
                let sketches = [
                    (doc.sketch(l.sketches[0])?, self.sketch_frame(l.sketches[0])?),
                    (doc.sketch(l.sketches[1])?, self.sketch_frame(l.sketches[1])?),
                ];
                Some(tenon_model::regen::lofted_words(l, sketches, &sizes))
            }
            _ => None,
        }
    }

    pub(crate) fn open_contour_roll(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let sizes = self.document().sheet_sizes().map_err(|e| if self.document().sheet_metal().is_none() { NOT_SHEET.to_owned() } else { e })?;
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::ContourRoll(c) => RollPanel {
                    editing,
                    sketch: c.sketch,
                    axis: match c.axis {
                        AxisRef::Origin(a) => AxisChoice::Origin(a),
                        AxisRef::SketchLine(l) => AxisChoice::Line(l),
                        AxisRef::Work(w) => AxisChoice::Work(w),
                    },
                    degrees: c.angle.to_degrees(),
                    direction: c.direction,
                    position: c.position,
                    own_radius: c.radius.is_some(),
                    radius: c.radius.unwrap_or(sizes.bend_radius),
                    new_solid: c.operation == Operation::NewBody,
                },
                _ => return Err("not a contour roll".into()),
            },
            None => {
                let sketch = self.open_profile_sketch()?;
                let axis = self.default_roll_axis(sketch);
                RollPanel {
                    editing: None,
                    sketch,
                    axis,
                    degrees: 180.0,
                    direction: Sides::Default,
                    position: Sides::Default,
                    own_radius: false,
                    radius: sizes.bend_radius,
                    new_solid: false,
                }
            }
        };
        self.panel = Some(Panel::ContourRoll(panel));
        self.start_equations();
        self.view.selection.clear();
        self.set_status("Contour Roll: the profile is rolled round the axis as sheet. Click a line of its sketch or an axis to roll about, set the angle, then OK.");
        Ok(())
    }

    /// The axis a new contour roll of `sketch` starts with: its centre line, else a construction
    /// line, else the origin axis in its plane that the profile lies clear of.
    fn default_roll_axis(&self, sketch: FeatureId) -> AxisChoice {
        let doc = self.document();
        let Some(sk) = doc.sketch(sketch) else { return AxisChoice::Origin(OriginAxis::Z) };
        if let Some((id, _)) = sk.entities().find(|(_, e)| e.centerline) {
            return AxisChoice::Line(id);
        }
        if let Some((id, _)) = sk.entities().find(|(id, e)| e.construction && sk.is_line(*id)) {
            return AxisChoice::Line(id);
        }
        let Some(frame) = self.sketch_frame(sketch) else { return AxisChoice::Origin(OriginAxis::Z) };
        let in_plane: Vec<OriginAxis> =
            [OriginAxis::Z, OriginAxis::Y, OriginAxis::X].into_iter().filter(|a| a.axis().dir().dot(frame.z()).abs() < 1e-6).collect();
        let points = sk.extent();
        let clear = |a: &OriginAxis| {
            let axis = a.axis();
            let side = frame.z().cross(axis.dir());
            let s: Vec<f64> = points.iter().map(|q| (frame.plane_point(*q) - axis.origin()).dot(side)).collect();
            s.iter().all(|x| *x > 1e-6) || s.iter().all(|x| *x < -1e-6)
        };
        AxisChoice::Origin(in_plane.iter().find(|a| clear(a)).or(in_plane.first()).copied().unwrap_or(OriginAxis::Z))
    }

    /// The sketches that hold an open profile: lines and arcs, and no closed region.
    fn open_profiles(&self) -> Vec<FeatureId> {
        let doc = self.document();
        self.sketches()
            .iter()
            .map(|s| s.0)
            .filter(|id| {
                doc.sketch(*id).is_some_and(|sk| {
                    sk.entities().any(|(_, e)| !e.construction && !e.reference && matches!(e.geometry, Geometry::Line { .. } | Geometry::Arc { .. }))
                        && tenon_sketch::regions(sk).is_empty()
                })
            })
            .collect()
    }

    pub(crate) fn open_lofted_flange(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let sizes = self.document().sheet_sizes().map_err(|e| if self.document().sheet_metal().is_none() { NOT_SHEET.to_owned() } else { e })?;
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::LoftedFlange(l) => LoftedPanel {
                    editing,
                    sketches: [Some(l.sketches[0]), Some(l.sketches[1])],
                    press_brake: matches!(l.forming, Forming::PressBrake { .. }),
                    chord: if let Forming::PressBrake { chord } = l.forming { chord } else { 0.5 },
                    position: l.position,
                    own_radius: l.radius.is_some(),
                    radius: l.radius.unwrap_or(sizes.bend_radius),
                    new_solid: l.operation == Operation::NewBody,
                },
                _ => return Err("not a lofted flange".into()),
            },
            None => {
                if matches!(self.mode, crate::workbench::Mode::Sketch(_)) {
                    self.finish_sketch();
                }
                // The last two open profiles nothing uses yet, in order.
                let doc = self.document();
                let used: std::collections::BTreeSet<FeatureId> = doc.features().iter().flat_map(|f| f.kind.depends_on()).collect();
                let free: Vec<FeatureId> = self.open_profiles().into_iter().filter(|id| !used.contains(id)).collect();
                if free.is_empty() {
                    return Err(
                        "draw two open profiles first, each in a sketch of its own: lines and arcs end to end, as the sheet's two ends".into()
                    );
                }
                let take = |n: usize| free.len().checked_sub(n).and_then(|i| free.get(i)).copied();
                let sketches = if free.len() >= 2 { [take(2), take(1)] } else { [take(1), None] };
                LoftedPanel {
                    editing: None,
                    sketches,
                    press_brake: false,
                    chord: 0.5,
                    position: Sides::Default,
                    own_radius: false,
                    radius: sizes.bend_radius,
                    new_solid: false,
                }
            }
        };
        let both = panel.sketches.iter().all(Option::is_some);
        self.panel = Some(Panel::LoftedFlange(panel));
        self.start_equations();
        self.view.selection.clear();
        self.set_status(if both {
            "Lofted Flange: a sheet between the two profiles. Click a profile to change it, choose how it is formed, then OK."
        } else {
            "Lofted Flange: click the second profile (a sketch of lines and arcs), then OK."
        });
        Ok(())
    }

    pub(crate) fn open_punch(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Punch(p) => PunchPanel {
                    editing,
                    sketch: Some(p.sketch),
                    points: p.points.clone(),
                    shape: p.shape,
                    degrees: p.rotation.to_degrees(),
                    flip: p.flip,
                    face: None,
                    face_points: Vec::new(),
                    library: false,
                },
                _ => return Err("not a punch".into()),
            },
            None => {
                self.document().sheet_sizes().map_err(|e| if self.document().sheet_metal().is_none() { NOT_SHEET.to_owned() } else { e })?;
                if !self.has_sheet() {
                    return Err(NO_SHEET.into());
                }
                // The sketch being edited, else the last one of centre points that nothing uses.
                let sketch = match &self.mode {
                    crate::workbench::Mode::Sketch(s) => {
                        let id = s.feature;
                        self.finish_sketch();
                        Some(id)
                    }
                    crate::workbench::Mode::Model => {
                        let doc = self.document();
                        let used: std::collections::BTreeSet<FeatureId> = doc.features().iter().flat_map(|f| f.kind.depends_on()).collect();
                        self.sketches()
                            .iter()
                            .rev()
                            .map(|s| s.0)
                            .find(|id| !used.contains(id) && doc.sketch(*id).is_some_and(|sk| !tenon_model::hole_centres(sk).is_empty()))
                    }
                };
                PunchPanel {
                    editing: None,
                    sketch: sketch.filter(|id| self.document().sketch(*id).is_some_and(|sk| !tenon_model::hole_centres(sk).is_empty())),
                    points: Vec::new(),
                    shape: sheet::PUNCH_LIBRARY[0].starts,
                    degrees: 0.0,
                    flip: false,
                    face: None,
                    face_points: Vec::new(),
                    library: true,
                }
            }
        };
        let on_face = panel.on_face();
        self.panel = Some(Panel::Punch(panel));
        self.start_equations();
        self.view.selection.clear();
        self.set_status(if on_face {
            "Punch Tool: choose a shape, click a flat face of the sheet where each stamp goes, then OK."
        } else {
            "Punch Tool: choose a shape: it is stamped at each centre point of the sketch. Then OK."
        });
        Ok(())
    }

    /// The fields of these three panels. Returns whether Enter was pressed in a value.
    pub(crate) fn sheet_make_properties(
        &self,
        ui: &mut Ui,
        panel: &mut Panel,
        eqs: &mut Equations,
        sketches: &[(FeatureId, String)],
        work_names: &WorkNames,
        t: &Tokens,
    ) -> bool {
        let mut enter = false;
        let sizes = self.document().sheet_sizes().ok();
        let name = |id: FeatureId| sketches.iter().find(|s| s.0 == id).map_or("?".to_owned(), |s| s.1.clone());
        match panel {
            Panel::ContourRoll(p) => {
                let lines: Vec<EntityId> = self
                    .document()
                    .sketch(p.sketch)
                    .map_or_else(Vec::new, |sk| sk.entities().map(|(id, _)| id).filter(|id| sk.is_line(*id)).collect());
                section(ui, "Input Geometry", true, |ui| {
                    egui::Grid::new("tn_props_roll_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Profile").on_hover_text("An open profile: lines and arcs end to end, all on one side of the axis");
                        let doc = self.document();
                        egui::ComboBox::from_id_salt("tn_props_roll_sketch").selected_text(name(p.sketch)).show_ui(ui, |ui| {
                            for (id, n) in sketches.iter().filter(|s| p.editing.is_none_or(|e| doc.index_of(s.0) < doc.index_of(e))) {
                                ui.selectable_value(&mut p.sketch, *id, n);
                            }
                        });
                        ui.end_row();
                        ui.label("Axis")
                            .on_hover_text("What it is rolled about, in the sketch's plane: click a line of the sketch or an axis in the view");
                        let axis_name = |a: &AxisChoice| match a {
                            AxisChoice::Origin(o) => format!("{o:?} Axis"),
                            AxisChoice::Line(l) => format!("Sketch line {}", l.0),
                            AxisChoice::Work(w) => work_names.iter().find(|x| x.0 == *w).map_or("Work Axis".into(), |x| x.1.clone()),
                        };
                        let list = egui::ComboBox::from_id_salt("tn_props_roll_axis").selected_text(axis_name(&p.axis)).show_ui(ui, |ui| {
                            for o in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
                                let r = ui.selectable_value(&mut p.axis, AxisChoice::Origin(o), format!("{o:?} Axis"));
                                crate::drawing::remember(ui, &format!("tn_roll_axis_{o:?}"), r.rect);
                            }
                            for l in &lines {
                                ui.selectable_value(&mut p.axis, AxisChoice::Line(*l), format!("Sketch line {}", l.0));
                            }
                            for (w, n, _) in work_names.iter().filter(|x| x.2 == "axis") {
                                ui.selectable_value(&mut p.axis, AxisChoice::Work(*w), n);
                            }
                        });
                        crate::drawing::remember(ui, "tn_roll_axis", list.response.rect);
                        ui.end_row();
                    });
                });
                section(ui, "Behavior", true, |ui| {
                    egui::Grid::new("tn_props_roll_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Rolled Angle").on_hover_text("How far round the axis, up to 359.9 degrees: a sheet has a seam");
                        enter |=
                            eq_value_field(ui, egui::Id::new("tn_props_roll_angle"), &mut p.degrees, eqs, "degrees", "deg", 1.0..=359.9, 110.0, t)
                                .entered;
                        ui.end_row();
                        ui.label("Direction").on_hover_text("Which way round the axis from the sketch");
                        choice(
                            ui,
                            "tn_roll_direction",
                            &mut p.direction,
                            &three("The way the axis turns", "The other way", "Half each way from the sketch"),
                        );
                        ui.end_row();
                        ui.label("Position").on_hover_text("Which side of the profile the sheet's thickness goes");
                        choice(ui, "tn_roll_position", &mut p.position, &positions());
                        ui.end_row();
                    });
                });
                enter |= radius_section(
                    ui,
                    "roll",
                    &mut p.own_radius,
                    &mut p.radius,
                    eqs,
                    sizes.map(|s| s.bend_radius),
                    "Of the bends made where two lines of the profile meet at a corner",
                    t,
                );
                if !self.scene.bodies.is_empty() || p.new_solid {
                    section(ui, "Output", true, |ui| {
                        ui.checkbox(&mut p.new_solid, "New solid").on_hover_text("A sheet of its own, not joined to the one it touches");
                    });
                }
                if let Some(Ok(words)) = self.feature_words(&p.kind()) {
                    note(ui, "tn_roll_note", &words, t);
                }
            }
            Panel::LoftedFlange(p) => {
                section(ui, "Input Geometry", true, |ui| {
                    egui::Grid::new("tn_props_lofted_input").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        let profiles = self.open_profiles();
                        for (i, label) in ["First Profile", "Second Profile"].into_iter().enumerate() {
                            ui.label(label).on_hover_text("An open profile: lines and arcs end to end. Click it in the view, or choose it here");
                            let shown = p.sketches[i].map_or("Click a sketch".to_owned(), name);
                            let list = egui::ComboBox::from_id_salt(("tn_props_lofted_sketch", i)).selected_text(shown).show_ui(ui, |ui| {
                                for id in &profiles {
                                    ui.selectable_value(&mut p.sketches[i], Some(*id), name(*id));
                                }
                            });
                            crate::drawing::remember(ui, &format!("tn_lofted_sketch_{i}"), list.response.rect);
                            ui.end_row();
                        }
                    });
                });
                section(ui, "Behavior", true, |ui| {
                    egui::Grid::new("tn_props_lofted_behavior").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Forming").on_hover_text("How the sheet is made, which is also how it is laid flat");
                        choice(
                            ui,
                            "tn_lofted_forming",
                            &mut p.press_brake,
                            &[
                                (
                                    false,
                                    "die",
                                    "Die Formed",
                                    "One smooth surface, pressed in a die. It is laid flat by triangles, which is close and not exact",
                                ),
                                (true, "brake", "Press Brake", "Flat facets with a bend between each two, for a press brake. They unfold exactly"),
                            ],
                        );
                        ui.end_row();
                        if p.press_brake {
                            ui.label("Chord Tolerance").on_hover_text("How far a facet may leave the arc of a profile: smaller makes more facets");
                            enter |= eq_value_field(
                                ui,
                                egui::Id::new("tn_props_lofted_chord"),
                                &mut p.chord,
                                eqs,
                                "chord",
                                "mm",
                                0.001..=1000.0,
                                110.0,
                                t,
                            )
                            .entered;
                            ui.end_row();
                        }
                        ui.label("Position").on_hover_text("Which side of the profiles the sheet's thickness goes");
                        choice(ui, "tn_lofted_position", &mut p.position, &positions());
                        ui.end_row();
                    });
                });
                enter |= radius_section(
                    ui,
                    "lofted",
                    &mut p.own_radius,
                    &mut p.radius,
                    eqs,
                    sizes.map(|s| s.bend_radius),
                    if p.press_brake { "Of the bends between the facets" } else { "Of the corners of the profiles, which are rounded to it" },
                    t,
                );
                if !self.scene.bodies.is_empty() || p.new_solid {
                    section(ui, "Output", true, |ui| {
                        ui.checkbox(&mut p.new_solid, "New solid").on_hover_text("A sheet of its own, not joined to the one it touches");
                    });
                }
                if let Some(kind) = p.kind()
                    && let Some(words) = self.feature_words(&kind)
                {
                    note(ui, "tn_lofted_note", &words.unwrap_or_else(|e| e), t);
                }
            }
            Panel::Punch(p) => {
                let family = sheet::punch::family_of(&p.shape);
                section(ui, "Shape", true, |ui| {
                    if crate::drawing::button(ui, "Library...", "tn_punch_library_open")
                        .on_hover_text("The punch library's window: every shape, by kind, with a picture of it")
                        .clicked()
                    {
                        p.library = true;
                    }
                    egui::Grid::new("tn_props_punch_shape").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Shape").on_hover_text("A shape of the punch library. Library shows them all");
                        let list =
                            egui::ComboBox::from_id_salt("tn_props_punch_family").selected_text(family.map_or("?", |f| f.name)).show_ui(ui, |ui| {
                                for f in &sheet::PUNCH_LIBRARY {
                                    let r = ui.selectable_label(family.is_some_and(|x| x.id == f.id), f.name);
                                    crate::drawing::remember(ui, &format!("tn_punch_family_{}", f.id), r.rect);
                                    if r.on_hover_text(f.about).clicked() {
                                        p.shape = f.starts;
                                    }
                                }
                            });
                        crate::drawing::remember(ui, "tn_punch_family", list.response.rect);
                        ui.end_row();
                        let keys: Vec<(&'static str, &'static str, &'static str, &mut f64)> = sizes_of(&mut p.shape);
                        for (label, tip, key, value) in keys {
                            ui.label(label).on_hover_text(tip);
                            enter |=
                                eq_value_field(ui, egui::Id::new(format!("tn_props_punch_{key}")), value, eqs, key, "mm", 0.01..=10_000.0, 110.0, t)
                                    .entered;
                            ui.end_row();
                        }
                        ui.label("Rotation").on_hover_text("How far the shape is turned, from the sketch's X");
                        enter |= eq_value_field(
                            ui,
                            egui::Id::new("tn_props_punch_rotation"),
                            &mut p.degrees,
                            eqs,
                            "degrees",
                            "deg",
                            -360.0..=360.0,
                            110.0,
                            t,
                        )
                        .entered;
                        ui.end_row();
                        if p.shape.is_formed() {
                            ui.label("Direction").on_hover_text("Which side of the sheet it stands out of");
                            flip_button(ui, &mut p.flip, t);
                            ui.end_row();
                        }
                    });
                });
                section(ui, "Placement", true, |ui| {
                    egui::Grid::new("tn_props_punch_place").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Centres").on_hover_text("A sketch's centre points, on a flat face of the sheet; or clicks on a face");
                        let shown = p.sketch.map_or("Clicks on a face".to_owned(), name);
                        let doc = self.document();
                        let with_points: Vec<FeatureId> = sketches
                            .iter()
                            .map(|s| s.0)
                            .filter(|id| doc.sketch(*id).is_some_and(|sk| !tenon_model::hole_centres(sk).is_empty()))
                            .collect();
                        let list = egui::ComboBox::from_id_salt("tn_props_punch_sketch").selected_text(shown).show_ui(ui, |ui| {
                            if p.editing.is_none() {
                                ui.selectable_value(&mut p.sketch, None, "Clicks on a face");
                            }
                            for id in with_points.iter().filter(|id| p.editing.is_none_or(|e| doc.index_of(**id) < doc.index_of(e))) {
                                let r = ui.selectable_value(&mut p.sketch, Some(*id), name(*id));
                                crate::drawing::remember(ui, &format!("tn_punch_sketch_{}", id.0), r.rect);
                                if r.clicked() {
                                    // The points were of the sketch it had: of another, all its centres.
                                    p.points = match p.editing.and_then(|e| doc.feature(e)).map(|f| &f.kind) {
                                        Some(FeatureKind::Punch(was)) if was.sketch == *id => was.points.clone(),
                                        _ => Vec::new(),
                                    };
                                }
                            }
                        });
                        crate::drawing::remember(ui, "tn_punch_sketch", list.response.rect);
                        ui.end_row();
                        let stamps = match p.sketch {
                            Some(id) if p.points.is_empty() => doc.sketch(id).map_or(0, |sk| tenon_model::hole_centres(sk).len()),
                            Some(_) => p.points.len(),
                            None => p.face_points.len(),
                        };
                        ui.label("Stamps");
                        ui.label(if stamps == 0 { "None yet: click a flat face".to_owned() } else { stamps.to_string() });
                        ui.end_row();
                    });
                });
            }
            _ => {}
        }
        enter
    }

    /// The closest sketch curve of an open profile under `pos`.
    fn profile_at(&self, pos: Pos2, rect: Rect) -> Option<FeatureId> {
        let doc = self.document();
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let mut best: Option<(FeatureId, f32)> = None;
        // (A feature being edited goes only by what comes before it.)
        let editing = if let Some(Panel::LoftedFlange(p)) = &self.panel { p.editing } else { None };
        for id in self.open_profiles().into_iter().filter(|id| editing.is_none_or(|e| doc.index_of(*id) < doc.index_of(e))) {
            let (Some(sk), Some(frame)) = (doc.sketch(id), self.sketch_frame(id)) else { continue };
            let screen = |q: Vec2| self.view.camera.project(frame.plane_point(q), w, h).map(|(x, y, _)| rect.min + vec2(x as f32, y as f32));
            for (e, entity) in sk.entities() {
                if entity.construction || entity.reference {
                    continue;
                }
                let points: Vec<Vec2> = if let Some((a, b)) = sk.line(e) {
                    vec![a, b]
                } else if let Some(arc) = sk.arc(e) {
                    (0..=24).map(|i| Vec2::polar(arc.center, arc.radius, arc.start + arc.sweep() * f64::from(i) / 24.0)).collect()
                } else {
                    continue;
                };
                let on_screen: Vec<Pos2> = points.into_iter().filter_map(screen).collect();
                for pair in on_screen.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    let ab = b - a;
                    let f = if ab.length_sq() > 0.0 { ((pos - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
                    let d = (a + ab * f).distance(pos);
                    if d <= 7.0 && best.is_none_or(|x| d < x.1) {
                        best = Some((id, d));
                    }
                }
            }
        }
        best.map(|b| b.0)
    }

    /// A click on an open profile while a Lofted Flange is open: it is the second profile when
    /// that is still to be picked, and taken back when it is one of the two.
    pub(crate) fn lofted_pointer(&mut self, resp: &egui::Response, rect: Rect) {
        self.view.hover = None;
        if !resp.clicked() {
            return;
        }
        let Some(id) = resp.interact_pointer_pos().and_then(|pos| self.profile_at(pos, rect)) else { return };
        let Some(Panel::LoftedFlange(p)) = &mut self.panel else { return };
        if let Some(at) = p.sketches.iter().position(|s| *s == Some(id)) {
            p.sketches[at] = None;
        } else if let Some(free) = p.sketches.iter().position(Option::is_none) {
            p.sketches[free] = Some(id);
        } else {
            p.sketches[1] = Some(id);
        }
        let ready = p.sketches.iter().all(Option::is_some);
        self.set_status(if ready {
            "Lofted Flange: both profiles are picked. Choose how it is formed, then OK."
        } else {
            "Lofted Flange: click the other profile."
        });
    }

    /// Where the centres clicked on a face are on the screen.
    fn punch_face_marks(&self, rect: Rect) -> Vec<Pos2> {
        let Some(Panel::Punch(p)) = &self.panel else { return Vec::new() };
        let Some((_, frame)) = &p.face else { return Vec::new() };
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        p.face_points
            .iter()
            .filter_map(|v| self.view.camera.project(frame.plane_point(*v), w, h))
            .map(|(x, y, _)| rect.min + vec2(x as f32, y as f32))
            .collect()
    }

    /// A click on a flat face while a Punch is placed by clicks: a stamp there, on the first face
    /// clicked; one on a stamp already there takes it away.
    pub(crate) fn punch_face_pick(&mut self, resp: &egui::Response, rect: Rect) {
        let Some(pos) = resp.hover_pos().or_else(|| resp.interact_pointer_pos()) else { return };
        let near = self.punch_face_marks(rect).iter().position(|m| m.distance(pos) <= 9.0);
        let (x, y, w, h) = (f64::from(pos.x - rect.left()), f64::from(pos.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
        let (o, d) = self.view.camera.ray(x, y, w, h);
        let hit = {
            let (meshes, tree) = (self.scene.placed(), self.body_tree());
            pick_face_in(tree.as_deref(), &meshes, o, d)
        };
        let flat = hit.as_ref().and_then(|f| {
            let info = &self.scene.bodies.get(f.body)?.faces.get(f.face as usize)?.1;
            let frame = matches!(info.surface, SurfaceKind::Plane { .. }).then(|| tenon_model::frame_on_face(&info.surface)).flatten()?;
            Some((self.face_ref_of(f.body, f.face)?, frame, f.point))
        });
        self.view.hover = if near.is_none() { hit.map(|f| Pick::Face { body: f.body, face: f.face }).filter(|_| flat.is_some()) } else { None };
        if !resp.clicked() {
            return;
        }
        if let Some(i) = near {
            if let Some(Panel::Punch(p)) = &mut self.panel {
                p.face_points.remove(i);
            }
            return;
        }
        let Some((face, frame, point)) = flat else {
            self.set_error("Click a flat face of the sheet where the stamp goes.");
            return;
        };
        let Some(Panel::Punch(p)) = &mut self.panel else { return };
        match &p.face {
            Some((f, _)) if f.origin != face.origin => {
                self.set_error("These stamps go on the face clicked first: OK them, then start another Punch for this face.");
                return;
            }
            Some(_) => {}
            None => p.face = Some((face, frame)),
        }
        let at = frame.to_local(point);
        // (To a hundredth of a millimetre: where the pointer happened to be.)
        let tidy = |v: f64| (v * 100.0).round() / 100.0;
        p.face_points.push(Vec2::new(tidy(at.x), tidy(at.y)));
        let n = p.face_points.len();
        self.set_status(format!(
            "{n} stamp{} on the face. Click for more; edit its sketch afterwards to dimension them.",
            if n == 1 { "" } else { "s" }
        ));
    }

    /// The stamps clicked on a face, on the part.
    pub(crate) fn punch_markers(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        let painter = ui.painter().with_clip_rect(rect);
        for at in self.punch_face_marks(rect) {
            painter.circle_filled(at, 4.5, t.accent);
            painter.line_segment([at - vec2(8.0, 0.0), at + vec2(8.0, 0.0)], Stroke::new(1.0, t.accent));
            painter.line_segment([at - vec2(0.0, 8.0), at + vec2(0.0, 8.0)], Stroke::new(1.0, t.accent));
        }
    }

    /// Makes the sketch a punch placed by clicks on a face goes on, then the punch, as one step.
    /// Returns whether it was made.
    pub(crate) fn commit_punch_on_face(&mut self, ptrs: &[(&'static str, &'static str)], p: &PunchPanel) -> bool {
        let Some((face, _)) = p.face.clone() else {
            self.set_error("Punch Tool: click a flat face of the sheet where the stamp goes, or choose a sketch of centre points.");
            return false;
        };
        if p.face_points.is_empty() {
            self.set_error("Punch Tool: click a flat face of the sheet where the stamp goes.");
            return false;
        }
        let mark = self.session.mark();
        let made = (|| -> Result<(), String> {
            let r = self.exec("sketch.create", json!({ "face": face, "project_origin": false }))?;
            let sketch = r["feature"].as_u64().and_then(|v| u32::try_from(v).ok()).map(FeatureId).ok_or("internal: no sketch made")?;
            let mut points = Vec::new();
            for v in &p.face_points {
                let r = self.exec("sketch.point", json!({ "sketch": sketch.0, "x": v.x, "y": v.y }))?;
                points.push(r["point"].as_u64().and_then(|v| u32::try_from(v).ok()).map(EntityId).ok_or("internal: no point made")?);
            }
            let kind = FeatureKind::Punch(sheet::Punch { sketch, points, shape: p.shape, rotation: p.degrees.to_radians(), flip: p.flip });
            if self.commit_panel(ptrs, None, kind) { Ok(()) } else { Err(String::new()) }
        })();
        match made {
            Ok(()) => {
                self.session.squash_since(mark);
                true
            }
            Err(e) => {
                self.session.roll_back_to(mark);
                if !e.is_empty() {
                    self.set_error(e);
                }
                false
            }
        }
    }

    /// The punch library: what shapes there are, by kind, each with what it is and a picture of
    /// it. What is picked goes into the open Punch panel.
    pub(crate) fn punch_library_window(&mut self, ui: &mut Ui, t: &Tokens) {
        let Some(Panel::Punch(panel)) = &self.panel else { return };
        if !panel.library {
            return;
        }
        let now = sheet::punch::family_of(&panel.shape).map_or("", |f| f.id);
        let mut picked: Option<&'static sheet::PunchFamily> = None;
        let mut open = true;
        let mut close = false;
        egui::Window::new("Punch Library").open(&mut open).collapsible(false).resizable(false).default_pos([300.0, 150.0]).show(ui.ctx(), |ui| {
            ui.horizontal_top(|ui| {
                // What there is, by kind.
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    for category in ["Cuts", "Formed"] {
                        ui.label(RichText::new(category).font(theme::small()).color(t.text_dim));
                        for f in sheet::PUNCH_LIBRARY.iter().filter(|f| f.category == category) {
                            let r = ui.selectable_label(f.id == now, f.name);
                            crate::drawing::remember(ui, &format!("tn_punch_library_{}", f.id), r.rect);
                            if r.clicked() {
                                picked = Some(f);
                            }
                        }
                        ui.add_space(4.0);
                    }
                });
                ui.add_space(10.0);
                // The shape picked: what it is, a picture of it, what it takes.
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    let Some(family) = sheet::PUNCH_LIBRARY.iter().find(|f| f.id == now) else { return };
                    ui.label(RichText::new(family.name).font(theme::heading()).strong());
                    ui.add(egui::Label::new(RichText::new(family.about).font(theme::small())).wrap());
                    ui.add_space(4.0);
                    shape_picture(ui, &panel.shape, t);
                    ui.add_space(4.0);
                    let takes: Vec<&str> = family.sizes.iter().map(|s| s.1).collect();
                    ui.label(RichText::new(format!("Sizes: {}. Set them in the panel.", takes.join(", "))).font(theme::small()).color(t.text_dim));
                    let shown = if family.category == "Cuts" {
                        "In the flat pattern: its outline, with its name."
                    } else {
                        "In the flat pattern: a centre mark and the line it bends at, with its name."
                    };
                    ui.label(RichText::new(shown).font(theme::small()).color(t.text_dim));
                });
            });
            ui.separator();
            close = crate::drawing::button(ui, "Close", "tn_punch_library_close").clicked();
        });
        if let Some(Panel::Punch(p)) = &mut self.panel {
            p.library = open && !close;
            if let Some(f) = picked {
                p.shape = f.starts;
                self.start_equations();
            }
        }
    }
}

/// A picture of a shape, to scale in a square.
fn shape_picture(ui: &mut Ui, shape: &PunchShape, t: &Tokens) {
    let (response, painter) = ui.allocate_painter(vec2(120.0, 90.0), egui::Sense::hover());
    let rect = response.rect;
    painter.rect_filled(rect, 3.0, t.panel.gamma_multiply(0.6));
    let region = shape.outline(|_| 0);
    let mut strokes: Vec<Vec<Vec2>> = Vec::new();
    for c in &region.outer.curves {
        match &c.curve {
            Curve2::Line { start, end } => strokes.push(vec![*start, *end]),
            Curve2::Arc { center, radius, start_angle, end_angle } => {
                strokes
                    .push((0..=24).map(|i| Vec2::polar(*center, *radius, start_angle + (end_angle - start_angle) * f64::from(i) / 24.0)).collect());
            }
            Curve2::Circle { center, radius } => {
                strokes.push((0..=48).map(|i| Vec2::polar(*center, *radius, std::f64::consts::TAU * f64::from(i) / 48.0)).collect());
            }
            _ => {}
        }
    }
    let reach = strokes.iter().flatten().fold(1e-6f64, |m, p| m.max(p.x.abs()).max(p.y.abs()));
    let scale = (rect.height().min(rect.width()) * 0.42) / reach as f32;
    let at = |p: Vec2| rect.center() + vec2(p.x as f32 * scale, -(p.y as f32) * scale);
    let pen = Stroke::new(1.5, t.text);
    for s in &strokes {
        painter.add(egui::Shape::line(s.iter().map(|p| at(*p)).collect(), pen));
    }
    // A formed shape's mark, and where it bends.
    if shape.is_formed() {
        let c = rect.center();
        let mark = Stroke::new(1.0, t.accent);
        painter.line_segment([c - vec2(8.0, 0.0), c + vec2(8.0, 0.0)], mark);
        painter.line_segment([c - vec2(0.0, 8.0), c + vec2(0.0, 8.0)], mark);
        if let PunchShape::Louver { length, width, .. } = shape {
            let (x, y) = (length / 2.0, -width / 2.0);
            painter.line_segment([at(Vec2::new(-x, y)), at(Vec2::new(x, y))], Stroke::new(2.0, t.accent));
        }
    }
}
