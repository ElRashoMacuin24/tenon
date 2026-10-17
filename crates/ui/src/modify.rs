//! Picking for the features that act on an existing body (Fillet, Chamfer, Shell): while one of
//! their panels is open, clicks in the viewport add or remove edges and faces, and the panel's
//! references are highlighted on the part.

use egui::{Pos2, Rect, Stroke, vec2};
use tenon_kernel::SurfaceKind;
use tenon_model::{AxisSel, DirectionRef, EdgeRef, FaceRef, FeatureId, Fingerprint, PlaneRef, Scene};
use tenon_render::pick::{pick_edge_in, pick_face_in};
use tenon_sketch::EntityId;

use crate::panels::{ChamferMethod, CopyKind, Panel, Slot};
use crate::theme::Tokens;
use crate::viewport::Pick;
use crate::work::{Reference, WorkSlot};
use crate::workbench::Workbench;

/// The edge of the scene a reference means: between the same two named faces, nearest by
/// geometry.
pub(crate) fn find_edge(scene: &Scene, r: &EdgeRef) -> Option<(usize, u32)> {
    let mut best: Option<(f64, usize, u32)> = None;
    for (bi, b) in scene.bodies.iter().enumerate() {
        for (ei, (names, fp)) in b.edges.iter().enumerate() {
            if *names != Some(r.faces) {
                continue;
            }
            let d = fp.distance(&r.fingerprint);
            if best.is_none_or(|x| d < x.0) {
                best = Some((d, bi, u32::try_from(ei).ok()?));
            }
        }
    }
    best.map(|(_, b, e)| (b, e))
}

/// The face of the scene a reference means: the same name, nearest by geometry.
pub(crate) fn find_face(scene: &Scene, r: &FaceRef) -> Option<(usize, u32)> {
    let mut best: Option<(f64, usize, u32)> = None;
    for (bi, b) in scene.bodies.iter().enumerate() {
        for (fi, (name, info)) in b.faces.iter().enumerate() {
            if *name != Some(r.origin) {
                continue;
            }
            let d = Fingerprint::of(info).distance(&r.fingerprint);
            if best.is_none_or(|x| d < x.0) {
                best = Some((d, bi, u32::try_from(fi).ok()?));
            }
        }
    }
    best.map(|(_, b, f)| (b, f))
}

/// What clicks pick while a panel is open.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Wants {
    pub edges: bool,
    pub faces: bool,
}

impl Workbench {
    /// What the open panel picks in the viewport, if it picks anything.
    pub(crate) fn panel_wants(&self) -> Option<Wants> {
        match &self.panel {
            Some(Panel::Fillet(_) | Panel::Flange(_) | Panel::Hem(_) | Panel::CornerSeam(_) | Panel::Bend(_) | Panel::ContourFlange(_)) => {
                Some(Wants { edges: true, faces: false })
            }
            Some(Panel::Chamfer(p)) => Some(Wants { edges: true, faces: p.method != ChamferMethod::Distance }),
            Some(Panel::Shell(_) | Panel::Draft(_) | Panel::Split(_) | Panel::Combine(_) | Panel::Thread(_)) => {
                Some(Wants { edges: false, faces: true })
            }
            // A support is on faces. A fixed one may be on edges in their place: it is on
            // whichever was clicked first, until that is cleared.
            Some(Panel::Support(s)) => {
                Some(Wants { edges: s.kind == tenon_model::study::SupportKind::Fixed && s.faces.is_empty(), faces: s.edges.is_empty() })
            }
            // A load: its direction, from a straight edge or a flat face, while that selector is
            // the active one; else its faces (the part's weight and its turning have none). A
            // force may be along edges in their place.
            Some(Panel::Load(_)) if self.picking_direction() || self.picking_load_point() => Some(Wants { edges: true, faces: true }),
            Some(Panel::Load(l)) => {
                Some(Wants { edges: l.what == crate::analysis::What::Force && l.faces.is_empty(), faces: l.what.on_faces() && l.edges.is_empty() })
            }
            Some(Panel::Pattern(p)) => Some(match p.slot {
                Slot::Features | Slot::Plane => Wants { edges: false, faces: true },
                Slot::Dir1 | Slot::Dir2 => Wants { edges: true, faces: false },
                Slot::Axis => Wants { edges: true, faces: true },
            }),
            Some(Panel::Work(w)) => Some(match w.slot {
                WorkSlot::A | WorkSlot::B => Wants { edges: false, faces: true },
                WorkSlot::Axis => Wants { edges: true, faces: true },
                WorkSlot::Edge => Wants { edges: true, faces: false },
            }),
            _ => None,
        }
    }

