//! The part workbench: one document, its regeneration, the 3D view, sketch mode and panels.
//!
//! Model edits go through the shared command registry (`tenon_io::cmd::run`), the same commands
//! the CLI and MCP server use. Regeneration runs on the worker thread; the last good scene stays on
//! screen while a new one is computed or when it fails.

use std::path::{Path, PathBuf};

use egui::{Frame, Ui};
use serde_json::{Value, json};
use tenon_kernel::{Kernel, MeshTol};
use tenon_model::worker::{Response, Worker};
use tenon_model::{Document, FeatureId, FeatureKind, FeatureStatus, PlaneRef, Scene, Session, regenerate_with, scene};

use crate::chrome::{Chrome, DOC_TABS_H, RIBBON_H, STATUS_H, TABS_H, TITLE_H};
use crate::commands;
use crate::panels::Panel;
use crate::sketcher::SketchMode;
use crate::theme::{self, Tokens};
use crate::viewport::{View, VisualStyle};

/// Platform services the workbench may use (native file dialogs).
#[derive(Default)]
pub struct Services {
    /// Choose a project to open.
    pub pick_open: Option<Box<dyn Fn() -> Option<PathBuf>>>,
    /// Choose a file of another kind to read: `(extension without dot)`.
    pub pick_open_ext: Option<Box<dyn Fn(&str) -> Option<PathBuf>>>,
    /// Choose where to save: `(suggested file name, extension without dot)`.
    pub pick_save: Option<Box<dyn Fn(&str, &str) -> Option<PathBuf>>>,
}

/// Where geometry comes from.
pub(crate) enum Geo {
    /// A worker thread with its own kernel (the app).
    Worker(Worker),
    /// A kernel on this thread (tests, headless use).
    Sync(Box<dyn Kernel>),
    /// No kernel: the document can be edited but not shown in 3D.
    Off,
}

pub(crate) enum Mode {
    Model,
    Sketch(Box<SketchMode>),
}

/// Commands that replace the open document (or close the app), so ask to save it first.
const REPLACE_DOCUMENT: &[&str] = &["file.new", "file.new_assembly", "file.new_drawing", "file.new_drawing_template", "file.open", "app.exit"];

/// The part workbench.
pub struct Workbench {
    pub(crate) chrome: Chrome,
    /// The part being modelled (in an assembly: the part edited in place, or an empty one).
    pub(crate) session: Session,
    pub(crate) geo: Geo,
    pub(crate) scene: Scene,
    pub(crate) scene_seq: u64,
    pub(crate) shown: Option<Document>,
    /// Document revision and open panel the shown document was made from: when neither
    /// changed, nothing needs comparing.
    pub(crate) shown_key: Option<u64>,
    /// The open assembly, if the workbench is in the assembly environment.
    pub(crate) asm: Option<Box<crate::assembly::AsmDoc>>,
    /// The open drawing, if the workbench is in the drawing environment (or editing one of its
    /// models).
    pub(crate) drw: Option<Box<crate::drawing::DrwDoc>>,
    /// Rebuild All: the next regeneration starts from scratch.
    rebuild_all: bool,
    /// The document revision last sent to the worker (a change of it is an edit, not a preview).
    sent_revision: Option<u64>,
    seq: u64,
    pub(crate) waiting: bool,
    pub(crate) regen_note: Option<String>,
    /// The feature failure the status bar shows, until a rebuild without it.
    failure_shown: Option<String>,
    pub(crate) view: View,
    pub(crate) mode: Mode,
    pub(crate) panel: Option<Panel>,
    pub(crate) status: String,
    pub(crate) status_error: bool,
    pub(crate) path: Option<PathBuf>,
    pub(crate) services: Services,
    exit: bool,
    pub(crate) step_seq: u64,
    step_export: Option<(u64, PathBuf)>,
    /// The last modelling or sketch command (for "Repeat").
    pub(crate) last_command: Option<&'static str>,
    /// OK, Cancel or Apply asked for from outside the open panel (properties, radial menu).
    pub(crate) panel_request: Option<crate::panels::PanelRequest>,
    /// A sketch tool to start once the sketch being created exists.
    pub(crate) pending_tool: Option<&'static str>,
    /// Start 2D Sketch is waiting for a plane or planar face to be picked.
    pub(crate) pick_plane: bool,
    /// Parameter values by name for the document revision they were computed at.
    pub(crate) param_env: (u64, std::sync::Arc<std::collections::BTreeMap<String, f64>>),
    /// Equations typed into the open feature panel's fields.
    pub(crate) panel_eqs: crate::properties::Equations,
    /// The Parameters dialog's list and the document revision it was made at.
    pub(crate) params_list: (u64, serde_json::Value),
    /// Regeneration checkpoints for the kernel on this thread (headless use).
    sync_cache: tenon_model::RegenCache,
    /// The part last regenerated on this thread (headless use), for measuring.
    sync_regen: tenon_model::Regen,
    /// Where unsaved work is kept against a crash (DEC-033), once the app gives a place.
    pub(crate) recovery: Option<crate::recovery::Recovery>,
    /// Work a Tenon that did not close properly left, offered back.
    pub(crate) recover_offer: Option<crate::recovery::Offer>,
    /// Seconds between autosaves.
    pub(crate) autosave_seconds: f64,
}

