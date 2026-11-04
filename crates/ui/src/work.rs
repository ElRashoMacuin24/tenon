//! Work planes, axes and points: drawn on the part, picked in the viewport and the browser, and
//! the panel that makes them.

use egui::{Align2, Pos2, Rect, Shape, Stroke, Ui, vec2};
use serde_json::{Value, json};
use tenon_geom::{Axis, Frame, Vec2, Vec3};
use tenon_model::{
    AxisSel, EdgeRef, FeatureId, FeatureKind, OriginAxis, OriginPlane, PlaneRef, PointSel, Ucs, WorkAxis, WorkGeom, WorkPlane, WorkPoint,
};

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
    /// Through three points (DEC-116).
    ThreePoints,
    Along,
    Planes,
    Center,
    Intersection,
    /// A UCS by typed offsets and turns, from the part's origin or another UCS (DEC-133).
    UcsOffset,
    /// A UCS through three picked points.
    UcsPoints,
}

impl WorkMethod {
    pub(crate) fn label(self) -> &'static str {
        match self {
            WorkMethod::Offset => "Offset from Plane",
            WorkMethod::Angle => "Angle to Plane around Edge",
            WorkMethod::Midplane => "Midplane between Two Planes",
            WorkMethod::ThreePoints => "Three Points",
            WorkMethod::Along => "On Edge, Axis or Cylinder",
            WorkMethod::Planes => "Intersection of Two Planes",
            WorkMethod::Center => "Center of Circular Edge",
            WorkMethod::Intersection => "Axis through Plane",
            WorkMethod::UcsOffset => "Offsets and Turns",
            WorkMethod::UcsPoints => "Three Points",
        }
    }
    /// The methods of the same kind (plane, axis or point).
    pub(crate) fn family(self) -> &'static [WorkMethod] {
        match self {
            WorkMethod::Offset | WorkMethod::Angle | WorkMethod::Midplane | WorkMethod::ThreePoints => {
                &[WorkMethod::Offset, WorkMethod::Angle, WorkMethod::Midplane, WorkMethod::ThreePoints]
            }
            WorkMethod::Along | WorkMethod::Planes => &[WorkMethod::Along, WorkMethod::Planes],
            WorkMethod::Center | WorkMethod::Intersection => &[WorkMethod::Center, WorkMethod::Intersection],
            WorkMethod::UcsOffset | WorkMethod::UcsPoints => &[WorkMethod::UcsOffset, WorkMethod::UcsPoints],
        }
    }
    /// The selectors it needs, in order.
    pub(crate) fn slots(self) -> &'static [WorkSlot] {
        match self {
            WorkMethod::Offset => &[WorkSlot::A],
            WorkMethod::Angle => &[WorkSlot::A, WorkSlot::Axis],
            WorkMethod::Midplane | WorkMethod::Planes => &[WorkSlot::A, WorkSlot::B],
            WorkMethod::ThreePoints | WorkMethod::UcsPoints => &[WorkSlot::P1, WorkSlot::P2, WorkSlot::P3],
            WorkMethod::UcsOffset => &[WorkSlot::Base],
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
    /// The points of a three-point plane.
    P1,
    P2,
    P3,
    /// What a UCS is measured from (nothing but a list: any is good, the part's origin too).
    Base,
}

