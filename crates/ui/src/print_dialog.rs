//! 3D printing (M7): the 3D Print Check window, and writing a part or an assembly out as the
//! triangles a slicer reads (STL, 3MF). The meshing is done where the kernel is: on the worker
//! in the app, here when headless.

use std::path::{Path, PathBuf};

use egui::{Color32, RichText, Ui};
use tenon_io::cmd::Written;
use tenon_io::print::{Report, Setup, Up};
use tenon_kernel::{Kernel, MeshTol, ShapeHandle};
use tenon_model::Document;

use crate::params_dialog::edit_cell;
use crate::properties::fmt_value;
use crate::theme::{self, Tokens};
use crate::viewport::Pick;
use crate::workbench::{Geo, Workbench};

/// Faces that lean out too far, and faces with too little behind them, in the view.
pub(crate) const OVERHANG: Color32 = Color32::from_rgb(0xe8, 0x95, 0x2c);
pub(crate) const THIN: Color32 = Color32::from_rgb(0xd0, 0x40, 0x30);
/// The window's width, and the width of the column that names each check.
const WIDTH: f32 = 372.0;
const TITLE_W: f32 = 70.0;
/// A check that found something.
const FAIL: Color32 = Color32::from_rgb(0xd9, 0x53, 0x4f);

/// A mesh file a slicer reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MeshFile {
    Stl,
    Mf3,
}

impl MeshFile {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            MeshFile::Stl => "stl",
            MeshFile::Mf3 => "3mf",
        }
    }
    fn name(self) -> &'static str {
        match self {
            MeshFile::Stl => "STL",
            MeshFile::Mf3 => "3MF",
        }
    }
}

/// What a job about the open part (or assembly) is for.
pub(crate) enum PartJob {
    File {
        path: PathBuf,
        kind: MeshFile,
    },
    Print {
        revision: u64,
        setup: Setup,
    },
    /// What the edges of a face are in a sketch with the sheet laid flat (Project Flat Pattern).
    FlatEdges {
        sketch: tenon_model::FeatureId,
    },
    /// The flat pattern of a sheet metal part, of this revision of it.
    Flat {
        revision: u64,
    },
    /// A stress study of this revision of the part: worked out (`solving`), or its mesh alone.
    Study {
        study: tenon_model::study::StudyId,
        revision: u64,
        solving: bool,
    },
}

/// What the window keeps.
#[derive(Clone, Debug)]
pub(crate) struct PrintState {
    pub open: bool,
    /// What to check against (the bed apart: it is only checked when ticked).
    pub setup: Setup,
    pub bed_on: bool,
    pub bed: [f64; 3],
    /// The last check: the document revision and setup it was of, and what it found.
    pub report: Option<(u64, Setup, Result<Report, String>)>,
    /// A check being made: the revision and setup it is of.
    pub pending: Option<(u64, Setup)>,
    /// What was found is tinted in the view.
    pub show: bool,
}

impl Default for PrintState {
    fn default() -> Self {
        // A bed of the commonest size, for when one is asked for.
        PrintState { open: false, setup: Setup::default(), bed_on: false, bed: [220.0, 220.0, 250.0], report: None, pending: None, show: true }
    }
}

impl PrintState {
    /// What is checked against as it stands in the window.
    pub(crate) fn wanted(&self) -> Setup {
        Setup { bed: self.bed_on.then_some(self.bed), ..self.setup }
    }
}

fn up_label(up: Up) -> &'static str {
    match up {
        Up::X => "+X",
        Up::Y => "+Y",
        Up::Z => "+Z",
        Up::NegX => "-X",
        Up::NegY => "-Y",
        Up::NegZ => "-Z",
    }
}

fn solids(r: &tenon_model::Regen) -> Result<Vec<ShapeHandle>, String> {
    if let Some((_, msg)) = r.first_error() {
        return Err(format!("the part does not regenerate: {msg}"));
    }
    if r.bodies.is_empty() {
        return Err("there is no solid".into());
    }
    Ok(r.bodies.iter().map(|b| b.shape).collect())
}

fn unexpected<T>() -> Result<T, String> {
    Err("internal: unexpected result".into())
}

