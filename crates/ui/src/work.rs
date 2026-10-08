//! Work planes, axes and points: drawn on the part, picked in the viewport and the browser, and
//! the panel that makes them.

use egui::{Align2, Pos2, Rect, Shape, Stroke, Ui, vec2};
use serde_json::{Value, json};
use tenon_geom::{Axis, Frame, Vec2, Vec3};
use tenon_model::{AxisSel, EdgeRef, FeatureId, FeatureKind, OriginAxis, OriginPlane, PlaneRef, WorkAxis, WorkGeom, WorkPlane, WorkPoint};

use crate::panels::{Panel, PatternPanel, Slot};
use crate::theme::{self, Tokens};
use crate::viewport::Pick;
use crate::workbench::Workbench;

/// How a work feature is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkMethod {
    Offset,
    Angle,
    Midplane,
    Along,
    Planes,
    Center,
    Intersection,
}

impl WorkMethod {
    pub(crate) fn label(self) -> &'static str {
        match self {
            WorkMethod::Offset => "Offset from Plane",
            WorkMethod::Angle => "Angle to Plane around Edge",
            WorkMethod::Midplane => "Midplane between Two Planes",
            WorkMethod::Along => "On Edge, Axis or Cylinder",
            WorkMethod::Planes => "Intersection of Two Planes",
            WorkMethod::Center => "Center of Circular Edge",
            WorkMethod::Intersection => "Axis through Plane",
        }
    }
    /// The methods of the same kind (plane, axis or point).
    pub(crate) fn family(self) -> &'static [WorkMethod] {
        match self {
            WorkMethod::Offset | WorkMethod::Angle | WorkMethod::Midplane => &[WorkMethod::Offset, WorkMethod::Angle, WorkMethod::Midplane],
            WorkMethod::Along | WorkMethod::Planes => &[WorkMethod::Along, WorkMethod::Planes],
            WorkMethod::Center | WorkMethod::Intersection => &[WorkMethod::Center, WorkMethod::Intersection],
        }
    }
    /// The selectors it needs, in order.
    pub(crate) fn slots(self) -> &'static [WorkSlot] {
        match self {
            WorkMethod::Offset => &[WorkSlot::A],
            WorkMethod::Angle => &[WorkSlot::A, WorkSlot::Axis],
            WorkMethod::Midplane | WorkMethod::Planes => &[WorkSlot::A, WorkSlot::B],
            WorkMethod::Along => &[WorkSlot::Axis],
            WorkMethod::Center => &[WorkSlot::Edge],
            WorkMethod::Intersection => &[WorkSlot::Axis, WorkSlot::A],
        }
    }
}

/// Which selector of the work panel takes the next pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkSlot {
    A,
    B,
    Axis,
    Edge,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkPanel {
    pub editing: Option<FeatureId>,
    pub method: WorkMethod,
    pub a: Option<PlaneRef>,
    pub b: Option<PlaneRef>,
    pub axis: Option<AxisSel>,
    pub edge: Option<EdgeRef>,
    pub distance: f64,
    pub degrees: f64,
    pub slot: WorkSlot,
}

impl WorkPanel {
    fn new(method: WorkMethod) -> WorkPanel {
        WorkPanel { editing: None, method, a: None, b: None, axis: None, edge: None, distance: 10.0, degrees: 90.0, slot: method.slots()[0] }
    }

    fn from_kind(editing: Option<FeatureId>, k: &FeatureKind) -> Option<WorkPanel> {
        let mut p = match k {
            FeatureKind::WorkPlane(WorkPlane::Offset { base, distance }) => {
                WorkPanel { a: Some(base.clone()), distance: *distance, ..WorkPanel::new(WorkMethod::Offset) }
            }
            FeatureKind::WorkPlane(WorkPlane::Angle { base, axis, angle }) => {
                WorkPanel { a: Some(base.clone()), axis: Some(axis.clone()), degrees: angle.to_degrees(), ..WorkPanel::new(WorkMethod::Angle) }
            }
            FeatureKind::WorkPlane(WorkPlane::Midplane { a, b }) => {
                WorkPanel { a: Some(a.clone()), b: Some(b.clone()), ..WorkPanel::new(WorkMethod::Midplane) }
            }
            FeatureKind::WorkAxis(WorkAxis::Along { axis }) => WorkPanel { axis: Some(axis.clone()), ..WorkPanel::new(WorkMethod::Along) },
            FeatureKind::WorkAxis(WorkAxis::Planes { a, b }) => {
                WorkPanel { a: Some(a.clone()), b: Some(b.clone()), ..WorkPanel::new(WorkMethod::Planes) }
            }
            FeatureKind::WorkPoint(WorkPoint::Center { edge }) => WorkPanel { edge: Some(edge.clone()), ..WorkPanel::new(WorkMethod::Center) },
            FeatureKind::WorkPoint(WorkPoint::Intersection { axis, plane }) => {
                WorkPanel { axis: Some(axis.clone()), a: Some(plane.clone()), ..WorkPanel::new(WorkMethod::Intersection) }
            }
            _ => return None,
        };
        p.editing = editing;
        Some(p)
    }

