//! The Stress Analysis environment (Environments > Stress Analysis): a study of the part set
//! up on the part itself. Its faces are clicked to say where it is held and what pushes on it;
//! the browser shows the study in place of the features; supports and loads are drawn on the
//! part. Everything it does is a `study.*` command (DEC-093).
//!
//! Working a study out (dividing the part into elements, solving, showing stress) is not here
//! yet: it waits on the mesher and solver (DEC-094), and the buttons for it say so.

use egui::{Align2, Color32, Pos2, Rect, Stroke, Ui, vec2};
use serde_json::{Value, json};
use tenon_geom::Vec3;
use tenon_kernel::SurfaceKind;
use tenon_model::FaceRef;
use tenon_model::study::{LoadKind, Study, StudyId};

use crate::browser::{RowStyle, row};
use crate::commands::ANALYSIS_TAB;
use crate::icons::Icon;
use crate::panels::Panel;
use crate::properties::{eq_field, flip_button, picked_row, section};
use crate::theme::{self, Tokens};
use crate::viewport::Pick;
use crate::workbench::Workbench;

/// The colour of faces that are held, and of faces that are loaded.
pub(crate) const HELD: Color32 = Color32::from_rgb(0x4a, 0x86, 0xd0);
pub(crate) const LOADED: Color32 = Color32::from_rgb(0xe0, 0xb0, 0x30);

/// The environment while it is entered.
#[derive(Clone, Debug, Default)]
pub(crate) struct Env {
    /// The study being worked on (the part's first, when this one is gone).
    pub study: Option<StudyId>,
    /// The support or load clicked in the browser: its faces are picked out.
    pub selected: Option<u32>,
}

/// Fixed: the faces that are held.
#[derive(Clone, Debug)]
pub(crate) struct SupportPanel {
    pub study: StudyId,
    /// The support being changed; None for a new one.
    pub editing: Option<u32>,
    pub faces: Vec<FaceRef>,
}

/// The way a force pushes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Way {
    /// Square to each face, into it.
    Normal,
    X,
    Y,
    Z,
    /// Some other direction, as a command gave it: kept as it is.
    Other(Vec3),
}

impl Way {
    fn label(self) -> &'static str {
        match self {
            Way::Normal => "Normal to Face",
            Way::X => "Along X",
            Way::Y => "Along Y",
            Way::Z => "Along Z",
            Way::Other(_) => "As Set",
        }
    }

    fn of(direction: Option<Vec3>) -> (Way, bool) {
        let Some(d) = direction else { return (Way::Normal, false) };
        let axis = |v: Vec3| (d - v).len() < 1e-9;
        for (way, v) in [(Way::X, Vec3::X), (Way::Y, Vec3::Y), (Way::Z, Vec3::Z)] {
            if axis(v) {
                return (way, false);
            }
            if axis(-v) {
                return (way, true);
            }
        }
        (Way::Other(d), false)
    }

    /// The direction, as the command takes it (`flip`: the other way along it).
    fn said(self, flip: bool) -> Value {
        let sign = if flip { -1.0 } else { 1.0 };
        match self {
            Way::Normal => json!("normal"),
            Way::X => json!([sign, 0.0, 0.0]),
            Way::Y => json!([0.0, sign, 0.0]),
            Way::Z => json!([0.0, 0.0, sign]),
            Way::Other(d) => json!([d.x * sign, d.y * sign, d.z * sign]),
        }
    }

    fn vector(self, flip: bool) -> Option<Vec3> {
        let v = match self {
            Way::Normal => return None,
            Way::X => Vec3::X,
            Way::Y => Vec3::Y,
            Way::Z => Vec3::Z,
            Way::Other(d) => d,
        };
        Some(if flip { -v } else { v })
    }
}