impl Workbench {
    /// Runs `work` on the open part's solids with the kernel, wherever that is; what it returns
    /// comes back to [`Workbench::part_job_done`] as `job`.
    pub(crate) fn part_job<T: Send + 'static>(
        &mut self,
        job: PartJob,
        work: impl FnOnce(&mut dyn Kernel, &[ShapeHandle], &Document) -> Result<T, String> + Send + 'static,
    ) -> Result<(), String> {
        let doc = self.session.document().clone();
        let done: Result<T, String> = match &mut self.geo {
            Geo::Off => return Err("there is no geometry kernel".into()),
            Geo::Sync(k) => {
                let shapes = solids(self.session.regen(k.as_mut()));
                shapes.and_then(|shapes| work(k.as_mut(), &shapes, &doc))
            }
            Geo::Worker(w) => {
                self.step_seq += 1;
                self.part_jobs.insert(self.step_seq, job);
                w.job(
                    self.step_seq,
                    Box::new(move |k, _| {
                        // The part as it is saved, not as the view may be showing it (a sketch
                        // open, a panel's preview).
                        let mut regen = tenon_model::regenerate(&doc, k);
                        let done = solids(&regen).and_then(|shapes| work(k, &shapes, &doc));
                        regen.release(k);
                        Box::new(done)
                    }),
                );
                return Ok(());
            }
        };
        self.finish_part_job(job, Box::new(done));
        Ok(())
    }

    /// A job finished. True when it was one of these.
    pub(crate) fn part_job_done(&mut self, request: u64, result: Box<dyn std::any::Any + Send>) -> Result<(), Box<dyn std::any::Any + Send>> {
        match self.part_jobs.remove(&request) {
            Some(job) => {
                self.finish_part_job(job, result);
                Ok(())
            }
            None => Err(result),
        }
    }

    fn finish_part_job(&mut self, job: PartJob, result: Box<dyn std::any::Any + Send>) {
        match job {
            PartJob::File { path, kind } => {
                let written = result.downcast::<Result<Written, String>>().map_or_else(|_| unexpected(), |b| *b);
                match written.and_then(|w| std::fs::write(&path, &w.data).map(|()| w).map_err(|e| format!("cannot write {}: {e}", path.display()))) {
                    Ok(w) if w.watertight => self.set_status(format!("Exported {} ({} triangles)", path.display(), w.triangles)),
                    // Written all the same: the file is what was asked for, and says what is wrong with the part.
                    Ok(_) => self.set_error(format!(
                        "Exported {}, but its surface is not closed: a slicer may refuse it. See Environments > 3D Print Check.",
                        path.display()
                    )),
                    Err(e) => self.set_error(format!("{} export failed: {e}", kind.name())),
                }
            }
            PartJob::Flat { revision } => self.flat_done(revision, result),
            PartJob::Study { study, revision, solving } => self.study_job_done(study, revision, solving, result),
            PartJob::FlatEdges { sketch } => self.flat_edges_done(sketch, result),
            PartJob::Print { revision, setup } => {
                let report = result.downcast::<Result<Report, String>>().map_or_else(|_| unexpected(), |b| *b);
                let print = &mut self.chrome.print;
                if print.pending == Some((revision, setup)) {
                    print.pending = None;
                }
                print.report = Some((revision, setup, report));
            }
        }
    }

    /// Writes the open part as a mesh file, to the usual tolerance kept over the whole surface.
    pub(crate) fn export_mesh(&mut self, path: &Path, kind: MeshFile) -> Result<(), String> {
        let job = PartJob::File { path: path.to_path_buf(), kind };
        self.part_job(job, move |k, shapes, doc| {
            let tol = MeshTol::exact();
            match kind {
                MeshFile::Stl => tenon_io::cmd::stl_data(k, shapes, &doc.name, &tol, false),
                MeshFile::Mf3 => tenon_io::cmd::part_3mf(k, shapes, doc, &tol),
            }
            .map_err(|e| e.0)
        })?;
        if !self.part_jobs.is_empty() {
            self.set_status(format!("Exporting {}...", kind.name()));
        }
        Ok(())
    }

    /// Writes the open assembly as a mesh file: every visible component where it is shown.
    pub(crate) fn export_assembly_mesh(&mut self, path: &Path, kind: MeshFile) -> Result<(), String> {
        let frames = self.display_frames();
        let a = self.asm.as_mut().ok_or("no assembly")?;
        let asm = a.session.assembly().clone();
        let infos = tenon_io::asm::object_infos(&asm, &a.session.parts);
        let tol = MeshTol::exact();
        let write = move |k: &mut dyn Kernel, placed: &[tenon_assembly::interfere::Placed]| match kind {
            MeshFile::Stl => tenon_io::asm::placed_stl(k, &asm.name, placed, &tol, false),
            MeshFile::Mf3 => tenon_io::asm::placed_3mf(k, &asm.name, placed, &infos, &tol),
        };
        let job = PartJob::File { path: path.to_path_buf(), kind };
        let done = match &mut self.geo {
            Geo::Off => return Err("there is no geometry kernel".into()),
            Geo::Sync(k) => {
                let shown = a.session.assembly().clone();
                let placed = tenon_assembly::interfere::placed_parts(k.as_mut(), &shown, &mut a.session.parts, Some(&frames));
                placed.and_then(|placed| write(k.as_mut(), &placed))
            }
            Geo::Worker(w) => {
                let items = crate::assembly::slot_items_at(a, &frames);
                self.step_seq += 1;
                self.part_jobs.insert(self.step_seq, job);
                w.job(self.step_seq, Box::new(move |k, slots| Box::new(write(k, &crate::assembly::placed_from_slots(&items, slots)))));
                self.set_status(format!("Exporting {}...", kind.name()));
                return Ok(());
            }
        };
        self.finish_part_job(job, Box::new(done));
        Ok(())
    }

    /// Checks the open part against what the window says, unless that check is made or being made.
    fn check_print(&mut self) {
        let wanted = (self.session.revision(), self.chrome.print.wanted());
        let print = &self.chrome.print;
        if print.pending == Some(wanted) || print.report.as_ref().is_some_and(|(r, s, _)| (*r, *s) == wanted) {
            return;
        }
        let (revision, setup) = wanted;
        self.chrome.print.pending = Some(wanted);
        if let Err(e) = self.part_job(PartJob::Print { revision, setup }, move |k, shapes, _| tenon_io::print::check(k, shapes, &setup)) {
            self.chrome.print.pending = None;
            self.chrome.print.report = Some((revision, setup, Err(e)));
        }
    }

    /// The check of the part as it is now, if there is one.
    pub(crate) fn print_report(&self) -> Option<&Report> {
        let print = &self.chrome.print;
        let (revision, setup, report) = print.report.as_ref()?;
        (*revision == self.session.revision() && *setup == print.wanted()).then_some(report.as_ref().ok()).flatten()
    }

    /// What the check found, to tint in the view: thin walls over overhangs.
    pub(crate) fn print_flags(&self) -> Vec<(Pick, Color32)> {
        let print = &self.chrome.print;
        if !print.open || !print.show || self.in_assembly() || self.in_drawing() || self.is_sketching() {
            return Vec::new();
        }
        let Some(report) = self.print_report() else { return Vec::new() };
        let over = report.overhangs.iter().map(|o| (Pick::Face { body: o.body, face: o.face }, OVERHANG));
        let thin = report.thin.iter().map(|t| (Pick::Face { body: t.body, face: t.face }, THIN));
        over.chain(thin).collect()
    }

    /// The printer's build space, drawn round the part as it would stand on the bed, while the
    /// bed is ticked in the 3D Print Check window: a box the bed's size and the printer's
    /// height, its long side along the part's (the part may be turned on the bed).
    pub(crate) fn print_overlay(&self, ui: &Ui, rect: egui::Rect, t: &Tokens) {
        #[cfg(test)]
        ui.data_mut(|d| d.remove::<Vec<tenon_geom::Vec3>>(egui::Id::new("tn_print_space")));
        let print = &self.chrome.print;
        if !print.open || !print.bed_on || self.in_assembly() || self.in_drawing() || self.is_sketching() {
            return;
        }
        let Some(b) = self.scene.bbox() else { return };
        use tenon_geom::Vec3;
        let up = print.setup.up.dir();
        // The bed's two sides: the part's axes square to up.
        let (mut side, mut across) = match print.setup.up {
            Up::X | Up::NegX => (Vec3::Y, Vec3::Z),
            Up::Y | Up::NegY => (Vec3::X, Vec3::Z),
            Up::Z | Up::NegZ => (Vec3::X, Vec3::Y),
        };
        let size = b.max - b.min;
        let [mut long, mut short, height] = print.bed;
        if short > long {
            (long, short) = (short, long);
        }
        if size.dot(across) > size.dot(side) {
            (side, across) = (across, side);
        }
        // Where the part stands: its lowest point, as it is printed.
        let corners = [b.min, b.max];
        let low = (0..8).map(|i| Vec3::new(corners[i & 1].x, corners[(i >> 1) & 1].y, corners[(i >> 2) & 1].z).dot(up)).fold(f64::MAX, f64::min);
        let centre = b.center();
        let floor = centre + up * (low - centre.dot(up));
        let at = |i: usize| {
            let s = if i & 1 == 0 { -0.5 } else { 0.5 };
            let a = if i & 2 == 0 { -0.5 } else { 0.5 };
            let h = if i & 4 == 0 { 0.0 } else { height };
            floor + side * (s * long) + across * (a * short) + up * h
        };
        let space: Vec<Vec3> = (0..8).map(at).collect();
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let screen = |p: Vec3| self.view.camera.project(p, w, h).map(|(x, y, _)| rect.min + egui::vec2(x as f32, y as f32));
        let painter = ui.painter().with_clip_rect(rect);
        let ink = t.accent.gamma_multiply(0.8);
        for (i, j) in [(0, 1), (1, 3), (3, 2), (2, 0), (4, 5), (5, 7), (7, 6), (6, 4), (0, 4), (1, 5), (2, 6), (3, 7)] {
            if let (Some(p), Some(q)) = (screen(space[i]), screen(space[j])) {
                // (The bed itself a little heavier than the walls of the space.)
                painter.line_segment([p, q], egui::Stroke::new(if j < 4 { 1.6 } else { 1.0 }, ink));
            }
        }
        if let Some(p) = screen(space[0]) {
            let text = format!("Build space {} x {} x {} mm", fmt_value(long), fmt_value(short), fmt_value(height));
            painter.text(p + egui::vec2(6.0, 4.0), egui::Align2::LEFT_TOP, text, theme::small(), ink);
        }
        #[cfg(test)]
        ui.data_mut(|d| d.insert_temp(egui::Id::new("tn_print_space"), space));
    }

    pub(crate) fn print_window(&mut self, ui: &mut Ui, t: &Tokens) {
        if !self.chrome.print.open {
            return;
        }
        if self.in_assembly() || self.in_drawing() {
            self.chrome.print.open = false;
            return;
        }
        // Checked again whenever the part or what it is checked against changes (not while a
        // sketch is open or the part is still being rebuilt: it is about to change again).
        if !self.is_sketching() && !self.waiting {
            self.check_print();
        }
        let mut open = true;
        let mut setup = self.chrome.print.setup;
        let (mut bed_on, mut bed, mut show) = (self.chrome.print.bed_on, self.chrome.print.bed, self.chrome.print.show);
        let report = self.print_report().cloned();
        let failed = self.chrome.print.report.as_ref().and_then(|(_, _, r)| r.as_ref().err().cloned());
        let checking = self.chrome.print.pending.is_some() || self.waiting;
        let mut wrong: Option<String> = None;
        let mut export: Option<&'static str> = None;
        egui::Window::new("3D Print Check").open(&mut open).collapsible(false).resizable(false).default_pos([300.0, 170.0]).show(ui.ctx(), |ui| {
            ui.set_width(WIDTH);
            ui.label(RichText::new("Printer").font(theme::body()).strong());
            egui::Grid::new("tn_print_setup").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Up").on_hover_text("Which way the part stands on the bed");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for up in Up::ALL {
                        let r = ui.selectable_label(setup.up == up, up_label(up));
                        crate::drawing::remember(ui, &format!("tn_print_up_{}", up.name()), r.rect);
                        if r.clicked() {
                            setup.up = up;
                        }
                    }
                });
                ui.end_row();
                ui.label("Overhang limit").on_hover_text("How far from vertical a face may lean out over air before it wants support");
                ui.horizontal(|ui| {
                    if let Some(typed) = edit_cell(ui, egui::Id::new("tn_print_overhang"), &fmt_value(setup.overhang), 60.0, t) {
                        match typed.parse::<f64>() {
                            Ok(v) if v > 0.0 && v <= 90.0 => setup.overhang = v,
                            _ => wrong = Some(format!("`{typed}` is not an overhang limit: type an angle up to 90 degrees")),
                        }
                    }
                    ui.label("° from vertical");
                });
                ui.end_row();
                ui.label("Minimum wall").on_hover_text("The thinnest wall the printer makes: about twice its nozzle");
                ui.horizontal(|ui| {
                    if let Some(typed) = edit_cell(ui, egui::Id::new("tn_print_wall"), &fmt_value(setup.min_wall), 60.0, t) {
                        match typed.parse::<f64>() {
                            Ok(v) if v > 0.0 && v <= 100.0 => setup.min_wall = v,
                            _ => wrong = Some(format!("`{typed}` is not a wall thickness: type a size up to 100 mm")),
                        }
                    }
                    ui.label("mm");
                });
                ui.end_row();
                let r = ui.checkbox(&mut bed_on, "Bed");
                crate::drawing::remember(ui, "tn_print_bed", r.rect);
                r.on_hover_text("Check that the part fits the printer: its bed, and how high it prints");
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(bed_on, |ui| {
                        for (i, axis) in ["x", "y", "height"].iter().enumerate() {
                            let Some(slot) = bed.get_mut(i) else { continue };
                            if let Some(typed) = edit_cell(ui, egui::Id::new(("tn_print_bed", *axis)), &fmt_value(*slot), 44.0, t) {
                                match typed.parse::<f64>() {
                                    Ok(v) if v > 0.0 && v.is_finite() => *slot = v,
                                    _ => wrong = Some(format!("`{typed}` is not a size: type a number of mm")),
                                }
                            }
                        }
                        ui.label("mm (x, y, height)");
                    });
                });
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Result").font(theme::body()).strong());
                if checking {
                    ui.label(RichText::new("Checking...").font(theme::small()).color(t.text_dim));
                }
            });
            match (&report, &failed) {
                (Some(r), _) => {
                    // (Columns of widths of their own, not a grid: a grid measures a
                    // wrapped label a letter wide the first time, and the window with it.)
                    for (title, ok, text) in rows(r) {
                        ui.horizontal_top(|ui| {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
                            let colour = match ok {
                                Some(true) => t.ok,
                                Some(false) => FAIL,
                                None => t.text_disabled,
                            };
                            ui.painter().circle_filled(rect.center(), 4.5, colour);
                            ui.allocate_ui_with_layout(egui::vec2(TITLE_W, 16.0), egui::Layout::left_to_right(egui::Align::Min), |ui| {
                                ui.set_min_width(TITLE_W);
                                ui.label(title);
                            });
                            ui.vertical(|ui| {
                                ui.set_width(WIDTH - TITLE_W - 10.0 - 2.0 * ui.spacing().item_spacing.x);
                                ui.add(egui::Label::new(text).wrap());
                            });
                        });
                    }
                    let verdict = if r.ok() {
                        "Nothing found against printing it as it stands."
                    } else if r.watertight() {
                        "It prints, with what is marked above looked after (supports, another way up, thicker walls)."
                    } else {
                        "Its surface is not closed: a slicer cannot tell inside from outside."
                    };
                    ui.add(egui::Label::new(RichText::new(verdict).font(theme::small()).color(t.text_dim)).wrap());
                }
                (None, Some(why)) if !checking => {
                    ui.add(egui::Label::new(RichText::new(format!("Not checked: {why}")).color(FAIL)).wrap());
                }
                _ => {
                    ui.label(RichText::new("The part is read as triangles, as a slicer will read it.").font(theme::small()).color(t.text_dim));
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let r = ui.checkbox(&mut show, "Show in the model");
                crate::drawing::remember(ui, "tn_print_show", r.rect);
                r.on_hover_text("Tint what was found: overhangs orange, thin walls red");
                swatch(ui, OVERHANG, "overhang", t);
                swatch(ui, THIN, "thin wall", t);
            });
            ui.separator();
            ui.horizontal(|ui| {
                if crate::drawing::button(ui, "Export STL...", "tn_print_stl").clicked() {
                    export = Some("export.stl");
                }
                if crate::drawing::button(ui, "Export 3MF...", "tn_print_3mf")
                    .on_hover_text("With the part's material and colour, in millimetres")
                    .clicked()
                {
                    export = Some("export.3mf");
                }
            });
        });
        let print = &mut self.chrome.print;
        (print.open, print.setup, print.bed_on, print.bed, print.show) = (open, setup, bed_on, bed, show);
        if let Some(e) = wrong {
            self.set_error(e);
        }
        if let Some(id) = export {
            self.command(id);
        }
    }
}