    pub(crate) fn title(&self) -> &'static str {
        match self.method {
            WorkMethod::Offset | WorkMethod::Angle | WorkMethod::Midplane => "Work Plane",
            WorkMethod::Along | WorkMethod::Planes => "Work Axis",
            WorkMethod::Center | WorkMethod::Intersection => "Work Point",
        }
    }

    pub(crate) fn command(&self) -> &'static str {
        match self.title() {
            "Work Plane" => "work.plane",
            "Work Axis" => "work.axis",
            _ => "work.point",
        }
    }

    /// The feature as set up, if every selector is filled.
    pub(crate) fn kind(&self) -> Option<FeatureKind> {
        Some(match self.method {
            WorkMethod::Offset => FeatureKind::WorkPlane(WorkPlane::Offset { base: self.a.clone()?, distance: self.distance }),
            WorkMethod::Angle => {
                FeatureKind::WorkPlane(WorkPlane::Angle { base: self.a.clone()?, axis: self.axis.clone()?, angle: self.degrees.to_radians() })
            }
            WorkMethod::Midplane => FeatureKind::WorkPlane(WorkPlane::Midplane { a: self.a.clone()?, b: self.b.clone()? }),
            WorkMethod::Along => FeatureKind::WorkAxis(WorkAxis::Along { axis: self.axis.clone()? }),
            WorkMethod::Planes => FeatureKind::WorkAxis(WorkAxis::Planes { a: self.a.clone()?, b: self.b.clone()? }),
            WorkMethod::Center => FeatureKind::WorkPoint(WorkPoint::Center { edge: self.edge.clone()? }),
            WorkMethod::Intersection => FeatureKind::WorkPoint(WorkPoint::Intersection { axis: self.axis.clone()?, plane: self.a.clone()? }),
        })
    }

    /// After a selector is filled, the next empty one takes the picks.
    pub(crate) fn advance(&mut self) {
        let filled = |s: &WorkSlot| match s {
            WorkSlot::A => self.a.is_some(),
            WorkSlot::B => self.b.is_some(),
            WorkSlot::Axis => self.axis.is_some(),
            WorkSlot::Edge => self.edge.is_some(),
        };
        if let Some(next) = self.method.slots().iter().find(|s| !filled(s)) {
            self.slot = *next;
        }
    }

    /// What the active selector takes: (planes, axes).
    pub(crate) fn accepts(&self) -> (bool, bool) {
        match self.slot {
            WorkSlot::A | WorkSlot::B => (true, false),
            WorkSlot::Axis => (false, true),
            WorkSlot::Edge => (false, false),
        }
    }
}

/// Registry parameters for a plane.
pub(crate) fn plane_json(p: &PlaneRef) -> Value {
    match p {
        PlaneRef::Origin(o) => json!(format!("{o:?}").to_lowercase()),
        PlaneRef::Face(f) => json!(f),
        PlaneRef::Work(id) => json!({ "work": id.0 }),
    }
}

/// Registry parameters for an axis.
pub(crate) fn axis_json(a: &AxisSel) -> Value {
    match a {
        AxisSel::Origin(o) => json!(format!("{o:?}").to_lowercase()),
        AxisSel::Edge(e) => json!(e),
        AxisSel::Face(f) => json!(f),
        AxisSel::Work(id) => json!({ "work": id.0 }),
    }
}

