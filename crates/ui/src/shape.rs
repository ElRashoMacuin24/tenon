//! Optimize Shape in the Stress Analysis environment (DEC-123): its settings, the faces it keeps
//! and its planes of symmetry; the run, with where it is and Esc to stop it; the shape drawn
//! over the part, with the share kept on a slider; and the shape written out.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui::{Color32, Pos2, Rect, Ui, pos2, vec2};
use serde_json::{Value, json};
use tenon_fea::Cancel;
use tenon_fea::topo::{Extracted, Phase, Progress, Report};
use tenon_io::shape::{SMOOTHING, Shaped};
use tenon_model::study::{Optimize, Resolution, StudyId, Target};
use tenon_model::{Document, FaceRef, FeatureId, FeatureKind, OriginPlane, PlaneRef, Session, WorkGeom};

use crate::panels::Panel;
use crate::print_dialog::PartJob;
use crate::properties::{eq_field, section};
use crate::results::figures;
use crate::theme::{self, Tokens};
use crate::workbench::{Geo, Workbench};

/// The colour of kept faces' marks, and of the shape.
pub(crate) const KEPT: Color32 = Color32::from_rgb(0x3a, 0xa8, 0x6a);
const SHAPE: [f32; 3] = [0.82, 0.62, 0.30];

/// Optimize Shape's settings, as its panel has them.
#[derive(Clone, Debug)]
pub(crate) struct ShapePanel {
    pub study: StudyId,
    /// The target as a mass (g), not as a share taken away (%).
    pub by_grams: bool,
    pub remove: f64,
    pub grams: f64,
    /// The thinnest member left to the element size.
    pub auto_member: bool,
    pub member: f64,
    pub resolution: Resolution,
    /// A size of element the study holds (from a command or a file), in place of the
    /// resolution's. Picking a resolution clears it.
    pub element_size: Option<f64>,
    /// The most rounds the study holds (from a command or a file).
    pub iterations: Option<u32>,
    pub sharpen: bool,
    /// What the part weighs, g, to say what is kept.
    pub part_grams: f64,
}

impl ShapePanel {
    fn of(study: StudyId, o: &Optimize, part_grams: f64) -> ShapePanel {
        let (by_grams, remove, grams) = match o.target {
            Target::Remove(r) => (false, r, part_grams * (1.0 - r / 100.0)),
            // (Of a part not built yet, its mass says nothing of the share: see `part_is`.)
            Target::Grams(g) if part_grams > 0.0 => (true, 100.0 * (1.0 - g / part_grams), g),
            Target::Grams(g) => (true, 50.0, g),
        };
        ShapePanel {
            study,
            by_grams,
            remove,
            grams,
            auto_member: o.min_member.is_none(),
            member: o.min_member.unwrap_or(5.0),
            resolution: o.resolution,
            element_size: o.element_size,
            iterations: o.iterations,
            sharpen: o.sharpen,
            part_grams,
        }
    }

    /// The settings as `study.shape` takes them. (The size and the rounds are each said, as a
    /// number or as null: OK keeps what the panel shows.)
    pub(crate) fn command(&self) -> Value {
        let mut p = json!({
            "study": self.study.0,
            "min_member": if self.auto_member { Value::Null } else { json!(self.member) },
            "resolution": self.resolution.word(),
            "element_size": self.element_size,
            "iterations": self.iterations,
            "sharpen": self.sharpen,
        });
        if self.by_grams {
            p["grams"] = json!(self.grams);
        } else {
            p["remove"] = json!(self.remove);
        }
        p
    }

    /// The part weighs `part_grams`, as the scene now has it: it may not have when this was opened.
    /// What goes by it follows, and what was asked does not.
    pub(crate) fn part_is(&mut self, part_grams: f64) {
        if part_grams == self.part_grams {
            return;
        }
        self.part_grams = part_grams;
        if part_grams > 0.0 {
            if self.by_grams {
                self.remove = 100.0 * (1.0 - self.grams / part_grams);
            } else {
                self.grams = part_grams * (1.0 - self.remove / 100.0);
            }
        }
    }

    /// About how much is kept, g.
    fn kept_grams(&self) -> f64 {
        if self.by_grams { self.grams } else { self.part_grams * (1.0 - self.remove / 100.0) }
    }
}

/// Keep Faces: faces Optimize Shape keeps solid, and how deep.
#[derive(Clone, Debug)]
pub(crate) struct KeepPanel {
    pub study: StudyId,
    /// The kept faces being changed; None for new ones.
    pub editing: Option<u32>,
    pub faces: Vec<FaceRef>,
    /// Material kept to a depth under them (else one layer of elements).
    pub deep: bool,
    pub depth: f64,
}

impl KeepPanel {
    pub(crate) fn command(&self) -> (&'static str, Value) {
        let mut p = json!({ "study": self.study.0, "faces": self.faces });
        match self.editing {
            Some(item) => {
                p["item"] = json!(item);
                p["depth"] = if self.deep { json!(self.depth) } else { Value::Null };
                ("study.set", p)
            }
            None => {
                if self.deep {
                    p["depth"] = json!(self.depth);
                }
                ("study.keep", p)
            }
        }
    }
}

