//! The assembly environment: an open assembly, its parts' geometry (one worker slot per part),
//! the flattened scene the viewport shows, picking and dragging components, the
//! degrees-of-freedom symbols, interference, the parts list, the exploded view, and editing a
//! part in place.
//!
//! The viewport draws a [`Scene`] whose bodies are every visible component's bodies moved to
//! where the component is (`AsmDoc::map` says which component each body belongs to). Everything
//! that works on a scene (picking, highlighting, the GPU upload) works on that unchanged.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Align2, Color32, Pos2, Rect, Shape, Stroke, Ui, vec2};
use serde_json::{Value, json};
use tenon_assembly::interfere::{Clash, Placed};
use tenon_assembly::math::M3;
use tenon_assembly::session::{self as asm_session, solve_assembly};
use tenon_assembly::solve::{BodyDof, Motion};
use tenon_assembly::{AsmSession, Assembly, ComponentId};
use tenon_geom::{Aabb3, Axis, Frame, Vec3};
use tenon_kernel::{CurveKind, EdgePolyline, FaceInfo, Mesh, SurfaceKind};
use tenon_model::{BodyView, EdgeFingerprint, Scene};
use tenon_render::Camera;
use tenon_render::pick::pick_face;

use crate::theme::{self, Tokens};
use crate::viewport::{Nav, Pick};
use crate::workbench::{Geo, Mode, Workbench};

/// A part being edited in place.
pub(crate) struct Editing {
    pub component: ComponentId,
    /// The part's key (path).
    pub key: String,
    /// Where the component is: the part is shown in its own coordinates, the rest moved by the
    /// inverse of this.
    pub frame: Frame,
    /// The other components, in the edited part's coordinates, with their keys.
    pub context: Vec<(u64, Mesh)>,
    /// The part's document revision when editing started.
    pub start_revision: u64,
}

/// A component being dragged.
pub(crate) struct Drag {
    pub component: ComponentId,
    /// The point grabbed (assembly coordinates) and where the pointer was.
    pub grab: Vec3,
    pub press: Pos2,
    pub start: Frame,
    /// Turning about the component's centre instead of moving.
    pub rotate: bool,
    pub center: Vec3,
    /// The assembly when the drag started, and as it is now.
    pub base: Assembly,
    pub working: Assembly,
}

/// What a job sent to the worker is for.
pub(crate) enum AsmJob {
    Interference,
    Step(PathBuf),
}

/// An open assembly.
pub(crate) struct AsmDoc {
    pub session: AsmSession,
    pub path: Option<PathBuf>,
    /// Which component each body of the shown scene belongs to: (component, body of its part).
    pub map: Vec<(ComponentId, usize)>,
    /// Per body of the shown scene: what it was made from (for the GPU upload).
    pub keys: Vec<u64>,
    /// Where each component's bodies are in the shown scene, and the key they were made from.
    cache: BTreeMap<ComponentId, (u64, std::ops::Range<usize>)>,
    /// The scene sequence number of the last scene built here (its bodies can be reused).
    flat_seq: Option<u64>,
    /// What the shown scene was made from.
    pub shown: Option<u64>,
    /// Worker slot of each part, and the document revision last sent to it.
    slots: BTreeMap<String, (u64, Option<u64>)>,
    next_slot: u64,
    pub editing: Option<Editing>,
    pub exploded: bool,
    pub show_dof: bool,
    pub selected: Vec<ComponentId>,
    pub hover: Option<ComponentId>,
    pub drag: Option<Drag>,
    /// Free Rotate: dragging turns components.
    pub rotate: bool,
    /// The last interference check (shown until closed).
    pub clashes: Option<Result<Vec<Clash>, String>>,
    pub jobs: BTreeMap<u64, AsmJob>,
    pub bom: bool,
    /// A part changed after it was shown: solve once its geometry is current.
    pub needs_update: bool,
    /// Degrees of freedom and what they were computed from.
    dof: Option<(u64, Vec<BodyDof>, usize)>,
    /// Relationships open in the browser.
    pub relationships_open: bool,
    pub expanded: std::collections::BTreeSet<ComponentId>,
}

impl AsmDoc {
    pub(crate) fn new(session: AsmSession, path: Option<PathBuf>) -> AsmDoc {
        AsmDoc {
            session,
            path,
            map: Vec::new(),
            keys: Vec::new(),
            cache: BTreeMap::new(),
            flat_seq: None,
            shown: None,
            slots: BTreeMap::new(),
            next_slot: 0,
            editing: None,
            exploded: false,
            show_dof: false,
            selected: Vec::new(),
            hover: None,
            drag: None,
            rotate: false,
            clashes: None,
            jobs: BTreeMap::new(),
            bom: false,
            needs_update: false,
            dof: None,
            relationships_open: true,
            expanded: Default::default(),
        }
    }

    /// The assembly as it is shown: while dragging, the dragged state.
    pub(crate) fn shown_assembly(&self) -> &Assembly {
        self.drag.as_ref().map_or_else(|| self.session.assembly(), |d| &d.working)
    }

    /// The name of a component.
    pub(crate) fn name(&self, id: ComponentId) -> String {
        self.session.assembly().component(id).map_or_else(|| id.to_string(), |c| c.name.clone())
    }
}

// ---- moving geometry --------------------------------------------------------------------------

fn v3(p: [f32; 3]) -> Vec3 {
    Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
}