impl WorkSlot {
    /// Which of three points it takes, if it takes a point.
    pub(crate) fn point(self) -> Option<usize> {
        match self {
            WorkSlot::P1 => Some(0),
            WorkSlot::P2 => Some(1),
            WorkSlot::P3 => Some(2),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct WorkPanel {
    pub editing: Option<FeatureId>,
    pub method: WorkMethod,
    pub a: Option<PlaneRef>,
    pub b: Option<PlaneRef>,
    pub axis: Option<AxisSel>,
    pub edge: Option<EdgeRef>,
    /// A three-point plane's points.
    pub points: [Option<PointSel>; 3],
    pub distance: f64,
    pub degrees: f64,
    pub slot: WorkSlot,
    /// A UCS by offsets and turns: what it is measured from, along its X, Y and Z, then in
    /// degrees about its own X, Y and Z.
    pub base: Option<FeatureId>,
    pub offsets: [f64; 3],
    pub turns: [f64; 3],
}

impl WorkPanel {
    fn new(method: WorkMethod) -> WorkPanel {
        WorkPanel {
            editing: None,
            method,
            a: None,
            b: None,
            axis: None,
            edge: None,
            points: [None, None, None],
            distance: 10.0,
            degrees: 90.0,
            slot: method.slots()[0],
            base: None,
            offsets: [0.0; 3],
            turns: [0.0; 3],
        }
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
            FeatureKind::WorkPlane(WorkPlane::ThreePoints { points }) => {
                WorkPanel { points: points.clone().map(Some), ..WorkPanel::new(WorkMethod::ThreePoints) }
            }
            FeatureKind::WorkAxis(WorkAxis::Along { axis }) => WorkPanel { axis: Some(axis.clone()), ..WorkPanel::new(WorkMethod::Along) },
            FeatureKind::WorkAxis(WorkAxis::Planes { a, b }) => {
                WorkPanel { a: Some(a.clone()), b: Some(b.clone()), ..WorkPanel::new(WorkMethod::Planes) }
            }
            FeatureKind::WorkPoint(WorkPoint::Center { edge }) => WorkPanel { edge: Some(edge.clone()), ..WorkPanel::new(WorkMethod::Center) },
            FeatureKind::WorkPoint(WorkPoint::Intersection { axis, plane }) => {
                WorkPanel { axis: Some(axis.clone()), a: Some(plane.clone()), ..WorkPanel::new(WorkMethod::Intersection) }
            }
            FeatureKind::Ucs(Ucs::Offset { base, x, y, z, rx, ry, rz }) => WorkPanel {
                base: *base,
                offsets: [*x, *y, *z],
                turns: [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()],
                ..WorkPanel::new(WorkMethod::UcsOffset)
            },
            FeatureKind::Ucs(Ucs::ThreePoints { points }) => {
                WorkPanel { points: (**points).clone().map(Some), ..WorkPanel::new(WorkMethod::UcsPoints) }
            }
            _ => return None,
        };
        p.editing = editing;
        Some(p)
    }

    pub(crate) fn title(&self) -> &'static str {
        match self.method {
            WorkMethod::Offset | WorkMethod::Angle | WorkMethod::Midplane | WorkMethod::ThreePoints => "Work Plane",
            WorkMethod::Along | WorkMethod::Planes => "Work Axis",
            WorkMethod::Center | WorkMethod::Intersection => "Work Point",
            WorkMethod::UcsOffset | WorkMethod::UcsPoints => "UCS",
        }
    }

    pub(crate) fn command(&self) -> &'static str {
        match self.title() {
            "Work Plane" => "work.plane",
            "Work Axis" => "work.axis",
            "UCS" => "work.ucs",
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
            WorkMethod::ThreePoints => {
                let [a, b, c] = &self.points;
                FeatureKind::WorkPlane(WorkPlane::ThreePoints { points: [a.clone()?, b.clone()?, c.clone()?] })
            }
            WorkMethod::Along => FeatureKind::WorkAxis(WorkAxis::Along { axis: self.axis.clone()? }),
            WorkMethod::Planes => FeatureKind::WorkAxis(WorkAxis::Planes { a: self.a.clone()?, b: self.b.clone()? }),
            WorkMethod::Center => FeatureKind::WorkPoint(WorkPoint::Center { edge: self.edge.clone()? }),
            WorkMethod::Intersection => FeatureKind::WorkPoint(WorkPoint::Intersection { axis: self.axis.clone()?, plane: self.a.clone()? }),
            WorkMethod::UcsOffset => FeatureKind::Ucs(Ucs::Offset {
                base: self.base,
                x: self.offsets[0],
                y: self.offsets[1],
                z: self.offsets[2],
                rx: self.turns[0].to_radians(),
                ry: self.turns[1].to_radians(),
                rz: self.turns[2].to_radians(),
            }),
            WorkMethod::UcsPoints => {
                let [a, b, c] = &self.points;
                FeatureKind::Ucs(Ucs::ThreePoints { points: Box::new([a.clone()?, b.clone()?, c.clone()?]) })
            }
        })
    }

    /// After a selector is filled, the next empty one takes the picks.
    pub(crate) fn advance(&mut self) {
        let filled = |s: &WorkSlot| match s {
            WorkSlot::A => self.a.is_some(),
            WorkSlot::B => self.b.is_some(),
            WorkSlot::Axis => self.axis.is_some(),
            WorkSlot::Edge => self.edge.is_some(),
            WorkSlot::P1 | WorkSlot::P2 | WorkSlot::P3 => s.point().is_some_and(|i| self.points[i].is_some()),
            WorkSlot::Base => true,
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
            WorkSlot::Edge | WorkSlot::P1 | WorkSlot::P2 | WorkSlot::P3 | WorkSlot::Base => (false, false),
        }
    }

    /// Puts a point into the active point selector, and moves on.
    pub(crate) fn put_point(&mut self, p: PointSel) -> bool {
        let Some(i) = self.slot.point() else { return false };
        self.points[i] = Some(p);
        self.advance();
        true
    }
}