/// Symmetry: the planes the shape is to be the same on both sides of: origin planes and work
/// planes, each ticked. (A face of the part cannot be one: a solid has material on one side of
/// its own face and none on the other.)
#[derive(Clone, Debug)]
pub(crate) struct SymmetryPanel {
    pub study: StudyId,
    /// XY, YZ, XZ.
    pub origin: [bool; 3],
    /// The part's work planes: which, what it is called, and whether it is ticked.
    pub work: Vec<(FeatureId, String, bool)>,
    /// What it holds that cannot be a plane of symmetry (a work plane that is gone, a face from
    /// a file): what it is called, and whether it is still ticked. It goes when it is unticked.
    pub lost: Vec<(PlaneRef, String, bool)>,
}

const ORIGINS: [OriginPlane; 3] = [OriginPlane::XY, OriginPlane::YZ, OriginPlane::XZ];

impl SymmetryPanel {
    pub(crate) fn planes(&self) -> Vec<PlaneRef> {
        let origin = ORIGINS.iter().zip(self.origin).filter(|(_, on)| *on).map(|(o, _)| PlaneRef::Origin(*o));
        let work = self.work.iter().filter(|w| w.2).map(|w| PlaneRef::Work(w.0));
        origin.chain(work).chain(self.lost.iter().filter(|l| l.2).map(|l| l.0.clone())).collect()
    }
}

/// A shape that came back, as it is shown.
#[derive(Clone, Debug)]
pub(crate) struct ShapeView {
    pub study: StudyId,
    /// What it hangs on (`basis_of`), as it was when it was worked out.
    pub basis: u64,
    pub shaped: Arc<Shaped>,
    /// The surface drawn: for the share asked, or another on the slider.
    pub shown: Extracted,
    /// The share that surface was made for, percent.
    pub shown_percent: f64,
    /// The share on the slider, percent.
    pub percent: f64,
    pub visible: bool,
    /// The surface drawn is closed.
    pub closed: bool,
}

impl ShapeView {
    /// What there is to know of the shape, beside how it ended: the run's warnings, and in place of
    /// what they say of its surface, which is of the share it was worked out for, the same of the
    /// surface drawn: whether it is closed, the pieces it leaves out, whether it joins the loads.
    pub(crate) fn said(&self) -> Vec<String> {
        let of_the_run = tenon_io::shape::said_of_surface(false, &self.shaped.shown);
        let run = self.shaped.warnings.iter().filter(|w| !of_the_run.contains(w)).cloned();
        run.chain(tenon_io::shape::said_of_surface(self.closed, &self.shown)).collect()
    }
}