impl Workbench {
    fn with_geo(geo: Geo, services: Services) -> Self {
        Workbench {
            chrome: Chrome::default(),
            session: Session::default(),
            geo,
            scene: Scene::default(),
            scene_seq: 0,
            shown: None,
            shown_key: None,
            asm: None,
            drw: None,
            rebuild_all: false,
            sent_revision: None,
            seq: 0,
            waiting: false,
            regen_note: None,
            failure_shown: None,
            view: View::default(),
            mode: Mode::Model,
            panel: None,
            status: "Ready. Start with New Sketch.".into(),
            status_error: false,
            path: None,
            services,
            exit: false,
            step_seq: 0,
            step_export: None,
            last_command: None,
            panel_request: None,
            pending_tool: None,
            pick_plane: false,
            param_env: (0, Default::default()),
            panel_eqs: Default::default(),
            params_list: (0, serde_json::Value::Null),
            sync_cache: Default::default(),
            sync_regen: Default::default(),
            recovery: None,
            recover_offer: None,
            autosave_seconds: crate::recovery::AUTOSAVE_SECONDS,
        }
    }

    /// The app: regeneration on a worker thread with the kernel `make_kernel` creates there.
    /// `waker` is called when results arrive (e.g. to repaint).
    pub fn new(make_kernel: impl FnOnce() -> Box<dyn Kernel> + Send + 'static, waker: Option<Box<dyn Fn() + Send>>, services: Services) -> Self {
        match Worker::spawn(make_kernel, MeshTol::default(), waker) {
            Ok(w) => Self::with_geo(Geo::Worker(w), services),
            Err(e) => {
                let mut wb = Self::with_geo(Geo::Off, services);
                wb.set_error(format!("cannot start the geometry thread: {e}"));
                wb
            }
        }
    }

    /// Regeneration on the calling thread (tests and tools).
    pub fn headless(kernel: Box<dyn Kernel>) -> Self {
        Self::with_geo(Geo::Sync(kernel), Services::default())
    }

    /// No geometry at all (UI tests without a kernel).
    pub fn without_kernel() -> Self {
        Self::with_geo(Geo::Off, Services::default())
    }

    pub fn document(&self) -> &Document {
        self.session.document()
    }
    pub fn scene(&self) -> &Scene {
        &self.scene
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn exit_requested(&self) -> bool {
        self.exit
    }
    /// The UI colour scheme (Tools > Application Options).
    pub fn theme(&self) -> theme::ThemeName {
        self.chrome.theme
    }
    pub fn set_theme(&mut self, name: theme::ThemeName) {
        self.chrome.theme = name;
    }
    /// Shows an error in the status bar.
    pub fn report_error(&mut self, message: impl Into<String>) {
        self.set_error(message);
    }
    /// True while regeneration or an export is running on the worker.
    pub fn is_busy(&self) -> bool {
        self.waiting
            || self.step_export.is_some()
            || self.asm.as_ref().is_some_and(|a| !a.jobs.is_empty())
            || self.drw.as_ref().is_some_and(|d| !d.jobs.is_empty())
    }
    pub fn is_sketching(&self) -> bool {
        matches!(self.mode, Mode::Sketch(_))
    }

    pub(crate) fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.status_error = false;
    }
    pub(crate) fn set_error(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.status_error = true;
    }

    /// Runs a registry command (model or file) on the document.
    pub fn exec(&mut self, id: &str, params: Value) -> Result<Value, String> {
        let kernel: Option<&mut dyn Kernel> = match &mut self.geo {
            Geo::Sync(k) => Some(k.as_mut()),
            _ => None,
        };
        tenon_io::cmd::run(&mut self.session, id, &params, kernel).map_err(|e| e.to_string())
    }

    /// Runs a command and reports failure in the status bar.
    pub(crate) fn exec_status(&mut self, id: &str, params: Value) -> Option<Value> {
        match self.exec(id, params) {
            Ok(v) => Some(v),
            Err(e) => {
                self.set_error(e);
                None
            }
        }
    }

    /// Runs a ribbon/menu command by id, reporting errors in the status bar.
    pub(crate) fn command(&mut self, id: &str) {
        let repeatable = commands::find(id)
            .filter(|c| ["sketch.", "model.", "work.", "inspect."].iter().any(|p| c.id.starts_with(p)) && c.id != "sketch.finish")
            .or_else(|| commands::find(id).filter(|c| ["asm.constrain", "asm.joint", "asm.place"].contains(&c.id)))
            .map(|c| c.id);
        match self.run_ui(id) {
            Ok(()) => {
                if repeatable.is_some() {
                    self.last_command = repeatable;
                }
            }
            Err(e) => self.set_error(e),
        }
    }