/// Registry parameters for a plane.
pub(crate) fn plane_json(p: &PlaneRef) -> Value {
    match p {
        PlaneRef::Origin(o) => json!(format!("{o:?}").to_lowercase()),
        PlaneRef::Face(f) => json!(f),
        PlaneRef::Work(id) => json!({ "work": id.0 }),
        PlaneRef::Ucs { feature, plane } => json!({ "ucs": feature.0, "plane": format!("{plane:?}").to_lowercase() }),
    }
}

/// Registry parameters for an axis.
pub(crate) fn axis_json(a: &AxisSel) -> Value {
    match a {
        AxisSel::Origin(o) => json!(format!("{o:?}").to_lowercase()),
        AxisSel::Edge(e) => json!(e),
        AxisSel::Face(f) => json!(f),
        AxisSel::Work(id) => json!({ "work": id.0 }),
        AxisSel::Ucs { feature, axis } => json!({ "ucs": feature.0, "axis": format!("{axis:?}").to_lowercase() }),
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
        FeatureKind::WorkPlane(WorkPlane::ThreePoints { points }) => ("work.plane", json!({ "by": "three_points", "points": points })),
        FeatureKind::WorkAxis(WorkAxis::Along { axis }) => ("work.axis", json!({ "axis": axis_json(axis) })),
        FeatureKind::WorkAxis(WorkAxis::Planes { a, b }) => ("work.axis", json!({ "a": plane_json(a), "b": plane_json(b) })),
        FeatureKind::WorkPoint(WorkPoint::Center { edge }) => ("work.point", json!({ "edge": edge })),
        FeatureKind::WorkPoint(WorkPoint::Intersection { axis, plane }) => {
            ("work.point", json!({ "axis": axis_json(axis), "plane": plane_json(plane) }))
        }
        FeatureKind::Ucs(Ucs::Offset { base, x, y, z, rx, ry, rz }) => {
            let mut p = json!({ "by": "offset", "x": x, "y": y, "z": z, "rx": rx, "ry": ry, "rz": rz });
            if let Some(id) = base {
                p["base"] = json!(id.0);
            }
            ("work.ucs", p)
        }
        FeatureKind::Ucs(Ucs::ThreePoints { points }) => ("work.ucs", json!({ "by": "three_points", "points": points })),
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
            // (A hidden one is picked from the browser, not in the view.)
            if !self.work_visible(*id) {
                continue;
            }
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

    /// How long the axes of a UCS are drawn: a share of the part, kept between so many pixels of the
    /// view, so that it can be seen and clicked on a part of a millimetre and does not fill the view
    /// on one of two.
    pub(crate) fn ucs_size(&self, f: &Frame, rect: Rect) -> f64 {
        const PIXELS: (f64, f64) = (56.0, 130.0);
        let wanted = self.scene.bbox().map_or(13.5, |b| b.diagonal() * 0.16).max(1e-6);
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let camera = &self.view.camera;
        match (camera.project(f.origin(), w, h), camera.project(f.origin() + camera.right() * wanted, w, h)) {
            (Some(a), Some(b)) => {
                let per_mm = ((b.0 - a.0).hypot(b.1 - a.1) / wanted).max(1e-9);
                wanted.clamp(PIXELS.0 / per_mm, PIXELS.1 / per_mm)
            }
            _ => wanted,
        }
    }

    /// What of a UCS is drawn where, each as it stands on the screen: its origin, the end of each
    /// axis, and the square of each plane at the corner of the triad. None when its origin is behind
    /// the camera; of an axis or a square that reaches behind it, as much as lies in front.
    pub(crate) fn ucs_marks(&self, f: &Frame, rect: Rect) -> Option<(Pos2, [Pos2; 3], [[Pos2; 4]; 3])> {
        let size = self.ucs_size(f, rect);
        let o = self.to_screen(f.origin(), rect)?;
        // What lies in front, halving the way from the origin until it does.
        let seen = |at: Vec3| {
            let mut part = 1.0;
            for _ in 0..12 {
                if let Some(s) = self.to_screen(f.origin() + at * part, rect) {
                    return s;
                }
                part *= 0.5;
            }
            o
        };
        let s = size * 0.4;
        let square = |u: Vec3, v: Vec3| [o, seen(u * s), seen((u + v) * s), seen(v * s)];
        Some((o, [seen(f.x() * size), seen(f.y() * size), seen(f.z() * size)], [square(f.x(), f.y()), square(f.y(), f.z()), square(f.x(), f.z())]))
    }

    /// What a work feature is called in the view. One a panel shows as it would be has no name yet.
    fn work_label(&self, id: FeatureId, g: &WorkGeom) -> String {
        self.document().feature(id).map_or_else(
            || {
                match g {
                    WorkGeom::Plane(_) => "New plane",
                    WorkGeom::Axis(_) => "New axis",
                    WorkGeom::Point(_) => "New point",
                    WorkGeom::Ucs(_) => "New UCS",
                }
                .to_owned()
            },
            |f| f.name.clone(),
        )
    }

    /// The part of a UCS under a screen point among planes, axes and origins.
    pub(crate) fn ucs_at(&self, p: Pos2, rect: Rect, planes: bool, axes: bool, points: bool) -> Option<Reference> {
        const AXES: [OriginAxis; 3] = [OriginAxis::X, OriginAxis::Y, OriginAxis::Z];
        const PLANES: [OriginPlane; 3] = [OriginPlane::XY, OriginPlane::YZ, OriginPlane::XZ];
        for (id, g) in &self.scene.work {
            let WorkGeom::Ucs(f) = g else { continue };
            if !self.work_visible(*id) {
                continue;
            }
            let Some((o, tips, squares)) = self.ucs_marks(f, rect) else { continue };
            if points && o.distance(p) <= 8.0 {
                return Some(Reference::UcsOrigin(*id));
            }
            if axes {
                for (tip, a) in tips.iter().zip(AXES) {
                    let seg = *tip - o;
                    let k = ((p - o).dot(seg) / seg.length_sq().max(1e-6)).clamp(0.0, 1.0);
                    if (o + seg * k).distance(p) <= 6.0 {
                        return Some(Reference::UcsAxis(*id, a));
                    }
                }
            }
            if planes {
                for (square, plane) in squares.iter().zip(PLANES) {
                    let side = |a: Pos2, b: Pos2| (b - a).x * (p - a).y - (b - a).y * (p - a).x;
                    let s: Vec<f32> = (0..4).map(|i| side(square[i], square[(i + 1) % 4])).collect();
                    if s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0) {
                        return Some(Reference::UcsPlane(*id, plane));
                    }
                }
            }
        }
        None
    }

    /// The part of a UCS under the pointer, among what is asked for: lit, and picked by a click.
    /// Returns whether there is one.
    pub(crate) fn pick_ucs_part(&mut self, resp: &egui::Response, rect: Rect, planes: bool, axes: bool, points: bool) -> bool {
        self.view.ucs_hover = None;
        let Some(r) = resp.hover_pos().and_then(|pos| self.ucs_at(pos, rect, planes, axes, points)) else { return false };
        self.view.hover = None;
        self.view.ucs_hover = Some(r);
        if resp.clicked() {
            self.pick_reference(r);
        }
        true
    }

    /// An axis of UCS `id` as the part has it now: a point on it and its direction.
    pub(crate) fn ucs_line(&self, id: FeatureId, axis: OriginAxis) -> Option<(Vec3, Vec3)> {
        self.scene.work.iter().find(|(w, _)| *w == id).and_then(|(_, g)| match g {
            WorkGeom::Ucs(f) => tenon_model::regen::ucs_axis(f, axis).map(|a| (a.origin(), a.dir())),
            _ => None,
        })
    }

    /// Draws a UCS: three axes in their colours with their letters, the corner of each plane, and
    /// its name at its origin. The part under the pointer stands out.
    fn draw_ucs(&self, painter: &egui::Painter, id: FeatureId, name: String, f: &Frame, hot: bool, rect: Rect, t: &Tokens) {
        const COLORS: [egui::Color32; 3] =
            [egui::Color32::from_rgb(0xe0, 0x50, 0x50), egui::Color32::from_rgb(0x50, 0xb8, 0x60), egui::Color32::from_rgb(0x50, 0x80, 0xe8)];
        const LETTERS: [&str; 3] = ["X", "Y", "Z"];
        let Some((o, tips, squares)) = self.ucs_marks(f, rect) else { return };
        let lit = self.view.ucs_hover;
        for (square, plane) in squares.iter().zip([OriginPlane::XY, OriginPlane::YZ, OriginPlane::XZ]) {
            let on = lit == Some(Reference::UcsPlane(id, plane));
            painter.add(Shape::convex_polygon(
                square.to_vec(),
                t.tint_work.gamma_multiply(if on { 0.6 } else { 0.18 }),
                Stroke::new(if on { 2.0 } else { 0.8 }, t.tint_work),
            ));
        }
        for ((tip, color), (letter, axis)) in tips.iter().zip(COLORS).zip(LETTERS.into_iter().zip([OriginAxis::X, OriginAxis::Y, OriginAxis::Z])) {
            let on = lit == Some(Reference::UcsAxis(id, axis));
            painter.line_segment(
                [o, *tip],
                Stroke::new(
                    if on {
                        4.0
                    } else if hot {
                        3.0
                    } else {
                        2.0
                    },
                    color,
                ),
            );
            painter.text(*tip + vec2(3.0, -3.0), Align2::LEFT_BOTTOM, letter, theme::small(), color);
        }
        let on = lit == Some(Reference::UcsOrigin(id));
        painter.circle_filled(o, if on { 6.0 } else { 3.5 }, t.tint_work);
        // (On a backing: it may lie over a lit face.)
        let at = o + vec2(7.0, 7.0);
        let galley = painter.layout_no_wrap(name, theme::small(), t.text);
        painter.rect_filled(Rect::from_min_size(at - vec2(3.0, 1.0), galley.size() + vec2(6.0, 2.0)), 3.0, t.panel.gamma_multiply(0.85));
        painter.galley(at, galley, t.text);
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
            // (Hidden from the browser: not drawn, unless the open panel uses it.)
            if !hot && !self.work_visible(*id) {
                continue;
            }
            let name = self.work_label(*id, g);
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
                WorkGeom::Ucs(f) => self.draw_ucs(&painter, *id, name, f, hot, rect, t),
            }
        }
    }

    /// Whether a work feature is shown: unless the user hid it (DEC-106).
    pub(crate) fn work_visible(&self, id: FeatureId) -> bool {
        self.document().feature(id).is_none_or(|f| f.visible != Some(false))
    }

    /// Work features the open panel refers to (to highlight them).
    fn panel_work_refs(&self) -> Vec<FeatureId> {
        let plane = |p: &Option<PlaneRef>| match p {
            Some(PlaneRef::Work(id) | PlaneRef::Ucs { feature: id, .. }) => Some(*id),
            _ => None,
        };
        let axis = |a: &Option<AxisSel>| match a {
            Some(AxisSel::Work(id) | AxisSel::Ucs { feature: id, .. }) => Some(*id),
            _ => None,
        };
        match &self.panel {
            Some(Panel::Work(w)) => [plane(&w.a), plane(&w.b), axis(&w.axis), w.base].into_iter().flatten().collect(),
            Some(Panel::Derive(d)) => plane(&d.mirror).into_iter().collect(),
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
            Some(Panel::Draft(d)) => (d.slot == crate::bodies::DraftSlot::Plane, false),
            Some(Panel::Split(_)) => (true, false),
            Some(Panel::Primitive(p)) => (p.on.is_none(), false),
            Some(Panel::Revolve(_) | Panel::Coil(_)) => (false, true),
            // A load's direction may be along a work axis.
            Some(Panel::Load(_)) => (false, self.picking_direction()),
            _ => (false, false),
        }
    }

    /// Puts a work feature, an origin plane or an origin axis into the open panel's active
    /// selector. Returns false when it does not take it.
    pub(crate) fn pick_reference(&mut self, r: Reference) -> bool {
        let is_plane = |wb: &Workbench, id: FeatureId| wb.document().feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::WorkPlane(_)));
        let is_axis = |wb: &Workbench, id: FeatureId| wb.document().feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::WorkAxis(_)));
        let is_ucs = |wb: &Workbench, id: FeatureId| wb.document().feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::Ucs(_)));
        // Measure takes a work feature, or a part of a UCS, as what to measure.
        if matches!(self.panel, Some(Panel::Measure(_))) {
            use tenon_model::measure::{Entity, WorkPart};
            let work = match r {
                Reference::Work(id) => Some((Entity::Work { feature: id, part: WorkPart::Whole }, self.feature_name(id))),
                Reference::UcsOrigin(id) => {
                    Some((Entity::Work { feature: id, part: WorkPart::Whole }, format!("{}'s centre", self.feature_name(id))))
                }
                Reference::UcsPlane(id, p) => {
                    Some((Entity::Work { feature: id, part: WorkPart::Plane(p) }, format!("{} {p:?} Plane", self.feature_name(id))))
                }
                Reference::UcsAxis(id, a) => {
                    Some((Entity::Work { feature: id, part: WorkPart::Axis(a) }, format!("{} {a:?} Axis", self.feature_name(id))))
                }
                _ => None,
            };
            return match (work, &mut self.panel) {
                (Some(w), Some(Panel::Measure(m))) => {
                    m.take(None, Some(w));
                    self.request_measure();
                    true
                }
                _ => false,
            };
        }
        // What a UCS is measured from.
        if let Some(Panel::Work(w)) = &self.panel
            && w.slot == WorkSlot::Base
        {
            let base = match r {
                Reference::Point => Some(None),
                Reference::Work(id) | Reference::UcsOrigin(id) if is_ucs(self, id) => Some(Some(id)),
                _ => None,
            };
            return match (base, &mut self.panel) {
                (Some(b), Some(Panel::Work(w))) => {
                    w.base = b;
                    true
                }
                _ => false,
            };
        }
        let plane = match r {
            Reference::Plane(o) => Some(PlaneRef::Origin(o)),
            Reference::Work(id) if is_plane(self, id) => Some(PlaneRef::Work(id)),
            Reference::UcsPlane(feature, plane) => Some(PlaneRef::Ucs { feature, plane }),
            _ => None,
        };
        let axis = match r {
            Reference::Axis(o) => Some(AxisSel::Origin(o)),
            Reference::Work(id) if is_axis(self, id) => Some(AxisSel::Work(id)),
            Reference::UcsAxis(feature, axis) => Some(AxisSel::Ucs { feature, axis }),
            _ => None,
        };
        if self.picking_direction() {
            // (Read off the axis as it is now: a load keeps a direction, not what gave it.)
            let d = match axis {
                Some(AxisSel::Origin(tenon_model::OriginAxis::X)) => Some((tenon_geom::Vec3::X, tenon_geom::Vec3::ZERO)),
                Some(AxisSel::Origin(tenon_model::OriginAxis::Y)) => Some((tenon_geom::Vec3::Y, tenon_geom::Vec3::ZERO)),
                Some(AxisSel::Origin(tenon_model::OriginAxis::Z)) => Some((tenon_geom::Vec3::Z, tenon_geom::Vec3::ZERO)),
                Some(AxisSel::Work(id)) => self.work_axis_line(id),
                Some(AxisSel::Ucs { feature, axis }) => self.ucs_line(feature, axis).map(|(at, way)| (way, at)),
                _ => None,
            };
            return d.is_some_and(|(d, through)| self.set_load_direction(d, Some(through)));
        }
        // (Extrude To, or a Hole's To: the plane it goes as far as.)
        if self.picking_to()
            && let Some(p) = plane.clone()
        {
            return self.set_to_plane(p);
        }
        // A point of a three-point plane: the origin, or a work point (DEC-116).
        let is_point = |wb: &Workbench, id: FeatureId| wb.document().feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::WorkPoint(_)));
        if let Some(Panel::Work(w)) = &self.panel
            && w.slot.point().is_some()
        {
            let point = match r {
                Reference::Point => Some(PointSel::Origin),
                Reference::Work(id) if is_point(self, id) => Some(PointSel::Work(id)),
                Reference::UcsOrigin(id) => Some(PointSel::Ucs(id)),
                Reference::Work(id) if is_ucs(self, id) => Some(PointSel::Ucs(id)),
                _ => None,
            };
            return match (point, &mut self.panel) {
                (Some(p), Some(Panel::Work(w))) => w.put_point(p),
                _ => false,
            };
        }
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
            Some(Panel::Draft(_) | Panel::Split(_)) => plane.is_some_and(|p| self.bodies_plane(p)),
            Some(Panel::Primitive(_)) => plane.is_some_and(|p| self.primitive_on(p)),
            Some(Panel::Revolve(_) | Panel::Coil(_)) => match axis {
                Some(AxisSel::Origin(o)) => self.set_panel_axis(crate::panels::AxisChoice::Origin(o)),
                Some(AxisSel::Work(w)) => self.set_panel_axis(crate::panels::AxisChoice::Work(w)),
                Some(AxisSel::Ucs { feature, axis }) => self.set_panel_axis(crate::panels::AxisChoice::Ucs(feature, axis)),
                _ => false,
            },
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
            "UCS" => {
                "UCS: type how far it is moved and turned from the part's origin or another UCS, or choose Three Points and click them, then OK."
            }
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
        let circle = match p {
            Pick::Edge { body, edge } => {
                matches!(self.scene.bodies.get(body).and_then(|b| b.curves.get(edge as usize)), Some(tenon_kernel::CurveKind::Circle { .. }))
            }
            Pick::Face { .. } => false,
        };
        // The end of the edge nearer the pointer: where it was clicked.
        let near = match p {
            Pick::Edge { body, edge } => {
                let ends = self.scene.bodies.get(body).and_then(|b| b.ends.get(edge as usize)).copied();
                let pointer = self.view.pointer;
                ends.and_then(|[a, b]| {
                    let (sa, sb) = (self.to_screen(a, self.view.rect)?, self.to_screen(b, self.view.rect)?);
                    Some(if pointer.is_none_or(|q| q.distance(sa) <= q.distance(sb)) { a } else { b })
                })
            }
            Pick::Face { .. } => None,
        };
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
            // A point: a circular edge's centre, or the corner of a straight edge nearer where it
            // was clicked.
            (WorkSlot::P1 | WorkSlot::P2 | WorkSlot::P3, _, Some(e), _) if circle => return w.put_point(PointSel::Center(e)),
            (WorkSlot::P1 | WorkSlot::P2 | WorkSlot::P3, _, Some(edge), _) => {
                let Some(near) = near else { return false };
                return w.put_point(PointSel::Vertex { edge, near });
            }
            _ => return false,
        }
        w.advance();
        true
    }

    /// The work point under a screen point (drawn where it is).
    pub(crate) fn work_point_at(&self, p: Pos2, rect: Rect) -> Option<FeatureId> {
        self.scene
            .work
            .iter()
            .filter(|(id, _)| self.work_visible(*id))
            .filter_map(|(id, g)| match g {
                WorkGeom::Point(q) => Some((*id, self.to_screen(*q, rect)?.distance(p))),
                _ => None,
            })
            .filter(|(_, d)| *d <= 8.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// The point of a sketch shown in the view under a screen point.
    pub(crate) fn sketch_point_at(&self, p: Pos2, rect: Rect) -> Option<(FeatureId, tenon_sketch::EntityId)> {
        let mut best: Option<(f32, FeatureId, tenon_sketch::EntityId)> = None;
        for (id, _) in self.shown_sketches() {
            let (Some(sk), Some(frame)) = (self.document().sketch(id), self.sketch_frame(id)) else { continue };
            for (e, entity) in sk.entities() {
                let (true, Some(q)) = (entity.geometry.is_point(), sk.point(e)) else { continue };
                let Some(s) = self.to_screen(frame.plane_point(q), rect) else { continue };
                let d = s.distance(p);
                if d <= 8.0 && best.is_none_or(|b| d < b.0) {
                    best = Some((d, id, e));
                }
            }
        }
        best.map(|(_, id, e)| (id, e))
    }
}

/// Something picked from the origin folder or a work feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reference {
    Plane(OriginPlane),
    Axis(OriginAxis),
    Work(FeatureId),
    /// The origin's centre point.
    Point,
    /// A plane of a UCS.
    UcsPlane(FeatureId, OriginPlane),
    /// An axis of a UCS.
    UcsAxis(FeatureId, OriginAxis),
    /// The origin of a UCS.
    UcsOrigin(FeatureId),
}

fn set_pattern_ref(p: &mut PatternPanel, plane: Option<PlaneRef>, axis: Option<AxisSel>) -> bool {
    match (p.slot, plane, axis) {
        (Slot::Plane, Some(pl), _) => p.plane = pl,
        (Slot::Axis, _, Some(a)) => p.axis = a,
        (Slot::Dir1, _, Some(a)) => match a {
            AxisSel::Origin(o) => p.dir1 = tenon_model::DirectionRef::Origin(o),
            AxisSel::Work(id) => p.dir1 = tenon_model::DirectionRef::Work(id),
            AxisSel::Ucs { feature, axis } => p.dir1 = tenon_model::DirectionRef::Ucs { feature, axis },
            _ => return false,
        },
        (Slot::Dir2, _, Some(a)) => {
            p.dir2 = match a {
                AxisSel::Origin(o) => Some(tenon_model::DirectionRef::Origin(o)),
                AxisSel::Work(id) => Some(tenon_model::DirectionRef::Work(id)),
                AxisSel::Ucs { feature, axis } => Some(tenon_model::DirectionRef::Ucs { feature, axis }),
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