/// The registry command and parameters for a work feature.
pub(crate) fn work_params(kind: &FeatureKind) -> Option<(&'static str, Value)> {
    Some(match kind {
        FeatureKind::WorkPlane(WorkPlane::Offset { base, distance }) => {
            ("work.plane", json!({ "by": "offset", "base": plane_json(base), "distance": distance }))
        }
        FeatureKind::WorkPlane(WorkPlane::Angle { base, axis, angle }) => {
            ("work.plane", json!({ "by": "angle", "base": plane_json(base), "axis": axis_json(axis), "angle": angle }))
        }
        FeatureKind::WorkPlane(WorkPlane::Midplane { a, b }) => ("work.plane", json!({ "by": "midplane", "a": plane_json(a), "b": plane_json(b) })),
        FeatureKind::WorkAxis(WorkAxis::Along { axis }) => ("work.axis", json!({ "axis": axis_json(axis) })),
        FeatureKind::WorkAxis(WorkAxis::Planes { a, b }) => ("work.axis", json!({ "a": plane_json(a), "b": plane_json(b) })),
        FeatureKind::WorkPoint(WorkPoint::Center { edge }) => ("work.point", json!({ "edge": edge })),
        FeatureKind::WorkPoint(WorkPoint::Intersection { axis, plane }) => {
            ("work.point", json!({ "axis": axis_json(axis), "plane": plane_json(plane) }))
        }
        _ => return None,
    })
}

impl Workbench {
    /// How big work planes are drawn, and how long axes.
    fn work_half(&self) -> f64 {
        self.scene.bbox().map_or(30.0, |b| (b.diagonal() * 0.35).max(15.0))
    }

    /// The centre of the part, for placing drawn work planes and axes.
    fn work_centre(&self) -> Vec3 {
        self.scene.bbox().map_or(Vec3::ZERO, |b| b.center())
    }

