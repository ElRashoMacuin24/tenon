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
use tenon_model::study::{EARTH, LoadKind, Study, StudyId, SupportKind};
use tenon_model::{FaceRef, WorkGeom};

use crate::browser::{RowStyle, row};
use crate::commands::ANALYSIS_TAB;
use crate::icons::Icon;
use crate::panels::Panel;
use crate::properties::{eq_field, flip_button, fmt_value, picked_row, section, slot_button};
use crate::theme::{self, Tokens};
use crate::viewport::Pick;
use crate::workbench::Workbench;

/// The colour of the marks on faces that are held, and on what is loaded.
pub(crate) const HELD: Color32 = Color32::from_rgb(0x4a, 0x86, 0xd0);
pub(crate) const LOADED: Color32 = Color32::from_rgb(0xe0, 0xb0, 0x30);

/// The environment while it is entered.
#[derive(Clone, Debug, Default)]
pub(crate) struct Env {
    /// The study being worked on (the part's first, when this one is gone).
    pub study: Option<StudyId>,
    /// The support or load clicked in the browser: its faces are picked out.
    pub selected: Option<u32>,
    /// Boundary Conditions is off: supports and loads are not drawn on the part.
    pub hide_marks: bool,
}

/// Fixed, Pin or Frictionless: the faces that are held.
#[derive(Clone, Debug)]
pub(crate) struct SupportPanel {
    pub study: StudyId,
    /// The support being changed; None for a new one.
    pub editing: Option<u32>,
    pub kind: SupportKind,
    pub faces: Vec<FaceRef>,
}

impl SupportPanel {
    pub(crate) fn title(&self) -> &'static str {
        self.kind.word()
    }

    /// What its faces are, for the status bar and the faces row.
    fn faces_are(&self) -> &'static str {
        match self.kind {
            SupportKind::Fixed => "the faces that are held still",
            SupportKind::Pin { .. } => "the round faces that are on the pin",
            SupportKind::Frictionless => "the flat or round faces that may slide",
        }
    }

    /// The command that makes or changes it.
    pub(crate) fn command(&self) -> (&'static str, Value) {
        let mut p = json!({ "study": self.study.0, "faces": self.faces });
        if let SupportKind::Pin { radial, axial, tangential } = self.kind {
            p["radial"] = json!(radial);
            p["axial"] = json!(axial);
            p["tangential"] = json!(tangential);
        }
        match (self.editing, self.kind) {
            (Some(item), _) => {
                p["item"] = json!(item);
                ("study.set", p)
            }
            (None, SupportKind::Fixed) => ("study.fix", p),
            (None, SupportKind::Pin { .. }) => ("study.pin", p),
            (None, SupportKind::Frictionless) => ("study.frictionless", p),
        }
    }
}

/// The way a force pushes, or the part's weight pulls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Way {
    /// Square to each face, into it.
    Normal,
    X,
    Y,
    Z,
    /// Some other direction: along an edge, a face or a work axis that was clicked, or as a
    /// command gave it.
    Other(Vec3),
}

impl Way {
    fn label(self) -> String {
        match self {
            Way::Normal => "Normal to Face".into(),
            Way::X => "X Axis".into(),
            Way::Y => "Y Axis".into(),
            Way::Z => "Z Axis".into(),
            Way::Other(d) => format!("{}, {}, {}", fmt_value(d.x), fmt_value(d.y), fmt_value(d.z)),
        }
    }

    fn of(direction: Option<Vec3>) -> (Way, bool) {
        let Some(d) = direction else { return (Way::Normal, false) };
        let len = d.len();
        if len <= 0.0 {
            return (Way::Normal, false);
        }
        let d = d * (1.0 / len);
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

    fn vector(self, flip: bool) -> Option<Vec3> {
        let v = match self {
            Way::Normal => return None,
            Way::X => Vec3::X,
            Way::Y => Vec3::Y,
            Way::Z => Vec3::Z,
            Way::Other(d) => d,
        };
        // (Adding 0 makes a -0 a 0.)
        Some(if flip { Vec3::new(-v.x + 0.0, -v.y + 0.0, -v.z + 0.0) } else { v })
    }
}

/// Which load a panel sets up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum What {
    Force,
    Pressure,
    Gravity,
}

/// Force, Pressure or Gravity: where it is, how large, which way.
#[derive(Clone, Debug)]
pub(crate) struct LoadPanel {
    pub study: StudyId,
    pub editing: Option<u32>,
    pub what: What,
    pub faces: Vec<FaceRef>,
    /// Newtons; megapascals for a pressure; mm/s^2 for gravity.
    pub magnitude: f64,
    pub way: Way,
    /// Along its direction the other way; for one square to its faces, pulling.
    pub flip: bool,
    /// The direction selector is the one that takes clicks: the next click on a straight
    /// edge, a flat face or a work axis gives the direction.
    pub picking: bool,
    /// Use Vector Components: the force as its parts along X, Y and Z (N).
    pub components: Option<Vec3>,
}