    /// Ribbon, menu and toolbar commands. New, Open and Exit ask first when they would lose unsaved
    /// changes, and run once "Save changes?" is answered.
    pub fn run_ui(&mut self, id: &str) -> Result<(), String> {
        if REPLACE_DOCUMENT.contains(&id) && self.ask_to_save(id) {
            return Ok(());
        }
        self.run_unprompted(id)
    }

    /// The window is being closed: true when it may close now. With unsaved changes it shows
    /// "Save changes?" instead, and the app exits after Save or Don't Save (see
    /// [`Workbench::exit_requested`]).
    pub fn close_requested(&mut self) -> bool {
        self.exit || !self.ask_to_save("app.exit")
    }

    /// Shows "Save changes?" before `then` runs when anything has unsaved changes; false when
    /// nothing would be lost.
    fn ask_to_save(&mut self, then: &str) -> bool {
        let files = self.unsaved_files();
        if files.is_empty() {
            return false;
        }
        self.chrome.save_prompt = Some(crate::chrome::SavePrompt { then: then.to_owned(), files, focused: false });
        true
    }

    /// Save in "Save changes?": the open document from its top level, which saves everything
    /// changed in it (a drawing the models changed from it, an assembly its parts), the model or
    /// part being edited included. A document never saved asks where to save it.
    pub(crate) fn save_everything(&mut self) -> Result<(), String> {
        if self.drw.is_some() { self.save_drawing_ui(false) } else { self.run_unprompted("file.save") }
    }

