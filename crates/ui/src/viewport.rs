//! The 3D viewport: drawing (GPU or software), navigation, picking, orientation cube, navigation
//! bar and axis triad.

use std::hash::{Hash, Hasher};

use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use tenon_geom::{Aabb3, Vec3};
use tenon_kernel::SurfaceKind;
use tenon_model::{FaceRef, Fingerprint, Scene};
use tenon_render::gpu::{BodyColors, Viewport};
use tenon_render::pick::{pick_edge, pick_face};
use tenon_render::raster::{self, Style};
use tenon_render::{Camera, Projection};

use crate::chrome::icon_button;
use crate::commands::NAV_BAR;
use crate::theme::{self, Tokens};
use crate::workbench::{Mode, Workbench};

/// Something picked in the 3D view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Pick {
    Face { body: usize, face: u32 },
    Edge { body: usize, edge: u32 },
}

impl Pick {
    pub(crate) fn valid_in(&self, s: &Scene) -> bool {
        match self {
            Pick::Face { body, face } => s.bodies.get(*body).is_some_and(|b| (*face as usize) < b.faces.len()),
            Pick::Edge { body, edge } => s.bodies.get(*body).is_some_and(|b| b.mesh.edges.iter().any(|e| e.edge == *edge)),
        }
    }
}

/// What the left mouse button does in the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Nav {
    Select,
    Orbit,
    Pan,
    Zoom,
}

struct Gpu {
    viewport: Viewport,
    texture: Option<egui::TextureId>,
}

/// How the model is drawn (View > Visual Style).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum VisualStyle {
    #[default]
    ShadedEdges,
    Shaded,
    Wireframe,
}

impl VisualStyle {
    fn faces(self) -> bool {
        self != VisualStyle::Wireframe
    }
    fn edges(self) -> bool {
        self != VisualStyle::Shaded
    }
}

/// A camera move in progress (view changes glide rather than jump).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewAnim {
    pub from: Camera,
    pub to: Camera,
    pub start: f64,
}

/// Length of a view transition in seconds.
pub(crate) const VIEW_TRANSITION: f64 = 0.3;
/// Views remembered for "Previous View".
const VIEW_HISTORY: usize = 30;

/// View state of the workbench.
pub(crate) struct View {
    pub camera: Camera,
    pub fitted: bool,
    pub nav: Nav,
    pub hover: Option<Pick>,
    pub selection: Vec<Pick>,
    pub anim: Option<ViewAnim>,
    pub style: VisualStyle,
    /// Where a selection box started.
    pub box_start: Option<Pos2>,
    /// Home orientation: yaw, pitch, roll.
    pub home: (f64, f64, f64),
    /// Earlier views, most recent last.
    pub previous: Vec<Camera>,
    /// The view to return to when the sketch being edited is finished.
    pub sketch_return: Option<Camera>,
    /// Input time of the current frame (seconds).
    pub now: f64,
    gpu: Option<Gpu>,
    uploaded: Option<u64>,
    soft: Option<(egui::TextureHandle, u64)>,
    /// Last viewport rectangle (points).
    pub rect: Rect,
}

impl Default for View {
    fn default() -> Self {
        let c = Camera::default();
        View {
            camera: c,
            fitted: false,
            nav: Nav::Select,
            hover: None,
            selection: Vec::new(),
            anim: None,
            style: VisualStyle::default(),
            box_start: None,
            home: (c.yaw, c.pitch, c.roll),
            previous: Vec::new(),
            sketch_return: None,
            now: 0.0,
            gpu: None,
            uploaded: None,
            soft: None,
            rect: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
        }
    }
}

