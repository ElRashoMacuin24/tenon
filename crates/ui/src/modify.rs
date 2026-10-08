//! Picking for the features that act on an existing body (Fillet, Chamfer, Shell): while one of
//! their panels is open, clicks in the viewport add or remove edges and faces, and the panel's
//! references are highlighted on the part.

use egui::{Pos2, Rect, Stroke, vec2};
use tenon_kernel::SurfaceKind;
use tenon_model::{AxisSel, DirectionRef, EdgeRef, FaceRef, FeatureId, Fingerprint, PlaneRef, Scene};
use tenon_render::pick::{pick_edge, pick_face};
use tenon_sketch::EntityId;

use crate::panels::{ChamferMethod, CopyKind, Panel, Slot};
use crate::theme::Tokens;
use crate::viewport::Pick;
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
            Some(Panel::Fillet(_)) => Some(Wants { edges: true, faces: false }),
            Some(Panel::Chamfer(p)) => Some(Wants { edges: true, faces: p.method != ChamferMethod::Distance }),
            Some(Panel::Shell(_)) => Some(Wants { edges: false, faces: true }),
            Some(Panel::Pattern(p)) => Some(match p.slot {
                Slot::Features | Slot::Plane => Wants { edges: false, faces: true },
                Slot::Dir1 | Slot::Dir2 => Wants { edges: true, faces: false },
                Slot::Axis => Wants { edges: true, faces: true },
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
            Some(Panel::Chamfer(p)) => {
                let mut v = edges(&p.edges);
                v.extend(p.reference.as_ref().and_then(face));
                v
            }
            Some(Panel::Shell(p)) => p.faces.iter().filter_map(face).collect(),
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
                            DirectionRef::Origin(_) => None,
                        })
                        .collect(),
                    CopyKind::Circular => match &p.axis {
                        AxisSel::Edge(e) => edges(std::slice::from_ref(e)),
                        AxisSel::Face(f) => face(f).into_iter().collect(),
                        AxisSel::Origin(_) => Vec::new(),
                    },
                    CopyKind::Mirror => match &p.plane {
                        PlaneRef::Face(f) => face(f).into_iter().collect(),
                        PlaneRef::Origin(_) => Vec::new(),
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

    fn face_ref_of(&self, body: usize, face: u32) -> Option<FaceRef> {
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
            (Pick::Edge { body, edge }, _) => wants.edges && self.edge_ref_of(body, edge).is_some(),
            (Pick::Face { body, face }, _) => wants.faces && self.face_ref_of(body, face).is_some(),
        }
    }

    /// Adds a pick to the panel, or removes it when it is there already (`toggle`). Returns
    /// false when the panel does not take it.
    pub(crate) fn pick_into_panel(&mut self, p: Pick, toggle: bool) -> bool {
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
        let same_edge = |r: &EdgeRef| matches!(p, Pick::Edge { body, edge } if find_edge(scene, r) == Some((body, edge)));
        let same_face = |r: &FaceRef| matches!(p, Pick::Face { body, face } if find_face(scene, r) == Some((body, face)));
        match (&mut self.panel, eref, fref) {
            (Some(Panel::Fillet(f)), Some(r), _) => edit_list(&mut f.edges, r, present, toggle, same_edge),
            (Some(Panel::Chamfer(c)), Some(r), _) => edit_list(&mut c.edges, r, present, toggle, same_edge),
            (Some(Panel::Chamfer(c)), None, Some(r)) if c.method != ChamferMethod::Distance => {
                if present && toggle {
                    c.reference = None;
                } else {
                    c.reference = Some(r);
                }
            }
            (Some(Panel::Shell(s)), None, Some(r)) => edit_list(&mut s.faces, r, present, toggle, same_face),
            _ => return false,
        }
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
        if matches!(self.panel, Some(Panel::Hole(_))) {
            self.view.hover = None;
            if resp.clicked()
                && let Some(id) = self.hole_point_at(resp.interact_pointer_pos(), rect)
                && let Some(Panel::Hole(p)) = &mut self.panel
            {
                if let Some(i) = p.points.iter().position(|q| *q == id) {
                    p.points.remove(i);
                } else {
                    p.points.push(id);
                }
            }
            return true;
        }
        let Some(wants) = self.panel_wants() else { return false };
        let meshes: Vec<&tenon_kernel::Mesh> = self.scene.bodies.iter().map(|b| &b.mesh).collect();
        let hover = resp.hover_pos().and_then(|p| {
            let (x, y, w, h) = (f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
            if wants.edges
                && let Some(e) = pick_edge(&meshes, &self.view.camera, w, h, x, y, 5.0)
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
            pick_face(&meshes, o, d).map(|f| Pick::Face { body: f.body, face: f.face }).filter(|p| self.referable(*p, wants))
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
            Some(Panel::Chamfer(c)) => c.edges.clear(),
            Some(Panel::Shell(s)) => s.faces.clear(),
            Some(Panel::Hole(h)) => h.points.clear(),
            Some(Panel::Pattern(p)) => p.features.clear(),
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
