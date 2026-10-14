//! The Stress Analysis environment (Environments > Stress Analysis): a study of the part set
//! up on the part itself. Its faces are clicked to say where it is held and what pushes on it;
//! the browser shows the study in place of the features; supports and loads are drawn on the
//! part. Everything it does is a `study.*` command (DEC-093).
//!
//! Working a study out, and showing what it found, is `crate::results`.

use egui::{Align2, Color32, Pos2, Rect, Stroke, Ui, vec2};
use serde_json::{Value, json};
use tenon_geom::Vec3;
use tenon_kernel::SurfaceKind;
use tenon_model::study::{EARTH, LoadKind, Study, StudyId, SupportKind};
use tenon_model::{EdgeRef, FaceRef, WorkGeom};

use crate::browser::{RowStyle, row};
use crate::commands::ANALYSIS_TAB;
use crate::icons::Icon;
use crate::modify::{find_edge, find_face};
use crate::panels::Panel;
use crate::properties::{eq_field, flip_button, fmt_value, section, slot_button};
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
    /// What there is to show of a study: its mesh, and its results when it was solved.
    pub outcome: Option<crate::results::Outcome>,
    /// Which result is shown in colour, and how the part's bending is drawn.
    pub field: crate::results::Field,
    pub deform: crate::results::Deform,
    /// Mesh View: the mesh's lines are drawn.
    pub mesh_view: bool,
    /// A study is being worked out.
    pub running: Option<crate::results::Running>,
}

/// Fixed, Pin or Frictionless: the faces (or, of a fixed support, the edges) that are held.
#[derive(Clone, Debug)]
pub(crate) struct SupportPanel {
    pub study: StudyId,
    /// The support being changed; None for a new one.
    pub editing: Option<u32>,
    pub kind: SupportKind,
    pub faces: Vec<FaceRef>,
    /// A fixed support may be on edges in place of faces.
    pub edges: Vec<EdgeRef>,
}

impl SupportPanel {
    pub(crate) fn title(&self) -> &'static str {
        self.kind.word()
    }

    /// What it is on, for the status bar and the faces row.
    fn faces_are(&self) -> &'static str {
        match self.kind {
            SupportKind::Fixed => "the faces, or the edges, that are held still",
            SupportKind::Pin { .. } => "the round faces that are on the pin",
            SupportKind::Frictionless => "the flat or round faces that may slide",
        }
    }

    /// The command that makes or changes it.
    pub(crate) fn command(&self) -> (&'static str, Value) {
        let mut p = json!({ "study": self.study.0 });
        if self.edges.is_empty() {
            p["faces"] = json!(self.faces);
        } else {
            p["edges"] = json!(self.edges);
        }
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

/// The way a load pushes, or the axis it turns about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Way {
    /// Its faces' own: a force square to each face and into it, a moment about its face's axis.
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

    /// The same for a size with its sign: a negative size is the other way.
    fn of_signed(direction: Vec3, size: f64) -> (Way, bool, f64) {
        let (way, flip) = Way::of(Some(if size < 0.0 { -direction } else { direction }));
        (way, flip, size.abs())
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
    Moment,
    Bearing,
    Remote,
    Body,
}

impl What {
    fn of(kind: &LoadKind) -> What {
        match kind {
            LoadKind::Force { .. } => What::Force,
            LoadKind::Pressure { .. } => What::Pressure,
            LoadKind::Gravity { .. } => What::Gravity,
            LoadKind::Moment { .. } => What::Moment,
            LoadKind::Bearing { .. } => What::Bearing,
            LoadKind::RemoteForce { .. } => What::Remote,
            LoadKind::Body { .. } => What::Body,
        }
    }

    fn title(self) -> &'static str {
        match self {
            What::Force => "Force",
            What::Pressure => "Pressure",
            What::Gravity => "Gravity",
            What::Moment => "Moment",
            What::Bearing => "Bearing Load",
            What::Remote => "Remote Force",
            What::Body => "Body",
        }
    }

    /// The ribbon's command that starts one.
    pub(crate) fn command(self) -> &'static str {
        match self {
            What::Force => "study.force",
            What::Pressure => "study.pressure",
            What::Gravity => "study.gravity",
            What::Moment => "study.moment",
            What::Bearing => "study.bearing",
            What::Remote => "study.remote_force",
            What::Body => "study.body",
        }
    }

    pub(crate) fn of_command(id: &str) -> Option<What> {
        [What::Force, What::Pressure, What::Gravity, What::Moment, What::Bearing, What::Remote, What::Body].into_iter().find(|w| w.command() == id)
    }

    /// Whether it is on faces (the part's weight and its turning are on all of it).
    pub(crate) fn on_faces(self) -> bool {
        !matches!(self, What::Gravity | What::Body)
    }

    /// Whether it turns about an axis, where the others push along a direction.
    fn turns(self) -> bool {
        matches!(self, What::Moment | What::Body)
    }

    /// How large it is: what the field is called, its unit, and what it may be.
    pub(crate) fn size(self) -> (&'static str, &'static str, std::ops::RangeInclusive<f64>, &'static str) {
        match self {
            What::Force => ("Force", "N", 0.001..=1.0e12, "The whole force, spread evenly over the faces (or along the edges)"),
            What::Pressure => ("Pressure", "MPa", -1.0e6..=1.0e6, "On the faces, square to them and into them. Negative pulls."),
            What::Gravity => ("Acceleration", "mm/s²", 0.001..=1.0e9, "How hard the part's own weight pulls: 9810 mm/s² on the Earth"),
            What::Moment => ("Moment", "N mm", 0.001..=1.0e15, "The whole turning load on the faces: 1 N m is 1000 N mm"),
            What::Bearing => ("Load", "N", 0.001..=1.0e12, "The whole load of the shaft, on the half of the round faces it pushes against"),
            What::Remote => ("Force", "N", 0.001..=1.0e12, "The force that acts at the point below"),
            What::Body => ("Speed", "rpm", 0.001..=1.0e7, "How fast the part turns, in turns a minute"),
        }
    }
}