impl LoadPanel {
    pub(crate) fn title(&self) -> &'static str {
        match self.what {
            What::Force => "Force",
            What::Pressure => "Pressure",
            What::Gravity => "Gravity",
        }
    }

    /// The load as it stands: its size with its sign, and where it points.
    pub(crate) fn shown(&self) -> LoadKind {
        match (self.what, self.components) {
            (What::Pressure, _) => LoadKind::Pressure { megapascals: self.magnitude },
            (What::Gravity, _) => {
                LoadKind::Gravity { direction: self.way.vector(self.flip).unwrap_or(Vec3::new(0.0, 0.0, -1.0)), acceleration: self.magnitude }
            }
            (What::Force, Some(v)) => {
                let newtons = v.len();
                LoadKind::Force { newtons, direction: (newtons > 0.0).then(|| v * (1.0 / newtons)) }
            }
            (What::Force, None) if self.way == Way::Normal => {
                LoadKind::Force { newtons: if self.flip { -self.magnitude } else { self.magnitude }, direction: None }
            }
            (What::Force, None) => LoadKind::Force { newtons: self.magnitude, direction: self.way.vector(self.flip) },
        }
    }

    /// The command that makes or changes it.
    pub(crate) fn command(&self) -> (&'static str, Value) {
        let mut p = json!({ "study": self.study.0 });
        let said = |d: Option<Vec3>| d.map_or(json!("normal"), |d| json!([d.x, d.y, d.z]));
        let made = match self.shown() {
            LoadKind::Pressure { megapascals } => {
                p["faces"] = json!(self.faces);
                p["megapascals"] = json!(megapascals);
                "study.pressure"
            }
            LoadKind::Force { newtons, direction } => {
                p["faces"] = json!(self.faces);
                p["newtons"] = json!(newtons);
                p["direction"] = said(direction);
                "study.force"
            }
            LoadKind::Gravity { direction, acceleration } => {
                p["direction"] = said(Some(direction));
                p["acceleration"] = json!(acceleration);
                "study.gravity"
            }
        };
        match self.editing {
            Some(item) => {
                p["item"] = json!(item);
                ("study.set", p)
            }
            None => (made, p),
        }
    }

    /// Use Vector Components, turned on or off: the same force said the other way.
    fn set_components(&mut self, on: bool, face_normal: Option<Vec3>) {
        match (on, self.components) {
            (true, None) => {
                // (Square to its faces it has no one direction: that of its first face, if
                // that is flat; else straight down.)
                let inward = face_normal.map_or(Vec3::new(0.0, 0.0, -1.0), |n| Vec3::new(-n.x + 0.0, -n.y + 0.0, -n.z + 0.0));
                let d = self.way.vector(self.flip).unwrap_or(if self.flip { -inward } else { inward });
                let round = |v: f64| (v * self.magnitude * 1000.0).round() / 1000.0 + 0.0;
                self.components = Some(Vec3::new(round(d.x), round(d.y), round(d.z)));
                self.picking = false;
            }
            (false, Some(v)) => {
                self.components = None;
                if v.len() > 0.0 {
                    self.magnitude = (v.len() * 1000.0).round() / 1000.0;
                    (self.way, self.flip) = Way::of(Some(v));
                }
            }
            _ => {}
        }
    }
}

/// The fields of a support being set up.
pub(crate) fn support_fields(ui: &mut Ui, p: &mut SupportPanel) -> bool {
    let mut clear = false;
    section(ui, "Location", true, |ui| {
        egui::Grid::new("tn_props_support").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Faces").on_hover_text(format!("{}: click them in the part", p.faces_are()));
            clear = picked_row(ui, p.faces.len(), "click faces");
            ui.end_row();
        });
    });
    if let SupportKind::Pin { radial, axial, tangential } = &mut p.kind {
        section(ui, "Hold", true, |ui| {
            ui.checkbox(radial, "Fix Radial Direction").on_hover_text("The face cannot move across the pin");
            ui.checkbox(axial, "Fix Axial Direction").on_hover_text("The face cannot move along the pin");
            ui.checkbox(tangential, "Fix Tangential Direction").on_hover_text("The face cannot turn on the pin");
        });
    }
    clear
}