/// The selection rectangle: solid when dragged left to right (window: what is fully inside),
/// dashed right to left (crossing: whatever it touches).
fn draw_selection_box(ui: &Ui, a: Pos2, b: Pos2) {
    let r = Rect::from_two_pos(a, b);
    let crossing = b.x < a.x;
    let color = if crossing { Color32::from_rgb(0x6c, 0xd0, 0x7a) } else { Color32::from_rgb(0x5a, 0xa8, 0xf0) };
    let p = ui.painter();
    p.rect_filled(r, 0.0, color.gamma_multiply(0.12));
    let stroke = Stroke::new(1.0, color);
    if crossing {
        let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        p.extend(Shape::dashed_line(&pts, stroke, 5.0, 3.0));
    } else {
        p.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
    }
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl View {
    pub(crate) fn toggle_nav(&mut self, id: &str) {
        let n = match id {
            "view.orbit" => Nav::Orbit,
            "view.pan" => Nav::Pan,
            _ => Nav::Zoom,
        };
        self.nav = if self.nav == n { Nav::Select } else { n };
    }
}

fn srgb(c: Color32) -> [f32; 3] {
    let lin = |v: u8| {
        let s = f32::from(v) / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    [lin(c.r()), lin(c.g()), lin(c.b())]
}

const BODY: Color32 = Color32::from_rgb(0xb8, 0xc0, 0xc8);
const BODY_DIM: Color32 = Color32::from_rgb(0x8a, 0x92, 0x9a);
const EDGE: Color32 = Color32::from_rgb(0x18, 0x1c, 0x22);
const HOVER: Color32 = Color32::from_rgb(0x8f, 0xdc, 0xd6);
const SELECTED: Color32 = Color32::from_rgb(0xf0, 0xa0, 0x3c);

impl Workbench {
    /// Glides the camera to `to`, remembering the current view for "Previous View".
    pub(crate) fn animate_to(&mut self, to: Camera) {
        let from = self.view.camera;
        if self.view.previous.last() != Some(&from) {
            self.view.previous.push(from);
            if self.view.previous.len() > VIEW_HISTORY {
                self.view.previous.remove(0);
            }
        }
        self.view.anim = Some(ViewAnim { from, to, start: self.view.now });
    }

    /// Advances a view transition; true while one runs.
    pub(crate) fn step_view_anim(&mut self) -> bool {
        let Some(a) = self.view.anim else { return false };
        let t = (self.view.now - a.start) / VIEW_TRANSITION;
        if t >= 1.0 || !t.is_finite() {
            self.view.camera = a.to;
            self.view.anim = None;
            false
        } else {
            self.view.camera = Camera::lerp(&a.from, &a.to, smoothstep(t));
            true
        }
    }

    /// The camera framing the part from the eye direction `eye_dir`.
    fn fitted_along(&self, eye_dir: Vec3) -> Camera {
        let mut c = self.view.anim.map_or(self.view.camera, |a| a.to);
        c.look_along(eye_dir);
        if let Some(b) = self.scene_box().or_else(|| self.sketch_box()) {
            c.fit(&b);
        }
        c
    }

    /// Looks from a direction (orientation cube, standard views), framing the part.
    pub(crate) fn look_from(&mut self, eye_dir: Vec3) {
        let to = self.fitted_along(eye_dir);
        self.animate_to(to);
    }

    pub(crate) fn home_view(&mut self) {
        let (yaw, pitch, roll) = self.view.home;
        let mut to = self.view.anim.map_or(self.view.camera, |a| a.to);
        (to.yaw, to.pitch, to.roll) = (yaw, pitch, roll);
        if let Some(b) = self.scene_box().or_else(|| self.sketch_box()) {
            to.fit(&b);
        }
        self.animate_to(to);
    }

    pub(crate) fn zoom_all(&mut self) {
        let mut to = self.view.anim.map_or(self.view.camera, |a| a.to);
        match self.scene_box().or_else(|| self.sketch_box()) {
            Some(b) => to.fit(&b),
            None => {
                to.target = Vec3::ZERO;
                to.distance = 200.0;
            }
        }
        self.animate_to(to);
    }

    pub(crate) fn previous_view(&mut self) -> Result<(), String> {
        let to = self.view.previous.pop().ok_or("there is no previous view")?;
        self.view.anim = Some(ViewAnim { from: self.view.camera, to, start: self.view.now });
        Ok(())
    }

    pub(crate) fn scene_box(&self) -> Option<Aabb3> {
        self.scene.bbox()
    }

    /// Radius of everything the camera may need in its depth range.
    fn scene_radius(&self) -> f64 {
        let c = &self.view.camera;
        match self.scene_box() {
            Some(b) => b.diagonal() / 2.0 + b.center().dist(c.target) + 1.0,
            None => c.distance.max(100.0),
        }
    }

    pub(crate) fn fit_view(&mut self) {
        let b = self.scene_box().or_else(|| self.sketch_box());
        match b {
            Some(b) => self.view.camera.fit(&b),
            None => {
                self.view.camera.target = Vec3::ZERO;
                self.view.camera.distance = 200.0;
            }
        }
    }

    pub(crate) fn look_at(&mut self) -> Result<(), String> {
        if let Mode::Sketch(s) = &self.mode {
            let f = self.sketch_frame(s.feature).ok_or("the sketch plane is not known yet")?;
            self.look_at_frame(&f);
            return Ok(());
        }
        let (n, c) = self.selected_plane().ok_or("select a planar face first")?;
        let mut to = self.view.anim.map_or(self.view.camera, |a| a.to);
        to.look_along(n);
        to.target = c;
        self.animate_to(to);
        Ok(())
    }

    /// The current camera turned square to a plane, with the plane's X axis pointing right.
    pub(crate) fn square_to(&self, f: &tenon_geom::Frame) -> Camera {
        let mut to = self.view.anim.map_or(self.view.camera, |a| a.to);
        to.look_along(f.z());
        let (r, u) = (to.right(), to.up());
        to.roll = (-f.x().dot(u)).atan2(f.x().dot(r));
        to
    }

    /// Looks square at a plane with its X axis pointing right (sketch planes).
    pub(crate) fn look_at_frame(&mut self, f: &tenon_geom::Frame) {
        let to = self.square_to(f);
        self.animate_to(to);
    }

    /// Normal and centroid of the selected planar face.
    fn selected_plane(&self) -> Option<(Vec3, Vec3)> {
        self.view.selection.iter().find_map(|p| match p {
            Pick::Face { body, face } => {
                let (_, info) = self.scene.bodies.get(*body)?.faces.get(*face as usize)?;
                match info.surface {
                    SurfaceKind::Plane { normal, .. } => Some((normal, info.centroid)),
                    _ => None,
                }
            }
            Pick::Edge { .. } => None,
        })
    }

    /// A persistent reference to the selected face, if it is planar and referable.
    pub(crate) fn selected_face_ref(&self) -> Option<FaceRef> {
        self.view.selection.iter().find_map(|p| match p {
            Pick::Face { body, face } => {
                let (name, info) = self.scene.bodies.get(*body)?.faces.get(*face as usize)?;
                match info.surface {
                    SurfaceKind::Plane { .. } => Some(FaceRef { origin: (*name)?, fingerprint: Fingerprint::of(info) }),
                    _ => None,
                }
            }
            Pick::Edge { .. } => None,
        })
    }

    pub(crate) fn viewport(&mut self, ui: &mut Ui, render: Option<&egui_wgpu::RenderState>, t: &Tokens) {
        let rect = ui.max_rect();
        self.view.rect = rect;
        if self.step_view_anim() {
            ui.ctx().request_repaint();
        }
        let mut mesh = egui::Mesh::default();
        for (p, c) in [
            (rect.left_top(), t.viewport_top),
            (rect.right_top(), t.viewport_top),
            (rect.right_bottom(), t.viewport_bottom),
            (rect.left_bottom(), t.viewport_bottom),
        ] {
            mesh.colored_vertex(p, c);
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ui.painter().add(Shape::mesh(mesh));

        let resp = ui.interact(rect, ui.id().with("viewport"), Sense::click_and_drag());
        // Right button: the radial menu. A click opens it; a flick picks the slot it points at.
        if resp.drag_started_by(egui::PointerButton::Secondary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            self.open_radial(o);
        }
        if resp.drag_stopped_by(egui::PointerButton::Secondary)
            && let (Some(c), Some(p)) = (self.chrome.radial.as_ref().map(|r| r.center), resp.interact_pointer_pos().or_else(|| resp.hover_pos()))
        {
            self.radial_flick(c, p);
        }
        if resp.secondary_clicked()
            && let Some(p) = resp.interact_pointer_pos().or_else(|| resp.hover_pos())
        {
            self.open_radial(p);
        }
        self.navigate(ui, &resp, rect);
        let sketching = matches!(self.mode, Mode::Sketch(_));
        if !sketching {
            self.model_pointer(ui, &resp, rect);
        }
        self.draw_scene(ui, rect, render);
        if self.has_properties() {
            self.manipulator(ui, rect, t);
            self.mini_toolbar(ui, rect, t);
        }
        if sketching {
            self.sketch_ui(ui, &resp, rect, t);
        }
        self.messages(ui, rect, t);
        self.triad(ui, pos2(rect.left() + 46.0, rect.bottom() - 46.0), t);
        if self.chrome.show_cube {
            self.cube(ui, pos2(rect.right() - 92.0, rect.top() + 82.0), t);
        }
        if self.chrome.show_navbar {
            self.nav_bar(ui, rect, t);
        }
    }

    /// What a left-button drag does: a navigation-bar tool, or a function key held down
    /// (F2 pan, F3 zoom, F4 orbit), else select.
    pub(crate) fn left_drag_tool(&self, ui: &Ui) -> Nav {
        let (f2, f3, f4) = ui.input(|i| (i.key_down(egui::Key::F2), i.key_down(egui::Key::F3), i.key_down(egui::Key::F4)));
        if f4 {
            Nav::Orbit
        } else if f2 {
            Nav::Pan
        } else if f3 {
            Nav::Zoom
        } else {
            self.view.nav
        }
    }

    /// Middle drag pans, Shift + middle drag orbits, the wheel zooms about the pointer; the left
    /// button navigates only with a navigation tool active.
    fn navigate(&mut self, ui: &Ui, resp: &egui::Response, rect: Rect) {
        let d = resp.drag_delta();
        let (dx, dy) = (f64::from(d.x), f64::from(d.y));
        let shift = ui.input(|i| i.modifiers.shift);
        let tool = self.left_drag_tool(ui);
        let left = resp.dragged_by(egui::PointerButton::Primary) && tool != Nav::Select;
        let middle = resp.dragged_by(egui::PointerButton::Middle);
        let mut moved = true;
        if (middle && shift) || (left && tool == Nav::Orbit) {
            self.view.camera.orbit(dx, dy);
        } else if middle || (left && tool == Nav::Pan) {
            self.view.camera.pan(dx, dy, f64::from(rect.height()));
        } else if left && tool == Nav::Zoom {
            self.view.camera.zoom((dy * 0.01).exp(), None);
        } else {
            moved = false;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y + (i.zoom_delta() - 1.0) * 200.0);
            if scroll.abs() > 0.01 {
                let towards = resp.hover_pos().and_then(|p| self.point_under(p, rect));
                self.view.camera.zoom((-f64::from(scroll) * 0.0015).exp(), towards);
                moved = true;
            }
        }
        if moved {
            // The user took over: stop any view transition where it is.
            self.view.anim = None;
        }
    }

    /// Faces and edges inside (window, dragged left to right) or touching (crossing, right to
    /// left) a screen rectangle.
    pub(crate) fn box_pick(&self, a: Pos2, b: Pos2, rect: Rect) -> Vec<Pick> {
        let sel = Rect::from_two_pos(a, b);
        let crossing = b.x < a.x;
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let cam = &self.view.camera;
        let screen = |p: [f32; 3]| {
            cam.project(Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])), w, h).map(|(x, y, _)| rect.min + vec2(x as f32, y as f32))
        };
        let mut out = Vec::new();
        for (bi, b) in self.scene.bodies.iter().enumerate() {
            let m = &b.mesh;
            for range in &m.faces {
                let idx = m.indices.get(range.first as usize..(range.first as usize).saturating_add(range.count as usize)).unwrap_or(&[]);
                let pts: Vec<Option<Pos2>> = idx.iter().map(|i| m.positions.get(*i as usize).and_then(|p| screen(*p))).collect();
                let hit = if crossing {
                    pts.iter().flatten().any(|p| sel.contains(*p))
                        || pts.as_chunks::<3>().0.iter().any(|t| match (t[0], t[1], t[2]) {
                            (Some(p0), Some(p1), Some(p2)) => {
                                [sel.min, sel.max, sel.left_bottom(), sel.right_top()].iter().any(|c| inside(&[p0, p1, p2], *c))
                            }
                            _ => false,
                        })
                } else {
                    !pts.is_empty() && pts.iter().all(|p| p.is_some_and(|p| sel.contains(p)))
                };
                if hit {
                    out.push(Pick::Face { body: bi, face: range.face });
                }
            }
            for e in &m.edges {
                let pts: Vec<Option<Pos2>> = e.points.iter().map(|p| screen(*p)).collect();
                let hit = if crossing {
                    pts.iter().flatten().any(|p| sel.contains(*p))
                } else {
                    !pts.is_empty() && pts.iter().all(|p| p.is_some_and(|p| sel.contains(p)))
                };
                if hit {
                    out.push(Pick::Edge { body: bi, edge: e.edge });
                }
            }
        }
        out
    }

    /// The model point under a screen position (face hit), if any.
    pub(crate) fn point_under(&self, p: Pos2, rect: Rect) -> Option<Vec3> {
        let (o, d) =
            self.view.camera.ray(f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
        let meshes: Vec<&tenon_kernel::Mesh> = self.scene.bodies.iter().map(|b| &b.mesh).collect();
        pick_face(&meshes, o, d).map(|h| h.point)
    }

    fn model_pointer(&mut self, ui: &Ui, resp: &egui::Response, rect: Rect) {
        // Selection box: a left drag with no navigation tool.
        if resp.drag_started_by(egui::PointerButton::Primary) && self.left_drag_tool(ui) == Nav::Select {
            self.view.box_start = ui.input(|i| i.pointer.press_origin());
        }
        if let Some(start) = self.view.box_start {
            let now = resp.interact_pointer_pos().or_else(|| resp.hover_pos()).unwrap_or(start);
            draw_selection_box(ui, start, now);
            if resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
                self.view.box_start = None;
                let picks = self.box_pick(start, now, rect);
                if ui.input(|i| i.modifiers.command || i.modifiers.shift) {
                    for p in picks {
                        if !self.view.selection.contains(&p) {
                            self.view.selection.push(p);
                        }
                    }
                } else {
                    self.view.selection = picks;
                }
                let (faces, edges) = self.view.selection.iter().fold((0, 0), |(f, e), p| match p {
                    Pick::Face { .. } => (f + 1, e),
                    Pick::Edge { .. } => (f, e + 1),
                });
                self.set_status(format!("{faces} faces and {edges} edges selected"));
            }
            return;
        }
        let meshes: Vec<&tenon_kernel::Mesh> = self.scene.bodies.iter().map(|b| &b.mesh).collect();
        self.view.hover = resp.hover_pos().and_then(|p| {
            let (x, y, w, h) = (f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
            if let Some(e) = pick_edge(&meshes, &self.view.camera, w, h, x, y, 5.0) {
                return Some(Pick::Edge { body: e.body, edge: e.edge });
            }
            let (o, d) = self.view.camera.ray(x, y, w, h);
            pick_face(&meshes, o, d).map(|f| Pick::Face { body: f.body, face: f.face })
        });
        if resp.clicked() {
            let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
            match (self.view.hover, add) {
                (Some(p), true) => {
                    if let Some(i) = self.view.selection.iter().position(|q| *q == p) {
                        self.view.selection.remove(i);
                    } else {
                        self.view.selection.push(p);
                    }
                }
                (Some(p), false) => self.view.selection = vec![p],
                (None, _) => self.view.selection.clear(),
            }
            if let Some(Pick::Face { body, face }) = self.view.selection.last() {
                let note = self.scene.bodies.get(*body).and_then(|b| b.faces.get(*face as usize)).map(|(name, info)| {
                    let what = match name {
                        Some(o) => o.describe(&self.feature_name(o.feature())),
                        None => "face".into(),
                    };
                    format!("Selected {what} (area {:.2} mm^2)", info.area)
                });
                if let Some(n) = note {
                    self.set_status(n);
                }
            }
        }
    }

    fn colors(&self) -> Vec<BodyColors> {
        let dim = matches!(self.mode, Mode::Sketch(_));
        (0..self.scene.bodies.len())
            .map(|bi| {
                // In wireframe the edges are all there is, so they take the body colour.
                let edge = if self.view.style == VisualStyle::Wireframe { BODY } else { EDGE };
                let mut c = BodyColors { face: srgb(if dim { BODY_DIM } else { BODY }), edge: srgb(edge), ..Default::default() };
                for (p, color) in self.view.selection.iter().map(|p| (p, SELECTED)).chain(self.view.hover.iter().map(|p| (p, HOVER))) {
                    match p {
                        Pick::Face { body, face } if *body == bi => c.faces.push((*face, srgb(color))),
                        Pick::Edge { body, edge } if *body == bi => c.edges.push((*edge, srgb(color))),
                        _ => {}
                    }
                }
                c
            })
            .collect()
    }

    fn draw_scene(&mut self, ui: &Ui, rect: Rect, render: Option<&egui_wgpu::RenderState>) {
        let colors = self.colors();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.scene_seq.hash(&mut h);
        format!("{colors:?}").hash(&mut h);
        let key = h.finish();
        let radius = self.scene_radius();
        let ppp = ui.ctx().pixels_per_point();
        let (w, hgt) = ((rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        match render {
            Some(rs) => {
                let gpu = self.view.gpu.get_or_insert_with(|| Gpu { viewport: Viewport::new(&rs.device), texture: None });
                if self.view.uploaded != Some(key) {
                    let pairs: Vec<(&tenon_kernel::Mesh, &BodyColors)> = self.scene.bodies.iter().map(|b| &b.mesh).zip(colors.iter()).collect();
                    gpu.viewport.set_bodies(&rs.device, &pairs);
                    self.view.uploaded = Some(key);
                }
                gpu.viewport.set_visible(self.view.style.faces(), self.view.style.edges());
                if let Some((view, recreated)) = gpu.viewport.render(&rs.device, &rs.queue, &self.view.camera, w, hgt, radius) {
                    let mut r = rs.renderer.write();
                    match gpu.texture {
                        Some(id) if recreated => r.update_egui_texture_from_wgpu_texture(&rs.device, view, egui_wgpu::wgpu::FilterMode::Linear, id),
                        Some(_) => {}
                        None => gpu.texture = Some(r.register_native_texture(&rs.device, view, egui_wgpu::wgpu::FilterMode::Linear)),
                    }
                }
                if let Some(id) = gpu.texture {
                    ui.painter().image(id, rect, uv, Color32::WHITE);
                }
            }
            None if !self.scene.bodies.is_empty() => {
                // Software fallback (no wgpu): half resolution, redrawn only when something changed.
                let (sw, sh) = ((w / 2).max(1), (hgt / 2).max(1));
                let mut kh = std::collections::hash_map::DefaultHasher::new();
                key.hash(&mut kh);
                (sw, sh).hash(&mut kh);
                self.view.style.hash(&mut kh);
                format!("{:?}", self.view.camera).hash(&mut kh);
                let skey = kh.finish();
                if self.view.soft.as_ref().is_none_or(|(_, k)| *k != skey) {
                    let meshes: Vec<&tenon_kernel::Mesh> = self.scene.bodies.iter().map(|b| &b.mesh).collect();
                    let to8 = |c: Color32| [c.r(), c.g(), c.b(), 255];
                    let mut face_colors = Vec::new();
                    for p in self.view.selection.iter().chain(self.view.hover.iter()) {
                        if let Pick::Face { body, face } = p {
                            face_colors.push((*body, *face, to8(if Some(*p) == self.view.hover { HOVER } else { SELECTED })));
                        }
                    }
                    let style = Style {
                        body: to8(BODY),
                        edge: to8(if self.view.style == VisualStyle::Wireframe { BODY } else { EDGE }),
                        face_colors,
                        faces: self.view.style.faces(),
                        edges: self.view.style.edges(),
                        ..Style::default()
                    };
                    let img = raster::render(&meshes, &self.view.camera, sw, sh, &style);
                    let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.rgba);
                    let tex = ui.ctx().load_texture("tenon-viewport", color, egui::TextureOptions::LINEAR);
                    self.view.soft = Some((tex, skey));
                }
                if let Some((tex, _)) = &self.view.soft {
                    ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
                }
            }
            None => {}
        }
    }

    fn messages(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        let p = ui.painter();
        if self.document().features().is_empty() && !matches!(self.mode, Mode::Sketch(_)) {
            p.text(rect.center() - vec2(0.0, 12.0), Align2::CENTER_CENTER, "Start with New Sketch (Model tab).", theme::heading(), t.viewport_text);
            p.text(
                rect.center() + vec2(0.0, 12.0),
                Align2::CENTER_CENTER,
                "Pick a plane, draw a closed profile, then Extrude.",
                theme::small(),
                t.text_dim,
            );
        }
        let banner = if let Some(n) = &self.regen_note {
            Some((format!("Regeneration failed: {n}"), t.history_marker))
        } else {
            self.scene.status.iter().find_map(|(f, s)| match s {
                tenon_model::FeatureStatus::Error { message } => Some((format!("{}: {message}", self.feature_name(*f)), t.history_marker)),
                _ => None,
            })
        };
        if let Some((text, color)) = banner {
            let galley = p.layout_no_wrap(text, theme::body(), color);
            let r = Rect::from_min_size(pos2(rect.left() + 12.0, rect.top() + 10.0), galley.size() + vec2(16.0, 10.0));
            p.rect_filled(r, 4.0, t.panel.gamma_multiply(0.92));
            p.galley(r.min + vec2(8.0, 5.0), galley, color);
        }
        if self.waiting {
            p.text(pos2(rect.right() - 14.0, rect.bottom() - 12.0), Align2::RIGHT_BOTTOM, "Regenerating...", theme::small(), t.text_dim);
        }
    }

    /// A camera with the current orientation for the small widgets (cube, triad).
    fn widget_camera(&self) -> Camera {
        Camera { target: Vec3::ZERO, distance: 10.0, projection: Projection::Orthographic, fov_y: 0.6, ..self.view.camera }
    }

    fn triad(&self, ui: &Ui, origin: Pos2, t: &Tokens) {
        let c = self.widget_camera();
        for (axis, color, label) in [(Vec3::X, t.axis_x, "X"), (Vec3::Y, t.axis_y, "Y"), (Vec3::Z, t.axis_z, "Z")] {
            let (r, u) = (c.right(), c.up());
            let end = origin + vec2((axis.dot(r) * 28.0) as f32, (-axis.dot(u) * 28.0) as f32);
            ui.painter().line_segment([origin, end], Stroke::new(2.0, color));
            ui.painter().text(origin + (end - origin) * 1.3, Align2::CENTER_CENTER, label, theme::small(), color);
        }
    }

    fn nav_bar(&mut self, ui: &Ui, viewport: Rect, t: &Tokens) {
        let size = 28.0;
        let h = NAV_BAR.len() as f32 * (size + 2.0) + 8.0;
        let bar = Rect::from_min_size(pos2(viewport.right() - size - 14.0, viewport.center().y - h / 2.0), vec2(size + 8.0, h));
        ui.painter().rect_filled(bar, 6.0, t.panel.gamma_multiply(0.85));
        let mut clicked = None;
        for (i, cmd) in NAV_BAR.iter().enumerate() {
            let br = Rect::from_min_size(pos2(bar.left() + 4.0, bar.top() + 4.0 + i as f32 * (size + 2.0)), vec2(size, size));
            let active = matches!((cmd.id, self.view.nav), ("view.orbit", Nav::Orbit) | ("view.pan", Nav::Pan) | ("view.zoom", Nav::Zoom));
            if icon_button(ui, br, cmd, active, t) {
                clicked = Some(cmd.id);
            }
        }
        if let Some(id) = clicked {
            self.command(id);
        }
    }
}

pub(crate) fn inside(poly: &[Pos2], p: Pos2) -> bool {
    let n = poly.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let c = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if c != 0.0 {
            if sign != 0.0 && c.signum() != sign {
                return false;
            }
            sign = c.signum();
        }
    }
    true
}