/// A load: where it is, how large, which way.
#[derive(Clone, Debug)]
pub(crate) struct LoadPanel {
    pub study: StudyId,
    pub editing: Option<u32>,
    pub what: What,
    pub faces: Vec<FaceRef>,
    /// A force may be along edges in place of faces.
    pub edges: Vec<EdgeRef>,
    /// Newtons; megapascals for a pressure; mm/s^2 for gravity; N mm for a moment; turns a
    /// minute for a body.
    pub magnitude: f64,
    pub way: Way,
    /// Along its direction the other way; for one square to its faces, pulling.
    pub flip: bool,
    /// The direction selector is the one that takes clicks: the next click on a straight
    /// edge, a flat face or a work axis gives the direction.
    pub picking: bool,
    /// Use Vector Components: the force as its parts along X, Y and Z (N).
    pub components: Option<Vec3>,
    /// Where a remote force acts; a point the axis of a body's turning goes through.
    pub point: Vec3,
    /// The point's selector is the one that takes clicks: the next click on a face or an edge
    /// gives the point (its middle).
    pub picking_point: bool,
}

impl LoadPanel {
    pub(crate) fn title(&self) -> &'static str {
        self.what.title()
    }

    /// What the direction selector shows.
    fn way_label(&self) -> String {
        match (self.way, self.what) {
            (Way::Normal, What::Moment) => "Face's Own Axis".into(),
            (way, _) => way.label(),
        }
    }

    /// The load as it stands: its size with its sign, and where it points.
    pub(crate) fn shown(&self) -> LoadKind {
        let way = |fallback: Vec3| self.way.vector(self.flip).unwrap_or(fallback);
        let down = Vec3::new(0.0, 0.0, -1.0);
        // (Its faces' own way has no vector to turn round: the other way is a negative size.)
        let signed = if self.flip { -self.magnitude } else { self.magnitude };
        match (self.what, self.components) {
            (What::Pressure, _) => LoadKind::Pressure { megapascals: self.magnitude },
            (What::Gravity, _) => LoadKind::Gravity { direction: way(down), acceleration: self.magnitude },
            (What::Force, Some(v)) => {
                let newtons = v.len();
                LoadKind::Force { newtons, direction: (newtons > 0.0).then(|| v * (1.0 / newtons)) }
            }
            (What::Force, None) if self.way == Way::Normal => LoadKind::Force { newtons: signed, direction: None },
            (What::Force, None) => LoadKind::Force { newtons: self.magnitude, direction: self.way.vector(self.flip) },
            (What::Moment, _) if self.way == Way::Normal => LoadKind::Moment { newton_millimetres: signed, axis: None },
            (What::Moment, _) => LoadKind::Moment { newton_millimetres: self.magnitude, axis: self.way.vector(self.flip) },
            (What::Bearing, _) => LoadKind::Bearing { newtons: self.magnitude, direction: way(Vec3::X) },
            (What::Remote, _) => LoadKind::RemoteForce { newtons: self.magnitude, direction: way(down), point: self.point },
            (What::Body, _) => LoadKind::Body { axis: way(Vec3::Z), point: self.point, rpm: self.magnitude },
        }
    }

    /// The command that makes or changes it.
    pub(crate) fn command(&self) -> (&'static str, Value) {
        let mut p = json!({ "study": self.study.0 });
        if self.what.on_faces() {
            if self.edges.is_empty() {
                p["faces"] = json!(self.faces);
            } else {
                p["edges"] = json!(self.edges);
            }
        }
        let xyz = |d: Vec3| json!([d.x, d.y, d.z]);
        match self.shown() {
            LoadKind::Pressure { megapascals } => p["megapascals"] = json!(megapascals),
            LoadKind::Force { newtons, direction } => {
                p["newtons"] = json!(newtons);
                p["direction"] = direction.map_or(json!("normal"), xyz);
            }
            LoadKind::Gravity { direction, acceleration } => {
                p["direction"] = xyz(direction);
                p["acceleration"] = json!(acceleration);
            }
            LoadKind::Moment { newton_millimetres, axis } => {
                p["newton_millimetres"] = json!(newton_millimetres);
                p["axis"] = axis.map_or(json!("own"), xyz);
            }
            LoadKind::Bearing { newtons, direction } => {
                p["newtons"] = json!(newtons);
                p["direction"] = xyz(direction);
            }
            LoadKind::RemoteForce { newtons, direction, point } => {
                p["newtons"] = json!(newtons);
                p["direction"] = xyz(direction);
                p["point"] = xyz(point);
            }
            LoadKind::Body { axis, point, rpm } => {
                p["rpm"] = json!(rpm);
                p["axis"] = xyz(axis);
                p["point"] = xyz(point);
            }
        }
        match self.editing {
            Some(item) => {
                p["item"] = json!(item);
                ("study.set", p)
            }
            None => (self.what.command(), p),
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

/// "2 faces", "1 edge", or what to do when there are none.
fn picked_text(faces: usize, edges: usize, none: &str) -> String {
    let count = |n: usize, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
    match (faces, edges) {
        (0, 0) => none.to_owned(),
        (f, 0) => count(f, "face"),
        (_, e) => count(e, "edge"),
    }
}

/// The fields of a support being set up.
pub(crate) fn support_fields(ui: &mut Ui, p: &mut SupportPanel) -> bool {
    let mut clear = false;
    let fixed = p.kind == SupportKind::Fixed;
    section(ui, "Location", true, |ui| {
        egui::Grid::new("tn_props_support").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(if fixed { "Faces or Edges" } else { "Faces" }).on_hover_text(format!("{}: click them in the part", p.faces_are()));
            ui.horizontal(|ui| {
                ui.label(picked_text(p.faces.len(), p.edges.len(), if fixed { "click faces or edges" } else { "click faces" }));
                if !(p.faces.is_empty() && p.edges.is_empty()) && ui.small_button("✖").on_hover_text("Clear").clicked() {
                    clear = true;
                }
            });
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
/// flat. Returns (clear what it is on, Enter was pressed).
pub(crate) fn load_fields(ui: &mut Ui, p: &mut LoadPanel, face_normal: Option<Vec3>, t: &Tokens) -> (bool, bool) {
    let (mut clear, mut enter) = (false, false);
    if p.what.on_faces() {
        section(ui, "Location", true, |ui| {
            egui::Grid::new("tn_props_load_faces").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                let (name, tip, none) = match p.what {
                    What::Force => (
                        "Faces or Edges",
                        "The faces it is spread over, or the edges it is spread along: click them in the part",
                        "click faces or edges",
                    ),
                    What::Bearing => ("Faces", "The round faces the shaft presses on: click them in the part", "click round faces"),
                    _ => ("Faces", "The faces it is on: click them in the part", "click faces"),
                };
                ui.label(name).on_hover_text(tip);
                let active = !p.picking;
                let label = picked_text(p.faces.len(), p.edges.len(), none);
                ui.horizontal(|ui| {
                    // (What it is on takes the clicks unless the direction does.)
                    if slot_button(ui, active, &label, t) {
                        p.picking = false;
                    }
                    if !(p.faces.is_empty() && p.edges.is_empty()) && ui.small_button("✖").on_hover_text("Clear").clicked() {
                        clear = true;
                    }
                });
                ui.end_row();
            });
        });
    }
    section(ui, "Magnitude", true, |ui| {
        egui::Grid::new("tn_props_load_size").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| match &mut p.components {
            Some(v) if p.what == What::Force => {
                for (label, id, part) in
                    [("Fx", "tn_props_load_fx", &mut v.x), ("Fy", "tn_props_load_fy", &mut v.y), ("Fz", "tn_props_load_fz", &mut v.z)]
                {
                    ui.label(label).on_hover_text("The part of the force along that axis of the part");
                    enter |= eq_field(ui, egui::Id::new(id), part, None, "N", -1.0e12..=1.0e12, 110.0, t).entered;
                    ui.end_row();
                }
            }
            _ => {
                let (label, unit, range, tip) = p.what.size();
                ui.label(label).on_hover_text(tip);
                enter |= eq_field(ui, egui::Id::new("tn_props_load_mag"), &mut p.magnitude, None, unit, range, 110.0, t).entered;
                ui.end_row();
            }
        });
    });
    if p.what != What::Pressure {
        let turns = p.what.turns();
        section(ui, if turns { "Axis" } else { "Direction" }, true, |ui| {
            if p.components.is_none() {
                egui::Grid::new("tn_props_load_dir").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label(if turns { "Axis" } else { "Direction" }).on_hover_text(if turns {
                        "Click, then a straight edge, a face or a work axis in the part; or choose an axis from the list. Seen from the axis's tip it turns anticlockwise"
                    } else {
                        "Click, then a straight edge, a flat face or a work axis in the part; or choose an axis from the list"
                    });
                    ui.horizontal(|ui| {
                        let pick = ui.scope(|ui| slot_button(ui, p.picking, &p.way_label(), t));
                        crate::drawing::remember(ui, "tn_props_load_pick", pick.response.rect);
                        if pick.inner {
                            p.picking = !p.picking;
                            p.picking_point = false;
                        }
                        let list = egui::ComboBox::from_id_salt("tn_props_load_way").selected_text("").width(18.0).show_ui(ui, |ui| {
                            // (A force may be square to its faces, a moment about its faces' own axis.)
                            let own = matches!(p.what, What::Force | What::Moment) && p.edges.is_empty();
                            let ways: &[Way] = if own { &[Way::Normal, Way::X, Way::Y, Way::Z] } else { &[Way::X, Way::Y, Way::Z] };
                            for way in ways {
                                let label = if *way == Way::Normal && p.what == What::Moment { "Face's Own Axis".to_owned() } else { way.label() };
                                if ui.selectable_label(p.way == *way, label).clicked() {
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
    if matches!(p.what, What::Remote | What::Body) {
        let (title, tip) = if p.what == What::Remote {
            ("Acts At", "Where the force acts, in the part's coordinates")
        } else {
            ("Axis Through", "A point the axis of its turning goes through, in the part's coordinates")
        };
        section(ui, title, true, |ui| {
            egui::Grid::new("tn_props_load_point").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Point").on_hover_text("Click, then a face or an edge in the part: its middle; or type it below");
                let pick = ui.scope(|ui| slot_button(ui, p.picking_point, if p.picking_point { "click in the part" } else { "Pick" }, t));
                crate::drawing::remember(ui, "tn_props_load_point_pick", pick.response.rect);
                if pick.inner {
                    p.picking_point = !p.picking_point;
                    p.picking = false;
                }
                ui.end_row();
                for (label, id, part) in
                    [("X", "tn_props_load_px", &mut p.point.x), ("Y", "tn_props_load_py", &mut p.point.y), ("Z", "tn_props_load_pz", &mut p.point.z)]
                {
                    ui.label(label).on_hover_text(tip);
                    enter |= eq_field(ui, egui::Id::new(id), part, None, "mm", -1.0e9..=1.0e9, 110.0, t).entered;
                    ui.end_row();
                }
            });
        });
    }
    (clear, enter)
}

fn load_text(kind: &LoadKind) -> String {
    let n = |v: f64| if v.fract() == 0.0 && v.abs() < 1e9 { format!("{v:.0}") } else { fmt_value(v) };
    match kind {
        LoadKind::Force { newtons, .. } | LoadKind::Bearing { newtons, .. } | LoadKind::RemoteForce { newtons, .. } => format!("{} N", n(*newtons)),
        LoadKind::Pressure { megapascals } => format!("{} MPa", n(*megapascals)),
        LoadKind::Gravity { acceleration, .. } => format!("{} m/s²", fmt_value(acceleration / 1000.0)),
        LoadKind::Moment { newton_millimetres, .. } => format!("{} N mm", n(*newton_millimetres)),
        LoadKind::Body { rpm, .. } => format!("{} rpm", n(*rpm)),
    }
}

/// What a row says of faces (or edges) the part no longer has: " (face lost)", " (1 of 3
/// edges lost)".
fn lost_note(lost: usize, of: usize, what: &str) -> String {
    match (lost, of) {
        (0, _) => String::new(),
        (1, 1) => format!(" ({what} lost)"),
        (lost, of) if lost == of => format!(" ({what}s lost)"),
        (lost, of) => format!(" ({lost} of {of} {what}s lost)"),
    }
}

fn support_icon(kind: SupportKind) -> Icon {
    match kind {
        SupportKind::Fixed => Icon::Fixed,
        SupportKind::Pin { .. } => Icon::Pin,
        SupportKind::Frictionless => Icon::Frictionless,
    }
}

fn load_icon(kind: &LoadKind) -> Icon {
    match kind {
        LoadKind::Force { .. } => Icon::Force,
        LoadKind::Pressure { .. } => Icon::Pressure,
        LoadKind::Gravity { .. } => Icon::Gravity,
        LoadKind::Moment { .. } => Icon::Moment,
        LoadKind::Bearing { .. } => Icon::BearingLoad,
        LoadKind::RemoteForce { .. } => Icon::RemoteForce,
        LoadKind::Body { .. } => Icon::BodyLoad,
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
    /// Shows a result in colour on the part.
    Show(crate::results::Field),
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
        // (Esc stops a study that is being worked out, before it is anything else.)
        if id == "ui.cancel" && self.stop_study() {
            return Some(Ok(()));
        }
        if !id.starts_with("analysis.") && !id.starts_with("study.") {
            let changes = ["sketch.", "work.", "sheet."].iter().any(|p| id.starts_with(p)) || (id.starts_with("model.") && id != "model.rebuild");
            return (self.in_analysis() && changes).then(|| Err("That changes the part: Finish Analysis first.".into()));
        }
        if self.in_assembly() || self.in_drawing() {
            return Some(Err("Stress Analysis is of a part: open the part, or edit it in place.".into()));
        }
        if let Some(what) = What::of_command(id) {
            return Some(self.open_load(what, None));
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
            "analysis.simulate" => self.work_study_out(true),
            "analysis.mesh_view" => self.toggle_mesh_view(),
            "analysis.deform" | "analysis.deform.undeformed" | "analysis.deform.adjusted" | "analysis.deform.actual" => self.set_deform(id),
            "study.new" => self.study_create(),
            "study.fix" => self.open_support(SupportKind::Fixed, None),
            "study.pin" => self.open_support(SupportKind::PIN, None),
            "study.frictionless" => self.open_support(SupportKind::Frictionless, None),
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

    /// Displacement Display: how the part's bending is drawn (the button goes round the three).
    fn set_deform(&mut self, id: &str) -> Result<(), String> {
        use crate::results::Deform;
        if !self.in_analysis() {
            return Err("Displacement Display is of the Stress Analysis environment (Environments > Stress Analysis).".into());
        }
        let Some(env) = self.analysis.as_mut() else { return Ok(()) };
        env.deform = match id {
            "analysis.deform.undeformed" => Deform::Undeformed,
            "analysis.deform.adjusted" => Deform::Adjusted,
            "analysis.deform.actual" => Deform::Actual,
            _ => match env.deform {
                Deform::Adjusted => Deform::Actual,
                Deform::Actual => Deform::Undeformed,
                Deform::Undeformed => Deform::Adjusted,
            },
        };
        let said = match env.deform {
            Deform::Undeformed => "Results are shown on the part as it is, not bent.",
            Deform::Adjusted => "The part's bending is shown large enough to see (the scale says how many times over).",
            Deform::Actual => "The part is shown bent as far as it really moves.",
        };
        self.set_status(said);
        Ok(())
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

    /// How many of those faces and edges the part no longer has (a feature that made them was
    /// deleted or changed). Nothing is called lost while the part is still being built.
    fn lost(&self, faces: &[FaceRef], edges: &[EdgeRef]) -> usize {
        if self.shown.is_none() || self.is_busy() || self.scene.bodies.is_empty() {
            return 0;
        }
        let faces = faces.iter().filter(|r| find_face(&self.scene, r).is_none()).count();
        faces + edges.iter().filter(|r| find_edge(&self.scene, r).is_none()).count()
    }

    /// What a support or load that is opened to be changed is on: the faces and edges the part
    /// still has. The rest are left out, and the status bar says so (the last is true then).
    fn still_there(&mut self, name: &str, faces: &[FaceRef], edges: &[EdgeRef]) -> (Vec<FaceRef>, Vec<EdgeRef>, bool) {
        let lost = self.lost(faces, edges);
        if lost == 0 {
            return (faces.to_vec(), edges.to_vec(), false);
        }
        let (what, of) = if edges.is_empty() { ("face", faces.len()) } else { ("edge", edges.len()) };
        let which = if of == 1 { format!("its {what} is") } else { format!("{lost} of its {of} {what}s are") };
        self.set_error(format!("{name}: {which} no longer on the part, and left out. Click its {what}s again, then OK."));
        let faces = faces.iter().filter(|r| find_face(&self.scene, r).is_some()).cloned().collect();
        let edges = edges.iter().filter(|r| find_edge(&self.scene, r).is_some()).cloned().collect();
        (faces, edges, true)
    }

    /// Fixed, Pin or Frictionless: a new support, or (with `editing`) one of the study's
    /// changed.
    pub(crate) fn open_support(&mut self, kind: SupportKind, editing: Option<u32>) -> Result<(), String> {
        let study = self.study_for_new()?;
        let mut told = false;
        let mut panel = match editing {
            Some(id) => {
                let s = self.active_study().and_then(|s| s.supports.iter().find(|x| x.id == id)).cloned().ok_or("no such support")?;
                let (faces, edges, lost) = self.still_there(&s.name, &s.faces, &s.edges);
                told = lost;
                SupportPanel { study, editing, kind: s.kind, faces, edges }
            }
            None => {
                // (Of what was selected beforehand: its faces, or for a fixed support with no
                // face among them its edges. A support is on the one or on the other.)
                let faces = self.picked_faces();
                let edges = if kind == SupportKind::Fixed && faces.is_empty() { self.picked_edges() } else { Vec::new() };
                SupportPanel { study, editing, kind, faces, edges }
            }
        };
        // (And of those faces, the ones this kind of support can hold.)
        panel.faces.retain(|f| panel.kind.takes(&f.fingerprint.surface).is_ok());
        self.view.selection.clear();
        if !told {
            self.set_status(format!("{}: click {}, then OK.", panel.title(), panel.faces_are()));
        }
        self.panel = Some(Panel::Support(panel));
        self.start_equations();
        Ok(())
    }

    /// A new load, or (with `editing`) one of the study's changed.
    pub(crate) fn open_load(&mut self, what: What, editing: Option<u32>) -> Result<(), String> {
        let study = self.study_for_new()?;
        let blank = |what, faces, magnitude| LoadPanel {
            study,
            editing,
            what,
            faces,
            edges: Vec::new(),
            magnitude,
            way: Way::Normal,
            flip: false,
            picking: false,
            components: None,
            point: Vec3::ZERO,
            picking_point: false,
        };
        // (Down: against Z.)
        let down = |p: LoadPanel| LoadPanel { way: Way::Z, flip: true, ..p };
        let mut told = false;
        let mut panel = match editing {
            Some(id) => {
                let load = self.active_study().and_then(|s| s.loads.iter().find(|l| l.id == id)).cloned().ok_or("no such load")?;
                let (faces, edges, lost) = self.still_there(&load.name, &load.faces, &load.edges);
                told = lost;
                // A size with its direction, as the panel says it: never negative, the other
                // way in its place.
                let along = |what, direction: Vec3, size: f64, faces| {
                    let (way, flip, size) = Way::of_signed(direction, size);
                    LoadPanel { way, flip, ..blank(what, faces, size) }
                };
                match load.kind {
                    LoadKind::Pressure { megapascals } => blank(What::Pressure, faces, megapascals),
                    // (Square to its faces, a negative force pulls: shown as flipped.)
                    LoadKind::Force { newtons, direction: None } => LoadPanel { flip: newtons < 0.0, ..blank(What::Force, faces, newtons.abs()) },
                    LoadKind::Force { newtons, direction: Some(d) } => LoadPanel { edges, ..along(What::Force, d, newtons, faces) },
                    LoadKind::Gravity { direction, acceleration } => along(What::Gravity, direction, acceleration, Vec::new()),
                    LoadKind::Moment { newton_millimetres, axis: None } => {
                        LoadPanel { flip: newton_millimetres < 0.0, ..blank(What::Moment, faces, newton_millimetres.abs()) }
                    }
                    LoadKind::Moment { newton_millimetres, axis: Some(axis) } => along(What::Moment, axis, newton_millimetres, faces),
                    LoadKind::Bearing { newtons, direction } => along(What::Bearing, direction, newtons, faces),
                    LoadKind::RemoteForce { newtons, direction, point } => LoadPanel { point, ..along(What::Remote, direction, newtons, faces) },
                    LoadKind::Body { axis, point, rpm } => LoadPanel { point, ..along(What::Body, axis, rpm, Vec::new()) },
                }
            }
            None => {
                let faces = self.picked_faces();
                match what {
                    What::Force => {
                        // (Along edges it has no face to be square to: down, until it is said.)
                        let edges = if faces.is_empty() { self.picked_edges() } else { Vec::new() };
                        let panel = blank(what, faces, 100.0);
                        if edges.is_empty() { panel } else { down(LoadPanel { edges, ..panel }) }
                    }
                    What::Pressure => blank(what, faces, 1.0),
                    // (1 N m, about its faces' own axis.)
                    What::Moment => blank(what, faces, 1000.0),
                    What::Bearing => down(blank(what, faces, 100.0)),
                    What::Remote => down(blank(what, faces, 100.0)),
                    What::Gravity | What::Body => {
                        // (Each is on all of the part, and there once.)
                        if let Some(there) = self.active_study().and_then(|s| s.loads.iter().find(|l| What::of(&l.kind) == what)) {
                            let it = there.kind.whole_part().unwrap_or("that load");
                            return Err(format!("The study has {it} already ({}): double-click it to change it.", there.name));
                        }
                        if what == What::Gravity {
                            down(blank(what, Vec::new(), EARTH))
                        } else {
                            LoadPanel { way: Way::Z, ..blank(what, Vec::new(), 1000.0) }
                        }
                    }
                }
            }
        };
        // (Of the faces that were selected beforehand, those this kind of load can be on.)
        let kind = panel.shown();
        panel.faces.retain(|f| kind.takes(&f.fingerprint.surface).is_ok());
        self.view.selection.clear();
        if !told {
            self.set_status(match panel.what {
                What::Pressure => "Pressure: click the faces it is on, set how large it is, then OK.",
                What::Force => "Force: click the faces it is on (or the edges it is along), set how large it is and which way it pushes, then OK.",
                What::Gravity => "Gravity: the part's own weight, from its material's density. Set which way it pulls, then OK.",
                What::Moment => "Moment: click the faces it turns, set how large it is and the axis it turns about, then OK.",
                What::Bearing => "Bearing Load: click the round faces the shaft presses on, set its load and which way it presses, then OK.",
                What::Remote => "Remote Force: click the faces that carry it, set the force, which way it pushes and where it acts, then OK.",
                What::Body => "Body: the part turning, flung outward by its own mass. Set how fast, and the axis it turns about, then OK.",
            });
        }
        self.panel = Some(Panel::Load(panel));
        self.load_picked();
        self.start_equations();
        Ok(())
    }

    /// After the open load panel was given something to be on: a force along edges has no face
    /// to be square to, and a bearing load presses across its bore. Either is given a
    /// direction it can have, and the status bar says so.
    pub(crate) fn load_picked(&mut self) {
        let Some(Panel::Load(p)) = &self.panel else { return };
        let turned = match p.what {
            What::Force if !p.edges.is_empty() && p.way == Way::Normal && p.components.is_none() => {
                Some((Way::Z, true, "Force: along edges it has no face to be square to. It pushes down (-Z) until its direction is set."))
            }
            What::Bearing => {
                let axis = p.faces.first().and_then(|r| self.face_axis(r)).map(Vec3::normalized);
                match (axis, p.way.vector(p.flip)) {
                    // (Down, unless the bore itself stands up.)
                    (Some(a), Some(d)) if a.dot(d).abs() > 0.999 && a.z.abs() > 0.5 => {
                        Some((Way::X, false, "Bearing Load: it presses across its bore: along X until its direction is set."))
                    }
                    (Some(a), Some(d)) if a.dot(d).abs() > 0.999 => {
                        Some((Way::Z, true, "Bearing Load: it presses across its bore: down (-Z) until its direction is set."))
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        let Some((way, flip, said)) = turned else { return };
        if let Some(Panel::Load(p)) = &mut self.panel {
            (p.way, p.flip) = (way, flip);
        }
        self.set_status(said);
    }

    /// OK on a support. Returns true when it was made or changed.
    pub(crate) fn commit_support(&mut self, p: &SupportPanel) -> bool {
        if p.faces.is_empty() && p.edges.is_empty() {
            let or_edge = if p.kind == SupportKind::Fixed { " or edge" } else { "" };
            self.set_error(format!("{}: click at least one face{or_edge}", p.title()));
            return false;
        }
        let (id, params) = p.command();
        self.exec_status(id, params).is_some()
    }

    /// OK on a load. Returns true when it was made or changed.
    pub(crate) fn commit_load(&mut self, p: &LoadPanel) -> bool {
        if p.what.on_faces() && p.faces.is_empty() && p.edges.is_empty() {
            let or_edge = if p.what == What::Force { " or edge" } else { "" };
            self.set_error(format!("{}: click at least one face{or_edge}", p.title()));
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

    /// Whether the open load panel's point selector is the one that takes clicks.
    pub(crate) fn picking_load_point(&self) -> bool {
        matches!(&self.panel, Some(Panel::Load(p)) if p.picking_point && matches!(p.what, What::Remote | What::Body))
    }

    /// Gives the open load panel its point (from a click on a face or an edge).
    /// Returns false when no load panel is waiting for one.
    pub(crate) fn set_load_point(&mut self, at: Vec3) -> bool {
        if !self.picking_load_point() {
            return false;
        }
        if let Some(Panel::Load(p)) = &mut self.panel {
            // (To the thousandth of a micron: the scene's points are single precision.)
            let round = |v: f64| (v * 1.0e6).round() / 1.0e6 + 0.0;
            p.point = Vec3::new(round(at.x), round(at.y), round(at.z));
            p.picking_point = false;
        }
        true
    }

    /// The direction a click on that edge or face gives: along a straight edge, or square to a
    /// flat face and into it. For a load that turns it is an axis: the way a flat face looks,
    /// or the axis of a round one.
    pub(crate) fn direction_of(&self, pick: Pick) -> Option<Vec3> {
        let turns = matches!(&self.panel, Some(Panel::Load(p)) if p.what.turns());
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
            Pick::Face { body, face } => match &self.scene.bodies.get(body)?.faces.get(face as usize)?.1.surface {
                SurfaceKind::Plane { normal, .. } => Some(if turns { *normal } else { -*normal }),
                SurfaceKind::Cylinder { axis, .. } | SurfaceKind::Cone { axis, .. } if turns => Some(axis.dir()),
                _ => None,
            },
        }
    }

    /// A point on the line a click on that edge or face gives as an axis: on the edge, on a
    /// round face's axis, in the middle of a flat face.
    pub(crate) fn axis_point_of(&self, pick: Pick) -> Option<Vec3> {
        match pick {
            Pick::Edge { body, edge } => self.scene.bodies.get(body)?.edges.get(edge as usize).map(|(_, fp)| fp.mid),
            Pick::Face { body, face } => {
                let info = &self.scene.bodies.get(body)?.faces.get(face as usize)?.1;
                Some(match &info.surface {
                    SurfaceKind::Cylinder { axis, .. } | SurfaceKind::Cone { axis, .. } => axis.origin(),
                    _ => info.centroid,
                })
            }
        }
    }

    /// Gives the open load panel its direction (from a click on an edge, a face or a work
    /// axis). `through`: a point on what was clicked, which the axis of a part's turning goes
    /// through. Returns false when no load panel is waiting for a direction.
    pub(crate) fn set_load_direction(&mut self, d: Vec3, through: Option<Vec3>) -> bool {
        if !self.picking_direction() || d.len() <= 0.0 {
            return false;
        }
        if let Some(Panel::Load(p)) = &mut self.panel {
            (p.way, p.flip) = Way::of(Some(d));
            p.picking = false;
            if let (What::Body, Some(at)) = (p.what, through) {
                // (To the thousandth of a micron: the scene's points are single precision.)
                let round = |v: f64| (v * 1.0e6).round() / 1.0e6 + 0.0;
                p.point = Vec3::new(round(at.x), round(at.y), round(at.z));
            }
        }
        true
    }

    /// Work axis `id` as the part has it now: its direction, and a point on it.
    pub(crate) fn work_axis_line(&self, id: tenon_model::FeatureId) -> Option<(Vec3, Vec3)> {
        self.scene.work.iter().find(|(w, _)| *w == id).and_then(|(_, g)| match g {
            WorkGeom::Axis(a) => Some((a.dir(), a.origin())),
            _ => None,
        })
    }

    /// The faces (or edges) of the support or load clicked in the browser, picked out as
    /// selected. (The others are shown by their marks alone: a part with its faces all
    /// coloured in hides the marks, and the part.)
    pub(crate) fn analysis_highlights(&self) -> Vec<(Pick, Color32)> {
        let (Some(study), Some(selected)) = (self.active_study(), self.analysis.as_ref().and_then(|e| e.selected)) else { return Vec::new() };
        let face = |r: &FaceRef| find_face(&self.scene, r).map(|(body, face)| Pick::Face { body, face });
        let edge = |r: &EdgeRef| find_edge(&self.scene, r).map(|(body, edge)| Pick::Edge { body, edge });
        let on = study.supports.iter().map(|s| (s.id, &s.faces, &s.edges)).chain(study.loads.iter().map(|l| (l.id, &l.faces, &l.edges)));
        on.filter(|(id, ..)| *id == selected)
            .flat_map(|(_, faces, edges)| faces.iter().filter_map(face).chain(edges.iter().filter_map(edge)))
            .map(|pick| (pick, crate::viewport::SELECTED))
            .collect()
    }

    /// Where an edge is: the middle of it.
    fn edge_at(&self, r: &EdgeRef) -> Option<Vec3> {
        let (body, edge) = find_edge(&self.scene, r)?;
        self.scene.bodies.get(body)?.edges.get(edge as usize).map(|(_, fp)| fp.mid)
    }

    /// A face's own axis: the way a flat face looks, the axis of a round one.
    fn face_axis(&self, r: &FaceRef) -> Option<Vec3> {
        let (body, face) = find_face(&self.scene, r)?;
        match &self.scene.bodies.get(body)?.faces.get(face as usize)?.1.surface {
            SurfaceKind::Plane { normal, .. } => Some(*normal),
            SurfaceKind::Cylinder { axis, .. } => Some(axis.dir()),
            _ => None,
        }
    }

    /// Where a face is and which way it looks (None for a face that is not flat).
    fn face_at(&self, r: &FaceRef) -> Option<(Vec3, Option<Vec3>)> {
        let (body, face) = find_face(&self.scene, r)?;
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
        // A load that turns: an arc round its axis at `at`, three quarters of the way round
        // and anticlockwise seen from the axis's tip, with its head where it ends, and the
        // axis itself `reach` long each way.
        let turn = |at: Vec3, axis: Vec3, radius: f64, axis_reach: f64, color: Color32, label: Option<&str>| {
            let Some(c) = screen(at) else { return };
            mark("load", c);
            if axis.len() <= 0.0 {
                return;
            }
            let k = axis.normalized();
            // (Two directions square to the axis and to each other.)
            let u = if k.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
            let u = (u - k * u.dot(k)).normalized();
            let v = k.cross(u);
            const STEPS: u32 = 30;
            let round: Vec<Pos2> = (0..=STEPS)
                .filter_map(|i| {
                    let a = f64::from(i) / f64::from(STEPS) * 1.5 * std::f64::consts::PI;
                    screen(at + (u * a.cos() + v * a.sin()) * radius)
                })
                .collect();
            if round.len() < 4 {
                return;
            }
            let (tip, before) = (round[round.len() - 1], round[round.len() - 3]);
            for (width, color) in [(5.0, edge), (2.5, color)] {
                if let (Some(a), Some(b)) = (screen(at - k * axis_reach), screen(at + k * axis_reach)) {
                    painter.line_segment([a, b], Stroke::new(width - 1.0, color));
                }
                painter.add(egui::Shape::line(round[..round.len() - 2].to_vec(), Stroke::new(width, color)));
                let d = tip - before;
                if d.length() > 0.5 {
                    let along = d / d.length();
                    let across = vec2(-along.y, along.x);
                    let grow = (width - 2.5) * 0.6;
                    let barbs = vec![
                        tip + along * grow,
                        tip - along * (12.0 + grow) + across * (5.5 + grow),
                        tip - along * (12.0 + grow) - across * (5.5 + grow),
                    ];
                    painter.add(egui::Shape::convex_polygon(barbs, color, Stroke::NONE));
                }
            }
            if let Some(text) = label {
                // (Above the arc; or, where the axis is drawn long, at its tip: clear of what
                // is on the part.)
                let top = round.iter().map(|p| p.y).fold(c.y, f32::min);
                let at =
                    screen(at + k * axis_reach).filter(|_| axis_reach > radius).map_or(egui::pos2(c.x, top - 12.0), |tip| tip + vec2(0.0, -12.0));
                label_at(at, Align2::CENTER_CENTER, text);
            }
        };
        // Something that is on a face and has no one way to point there: marked where it is.
        let spot = |c: Pos2, color: Color32, label: Option<&str>| {
            mark("load", c);
            ring(c, color);
            if let Some(text) = label {
                label_at(c + vec2(0.0, -16.0), Align2::CENTER_CENTER, text);
            }
        };
        let load = |faces: &[FaceRef], edges: &[EdgeRef], kind: &LoadKind, color: Color32| {
            let text = load_text(kind);
            // (The other way for a size that is negative.)
            let signed = |d: Vec3, size: f64| if size < 0.0 { -d } else { d };
            match kind {
                LoadKind::Gravity { direction, .. } => {
                    // The part's weight: an arrow from its centre of gravity, the way it pulls.
                    if let (Some(at), len) = (self.weight_at(), direction.len())
                        && len > 0.0
                    {
                        let d = *direction * (1.0 / len);
                        arrow(at + d * (reach * 2.0), d, color, Some(&format!("g {text}")), false, true);
                    }
                    return;
                }
                LoadKind::Body { axis, point, .. } => {
                    // The part's turning: round its axis, beside the part's centre of gravity.
                    if let Some(centre) = self.weight_at()
                        && axis.len() > 0.0
                    {
                        let k = axis.normalized();
                        turn(*point + k * (centre - *point).dot(k), k, reach * 3.0, reach * 7.0, color, Some(&text));
                    }
                    return;
                }
                LoadKind::RemoteForce { newtons, direction, point } => {
                    // A force at a point away from the part, and a line to each face that
                    // carries it.
                    if let Some(from) = screen(*point) {
                        for (at, _) in faces.iter().filter_map(|r| self.face_at(r)) {
                            let Some(c) = screen(at) else { continue };
                            mark("load", c);
                            for (width, color) in [(3.5, edge), (1.5, color)] {
                                painter.line_segment([from, c], Stroke::new(width, color));
                                painter.circle_filled(c, width + 1.5, color);
                            }
                        }
                    }
                    arrow(*point, signed(*direction, *newtons), color, Some(&text), false, false);
                    return;
                }
                _ => {}
            }
            // A force along edges: an arrow at the middle of each.
            if let LoadKind::Force { direction: Some(d), newtons } = kind {
                for (i, at) in edges.iter().filter_map(|r| self.edge_at(r)).enumerate() {
                    arrow(at, signed(*d, *newtons), color, (i == 0).then_some(text.as_str()), false, false);
                }
            }
            // (A moment about its faces' own axis turns about its first face's, as the solver has it.)
            let own = faces.first().and_then(|r| self.face_axis(r));
            for (i, r) in faces.iter().enumerate() {
                let Some((at, normal)) = self.face_at(r) else { continue };
                let label = (i == 0).then_some(text.as_str());
                let dir = match kind {
                    LoadKind::Force { direction: Some(d), newtons } | LoadKind::Bearing { direction: d, newtons } => Some(signed(*d, *newtons)),
                    LoadKind::Force { direction: None, newtons } => normal.map(|n| signed(-n, *newtons)),
                    LoadKind::Pressure { megapascals } => normal.map(|n| signed(-n, *megapascals)),
                    LoadKind::Moment { newton_millimetres, axis } => {
                        match axis.or(own) {
                            Some(k) => turn(at, signed(k, *newton_millimetres), reach * 1.6, reach * 0.8, color, label),
                            None => {
                                if let Some(c) = screen(at) {
                                    spot(c, color, label);
                                }
                            }
                        }
                        continue;
                    }
                    LoadKind::Gravity { .. } | LoadKind::RemoteForce { .. } | LoadKind::Body { .. } => None,
                };
                match dir {
                    Some(d) => arrow(at, d, color, label, matches!(kind, LoadKind::Pressure { .. }), false),
                    // A round face is pushed all over, each way its own: marked where it is.
                    None => {
                        if let Some(c) = screen(at) {
                            spot(c, color, label);
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
        // (A support on edges is marked at the middle of each.)
        let hold = |faces: &[FaceRef], edges: &[EdgeRef], kind: SupportKind, color: Color32| {
            let faces = faces.iter().filter_map(|r| self.face_at(r)).map(|(at, _)| at);
            for at in faces.chain(edges.iter().filter_map(|r| self.edge_at(r))) {
                held(at, kind, color);
            }
        };
        let hidden = self.analysis.as_ref().is_some_and(|e| e.hide_marks);
        if let Some(study) = self.active_study().filter(|_| !hidden) {
            // (What is suppressed is not there for now, and is not drawn.)
            for s in study.supports.iter().filter(|s| Some(s.id) != editing && !s.suppressed) {
                hold(&s.faces, &s.edges, s.kind, HELD);
            }
            for l in study.loads.iter().filter(|l| Some(l.id) != editing && !l.suppressed) {
                load(&l.faces, &l.edges, &l.kind, LOADED);
            }
        }
        // What is being set up, as it will be.
        match &self.panel {
            Some(Panel::Support(p)) => hold(&p.faces, &p.edges, p.kind, t.accent),
            Some(Panel::Load(p)) => load(&p.faces, &p.edges, &p.shown(), t.accent),
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
        // (How many faces or edges of each support and load of the study worked on the part no
        // longer has.)
        let lost: std::collections::BTreeMap<u32, usize> = studies
            .iter()
            .filter(|s| Some(s.id) == active)
            .flat_map(|s| s.supports.iter().map(|x| (x.id, &x.faces, &x.edges)).chain(s.loads.iter().map(|l| (l.id, &l.faces, &l.edges))))
            .map(|(id, faces, edges)| (id, self.lost(faces, edges)))
            .collect();
        let lost = |id: u32| lost.get(&id).copied().unwrap_or(0);
        // What there is to show of the study worked on: its mesh's size, and its results in words.
        use crate::results::{Field, figures};
        let shown: Option<(usize, usize, Option<Vec<(Field, String)>>)> = self.outcome().and_then(|o| {
            let mesh = o.mesh()?;
            let results = o.solved.as_ref().map(|s| {
                let factor = s.safety_factor().map_or("none: nothing works it".to_owned(), figures);
                vec![
                    (Field::VonMises, format!("Von Mises Stress: {} MPa", figures(s.results.max_von_mises.value))),
                    (Field::Displacement, format!("Displacement: {} mm", figures(s.results.max_displacement.value))),
                    (Field::Safety, format!("Safety Factor: {factor}")),
                ]
            });
            Some((mesh.nodes.len(), mesh.tets.len(), results))
        });
        let (field, stale) = (self.analysis.as_ref().map(|e| e.field).unwrap_or_default(), self.outcome_is_stale());
        // (What is being worked out of the study worked on: Some(true) its answer, Some(false)
        // its mesh alone.)
        let running: Option<bool> = self.analysis.as_ref().and_then(|e| e.running.as_ref()).filter(|r| Some(r.study) == active).map(|r| r.solving);
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
                // (What a support or load is on: faces, or edges.)
                let gone = |id: u32, faces: usize, edges: usize| lost_note(lost(id), faces + edges, if edges > 0 { "edge" } else { "face" });
                for s in &study.supports {
                    let label = format!("{}{}{}", s.name, gone(s.id, s.faces.len(), s.edges.len()), quiet(s.suppressed));
                    let (resp, _) = row(ui, t, 3, support_icon(s.kind), &label, None, style(s.id, s.suppressed));
                    item_row(resp, study.id, s.id, &s.name, s.suppressed, lost(s.id) > 0, &mut action);
                }
                let unloaded = study.loads.iter().all(|l| l.suppressed);
                let (resp, _) = row(ui, t, 2, Icon::Folder, &format!("Loads({})", study.loads.len()), None, needed(unloaded));
                if unloaded {
                    resp.on_hover_text(
                        "The study needs a load (a force, a pressure, the part's weight...): with nothing pushing on it there is no stress to find",
                    );
                }
                for l in &study.loads {
                    let label = format!("{} ({}){}{}", l.name, load_text(&l.kind), gone(l.id, l.faces.len(), l.edges.len()), quiet(l.suppressed));
                    let (resp, _) = row(ui, t, 3, load_icon(&l.kind), &label, None, style(l.id, l.suppressed));
                    item_row(resp, study.id, l.id, &l.name, l.suppressed, lost(l.id) > 0, &mut action);
                }
                let mesh = match (&shown, study.mesh_size) {
                    _ if running == Some(false) => "Mesh: being made...".to_owned(),
                    (Some((_, elements, _)), _) => format!("Mesh: {elements} elements"),
                    (None, Some(size)) => format!("Mesh: {size} mm"),
                    (None, None) => "Mesh: sized from the part".to_owned(),
                };
                let (resp, _) = row(ui, t, 2, Icon::MeshGrid, &mesh, None, RowStyle::Normal);
                let about = match &shown {
                    Some((nodes, elements, _)) => {
                        format!("{elements} ten-node tetrahedra with {nodes} nodes. Double-click to set the size of the elements")
                    }
                    None => "Double-click to set the size of the elements".to_owned(),
                };
                if resp.on_hover_text(about).double_clicked() {
                    action = Some(StudyAction::Run("study.mesh_size"));
                }
                // The results: each a row that shows it in colour on the part when clicked.
                match &shown {
                    Some((_, _, Some(results))) => {
                        row(ui, t, 2, Icon::Folder, "Results", None, RowStyle::Normal);
                        for (f, text) in results {
                            let (resp, _) = row(ui, t, 3, Icon::Stress, text, None, if *f == field { RowStyle::Active } else { RowStyle::Normal });
                            if resp.on_hover_text("Click to show it in colour on the part").clicked() {
                                action = Some(StudyAction::Show(*f));
                            }
                        }
                    }
                    _ => {
                        let (text, tip) = if running == Some(true) {
                            ("Results: being worked out...", "Esc stops it")
                        } else if stale {
                            ("Results: out of date (Simulate)", "The part or the study has changed since it was worked out")
                        } else {
                            ("Results: none yet (Simulate)", "Simulate works the study out")
                        };
                        let (resp, _) = row(ui, t, 2, Icon::Simulate, text, None, RowStyle::Dim);
                        if resp.on_hover_text(tip).double_clicked() {
                            action = Some(StudyAction::Run("analysis.simulate"));
                        }
                    }
                }
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
            Some(StudyAction::Show(field)) => {
                if let Some(env) = &mut self.analysis {
                    env.field = field;
                }
                Ok(())
            }
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
        let what = study.loads.iter().find(|l| l.id == id).map(|l| What::of(&l.kind));
        match what {
            Some(what) => self.open_load(what, Some(id)),
            None => Err("no such support or load".into()),
        }
    }
}

/// A support's or a load's row: a click picks its faces (or edges) out, a double click opens
/// it, the menu edits, renames, suppresses or deletes it.
fn item_row(resp: egui::Response, study: StudyId, id: u32, name: &str, suppressed: bool, lost: bool, action: &mut Option<StudyAction>) {
    let resp = resp.on_hover_text(if lost {
        "It is on a face or an edge the part no longer has (a feature that made it was deleted or changed): double-click it and click them again"
    } else {
        "Click to see what it is on; double-click to change it; right-click for more"
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