/// Force or Pressure: the faces it is on and how large it is.
#[derive(Clone, Debug)]
pub(crate) struct LoadPanel {
    pub study: StudyId,
    pub editing: Option<u32>,
    pub pressure: bool,
    pub faces: Vec<FaceRef>,
    /// Newtons, or megapascals for a pressure.
    pub magnitude: f64,
    pub way: Way,
    /// Along its direction the other way; for one square to its faces, pulling.
    pub flip: bool,
}

impl LoadPanel {
    pub(crate) fn title(&self) -> &'static str {
        if self.pressure { "Pressure" } else { "Force" }
    }

    /// The load as it would be drawn: its size with its sign, and where it points.
    fn shown(&self) -> LoadKind {
        if self.pressure {
            LoadKind::Pressure { megapascals: self.magnitude }
        } else if self.way == Way::Normal {
            LoadKind::Force { newtons: if self.flip { -self.magnitude } else { self.magnitude }, direction: None }
        } else {
            LoadKind::Force { newtons: self.magnitude, direction: self.way.vector(self.flip) }
        }
    }

    /// The command that makes or changes it.
    pub(crate) fn command(&self) -> (&'static str, Value) {
        let mut p = json!({ "study": self.study.0, "faces": self.faces });
        match self.shown() {
            LoadKind::Pressure { megapascals } => p["megapascals"] = json!(megapascals),
            LoadKind::Force { newtons, .. } => {
                p["newtons"] = json!(newtons);
                p["direction"] = self.way.said(self.flip);
            }
        }
        match self.editing {
            Some(item) => {
                p["item"] = json!(item);
                ("study.set", p)
            }
            None => (if self.pressure { "study.pressure" } else { "study.force" }, p),
        }
    }
}

/// The fields of a support being set up.
pub(crate) fn support_fields(ui: &mut Ui, p: &mut SupportPanel) -> bool {
    let mut clear = false;
    section(ui, "Location", true, |ui| {
        egui::Grid::new("tn_props_support").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Faces").on_hover_text("The faces that are held still: click them in the part");
            clear = picked_row(ui, p.faces.len(), "click faces");
            ui.end_row();
        });
    });
    clear
}

/// The fields of a load being set up. Returns (clear the faces, Enter was pressed).
pub(crate) fn load_fields(ui: &mut Ui, p: &mut LoadPanel, t: &Tokens) -> (bool, bool) {
    let (mut clear, mut enter) = (false, false);
    section(ui, "Location", true, |ui| {
        egui::Grid::new("tn_props_load_faces").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Faces").on_hover_text("The faces it is on: click them in the part");
            clear = picked_row(ui, p.faces.len(), "click faces");
            ui.end_row();
        });
    });
    section(ui, "Magnitude", true, |ui| {
        egui::Grid::new("tn_props_load_size").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            if p.pressure {
                ui.label("Pressure").on_hover_text("On the faces, square to them and into them. Negative pulls.");
                enter |= eq_field(ui, egui::Id::new("tn_props_load_mag"), &mut p.magnitude, None, "MPa", -1.0e6..=1.0e6, 110.0, t).entered;
                ui.end_row();
            } else {
                ui.label("Force").on_hover_text("The whole force, spread evenly over the faces");
                enter |= eq_field(ui, egui::Id::new("tn_props_load_mag"), &mut p.magnitude, None, "N", 0.001..=1.0e12, 110.0, t).entered;
                ui.end_row();
                ui.label("Direction");
                egui::ComboBox::from_id_salt("tn_props_load_way").selected_text(p.way.label()).show_ui(ui, |ui| {
                    for way in [Way::Normal, Way::X, Way::Y, Way::Z] {
                        ui.selectable_value(&mut p.way, way, way.label());
                    }
                });
                ui.end_row();
                ui.label("");
                flip_button(ui, &mut p.flip, t);
                ui.end_row();
            }
        });
    });
    (clear, enter)
}

fn load_text(kind: &LoadKind) -> String {
    let n = |v: f64| if v.fract() == 0.0 && v.abs() < 1e9 { format!("{v:.0}") } else { format!("{v}") };
    match kind {
        LoadKind::Force { newtons, .. } => format!("{} N", n(*newtons)),
        LoadKind::Pressure { megapascals } => format!("{} MPa", n(*megapascals)),
    }
}