    fn plane_corners(&self, f: &Frame) -> [Vec3; 4] {
        let h = self.work_half();
        let c = f.to_local(self.work_centre());
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(a, b)| f.plane_point(Vec2::new(c.x + a * h, c.y + b * h)))
    }

    fn axis_ends(&self, a: &Axis) -> (Vec3, Vec3) {
        let h = self.work_half() * 1.3;
        let t = (self.work_centre() - a.origin()).dot(a.dir());
        let m = a.origin() + a.dir() * t;
        (m - a.dir() * h, m + a.dir() * h)
    }

    fn to_screen(&self, p: Vec3, rect: Rect) -> Option<Pos2> {
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        self.view.camera.project(p, w, h).map(|(x, y, _)| rect.min + vec2(x as f32, y as f32))
    }

    /// The work feature under a screen point among planes and/or axes, and how far along the
    /// pointer ray it is.
    pub(crate) fn work_at(&self, p: Pos2, rect: Rect, planes: bool, axes: bool) -> Option<(FeatureId, f64)> {
        let (o, d) =
            self.view.camera.ray(f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
        let half = self.work_half();
        let mut best: Option<(FeatureId, f64)> = None;
        let mut take = |id: FeatureId, t: f64| {
            if best.is_none_or(|b| t < b.1) {
                best = Some((id, t));
            }
        };
        for (id, g) in &self.scene.work {
            match g {
                WorkGeom::Plane(f) if planes => {
                    let den = d.dot(f.z());
                    if den.abs() < 1e-9 {
                        continue;
                    }
                    let t = (f.origin() - o).dot(f.z()) / den;
                    let c = f.to_local(self.work_centre());
                    let l = f.to_local(o + d * t);
                    if t > 0.0 && (l.x - c.x).abs() <= half && (l.y - c.y).abs() <= half {
                        take(*id, t);
                    }
                }
                WorkGeom::Axis(a) if axes => {
                    let (e0, e1) = self.axis_ends(a);
                    if let (Some(s0), Some(s1)) = (self.to_screen(e0, rect), self.to_screen(e1, rect)) {
                        let seg = s1 - s0;
                        let k = ((p - s0).dot(seg) / seg.length_sq().max(1e-6)).clamp(0.0, 1.0);
                        if (s0 + seg * k).distance(p) <= 6.0 {
                            take(*id, (e0.lerp(e1, f64::from(k)) - o).dot(d));
                        }
                    }
                }
                _ => {}
            }
        }
        best
    }

    /// Draws the work features; the one under the pointer, and those picked in a panel, stand out.
    pub(crate) fn draw_work(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        if self.scene.work.is_empty() {
            return;
        }
        let picked = self.panel_work_refs();
        let painter = ui.painter().with_clip_rect(rect);
        let color = t.tint_work;
        for (id, g) in &self.scene.work {
            let hot = self.view.work_hover == Some(*id) || picked.contains(id);
            let name = self.feature_name(*id);
            match g {
                WorkGeom::Plane(f) => {
                    let pts: Option<Vec<Pos2>> = self.plane_corners(f).iter().map(|q| self.to_screen(*q, rect)).collect();
                    let Some(pts) = pts else { continue };
                    painter.add(Shape::convex_polygon(pts.clone(), color.gamma_multiply(if hot { 0.4 } else { 0.14 }), Stroke::NONE));
                    painter.add(Shape::closed_line(pts.clone(), Stroke::new(if hot { 2.0 } else { 1.0 }, color)));
                    painter.text(pts[3] + vec2(4.0, 4.0), Align2::LEFT_TOP, name, theme::small(), if hot { t.text } else { t.viewport_text });
                }
                WorkGeom::Axis(a) => {
                    let (e0, e1) = self.axis_ends(a);
                    if let (Some(s0), Some(s1)) = (self.to_screen(e0, rect), self.to_screen(e1, rect)) {
                        painter.extend(Shape::dashed_line(&[s0, s1], Stroke::new(if hot { 2.5 } else { 1.5 }, color), 10.0, 4.0));
                        painter.text(s1 + vec2(4.0, 0.0), Align2::LEFT_CENTER, name, theme::small(), if hot { t.text } else { t.viewport_text });
                    }
                }
                WorkGeom::Point(q) => {
                    if let Some(s) = self.to_screen(*q, rect) {
                        painter.circle_filled(s, if hot { 5.0 } else { 3.5 }, color);
                        painter.text(s + vec2(6.0, -6.0), Align2::LEFT_BOTTOM, name, theme::small(), if hot { t.text } else { t.viewport_text });
                    }
                }
            }
        }
    }

    /// Work features the open panel refers to (to highlight them).
    fn panel_work_refs(&self) -> Vec<FeatureId> {
        let plane = |p: &Option<PlaneRef>| match p {
            Some(PlaneRef::Work(id)) => Some(*id),
            _ => None,
        };
        let axis = |a: &Option<AxisSel>| match a {
            Some(AxisSel::Work(id)) => Some(*id),
            _ => None,
        };
        match &self.panel {
            Some(Panel::Work(w)) => [plane(&w.a), plane(&w.b), axis(&w.axis)].into_iter().flatten().collect(),
            Some(Panel::Pattern(p)) => [plane(&Some(p.plane.clone())), axis(&Some(p.axis.clone()))].into_iter().flatten().collect(),
            _ => Vec::new(),
        }
    }

    /// Which work features the open panel's active selector takes: (planes, axes).
    pub(crate) fn work_accepted(&self) -> (bool, bool) {
        match &self.panel {
            Some(Panel::Work(w)) => w.accepts(),
            Some(Panel::Pattern(p)) => match p.slot {
                Slot::Plane => (true, false),
                Slot::Axis | Slot::Dir1 | Slot::Dir2 => (false, true),
                Slot::Features => (false, false),
            },
            _ => (false, false),
        }
    }

    /// Puts a work feature, an origin plane or an origin axis into the open panel's active
    /// selector. Returns false when it does not take it.
    pub(crate) fn pick_reference(&mut self, r: Reference) -> bool {
        let is_plane = |wb: &Workbench, id: FeatureId| wb.document().feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::WorkPlane(_)));
        let is_axis = |wb: &Workbench, id: FeatureId| wb.document().feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::WorkAxis(_)));
        let plane = match r {
            Reference::Plane(o) => Some(PlaneRef::Origin(o)),
            Reference::Work(id) if is_plane(self, id) => Some(PlaneRef::Work(id)),
            _ => None,
        };
        let axis = match r {
            Reference::Axis(o) => Some(AxisSel::Origin(o)),
            Reference::Work(id) if is_axis(self, id) => Some(AxisSel::Work(id)),
            _ => None,
        };
        match &mut self.panel {
            Some(Panel::Work(w)) => {
                match (w.slot, plane, axis) {
                    (WorkSlot::A, Some(p), _) => w.a = Some(p),
                    (WorkSlot::B, Some(p), _) => w.b = Some(p),
                    (WorkSlot::Axis, _, Some(a)) => w.axis = Some(a),
                    _ => return false,
                }
                w.advance();
                true
            }
            Some(Panel::Pattern(p)) => set_pattern_ref(p, plane, axis),
            _ => false,
        }
    }

    /// Work Plane, Work Axis or Work Point. A new one starts from what is selected.
    pub(crate) fn open_work(&mut self, method: WorkMethod, editing: Option<FeatureId>) -> Result<(), String> {
        let mut panel = match editing {
            Some(id) => WorkPanel::from_kind(editing, &self.document().feature(id).ok_or("no such feature")?.kind).ok_or("not a work feature")?,
            None => WorkPanel::new(method),
        };
        if editing.is_none() {
            // A selected face or edge fills the first selector.
            for p in std::mem::take(&mut self.view.selection) {
                match p {
                    Pick::Face { body, face } => {
                        if let Some((name, info)) = self.scene.bodies.get(body).and_then(|b| b.faces.get(face as usize))
                            && let Some(origin) = name
                        {
                            let fref = tenon_model::FaceRef { origin: *origin, fingerprint: tenon_model::Fingerprint::of(info) };
                            match info.surface {
                                tenon_kernel::SurfaceKind::Plane { .. } if panel.a.is_none() && panel.method != WorkMethod::Along => {
                                    panel.a = Some(PlaneRef::Face(fref))
                                }
                                tenon_kernel::SurfaceKind::Cylinder { .. } | tenon_kernel::SurfaceKind::Cone { .. } if panel.axis.is_none() => {
                                    panel.axis = Some(AxisSel::Face(fref))
                                }
                                _ => {}
                            }
                        }
                    }
                    Pick::Edge { body, edge } => {
                        if let Some(e) = self.scene.bodies.get(body).and_then(|b| b.edge_ref(edge)) {
                            if panel.method == WorkMethod::Center {
                                panel.edge.get_or_insert(e);
                            } else if panel.axis.is_none() {
                                panel.axis = Some(AxisSel::Edge(e));
                            }
                        }
                    }
                }
            }
            panel.advance();
        }
        self.set_status(match panel.title() {
            "Work Plane" => "Work Plane: click a face or plane (origin planes are in the browser), set the offset, then OK.",
            "Work Axis" => "Work Axis: click an edge, a cylinder or an axis, then OK.",
            _ => "Work Point: click a circular edge, then OK.",
        });
        self.panel = Some(Panel::Work(Box::new(panel)));
        self.start_equations();
        Ok(())
    }

    /// Viewport picks for the work panel: faces, edges and drawn work features.
    pub(crate) fn work_pick(&mut self, p: Pick) -> bool {
        let Some(Panel::Work(w)) = &self.panel else { return false };
        let slot = w.slot;
        let (fref, eref, surface) = match p {
            Pick::Face { body, face } => {
                let Some((name, info)) = self.scene.bodies.get(body).and_then(|b| b.faces.get(face as usize)) else { return false };
                let Some(origin) = name else { return false };
                (Some(tenon_model::FaceRef { origin: *origin, fingerprint: tenon_model::Fingerprint::of(info) }), None, Some(info.surface.clone()))
            }
            Pick::Edge { body, edge } => (None, self.scene.bodies.get(body).and_then(|b| b.edge_ref(edge)), None),
        };
        let Some(Panel::Work(w)) = &mut self.panel else { return false };
        match (slot, fref, eref, surface) {
            (WorkSlot::A, Some(f), _, Some(tenon_kernel::SurfaceKind::Plane { .. })) => w.a = Some(PlaneRef::Face(f)),
            (WorkSlot::B, Some(f), _, Some(tenon_kernel::SurfaceKind::Plane { .. })) => w.b = Some(PlaneRef::Face(f)),
            (WorkSlot::Axis, Some(f), _, Some(tenon_kernel::SurfaceKind::Cylinder { .. } | tenon_kernel::SurfaceKind::Cone { .. })) => {
                w.axis = Some(AxisSel::Face(f))
            }
            (WorkSlot::Axis, _, Some(e), _) => w.axis = Some(AxisSel::Edge(e)),
            (WorkSlot::Edge, _, Some(e), _) => w.edge = Some(e),
            _ => return false,
        }
        w.advance();
        true
    }
}

/// Something picked from the origin folder or a work feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reference {
    Plane(OriginPlane),
    Axis(OriginAxis),
    Work(FeatureId),
}

fn set_pattern_ref(p: &mut PatternPanel, plane: Option<PlaneRef>, axis: Option<AxisSel>) -> bool {
    match (p.slot, plane, axis) {
        (Slot::Plane, Some(pl), _) => p.plane = pl,
        (Slot::Axis, _, Some(a)) => p.axis = a,
        (Slot::Dir1, _, Some(a)) => match a {
            AxisSel::Origin(o) => p.dir1 = tenon_model::DirectionRef::Origin(o),
            AxisSel::Work(id) => p.dir1 = tenon_model::DirectionRef::Work(id),
            _ => return false,
        },
        (Slot::Dir2, _, Some(a)) => {
            p.dir2 = match a {
                AxisSel::Origin(o) => Some(tenon_model::DirectionRef::Origin(o)),
                AxisSel::Work(id) => Some(tenon_model::DirectionRef::Work(id)),
                _ => return false,
            };
            if p.count2 < 2.0 {
                p.count2 = 2.0;
            }
        }
        _ => return false,
    }
    true
}