/// What a shape of `study` hangs on, as a number that moves when that does: the part and that
/// study. Not what they are called, nor the other studies there are, so that naming the part does
/// not throw away a run of up to an hour.
pub(crate) fn basis_of(doc: &Document, study: StudyId) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut v = serde_json::to_value(doc).unwrap_or(Value::Null);
    if let Some(d) = v.as_object_mut() {
        for key in ["name", "part_number", "description"] {
            d.remove(key);
        }
        if let Some(Value::Array(studies)) = d.get_mut("studies") {
            studies.retain(|s| s["id"] == study.0);
            for s in studies.iter_mut().filter_map(Value::as_object_mut) {
                s.remove("name");
            }
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.to_string().hash(&mut h);
    h.finish()
}

/// A run of Optimize Shape: where it is, and how to stop it.
#[derive(Clone, Debug)]
pub(crate) struct Shaping {
    /// Which run it is: what comes back is taken only by the run that waits for it.
    pub run: u64,
    pub study: StudyId,
    /// What it hangs on (`basis_of`): once that moves on, the run is for nothing.
    pub basis: u64,
    pub cancel: Cancel,
    pub since: Instant,
    pub progress: Progress,
}

/// A value the study holds, with a button to clear it. True when that was pressed.
fn held_value(ui: &mut Ui, value: &str, key: &str) -> bool {
    ui.horizontal(|ui| {
        ui.label(value);
        let r = ui.small_button("✖").on_hover_text("Clear");
        crate::drawing::remember(ui, key, r.rect);
        r.clicked()
    })
    .inner
}

/// The fields of Optimize Shape's panel. Returns Enter was pressed.
pub(crate) fn shape_fields(ui: &mut Ui, p: &mut ShapePanel, kept: usize, symmetry: usize, t: &Tokens) -> bool {
    let mut enter = false;
    section(ui, "Target", true, |ui| {
        egui::Grid::new("tn_props_shape_target").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Keep");
            ui.horizontal(|ui| {
                for (grams, name, tip) in [(false, "Remove %", "Take this share of the mass away"), (true, "Mass g", "Come to this mass")] {
                    let r = ui.selectable_label(p.by_grams == grams, name).on_hover_text(tip);
                    crate::drawing::remember(ui, &format!("tn_shape_by_{}", if grams { "grams" } else { "remove" }), r.rect);
                    if r.clicked() {
                        p.by_grams = grams;
                    }
                }
            });
            ui.end_row();
            if p.by_grams {
                ui.label("Mass");
                enter |= eq_field(ui, egui::Id::new("tn_props_shape_grams"), &mut p.grams, None, "g", 0.001..=1.0e9, 110.0, t).entered;
            } else {
                ui.label("Remove");
                enter |= eq_field(ui, egui::Id::new("tn_props_shape_remove"), &mut p.remove, None, "%", 5.0..=95.0, 110.0, t).entered;
            }
            ui.end_row();
            ui.label("");
            let said = if p.part_grams > 0.0 {
                format!("Part {} g, about {} g kept", figures(p.part_grams), figures(p.kept_grams()))
            } else {
                "The part is being built".to_owned()
            };
            ui.label(egui::RichText::new(said).color(t.text_dim));
            ui.end_row();
        });
    });
    section(ui, "Settings", true, |ui| {
        egui::Grid::new("tn_props_shape_settings").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Smallest member").on_hover_text("No member of the shape is thinner than about this");
            ui.horizontal(|ui| {
                ui.checkbox(&mut p.auto_member, "Auto").on_hover_text("Three elements across");
                if !p.auto_member {
                    enter |= eq_field(ui, egui::Id::new("tn_props_shape_member"), &mut p.member, None, "mm", 0.001..=1.0e6, 80.0, t).entered;
                }
            });
            ui.end_row();
            ui.label("Resolution").on_hover_text("How finely the part is divided: finer is smoother, and slower");
            ui.horizontal(|ui| {
                for r in [Resolution::Coarse, Resolution::Medium, Resolution::Fine] {
                    let name = match r {
                        Resolution::Coarse => "Coarse",
                        Resolution::Medium => "Medium",
                        Resolution::Fine => "Fine",
                    };
                    let resp = ui.selectable_label(p.resolution == r, name);
                    crate::drawing::remember(ui, &format!("tn_shape_{}", r.word()), resp.rect);
                    if resp.clicked() {
                        p.resolution = r;
                        p.element_size = None;
                    }
                }
            });
            ui.end_row();
            // (Set by a command or a file: shown while they are, and cleared here.)
            if let Some(size) = p.element_size {
                ui.label("Element size").on_hover_text("The size of the elements the study holds, in place of the resolution's");
                if held_value(ui, &format!("{} mm", figures(size)), "tn_shape_clear_size") {
                    p.element_size = None;
                }
                ui.end_row();
            }
            if let Some(rounds) = p.iterations {
                ui.label("Most rounds").on_hover_text("It stops at this many rounds, settled or not");
                if held_value(ui, &rounds.to_string(), "tn_shape_clear_rounds") {
                    p.iterations = None;
                }
                ui.end_row();
            }
            ui.label("Sharpen");
            ui.checkbox(&mut p.sharpen, "Toward solid and empty").on_hover_text("Fewer half-filled places, in more rounds");
            ui.end_row();
            ui.label("Kept faces");
            ui.label(if kept == 0 { "none (Keep Faces)".to_owned() } else { format!("{kept} set{}", if kept == 1 { "" } else { "s" }) });
            ui.end_row();
            ui.label("Symmetry");
            ui.label(if symmetry == 0 { "none (Symmetry)".to_owned() } else { format!("{symmetry} plane{}", if symmetry == 1 { "" } else { "s" }) });
            ui.end_row();
        });
    });
    enter
}

/// The fields of Keep Faces. Returns (clear the faces, Enter was pressed).
pub(crate) fn keep_fields(ui: &mut Ui, p: &mut KeepPanel, t: &Tokens) -> (bool, bool) {
    let (mut clear, mut enter) = (false, false);
    section(ui, "Location", true, |ui| {
        egui::Grid::new("tn_props_keep").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Faces").on_hover_text("The faces the shape keeps: click them in the part");
            ui.horizontal(|ui| {
                let n = p.faces.len();
                ui.label(if n == 0 { "click faces".to_owned() } else { format!("{n} face{}", if n == 1 { "" } else { "s" }) });
                if n > 0 && ui.small_button("✖").on_hover_text("Clear").clicked() {
                    clear = true;
                }
            });
            ui.end_row();
            ui.label("Depth").on_hover_text("How deep under the faces material is kept");
            ui.horizontal(|ui| {
                let r = ui.checkbox(&mut p.deep, "").on_hover_text("Off: one layer of elements");
                crate::drawing::remember(ui, "tn_keep_deep", r.rect);
                if p.deep {
                    enter |= eq_field(ui, egui::Id::new("tn_props_keep_depth"), &mut p.depth, None, "mm", 0.001..=1.0e6, 80.0, t).entered;
                } else {
                    ui.label("one layer");
                }
            });
            ui.end_row();
        });
    });
    (clear, enter)
}