/// What the browser asks for.
enum StudyAction {
    Activate(StudyId),
    Select(u32),
    Edit(u32),
    Remove(u32),
    Delete(StudyId),
    Run(&'static str),
}

impl Workbench {
    pub(crate) fn in_analysis(&self) -> bool {
        self.analysis.is_some() && !self.in_assembly() && !self.in_drawing()
    }

    /// The study the environment is working on.
    pub(crate) fn active_study(&self) -> Option<&Study> {
        let env = self.analysis.as_ref()?;
        let studies = self.document().studies();
        env.study.and_then(|id| studies.iter().find(|s| s.id == id)).or(studies.first())
    }

    /// The environment's own commands. While it is entered, the commands that change the part
    /// are refused: the part is changed in the part environment.
    pub(crate) fn analysis_command(&mut self, id: &str) -> Option<Result<(), String>> {
        if !id.starts_with("analysis.") && !id.starts_with("study.") {
            let changes = ["sketch.", "work.", "sheet."].iter().any(|p| id.starts_with(p)) || (id.starts_with("model.") && id != "model.rebuild");
            return (self.in_analysis() && changes).then(|| Err("That changes the part: Finish Analysis first.".into()));
        }
        if self.in_assembly() || self.in_drawing() {
            return Some(Err("Stress Analysis is of a part: open the part, or edit it in place.".into()));
        }
        Some(match id {
            "analysis.begin" => self.analysis_begin(),
            "analysis.finish" => {
                self.analysis_finish();
                Ok(())
            }
            "analysis.material" => {
                self.chrome.mass = true;
                Ok(())
            }
            "study.new" => self.study_create(),
            "study.fix" => self.open_support(None),
            "study.force" => self.open_load(false, None),
            "study.pressure" => self.open_load(true, None),
            "study.mesh_size" => self.open_mesh_size(),
            _ => return None,
        })
    }

    /// Environments > Stress Analysis.
    pub(crate) fn analysis_begin(&mut self) -> Result<(), String> {
        // (A part that was just opened has its solid on the way: that is not "none".)
        let built = self.shown.is_some() && !self.is_busy();
        if self.document().features().is_empty() || (built && self.scene.bodies.is_empty()) {
            return Err("Stress Analysis: there is no solid to study yet.".into());
        }
        if self.is_sketching() {
            self.finish_sketch();
        }
        self.chrome.sheet.flat_view = false;
        self.view.selection.clear();
        self.panel = None;
        let study = self.document().studies().first().map(|s| s.id);
        self.analysis = Some(Env { study, selected: None });
        self.chrome.tab = ANALYSIS_TAB;
        self.set_status(match study {
            Some(_) => "Stress Analysis: say where the part is held (Fixed) and what pushes on it (Force, Pressure).",
            None => "Stress Analysis: Create Study, then say where the part is held and what pushes on it.",
        });
        Ok(())
    }

    /// Finish Analysis: back to the part.
    pub(crate) fn analysis_finish(&mut self) {
        if matches!(self.panel, Some(Panel::Support(_) | Panel::Load(_))) {
            self.panel = None;
        }
        self.analysis = None;
        self.chrome.tab = self.home_tab();
        self.set_status("Ready");
    }

    /// Create Study: a new study, which becomes the one worked on.
    pub(crate) fn study_create(&mut self) -> Result<(), String> {
        if !self.in_analysis() {
            self.analysis_begin()?;
        }
        let made = self.exec("study.new", json!({}))?;
        let id = made["study"].as_u64().and_then(|v| u32::try_from(v).ok()).map(StudyId);
        if let Some(env) = &mut self.analysis {
            env.study = id;
            env.selected = None;
        }
        self.set_status(format!("{} created: say where the part is held (Fixed) and what pushes on it.", made["name"].as_str().unwrap_or("Study")));
        Ok(())
    }