    /// The feature a pattern copies when this face is clicked: the one that made it, if it adds
    /// or removes material and comes before the pattern.
    fn copyable_feature(&self, body: usize, face: u32) -> Option<FeatureId> {
        let id = self.scene.bodies.get(body)?.faces.get(face as usize)?.0?.feature();
        let f = self.document().feature(id)?;
        let before = match &self.panel {
            Some(Panel::Pattern(p)) => p.editing.is_none_or(|e| self.document().index_of(id) < self.document().index_of(e)),
            _ => true,
        };
        (f.kind.has_tool() && before).then_some(id)
    }

    /// Adds or removes a feature of the pattern being set up (a click on it in the browser).
    pub(crate) fn toggle_pattern_feature(&mut self, id: FeatureId) {
        let Some(Panel::Pattern(p)) = &self.panel else { return };
        let ok = self.document().feature(id).is_some_and(|f| f.kind.has_tool())
            && p.editing.is_none_or(|e| self.document().index_of(id) < self.document().index_of(e));
        if !ok {
            let name = self.feature_name(id);
            self.set_error(format!("{name} cannot be copied: only earlier features that add or remove material can"));
            return;
        }
        if let Some(Panel::Pattern(p)) = &mut self.panel {
            p.slot = Slot::Features;
            if let Some(i) = p.features.iter().position(|f| *f == id) {
                p.features.remove(i);
            } else {
                p.features.push(id);
            }
        }
    }

    /// The open panel's references, as picks on the shown part.
    pub(crate) fn panel_picks(&self) -> Vec<Pick> {
        let edges = |refs: &[EdgeRef]| -> Vec<Pick> {
            refs.iter().filter_map(|r| find_edge(&self.scene, r)).map(|(body, edge)| Pick::Edge { body, edge }).collect()
        };
        let face = |r: &FaceRef| find_face(&self.scene, r).map(|(body, face)| Pick::Face { body, face });
        match &self.panel {
            Some(Panel::Fillet(p)) => edges(&p.edges),
            Some(Panel::Flange(p)) => edges(&p.edges),
            Some(Panel::Hem(p)) => edges(&p.edges),
            Some(Panel::CornerSeam(p)) => edges(&p.edges),
            Some(Panel::Bend(p)) => edges(&p.edges),
            Some(Panel::ContourFlange(p)) => edges(&p.edges),
            Some(Panel::Chamfer(p)) => {
                let mut v = edges(&p.edges);
                v.extend(p.reference.as_ref().and_then(face));
                v
            }
            Some(Panel::Shell(p)) => p.faces.iter().filter_map(face).collect(),
            Some(Panel::Support(p)) => p.faces.iter().filter_map(face).chain(edges(&p.edges)).collect(),
            Some(Panel::Load(p)) => p.faces.iter().filter_map(face).chain(edges(&p.edges)).collect(),
            Some(Panel::Draft(_) | Panel::Split(_) | Panel::Combine(_) | Panel::Thread(_)) => self.bodies_picks(),
            Some(Panel::Measure(m)) => m.a.into_iter().chain(m.b).collect(),
            Some(Panel::Pattern(p)) => {
                // The faces of the features being copied, and the picked direction, axis or plane.
                let mut v: Vec<Pick> = Vec::new();
                for (bi, b) in self.scene.bodies.iter().enumerate() {
                    for (fi, (name, _)) in b.faces.iter().enumerate() {
                        if name.is_some_and(|n| p.features.contains(&n.feature()))
                            && let Ok(face) = u32::try_from(fi)
                        {
                            v.push(Pick::Face { body: bi, face });
                        }
                    }
                }
                let refs: Vec<Pick> = match p.kind {
                    CopyKind::Rect => [Some(&p.dir1), p.dir2.as_ref()]
                        .into_iter()
                        .flatten()
                        .filter_map(|d| match d {
                            DirectionRef::Edge(e) => find_edge(&self.scene, e).map(|(body, edge)| Pick::Edge { body, edge }),
                            DirectionRef::Origin(_) | DirectionRef::Work(_) => None,
                        })
                        .collect(),
                    CopyKind::Circular => match &p.axis {
                        AxisSel::Edge(e) => edges(std::slice::from_ref(e)),
                        AxisSel::Face(f) => face(f).into_iter().collect(),
                        AxisSel::Origin(_) | AxisSel::Work(_) => Vec::new(),
                    },
                    CopyKind::Mirror => match &p.plane {
                        PlaneRef::Face(f) => face(f).into_iter().collect(),
                        PlaneRef::Origin(_) | PlaneRef::Work(_) => Vec::new(),
                    },
                };
                v.extend(refs);
                v
            }
            _ => Vec::new(),
        }
    }