fn f3(v: Vec3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

/// A rigid map: rotation, then translation.
#[derive(Clone, Copy)]
struct Rigid {
    r: M3,
    t: Vec3,
}

impl Rigid {
    fn of(f: &Frame) -> Rigid {
        Rigid { r: M3::of_frame(f), t: f.origin() }
    }
    /// From a component's part coordinates into the coordinates of `into` (the edited part).
    fn between(f: &Frame, into: &Frame) -> Rigid {
        let (a, b) = (Rigid::of(f), Rigid::of(into));
        let bt = b.r.transpose();
        Rigid { r: bt.mul(&a.r), t: bt.apply(a.t - b.t) }
    }
    fn point(&self, p: Vec3) -> Vec3 {
        self.r.apply(p) + self.t
    }
    fn dir(&self, v: Vec3) -> Vec3 {
        self.r.apply(v)
    }
}

fn placed_mesh(m: &Mesh, x: &Rigid) -> Mesh {
    Mesh {
        positions: m.positions.iter().map(|p| f3(x.point(v3(*p)))).collect(),
        normals: m.normals.iter().map(|n| f3(x.dir(v3(*n)))).collect(),
        indices: m.indices.clone(),
        faces: m.faces.clone(),
        edges: m.edges.iter().map(|e| EdgePolyline { edge: e.edge, points: e.points.iter().map(|p| f3(x.point(v3(*p)))).collect() }).collect(),
    }
}

fn placed_axis(a: &Axis, x: &Rigid) -> Axis {
    Axis::new(x.point(a.origin()), x.dir(a.dir())).unwrap_or(*a)
}

fn placed_info(info: &FaceInfo, x: &Rigid) -> FaceInfo {
    let surface = match &info.surface {
        SurfaceKind::Plane { origin, normal } => SurfaceKind::Plane { origin: x.point(*origin), normal: x.dir(*normal) },
        SurfaceKind::Cylinder { axis, radius } => SurfaceKind::Cylinder { axis: placed_axis(axis, x), radius: *radius },
        SurfaceKind::Cone { axis, half_angle, ref_radius } => {
            SurfaceKind::Cone { axis: placed_axis(axis, x), half_angle: *half_angle, ref_radius: *ref_radius }
        }
        SurfaceKind::Sphere { center, radius } => SurfaceKind::Sphere { center: x.point(*center), radius: *radius },
        SurfaceKind::Torus { axis, major_radius, minor_radius } => {
            SurfaceKind::Torus { axis: placed_axis(axis, x), major_radius: *major_radius, minor_radius: *minor_radius }
        }
        other => other.clone(),
    };
    FaceInfo { surface, area: info.area, centroid: x.point(info.centroid), reversed: info.reversed }
}

fn placed_body(b: &BodyView, x: &Rigid) -> BodyView {
    let mut mass = b.mass.clone();
    mass.center_of_mass = x.point(mass.center_of_mass);
    // R I R^T
    let i = mass.inertia;
    let r = x.r.0;
    let mut ri = [[0.0; 3]; 3];
    let mut out = [[0.0; 3]; 3];
    for a in 0..3 {
        for c in 0..3 {
            ri[a][c] = (0..3).map(|k| r[a][k] * i[k][c]).sum();
        }
    }
    for a in 0..3 {
        for c in 0..3 {
            out[a][c] = (0..3).map(|k| ri[a][k] * r[c][k]).sum();
        }
    }
    mass.inertia = out;
    let corners = |bb: &Aabb3| {
        let (l, h) = (bb.min, bb.max);
        (0..8).map(move |k| Vec3::new(if k & 1 == 0 { l.x } else { h.x }, if k & 2 == 0 { l.y } else { h.y }, if k & 4 == 0 { l.z } else { h.z }))
    };
    BodyView {
        mesh: placed_mesh(&b.mesh, x),
        faces: b.faces.iter().map(|(n, info)| (*n, placed_info(info, x))).collect(),
        volume: b.volume,
        mass,
        bbox: b.bbox.and_then(|bb| Aabb3::from_points(corners(&bb).map(|p| x.point(p)))),
        edges: b.edges.iter().map(|(n, fp)| (*n, EdgeFingerprint { mid: x.point(fp.mid), length: fp.length })).collect(),
        ends: b.ends.iter().map(|[s, e]| [x.point(*s), x.point(*e)]).collect(),
        curves: b
            .curves
            .iter()
            .map(|c| match c {
                CurveKind::Line { origin, dir } => CurveKind::Line { origin: x.point(*origin), dir: x.dir(*dir) },
                CurveKind::Circle { axis, radius } => CurveKind::Circle { axis: placed_axis(axis, x), radius: *radius },
                other => other.clone(),
            })
            .collect(),
    }
}

fn hash_frame(f: &Frame, h: &mut impl Hasher) {
    for v in [f.origin(), f.x(), f.y(), f.z()] {
        v.x.to_bits().hash(h);
        v.y.to_bits().hash(h);
        v.z.to_bits().hash(h);
    }
}

/// A camera seeing the same as `c`, in the coordinates of a part placed at `f` (`into_part`), or
/// back out of them.
pub(crate) fn camera_in(c: &Camera, f: &Frame, into_part: bool) -> Camera {
    let x = if into_part { Rigid::between(&Frame::WORLD, f) } else { Rigid::of(f) };
    let mut out = *c;
    out.target = x.point(c.target);
    out.look_along(x.dir(c.eye_dir()));
    let right = x.dir(c.right());
    let (r0, u0) = (out.right(), out.up());
    out.roll = (-right.dot(u0)).atan2(right.dot(r0));
    out
}

// ---- the workbench in an assembly -------------------------------------------------------------

impl Workbench {
    /// An assembly is open and no part is being edited in place.
    pub(crate) fn in_assembly(&self) -> bool {
        self.asm.as_ref().is_some_and(|a| a.editing.is_none())
    }

    /// A part of the open assembly is being edited in place.
    pub(crate) fn editing_in_place(&self) -> bool {
        self.asm.as_ref().is_some_and(|a| a.editing.is_some())
    }

    /// The open assembly's session (tests and tools).
    pub fn assembly(&self) -> Option<&AsmSession> {
        self.asm.as_ref().map(|a| &a.session)
    }

    /// Runs an assembly command (`asm.*`) on the open assembly.
    pub fn asm_exec(&mut self, id: &str, params: Value) -> Result<Value, String> {
        let a = self.asm.as_mut().ok_or("no assembly is open")?;
        let kernel: Option<&mut dyn tenon_kernel::Kernel> = match &mut self.geo {
            Geo::Sync(k) => Some(k.as_mut()),
            _ => None,
        };
        let r = tenon_io::asm::run(&mut a.session, id, &params, kernel).map_err(|e| e.to_string());
        a.dof = None;
        r
    }

    fn enter_assembly(&mut self, doc: AsmDoc) {
        self.leave_assembly();
        self.session.replace_document(tenon_model::Document::default(), None);
        self.asm = Some(Box::new(doc));
        self.path = None;
        self.mode = Mode::Model;
        self.panel = None;
        self.view = crate::viewport::View::default();
        self.scene = Scene::default();
        self.scene_seq += 1;
        self.chrome.tab = 0;
        self.chrome.expanded.clear();
    }

    /// Closes the assembly (its worker slots are freed).
    pub(crate) fn leave_assembly(&mut self) {
        self.take_assembly();
    }

    /// Closes the assembly and hands back its session (with a part edited in place put back).
    pub(crate) fn take_assembly(&mut self) -> Option<AsmSession> {
        let mut a = self.asm.take()?;
        if let Some(e) = a.editing.take()
            && let Some(p) = a.session.parts.get_mut(&e.key)
        {
            std::mem::swap(&mut self.session, &mut p.session);
        }
        if let Geo::Worker(w) = &self.geo {
            for (slot, _) in a.slots.values() {
                w.drop_slot(*slot);
            }
        }
        self.scene = Scene::default();
        self.scene_seq += 1;
        self.shown = None;
        self.shown_key = None;
        Some(a.session)
    }

    pub(crate) fn new_assembly(&mut self) {
        self.leave_drawing();
        self.enter_assembly(AsmDoc::new(AsmSession::default(), None));
        self.set_status("New assembly: place a part file to start (Assemble > Place, or P).");
    }

    /// Opens an assembly file and the part files it uses.
    pub fn open_assembly(&mut self, path: &Path) -> Result<(), String> {
        let (asm, parts) = tenon_io::asm::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let missing: Vec<String> = parts.values().filter_map(|p| p.missing.clone()).collect();
        let mut s = AsmSession::default();
        s.replace(asm, parts);
        self.leave_drawing();
        self.enter_assembly(AsmDoc::new(s, Some(path.to_path_buf())));
        match missing.first() {
            Some(m) => self.set_error(format!("Opened {} ({} part file(s) missing: {m})", path.display(), missing.len())),
            None => self.set_status(format!("Opened {}", path.display())),
        }
        Ok(())
    }

    /// Runs `f` with the part edited in place back in the assembly (for saving and exporting).
    fn with_parts_home<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let key = self.asm.as_ref().and_then(|a| a.editing.as_ref().map(|e| e.key.clone()));
        let swap = |wb: &mut Self| {
            if let (Some(k), Some(a)) = (&key, wb.asm.as_mut())
                && let Some(p) = a.session.parts.get_mut(k)
            {
                std::mem::swap(&mut wb.session, &mut p.session);
            }
        };
        swap(self);
        let r = f(self);
        swap(self);
        r
    }

    /// Saves the assembly and the parts changed in place.
    pub(crate) fn save_assembly(&mut self, path: &Path) -> Result<(), String> {
        let r = self.with_parts_home(|wb| wb.asm_exec("asm.save", json!({ "path": path.to_string_lossy() })))?;
        if let Some(a) = self.asm.as_mut() {
            a.path = Some(path.to_path_buf());
        }
        let parts = r["parts_saved"].as_array().map_or(0, Vec::len);
        self.set_status(if parts > 0 { format!("Saved {} and {parts} part file(s)", path.display()) } else { format!("Saved {}", path.display()) });
        Ok(())
    }

    /// Sends parts whose geometry is out of date to the worker (or regenerates them here), takes
    /// what arrived, and rebuilds the shown scene when anything changed.
    pub(crate) fn sync_assembly(&mut self) {
        let Some(a) = self.asm.as_mut() else { return };
        let editing = a.editing.as_ref().map(|e| e.key.clone());
        match &mut self.geo {
            Geo::Sync(k) => {
                // The part being edited in place is regenerated as the workbench's own document.
                if editing.is_none()
                    && let Err(e) = a.session.refresh(k.as_mut())
                {
                    self.regen_note = Some(e);
                }
            }
            Geo::Worker(w) => {
                for (key, p) in &a.session.parts {
                    if p.missing.is_some() || p.is_current() || editing.as_ref() == Some(key) {
                        continue;
                    }
                    let rev = p.session.revision();
                    if !a.slots.contains_key(key) {
                        a.next_slot += 1;
                        a.slots.insert(key.clone(), (a.next_slot, None));
                    }
                    if let Some(slot) = a.slots.get_mut(key)
                        && slot.1 != Some(rev)
                    {
                        w.regenerate_slot(slot.0, rev, p.session.document().clone(), false);
                        slot.1 = Some(rev);
                        self.waiting = true;
                    }
                }
            }
            Geo::Off => {}
        }
        let Some(a) = self.asm.as_mut() else { return };
        let all_current = a.session.parts.iter().all(|(k, p)| p.is_current() || editing.as_ref() == Some(k));
        if all_current && matches!(self.geo, Geo::Worker(_)) {
            self.waiting = false;
        }
        if a.needs_update && all_current && editing.is_none() {
            a.needs_update = false;
            match a.session.update() {
                Ok(s) if !s.converged => {
                    let names: Vec<String> =
                        s.failing.iter().filter_map(|(r, _)| a.session.assembly().relationship(*r)).map(|r| r.name.clone()).collect();
                    self.set_error(format!("Some relationships no longer hold: {}", names.join(", ")));
                }
                Ok(_) => {}
                Err(e) => self.set_error(e.0),
            }
            if let Some(a) = self.asm.as_mut() {
                a.dof = None;
            }
        }
        self.asm_panel_preview();
        if self.in_assembly() {
            self.flatten();
        }
    }

    /// A part's geometry arrived from the worker. True when it was for the assembly.
    pub(crate) fn asm_scene(&mut self, slot: u64, revision: u64, scene: Scene) -> bool {
        let Some(a) = self.asm.as_mut() else { return false };
        let Some(key) = a.slots.iter().find(|(_, (s, _))| *s == slot).map(|(k, _)| k.clone()) else { return false };
        if let Some(p) = a.session.parts.get_mut(&key)
            && p.session.revision() == revision
        {
            if p.scene.is_some() {
                // The part changed after it was first shown: the relationships may need solving.
                a.needs_update = true;
            }
            p.scene = Some((revision, Arc::new(scene)));
            a.dof = None;
        }
        true
    }

    /// Where each component is shown: while dragging, as dragged; with a relationship being set
    /// up, as it would place them; in the exploded view, exploded.
    fn display_frames(&self) -> BTreeMap<ComponentId, Frame> {
        let Some(a) = self.asm.as_ref() else { return BTreeMap::new() };
        let asm = a.shown_assembly();
        let mut frames: BTreeMap<ComponentId, Frame> =
            if a.exploded && a.drag.is_none() { asm_session::exploded(asm) } else { asm.components.iter().map(|c| (c.id, c.placement)).collect() };
        if let Some(crate::panels::Panel::Asm(p)) = &self.panel
            && let Some((_, Ok(preview))) = &p.preview
        {
            frames.extend(preview.iter().map(|(k, v)| (*k, *v)));
        }
        frames
    }

    /// Builds the shown scene from the components (only what changed is moved again).
    fn flatten(&mut self) {
        let frames = self.display_frames();
        let Some(a) = self.asm.as_mut() else { return };
        let asm = a.shown_assembly().clone();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let mut keys: Vec<(ComponentId, u64)> = Vec::new();
        for c in asm.components.iter().filter(|c| c.visible) {
            let mut ch = std::collections::hash_map::DefaultHasher::new();
            c.part.hash(&mut ch);
            let scene = a.session.parts.get(&c.part).and_then(|p| p.scene.as_ref());
            scene.map(|(r, s)| (*r, Arc::as_ptr(s) as usize)).hash(&mut ch);
            if let Some(f) = frames.get(&c.id) {
                hash_frame(f, &mut ch);
            }
            let k = ch.finish();
            k.hash(&mut h);
            keys.push((c.id, k));
        }
        let key = h.finish();
        if a.shown == Some(key) {
            return;
        }
        a.shown = Some(key);
        // Bodies of components that did not move are moved over from the scene shown now (not
        // copied); only the others are placed again.
        let reuse = a.flat_seq == Some(self.scene_seq);
        let mut old: Vec<Option<BodyView>> = if reuse { std::mem::take(&mut self.scene.bodies).into_iter().map(Some).collect() } else { Vec::new() };
        let cache = std::mem::take(&mut a.cache);
        let mut bodies = Vec::new();
        let (mut map, mut body_keys) = (Vec::new(), Vec::new());
        let mut next_cache = BTreeMap::new();
        for (id, k) in keys {
            let Some(c) = asm.component(id) else { continue };
            let kept: Option<Vec<BodyView>> = match cache.get(&id) {
                Some((ck, range)) if *ck == k && reuse => range.clone().map(|i| old.get_mut(i).and_then(Option::take)).collect(),
                _ => None,
            };
            let placed = kept.unwrap_or_else(|| match (asm_session::scene_of(&a.session.parts, c), frames.get(&id)) {
                (Some(s), Some(f)) => {
                    let x = Rigid::of(f);
                    s.bodies.iter().map(|b| placed_body(b, &x)).collect()
                }
                _ => Vec::new(),
            });
            next_cache.insert(id, (k, bodies.len()..bodies.len() + placed.len()));
            for (i, b) in placed.into_iter().enumerate() {
                map.push((id, i));
                body_keys.push(k ^ (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
                bodies.push(b);
            }
        }
        a.cache = next_cache;
        a.map = map;
        a.keys = body_keys;
        self.scene = Scene { bodies, ..Scene::default() };
        self.scene_seq += 1;
        a.flat_seq = Some(self.scene_seq);
        self.view.selection.clear();
        if !self.view.fitted && !self.scene.bodies.is_empty() {
            self.fit_view();
            self.view.fitted = true;
        }
    }

    /// The component a body of the shown scene belongs to.
    pub(crate) fn component_of_body(&self, body: usize) -> Option<ComponentId> {
        self.asm.as_ref().and_then(|a| a.map.get(body)).map(|(c, _)| *c)
    }

    /// Highlights in the assembly: the hovered and selected components, whole.
    pub(crate) fn asm_highlights(&self) -> Vec<(Pick, Color32)> {
        let Some(a) = self.asm.as_ref() else { return Vec::new() };
        let mut v = Vec::new();
        for (i, (c, _)) in a.map.iter().enumerate() {
            let color = if a.selected.contains(c) {
                crate::viewport::SELECTED
            } else if a.hover == Some(*c) {
                crate::viewport::HOVER
            } else {
                continue;
            };
            let n = self.scene.bodies.get(i).map_or(0, |b| b.faces.len());
            v.extend((0..n).map(|f| (Pick::Face { body: i, face: f as u32 }, color)));
        }
        v
    }

    /// The other components while a part is edited in place (drawn dimmed, not picked).
    pub(crate) fn context_meshes(&self) -> Vec<(u64, &Mesh)> {
        self.asm.as_ref().and_then(|a| a.editing.as_ref()).map(|e| e.context.iter().map(|(k, m)| (*k, m)).collect()).unwrap_or_default()
    }

    /// Mouse in the viewport of an assembly: hover and select components, drag them (their
    /// relationships hold), double-click to edit one in place.
    pub(crate) fn asm_pointer(&mut self, ui: &Ui, resp: &egui::Response, rect: Rect) {
        if matches!(self.panel, Some(crate::panels::Panel::Asm(_))) {
            self.asm_panel_pointer(ui, resp, rect);
            return;
        }
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let hit_at = |wb: &Self, p: Pos2| {
            let (o, d) = wb.view.camera.ray(f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), w, h);
            let meshes: Vec<&Mesh> = wb.scene.bodies.iter().map(|b| &b.mesh).collect();
            pick_face(&meshes, o, d).and_then(|hit| wb.component_of_body(hit.body).map(|c| (c, hit.point)))
        };
        let dragging = self.asm.as_ref().is_some_and(|a| a.drag.is_some());
        let hover = if dragging { None } else { resp.hover_pos().and_then(|p| hit_at(self, p)).map(|(c, _)| c) };
        if resp.drag_started_by(egui::PointerButton::Primary) && self.left_drag_tool(ui) == Nav::Select {
            let origin = ui.input(|i| i.pointer.press_origin());
            if let Some((c, grab)) = origin.and_then(|p| hit_at(self, p)) {
                self.start_drag(c, grab, origin.unwrap_or_default(), ui.input(|i| i.modifiers.shift));
            }
        }
        if dragging {
            if let Some(p) = resp.interact_pointer_pos().or_else(|| resp.hover_pos())
                && resp.dragged()
            {
                self.update_drag(p, rect);
            }
            if resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
                self.finish_drag();
            }
        }
        let Some(a) = self.asm.as_mut() else { return };
        a.hover = hover;
        if resp.double_clicked() {
            if let Some(c) = hover
                && let Err(e) = self.edit_in_place(c)
            {
                self.set_error(e);
            }
            return;
        }
        if resp.clicked() {
            let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
            match (hover, add) {
                (Some(c), true) => {
                    if let Some(i) = a.selected.iter().position(|x| *x == c) {
                        a.selected.remove(i);
                    } else {
                        a.selected.push(c);
                    }
                }
                (Some(c), false) => a.selected = vec![c],
                (None, _) => a.selected.clear(),
            }
            if let Some(c) = a.selected.last().copied() {
                let note = self.component_note(c);
                self.set_status(note);
            }
        }
    }

    /// What the status bar says about a component.
    fn component_note(&self, c: ComponentId) -> String {
        let Some(a) = self.asm.as_ref() else { return String::new() };
        let Some(comp) = a.session.assembly().component(c) else { return String::new() };
        let dof = self
            .asm_dof()
            .and_then(|(per, _)| a.session.assembly().components.iter().position(|x| x.id == c).and_then(|i| per.get(i)).map(BodyDof::count));
        let state = if comp.grounded { "grounded".to_string() } else { format!("{} degrees of freedom", dof.unwrap_or(6)) };
        format!("Selected {} ({state}). Drag to move it; double-click to edit its part.", comp.name)
    }

    fn start_drag(&mut self, c: ComponentId, grab: Vec3, press: Pos2, shift: bool) {
        let Some(a) = self.asm.as_mut() else { return };
        let Some(comp) = a.session.assembly().component(c).cloned() else { return };
        if comp.grounded {
            self.set_status(format!("{} is grounded; unground it (Assemble > Grounded) to move it.", comp.name));
            return;
        }
        let center = asm_session::local_bbox(&a.session.parts, &comp).map_or(comp.placement.origin(), |b| comp.placement.to_world(b.center()));
        let asm = a.session.assembly().clone();
        a.drag = Some(Drag { component: c, grab, press, start: comp.placement, rotate: a.rotate || shift, center, base: asm.clone(), working: asm });
        a.selected = vec![c];
    }

    fn update_drag(&mut self, pointer: Pos2, rect: Rect) {
        let cam = self.view.camera;
        let Some(a) = self.asm.as_mut() else { return };
        let Some(d) = a.drag.as_mut() else { return };
        let mut working = d.base.clone();
        let Some(comp) = working.component_mut(d.component) else { return };
        if d.rotate {
            let delta = pointer - d.press;
            let r = M3::exp(cam.up() * (f64::from(delta.x) * 0.01)).mul(&M3::exp(cam.right() * (f64::from(delta.y) * 0.01)));
            comp.placement = Motion { r, t: d.center - r.apply(d.center) }.apply_frame(&d.start);
        } else {
            let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
            let (o, dir) = cam.ray(f64::from(pointer.x - rect.left()), f64::from(pointer.y - rect.top()), w, h);
            let n = cam.forward();
            let den = dir.dot(n);
            if den.abs() < 1e-9 {
                return;
            }
            let p = o + dir * ((d.grab - o).dot(n) / den);
            let moved = d.start.origin() + (p - d.grab);
            let Some(f) = d.start.with_origin(moved) else { return };
            comp.placement = f;
        }
        solve_assembly(&mut working, &a.session.parts, Some(d.component));
        d.working = working;
    }

    fn finish_drag(&mut self) {
        let Some(a) = self.asm.as_mut() else { return };
        let Some(d) = a.drag.take() else { return };
        let working = d.working;
        if let Err(e) = a.session.edit(|asm, _| {
            *asm = working;
            Ok(())
        }) {
            self.set_error(e.0);
        }
        if let Some(a) = self.asm.as_mut() {
            a.dof = None;
            a.shown = None;
        }
    }

    /// Degrees of freedom of every component and the total (cached per change).
    pub(crate) fn asm_dof(&self) -> Option<(Vec<BodyDof>, usize)> {
        let a = self.asm.as_ref()?;
        if let Some((rev, per, total)) = &a.dof
            && *rev == self.dof_key()
        {
            return Some((per.clone(), *total));
        }
        Some(asm_session::dof(a.session.assembly(), &a.session.parts))
    }

    fn dof_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        if let Some(a) = self.asm.as_ref() {
            a.session.revision().hash(&mut h);
            for p in a.session.parts.values() {
                p.scene.as_ref().map(|(r, _)| *r).hash(&mut h);
            }
        }
        h.finish()
    }

    /// Keeps the degrees of freedom up to date while they are shown.
    pub(crate) fn refresh_dof(&mut self) {
        let key = self.dof_key();
        let Some(a) = self.asm.as_ref() else { return };
        if a.dof.as_ref().is_some_and(|(k, _, _)| *k == key) {
            return;
        }
        let (per, total) = asm_session::dof(a.session.assembly(), &a.session.parts);
        if let Some(a) = self.asm.as_mut() {
            a.dof = Some((key, per, total));
        }
    }

    // ---- editing a part in place ------------------------------------------------------------

    /// Edits a component's part where it sits in the assembly: the part becomes the workbench's
    /// document (with its own undo), the other components stay in view, dimmed.
    pub(crate) fn edit_in_place(&mut self, id: ComponentId) -> Result<(), String> {
        let a = self.asm.as_mut().ok_or("no assembly is open")?;
        if a.editing.is_some() {
            return Err("a part is already being edited; Return first".into());
        }
        let c = a.session.assembly().component(id).cloned().ok_or_else(|| format!("{id} does not exist"))?;
        let part = a.session.parts.get_mut(&c.part).ok_or("the component's part is not loaded")?;
        if let Some(why) = &part.missing {
            return Err(format!("{} is missing: {why}", c.name));
        }
        std::mem::swap(&mut self.session, &mut part.session);
        let scene = part.scene.as_ref().map(|(_, s)| (**s).clone());
        // The rest, moved into the part's coordinates.
        let mut context = Vec::new();
        for o in a.session.assembly().components.iter().filter(|o| o.visible && o.id != id) {
            let Some(s) = asm_session::scene_of(&a.session.parts, o) else { continue };
            let x = Rigid::between(&o.placement, &c.placement);
            let mut kh = std::collections::hash_map::DefaultHasher::new();
            ("context", o.id.0, &o.part).hash(&mut kh);
            hash_frame(&o.placement, &mut kh);
            hash_frame(&c.placement, &mut kh);
            let k = kh.finish();
            for (i, b) in s.bodies.iter().enumerate() {
                context.push((k ^ (i as u64), placed_mesh(&b.mesh, &x)));
            }
        }
        let start_revision = self.session.revision();
        a.editing = Some(Editing { component: id, key: c.part.clone(), frame: c.placement, context, start_revision });
        a.drag = None;
        a.hover = None;
        a.selected.clear();
        self.view.camera = camera_in(&self.view.camera, &c.placement, true);
        self.view.anim = None;
        self.view.selection.clear();
        self.view.hover = None;
        self.mode = Mode::Model;
        self.panel = None;
        if let Some(s) = scene {
            self.scene = s;
            self.scene_seq += 1;
        }
        self.shown = None;
        self.shown_key = None;
        self.chrome.tab = 0;
        self.set_status(format!("Editing {} in place. Return (on the ribbon) goes back to the assembly.", c.name));
        Ok(())
    }

    /// Back to the assembly from editing a part in place; the assembly follows the part's changes.
    pub(crate) fn return_to_assembly(&mut self) {
        if matches!(self.mode, Mode::Sketch(_)) {
            self.finish_sketch();
        }
        let Some(a) = self.asm.as_mut() else { return };
        let Some(e) = a.editing.take() else { return };
        let changed = self.session.revision() != e.start_revision;
        if let Some(p) = a.session.parts.get_mut(&e.key) {
            std::mem::swap(&mut self.session, &mut p.session);
        }
        // The assembly follows the part once its new geometry is there.
        a.needs_update |= changed;
        a.shown = None;
        a.dof = None;
        self.view.camera = camera_in(&self.view.camera, &e.frame, false);
        self.view.anim = None;
        self.view.selection.clear();
        self.view.hover = None;
        self.mode = Mode::Model;
        self.panel = None;
        self.chrome.tab = 0;
        let name = a.name(e.component);
        self.scene = Scene::default();
        self.scene_seq += 1;
        self.set_status(format!("Back in the assembly from {name}."));
    }

    // ---- commands ---------------------------------------------------------------------------

    /// Assembly commands from the ribbon, menus and keys.
    pub(crate) fn run_asm_ui(&mut self, id: &str) -> Result<(), String> {
        match id {
            "file.new_assembly" => self.new_assembly(),
            "asm.return" => {
                if !self.editing_in_place() {
                    return Err("no part is being edited in place".into());
                }
                self.return_to_assembly();
            }
            _ if self.asm.is_none() => return Err("open or start an assembly first (File > New Assembly)".into()),
            _ if self.editing_in_place() => return Err("Return to the assembly first".into()),
            "asm.place" => self.place_component()?,
            "asm.create" => self.create_component()?,
            "asm.edit" => {
                let c = self.asm.as_ref().and_then(|a| a.selected.last().copied()).ok_or("select a component first")?;
                self.edit_in_place(c)?;
            }
            "asm.free_rotate" => {
                if let Some(a) = self.asm.as_mut() {
                    a.rotate = !a.rotate;
                    let on = a.rotate;
                    self.set_status(if on { "Free Rotate: drag a component to turn it." } else { "Dragging moves components again." });
                }
            }
            "asm.ground" => {
                let a = self.asm.as_ref().ok_or("no assembly")?;
                let sel = a.selected.clone();
                if sel.is_empty() {
                    return Err("select the components to ground first".into());
                }
                let on = !sel.iter().all(|c| a.session.assembly().component(*c).is_some_and(|x| x.grounded));
                for c in sel {
                    self.asm_exec("asm.ground", json!({ "component": c.0, "grounded": on }))?;
                }
                self.set_status(if on { "Grounded." } else { "Free to move." });
            }
            "asm.update" => {
                let r = self.asm_exec("asm.update", json!({}))?;
                if r["converged"] == true {
                    self.set_status("Every relationship holds.");
                } else {
                    let names: Vec<String> =
                        r["failing"].as_array().into_iter().flatten().filter_map(|f| f["name"].as_str().map(str::to_owned)).collect();
                    self.set_error(format!("Not holding: {}", names.join(", ")));
                }
            }
            "asm.constrain" => self.open_asm_panel(crate::asm_panel::AsmTool::Constrain),
            "asm.joint" => self.open_asm_panel(crate::asm_panel::AsmTool::Joint),
            "asm.explode.tweak" => self.open_asm_panel(crate::asm_panel::AsmTool::Tweak),
            "asm.explode.toggle" => {
                let a = self.asm.as_mut().ok_or("no assembly")?;
                if a.session.assembly().explode.is_empty() && !a.exploded {
                    return Err("there is no exploded view yet: Auto Explode or Tweak first".into());
                }
                a.exploded = !a.exploded;
                let on = a.exploded;
                self.set_status(if on { "Exploded view." } else { "Assembled view." });
            }
            "asm.explode.auto" => {
                let r = self.asm_exec("asm.explode.auto", json!({}))?;
                if let Some(a) = self.asm.as_mut() {
                    a.exploded = true;
                }
                self.set_status(format!("Exploded in {} steps.", r["steps"]));
            }
            "asm.explode.clear" => {
                self.asm_exec("asm.explode.clear", json!({}))?;
                if let Some(a) = self.asm.as_mut() {
                    a.exploded = false;
                }
            }
            "asm.dof" => {
                if let Some(a) = self.asm.as_mut() {
                    a.show_dof = !a.show_dof;
                }
            }
            "asm.bom" => {
                if let Some(a) = self.asm.as_mut() {
                    a.bom = !a.bom;
                }
            }
            "asm.interference" => self.analyze_interference()?,
            other => {
                return Err(match crate::commands::find(other) {
                    Some(c) if !c.available() => format!("{} is not available yet: it arrives in milestone M{}.", c.label, c.milestone),
                    _ => format!("unknown command `{other}`"),
                });
            }
        }
        Ok(())
    }

    /// Place: choose a part file; the first goes to the origin, grounded, later ones beside
    /// what is there.
    fn place_component(&mut self) -> Result<(), String> {
        let path = self.services.pick_open.as_ref().and_then(|f| f()).ok_or("no file chosen")?;
        self.place_file(&path)
    }

    /// Places a part file in the open assembly.
    pub fn place_file(&mut self, path: &Path) -> Result<(), String> {
        if path.extension().is_some_and(|e| e == tenon_io::asm::EXTENSION) {
            return Err("sub-assemblies are not supported yet: place part files (.tenon)".into());
        }
        let a = self.asm.as_ref().ok_or("no assembly is open")?;
        let at = (!a.session.assembly().components.is_empty())
            .then(|| asm_session::assembly_bbox(a.session.assembly(), &a.session.parts))
            .flatten()
            .map(|b| json!([b.max.x + (b.diagonal() * 0.15).max(10.0), b.min.y, b.min.z]));
        let mut p = json!({ "path": path.to_string_lossy() });
        if let Some(at) = at {
            p["at"] = at;
        }
        let r = self.asm_exec("asm.insert", p)?;
        let grounded = if r["grounded"] == true { " (grounded)" } else { "" };
        self.set_status(format!("Placed {}{grounded}. Constrain (C) or Joint (J) it to the others.", r["name"].as_str().unwrap_or("")));
        if let Some(a) = self.asm.as_mut()
            && let Some(id) = r["component"].as_u64().and_then(|v| u32::try_from(v).ok())
        {
            a.selected = vec![ComponentId(id)];
        }
        Ok(())
    }

    /// Create: a new, empty part file placed at the origin and edited in place.
    fn create_component(&mut self) -> Result<(), String> {
        let path = self.services.pick_save.as_ref().and_then(|f| f("Part1.tenon", "tenon")).ok_or("no file chosen")?;
        self.create_part_file(&path)
    }

    /// Makes an empty part file, places it at the origin (grounded) and edits it in place.
    pub fn create_part_file(&mut self, path: &Path) -> Result<(), String> {
        let mut doc = tenon_model::Document::default();
        doc.name = path.file_stem().map_or_else(|| "Part1".into(), |s| s.to_string_lossy().into_owned());
        tenon_io::project::save(path, &doc, &serde_json::Map::new()).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        let r = self.asm_exec("asm.insert", json!({ "path": path.to_string_lossy(), "at": [0, 0, 0], "grounded": true }))?;
        let id = r["component"].as_u64().and_then(|v| u32::try_from(v).ok()).ok_or("internal: no component id")?;
        self.edit_in_place(ComponentId(id))?;
        self.set_status(format!("Created {}. Sketch on a plane or a face of another component, then Return.", path.display()));
        Ok(())
    }

    /// Interference: on the worker (or here, headless); the result shows in a window and as red
    /// volumes in the view.
    fn analyze_interference(&mut self) -> Result<(), String> {
        let a = self.asm.as_mut().ok_or("no assembly")?;
        match &mut self.geo {
            Geo::Sync(k) => {
                let asm = a.session.assembly().clone();
                a.clashes = Some(tenon_assembly::interfere::interference(k.as_mut(), &asm, &mut a.session.parts, None));
            }
            Geo::Worker(w) => {
                let items = slot_items(a);
                self.step_seq += 1;
                let request = self.step_seq;
                a.jobs.insert(request, AsmJob::Interference);
                w.job(
                    request,
                    Box::new(move |k, slots| {
                        let placed = placed_from_slots(&items, slots);
                        Box::new(tenon_assembly::interfere::clashes(k, &placed, None))
                    }),
                );
                self.set_status("Checking interference...");
                return Ok(());
            }
            Geo::Off => return Err("there is no geometry kernel".into()),
        }
        self.report_clashes();
        Ok(())
    }

    fn report_clashes(&mut self) {
        let Some(a) = self.asm.as_ref() else { return };
        let msg = match &a.clashes {
            Some(Ok(c)) if c.is_empty() => "No interference.".to_string(),
            Some(Ok(c)) => format!("{} interference(s), {:.3} mm^3 in all.", c.len(), c.iter().map(|x| x.volume).sum::<f64>()),
            Some(Err(e)) => format!("Interference check failed: {e}"),
            None => return,
        };
        self.set_status(msg);
    }

    /// STEP export of the assembly: on the worker, or here.
    pub(crate) fn export_assembly_step(&mut self, path: &Path) -> Result<(), String> {
        let frames = self.display_frames();
        let a = self.asm.as_mut().ok_or("no assembly")?;
        match &mut self.geo {
            Geo::Sync(k) => {
                let asm = a.session.assembly().clone();
                let data = tenon_assembly::interfere::export_step(k.as_mut(), &asm, &mut a.session.parts, Some(&frames))?;
                std::fs::write(path, data).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                self.set_status(format!("Exported {}", path.display()));
            }
            Geo::Worker(w) => {
                let mut items = slot_items(a);
                for it in &mut items {
                    if let Some(f) = frames.get(&it.0) {
                        it.2 = *f;
                    }
                }
                self.step_seq += 1;
                let request = self.step_seq;
                a.jobs.insert(request, AsmJob::Step(path.to_path_buf()));
                w.job(
                    request,
                    Box::new(move |k, slots| {
                        let placed = placed_from_slots(&items, slots);
                        Box::new(tenon_assembly::interfere::step_of(k, &placed))
                    }),
                );
                self.set_status("Exporting STEP...");
            }
            Geo::Off => return Err("there is no geometry kernel".into()),
        }
        Ok(())
    }

    /// A job finished. True when it was the assembly's.
    pub(crate) fn asm_job(&mut self, request: u64, result: Box<dyn std::any::Any + Send>) -> bool {
        let Some(a) = self.asm.as_mut() else { return false };
        let Some(job) = a.jobs.remove(&request) else { return false };
        match job {
            AsmJob::Interference => {
                a.clashes =
                    Some(result.downcast::<Result<Vec<Clash>, String>>().map(|b| *b).unwrap_or_else(|_| Err("internal: unexpected result".into())));
                self.report_clashes();
            }
            AsmJob::Step(path) => {
                let r = result.downcast::<Result<Vec<u8>, String>>().map(|b| *b).unwrap_or_else(|_| Err("internal: unexpected result".into()));
                match r.and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string())) {
                    Ok(()) => self.set_status(format!("Exported {}", path.display())),
                    Err(e) => self.set_error(format!("STEP export failed: {e}")),
                }
            }
        }
        true
    }

    // ---- drawing ----------------------------------------------------------------------------

    /// Over the view: interference volumes, exploded-view trails and the degrees-of-freedom
    /// symbols.
    pub(crate) fn asm_overlays(&mut self, ui: &Ui, rect: Rect, t: &Tokens) {
        if !self.in_assembly() {
            return;
        }
        let cam = self.view.camera;
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let screen = |q: Vec3| cam.project(q, w, h).map(|(x, y, _)| rect.min + vec2(x as f32, y as f32));
        let p = ui.painter();
        let Some(a) = self.asm.as_ref() else { return };
        // Interference: the overlapping volumes in red, over everything.
        if let Some(Ok(clashes)) = &a.clashes {
            let red = Color32::from_rgba_unmultiplied(0xe0, 0x30, 0x30, 150);
            for c in clashes {
                let m = &c.mesh;
                for tri in m.indices.as_chunks::<3>().0 {
                    let pts: Option<Vec<Pos2>> = tri.iter().map(|i| m.positions.get(*i as usize).and_then(|q| screen(v3(*q)))).collect();
                    if let Some(pts) = pts {
                        p.add(Shape::convex_polygon(pts, red, Stroke::NONE));
                    }
                }
            }
        }
        // Exploded view: dashed lines from where each moved component sits assembled.
        if a.exploded && a.drag.is_none() {
            let exploded = asm_session::exploded(a.session.assembly());
            for c in &a.session.assembly().components {
                let (Some(f), Some(b)) = (exploded.get(&c.id), asm_session::local_bbox(&a.session.parts, c)) else { continue };
                let (from, to) = (c.placement.to_world(b.center()), f.to_world(b.center()));
                if from.dist(to) < 1e-6 {
                    continue;
                }
                if let (Some(s), Some(e)) = (screen(from), screen(to)) {
                    p.extend(Shape::dashed_line(&[s, e], Stroke::new(1.2, t.accent), 6.0, 4.0));
                }
            }
        }
        if a.show_dof {
            self.draw_dof(ui, rect, t);
        }
    }

    fn draw_dof(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        let Some(a) = self.asm.as_ref() else { return };
        let Some((rev, per, _)) = &a.dof else { return };
        let _ = rev;
        let cam = self.view.camera;
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let screen = |q: Vec3| cam.project(q, w, h).map(|(x, y, _)| rect.min + vec2(x as f32, y as f32));
        let p = ui.painter();
        let color = Color32::from_rgb(0x4c, 0xc2, 0x8c);
        let stroke = Stroke::new(2.0, color);
        let frames = self.display_frames();
        for (c, d) in a.session.assembly().components.iter().zip(per) {
            if d.count() == 0 {
                continue;
            }
            let Some(b) = asm_session::local_bbox(&a.session.parts, c) else { continue };
            let f = frames.get(&c.id).copied().unwrap_or(c.placement);
            let center = f.to_world(b.center());
            let Some(o) = screen(center) else { continue };
            // A length on screen of about 34 pixels, whatever the zoom.
            let unit = screen(center + cam.right()).map_or(1.0, |q| (q - o).length().max(1e-3));
            let len = 34.0 / unit;
            for dir in &d.translations {
                let (Some(a2), Some(b2)) = (screen(center - *dir * f64::from(len)), screen(center + *dir * f64::from(len))) else { continue };
                p.line_segment([a2, b2], stroke);
                for (tip, from) in [(a2, b2), (b2, a2)] {
                    let v = (tip - from).normalized();
                    let n = vec2(-v.y, v.x);
                    p.add(Shape::convex_polygon(vec![tip, tip - v * 9.0 + n * 5.0, tip - v * 9.0 - n * 5.0], color, Stroke::NONE));
                }
            }
            for (axis, _) in &d.rotations {
                // A circle about the axis, through the centre.
                let u = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
                let e1 = axis.cross(u).normalized();
                let e2 = axis.cross(e1);
                let r = f64::from(len) * 0.8;
                let pts: Vec<Pos2> = (0..=28)
                    .filter_map(|i| {
                        let ang = f64::from(i) / 28.0 * std::f64::consts::TAU * 0.85;
                        screen(center + (e1 * ang.cos() + e2 * ang.sin()) * r)
                    })
                    .collect();
                if pts.len() > 2 {
                    let tip = pts[pts.len() - 1];
                    let v = (tip - pts[pts.len() - 2]).normalized();
                    let n = vec2(-v.y, v.x);
                    p.add(Shape::line(pts, stroke));
                    p.add(Shape::convex_polygon(vec![tip + v * 6.0, tip - v * 4.0 + n * 5.0, tip - v * 4.0 - n * 5.0], color, Stroke::NONE));
                }
            }
            p.text(o + vec2(8.0, -8.0), Align2::LEFT_BOTTOM, format!("{}", d.count()), theme::small(), t.text);
        }
    }

    /// The parts list and interference windows.
    pub(crate) fn asm_windows(&mut self, ui: &Ui) {
        let Some(a) = self.asm.as_mut() else { return };
        let mut open = a.bom;
        let mut export = false;
        if open {
            let rows = asm_session::bom(a.session.assembly(), &a.session.parts);
            egui::Window::new("Bill of Materials")
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(520.0)
                .default_pos([300.0, 170.0])
                .show(ui.ctx(), |ui| {
                    egui::Grid::new("tn_bom").num_columns(5).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                        for h in ["Item", "Part", "Name", "Qty", "Volume (mm^3)"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for r in &rows {
                            ui.label(r.item.to_string());
                            ui.label(&r.part);
                            ui.label(&r.name);
                            ui.label(r.quantity.to_string());
                            ui.label(r.volume.map_or_else(|| "-".into(), |v| format!("{v:.2}")));
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                    if ui.button("Export CSV...").clicked() {
                        export = true;
                    }
                });
        }
        a.bom = open;
        let mut close_clashes = false;
        if let Some(result) = &a.clashes {
            let names = |c: ComponentId| a.session.assembly().component(c).map_or_else(|| c.to_string(), |x| x.name.clone());
            let mut keep = true;
            egui::Window::new("Interference")
                .open(&mut keep)
                .collapsible(false)
                .resizable(false)
                .default_width(360.0)
                .default_pos([300.0, 170.0])
                .show(ui.ctx(), |ui| match result {
                    Ok(c) if c.is_empty() => {
                        ui.label("No interference.");
                    }
                    Ok(c) => {
                        egui::Grid::new("tn_clashes").num_columns(3).striped(true).show(ui, |ui| {
                            ui.strong("Component");
                            ui.strong("Component");
                            ui.strong("Volume (mm^3)");
                            ui.end_row();
                            for x in c {
                                ui.label(names(x.a));
                                ui.label(names(x.b));
                                ui.label(format!("{:.3}", x.volume));
                                ui.end_row();
                            }
                        });
                    }
                    Err(e) => {
                        ui.label(egui::RichText::new(e).color(Color32::from_rgb(0xd8, 0x44, 0x38)));
                    }
                });
            close_clashes = !keep;
        }
        if close_clashes {
            a.clashes = None;
        }
        if export {
            let name = format!("{}.csv", a.session.assembly().name);
            let text = tenon_io::asm::bom_csv(a.session.assembly(), &a.session.parts);
            if let Some(path) = self.services.pick_save.as_ref().and_then(|f| f(&name, "csv")) {
                match std::fs::write(&path, text) {
                    Ok(()) => self.set_status(format!("Exported {}", path.display())),
                    Err(e) => self.set_error(format!("cannot write {}: {e}", path.display())),
                }
            }
        }
    }

    /// Right-hand side of the status bar in an assembly.
    pub(crate) fn asm_status(&self) -> Option<String> {
        let a = self.asm.as_ref()?;
        if a.editing.is_some() {
            return None;
        }
        let n = a.session.assembly().components.len();
        let dof = a.dof.as_ref().map(|(_, _, d)| format!("      {d} DOF")).unwrap_or_default();
        Some(format!("{n} components{dof}"))
    }
}

/// What a worker job needs to find each component's solids: component, slot, placement, box.
type SlotItem = (ComponentId, u64, Frame, Option<Aabb3>);

fn slot_items(a: &AsmDoc) -> Vec<SlotItem> {
    a.session
        .assembly()
        .components
        .iter()
        .filter(|c| c.visible)
        .filter_map(|c| a.slots.get(&c.part).map(|(slot, _)| (c.id, *slot, c.placement, asm_session::local_bbox(&a.session.parts, c))))
        .collect()
}

fn placed_from_slots(items: &[SlotItem], slots: &BTreeMap<u64, tenon_model::Regen>) -> Vec<Placed> {
    items
        .iter()
        .filter_map(|(c, slot, frame, bbox)| {
            slots.get(slot).map(|r| Placed { component: *c, shapes: r.bodies.iter().map(|b| b.shape).collect(), frame: *frame, bbox: *bbox })
        })
        .collect()
}