    /// The study a new support or load goes into: the one worked on, made if there is none.
    fn study_for_new(&mut self) -> Result<StudyId, String> {
        if !self.in_analysis() {
            self.analysis_begin()?;
        }
        if self.active_study().is_none() {
            self.study_create()?;
        }
        self.active_study().map(|s| s.id).ok_or_else(|| "there is no study".to_string())
    }

    /// Fixed: a new support, or (with `editing`) one of the study's changed.
    pub(crate) fn open_support(&mut self, editing: Option<u32>) -> Result<(), String> {
        let study = self.study_for_new()?;
        let faces = match editing {
            Some(id) => self.active_study().and_then(|s| s.supports.iter().find(|x| x.id == id)).map(|x| x.faces.clone()).ok_or("no such support")?,
            None => self.picked_faces(),
        };
        self.view.selection.clear();
        self.panel = Some(Panel::Support(SupportPanel { study, editing, faces }));
        self.start_equations();
        self.set_status("Fixed: click the faces that are held still, then OK.");
        Ok(())
    }

    /// Force or Pressure: a new load, or (with `editing`) one of the study's changed.
    pub(crate) fn open_load(&mut self, pressure: bool, editing: Option<u32>) -> Result<(), String> {
        let study = self.study_for_new()?;
        let panel = match editing {
            Some(id) => {
                let load = self.active_study().and_then(|s| s.loads.iter().find(|l| l.id == id)).ok_or("no such load")?;
                match load.kind {
                    LoadKind::Pressure { megapascals } => {
                        LoadPanel { study, editing, pressure: true, faces: load.faces.clone(), magnitude: megapascals, way: Way::Normal, flip: false }
                    }
                    LoadKind::Force { newtons, direction } => {
                        let (way, flip) = Way::of(direction);
                        // (Square to its faces, a negative force pulls: shown as flipped.)
                        let pulls = direction.is_none() && newtons < 0.0;
                        LoadPanel { study, editing, pressure: false, faces: load.faces.clone(), magnitude: newtons.abs(), way, flip: flip || pulls }
                    }
                }
            }
            None => LoadPanel {
                study,
                editing,
                pressure,
                faces: self.picked_faces(),
                magnitude: if pressure { 1.0 } else { 100.0 },
                way: Way::Normal,
                flip: false,
            },
        };
        self.view.selection.clear();
        self.set_status(if panel.pressure {
            "Pressure: click the faces it is on, set how large it is, then OK."
        } else {
            "Force: click the faces it is on, set how large it is and which way it pushes, then OK."
        });
        self.panel = Some(Panel::Load(panel));
        self.start_equations();
        Ok(())
    }

    /// OK on a support. Returns true when it was made or changed.
    pub(crate) fn commit_support(&mut self, p: &SupportPanel) -> bool {
        if p.faces.is_empty() {
            self.set_error("Fixed: click at least one face".to_string());
            return false;
        }
        let done = match p.editing {
            Some(item) => self.exec_status("study.set", json!({ "study": p.study.0, "item": item, "faces": p.faces })),
            None => self.exec_status("study.fix", json!({ "study": p.study.0, "faces": p.faces })),
        };
        done.is_some()
    }

    /// OK on a load. Returns true when it was made or changed.
    pub(crate) fn commit_load(&mut self, p: &LoadPanel) -> bool {
        if p.faces.is_empty() {
            self.set_error(format!("{}: click at least one face", p.title()));
            return false;
        }
        let (id, params) = p.command();
        self.exec_status(id, params).is_some()
    }

    /// Mesh Settings: how large the study's elements are.
    fn open_mesh_size(&mut self) -> Result<(), String> {
        let study = self.study_for_new()?;
        let size = self.active_study().and_then(|s| s.mesh_size).unwrap_or(0.0);
        let what = crate::panels::ValueFor::MeshSize { study };
        self.panel = Some(Panel::Value(crate::panels::ValuePanel::new("Mesh Settings", "Element size (mm; 0: sized from the part)", size, what)));
        Ok(())
    }