/// The fields of Symmetry.
pub(crate) fn symmetry_fields(ui: &mut Ui, p: &mut SymmetryPanel) {
    section(ui, "Planes", true, |ui| {
        egui::Grid::new("tn_props_symmetry").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Origin planes");
            ui.horizontal(|ui| {
                for (i, name) in ["XY", "YZ", "XZ"].iter().enumerate() {
                    let r = ui.checkbox(&mut p.origin[i], *name);
                    crate::drawing::remember(ui, &format!("tn_symmetry_{name}"), r.rect);
                }
            });
            ui.end_row();
            ui.label("Work planes")
                .on_hover_text("A part that is not centred on an origin plane is the same on both sides of a work plane through its middle");
            ui.vertical(|ui| {
                if p.work.is_empty() {
                    ui.label("none (Finish Analysis to make one)");
                }
                for (id, name, on) in &mut p.work {
                    let r = ui.checkbox(on, name.as_str());
                    crate::drawing::remember(ui, &format!("tn_symmetry_work_{}", id.0), r.rect);
                }
            });
            ui.end_row();
            if !p.lost.is_empty() {
                ui.label("Lost").on_hover_text("These cannot be planes of symmetry: untick them, then OK");
                ui.vertical(|ui| {
                    for (i, (_, name, on)) in p.lost.iter_mut().enumerate() {
                        let r = ui.checkbox(on, format!("{name} (lost)"));
                        crate::drawing::remember(ui, &format!("tn_symmetry_lost_{i}"), r.rect);
                    }
                });
                ui.end_row();
            }
        });
        ui.label(egui::RichText::new("Up to three, at right angles; the supports and loads should be the same on both sides too.").small());
    });
}

impl Workbench {
    /// The part's mass, g, as the scene has it.
    pub(crate) fn part_grams(&self) -> f64 {
        let volume: f64 = self.scene.bodies.iter().map(|b| b.volume).sum();
        volume * self.document().density() / 1000.0
    }

    /// The study's Optimize Shape settings (the usual ones until any are said).
    fn shape_settings(&self, study: StudyId) -> Optimize {
        self.document().study(study).and_then(|s| s.optimize.clone()).unwrap_or_default()
    }

    /// Optimize Shape: its settings, then OK works it out.
    pub(crate) fn open_shape(&mut self) -> Result<(), String> {
        let study = self.study_for_new()?;
        if self.shaping().is_some() {
            return Err("A shape is being worked out: wait for it, or press Esc to stop it.".into());
        }
        let p = ShapePanel::of(study, &self.shape_settings(study), self.part_grams());
        self.panel = Some(Panel::Shape(p));
        self.set_status("Optimize Shape: how much to keep, how fine; OK works it out (minutes: Esc stops it and keeps the shape so far).");
        Ok(())
    }

    /// Keep Faces: new, or the set `editing`. (Of a set being changed: the faces the part still
    /// has, as a support's are.)
    pub(crate) fn open_keep(&mut self, editing: Option<u32>) -> Result<(), String> {
        let study = self.study_for_new()?;
        let mut told = false;
        let panel = match editing {
            Some(id) => {
                let k = self.shape_settings(study).kept.into_iter().find(|k| k.id == id).ok_or("no such kept faces")?;
                let (faces, _, lost) = self.still_there(&k.name, &k.faces, &[]);
                told = lost;
                KeepPanel { study, editing, faces, deep: k.depth.is_some(), depth: k.depth.unwrap_or(3.0) }
            }
            None => KeepPanel { study, editing, faces: self.picked_faces(), deep: false, depth: 3.0 },
        };
        self.view.selection.clear();
        self.panel = Some(Panel::Keep(panel));
        if !told {
            self.set_status("Keep Faces: click the faces the shape must keep (bolt holes, mating faces), then OK.");
        }
        Ok(())
    }

