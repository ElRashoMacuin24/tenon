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
use tenon_model::{Document, FeatureId, FeatureKind, FeatureStatus, PlaneRef, Scene, Session, regenerate, scene};

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

/// The part workbench.
pub struct Workbench {
    pub(crate) chrome: Chrome,
    pub(crate) session: Session,
    geo: Geo,
    pub(crate) scene: Scene,
    pub(crate) scene_seq: u64,
    shown: Option<Document>,
    seq: u64,
    pub(crate) waiting: bool,
    pub(crate) regen_note: Option<String>,
    pub(crate) view: View,
    pub(crate) mode: Mode,
    pub(crate) panel: Option<Panel>,
    pub(crate) status: String,
    pub(crate) status_error: bool,
    pub(crate) path: Option<PathBuf>,
    services: Services,
    exit: bool,
    step_seq: u64,
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
            seq: 0,
            waiting: false,
            regen_note: None,
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
        self.waiting || self.step_export.is_some()
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

    /// Ribbon, menu and toolbar commands.
    pub fn run_ui(&mut self, id: &str) -> Result<(), String> {
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
                // Forget what is shown so the whole tree regenerates.
                self.shown = None;
                self.set_status("Rebuilding all features...");
            }
            "app.about" => self.chrome.about = true,
            "app.exit" => self.exit = true,
            "inspect.mass" => self.chrome.mass = true,
            "file.new" => {
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
            "model.pattern.rect" => self.open_pattern(crate::panels::CopyKind::Rect, None)?,
            "model.pattern.circular" => self.open_pattern(crate::panels::CopyKind::Circular, None)?,
            "model.mirror" => self.open_pattern(crate::panels::CopyKind::Mirror, None)?,
            "tools.parameters" => self.chrome.params = !self.chrome.params,
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
                    Some(c) if !c.available() => format!("{} is not available yet: it arrives in milestone M{}.", c.label, c.milestone),
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

    /// Opens a project file.
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        self.exec("file.open", json!({ "path": path.to_string_lossy() })).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        self.path = Some(path.to_path_buf());
        self.mode = Mode::Model;
        self.panel = None;
        self.view = View::default();
        self.set_status(format!("Opened {}", path.display()));
        Ok(())
    }

    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        self.exec("file.save", json!({ "path": path.to_string_lossy() }))?;
        self.path = Some(path.to_path_buf());
        self.set_status(format!("Saved {}", path.display()));
        Ok(())
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

    /// Sends the shown document for regeneration when it changed; takes finished results.
    fn sync_geometry(&mut self) {
        let doc = self.preview_document().unwrap_or_else(|| self.session.document().clone());
        if self.shown.as_ref() != Some(&doc) {
            self.seq += 1;
            match &mut self.geo {
                Geo::Worker(w) => {
                    w.regenerate(self.seq, doc.clone());
                    self.waiting = true;
                }
                Geo::Sync(k) => {
                    let mut r = regenerate(&doc, k.as_mut());
                    let s = scene(&r, k.as_mut(), &MeshTol::default());
                    r.release(k.as_mut());
                    match s {
                        Ok(s) => self.set_scene(s),
                        Err(e) => self.regen_note = Some(e),
                    }
                }
                Geo::Off => {}
            }
            self.shown = Some(doc);
        }
        let mut responses = Vec::new();
        if let Geo::Worker(w) = &self.geo {
            while let Some(r) = w.try_recv() {
                responses.push(r);
            }
        }
        for r in responses {
            match r {
                Response::Scene { revision, scene } if revision == self.seq => {
                    self.waiting = false;
                    self.set_scene(*scene);
                }
                Response::Failed { revision, message } if revision == self.seq => {
                    self.waiting = false;
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
                _ => {}
            }
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
        if let Some(e) = errors.first() {
            self.set_error(e.clone());
        }
        self.view.selection.retain(|p| p.valid_in(&self.scene));
        self.view.hover = None;
    }

    pub(crate) fn feature_name(&self, id: FeatureId) -> String {
        self.document().feature(id).map_or_else(|| id.to_string(), |f| f.name.clone())
    }

    /// Draws the whole window. `render` is the wgpu state when the app renders with wgpu.
    pub fn ui(&mut self, ui: &mut Ui, render: Option<&egui_wgpu::RenderState>) {
        self.view.now = ui.input(|i| i.time);
        self.sync_geometry();
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
                            self.browser(ui, &t);
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
        self.radial_ui(ui, &t);
        if self.waiting {
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
            if let Some(k) = letter
                && let Some(c) = commands::for_key(k.name(), self.is_sketching())
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