    /// Runs a command without asking to save first.
    pub(crate) fn run_unprompted(&mut self, id: &str) -> Result<(), String> {
        // A button whose milestone has not come says so, whatever environment it is pressed in.
        if let Some(c) = commands::find(id).filter(|c| !c.available()) {
            return Err(c.not_yet());
        }
        let export_2d = matches!(id, "export.pdf" | "export.svg" | "export.dxf");
        if id.starts_with("drw.") || id == "file.new_drawing" || id == "file.new_drawing_template" || export_2d {
            return self.run_drw_ui(id);
        }
        if self.in_drawing() {
            match id {
                "edit.undo" | "edit.redo" => return self.drw_undo(id == "edit.undo"),
                "file.save" | "file.save_as" => return self.save_drawing_ui(id == "file.save_as"),
                "view.fit" | "view.home" => {
                    self.fit_sheet();
                    return Ok(());
                }
                "model.rebuild" => return self.run_drw_ui("drw.update"),
                "ui.cancel" => {
                    if let Some(d) = self.drw.as_mut() {
                        d.tool = None;
                    }
                    self.set_status("Ready");
                    return Ok(());
                }
                _ if ["sketch.", "model.", "work.", "asm.", "export."].iter().any(|p| id.starts_with(p))
                    || ["inspect.measure", "inspect.mass", "tools.parameters", "view.style", "view.look_at", "view.previous"].contains(&id) =>
                {
                    return Err("that works on a part or an assembly: select a view and use Open Model to edit its model".into());
                }
                _ => {}
            }
        }
        if id.starts_with("asm.") || id == "file.new_assembly" {
            return self.run_asm_ui(id);
        }
        // In an assembly (no part edited in place), part commands have nothing to work on.
        if self.in_assembly() {
            match id {
                "edit.undo" | "edit.redo" => {
                    let undo = id == "edit.undo";
                    let a = self.asm.as_mut().ok_or("no assembly")?;
                    if !(if undo { a.session.undo() } else { a.session.redo() }) {
                        return Err(format!("nothing to {}", if undo { "undo" } else { "redo" }));
                    }
                    a.selected.retain(|c| a.session.assembly().component(*c).is_some());
                    self.panel = None;
                    return Ok(());
                }
                "export.step" => {
                    let name = format!("{}.step", self.asm.as_ref().map_or("Assembly1".into(), |a| a.session.assembly().name.clone()));
                    let path = self.services.pick_save.as_ref().and_then(|f| f(&name, "step")).ok_or("no file chosen")?;
                    return self.export_assembly_step(&path);
                }
                "model.rebuild" => return self.run_asm_ui("asm.update"),
                _ if ["sketch.", "model.", "work."].iter().any(|p| id.starts_with(p)) || id == "inspect.measure" || id == "tools.parameters" => {
                    return Err("that works on a part: double-click a component to edit its part in place".into());
                }
                _ => {}
            }
        }
        match id {
            "view.browser" => self.chrome.show_browser = !self.chrome.show_browser,
            "view.cube" => self.chrome.show_cube = !self.chrome.show_cube,
            "view.navbar" => self.chrome.show_navbar = !self.chrome.show_navbar,
            "view.style" => {
                self.view.style = match self.view.style {
                    VisualStyle::ShadedEdges => VisualStyle::Shaded,
                    VisualStyle::Shaded => VisualStyle::Wireframe,
                    VisualStyle::Wireframe => VisualStyle::ShadedEdges,
                };
            }
            "view.style.shaded_edges" => self.view.style = VisualStyle::ShadedEdges,
            "view.style.shaded" => self.view.style = VisualStyle::Shaded,
            "view.style.wireframe" => self.view.style = VisualStyle::Wireframe,
            "view.orthographic" => self.view.camera.projection = tenon_render::Projection::Orthographic,
            "view.perspective" => self.view.camera.projection = tenon_render::Projection::Perspective,
            "tools.options" => self.chrome.options = true,
            "model.rebuild" => {
                // Forget what is shown so the whole tree regenerates, from scratch.
                self.shown = None;
                self.shown_key = None;
                self.rebuild_all = true;
                self.set_status("Rebuilding all features...");
            }
            "app.about" => self.chrome.about = true,
            "app.exit" => self.exit = true,
            "inspect.mass" => self.chrome.mass = true,
            "file.new" => {
                self.leave_drawing();
                self.leave_assembly();
                self.session.replace_document(Document::default(), None);
                self.path = None;
                self.mode = Mode::Model;
                self.panel = None;
                self.view = View::default();
                self.set_status("New part.");
            }
            "file.open" => {
                let p = self.services.pick_open.as_ref().and_then(|f| f()).ok_or("no file chosen")?;
                self.open(&p)?;
            }
            "file.save" | "file.save_as" if self.asm.is_some() => {
                let (saved, name) = self.asm.as_ref().map(|a| (a.path.clone(), a.session.assembly().name.clone())).unwrap_or_default();
                let path = match (saved, id) {
                    (Some(p), "file.save") => p,
                    _ => {
                        let file = format!("{name}.{}", tenon_io::asm::EXTENSION);
                        self.services.pick_save.as_ref().and_then(|f| f(&file, tenon_io::asm::EXTENSION)).ok_or("no file chosen")?
                    }
                };
                self.save_assembly(&path)?;
            }
            "file.save" | "file.save_as" => {
                let path = match (&self.path, id) {
                    (Some(p), "file.save") => p.clone(),
                    _ => {
                        let name = format!("{}.tenon", self.document().name);
                        self.services.pick_save.as_ref().and_then(|f| f(&name, "tenon")).ok_or("no file chosen")?
                    }
                };
                self.save(&path)?;
            }
            "export.step" => {
                let name = format!("{}.step", self.document().name);
                let path = self.services.pick_save.as_ref().and_then(|f| f(&name, "step")).ok_or("no file chosen")?;
                self.export_step(&path)?;
            }
            "export.stl" => {
                let name = format!("{}.stl", self.document().name);
                let path = self.services.pick_save.as_ref().and_then(|f| f(&name, "stl")).ok_or("no file chosen")?;
                self.export_stl(&path)?;
            }
            "edit.undo" => {
                if !self.session.undo() {
                    return Err("nothing to undo".into());
                }
                self.after_undo();
            }
            "edit.redo" => {
                if !self.session.redo() {
                    return Err("nothing to redo".into());
                }
                self.after_undo();
            }
            "sketch.new" => self.new_sketch()?,
            "sketch.finish" => self.finish_sketch(),
            "model.extrude" => self.open_extrude(None)?,
            "model.revolve" => self.open_revolve(None)?,
            "model.fillet" => self.open_modify("fillet", None)?,
            "model.chamfer" => self.open_modify("chamfer", None)?,
            "model.shell" => self.open_modify("shell", None)?,
            "model.hole" => self.open_hole(None)?,
            "model.rib" => self.open_rib(None)?,
            "model.pattern.rect" => self.open_pattern(crate::panels::CopyKind::Rect, None)?,
            "model.pattern.circular" => self.open_pattern(crate::panels::CopyKind::Circular, None)?,
            "model.mirror" => self.open_pattern(crate::panels::CopyKind::Mirror, None)?,
            "tools.parameters" => self.chrome.params = !self.chrome.params,
            "inspect.measure" => {
                self.panel = Some(Panel::Measure(Box::default()));
                self.set_status("Measure: click a face or edge, then another for the distance and angle.");
            }
            "work.plane" => self.open_work(crate::work::WorkMethod::Offset, None)?,
            "work.axis" => self.open_work(crate::work::WorkMethod::Along, None)?,
            "work.point" => self.open_work(crate::work::WorkMethod::Center, None)?,
            "ui.ok" | "ui.cancel" => {
                let ok = id == "ui.ok";
                if self.panel.is_some() {
                    self.panel_request = Some(if ok { crate::panels::PanelRequest::Ok } else { crate::panels::PanelRequest::Cancel });
                } else if let Mode::Sketch(s) = &mut self.mode {
                    // Ends the running sketch tool, like Esc.
                    s.clicks.clear();
                    s.picks.clear();
                    s.tool = crate::sketcher::Tool::Select;
                    self.set_status("Ready");
                }
            }
            "ui.repeat" => {
                let last = self.last_command.ok_or("there is no command to repeat")?;
                self.run_ui(last)?;
            }
            "view.home" => self.home_view(),
            "view.fit" => self.zoom_all(),
            "view.previous" => self.previous_view()?,
            "view.look_at" => self.look_at()?,
            "view.orbit" | "view.pan" | "view.zoom" => self.view.toggle_nav(id),
            _ if id.starts_with("sketch.") => self.sketch_tool(id)?,
            _ => {
                return Err(match commands::find(id) {
                    Some(c) if !c.available() => c.not_yet(),
                    _ => format!("unknown command `{id}`"),
                });
            }
        }
        Ok(())
    }

