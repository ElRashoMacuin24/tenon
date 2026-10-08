//! The part workbench: one document, its regeneration, the 3D view, sketch mode and panels.
//!
//! Model edits go through the shared command registry (`tenon_io::cmd::run`), the same commands
//! the CLI and MCP server use. Regeneration runs on the worker thread; the last good scene stays on
//! screen while a new one is computed or when it fails.

use std::path::{Path, PathBuf};

use egui::{Align2, Frame, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};
use tenon_kernel::{Kernel, MeshTol};
use tenon_model::worker::{Response, Worker};
use tenon_model::{Document, FeatureId, FeatureKind, FeatureStatus, OriginPlane, PlaneRef, Scene, Session, regenerate, scene};

use crate::chrome::{Chrome, DOC_TABS_H, RIBBON_H, STATUS_H, TABS_H, TITLE_H};
use crate::commands;
use crate::icons::{self, Icon};
use crate::panels::Panel;
use crate::sketcher::SketchMode;
use crate::theme::{self, Tokens};
use crate::viewport::View;

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
        if let Err(e) = self.run_ui(id) {
            self.set_error(e);
        }
    }

    /// Ribbon, menu and toolbar commands.
    pub fn run_ui(&mut self, id: &str) -> Result<(), String> {
        match id {
            "view.browser" => self.chrome.show_browser = !self.chrome.show_browser,
            "view.cube" => self.chrome.show_cube = !self.chrome.show_cube,
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
            "view.home" => {
                self.view.camera.set_view(tenon_render::StdView::Home);
                self.fit_view();
            }
            "view.fit" => self.fit_view(),
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
        self.sync_geometry();
        self.shortcuts(ui);
        let t = Tokens::DARK;
        egui::Panel::top("tn_title").exact_size(TITLE_H).frame(Frame::NONE.fill(t.title_bar)).show(ui, |ui| self.title_bar(ui, &t));
        egui::Panel::top("tn_tabs").exact_size(TABS_H).frame(Frame::NONE.fill(t.tab_strip)).show(ui, |ui| self.ribbon_tabs(ui, &t));
        egui::Panel::top("tn_ribbon").exact_size(RIBBON_H).frame(Frame::NONE.fill(t.ribbon)).show(ui, |ui| self.ribbon(ui, &t));
        egui::Panel::bottom("tn_status").exact_size(STATUS_H).frame(Frame::NONE.fill(t.title_bar)).show(ui, |ui| self.status_bar(ui, &t));
        egui::Panel::bottom("tn_docs").exact_size(DOC_TABS_H).frame(Frame::NONE.fill(t.tab_strip)).show(ui, |ui| self.doc_tabs(ui, &t));
        if self.chrome.show_browser {
            egui::Panel::left("tn_browser")
                .default_size(250.0)
                .size_range(180.0..=480.0)
                .resizable(true)
                .frame(Frame::NONE.fill(t.panel))
                .show(ui, |ui| self.browser(ui, &t));
        }
        egui::CentralPanel::default().frame(Frame::NONE.fill(t.viewport_bottom)).show(ui, |ui| self.viewport(ui, render, &t));
        self.file_menu(ui, &t);
        self.panels(ui);
        self.windows(ui);
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
        }
    }

    // ---- browser ----------------------------------------------------------------------------

    fn browser(&mut self, ui: &mut Ui, t: &Tokens) {
        let r = ui.max_rect();
        let header = Rect::from_min_size(r.min, vec2(r.width(), 26.0));
        ui.painter().rect_filled(header, 0.0, t.panel_header);
        ui.painter().text(pos2(header.left() + 10.0, header.center().y), Align2::LEFT_CENTER, "Model", theme::heading(), t.text);
        ui.painter().hline(header.x_range(), header.bottom(), Stroke::new(1.0, t.border));
        ui.add_space(30.0);
        let mut action: Option<BrowserAction> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let name = self.document().name.clone();
            row(ui, t, 0, Icon::Part, &name, None, RowStyle::Normal, "part");
            let bodies = self.scene.bodies.len();
            row(ui, t, 1, Icon::Folder, &format!("Solid Bodies ({bodies})"), None, RowStyle::Normal, "bodies");
            let origin = row(ui, t, 1, Icon::Folder, "Origin", Some(self.chrome.origin_open), RowStyle::Normal, "origin");
            if origin.clicked() {
                self.chrome.origin_open = !self.chrome.origin_open;
            }
            if self.chrome.origin_open {
                for (icon, label, plane) in [
                    (Icon::Plane, "YZ Plane", Some(OriginPlane::YZ)),
                    (Icon::Plane, "XZ Plane", Some(OriginPlane::XZ)),
                    (Icon::Plane, "XY Plane", Some(OriginPlane::XY)),
                    (Icon::Axis, "X Axis", None),
                    (Icon::Axis, "Y Axis", None),
                    (Icon::Axis, "Z Axis", None),
                    (Icon::Point, "Center Point", None),
                ] {
                    let resp = row(ui, t, 2, icon, label, None, RowStyle::Normal, label);
                    if let Some(p) = plane {
                        let resp = resp.on_hover_text("Double-click to start a sketch on this plane");
                        if resp.double_clicked() || (resp.clicked() && matches!(self.panel, Some(Panel::NewSketch))) {
                            action = Some(BrowserAction::SketchOn(p));
                        }
                    }
                }
            }
            let editing = match &self.mode {
                Mode::Sketch(s) => Some(s.feature),
                Mode::Model => None,
            };
            let features: Vec<(FeatureId, String, &'static str, bool)> =
                self.document().features().iter().map(|f| (f.id, f.name.clone(), f.kind.type_name(), f.suppressed)).collect();
            for (id, name, ty, suppressed) in features {
                let icon = match ty {
                    "Sketch" => Icon::NewSketch,
                    "Extrude" => Icon::Extrude,
                    _ => Icon::Revolve,
                };
                let status = self.scene.status.iter().find(|(f, _)| *f == id).map(|(_, s)| s.clone());
                let style = match (&status, suppressed, editing == Some(id)) {
                    (_, _, true) => RowStyle::Active,
                    (_, true, _) => RowStyle::Dim,
                    (Some(FeatureStatus::Error { .. }), _, _) => RowStyle::Error,
                    (Some(FeatureStatus::NotComputed), _, _) => RowStyle::Dim,
                    _ => RowStyle::Normal,
                };
                let mut resp = row(ui, t, 1, icon, &name, None, style, &format!("f{}", id.0));
                if let Some(FeatureStatus::Error { message }) = &status {
                    resp = resp.on_hover_text(message.clone());
                }
                if resp.double_clicked() {
                    action = Some(BrowserAction::Edit(id));
                }
                resp.context_menu(|ui| {
                    if ui.button("Edit").clicked() {
                        action = Some(BrowserAction::Edit(id));
                        ui.close();
                    }
                    if ui.button("Rename").clicked() {
                        action = Some(BrowserAction::Rename(id));
                        ui.close();
                    }
                    if ui.button(if suppressed { "Unsuppress" } else { "Suppress" }).clicked() {
                        action = Some(BrowserAction::Suppress(id, !suppressed));
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        action = Some(BrowserAction::Delete(id));
                        ui.close();
                    }
                });
            }
            let marker = row(ui, t, 1, Icon::FinishSketch, "End of history", None, RowStyle::Marker, "end");
            let _ = marker.on_hover_text("Rollback marker: features below it are not computed. Dragging it arrives in milestone M2.");
        });
        if let Some(a) = action {
            self.browser_action(a);
        }
    }

    fn browser_action(&mut self, a: BrowserAction) {
        let result = match a {
            BrowserAction::SketchOn(p) => self.create_sketch(json!({ "plane": format!("{p:?}").to_lowercase() })),
            BrowserAction::Edit(id) => self.edit_feature(id),
            BrowserAction::Rename(id) => {
                self.panel = Some(Panel::Rename { feature: id, name: self.feature_name(id) });
                Ok(())
            }
            BrowserAction::Suppress(id, on) => self.exec("feature.suppress", json!({ "feature": id.0, "suppressed": on })).map(|_| ()),
            BrowserAction::Delete(id) => {
                if matches!(&self.mode, Mode::Sketch(s) if s.feature == id) {
                    self.mode = Mode::Model;
                }
                self.exec("feature.delete", json!({ "feature": id.0 })).map(|_| ())
            }
        };
        if let Err(e) = result {
            self.set_error(e);
        }
    }

    /// Opens a feature for editing: a sketch in sketch mode, others in their panel.
    pub(crate) fn edit_feature(&mut self, id: FeatureId) -> Result<(), String> {
        match self.document().feature(id).map(|f| &f.kind) {
            Some(FeatureKind::Sketch { .. }) => self.enter_sketch(id),
            Some(FeatureKind::Extrude(_)) => self.open_extrude(Some(id)),
            Some(FeatureKind::Revolve(_)) => self.open_revolve(Some(id)),
            None => Err(format!("{id} does not exist")),
        }
    }

    /// Creates a sketch with `params` (plane or face) and starts editing it.
    pub(crate) fn create_sketch(&mut self, params: Value) -> Result<(), String> {
        let r = self.exec("sketch.create", params)?;
        let id = r["feature"].as_u64().and_then(|v| u32::try_from(v).ok()).ok_or("internal: no feature id")?;
        self.panel = None;
        self.enter_sketch(FeatureId(id))
    }

    fn new_sketch(&mut self) -> Result<(), String> {
        if let Some(face_ref) = self.selected_face_ref() {
            return self.create_sketch(json!({ "face": face_ref }));
        }
        self.panel = Some(Panel::NewSketch);
        self.set_status("Choose a plane for the sketch, or select a planar face first.");
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

enum BrowserAction {
    SketchOn(OriginPlane),
    Edit(FeatureId),
    Rename(FeatureId),
    Suppress(FeatureId, bool),
    Delete(FeatureId),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowStyle {
    Normal,
    Active,
    Dim,
    Error,
    Marker,
}

/// One browser row. `expand` draws a disclosure triangle.
#[allow(clippy::too_many_arguments)]
fn row(ui: &mut Ui, t: &Tokens, depth: u8, icon: Icon, label: &str, expand: Option<bool>, style: RowStyle, key: &str) -> egui::Response {
    let (rr, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 21.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::Default);
    let _ = key;
    if style == RowStyle::Active {
        ui.painter().rect_filled(rr, 0.0, t.pressed);
    } else if resp.hovered() {
        ui.painter().rect_filled(rr, 0.0, t.hover);
    }
    let indent = rr.left() + 8.0 + f32::from(depth) * 16.0;
    if let Some(open) = expand {
        let c = pos2(indent + 5.0, rr.center().y);
        let tri = if open {
            vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
        } else {
            vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)]
        };
        ui.painter().add(egui::Shape::convex_polygon(tri, t.text_dim, Stroke::NONE));
    }
    let x = indent + 14.0;
    let color = match style {
        RowStyle::Normal | RowStyle::Active => t.text,
        RowStyle::Dim => t.text_disabled,
        RowStyle::Error | RowStyle::Marker => t.history_marker,
    };
    if style == RowStyle::Marker {
        ui.painter().rect_filled(Rect::from_min_size(pos2(x, rr.center().y - 3.0), vec2(16.0, 6.0)), 1.0, t.history_marker);
    } else {
        icons::paint(
            ui.painter(),
            Rect::from_min_size(pos2(x, rr.center().y - 8.0), vec2(16.0, 16.0)),
            icon,
            if style == RowStyle::Error { color } else { t.icon },
        );
    }
    let text = if style == RowStyle::Error { format!("{label}  (!)") } else { label.to_owned() };
    ui.painter().text(pos2(x + 22.0, rr.center().y), Align2::LEFT_CENTER, text, theme::small(), color);
    resp
}