/// A colour and what it marks.
fn swatch(ui: &mut Ui, colour: Color32, what: &str, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, colour);
    ui.label(RichText::new(what).font(theme::small()).color(t.text_dim));
}

/// A count with its noun: "1 face", "3 faces".
fn count(n: usize, noun: &str) -> String {
    if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") }
}

/// The report's lines: what was checked, whether it passed (none: not checked), what was found.
pub(crate) fn rows(r: &Report) -> Vec<(&'static str, Option<bool>, String)> {
    let c = &r.closure;
    let closed = if r.watertight() {
        format!("Closed surface, {} triangles", r.triangles)
    } else {
        let mut found = Vec::new();
        for (n, one, many) in [
            (c.open_edges, "open edge", "open edges"),
            (c.crowded_edges, "edge shared by more than two triangles", "edges shared by more than two triangles"),
            (c.flipped_edges, "edge between triangles facing opposite ways", "edges between triangles facing opposite ways"),
        ] {
            if n > 0 {
                found.push(format!("{n} {}", if n == 1 { one } else { many }));
            }
        }
        found.join(", ")
    };
    let overhangs = if r.overhangs.is_empty() {
        format!("None past {}°", fmt_value(r.setup.overhang))
    } else {
        let steepest = r.overhangs.iter().map(|o| o.steepest).fold(0.0, f64::max);
        format!(
            "{}, {} mm² past {}° (to {}°): wants support",
            count(r.overhangs.len(), "face"),
            fmt_value(round_to(r.overhang_area, 0.1)),
            fmt_value(r.setup.overhang),
            fmt_value(round_to(steepest, 0.1))
        )
    };
    let walls = match (r.thinnest, r.thin.is_empty()) {
        (None, _) => "Nothing to measure through".to_owned(),
        (Some(least), true) => format!("Thinnest {} mm", fmt_value(round_to(least, 0.01))),
        (Some(least), false) => {
            format!("Thinnest {} mm: {} under {} mm", fmt_value(round_to(least, 0.01)), count(r.thin.len(), "face"), fmt_value(r.setup.min_wall))
        }
    };
    let extent = format!(
        "{} x {} x {} mm, {} high",
        fmt_value(round_to(r.size.x, 0.01)),
        fmt_value(round_to(r.size.y, 0.01)),
        fmt_value(round_to(r.size.z, 0.01)),
        fmt_value(round_to(r.height, 0.01))
    );
    let size = match r.fits {
        None => extent,
        Some(true) => format!("{extent}: fits the bed"),
        Some(false) => format!("{extent}: does not fit the bed"),
    };
    vec![
        ("Watertight", Some(r.watertight()), closed),
        ("Overhangs", Some(r.overhangs.is_empty()), overhangs),
        ("Walls", r.thinnest.map(|_| r.thin.is_empty()), walls),
        ("Size", r.fits, size),
    ]
}

fn round_to(v: f64, step: f64) -> f64 {
    (v / step).round() * step
}