    /// The faces of the support or load clicked in the browser, picked out as selected. (The
    /// others are shown by their marks alone: a part with its faces all coloured in hides
    /// the marks, and the part.)
    pub(crate) fn analysis_highlights(&self) -> Vec<(Pick, Color32)> {
        let (Some(study), Some(selected)) = (self.active_study(), self.analysis.as_ref().and_then(|e| e.selected)) else { return Vec::new() };
        let face = |r: &FaceRef| crate::modify::find_face(&self.scene, r).map(|(body, face)| (Pick::Face { body, face }, crate::viewport::SELECTED));
        let faces = study.supports.iter().map(|s| (s.id, &s.faces)).chain(study.loads.iter().map(|l| (l.id, &l.faces)));
        faces.filter(|(id, _)| *id == selected).flat_map(|(_, faces)| faces.iter().filter_map(face)).collect()
    }

    /// Where a face is and which way it looks (None for a face that is not flat).
    fn face_at(&self, r: &FaceRef) -> Option<(Vec3, Option<Vec3>)> {
        let (body, face) = crate::modify::find_face(&self.scene, r)?;
        let info = &self.scene.bodies.get(body)?.faces.get(face as usize)?.1;
        let normal = match &info.surface {
            SurfaceKind::Plane { normal, .. } => Some(*normal),
            _ => None,
        };
        Some((info.centroid, normal))
    }