    /// Whether work plane `id` is a plane on the part. (While the part is being built, whether
    /// the part says it is one.)
    fn work_plane_there(&self, id: FeatureId) -> bool {
        let said = self.document().feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::WorkPlane(_)) && !f.suppressed);
        let built = self.shown.is_some() && !self.is_busy() && !self.scene.bodies.is_empty();
        said && (!built || self.scene.work.iter().any(|(w, g)| *w == id && matches!(g, WorkGeom::Plane(_))))
    }

    /// Whether a plane of symmetry the study holds cannot be one: a work plane that is gone or
    /// that is not a plane, a face of the part (from a file: the commands refuse one).
    pub(crate) fn plane_lost(&self, plane: &PlaneRef) -> bool {
        match plane {
            PlaneRef::Origin(_) => false,
            PlaneRef::Face(_) => true,
            PlaneRef::Work(id) => !self.work_plane_there(*id),
        }
    }

    /// Symmetry: the planes the shape is the same on both sides of. Its work planes are the
    /// part's; what the study holds besides is shown as lost, to be unticked.
    pub(crate) fn open_symmetry(&mut self) -> Result<(), String> {
        let study = self.study_for_new()?;
        let features = self.document().features().iter();
        let work = features.filter(|f| self.work_plane_there(f.id)).map(|f| (f.id, f.name.clone(), false)).collect();
        let mut p = SymmetryPanel { study, origin: [false; 3], work, lost: Vec::new() };
        for plane in &self.shape_settings(study).symmetry {
            let listed = p.work.iter_mut().find(|w| PlaneRef::Work(w.0) == *plane);
            match (plane, listed) {
                (_, Some(w)) => w.2 = true,
                (PlaneRef::Origin(o), None) => {
                    if let Some(i) = ORIGINS.iter().position(|x| x == o) {
                        p.origin[i] = true;
                    }
                }
                (PlaneRef::Face(_), None) => p.lost.push((plane.clone(), "A face of the part".to_owned(), true)),
                (PlaneRef::Work(id), None) => {
                    let name = self.document().feature(*id).map_or_else(|| format!("Work plane {}", id.0), |f| f.name.clone());
                    p.lost.push((plane.clone(), name, true));
                }
            }
        }
        self.view.selection.clear();
        self.panel = Some(Panel::Symmetry(p));
        self.set_status("Symmetry: tick the origin planes or work planes the part is the same on both sides of, then OK.");
        Ok(())
    }

    /// OK on Optimize Shape: the settings kept with the study, then it is worked out. True when
    /// it started.
    pub(crate) fn commit_shape(&mut self, p: &ShapePanel) -> bool {
        if self.exec_status("study.shape", p.command()).is_none() {
            return false;
        }
        match self.optimize_shape() {
            Ok(()) => true,
            Err(e) => {
                self.set_error(e);
                false
            }
        }
    }

    /// OK on Keep Faces. True when they were kept.
    pub(crate) fn commit_keep(&mut self, p: &KeepPanel) -> bool {
        if p.faces.is_empty() {
            self.set_error("Keep Faces: click at least one face");
            return false;
        }
        let (id, params) = p.command();
        self.exec_status(id, params).is_some()
    }

    /// OK on Symmetry. True when the planes were kept.
    pub(crate) fn commit_symmetry(&mut self, p: &SymmetryPanel) -> bool {
        self.exec_status("study.shape", json!({ "study": p.study.0, "symmetry": p.planes() })).is_some()
    }

    /// The run of Optimize Shape there is, if any.
    pub(crate) fn shaping(&self) -> Option<&Shaping> {
        self.analysis.as_ref().and_then(|e| e.shaping.as_ref())
    }

    /// Works the active study's shape out, where the kernel is, with the settings it has.
    pub(crate) fn optimize_shape(&mut self) -> Result<(), String> {
        if !self.in_analysis() {
            self.analysis_begin()?;
        }
        self.no_run_going()?;
        let Some(study) = self.active_study().map(|s| s.id) else {
            return Err("There is no study yet: Create Study, then say where the part is held and what pushes on it.".into());
        };
        self.panel = None;
        let (cancel, basis) = (Cancel::default(), self.shape_basis(study));
        let progress: Progress = Arc::new(Mutex::new(Report::default()));
        self.step_seq += 1;
        let run = self.step_seq;
        let shaping = Shaping { run, study, basis, cancel: cancel.clone(), since: Instant::now(), progress: progress.clone() };
        let work = move |s: &mut Session, k: &mut dyn tenon_kernel::Kernel| -> Box<dyn std::any::Any + Send> {
            let settings = s.document().study(study).and_then(|st| st.optimize.clone()).unwrap_or_default();
            Box::new(tenon_io::shape::work_out(s, k, study, &settings, SMOOTHING, None, &cancel, Some(&progress)))
        };
        if let Some(env) = &mut self.analysis {
            env.shaping = Some(shaping);
        }
        match &mut self.geo {
            Geo::Off => {
                if let Some(env) = &mut self.analysis {
                    env.shaping = None;
                }
                return Err("there is no geometry kernel".into());
            }
            Geo::Sync(k) => {
                let done = work(&mut self.session, k.as_mut());
                self.shape_job_done(run, study, basis, done);
            }
            Geo::Worker(w) => {
                let doc = self.session.document().clone();
                self.part_jobs.insert(run, PartJob::Shape { run, study, basis });
                w.job(
                    run,
                    Box::new(move |k, _| {
                        let mut s = Session::default();
                        s.replace_document(doc, None);
                        let done = work(&mut s, k);
                        s.release(k);
                        done
                    }),
                );
                self.set_status("Optimizing the shape: the part is divided into elements, then solved round after round. Esc stops it and keeps the shape so far.");
            }
        }
        Ok(())
    }

    /// A shape came back. It is taken only by the run the environment there is waits for: one
    /// that was left behind is dropped, and says nothing (the status bar has moved on).
    pub(crate) fn shape_job_done(&mut self, run: u64, study: StudyId, basis: u64, result: Box<dyn std::any::Any + Send>) {
        let Some(took) = self.analysis.as_mut().and_then(|e| e.shaping.take_if(|r| r.run == run)).map(|r| r.since.elapsed().as_secs_f64()) else {
            return;
        };
        let name = self.document().study(study).map_or_else(|| "The study".to_owned(), |s| s.name.clone());
        // (The part or the study changed while it went on: whatever it came to is of what was.)
        if basis != self.shape_basis(study) {
            self.set_status(format!("{name}: the part or the study changed while the shape was worked out, so it is out of date and was not kept."));
            return;
        }
        let shaped = match result.downcast::<Result<Shaped, String>>().map_or_else(|_| Err("internal: unexpected result".to_owned()), |b| *b) {
            Ok(s) => s,
            Err(e) if e == tenon_fea::tools::STOPPED => {
                self.set_status(format!("{name}: stopped before a shape was found."));
                return;
            }
            Err(e) => {
                self.set_error(e);
                return;
            }
        };
        let o = &shaped.optimized;
        let ending = match o.stop {
            tenon_fea::topo::Stop::Settled => format!("settled after {} rounds", o.history.len()),
            tenon_fea::topo::Stop::LevelledOff => format!("levelled off after {} rounds", o.history.len()),
            tenon_fea::topo::Stop::Limit => format!("stopped at the most rounds ({})", o.history.len()),
            tenon_fea::topo::Stop::Budget => format!("out of time after {} rounds", o.history.len()),
            tenon_fea::topo::Stop::Stopped => format!("stopped after {} rounds: the shape so far", o.history.len()),
        };
        let kept = match shaped.shape_compliance {
            Some(c) if c > 0.0 => format!(" It keeps {} of the part's stiffness.", figures(o.solid_compliance / c)),
            _ => String::new(),
        };
        // The share it came to, and the share asked when that is not it (stopped early, or pieces
        // left out).
        let removed = 100.0 * (1.0 - shaped.shown.share);
        let asked = 100.0 * (1.0 - shaped.fraction);
        let removed =
            if (removed - asked).abs() > 1.0 { format!("{removed:.0}% removed, {asked:.0}% asked") } else { format!("{removed:.0}% removed") };
        // (Export Shape goes by the study being worked on: another may be, by now.)
        let keeps = if self.active_study().map(|s| s.id) == Some(study) {
            "Export Shape keeps it.".to_owned()
        } else {
            format!("Work on {name} to see it and export it.")
        };
        // (What else there is to know of it does not fit here: it is written over the view.)
        let said = format!(
            "{name}: a shape of {} g for a part of {} g ({removed}), {ending} in {} s.{kept} A guide to model over: {keeps}",
            figures(shaped.grams(shaped.shown.share)),
            figures(shaped.grams(1.0)),
            figures(took)
        );
        let percent = 100.0 * shaped.fraction;
        let closed = shaped.shown.skin.is_closed();
        let view =
            ShapeView { study, basis, shown: shaped.shown.clone(), shown_percent: percent, shaped: Arc::new(shaped), percent, visible: true, closed };
        if let Some(env) = &mut self.analysis {
            env.shape = Some(view);
        }
        self.set_status(said);
    }

    /// A run of a part or a study that has changed since it began comes to nothing, as Esc would
    /// find: it is stopped now, and said so now, and not at the end of however long it has left.
    /// (A shape goes by what it hangs on; what Simulate and Mesh View come to goes by the revision.)
    pub(crate) fn stop_a_run_gone_stale(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        let revision = self.session.revision();
        let env = self.analysis.as_ref();
        let simulating =
            env.and_then(|e| e.running.as_ref()).filter(|r| r.revision != revision && !r.cancel.load(Relaxed)).map(|r| (r.study, &r.cancel));
        let shaping = self.shaping().filter(|r| !r.cancel.load(Relaxed) && r.basis != self.shape_basis(r.study)).map(|r| (r.study, &r.cancel));
        let Some((study, cancel)) = shaping.or(simulating) else { return };
        cancel.store(true, Relaxed);
        let name = self.document().study(study).map_or_else(|| "The study".to_owned(), |s| s.name.clone());
        self.set_status(format!("{name}: the part or the study changed, so what is being worked out is out of date: it is being stopped."));
    }

    /// What a shape of `study` hangs on, as the part and the study now are: see [`basis_of`].
    /// (Worked out once for each revision: it is asked many times a frame.)
    pub(crate) fn shape_basis(&self, study: StudyId) -> u64 {
        let revision = self.session.revision();
        match self.shape_basis_memo.get() {
            Some((r, s, basis)) if r == revision && s == study => basis,
            _ => {
                let basis = basis_of(self.document(), study);
                self.shape_basis_memo.set(Some((revision, study, basis)));
                basis
            }
        }
    }

    /// The shape of the study worked on, if it is of the part as it is.
    pub(crate) fn shape_view(&self) -> Option<&ShapeView> {
        let env = self.analysis.as_ref().filter(|_| self.in_analysis())?;
        let active = self.active_study()?.id;
        env.shape.as_ref().filter(|v| v.study == active && v.basis == self.shape_basis(active))
    }

    /// The study worked on has a shape, and the part or the study has changed since.
    pub(crate) fn shape_is_stale(&self) -> bool {
        let Some(env) = self.analysis.as_ref() else { return false };
        let active = self.active_study().map(|s| s.id);
        env.shape.as_ref().is_some_and(|v| Some(v.study) == active && v.basis != self.shape_basis(v.study))
    }

    /// A shape is drawn over the part.
    pub(crate) fn shape_shown(&self) -> bool {
        self.shape_view().is_some_and(|v| v.visible)
    }

    /// The shape shown, or not.
    pub(crate) fn toggle_shape(&mut self) {
        if let Some(v) = self.analysis.as_mut().and_then(|e| e.shape.as_mut()) {
            v.visible = !v.visible;
        }
    }

    /// The shape not drawn, so that what was asked for in its place can be. Its row brings it
    /// back.
    pub(crate) fn hide_shape(&mut self) {
        if let Some(v) = self.analysis.as_mut().and_then(|e| e.shape.as_mut()) {
            v.visible = false;
        }
    }

    /// The share on the slider, percent: the surface made again for it.
    pub(crate) fn set_shape_share(&mut self, percent: f64) {
        let Some(v) = self.analysis.as_mut().and_then(|e| e.shape.as_mut()) else { return };
        let percent = percent.clamp(5.0, 100.0);
        v.percent = percent;
        v.shown = v.shaped.reshaped(percent / 100.0, SMOOTHING);
        v.shown_percent = percent;
        v.closed = v.shown.skin.is_closed();
    }

    /// Export Shape: the shape shown, as STL or 3MF.
    pub(crate) fn export_shape(&mut self) -> Result<(), String> {
        let Some(v) = self.shape_view() else {
            return Err("There is no shape to export: Optimize Shape first.".into());
        };
        // (A share still in its field when the button was pressed: the ribbon is drawn before the
        // slider has seen the field lose the focus, and it is this share that is meant.)
        let (percent, made_for) = (v.percent, v.shown_percent);
        if (percent - made_for).abs() > 1e-9 {
            self.set_shape_share(percent);
        }
        let Some(v) = self.shape_view() else {
            return Err("There is no shape to export: Optimize Shape first.".into());
        };
        let (skin, closed, joined) = (v.shown.skin.clone(), v.closed, !v.shown.load_not_joined);
        let doc = self.document();
        let name = doc.name.clone();
        let (color, material) = (doc.color().unwrap_or([0xb8, 0xc0, 0xc8]), doc.material().map_or_else(|| "Generic".to_owned(), |m| m.name.clone()));
        let file = format!("{name} shape.stl");
        let chosen = match &self.services.pick_save_of {
            Some(pick) => pick(&file, &["stl", "3mf"]),
            None => self.services.pick_save.as_ref().and_then(|pick| pick(&file, "stl")),
        };
        let Some(path) = chosen else { return Ok(()) };
        tenon_io::shape::write_shape(&path, &skin, &name, &material, color)?;
        let triangles = skin.triangles.len();
        // (Written all the same, as a part's export is: the file is what was asked for.)
        let wrong: Vec<&str> = [
            (!closed).then_some("its surface is not closed: a slicer may refuse it"),
            (!joined).then_some("what is kept at this share does not join every load to a support"),
        ]
        .into_iter()
        .flatten()
        .collect();
        if wrong.is_empty() {
            self.set_status(format!("Exported the shape to {} ({triangles} triangles).", path.display()));
        } else {
            self.set_error(format!("Exported the shape to {} ({triangles} triangles), but {}.", path.display(), wrong.join(", and ")));
        }
        Ok(())
    }

    /// The shape drawn over the part (shaded flat, toward the eye), and while a run goes on,
    /// where it is. The words go on last: a shape is large on the screen when zoomed in, and they are
    /// to be read over it.
    pub(crate) fn shape_overlay(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        #[cfg(test)]
        ui.data_mut(|d| d.remove::<usize>(egui::Id::new("tn_shape_drawn")));
        let painter = ui.painter().with_clip_rect(rect);
        let shown = self.shape_view().filter(|v| v.visible);
        if let Some(v) = shown {
            let skin = &v.shown.skin;
            let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
            let cam = &self.view.camera;
            let seen: Vec<Option<(Pos2, f64)>> =
                skin.positions.iter().map(|p| cam.project(*p, w, h).map(|(x, y, d)| (rect.min + vec2(x as f32, y as f32), d))).collect();
            // (Lit from the eye: what faces it is brightest.)
            let light = cam.eye_dir();
            let mut facing: Vec<(f64, usize, f32)> = Vec::with_capacity(skin.triangles.len() / 2);
            for (i, [a, b, c]) in skin.triangles.iter().enumerate() {
                let (Some((pa, da)), Some((pb, db)), Some((pc, dc))) = (seen[*a as usize], seen[*b as usize], seen[*c as usize]) else { continue };
                // (Wound to face out: seen from outside, clockwise on a screen whose y runs down.)
                let turn = (pb.x - pa.x) * (pc.y - pa.y) - (pb.y - pa.y) * (pc.x - pa.x);
                if turn >= 0.0 {
                    continue;
                }
                let (pa3, pb3, pc3) = (skin.positions[*a as usize], skin.positions[*b as usize], skin.positions[*c as usize]);
                let n = (pb3 - pa3).cross(pc3 - pa3);
                let lit = if n.len() > 0.0 { n.dot(light) / n.len() } else { 0.0 };
                facing.push((da + db + dc, i, (0.45 + 0.55 * lit.abs()) as f32));
            }
            facing.sort_by(|x, y| y.0.total_cmp(&x.0));
            let mut drawn = egui::Mesh::default();
            for (_, i, shade) in &facing {
                let color = Color32::from_rgb((SHAPE[0] * shade * 255.0) as u8, (SHAPE[1] * shade * 255.0) as u8, (SHAPE[2] * shade * 255.0) as u8);
                let first = drawn.vertices.len() as u32;
                for k in skin.triangles[*i] {
                    drawn.colored_vertex(seen[k as usize].map_or(Pos2::ZERO, |s| s.0), color);
                }
                drawn.add_triangle(first, first + 1, first + 2);
            }
            painter.add(egui::Shape::mesh(drawn));
            #[cfg(test)]
            ui.data_mut(|d| d.insert_temp(egui::Id::new("tn_shape_drawn"), facing.len()));
        }
        let over_the_view = |text: &str, at: Pos2| {
            let wide = (rect.right() - at.x - 18.0).max(100.0);
            let galley = |color| painter.layout(text.to_owned(), theme::body(), color, wide);
            let (shadow, written) = (galley(t.viewport_text_shadow), galley(t.viewport_text));
            let high = written.size().y;
            painter.galley(at + vec2(1.0, 1.0), shadow, t.viewport_text_shadow);
            painter.galley(at, written, t.viewport_text);
            high
        };
        // (Beside the legend, when a study's results are drawn: it starts just as high.)
        let going = pos2(rect.left() + if self.results_shown() { crate::results::LEGEND_REACHES + 10.0 } else { 18.0 }, rect.top() + 40.0);
        if let Some(r) = &self.port_run {
            let stopping = r.cancel.load(std::sync::atomic::Ordering::Relaxed);
            over_the_view(if stopping { crate::results::PORT_RUN_STOPS } else { crate::results::PORT_RUN_GOES }, going);
        }
        if let Some(r) = self.shaping() {
            let report = *r.progress.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let text = match (report.phase, report.last) {
                _ if r.cancel.load(std::sync::atomic::Ordering::Relaxed) => "Optimize Shape: stopping...".to_owned(),
                (Phase::Meshing, _) => "Optimize Shape: dividing the part into elements...".to_owned(),
                (Phase::Solid, _) => "Optimize Shape: the part solved solid...".to_owned(),
                (Phase::Iterating, Some(i)) => format!(
                    "Optimize Shape: round {} of at most {} ({} s each). Esc stops it and keeps the shape so far.",
                    i.number,
                    report.most,
                    figures(i.seconds)
                ),
                (Phase::Iterating, None) => "Optimize Shape: the first round...".to_owned(),
                (Phase::Shaping, _) => "Optimize Shape: making the surface of what is kept...".to_owned(),
                (Phase::Checking, _) => "Optimize Shape: the shape solved once...".to_owned(),
            };
            over_the_view(&text, going);
        }
        // What there is to know of it, written out: it is longer than the status bar has room for.
        // (Below where a run going on says where it is.)
        if let Some(v) = shown {
            let running = self.shaping().is_some() || self.port_run.is_some();
            let mut y = rect.top() + if running { 62.0 } else { 40.0 };
            for said in v.said() {
                y += over_the_view(&said, pos2(rect.left() + 18.0, y)) + 3.0;
            }
        }
    }

    /// The share kept, on a slider at the foot of the view, while a shape is shown.
    pub(crate) fn shape_slider(&mut self, ui: &Ui, rect: Rect, t: &Tokens) {
        let Some(v) = self.shape_view().filter(|v| v.visible) else { return };
        let (mut percent, asked, share, made_for) = (v.percent, 100.0 * v.shaped.fraction, v.shown.share, v.shown_percent);
        let mut at_rest = false;
        // (Clear of the axes in the corner.)
        egui::Area::new(egui::Id::new("tn_shape_slider")).fixed_pos(pos2(rect.left() + 110.0, rect.bottom() - 70.0)).fade_in(false).show(
            ui.ctx(),
            |ui| {
                egui::Frame::new().fill(t.panel).stroke(egui::Stroke::new(1.0, t.border)).corner_radius(4.0).inner_margin(8.0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Kept");
                        let r = ui.add(egui::Slider::new(&mut percent, 5.0..=100.0).show_value(false));
                        let field = egui::Id::new("tn_shape_keep");
                        // (Esc gives up what was typed.)
                        if eq_field(ui, field, &mut percent, None, "%", 5.0..=100.0, 70.0, t).given_up {
                            percent = made_for;
                        }
                        at_rest = !r.dragged() && !ui.memory(|m| m.has_focus(field));
                        ui.label(format!("{:.0}% of the part", 100.0 * share));
                    });
                    let note = if (percent - asked).abs() < 0.5 && (100.0 * share - asked).abs() <= 1.0 {
                        "The share it was optimized for.".to_owned()
                    } else if (percent - asked).abs() < 0.5 {
                        format!("Optimized for {asked:.0}%: it came to {:.0}%, stopped early or with pieces left out.", 100.0 * share)
                    } else if (percent - made_for).abs() < 1e-9 && percent - 100.0 * share > 1.0 {
                        // (A run stopped early: the rest of the part is at the lowest density still.)
                        format!("Optimized for {asked:.0}%: no more material ranks above this, or the rest is in pieces left out, so {percent:.0}% comes to {:.0}%.", 100.0 * share)
                    } else {
                        format!("Optimized for {asked:.0}%: other shares show where material ranks, not an optimum for that share.")
                    };
                    ui.label(egui::RichText::new(note).small().color(t.text_dim));
                });
            },
        );
        // (The slider follows the pointer and the field what is typed; the surface is made again
        // when the one is let go or the other left, by Enter or by a click elsewhere. Esc in the
        // field leaves it as it is.)
        if let Some(v) = self.analysis.as_mut().and_then(|e| e.shape.as_mut()) {
            v.percent = percent;
        }
        if at_rest && (percent - made_for).abs() > 1e-9 {
            self.set_shape_share(percent);
        }
    }
}