/// The fields of a load being set up. `face_normal`: the way its first face looks, if that is
/// flat. Returns (clear the faces, Enter was pressed).
pub(crate) fn load_fields(ui: &mut Ui, p: &mut LoadPanel, face_normal: Option<Vec3>, t: &Tokens) -> (bool, bool) {
    let (mut clear, mut enter) = (false, false);
    if p.what != What::Gravity {
        section(ui, "Location", true, |ui| {
            egui::Grid::new("tn_props_load_faces").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Faces").on_hover_text("The faces it is on: click them in the part");
                let active = !p.picking;
                let label = if p.faces.is_empty() { "click faces".to_owned() } else { format!("{} selected", p.faces.len()) };
                ui.horizontal(|ui| {
                    // (The faces take the clicks unless the direction does.)
                    if slot_button(ui, active, &label, t) {
                        p.picking = false;
                    }
                    if !p.faces.is_empty() && ui.small_button("✖").on_hover_text("Clear").clicked() {
                        clear = true;
                    }
                });
                ui.end_row();
            });
        });
    }
    section(ui, "Magnitude", true, |ui| {
        egui::Grid::new("tn_props_load_size").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            let mag = egui::Id::new("tn_props_load_mag");
            match (p.what, &mut p.components) {
                (What::Pressure, _) => {
                    ui.label("Pressure").on_hover_text("On the faces, square to them and into them. Negative pulls.");
                    enter |= eq_field(ui, mag, &mut p.magnitude, None, "MPa", -1.0e6..=1.0e6, 110.0, t).entered;
                    ui.end_row();
                }
                (What::Gravity, _) => {
                    ui.label("Acceleration").on_hover_text("How hard the part's own weight pulls: 9810 mm/s² on the Earth");
                    enter |= eq_field(ui, mag, &mut p.magnitude, None, "mm/s²", 0.001..=1.0e9, 110.0, t).entered;
                    ui.end_row();
                }
                (What::Force, Some(v)) => {
                    for (label, id, part) in
                        [("Fx", "tn_props_load_fx", &mut v.x), ("Fy", "tn_props_load_fy", &mut v.y), ("Fz", "tn_props_load_fz", &mut v.z)]
                    {
                        ui.label(label).on_hover_text("The part of the force along that axis of the part");
                        enter |= eq_field(ui, egui::Id::new(id), part, None, "N", -1.0e12..=1.0e12, 110.0, t).entered;
                        ui.end_row();
                    }
                }
                (What::Force, None) => {
                    ui.label("Force").on_hover_text("The whole force, spread evenly over the faces");
                    enter |= eq_field(ui, mag, &mut p.magnitude, None, "N", 0.001..=1.0e12, 110.0, t).entered;
                    ui.end_row();
                }
            }
        });
    });
    if p.what != What::Pressure {
        section(ui, "Direction", true, |ui| {
            if p.components.is_none() {
                egui::Grid::new("tn_props_load_dir").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Direction")
                        .on_hover_text("Click, then a straight edge, a flat face or a work axis in the part; or choose an axis from the list");
                    ui.horizontal(|ui| {
                        let pick = ui.scope(|ui| slot_button(ui, p.picking, &p.way.label(), t));
                        crate::drawing::remember(ui, "tn_props_load_pick", pick.response.rect);
                        if pick.inner {
                            p.picking = !p.picking;
                        }
                        let list = egui::ComboBox::from_id_salt("tn_props_load_way").selected_text("").width(18.0).show_ui(ui, |ui| {
                            let ways: &[Way] = if p.what == What::Force { &[Way::Normal, Way::X, Way::Y, Way::Z] } else { &[Way::X, Way::Y, Way::Z] };
                            for way in ways {
                                if ui.selectable_label(p.way == *way, way.label()).clicked() {
                                    p.way = *way;
                                    p.flip = false;
                                    p.picking = false;
                                }
                            }
                        });
                        crate::drawing::remember(ui, "tn_props_load_way", list.response.rect);
                    });
                    ui.end_row();
                    ui.label("");
                    flip_button(ui, &mut p.flip, t);
                    ui.end_row();
                });
            }
            if p.what == What::Force {
                let mut on = p.components.is_some();
                if ui.checkbox(&mut on, "Use Vector Components").on_hover_text("Say the force by its parts along X, Y and Z").changed() {
                    p.set_components(on, face_normal);
                }
            }
        });
    }
    (clear, enter)
}

fn load_text(kind: &LoadKind) -> String {
    let n = |v: f64| if v.fract() == 0.0 && v.abs() < 1e9 { format!("{v:.0}") } else { fmt_value(v) };
    match kind {
        LoadKind::Force { newtons, .. } => format!("{} N", n(*newtons)),
        LoadKind::Pressure { megapascals } => format!("{} MPa", n(*megapascals)),
        LoadKind::Gravity { acceleration, .. } => format!("{} m/s²", fmt_value(acceleration / 1000.0)),
    }
}