    fn after_undo(&mut self) {
        // The sketch being edited may be gone.
        if let Mode::Sketch(s) = &self.mode
            && self.document().sketch(s.feature).is_none()
        {
            self.mode = Mode::Model;
        }
        self.panel = None;
    }

    /// Opens a part (`.tenon`), an assembly (`.tenonasm`) or a drawing (`.tenondrw`).
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        if path.extension().is_some_and(|e| e == tenon_io::asm::EXTENSION) {
            return self.open_assembly(path);
        }
        if path.extension().is_some_and(|e| e == tenon_io::drw::EXTENSION) {
            return self.open_drawing(path);
        }
        // Read first: a file that cannot be opened leaves the assembly as it was.
        tenon_io::project::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        self.leave_drawing();
        self.leave_assembly();
        self.exec("file.open", json!({ "path": path.to_string_lossy() })).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        self.path = Some(path.to_path_buf());
        self.mode = Mode::Model;
        self.panel = None;
        self.view = View::default();
        self.set_status(format!("Opened {}", path.display()));
        Ok(())
    }

    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        let r = self.exec("file.save", json!({ "path": path.to_string_lossy() }))?;
        self.path = Some(path.to_path_buf());
        self.set_status(format!("Saved {}{}", path.display(), kept_note(&r)));
        Ok(())
    }

    /// Asks for a measurement of the open Measure panel's picks.
    pub(crate) fn request_measure(&mut self) {
        let Some(Panel::Measure(m)) = &self.panel else { return };
        let entity = |p: &crate::viewport::Pick| match *p {
            crate::viewport::Pick::Face { body, face } => tenon_model::measure::Entity::Face { body, face },
            crate::viewport::Pick::Edge { body, edge } => tenon_model::measure::Entity::Edge { body, edge },
        };
        let Some(a) = m.a.as_ref().map(entity) else { return };
        let b = m.b.as_ref().map(entity);
        self.step_seq += 1;
        let id = self.step_seq;
        let result = match &mut self.geo {
            Geo::Worker(w) => {
                w.measure(id, a, b);
                None
            }
            Geo::Sync(k) => Some(tenon_model::measure::measure(k.as_ref(), &self.sync_regen, a, b)),
            Geo::Off => Some(Err("there is no geometry kernel".into())),
        };
        if let Some(Panel::Measure(m)) = &mut self.panel {
            m.pending = result.is_none().then_some(id);
            m.result = result;
        }
    }

    fn export_step(&mut self, path: &Path) -> Result<(), String> {
        match &mut self.geo {
            Geo::Worker(w) => {
                // Separate numbering: regeneration results are matched against `seq`.
                self.step_seq += 1;
                w.export_step(self.step_seq, self.session.document().clone());
                self.step_export = Some((self.step_seq, path.to_path_buf()));
                self.set_status("Exporting STEP...");
                Ok(())
            }
            Geo::Sync(_) => {
                self.exec("export.step", json!({ "path": path.to_string_lossy() }))?;
                self.set_status(format!("Exported {}", path.display()));
                Ok(())
            }
            Geo::Off => Err("no geometry kernel".into()),
        }
    }

    fn export_stl(&mut self, path: &Path) -> Result<(), String> {
        if self.scene.bodies.is_empty() {
            return Err("there is no solid to export".into());
        }
        let mut all = tenon_kernel::Mesh::default();
        for b in &self.scene.bodies {
            let base = u32::try_from(all.positions.len()).map_err(|_| "mesh too large")?;
            all.positions.extend_from_slice(&b.mesh.positions);
            all.indices.extend(b.mesh.indices.iter().map(|i| i.saturating_add(base)));
        }
        let data = tenon_io::stl::write_binary(&all, &format!("Tenon {} (mm)", self.document().name));
        std::fs::write(path, data).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        self.set_status(format!("Exported {}", path.display()));
        Ok(())
    }

    /// Sends the shown document (or the assembly's parts) for regeneration when it changed;
    /// takes finished results.
    fn sync_geometry(&mut self) {
        if !self.in_assembly() && !self.in_drawing() {
            self.sync_part();
        }
        let mut responses = Vec::new();
        if let Geo::Worker(w) = &self.geo {
            while let Some(r) = w.try_recv() {
                responses.push(r);
            }
        }
        let assembly = self.in_assembly();
        for r in responses {
            match r {
                Response::Scene { slot, revision, scene } if slot != 0 => {
                    self.asm_scene(slot, revision, *scene);
                }
                Response::Scene { revision, scene, .. } if revision == self.seq && !assembly => {
                    self.waiting = false;
                    self.set_scene(*scene);
                }
                Response::Failed { slot, revision, message } if slot == 0 && revision == self.seq && !assembly => {
                    self.waiting = false;
                    self.regen_note = Some(message);
                }
                Response::Failed { slot, message, .. } if slot != 0 => {
                    self.regen_note = Some(message);
                }
                Response::Step { request, result } => {
                    if let Some((id, path)) = self.step_export.take() {
                        if id != request {
                            self.step_export = Some((id, path));
                            continue;
                        }
                        match result.and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string())) {
                            Ok(()) => self.set_status(format!("Exported {}", path.display())),
                            Err(e) => self.set_error(format!("STEP export failed: {e}")),
                        }
                    }
                }
                Response::Measure { request, result } => {
                    if let Some(Panel::Measure(m)) = &mut self.panel
                        && m.pending == Some(request)
                    {
                        m.pending = None;
                        m.result = Some(result);
                    }
                }
                Response::Job { request, result } => {
                    if self.owns_drw_job(request) {
                        self.drw_job(request, result);
                    } else {
                        self.asm_job(request, result);
                    }
                }
                _ => {}
            }
        }
        if self.asm.is_some() {
            self.sync_assembly();
        }
        self.sync_drawing();
    }

    /// Sends the part document for regeneration when it changed.
    fn sync_part(&mut self) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.session.revision().hash(&mut h);
        format!("{:?}", self.panel).hash(&mut h);
        // Opening or finishing a sketch changes what is shown (see SketchMode::base).
        match &self.mode {
            Mode::Sketch(s) => Some(s.feature).hash(&mut h),
            Mode::Model => None::<FeatureId>.hash(&mut h),
        }
        let key = h.finish();
        let changed = self.shown_key != Some(key);
        self.shown_key = Some(key);
        let doc = if changed {
            let mut d = self.preview_document().unwrap_or_else(|| self.session.document().clone());
            // While a sketch is open the part shows it as it was opened (see SketchMode::base).
            if let Mode::Sketch(s) = &self.mode
                && let (Some(base), Some(sk)) = (&s.base, d.sketch_mut(s.feature))
            {
                *sk = base.clone();
            }
            Some(d)
        } else {
            None
        };
        if let Some(doc) = doc.filter(|d| self.shown.as_ref() != Some(d)) {
            self.seq += 1;
            let fresh = std::mem::take(&mut self.rebuild_all);
            match &mut self.geo {
                Geo::Worker(w) => {
                    // The document itself unchanged (a value being dragged or typed into a
                    // panel): a preview, which does not cut short the one being computed.
                    let edit = self.sent_revision != Some(self.session.revision());
                    if edit || fresh {
                        w.regenerate(self.seq, doc.clone(), fresh);
                    } else {
                        w.preview(self.seq, doc.clone());
                    }
                    self.sent_revision = Some(self.session.revision());
                    self.waiting = true;
                }
                Geo::Sync(k) => {
                    if fresh {
                        self.sync_cache.release(k.as_mut());
                    }
                    let r = regenerate_with(&doc, k.as_mut(), Some(&mut self.sync_cache));
                    let s = scene(&r, k.as_mut(), &MeshTol::default());
                    // Kept for measuring, like the worker keeps its result.
                    let mut old = std::mem::replace(&mut self.sync_regen, r);
                    old.release(k.as_mut());
                    match s {
                        Ok(s) => self.set_scene(s),
                        Err(e) => self.regen_note = Some(e),
                    }
                }
                Geo::Off => {}
            }
            self.shown = Some(doc);
        }
    }

    fn set_scene(&mut self, s: Scene) {
        self.scene = s;
        self.scene_seq += 1;
        self.regen_note = None;
        if !self.view.fitted && !self.scene.bodies.is_empty() {
            self.fit_view();
            self.view.fitted = true;
        }
        let errors: Vec<String> = self
            .scene
            .status
            .iter()
            .filter_map(|(f, st)| match st {
                FeatureStatus::Error { message } => Some(format!("{}: {message}", self.feature_name(*f))),
                _ => None,
            })
            .collect();
        match errors.first() {
            Some(e) => {
                self.set_error(e.clone());
                self.failure_shown = Some(e.clone());
            }
            // The failure it showed is gone (fixed or undone): so is its message, unless
            // something else has been said since.
            None => {
                if self.failure_shown.take().is_some_and(|m| m == self.status) {
                    self.set_status("Ready");
                }
            }
        }
        self.view.selection.retain(|p| p.valid_in(&self.scene));
        self.view.hover = None;
    }

    pub(crate) fn feature_name(&self, id: FeatureId) -> String {
        self.document().feature(id).map_or_else(|| id.to_string(), |f| f.name.clone())
    }

    /// Draws the whole window. `render` is the wgpu state when the app renders with wgpu.
    pub fn ui(&mut self, ui: &mut Ui, render: Option<&egui_wgpu::RenderState>) {
        // While "Save changes?" or "Recover unsaved work?" is open, keys are for it alone (Delete
        // must not delete what it asks about); they are handed back just before the prompts are
        // drawn.
        let held: Vec<egui::Event> = if self.chrome.save_prompt.is_some() || self.recover_offer.is_some() {
            ui.input_mut(|i| {
                let (keys, rest) =
                    std::mem::take(&mut i.events).into_iter().partition(|e| matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
                i.events = rest;
                keys
            })
        } else {
            Vec::new()
        };
        self.view.now = ui.input(|i| i.time);
        self.sync_geometry();
        if self.in_assembly() {
            self.refresh_dof();
        }
        // Parameter values, for value fields that take equations.
        if self.param_env.0 != self.session.revision() {
            self.param_env = (self.session.revision(), std::sync::Arc::new(self.document().parameter_values()));
        }
        crate::properties::set_param_env(ui.ctx(), self.param_env.1.clone());
        self.shortcuts(ui);
        let theme = self.chrome.theme;
        if self.chrome.applied_theme != Some(theme) {
            theme::apply_theme(ui.ctx(), theme);
            self.chrome.applied_theme = Some(theme);
        }
        let t = Tokens::of(theme);
        egui::Panel::top("tn_title").exact_size(TITLE_H).frame(Frame::NONE.fill(t.title_bar)).show(ui, |ui| self.title_bar(ui, &t));
        egui::Panel::top("tn_tabs").exact_size(TABS_H).frame(Frame::NONE.fill(t.tab_strip)).show(ui, |ui| self.ribbon_tabs(ui, &t));
        egui::Panel::top("tn_ribbon").exact_size(RIBBON_H).frame(Frame::NONE.fill(t.ribbon)).show(ui, |ui| self.ribbon(ui, &t));
        egui::Panel::bottom("tn_status").exact_size(STATUS_H).frame(Frame::NONE.fill(t.status_bar)).show(ui, |ui| self.status_bar(ui, &t));
        egui::Panel::bottom("tn_docs").exact_size(DOC_TABS_H).frame(Frame::NONE.fill(t.tab_strip)).show(ui, |ui| self.doc_tabs(ui, &t));
        // Left column: the properties panel of a running feature command above the browser.
        let props = self.has_properties();
        if self.chrome.show_browser || props {
            egui::Panel::left("tn_browser").default_size(262.0).size_range(200.0..=480.0).resizable(true).frame(Frame::NONE.fill(t.panel)).show(
                ui,
                |ui| {
                    let full = ui.max_rect();
                    let split = match (props, self.chrome.show_browser) {
                        (true, true) => full.top() + full.height() * 0.58,
                        (true, false) => full.bottom(),
                        _ => full.top(),
                    };
                    if props {
                        let r = egui::Rect::from_min_max(full.min, egui::pos2(full.right(), split));
                        ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                            ui.set_clip_rect(r);
                            self.properties(ui, &t);
                        });
                    }
                    if self.chrome.show_browser {
                        let r = egui::Rect::from_min_max(egui::pos2(full.left(), split), full.max);
                        ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                            ui.set_clip_rect(r);
                            if self.in_drawing() {
                                self.drw_browser(ui, &t);
                            } else if self.in_assembly() {
                                self.asm_browser(ui, &t);
                            } else {
                                self.browser(ui, &t);
                            }
                        });
                    }
                },
            );
        }
        egui::CentralPanel::default().frame(Frame::NONE.fill(t.viewport_bottom)).show(ui, |ui| self.viewport(ui, render, &t));
        self.file_menu(ui, &t);
        self.dropdown(ui, &t);
        self.panels(ui);
        self.windows(ui);
        self.asm_windows(ui);
        self.drw_windows(ui);
        self.radial_ui(ui, &t);
        ui.input_mut(|i| i.events.extend(held));
        self.save_prompt(ui);
        self.recover_prompt(ui);
        self.autosave();
        if self.waiting || self.is_busy() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    fn shortcuts(&mut self, ui: &Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (cmd, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |k: egui::Key| ui.input(|i| i.key_pressed(k));
        if pressed(egui::Key::F5) {
            self.command("view.previous");
        } else if pressed(egui::Key::F6) {
            self.command("view.home");
        }
        if cmd {
            if pressed(egui::Key::Z) {
                self.command(if shift { "edit.redo" } else { "edit.undo" });
            } else if pressed(egui::Key::Y) {
                self.command("edit.redo");
            } else if pressed(egui::Key::S) {
                self.command("file.save");
            } else if pressed(egui::Key::O) {
                self.command("file.open");
            } else if pressed(egui::Key::N) {
                self.command("file.new");
            }
        } else {
            // Single-letter shortcuts (E extrude, S sketch, L line, D dimension, ...).
            let letter = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } if modifiers.is_none() => Some(*key),
                    _ => None,
                })
            });
            let env = if self.in_drawing() {
                commands::Env::Drawing
            } else if self.in_assembly() {
                commands::Env::Assembly
            } else {
                commands::Env::Part { sketching: self.is_sketching() }
            };
            if let Some(k) = letter
                && let Some(c) = commands::for_key(k.name(), env)
            {
                self.command(c.id);
            }
        }
    }

    /// Opens a feature for editing: a sketch in sketch mode, others in their panel.
    pub(crate) fn edit_feature(&mut self, id: FeatureId) -> Result<(), String> {
        match self.document().feature(id).map(|f| &f.kind) {
            Some(FeatureKind::Sketch { .. }) => self.enter_sketch(id),
            Some(FeatureKind::Extrude(_)) => self.open_extrude(Some(id)),
            Some(FeatureKind::Revolve(_)) => self.open_revolve(Some(id)),
            Some(FeatureKind::Fillet(_)) => self.open_modify("fillet", Some(id)),
            Some(FeatureKind::Chamfer(_)) => self.open_modify("chamfer", Some(id)),
            Some(FeatureKind::Shell(_)) => self.open_modify("shell", Some(id)),
            Some(FeatureKind::Hole(_)) => self.open_hole(Some(id)),
            Some(FeatureKind::Rib(_)) => self.open_rib(Some(id)),
            Some(FeatureKind::PatternRect(_)) => self.open_pattern(crate::panels::CopyKind::Rect, Some(id)),
            Some(FeatureKind::PatternCircular(_)) => self.open_pattern(crate::panels::CopyKind::Circular, Some(id)),
            Some(FeatureKind::Mirror(_)) => self.open_pattern(crate::panels::CopyKind::Mirror, Some(id)),
            Some(FeatureKind::WorkPlane(_) | FeatureKind::WorkAxis(_) | FeatureKind::WorkPoint(_)) => {
                self.open_work(crate::work::WorkMethod::Offset, Some(id))
            }
            None => Err(format!("{id} does not exist")),
        }
    }

    /// Creates a sketch with `params` (plane or face) and starts editing it.
    pub(crate) fn create_sketch(&mut self, params: Value) -> Result<(), String> {
        let r = self.exec("sketch.create", params)?;
        let id = r["feature"].as_u64().and_then(|v| u32::try_from(v).ok()).ok_or("internal: no feature id")?;
        self.panel = None;
        self.pick_plane = false;
        self.enter_sketch(FeatureId(id))?;
        if let Some(tool) = self.pending_tool.take() {
            self.sketch_tool(tool)?;
        }
        Ok(())
    }

    fn new_sketch(&mut self) -> Result<(), String> {
        if let Some(face_ref) = self.selected_face_ref() {
            return self.create_sketch(json!({ "face": face_ref }));
        }
        // Show the origin planes in the viewport and wait for a plane or planar face.
        self.panel = None;
        self.pick_plane = true;
        self.chrome.origin_open = true;
        self.set_status("Select a plane to create a sketch on, or a planar face of the part (Esc cancels).");
        Ok(())
    }

    /// The plane frame of a sketch feature (from the last regeneration, or the origin plane).
    pub(crate) fn sketch_frame(&self, id: FeatureId) -> Option<tenon_geom::Frame> {
        if let Some(f) = self.scene.sketch_frames.get(&id) {
            return Some(*f);
        }
        match &self.document().feature(id)?.kind {
            FeatureKind::Sketch { plane: PlaneRef::Origin(p), .. } => Some(p.frame()),
            _ => None,
        }
    }

    /// Ids of the sketches in the document, most recent last.
    pub(crate) fn sketches(&self) -> Vec<(FeatureId, String)> {
        self.document().features().iter().filter(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| (f.id, f.name.clone())).collect()
    }
}

/// " Kept the version-1 original as plate.v1.tenon." when a save upgraded a file from format
/// version 1 (older Tenon builds can still open the copy), from a save command's result.
pub(crate) fn kept_note(r: &Value) -> String {
    let names: Vec<String> = r["kept_version_1"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|p| Path::new(p).file_name().map_or_else(|| p.to_owned(), |n| n.to_string_lossy().into_owned()))
        .collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => format!(". Kept the version-1 original as {one}"),
        many => format!(". Kept the version-1 originals as {}", many.join(", ")),
    }
}