    fn edge_ref_of(&self, body: usize, edge: u32) -> Option<EdgeRef> {
        self.scene.bodies.get(body)?.edge_ref(edge)
    }

    /// The open panel takes a face or plane to go as far as: Extrude To, or a Hole's To.
    pub(crate) fn picking_to(&self) -> bool {
        match &self.panel {
            Some(Panel::Extrude(e)) => matches!(e.extent, crate::panels::ExtentChoice::To | crate::panels::ExtentChoice::Between),
            Some(Panel::Hole(h)) => h.to_plane,
            Some(Panel::Revolve(r)) => !r.full && r.term == crate::panels::RevTerm::To,
            _ => false,
        }
    }

    /// Puts the face or plane a feature goes as far as into the open panel.
    pub(crate) fn set_to_plane(&mut self, plane: PlaneRef) -> bool {
        match &mut self.panel {
            Some(Panel::Extrude(e)) if e.extent == crate::panels::ExtentChoice::To => e.to = Some(plane),
            // (Between: the start, then the end; a row clicked chooses which comes next.)
            Some(Panel::Extrude(e)) if e.extent == crate::panels::ExtentChoice::Between => {
                if e.pick_from || e.from.is_none() {
                    e.from = Some(plane);
                    e.pick_from = false;
                } else {
                    e.to = Some(plane);
                }
            }
            Some(Panel::Hole(h)) if h.to_plane => h.to = Some(plane),
            Some(Panel::Revolve(r)) if !r.full && r.term == crate::panels::RevTerm::To => r.to = Some(plane),
            _ => return false,
        }
        true
    }