    /// Supports and loads drawn on the part: a mark on each face that is held, an arrow on
    /// each face that is loaded, with how large the load is.
    pub(crate) fn analysis_overlay(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        if !self.in_analysis() {
            #[cfg(test)]
            ui.data_mut(|d| d.remove::<Vec<(&'static str, Pos2)>>(egui::Id::new("tn_analysis_marks")));
            return;
        }
        let painter = ui.painter().with_clip_rect(rect);
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let cam = &self.view.camera;
        let screen = |p: Vec3| cam.project(p, w, h).map(|(x, y, _)| rect.min + vec2(x as f32, y as f32));
        // (A step in the part that is some pixels long however far away the part is.)
        let reach = self.scene.bbox().map_or(1.0, |b| (b.max - b.min).len() * 0.05).max(1e-6);
        // A point `pixels` from `at` along `dir`, as the screen has it.
        let off = |at: Vec3, dir: Vec3, pixels: f32| -> Option<Pos2> {
            let (a, b) = (screen(at)?, screen(at + dir * reach)?);
            let d = b - a;
            (d.length() > 0.5).then(|| a + d / d.length() * pixels)
        };
        let edge = Color32::from_black_alpha(150);
        #[cfg(test)]
        let marks = std::cell::RefCell::new(Vec::<(&'static str, Pos2)>::new());
        let mark = |_what: &'static str, _at: Pos2| {
            #[cfg(test)]
            marks.borrow_mut().push((_what, _at));
        };
        let ring = |c: Pos2, color: Color32| {
            painter.circle_stroke(c, 7.0, Stroke::new(4.5, edge));
            painter.circle_stroke(c, 7.0, Stroke::new(2.5, color));
        };
        // An arrow with its head at the face and its tail back the way the load comes from;
        // `spread`: three of them side by side, for a load that is all over the face.
        let arrow = |tip: Vec3, dir: Vec3, color: Color32, label: Option<&str>, spread: bool| {
            let Some(head) = screen(tip) else { return };
            mark("load", head);
            let Some(tail) = off(tip, -dir, 54.0) else {
                // Seen end on: a ring.
                ring(head, color);
                return;
            };
            let along = (head - tail) / (head - tail).length();
            let across = vec2(-along.y, along.x);
            let shafts: &[f32] = if spread { &[-11.0, 0.0, 11.0] } else { &[0.0] };
            let length = if spread { 0.6 } else { 1.0 };
            for (width, color) in [(5.0, edge), (2.5, color)] {
                for s in shafts {
                    let (head, tail) = (head + across * *s, head + across * *s + (tail - head) * length);
                    painter.line_segment([tail, head - along * 8.0], Stroke::new(width, color));
                    let grow = (width - 2.5) * 0.6;
                    let barbs = vec![
                        head + along * grow,
                        head - along * (12.0 + grow) + across * (5.5 + grow),
                        head - along * (12.0 + grow) - across * (5.5 + grow),
                    ];
                    painter.add(egui::Shape::convex_polygon(barbs, color, Stroke::NONE));
                }
                if spread {
                    let back = head + (tail - head) * length;
                    painter.line_segment([back + across * shafts[0], back + across * shafts[2]], Stroke::new(width, color));
                }
            }
            if let Some(text) = label {
                let at = head + (tail - head) * length - along * 12.0;
                painter.text(at + vec2(1.0, 1.0), Align2::CENTER_CENTER, text, theme::body(), edge);
                painter.text(at, Align2::CENTER_CENTER, text, theme::body(), t.viewport_text);
            }
        };
        let held = |at: Vec3, color: Color32| {
            let Some(c) = screen(at) else { return };
            mark("held", c);
            // The ground mark: a bar with strokes under it.
            for (width, color) in [(5.0, edge), (2.5, color)] {
                painter.line_segment([c + vec2(-12.0, 0.0), c + vec2(12.0, 0.0)], Stroke::new(width, color));
                for i in 0..5 {
                    let x = -10.0 + 5.0 * i as f32;
                    painter.line_segment([c + vec2(x + 4.0, 1.0), c + vec2(x, 9.0)], Stroke::new(width - 1.0, color));
                }
            }
        };
        let load = |faces: &[FaceRef], kind: &LoadKind, color: Color32| {
            let text = load_text(kind);
            for (i, r) in faces.iter().enumerate() {
                let Some((at, normal)) = self.face_at(r) else { continue };
                let label = (i == 0).then_some(text.as_str());
                let dir = match kind {
                    LoadKind::Force { direction: Some(d), newtons } => Some(if *newtons < 0.0 { -*d } else { *d }),
                    LoadKind::Force { direction: None, newtons } => normal.map(|n| if *newtons < 0.0 { n } else { -n }),
                    LoadKind::Pressure { megapascals } => normal.map(|n| if *megapascals < 0.0 { n } else { -n }),
                };
                match dir {
                    Some(d) => arrow(at, d, color, label, matches!(kind, LoadKind::Pressure { .. })),
                    // A round face is pushed all over, each way its own: marked where it is.
                    None => {
                        if let Some(c) = screen(at) {
                            mark("load", c);
                            ring(c, color);
                            if let Some(text) = label {
                                painter.text(c + vec2(0.0, -16.0), Align2::CENTER_CENTER, text, theme::body(), t.viewport_text);
                            }
                        }
                    }
                }
            }
        };
        let editing = match &self.panel {
            Some(Panel::Support(p)) => p.editing,
            Some(Panel::Load(p)) => p.editing,
            _ => None,
        };
        if let Some(study) = self.active_study() {
            for s in study.supports.iter().filter(|s| Some(s.id) != editing) {
                for r in &s.faces {
                    if let Some((at, _)) = self.face_at(r) {
                        held(at, HELD);
                    }
                }
            }
            for l in study.loads.iter().filter(|l| Some(l.id) != editing) {
                load(&l.faces, &l.kind, LOADED);
            }
        }
        // What is being set up, as it will be.
        match &self.panel {
            Some(Panel::Support(p)) => {
                for r in &p.faces {
                    if let Some((at, _)) = self.face_at(r) {
                        held(at, t.accent);
                    }
                }
            }
            Some(Panel::Load(p)) => load(&p.faces, &p.shown(), t.accent),
            _ => {}
        }
        // (Tests look for a mark where a face is.)
        #[cfg(test)]
        ui.data_mut(|d| d.insert_temp(egui::Id::new("tn_analysis_marks"), marks.into_inner()));
    }

    /// The browser in the environment: the part's studies, and under the one worked on its
    /// material, its supports, its loads, its mesh and its results.
    pub(crate) fn study_browser(&mut self, ui: &mut Ui, t: &Tokens) {
        #[cfg(test)]
        ui.data_mut(|d| d.remove::<Vec<(String, Rect)>>(egui::Id::new("tn_browser_rows")));
        let r = ui.max_rect();
        let header = Rect::from_min_size(r.min, vec2(r.width(), 26.0));
        let p = ui.painter();
        p.rect_filled(header, 0.0, t.panel_header);
        let tab = Rect::from_min_size(header.min, vec2(70.0, header.height()));
        p.rect_filled(tab, 0.0, t.panel);
        p.text(egui::pos2(tab.left() + 8.0, tab.center().y), Align2::LEFT_CENTER, "Study", theme::body(), t.text);
        p.hline(header.x_range(), header.bottom(), Stroke::new(1.0, t.border));
        ui.add_space(header.height() + 2.0);

        let active = self.active_study().map(|s| s.id);
        let selected = self.analysis.as_ref().and_then(|e| e.selected);
        let studies: Vec<Study> = self.document().studies().to_vec();
        let material = self.document().material().map(|m| m.name.clone());
        let strong = self.document().strength().is_some();
        let name = self.document().name.clone();
        let mut action: Option<StudyAction> = None;
        egui::ScrollArea::vertical().id_salt("tn_study_scroll").auto_shrink([false, false]).show(ui, |ui| {
            row(ui, t, 0, Icon::Part, &name, None, RowStyle::Normal);
            if studies.is_empty() {
                let (resp, _) = row(ui, t, 1, Icon::StudyNew, "No study yet: Create Study", None, RowStyle::Dim);
                if resp.clicked() || resp.double_clicked() {
                    action = Some(StudyAction::Run("study.new"));
                }
            }
            for study in &studies {
                let on = Some(study.id) == active;
                let (resp, _) = row(ui, t, 1, Icon::StudyNew, &study.name, None, if on { RowStyle::Active } else { RowStyle::Normal });
                let resp = resp.on_hover_text("Double-click to work on this study; right-click to delete it");
                if resp.double_clicked() {
                    action = Some(StudyAction::Activate(study.id));
                }
                resp.context_menu(|ui| {
                    if ui.button("Delete Study").clicked() {
                        action = Some(StudyAction::Delete(study.id));
                        ui.close();
                    }
                });
                if !on {
                    continue;
                }
                // Its material: the part's.
                let (label, style) = match (&material, strong) {
                    (Some(m), true) => (format!("Material: {m}"), RowStyle::Normal),
                    (Some(m), false) => (format!("Material: {m} (no strength known)"), RowStyle::Error),
                    (None, _) => ("Material: none (Assign)".to_owned(), RowStyle::Error),
                };
                let (resp, _) = row(ui, t, 2, Icon::Body, &label, None, style);
                if resp.on_hover_text("Double-click to choose the part's material").double_clicked() {
                    action = Some(StudyAction::Run("analysis.material"));
                }
                // (What the study still lacks is written in red, and says why when pointed at.)
                let needed = |n: usize| if n == 0 { RowStyle::Error } else { RowStyle::Normal };
                let (resp, _) = row(ui, t, 2, Icon::Folder, &format!("Constraints({})", study.supports.len()), None, needed(study.supports.len()));
                if study.supports.is_empty() {
                    resp.on_hover_text("The study needs a support (Fixed): a part that is held nowhere moves off, however small the load");
                }
                for s in &study.supports {
                    let style = if Some(s.id) == selected { RowStyle::Active } else { RowStyle::Normal };
                    let (resp, _) = row(ui, t, 3, Icon::Fixed, &s.name, None, style);
                    item_row(resp, s.id, &mut action);
                }
                let (resp, _) = row(ui, t, 2, Icon::Folder, &format!("Loads({})", study.loads.len()), None, needed(study.loads.len()));
                if study.loads.is_empty() {
                    resp.on_hover_text("The study needs a load (Force or Pressure): with nothing pushing on it there is no stress to find");
                }
                for l in &study.loads {
                    let style = if Some(l.id) == selected { RowStyle::Active } else { RowStyle::Normal };
                    let icon = if matches!(l.kind, LoadKind::Pressure { .. }) { Icon::Pressure } else { Icon::Force };
                    let (resp, _) = row(ui, t, 3, icon, &format!("{} ({})", l.name, load_text(&l.kind)), None, style);
                    item_row(resp, l.id, &mut action);
                }
                let mesh = match study.mesh_size {
                    Some(size) => format!("Mesh: {size} mm"),
                    None => "Mesh: sized from the part".to_owned(),
                };
                let (resp, _) = row(ui, t, 2, Icon::MeshGrid, &mesh, None, RowStyle::Normal);
                if resp.on_hover_text("Double-click to set the size of the elements").double_clicked() {
                    action = Some(StudyAction::Run("study.mesh_size"));
                }
                let (resp, _) = row(ui, t, 2, Icon::Simulate, "Results: none yet", None, RowStyle::Dim);
                resp.on_hover_text("Working a study out (Simulate) arrives with milestone M11's mesher and solver");
            }
        });
        let done = match action {
            None => Ok(()),
            Some(StudyAction::Activate(id)) => {
                if let Some(env) = &mut self.analysis {
                    env.study = Some(id);
                    env.selected = None;
                }
                Ok(())
            }
            Some(StudyAction::Select(id)) => {
                if let Some(env) = &mut self.analysis {
                    env.selected = if env.selected == Some(id) { None } else { Some(id) };
                }
                Ok(())
            }
            Some(StudyAction::Edit(id)) => self.edit_study_item(id),
            Some(StudyAction::Remove(id)) => {
                let study = self.active_study().map(|s| s.id.0);
                self.exec("study.remove", json!({ "study": study, "item": id })).map(|_| ())
            }
            Some(StudyAction::Delete(id)) => self.exec("study.delete", json!({ "study": id.0 })).map(|_| ()),
            Some(StudyAction::Run(id)) => self.run_ui(id),
        };
        if let Err(e) = done {
            self.set_error(e);
        }
    }

    /// The name of the study's support or load `id`.
    pub(crate) fn study_item_name(&self, id: u32) -> Option<String> {
        let study = self.active_study()?;
        let support = study.supports.iter().find(|s| s.id == id).map(|s| s.name.clone());
        support.or_else(|| study.loads.iter().find(|l| l.id == id).map(|l| l.name.clone()))
    }

    /// Opens the panel of the study's support or load `id`.
    fn edit_study_item(&mut self, id: u32) -> Result<(), String> {
        let Some(study) = self.active_study() else { return Err("there is no study".into()) };
        if study.supports.iter().any(|s| s.id == id) {
            return self.open_support(Some(id));
        }
        match study.loads.iter().find(|l| l.id == id).map(|l| matches!(l.kind, LoadKind::Pressure { .. })) {
            Some(pressure) => self.open_load(pressure, Some(id)),
            None => Err("no such support or load".into()),
        }
    }
}

/// A support's or a load's row: a click picks its faces out, a double click opens it, the menu
/// edits or deletes it.
fn item_row(resp: egui::Response, id: u32, action: &mut Option<StudyAction>) {
    let resp = resp.on_hover_text("Click to see its faces; double-click to change it; right-click to delete it");
    if resp.double_clicked() {
        *action = Some(StudyAction::Edit(id));
    } else if resp.clicked() {
        *action = Some(StudyAction::Select(id));
    }
    resp.context_menu(|ui| {
        if ui.button("Edit").clicked() {
            *action = Some(StudyAction::Edit(id));
            ui.close();
        }
        if ui.button("Delete").clicked() {
            *action = Some(StudyAction::Remove(id));
            ui.close();
        }
    });
}