/// What a row says of faces the part no longer has: " (face lost)", " (1 of 3 faces lost)".
fn lost_note(lost: usize, of: usize) -> String {
    match (lost, of) {
        (0, _) => String::new(),
        (1, 1) => " (face lost)".into(),
        (lost, of) if lost == of => " (faces lost)".into(),
        (lost, of) => format!(" ({lost} of {of} faces lost)"),
    }
}

fn support_icon(kind: SupportKind) -> Icon {
    match kind {
        SupportKind::Fixed => Icon::Fixed,
        SupportKind::Pin { .. } => Icon::Pin,
        SupportKind::Frictionless => Icon::Frictionless,
    }
}

/// What the browser asks for.
enum StudyAction {
    Activate(StudyId),
    Select(u32),
    Edit(u32),
    Rename(StudyId, Option<u32>, String),
    Suppress(u32, bool),
    Remove(u32),
    Copy(StudyId),
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
            "analysis.marks" => self.toggle_marks(),
            "study.new" => self.study_create(),
            "study.fix" => self.open_support(SupportKind::Fixed, None),
            "study.pin" => self.open_support(SupportKind::PIN, None),
            "study.frictionless" => self.open_support(SupportKind::Frictionless, None),
            "study.force" => self.open_load(What::Force, None),
            "study.pressure" => self.open_load(What::Pressure, None),
            "study.gravity" => self.open_load(What::Gravity, None),
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
        self.analysis = Some(Env { study, ..Env::default() });
        self.chrome.tab = ANALYSIS_TAB;
        self.set_status(match study {
            Some(_) => "Stress Analysis: say where the part is held (Fixed) and what pushes on it (Force, Pressure).",
            None => "Stress Analysis: Create Study, then say where the part is held and what pushes on it.",
        });
        Ok(())
    }

    /// Finish Analysis: back to the part.
    pub(crate) fn analysis_finish(&mut self) {
        if matches!(self.panel, Some(Panel::Support(_) | Panel::Load(_) | Panel::RenameStudy { .. })) {
            self.panel = None;
        }
        self.analysis = None;
        self.chrome.tab = self.home_tab();
        self.set_status("Ready");
    }

    /// Boundary Conditions: supports and loads drawn on the part, or not.
    fn toggle_marks(&mut self) -> Result<(), String> {
        if !self.in_analysis() {
            return Err("Boundary Conditions is of the Stress Analysis environment (Environments > Stress Analysis).".into());
        }
        let Some(env) = self.analysis.as_mut() else { return Ok(()) };
        env.hide_marks = !env.hide_marks;
        let hidden = env.hide_marks;
        self.set_status(if hidden {
            "Supports and loads are hidden: Boundary Conditions shows them again."
        } else {
            "Supports and loads are shown on the part."
        });
        Ok(())
    }

    /// Create Study: a new study, which becomes the one worked on.
    pub(crate) fn study_create(&mut self) -> Result<(), String> {
        if !self.in_analysis() {
            self.analysis_begin()?;
        }
        let made = self.exec("study.new", json!({}))?;
        self.work_on(&made);
        self.set_status(format!("{} created: say where the part is held (Fixed) and what pushes on it.", made["name"].as_str().unwrap_or("Study")));
        Ok(())
    }

    /// The study a command made becomes the one worked on.
    fn work_on(&mut self, made: &Value) {
        let id = made["study"].as_u64().and_then(|v| u32::try_from(v).ok()).map(StudyId);
        if let Some(env) = &mut self.analysis {
            env.study = id;
            env.selected = None;
        }
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

    /// How many of `faces` the part no longer has (a feature that made them was deleted or
    /// changed). Nothing is called lost while the part is still being built.
    fn lost_faces(&self, faces: &[FaceRef]) -> usize {
        if self.shown.is_none() || self.is_busy() || self.scene.bodies.is_empty() {
            return 0;
        }
        faces.iter().filter(|r| crate::modify::find_face(&self.scene, r).is_none()).count()
    }

    /// The faces of a support or load that is opened to be changed: those the part still has.
    /// The rest are left out, and the status bar says so.
    fn faces_still_there(&mut self, name: &str, faces: &[FaceRef]) -> Vec<FaceRef> {
        let lost = self.lost_faces(faces);
        if lost == 0 {
            return faces.to_vec();
        }
        let which = if faces.len() == 1 { "its face is".to_owned() } else { format!("{lost} of its {} faces are", faces.len()) };
        self.set_error(format!("{name}: {which} no longer on the part, and left out. Click its faces again, then OK."));
        faces.iter().filter(|r| crate::modify::find_face(&self.scene, r).is_some()).cloned().collect()
    }

    /// Fixed, Pin or Frictionless: a new support, or (with `editing`) one of the study's
    /// changed.
    pub(crate) fn open_support(&mut self, kind: SupportKind, editing: Option<u32>) -> Result<(), String> {
        let study = self.study_for_new()?;
        let mut told = false;
        let mut panel = match editing {
            Some(id) => {
                let s = self.active_study().and_then(|s| s.supports.iter().find(|x| x.id == id)).cloned().ok_or("no such support")?;
                let faces = self.faces_still_there(&s.name, &s.faces);
                told = faces.len() < s.faces.len();
                SupportPanel { study, editing, kind: s.kind, faces }
            }
            None => SupportPanel { study, editing, kind, faces: self.picked_faces() },
        };
        // (Of what was selected beforehand, the faces this kind of support can hold.)
        panel.faces.retain(|f| panel.kind.takes(&f.fingerprint.surface).is_ok());
        self.view.selection.clear();
        if !told {
            self.set_status(format!("{}: click {}, then OK.", panel.title(), panel.faces_are()));
        }
        self.panel = Some(Panel::Support(panel));
        self.start_equations();
        Ok(())
    }

    /// Force, Pressure or Gravity: a new load, or (with `editing`) one of the study's changed.
    pub(crate) fn open_load(&mut self, what: What, editing: Option<u32>) -> Result<(), String> {
        let study = self.study_for_new()?;
        let blank = |what, faces, magnitude| LoadPanel {
            study,
            editing,
            what,
            faces,
            magnitude,
            way: Way::Normal,
            flip: false,
            picking: false,
            components: None,
        };
        let mut told = false;
        let panel = match editing {
            Some(id) => {
                let load = self.active_study().and_then(|s| s.loads.iter().find(|l| l.id == id)).cloned().ok_or("no such load")?;
                let faces = self.faces_still_there(&load.name, &load.faces);
                told = faces.len() < load.faces.len();
                match load.kind {
                    LoadKind::Pressure { megapascals } => blank(What::Pressure, faces, megapascals),
                    LoadKind::Force { newtons, direction } => {
                        let (way, flip) = Way::of(direction);
                        // (Square to its faces, a negative force pulls: shown as flipped.)
                        let pulls = direction.is_none() && newtons < 0.0;
                        LoadPanel { way, flip: flip || pulls, ..blank(What::Force, faces, newtons.abs()) }
                    }
                    LoadKind::Gravity { direction, acceleration } => {
                        let (way, flip) = Way::of(Some(direction));
                        LoadPanel { way, flip, ..blank(What::Gravity, Vec::new(), acceleration) }
                    }
                }
            }
            None => match what {
                What::Force => blank(what, self.picked_faces(), 100.0),
                What::Pressure => blank(what, self.picked_faces(), 1.0),
                What::Gravity => {
                    if let Some(there) = self.active_study().and_then(|s| s.loads.iter().find(|l| !l.kind.on_faces())) {
                        return Err(format!("The study has the part's weight already ({}): double-click it to change it.", there.name));
                    }
                    // (Down: against Z.)
                    LoadPanel { way: Way::Z, flip: true, ..blank(what, Vec::new(), EARTH) }
                }
            },
        };
        self.view.selection.clear();
        if !told {
            self.set_status(match panel.what {
                What::Pressure => "Pressure: click the faces it is on, set how large it is, then OK.",
                What::Force => "Force: click the faces it is on, set how large it is and which way it pushes, then OK.",
                What::Gravity => "Gravity: the part's own weight, from its material's density. Set which way it pulls, then OK.",
            });
        }
        self.panel = Some(Panel::Load(panel));
        self.start_equations();
        Ok(())
    }

    /// OK on a support. Returns true when it was made or changed.
    pub(crate) fn commit_support(&mut self, p: &SupportPanel) -> bool {
        if p.faces.is_empty() {
            self.set_error(format!("{}: click at least one face", p.title()));
            return false;
        }
        let (id, params) = p.command();
        self.exec_status(id, params).is_some()
    }

    /// OK on a load. Returns true when it was made or changed.
    pub(crate) fn commit_load(&mut self, p: &LoadPanel) -> bool {
        if p.what != What::Gravity && p.faces.is_empty() {
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

    /// The way the first face of the open load panel looks, if that face is flat.
    pub(crate) fn load_face_normal(&self) -> Option<Vec3> {
        let Some(Panel::Load(p)) = &self.panel else { return None };
        self.face_at(p.faces.first()?)?.1
    }

    /// Whether the open load panel's direction selector is the one that takes clicks.
    pub(crate) fn picking_direction(&self) -> bool {
        matches!(&self.panel, Some(Panel::Load(p)) if p.picking && p.components.is_none())
    }

    /// The direction a click on that edge or face gives: along a straight edge, or square to a
    /// flat face and into it.
    pub(crate) fn direction_of(&self, pick: Pick) -> Option<Vec3> {
        match pick {
            Pick::Edge { body, edge } => {
                // (The scene keeps an edge's points, not what curve it is: straight is every
                // point on the line from its first to its last.)
                let line = self.scene.bodies.get(body)?.mesh.edges.iter().find(|e| e.edge == edge)?;
                let at = |q: &[f32; 3]| Vec3::new(f64::from(q[0]), f64::from(q[1]), f64::from(q[2]));
                let (a, z) = (at(line.points.first()?), at(line.points.last()?));
                let len = (z - a).len();
                if len < 1e-6 {
                    return None;
                }
                let d = (z - a) * (1.0 / len);
                let straight = line.points.iter().all(|q| {
                    let r = at(q) - a;
                    (r - d * r.dot(d)).len() <= 1e-4 * len.max(1.0)
                });
                // (The points are single precision: an edge along X is along X, not nearly.)
                let round = |v: f64| (v * 1.0e6).round() / 1.0e6 + 0.0;
                straight.then(|| Vec3::new(round(d.x), round(d.y), round(d.z)))
            }
            Pick::Face { body, face } => match self.scene.bodies.get(body)?.faces.get(face as usize)?.1.surface {
                SurfaceKind::Plane { normal, .. } => Some(-normal),
                _ => None,
            },
        }
    }

    /// Gives the open load panel its direction (from a click on an edge, a face or a work
    /// axis). Returns false when no load panel is waiting for one.
    pub(crate) fn set_load_direction(&mut self, d: Vec3) -> bool {
        if !self.picking_direction() || d.len() <= 0.0 {
            return false;
        }
        if let Some(Panel::Load(p)) = &mut self.panel {
            (p.way, p.flip) = Way::of(Some(d));
            p.picking = false;
        }
        true
    }

    /// The direction of work axis `id`, as the part has it now.
    pub(crate) fn work_axis_direction(&self, id: tenon_model::FeatureId) -> Option<Vec3> {
        self.scene.work.iter().find(|(w, _)| *w == id).and_then(|(_, g)| match g {
            WorkGeom::Axis(a) => Some(a.dir()),
            _ => None,
        })
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

    /// Where the part's weight is drawn: its centre of gravity (of all its solids).
    fn weight_at(&self) -> Option<Vec3> {
        let volume: f64 = self.scene.bodies.iter().map(|b| b.mass.volume).sum();
        if volume <= 0.0 {
            return self.scene.bbox().map(|b| b.center());
        }
        let moment = self.scene.bodies.iter().fold(Vec3::ZERO, |m, b| m + b.mass.center_of_mass * b.mass.volume);
        Some(moment * (1.0 / volume))
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
        let label_at = |at: Pos2, align: Align2, text: &str| {
            painter.text(at + vec2(1.0, 1.0), align, text, theme::body(), edge);
            painter.text(at, align, text, theme::body(), t.viewport_text);
        };
        // An arrow with its head at the face and its tail back the way the load comes from;
        // `spread`: three of them side by side, for a load that is all over the face;
        // `beside`: its size written beside its shaft and not at its tail (the part's weight
        // starts where the marks of the faces above it are).
        let arrow = |tip: Vec3, dir: Vec3, color: Color32, label: Option<&str>, spread: bool, beside: bool| {
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
            match label {
                Some(text) if beside => {
                    let side = if across.x >= 0.0 { across } else { -across };
                    label_at(head + (tail - head) * 0.5 + side * 10.0, Align2::LEFT_CENTER, text);
                }
                Some(text) => label_at(head + (tail - head) * length - along * 12.0, Align2::CENTER_CENTER, text),
                None => {}
            }
        };
        // A support's mark: the ground mark for a fixed face, a pin seen end on, a bar on
        // rollers for a face that slides.
        let held = |at: Vec3, kind: SupportKind, color: Color32| {
            let Some(c) = screen(at) else { return };
            mark("held", c);
            for (width, color) in [(5.0, edge), (2.5, color)] {
                match kind {
                    SupportKind::Fixed => {
                        painter.line_segment([c + vec2(-12.0, 0.0), c + vec2(12.0, 0.0)], Stroke::new(width, color));
                        for i in 0..5 {
                            let x = -10.0 + 5.0 * i as f32;
                            painter.line_segment([c + vec2(x + 4.0, 1.0), c + vec2(x, 9.0)], Stroke::new(width - 1.0, color));
                        }
                    }
                    SupportKind::Pin { .. } => {
                        painter.circle_stroke(c, 9.0, Stroke::new(width, color));
                        painter.circle_filled(c, width, color);
                    }
                    SupportKind::Frictionless => {
                        painter.line_segment([c + vec2(-12.0, -4.0), c + vec2(12.0, -4.0)], Stroke::new(width, color));
                        for x in [-7.0, 7.0] {
                            painter.circle_stroke(c + vec2(x, 2.0), 4.0, Stroke::new(width - 1.0, color));
                        }
                        painter.line_segment([c + vec2(-12.0, 8.0), c + vec2(12.0, 8.0)], Stroke::new(width - 1.0, color));
                    }
                }
            }
        };
        let load = |faces: &[FaceRef], kind: &LoadKind, color: Color32| {
            let text = load_text(kind);
            if let LoadKind::Gravity { direction, .. } = kind {
                // The part's weight: an arrow from its centre of gravity, the way it pulls.
                if let (Some(at), len) = (self.weight_at(), direction.len())
                    && len > 0.0
                {
                    let d = *direction * (1.0 / len);
                    arrow(at + d * (reach * 2.0), d, color, Some(&format!("g {text}")), false, true);
                }
                return;
            }
            for (i, r) in faces.iter().enumerate() {
                let Some((at, normal)) = self.face_at(r) else { continue };
                let label = (i == 0).then_some(text.as_str());
                let dir = match kind {
                    LoadKind::Force { direction: Some(d), newtons } => Some(if *newtons < 0.0 { -*d } else { *d }),
                    LoadKind::Force { direction: None, newtons } => normal.map(|n| if *newtons < 0.0 { n } else { -n }),
                    LoadKind::Pressure { megapascals } => normal.map(|n| if *megapascals < 0.0 { n } else { -n }),
                    LoadKind::Gravity { .. } => None,
                };
                match dir {
                    Some(d) => arrow(at, d, color, label, matches!(kind, LoadKind::Pressure { .. }), false),
                    // A round face is pushed all over, each way its own: marked where it is.
                    None => {
                        if let Some(c) = screen(at) {
                            mark("load", c);
                            ring(c, color);
                            if let Some(text) = label {
                                label_at(c + vec2(0.0, -16.0), Align2::CENTER_CENTER, text);
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
        let hidden = self.analysis.as_ref().is_some_and(|e| e.hide_marks);
        if let Some(study) = self.active_study().filter(|_| !hidden) {
            // (What is suppressed is not there for now, and is not drawn.)
            for s in study.supports.iter().filter(|s| Some(s.id) != editing && !s.suppressed) {
                for r in &s.faces {
                    if let Some((at, _)) = self.face_at(r) {
                        held(at, s.kind, HELD);
                    }
                }
            }
            for l in study.loads.iter().filter(|l| Some(l.id) != editing && !l.suppressed) {
                load(&l.faces, &l.kind, LOADED);
            }
        }
        // What is being set up, as it will be.
        match &self.panel {
            Some(Panel::Support(p)) => {
                for r in &p.faces {
                    if let Some((at, _)) = self.face_at(r) {
                        held(at, p.kind, t.accent);
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
        // (How many faces of each support and load of the study worked on the part no longer has.)
        let lost: std::collections::BTreeMap<u32, usize> = studies
            .iter()
            .filter(|s| Some(s.id) == active)
            .flat_map(|s| s.supports.iter().map(|x| (x.id, &x.faces)).chain(s.loads.iter().map(|l| (l.id, &l.faces))))
            .map(|(id, faces)| (id, self.lost_faces(faces)))
            .collect();
        let lost = |id: u32| lost.get(&id).copied().unwrap_or(0);
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
                let resp = resp.on_hover_text("Double-click to work on this study; right-click to rename, copy or delete it");
                if resp.double_clicked() {
                    action = Some(StudyAction::Activate(study.id));
                }
                resp.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        action = Some(StudyAction::Rename(study.id, None, study.name.clone()));
                        ui.close();
                    }
                    if ui.button("Copy Study").on_hover_text("The same study again, to change and compare").clicked() {
                        action = Some(StudyAction::Copy(study.id));
                        ui.close();
                    }
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
                // (What the study still lacks is written in red, and says why when pointed at.
                // What is suppressed does not count.)
                let needed = |lacking: bool| if lacking { RowStyle::Error } else { RowStyle::Normal };
                let unheld = study.supports.iter().all(|s| s.suppressed);
                let (resp, _) = row(ui, t, 2, Icon::Folder, &format!("Constraints({})", study.supports.len()), None, needed(unheld));
                if unheld {
                    resp.on_hover_text("The study needs a support (Fixed): a part that is held nowhere moves off, however small the load");
                }
                let style = |id: u32, suppressed: bool| match (Some(id) == selected, suppressed) {
                    (true, _) => RowStyle::Active,
                    (false, true) => RowStyle::Dim,
                    (false, false) if lost(id) > 0 => RowStyle::Error,
                    (false, false) => RowStyle::Normal,
                };
                let quiet = |suppressed: bool| if suppressed { " (suppressed)" } else { "" };
                for s in &study.supports {
                    let label = format!("{}{}{}", s.name, lost_note(lost(s.id), s.faces.len()), quiet(s.suppressed));
                    let (resp, _) = row(ui, t, 3, support_icon(s.kind), &label, None, style(s.id, s.suppressed));
                    item_row(resp, study.id, s.id, &s.name, s.suppressed, lost(s.id) > 0, &mut action);
                }
                let unloaded = study.loads.iter().all(|l| l.suppressed);
                let (resp, _) = row(ui, t, 2, Icon::Folder, &format!("Loads({})", study.loads.len()), None, needed(unloaded));
                if unloaded {
                    resp.on_hover_text("The study needs a load (Force, Pressure or Gravity): with nothing pushing on it there is no stress to find");
                }
                for l in &study.loads {
                    let icon = match l.kind {
                        LoadKind::Force { .. } => Icon::Force,
                        LoadKind::Pressure { .. } => Icon::Pressure,
                        LoadKind::Gravity { .. } => Icon::Gravity,
                    };
                    let label = format!("{} ({}){}{}", l.name, load_text(&l.kind), lost_note(lost(l.id), l.faces.len()), quiet(l.suppressed));
                    let (resp, _) = row(ui, t, 3, icon, &label, None, style(l.id, l.suppressed));
                    item_row(resp, study.id, l.id, &l.name, l.suppressed, lost(l.id) > 0, &mut action);
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
        let study = self.active_study().map(|s| s.id.0);
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
            Some(StudyAction::Rename(study, item, name)) => {
                self.panel = Some(Panel::RenameStudy { study, item, name });
                Ok(())
            }
            Some(StudyAction::Suppress(id, on)) => self.exec("study.set", json!({ "study": study, "item": id, "suppressed": on })).map(|_| ()),
            Some(StudyAction::Remove(id)) => self.exec("study.remove", json!({ "study": study, "item": id })).map(|_| ()),
            Some(StudyAction::Copy(id)) => self.exec("study.copy", json!({ "study": id.0 })).map(|made| {
                self.work_on(&made);
                self.set_status(format!("{} is a copy of the study: change it, and compare.", made["name"].as_str().unwrap_or("The new study")));
            }),
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
        if let Some(s) = study.supports.iter().find(|s| s.id == id) {
            return self.open_support(s.kind, Some(id));
        }
        let what = study.loads.iter().find(|l| l.id == id).map(|l| match l.kind {
            LoadKind::Force { .. } => What::Force,
            LoadKind::Pressure { .. } => What::Pressure,
            LoadKind::Gravity { .. } => What::Gravity,
        });
        match what {
            Some(what) => self.open_load(what, Some(id)),
            None => Err("no such support or load".into()),
        }
    }
}

/// A support's or a load's row: a click picks its faces out, a double click opens it, the menu
/// edits, renames, suppresses or deletes it.
fn item_row(resp: egui::Response, study: StudyId, id: u32, name: &str, suppressed: bool, lost: bool, action: &mut Option<StudyAction>) {
    let resp = resp.on_hover_text(if lost {
        "It is on a face the part no longer has (a feature that made it was deleted or changed): double-click it and click its faces again"
    } else {
        "Click to see its faces; double-click to change it; right-click for more"
    });
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
        if ui.button("Rename").clicked() {
            *action = Some(StudyAction::Rename(study, Some(id), name.to_owned()));
            ui.close();
        }
        let (label, tip) =
            if suppressed { ("Unsuppress", "Put it back into the study") } else { ("Suppress", "Leave it out of the study for now, and keep it") };
        if ui.button(label).on_hover_text(tip).clicked() {
            *action = Some(StudyAction::Suppress(id, !suppressed));
            ui.close();
        }
        if ui.button("Delete").clicked() {
            *action = Some(StudyAction::Remove(id));
            ui.close();
        }
    });
}