    /// A work plane, or a flat face of the part, under the pointer while the open panel picks
    /// what it goes as far as: lit, and picked by a click. Returns whether there is one.
    fn pick_end_plane(&mut self, resp: &egui::Response, rect: Rect) -> bool {
        if let Some(pos) = resp.hover_pos()
            && let Some((id, _)) = self.work_at(pos, rect, true, false)
        {
            self.view.hover = None;
            self.view.work_hover = Some(id);
            if resp.clicked() {
                self.pick_reference(Reference::Work(id));
            }
            return true;
        }
        let (meshes, tree) = (self.scene.placed(), self.body_tree());
        let flat = resp.hover_pos().and_then(|p| {
            let (x, y, w, h) = (f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
            let (o, d) = self.view.camera.ray(x, y, w, h);
            let f = pick_face_in(tree.as_deref(), &meshes, o, d)?;
            let planar = self
                .scene
                .bodies
                .get(f.body)
                .and_then(|b| b.faces.get(f.face as usize))
                .is_some_and(|x| matches!(x.1.surface, SurfaceKind::Plane { .. }));
            planar
                .then_some(Pick::Face { body: f.body, face: f.face })
                .filter(|p| matches!(p, Pick::Face { body, face } if self.face_ref_of(*body, *face).is_some()))
        });
        self.view.hover = flat;
        let Some(Pick::Face { body, face }) = flat else { return false };
        if resp.clicked()
            && let Some(r) = self.face_ref_of(body, face)
        {
            self.set_to_plane(PlaneRef::Face(r));
        }
        true
    }

    pub(crate) fn face_ref_of(&self, body: usize, face: u32) -> Option<FaceRef> {
        let (name, info) = self.scene.bodies.get(body)?.faces.get(face as usize)?;
        Some(FaceRef { origin: (*name)?, fingerprint: Fingerprint::of(info) })
    }

    /// True if the pick can go into the open panel.
    fn referable(&self, p: Pick, wants: Wants) -> bool {
        let surface = |body: usize, face: u32| self.scene.bodies.get(body).and_then(|b| b.faces.get(face as usize)).map(|f| &f.1.surface);
        match (p, &self.panel) {
            (Pick::Face { body, face }, Some(Panel::Pattern(pp))) if wants.faces => match pp.slot {
                Slot::Features => self.copyable_feature(body, face).is_some(),
                Slot::Plane => self.face_ref_of(body, face).is_some() && matches!(surface(body, face), Some(SurfaceKind::Plane { .. })),
                Slot::Axis => {
                    self.face_ref_of(body, face).is_some()
                        && matches!(surface(body, face), Some(SurfaceKind::Cylinder { .. } | SurfaceKind::Cone { .. }))
                }
                Slot::Dir1 | Slot::Dir2 => false,
            },
            (Pick::Face { body, face }, Some(Panel::Work(w))) if wants.faces => {
                self.face_ref_of(body, face).is_some()
                    && match w.slot {
                        WorkSlot::A | WorkSlot::B => matches!(surface(body, face), Some(SurfaceKind::Plane { .. })),
                        WorkSlot::Axis => matches!(surface(body, face), Some(SurfaceKind::Cylinder { .. } | SurfaceKind::Cone { .. })),
                        WorkSlot::Edge => false,
                    }
            }
            (Pick::Face { body, face }, Some(Panel::Draft(_) | Panel::Split(_) | Panel::Combine(_) | Panel::Thread(_))) => {
                self.bodies_referable(body, face)
            }
            // A direction is read off the edge or face as it is: it needs no name.
            (pick, Some(Panel::Load(_))) if self.picking_direction() => self.direction_of(pick).is_some(),
            // A point is the middle of the face or edge clicked.
            (pick, Some(Panel::Load(_))) if self.picking_load_point() => self.axis_point_of(pick).is_some(),
            // A support takes the faces its kind can hold: a pin round ones, a sliding one
            // flat or round ones.
            (Pick::Face { body, face }, Some(Panel::Support(s))) => {
                wants.faces && self.face_ref_of(body, face).is_some_and(|r| s.kind.takes(&r.fingerprint.surface).is_ok())
            }
            // A load, the faces it can be on: a bearing load round ones.
            (Pick::Face { body, face }, Some(Panel::Load(l))) => {
                wants.faces && self.face_ref_of(body, face).is_some_and(|r| l.shown().takes(&r.fingerprint.surface).is_ok())
            }
            (Pick::Edge { body, edge }, _) => wants.edges && self.edge_ref_of(body, edge).is_some(),
            (Pick::Face { body, face }, _) => wants.faces && self.face_ref_of(body, face).is_some(),
        }
    }

    /// Adds a pick to the panel, or removes it when it is there already (`toggle`). Returns
    /// false when the panel does not take it.
    pub(crate) fn pick_into_panel(&mut self, p: Pick, toggle: bool) -> bool {
        if matches!(self.panel, Some(Panel::Work(_))) {
            return self.work_pick(p);
        }
        if self.picking_direction() {
            let through = self.axis_point_of(p);
            return self.direction_of(p).is_some_and(|d| self.set_load_direction(d, through));
        }
        if self.picking_load_point() {
            let at = match p {
                Pick::Face { body, face } => self.scene.bodies.get(body).and_then(|b| b.faces.get(face as usize)).map(|f| f.1.centroid),
                Pick::Edge { .. } => self.axis_point_of(p),
            };
            return at.is_some_and(|at| self.set_load_point(at));
        }
        if matches!(self.panel, Some(Panel::Draft(_) | Panel::Split(_) | Panel::Combine(_) | Panel::Thread(_))) {
            return self.bodies_pick(p, toggle);
        }
        if let Some(Panel::Pattern(pp)) = &self.panel {
            let slot = pp.slot;
            match (slot, p) {
                (Slot::Features, Pick::Face { body, face }) => {
                    let Some(id) = self.copyable_feature(body, face) else { return false };
                    if let Some(Panel::Pattern(pp)) = &mut self.panel {
                        match pp.features.iter().position(|f| *f == id) {
                            Some(i) if toggle => {
                                pp.features.remove(i);
                            }
                            Some(_) => {}
                            None => pp.features.push(id),
                        }
                    }
                }
                (Slot::Dir1 | Slot::Dir2 | Slot::Axis, Pick::Edge { body, edge }) => {
                    let Some(r) = self.edge_ref_of(body, edge) else { return false };
                    if let Some(Panel::Pattern(pp)) = &mut self.panel {
                        match slot {
                            Slot::Dir1 => pp.dir1 = DirectionRef::Edge(r),
                            Slot::Dir2 => {
                                pp.dir2 = Some(DirectionRef::Edge(r));
                                if pp.count2 < 2.0 {
                                    pp.count2 = 2.0;
                                }
                            }
                            _ => pp.axis = AxisSel::Edge(r),
                        }
                    }
                }
                (Slot::Axis | Slot::Plane, Pick::Face { body, face }) => {
                    let Some(r) = self.face_ref_of(body, face) else { return false };
                    if let Some(Panel::Pattern(pp)) = &mut self.panel {
                        if slot == Slot::Axis {
                            pp.axis = AxisSel::Face(r);
                        } else {
                            pp.plane = PlaneRef::Face(r);
                        }
                    }
                }
                _ => return false,
            }
            return true;
        }
        let present = self.panel_picks().contains(&p);
        let scene = &self.scene;
        let (eref, fref) = match p {
            Pick::Edge { body, edge } => (self.edge_ref_of(body, edge), None),
            Pick::Face { body, face } => (None, self.face_ref_of(body, face)),
        };
        // What the edge is, for the panels that take it differently: a sharp edge between two
        // faces of the sheet, or not.
        let thickness = self.document().sheet_sizes().ok().map(|s| s.thickness);
        let sharp = matches!((p, thickness), (Pick::Edge { body, edge }, Some(t)) if scene.bodies.get(body).is_some_and(|b| tenon_model::sheet::sharp_edge(b, edge, t)));
        let mut refused: Option<&str> = None;
        let same_edge = |r: &EdgeRef| matches!(p, Pick::Edge { body, edge } if find_edge(scene, r) == Some((body, edge)));
        let same_face = |r: &FaceRef| matches!(p, Pick::Face { body, face } if find_face(scene, r) == Some((body, face)));
        match (&mut self.panel, eref, fref) {
            (Some(Panel::Fillet(f)), Some(r), _) => edit_list(&mut f.edges, r, present, toggle, same_edge),
            (Some(Panel::Flange(f)), Some(r), _) => edit_list(&mut f.edges, r, present, toggle, same_edge),
            (Some(Panel::Hem(f)), Some(r), _) => edit_list(&mut f.edges, r, present, toggle, same_edge),
            (Some(Panel::ContourFlange(f)), Some(r), _) => edit_list(&mut f.edges, r, present, toggle, same_edge),
            (Some(Panel::Bend(f)), Some(r), _) => {
                // The first edge says which bend it is: on sharp edges, as many as are picked, or
                // between an edge of each of two faces (a third takes the place of the second).
                if f.edges.is_empty() && f.editing.is_none() {
                    f.sharp = sharp;
                }
                if f.sharp != sharp && !present {
                    refused = Some(if f.sharp {
                        "Bend: this bend is on sharp edges, and that edge is not one (it is on the rim of the sheet). Clear the edges to bend between two faces."
                    } else {
                        "Bend: this bend joins an edge of each of two faces, and that edge is a sharp edge between two. Clear the edges to bend sharp edges."
                    });
                } else {
                    edit_list(&mut f.edges, r, present, toggle, same_edge);
                    if !f.sharp && f.edges.len() > 2 {
                        f.edges.remove(1);
                    }
                }
            }
            (Some(Panel::CornerSeam(f)), Some(r), _) => {
                edit_list(&mut f.edges, r, present, toggle, same_edge);
                // Two edges make a corner to close: a third takes the place of the second. (A
                // rip opens as many corners as are picked.)
                if !f.rip && f.edges.len() > 2 {
                    f.edges.remove(1);
                }
            }
            (Some(Panel::Chamfer(c)), Some(r), _) => edit_list(&mut c.edges, r, present, toggle, same_edge),
            (Some(Panel::Chamfer(c)), None, Some(r)) if c.method != ChamferMethod::Distance => {
                if present && toggle {
                    c.reference = None;
                } else {
                    c.reference = Some(r);
                }
            }
            (Some(Panel::Shell(s)), None, Some(r)) => edit_list(&mut s.faces, r, present, toggle, same_face),
            // (A support or a load is on faces or on edges, never on both.)
            (Some(Panel::Support(s)), None, Some(r)) if s.edges.is_empty() => edit_list(&mut s.faces, r, present, toggle, same_face),
            (Some(Panel::Support(s)), Some(r), _) if s.faces.is_empty() && s.kind == tenon_model::study::SupportKind::Fixed => {
                edit_list(&mut s.edges, r, present, toggle, same_edge);
            }
            (Some(Panel::Load(l)), None, Some(r)) if l.edges.is_empty() => edit_list(&mut l.faces, r, present, toggle, same_face),
            (Some(Panel::Load(l)), Some(r), _) if l.faces.is_empty() && l.what == crate::analysis::What::Force => {
                edit_list(&mut l.edges, r, present, toggle, same_edge);
            }
            _ => return false,
        }
        if let Some(why) = refused {
            self.set_error(why.to_string());
        }
        self.load_picked();
        true
    }

    /// Sketch points a hole can go at, with their screen positions.
    fn hole_point_marks(&self, rect: Rect) -> Vec<(EntityId, Pos2)> {
        let Some(Panel::Hole(p)) = &self.panel else { return Vec::new() };
        let (Some(sk), Some(frame)) = (self.document().sketch(p.sketch), self.sketch_frame(p.sketch)) else { return Vec::new() };
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        sk.entities()
            .filter(|(id, e)| !e.construction || p.points.contains(id))
            .filter_map(|(id, _)| {
                let at = frame.plane_point(sk.point(id)?);
                self.view.camera.project(at, w, h).map(|(x, y, _)| (id, rect.min + vec2(x as f32, y as f32)))
            })
            .collect()
    }

    /// The hole centre candidate under the pointer.
    fn hole_point_at(&self, pos: Option<Pos2>, rect: Rect) -> Option<EntityId> {
        let pos = pos?;
        self.hole_point_marks(rect)
            .into_iter()
            .map(|(id, at)| (id, at.distance(pos)))
            .filter(|(_, d)| *d <= 9.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Measure: the shortest distance drawn between its nearest points, with its value.
    pub(crate) fn measure_overlay(&self, ui: &egui::Ui, rect: Rect, t: &Tokens) {
        let Some(Panel::Measure(m)) = &self.panel else { return };
        let Some(Ok(result)) = &m.result else { return };
        let (Some((a, b)), Some(d)) = (result.nearest, result.get("Distance")) else { return };
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let screen = |q: tenon_geom::Vec3| self.view.camera.project(q, w, h).map(|(x, y, _)| rect.min + vec2(x as f32, y as f32));
        let (Some(sa), Some(sb)) = (screen(a), screen(b)) else { return };
        let p = ui.painter().with_clip_rect(rect);
        p.line_segment([sa, sb], Stroke::new(1.5, t.accent));
        p.circle_filled(sa, 3.0, t.accent);
        p.circle_filled(sb, 3.0, t.accent);
        let label = format!("{} mm", crate::properties::fmt_value(d));
        let at = sa + (sb - sa) * 0.5 + vec2(6.0, -6.0);
        let galley = p.layout_no_wrap(label, crate::theme::body(), t.text);
        p.rect_filled(Rect::from_min_size(at - vec2(3.0, 2.0), galley.size() + vec2(6.0, 4.0)), 3.0, t.panel.gamma_multiply(0.9));
        p.galley(at, galley, t.text);
    }

    /// The hole panel's centre points on the part: picked ones filled, the others hollow.
    pub(crate) fn hole_markers(&self, ui: &egui::Ui, rect: Rect, t: &Tokens) {
        let Some(Panel::Hole(p)) = &self.panel else { return };
        let hover = self.hole_point_at(ui.input(|i| i.pointer.hover_pos()).filter(|q| rect.contains(*q)), rect);
        let painter = ui.painter().with_clip_rect(rect);
        for (id, at) in self.hole_point_marks(rect) {
            let on = p.points.contains(&id);
            let color = if on { t.accent } else { t.text_dim };
            if hover == Some(id) {
                painter.circle_stroke(at, 8.0, Stroke::new(2.0, t.accent));
            }
            if on {
                painter.circle_filled(at, 4.5, color);
                painter.line_segment([at - vec2(8.0, 0.0), at + vec2(8.0, 0.0)], Stroke::new(1.0, color));
                painter.line_segment([at - vec2(0.0, 8.0), at + vec2(0.0, 8.0)], Stroke::new(1.0, color));
            } else {
                painter.circle_stroke(at, 4.5, Stroke::new(1.5, color));
            }
        }
    }

    /// Hover and clicks in the viewport while a Fillet, Chamfer, Shell or Hole panel is open.
    /// Returns false when no such panel is open.
    pub(crate) fn panel_pointer(&mut self, resp: &egui::Response, rect: Rect) -> bool {
        if matches!(self.panel, Some(Panel::Measure(_))) {
            // Any face or edge: the first click is the first entity, the second the second, a
            // third starts over.
            let (meshes, tree) = (self.scene.placed(), self.body_tree());
            let hover = resp.hover_pos().and_then(|p| {
                let (x, y, w, h) = (f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
                if let Some(e) = pick_edge_in(tree.as_deref(), &meshes, &self.view.camera, w, h, x, y, 5.0) {
                    return Some(Pick::Edge { body: e.body, edge: e.edge });
                }
                let (o, d) = self.view.camera.ray(x, y, w, h);
                pick_face_in(tree.as_deref(), &meshes, o, d).map(|f| Pick::Face { body: f.body, face: f.face })
            });
            self.view.hover = hover;
            if resp.clicked()
                && let Some(p) = hover
            {
                if let Some(Panel::Measure(m)) = &mut self.panel {
                    match (m.a, m.b) {
                        (None, _) => m.a = Some(p),
                        (Some(_), None) => m.b = Some(p),
                        _ => {
                            m.a = Some(p);
                            m.b = None;
                        }
                    }
                }
                self.request_measure();
            }
            return true;
        }
        if matches!(self.panel, Some(Panel::Hole(_))) {
            self.view.hover = None;
            let point = self.hole_point_at(resp.interact_pointer_pos(), rect);
            if resp.clicked()
                && let Some(id) = point
                && let Some(Panel::Hole(p)) = &mut self.panel
            {
                if let Some(i) = p.points.iter().position(|q| *q == id) {
                    p.points.remove(i);
                } else {
                    p.points.push(id);
                }
                return true;
            }
            // Termination To: a work plane or a flat face is where the holes go.
            if self.picking_to() {
                self.pick_end_plane(resp, rect);
            }
            return true;
        }
        if matches!(self.panel, Some(Panel::Revolve(_) | Panel::Coil(_))) {
            // A work axis, or a line of the profile's sketch, clicked: the axis to turn about.
            if let Some(pos) = resp.hover_pos()
                && let Some((id, _)) = self.work_at(pos, rect, false, true)
            {
                self.view.hover = None;
                self.view.work_hover = Some(id);
                if resp.clicked() {
                    self.pick_reference(Reference::Work(id));
                }
                return true;
            }
            if let Some(line) = self.axis_line_at(resp.hover_pos(), rect) {
                self.view.hover = None;
                if resp.clicked() {
                    self.set_panel_axis(crate::panels::AxisChoice::Line(line));
                }
                return true;
            }
        }
        if matches!(
            self.panel,
            Some(Panel::Extrude(_) | Panel::Revolve(_) | Panel::Sweep(_) | Panel::Coil(_) | Panel::SheetFace(_) | Panel::SheetCut(_))
        ) {
            // A click inside a closed region of the profile's sketch: that region joins the
            // feature, or leaves it.
            if resp.clicked()
                && let Some(pos) = resp.interact_pointer_pos()
                && self.toggle_profile_at(pos, rect)
            {
                self.view.hover = None;
                return true;
            }
            // Extrude To: elsewhere, a work plane or a flat face is where it goes.
            if self.picking_to() {
                self.pick_end_plane(resp, rect);
                return true;
            }
            return false;
        }
        let Some(wants) = self.panel_wants() else { return false };
        // Work planes and axes are drawn over the part, so they are picked first.
        let (planes, axes) = self.work_accepted();
        if (planes || axes)
            && let Some(pos) = resp.hover_pos()
            && let Some((id, _)) = self.work_at(pos, rect, planes, axes)
        {
            self.view.hover = None;
            self.view.work_hover = Some(id);
            if resp.clicked() {
                self.pick_reference(Reference::Work(id));
            }
            return true;
        }
        let (meshes, tree) = (self.scene.placed(), self.body_tree());
        let hover = resp.hover_pos().and_then(|p| {
            let (x, y, w, h) = (f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
            if wants.edges
                && let Some(e) = pick_edge_in(tree.as_deref(), &meshes, &self.view.camera, w, h, x, y, 5.0)
            {
                let pick = Pick::Edge { body: e.body, edge: e.edge };
                if self.referable(pick, wants) {
                    return Some(pick);
                }
            }
            if !wants.faces {
                return None;
            }
            let (o, d) = self.view.camera.ray(x, y, w, h);
            pick_face_in(tree.as_deref(), &meshes, o, d).map(|f| Pick::Face { body: f.body, face: f.face }).filter(|p| self.referable(*p, wants))
        });
        self.view.hover = hover;
        if resp.clicked()
            && let Some(p) = hover
        {
            self.pick_into_panel(p, true);
        }
        true
    }

    /// Moves a box selection into the open panel. Returns false when no picking panel is open.
    pub(crate) fn absorb_selection(&mut self) -> bool {
        let Some(wants) = self.panel_wants() else { return false };
        let picks: Vec<Pick> = std::mem::take(&mut self.view.selection).into_iter().filter(|p| self.referable(*p, wants)).collect();
        for p in picks {
            if !self.panel_picks().contains(&p) {
                self.pick_into_panel(p, false);
            }
        }
        true
    }

    /// Clears the open panel's edges or faces.
    pub(crate) fn clear_panel_picks(&mut self) {
        match &mut self.panel {
            Some(Panel::Fillet(f)) => f.edges.clear(),
            Some(Panel::Flange(f)) => f.edges.clear(),
            Some(Panel::Hem(f)) => f.edges.clear(),
            Some(Panel::CornerSeam(f)) => f.edges.clear(),
            Some(Panel::Bend(f)) => f.edges.clear(),
            Some(Panel::ContourFlange(f)) => f.edges.clear(),
            Some(Panel::Chamfer(c)) => c.edges.clear(),
            Some(Panel::Shell(s)) => s.faces.clear(),
            Some(Panel::Support(s)) => {
                s.faces.clear();
                s.edges.clear();
            }
            Some(Panel::Load(l)) => {
                l.faces.clear();
                l.edges.clear();
            }
            Some(Panel::Hole(h)) => h.points.clear(),
            Some(Panel::Pattern(p)) => p.features.clear(),
            Some(Panel::Draft(d)) => d.faces.clear(),
            Some(Panel::Combine(c)) => c.tools.clear(),
            _ => {}
        }
    }

    /// Where to put the mini-toolbar for a picking panel: beside the last picked edge or face.
    pub(crate) fn panel_anchor(&self) -> Option<tenon_geom::Vec3> {
        match self.panel_picks().last()? {
            Pick::Edge { body, edge } => self.scene.bodies.get(*body)?.edges.get(*edge as usize).map(|(_, fp)| fp.mid),
            Pick::Face { body, face } => self.scene.bodies.get(*body)?.faces.get(*face as usize).map(|(_, info)| info.centroid),
        }
    }
}

fn edit_list<R>(list: &mut Vec<R>, r: R, present: bool, toggle: bool, same: impl Fn(&R) -> bool) {
    if present {
        if toggle {
            list.retain(|x| !same(x));
        }
    } else {
        list.push(r);
    }
}
