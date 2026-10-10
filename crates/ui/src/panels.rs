//! Feature and value dialogs. Extrude and revolve preview live: while their panel is open the
//! workbench regenerates the document with the pending feature; OK commits it as a command.

use egui::{Align2, Ui};
use serde_json::{Value, json};
use tenon_model::{
    AxisRef, AxisSel, Chamfer, ChamferSize, CircPattern, DRILL_POINT, DirectionRef, Document, EdgeRef, Extrude, ExtrudeExtent, FaceRef, FeatureId,
    FeatureKind, Fillet, Fingerprint, Hole, HoleExtent, HoleType, Mirror, Operation, OriginAxis, OriginPlane, PlaneRef, RectPattern, RegionSel,
    Revolve, RevolveAngle, Rib, RibExtent, Shell, hole_centres, open_lines,
};
use tenon_sketch::{Constraint, ConstraintId, EntityId};

use crate::viewport::Pick;
use crate::workbench::{Mode, Workbench};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtentChoice {
    Distance,
    ThroughAll,
}

/// Which way a feature goes from its sketch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Default,
    Flipped,
    Symmetric,
    /// Two distances, one each way.
    Asymmetric,
}

#[derive(Clone, Debug)]
pub(crate) struct ExtrudePanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub extent: ExtentChoice,
    pub direction: Direction,
    pub distance: f64,
    /// The second distance (asymmetric).
    pub distance_b: f64,
    pub operation: Operation,
    pub regions: RegionSel,
    /// The extrusion being edited has a taper already (zero or not): its parameter stays.
    pub keep_taper: bool,
    /// Degrees the sides lean in as they leave the sketch (negative: out); zero is straight.
    pub taper: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AxisChoice {
    Origin(OriginAxis),
    Line(EntityId),
    Work(FeatureId),
}

#[derive(Clone, Debug)]
pub(crate) struct RevolvePanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub axis: AxisChoice,
    pub full: bool,
    pub degrees: f64,
    pub symmetric: bool,
    pub operation: Operation,
    pub regions: RegionSel,
}

#[derive(Clone, Debug)]
pub(crate) enum ValueFor {
    Fillet {
        sketch: FeatureId,
        point: EntityId,
    },
    Offset {
        sketch: FeatureId,
        curves: Vec<EntityId>,
    },
    /// A new dimension (its value is replaced by the entered one).
    Dimension {
        sketch: FeatureId,
        constraint: Constraint,
        /// Where its value was put down (sketch coordinates), when it was placed by a click.
        at: Option<tenon_geom::Vec2>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ValuePanel {
    pub title: &'static str,
    pub label: &'static str,
    pub value: f64,
    pub what: ValueFor,
    /// An equation typed in place of the value (dimensions).
    pub equation: Option<String>,
}

impl ValuePanel {
    pub(crate) fn new(title: &'static str, label: &'static str, value: f64, what: ValueFor) -> Self {
        ValuePanel { title, label, value, what, equation: None }
    }
}

/// Fillet: rounds the picked edges.
#[derive(Clone, Debug)]
pub(crate) struct FilletPanel {
    pub editing: Option<FeatureId>,
    pub edges: Vec<EdgeRef>,
    pub radius: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChamferMethod {
    Distance,
    TwoDistances,
    DistanceAngle,
}

/// Chamfer: bevels the picked edges; the two-sided methods measure the first value on a picked
/// face.
#[derive(Clone, Debug)]
pub(crate) struct ChamferPanel {
    pub editing: Option<FeatureId>,
    pub edges: Vec<EdgeRef>,
    pub method: ChamferMethod,
    pub d1: f64,
    pub d2: f64,
    pub degrees: f64,
    pub reference: Option<FaceRef>,
}

/// Shell: hollows the body, opening the picked faces.
#[derive(Clone, Debug)]
pub(crate) struct ShellPanel {
    pub editing: Option<FeatureId>,
    pub faces: Vec<FaceRef>,
    pub thickness: f64,
    pub outside: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Seat {
    None,
    Counterbore,
    Countersink,
}

/// Hole: drilled at points of a sketch.
#[derive(Clone, Debug)]
pub(crate) struct HolePanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub points: Vec<EntityId>,
    pub diameter: f64,
    pub seat: Seat,
    pub seat_diameter: f64,
    pub bore_depth: f64,
    pub sink_degrees: f64,
    pub through: bool,
    pub depth: f64,
    pub flat: bool,
    pub tip_degrees: f64,
    pub reverse: bool,
}

impl HolePanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        FeatureKind::Hole(Hole {
            sketch: self.sketch,
            points: self.points.clone(),
            diameter: self.diameter,
            kind: match self.seat {
                Seat::None => HoleType::Simple,
                Seat::Counterbore => HoleType::Counterbore { diameter: self.seat_diameter, depth: self.bore_depth },
                Seat::Countersink => HoleType::Countersink { diameter: self.seat_diameter, angle: self.sink_degrees.to_radians() },
            },
            extent: if self.through { HoleExtent::ThroughAll } else { HoleExtent::Distance(self.depth) },
            tip_angle: if self.flat { None } else { Some(self.tip_degrees.to_radians()) },
            reverse: self.reverse,
        })
    }

    fn from_hole(editing: Option<FeatureId>, h: &Hole) -> HolePanel {
        let (seat, seat_diameter, bore_depth, sink_degrees) = match h.kind {
            HoleType::Simple => (Seat::None, h.diameter * 1.8, h.diameter / 2.0, 90.0),
            HoleType::Counterbore { diameter, depth } => (Seat::Counterbore, diameter, depth, 90.0),
            HoleType::Countersink { diameter, angle } => (Seat::Countersink, diameter, h.diameter / 2.0, angle.to_degrees()),
        };
        let (through, depth) = match h.extent {
            HoleExtent::Distance(d) => (false, d),
            HoleExtent::ThroughAll => (true, h.diameter * 2.0),
        };
        HolePanel {
            editing,
            sketch: h.sketch,
            points: h.points.clone(),
            diameter: h.diameter,
            seat,
            seat_diameter,
            bore_depth,
            sink_degrees,
            through,
            depth,
            flat: h.tip_angle.is_none(),
            tip_degrees: h.tip_angle.unwrap_or(DRILL_POINT).to_degrees(),
            reverse: h.reverse,
        }
    }
}

/// Rib: a thin wall from open lines of a sketch.
#[derive(Clone, Debug)]
pub(crate) struct RibPanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub lines: Vec<EntityId>,
    pub thickness: f64,
    pub to_next: bool,
    pub distance: f64,
    pub flip: bool,
}

impl RibPanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        FeatureKind::Rib(Rib {
            sketch: self.sketch,
            lines: self.lines.clone(),
            thickness: self.thickness,
            extent: if self.to_next { RibExtent::ToNext } else { RibExtent::Distance(self.distance) },
            flip: self.flip,
        })
    }
}

/// Measure: one or two faces or edges picked in the part, and what they measure.
#[derive(Clone, Debug, Default)]
pub(crate) struct MeasurePanel {
    pub a: Option<Pick>,
    pub b: Option<Pick>,
    pub result: Option<Result<tenon_model::measure::Measurement, String>>,
    /// The measurement asked for and not answered yet.
    pub pending: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CopyKind {
    Rect,
    Circular,
    Mirror,
}

/// Which selector of a pattern panel takes clicks in the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Features,
    Dir1,
    Dir2,
    Axis,
    Plane,
}

/// Rectangular Pattern, Circular Pattern and Mirror.
#[derive(Clone, Debug)]
pub(crate) struct PatternPanel {
    pub editing: Option<FeatureId>,
    pub kind: CopyKind,
    pub features: Vec<FeatureId>,
    pub slot: Slot,
    pub dir1: DirectionRef,
    pub count1: f64,
    pub spacing1: f64,
    pub reverse1: bool,
    pub dir2: Option<DirectionRef>,
    pub count2: f64,
    pub spacing2: f64,
    pub reverse2: bool,
    pub axis: AxisSel,
    pub count: f64,
    pub degrees: f64,
    pub reverse: bool,
    pub plane: PlaneRef,
}

fn whole(v: f64) -> u32 {
    v.round().clamp(1.0, f64::from(tenon_model::MAX_COPIES)) as u32
}

impl PatternPanel {
    fn new(kind: CopyKind, features: Vec<FeatureId>) -> PatternPanel {
        PatternPanel {
            editing: None,
            kind,
            features,
            slot: Slot::Features,
            dir1: DirectionRef::Origin(OriginAxis::X),
            count1: 2.0,
            spacing1: 10.0,
            reverse1: false,
            dir2: None,
            count2: 1.0,
            spacing2: 10.0,
            reverse2: false,
            axis: AxisSel::Origin(OriginAxis::Z),
            count: 6.0,
            degrees: 360.0,
            reverse: false,
            plane: PlaneRef::Origin(OriginPlane::YZ),
        }
    }

    fn from_kind(editing: Option<FeatureId>, k: &FeatureKind) -> Option<PatternPanel> {
        let mut p = match k {
            FeatureKind::PatternRect(r) => {
                let mut p = PatternPanel::new(CopyKind::Rect, r.features.clone());
                p.dir1 = r.dir1.clone();
                p.count1 = f64::from(r.count1);
                p.spacing1 = r.spacing1;
                p.reverse1 = r.reverse1;
                p.dir2 = r.dir2.clone();
                p.count2 = f64::from(r.count2);
                p.spacing2 = if r.spacing2 > 0.0 { r.spacing2 } else { 10.0 };
                p.reverse2 = r.reverse2;
                p
            }
            FeatureKind::PatternCircular(c) => {
                let mut p = PatternPanel::new(CopyKind::Circular, c.features.clone());
                p.axis = c.axis.clone();
                p.count = f64::from(c.count);
                p.degrees = c.angle.to_degrees();
                p.reverse = c.reverse;
                p
            }
            FeatureKind::Mirror(m) => {
                let mut p = PatternPanel::new(CopyKind::Mirror, m.features.clone());
                p.plane = m.plane.clone();
                p
            }
            _ => return None,
        };
        p.editing = editing;
        Some(p)
    }

    /// The feature as set up, if it is complete.
    pub(crate) fn kind(&self) -> Option<FeatureKind> {
        if self.features.is_empty() {
            return None;
        }
        let kind = match self.kind {
            CopyKind::Rect => {
                let two = self.dir2.is_some() && whole(self.count2) > 1;
                let p = RectPattern {
                    features: self.features.clone(),
                    dir1: self.dir1.clone(),
                    count1: whole(self.count1),
                    spacing1: self.spacing1,
                    reverse1: self.reverse1,
                    dir2: if two { self.dir2.clone() } else { None },
                    count2: if two { whole(self.count2) } else { 1 },
                    spacing2: if two { self.spacing2 } else { 0.0 },
                    reverse2: self.reverse2,
                };
                p.check().ok()?;
                FeatureKind::PatternRect(p)
            }
            CopyKind::Circular => {
                let p = CircPattern {
                    features: self.features.clone(),
                    axis: self.axis.clone(),
                    count: whole(self.count),
                    angle: self.degrees.to_radians(),
                    reverse: self.reverse,
                };
                p.check().ok()?;
                FeatureKind::PatternCircular(p)
            }
            CopyKind::Mirror => FeatureKind::Mirror(Mirror { features: self.features.clone(), plane: self.plane.clone() }),
        };
        Some(kind)
    }

    pub(crate) fn command(&self) -> &'static str {
        match self.kind {
            CopyKind::Rect => "model.pattern.rect",
            CopyKind::Circular => "model.pattern.circular",
            CopyKind::Mirror => "model.mirror",
        }
    }
}

impl FilletPanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        FeatureKind::Fillet(Fillet { edges: self.edges.clone(), radius: self.radius })
    }
}

impl ChamferPanel {
    pub(crate) fn kind(&self) -> Option<FeatureKind> {
        let size = match (self.method, self.reference.clone()) {
            (ChamferMethod::Distance, _) => ChamferSize::Equal(self.d1),
            (ChamferMethod::TwoDistances, Some(reference)) => ChamferSize::TwoDistances { d1: self.d1, d2: self.d2, reference },
            (ChamferMethod::DistanceAngle, Some(reference)) => {
                ChamferSize::DistanceAngle { distance: self.d1, angle: self.degrees.to_radians(), reference }
            }
            _ => return None,
        };
        Some(FeatureKind::Chamfer(Chamfer { edges: self.edges.clone(), size }))
    }
}

impl ShellPanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        FeatureKind::Shell(Shell { remove: self.faces.clone(), thickness: self.thickness, outside: self.outside })
    }
}

/// A request to finish the open panel from outside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanelRequest {
    Ok,
    Cancel,
    /// OK, then start the same command again.
    Apply,
}

#[derive(Clone, Debug)]
pub(crate) enum Panel {
    Extrude(ExtrudePanel),
    Revolve(RevolvePanel),
    Fillet(FilletPanel),
    Chamfer(ChamferPanel),
    Shell(ShellPanel),
    Hole(HolePanel),
    Pattern(Box<PatternPanel>),
    Work(Box<crate::work::WorkPanel>),
    Measure(Box<MeasurePanel>),
    Rib(RibPanel),
    Sweep(crate::sweeps::SweepPanel),
    Coil(crate::sweeps::CoilPanel),
    Loft(crate::sweeps::LoftPanel),
    Draft(crate::bodies::DraftPanel),
    Split(crate::bodies::SplitPanel),
    Combine(crate::bodies::CombinePanel),
    Thread(crate::bodies::ThreadPanel),
    Value(ValuePanel),
    EditDimension {
        sketch: FeatureId,
        constraint: ConstraintId,
        value: f64,
        angular: bool,
        equation: Option<String>,
    },
    Rename {
        feature: FeatureId,
        name: String,
    },
    /// Constrain, Joint or Tweak in an assembly.
    Asm(Box<crate::asm_panel::AsmPanel>),
}

impl Panel {
    /// The feature the panel edits, if it edits one.
    pub(crate) fn editing(&self) -> Option<FeatureId> {
        match self {
            Panel::Extrude(p) => p.editing,
            Panel::Revolve(p) => p.editing,
            Panel::Fillet(p) => p.editing,
            Panel::Chamfer(p) => p.editing,
            Panel::Shell(p) => p.editing,
            Panel::Hole(p) => p.editing,
            Panel::Pattern(p) => p.editing,
            Panel::Work(w) => w.editing,
            Panel::Rib(p) => p.editing,
            Panel::Sweep(p) => p.editing,
            Panel::Coil(p) => p.editing,
            Panel::Loft(p) => p.editing,
            Panel::Draft(p) => p.editing,
            Panel::Split(p) => p.editing,
            Panel::Combine(p) => p.editing,
            Panel::Thread(p) => p.editing,
            _ => None,
        }
    }

    /// The value fields that take equations as the panel stands: field key and the feature value
    /// (JSON pointer) it sets.
    pub(crate) fn eq_pointers(&self) -> Vec<(&'static str, &'static str)> {
        match self {
            Panel::Extrude(p) => match (p.extent, p.direction) {
                (ExtentChoice::ThroughAll, _) => vec![("taper", "/taper")],
                (_, Direction::Symmetric) => vec![("distance", "/extent/symmetric")],
                (_, Direction::Asymmetric) => vec![("distance", "/extent/two_sided/forward"), ("distance_b", "/extent/two_sided/backward")],
                _ => vec![("distance", "/extent/distance"), ("taper", "/taper")],
            },
            Panel::Revolve(p) if p.full => vec![],
            Panel::Revolve(p) => vec![("degrees", if p.symmetric { "/angle/symmetric" } else { "/angle/angle" })],
            Panel::Fillet(_) => vec![("radius", "/radius")],
            Panel::Chamfer(p) => match p.method {
                ChamferMethod::Distance => vec![("d1", "/size/equal")],
                ChamferMethod::TwoDistances => vec![("d1", "/size/two_distances/d1"), ("d2", "/size/two_distances/d2")],
                ChamferMethod::DistanceAngle => vec![("d1", "/size/distance_angle/distance"), ("degrees", "/size/distance_angle/angle")],
            },
            Panel::Shell(_) => vec![("thickness", "/thickness")],
            Panel::Hole(p) => {
                let mut v = vec![("diameter", "/diameter")];
                if !p.through {
                    v.push(("depth", "/extent/distance"));
                    if !p.flat {
                        v.push(("tip_degrees", "/tip_angle"));
                    }
                }
                match p.seat {
                    Seat::None => {}
                    Seat::Counterbore => v.extend([("seat_diameter", "/kind/counterbore/diameter"), ("bore_depth", "/kind/counterbore/depth")]),
                    Seat::Countersink => v.extend([("seat_diameter", "/kind/countersink/diameter"), ("sink_degrees", "/kind/countersink/angle")]),
                }
                v
            }
            Panel::Pattern(p) => match p.kind {
                CopyKind::Rect => {
                    let mut v = vec![("count1", "/count1"), ("spacing1", "/spacing1")];
                    if p.dir2.is_some() && whole(p.count2) > 1 {
                        v.extend([("count2", "/count2"), ("spacing2", "/spacing2")]);
                    }
                    v
                }
                CopyKind::Circular => vec![("count", "/count"), ("degrees", "/angle")],
                CopyKind::Mirror => vec![],
            },
            Panel::Rib(p) if p.to_next => vec![("thickness", "/thickness")],
            Panel::Rib(_) => vec![("thickness", "/thickness"), ("distance", "/extent/distance")],
            Panel::Coil(_) => vec![("pitch", "/pitch"), ("turns", "/turns")],
            Panel::Draft(_) => vec![("degrees", "/angle")],
            Panel::Thread(p) => {
                let mut v = Vec::new();
                if p.custom_pitch {
                    v.push(("pitch", "/pitch"));
                }
                if !p.full {
                    v.push(("length", "/length/distance"));
                }
                v
            }
            Panel::Work(w) => match w.method {
                crate::work::WorkMethod::Offset => vec![("distance", "/distance")],
                crate::work::WorkMethod::Angle => vec![("degrees", "/angle")],
                _ => vec![],
            },
            _ => vec![],
        }
    }
}

impl Workbench {
    /// A feature panel just opened: its fields start with the equations the feature has.
    pub(crate) fn start_equations(&mut self) {
        self.panel_eqs.clear();
        let Some(panel) = &self.panel else { return };
        let Some(id) = panel.editing() else { return };
        for (key, ptr) in panel.eq_pointers() {
            let path = tenon_model::ValuePath::Feature { feature: id, field: ptr.to_owned() };
            if let Some(name) = self.document().name_of(&path)
                && let Some(m) = self.document().parameters().model.iter().find(|m| m.name == name)
                && let Some(e) = &m.equation
            {
                self.panel_eqs.insert(key, e.clone());
            }
        }
    }

    /// The equations to send with a feature: every value field of the panel, with its equation
    /// or (when editing, to drop an old equation) its plain value.
    fn panel_equations(&self, ptrs: &[(&'static str, &'static str)], editing: bool, kind: &FeatureKind) -> serde_json::Map<String, Value> {
        let json = serde_json::to_value(kind).unwrap_or(Value::Null);
        let mut out = serde_json::Map::new();
        for (key, ptr) in ptrs {
            match self.panel_eqs.get(key) {
                Some(e) => {
                    out.insert((*ptr).to_owned(), json!(e));
                }
                None if editing => {
                    if let Some(v) = json.pointer(ptr).and_then(Value::as_f64) {
                        // Angles are stored in radians; equations and plain values are in degrees.
                        let deg = tenon_model::params::field_unit(kind, ptr) == Some(tenon_model::ParamUnit::Deg);
                        out.insert((*ptr).to_owned(), json!(if deg { v.to_degrees() } else { v }.to_string()));
                    }
                }
                None => {}
            }
        }
        out
    }
}

impl ExtrudePanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        let extent = match (self.extent, self.direction) {
            (ExtentChoice::ThroughAll, _) => ExtrudeExtent::ThroughAll,
            (ExtentChoice::Distance, Direction::Symmetric) => ExtrudeExtent::Symmetric(self.distance),
            (ExtentChoice::Distance, Direction::Asymmetric) => ExtrudeExtent::TwoSided { forward: self.distance, backward: self.distance_b },
            (ExtentChoice::Distance, _) => ExtrudeExtent::Distance(self.distance),
        };
        let reverse = self.direction == Direction::Flipped;
        // A taper goes one way from the sketch: the two-way extents are straight.
        let one_way = matches!(extent, ExtrudeExtent::Distance(_) | ExtrudeExtent::ThroughAll);
        let taper = (one_way && (self.taper != 0.0 || self.keep_taper)).then(|| self.taper.to_radians());
        FeatureKind::Extrude(Extrude { sketch: self.sketch, regions: self.regions.clone(), extent, reverse, operation: self.operation, taper })
    }
}

impl RevolvePanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        let angle = if self.full {
            RevolveAngle::Full
        } else if self.symmetric {
            RevolveAngle::Symmetric(self.degrees.to_radians())
        } else {
            RevolveAngle::Angle(self.degrees.to_radians())
        };
        let axis = match self.axis {
            AxisChoice::Origin(a) => AxisRef::Origin(a),
            AxisChoice::Line(l) => AxisRef::SketchLine(l),
            AxisChoice::Work(w) => AxisRef::Work(w),
        };
        FeatureKind::Revolve(Revolve { sketch: self.sketch, regions: self.regions.clone(), axis, angle, operation: self.operation })
    }
}

/// The document with `kind` added (or replacing feature `editing`).
fn with_feature(doc: &Document, editing: Option<FeatureId>, kind: FeatureKind) -> Option<Document> {
    let mut d = doc.clone();
    match editing {
        Some(id) => d.feature_mut(id)?.kind = kind,
        None => {
            d.add(kind).ok()?;
        }
    }
    Some(d)
}

impl Workbench {
    /// The document to show while a feature panel is open. Extrude and Revolve preview their
    /// result; Fillet, Chamfer and Shell show the part they act on (rolled back to before the
    /// feature when editing it), so edges and faces can be picked on it.
    pub(crate) fn preview_document(&self) -> Option<Document> {
        let rolled = |editing: Option<FeatureId>| editing.map(|id| self.document().rolled_back_to(id));
        match &self.panel {
            Some(Panel::Extrude(p)) => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Revolve(p)) => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Hole(p)) if !p.points.is_empty() => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Hole(p)) => rolled(p.editing),
            Some(Panel::Rib(p)) if !p.lines.is_empty() => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Rib(p)) => rolled(p.editing),
            Some(Panel::Pattern(p)) => match p.kind() {
                Some(kind) => with_feature(self.document(), p.editing, kind),
                None => rolled(p.editing),
            },
            Some(Panel::Work(w)) => match w.kind() {
                Some(kind) => with_feature(self.document(), w.editing, kind),
                None => rolled(w.editing),
            },
            Some(Panel::Sweep(p)) => match p.kind() {
                Some(kind) => with_feature(self.document(), p.editing, kind),
                None => rolled(p.editing),
            },
            Some(Panel::Coil(p)) => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Loft(p)) => match p.kind() {
                Some(kind) => with_feature(self.document(), p.editing, kind),
                None => rolled(p.editing),
            },
            Some(Panel::Draft(p)) if !p.faces.is_empty() => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Draft(p)) => rolled(p.editing),
            Some(Panel::Split(p)) => match p.kind() {
                Some(kind) => with_feature(self.document(), p.editing, kind),
                None => rolled(p.editing),
            },
            // The bodies as they are before, so each can be clicked.
            Some(Panel::Combine(p)) => rolled(p.editing),
            Some(Panel::Thread(p)) => match p.kind() {
                Some(kind) => with_feature(self.document(), p.editing, kind),
                None => rolled(p.editing),
            },
            Some(Panel::Fillet(p)) => rolled(p.editing),
            Some(Panel::Chamfer(p)) => rolled(p.editing),
            Some(Panel::Shell(p)) => rolled(p.editing),
            _ => None,
        }
    }

    /// Edge references of the edges picked in the viewport.
    pub(crate) fn picked_edges(&self) -> Vec<EdgeRef> {
        self.view
            .selection
            .iter()
            .filter_map(|p| match p {
                Pick::Edge { body, edge } => self.scene.bodies.get(*body)?.edge_ref(*edge),
                Pick::Face { .. } => None,
            })
            .collect()
    }

    /// Face references of the faces picked in the viewport.
    pub(crate) fn picked_faces(&self) -> Vec<FaceRef> {
        self.view
            .selection
            .iter()
            .filter_map(|p| match p {
                Pick::Face { body, face } => {
                    let (name, info) = self.scene.bodies.get(*body)?.faces.get(*face as usize)?;
                    Some(FaceRef { origin: (*name)?, fingerprint: Fingerprint::of(info) })
                }
                Pick::Edge { .. } => None,
            })
            .collect()
    }

    /// Fillet, Chamfer or Shell: a new one starts from the edges or faces already selected.
    pub(crate) fn open_modify(&mut self, which: &str, editing: Option<FeatureId>) -> Result<(), String> {
        let existing = match editing {
            Some(id) => Some(self.document().feature(id).ok_or("no such feature")?.kind.clone()),
            None => None,
        };
        // The document, not the scene: the scene may still be regenerating.
        let solid = self.document().features().iter().any(|f| {
            matches!(f.kind, FeatureKind::Extrude(_) | FeatureKind::Revolve(_) | FeatureKind::Sweep(_) | FeatureKind::Coil(_) | FeatureKind::Loft(_))
        });
        if editing.is_none() && !solid {
            return Err("there is no solid yet: extrude or revolve a sketch first".into());
        }
        let panel = match (which, existing) {
            ("fillet", Some(FeatureKind::Fillet(f))) => Panel::Fillet(FilletPanel { editing, edges: f.edges, radius: f.radius }),
            ("fillet", None) => Panel::Fillet(FilletPanel { editing, edges: self.picked_edges(), radius: 2.0 }),
            ("chamfer", Some(FeatureKind::Chamfer(c))) => {
                let (method, d1, d2, degrees, reference) = match c.size {
                    ChamferSize::Equal(d) => (ChamferMethod::Distance, d, d, 45.0, None),
                    ChamferSize::TwoDistances { d1, d2, reference } => (ChamferMethod::TwoDistances, d1, d2, 45.0, Some(reference)),
                    ChamferSize::DistanceAngle { distance, angle, reference } => {
                        (ChamferMethod::DistanceAngle, distance, distance, angle.to_degrees(), Some(reference))
                    }
                };
                Panel::Chamfer(ChamferPanel { editing, edges: c.edges, method, d1, d2, degrees, reference })
            }
            ("chamfer", None) => Panel::Chamfer(ChamferPanel {
                editing,
                edges: self.picked_edges(),
                method: ChamferMethod::Distance,
                d1: 1.0,
                d2: 1.0,
                degrees: 45.0,
                reference: None,
            }),
            ("shell", Some(FeatureKind::Shell(s))) => {
                Panel::Shell(ShellPanel { editing, faces: s.remove, thickness: s.thickness, outside: s.outside })
            }
            ("shell", None) => Panel::Shell(ShellPanel { editing, faces: self.picked_faces(), thickness: 2.0, outside: false }),
            _ => return Err("not a feature of that type".into()),
        };
        self.panel = Some(panel);
        self.start_equations();
        self.view.selection.clear();
        self.set_status(match which {
            "fillet" => "Fillet: click edges to round (click again to remove), set the radius, then OK.",
            "chamfer" => "Chamfer: click edges to bevel, set the distance, then OK.",
            _ => "Shell: click the faces to open, set the thickness, then OK.",
        });
        Ok(())
    }

    /// The sketch a new feature should use: the one being edited, else the newest.
    fn default_sketch(&mut self) -> Result<FeatureId, String> {
        if let Mode::Sketch(s) = &self.mode {
            let id = s.feature;
            self.finish_sketch();
            return Ok(id);
        }
        self.sketches().last().map(|s| s.0).ok_or_else(|| "draw a sketch first".to_string())
    }

    pub(crate) fn open_extrude(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Extrude(e) => {
                    let way = if e.reverse { Direction::Flipped } else { Direction::Default };
                    let (extent, direction, distance, distance_b) = match e.extent {
                        ExtrudeExtent::Distance(d) => (ExtentChoice::Distance, way, d, d),
                        ExtrudeExtent::Symmetric(d) => (ExtentChoice::Distance, Direction::Symmetric, d, d),
                        ExtrudeExtent::TwoSided { forward, backward } => (ExtentChoice::Distance, Direction::Asymmetric, forward, backward),
                        ExtrudeExtent::ThroughAll => (ExtentChoice::ThroughAll, way, 10.0, 10.0),
                    };
                    ExtrudePanel {
                        editing,
                        sketch: e.sketch,
                        extent,
                        direction,
                        distance,
                        distance_b,
                        operation: e.operation,
                        regions: e.regions.clone(),
                        taper: e.taper.unwrap_or(0.0).to_degrees(),
                        keep_taper: e.taper.is_some(),
                    }
                }
                _ => return Err("not an extrusion".into()),
            },
            None => {
                // Join makes a new body when there is none yet.
                let sketch = self.default_sketch()?;
                ExtrudePanel {
                    editing: None,
                    sketch,
                    extent: ExtentChoice::Distance,
                    direction: Direction::Default,
                    distance: 10.0,
                    distance_b: 10.0,
                    operation: Operation::Join,
                    regions: RegionSel::Default,
                    taper: 0.0,
                    keep_taper: false,
                }
            }
        };
        self.panel = Some(Panel::Extrude(panel));
        self.start_equations();
        self.set_status("Extrude: set the distance and output, then OK.");
        Ok(())
    }

    pub(crate) fn open_revolve(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Revolve(r) => {
                    let (full, degrees, symmetric) = match r.angle {
                        RevolveAngle::Full => (true, 360.0, false),
                        RevolveAngle::Angle(a) => (false, a.to_degrees(), false),
                        RevolveAngle::Symmetric(a) => (false, a.to_degrees(), true),
                    };
                    let axis = match r.axis {
                        AxisRef::Origin(a) => AxisChoice::Origin(a),
                        AxisRef::SketchLine(l) => AxisChoice::Line(l),
                        AxisRef::Work(w) => AxisChoice::Work(w),
                    };
                    RevolvePanel { editing, sketch: r.sketch, axis, full, degrees, symmetric, operation: r.operation, regions: r.regions.clone() }
                }
                _ => return Err("not a revolution".into()),
            },
            None => {
                let sketch = self.default_sketch()?;
                let axis = self.default_revolve_axis(sketch);
                RevolvePanel {
                    editing: None,
                    sketch,
                    axis,
                    full: true,
                    degrees: 360.0,
                    symmetric: false,
                    operation: Operation::Join,
                    regions: RegionSel::Default,
                }
            }
        };
        self.panel = Some(Panel::Revolve(panel));
        self.start_equations();
        self.set_status("Revolve: choose the axis and angle, then OK.");
        Ok(())
    }

    /// Rectangular Pattern, Circular Pattern or Mirror. A new one starts with the features of the
    /// selected faces.
    pub(crate) fn open_pattern(&mut self, kind: CopyKind, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => PatternPanel::from_kind(editing, &self.document().feature(id).ok_or("no such feature")?.kind).ok_or("not a pattern")?,
            None => {
                if !self.document().features().iter().any(|f| f.kind.has_tool()) {
                    return Err("there is no feature to copy yet: extrude, revolve or drill one first".into());
                }
                let mut features = Vec::new();
                for p in &self.view.selection {
                    if let Pick::Face { body, face } = p
                        && let Some(id) =
                            self.scene.bodies.get(*body).and_then(|b| b.faces.get(*face as usize)).and_then(|f| f.0).map(|o| o.feature())
                        && self.document().feature(id).is_some_and(|f| f.kind.has_tool())
                        && !features.contains(&id)
                    {
                        features.push(id);
                    }
                }
                PatternPanel::new(kind, features)
            }
        };
        self.view.selection.clear();
        self.set_status(match kind {
            CopyKind::Rect => "Rectangular Pattern: click the features to copy, set the direction, count and spacing, then OK.",
            CopyKind::Circular => "Circular Pattern: click the features to copy, set the axis, count and angle, then OK.",
            CopyKind::Mirror => "Mirror: click the features to mirror, choose the plane, then OK.",
        });
        self.panel = Some(Panel::Pattern(Box::new(panel)));
        self.start_equations();
        Ok(())
    }

    pub(crate) fn open_rib(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Rib(r) => RibPanel {
                    editing,
                    sketch: r.sketch,
                    lines: r.lines.clone(),
                    thickness: r.thickness,
                    to_next: r.extent == RibExtent::ToNext,
                    distance: match r.extent {
                        RibExtent::Distance(d) => d,
                        RibExtent::ToNext => 10.0,
                    },
                    flip: r.flip,
                },
                _ => return Err("not a rib".into()),
            },
            None => {
                if !self.document().features().iter().any(|f| f.kind.has_tool()) {
                    return Err("there is no part for a rib yet: extrude or revolve a sketch first".into());
                }
                // The sketch being edited, else the last sketch with open lines.
                let sketch = match &self.mode {
                    Mode::Sketch(s) => {
                        let id = s.feature;
                        self.finish_sketch();
                        id
                    }
                    Mode::Model => self
                        .sketches()
                        .iter()
                        .rev()
                        .map(|s| s.0)
                        .find(|id| self.document().sketch(*id).is_some_and(|sk| !open_lines(sk).is_empty()))
                        .ok_or("draw an open line in a sketch for the rib first")?,
                };
                let lines = self.document().sketch(sketch).map(open_lines).unwrap_or_default();
                RibPanel { editing: None, sketch, lines, thickness: 2.0, to_next: true, distance: 10.0, flip: false }
            }
        };
        self.panel = Some(Panel::Rib(panel));
        self.start_equations();
        self.set_status("Rib: set the thickness; it grows from the sketch lines until it meets the part.");
        Ok(())
    }

    pub(crate) fn open_hole(&mut self, editing: Option<FeatureId>) -> Result<(), String> {
        let panel = match editing {
            Some(id) => match &self.document().feature(id).ok_or("no such feature")?.kind {
                FeatureKind::Hole(h) => HolePanel::from_hole(editing, h),
                _ => return Err("not a hole".into()),
            },
            None => {
                if !self.document().features().iter().any(|f| {
                    matches!(
                        f.kind,
                        FeatureKind::Extrude(_) | FeatureKind::Revolve(_) | FeatureKind::Sweep(_) | FeatureKind::Coil(_) | FeatureKind::Loft(_)
                    )
                }) {
                    return Err("there is no solid to drill yet: extrude or revolve a sketch first".into());
                }
                // The sketch being edited, else the last sketch with centre points.
                let sketch = match &self.mode {
                    Mode::Sketch(s) => {
                        let id = s.feature;
                        self.finish_sketch();
                        id
                    }
                    Mode::Model => self
                        .sketches()
                        .iter()
                        .rev()
                        .map(|s| s.0)
                        .find(|id| self.document().sketch(*id).is_some_and(|sk| !hole_centres(sk).is_empty()))
                        .ok_or("put points in a sketch for the hole centres first (Point tool in the Sketch tab)")?,
                };
                let points = self.document().sketch(sketch).map(hole_centres).unwrap_or_default();
                let mut p = HolePanel::from_hole(
                    None,
                    &Hole {
                        sketch,
                        points,
                        diameter: 6.0,
                        kind: HoleType::Simple,
                        extent: HoleExtent::Distance(12.0),
                        tip_angle: Some(DRILL_POINT),
                        reverse: false,
                    },
                );
                // Drill into the part: flip when the part lies on the sketch normal's side.
                if let (Some(frame), Some(b)) = (self.sketch_frame(sketch), self.scene.bbox()) {
                    p.reverse = (b.center() - frame.origin()).dot(frame.z()) > 1e-9;
                }
                p
            }
        };
        self.panel = Some(Panel::Hole(panel));
        self.start_equations();
        self.set_status("Hole: click sketch points to add or remove centres, set the sizes, then OK.");
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn commit_feature(&mut self, editing: Option<FeatureId>, kind: FeatureKind) -> bool {
        self.commit_feature_eq(editing, kind, serde_json::Map::new())
    }

    /// Commits a feature from a panel with the equations typed into its fields.
    fn commit_panel(&mut self, ptrs: &[(&'static str, &'static str)], editing: Option<FeatureId>, kind: FeatureKind) -> bool {
        let eqs = self.panel_equations(ptrs, editing.is_some(), &kind);
        self.commit_feature_eq(editing, kind, eqs)
    }

    pub(crate) fn commit_feature_eq(&mut self, editing: Option<FeatureId>, kind: FeatureKind, equations: serde_json::Map<String, Value>) -> bool {
        let kind_json = serde_json::to_value(&kind).unwrap_or(Value::Null);
        let r = match editing {
            Some(id) if equations.is_empty() => self.exec_status("feature.update", json!({ "feature": id.0, "kind": kind_json })),
            Some(id) => self.exec_status("feature.update", json!({ "feature": id.0, "kind": kind_json, "equations": equations })),
            // A new feature with equations: added with them in one step.
            None if !equations.is_empty() => self.exec_status("feature.add", json!({ "kind": kind_json, "equations": equations })),
            None => {
                // Through the registry, like every other edit.
                let params = match &kind {
                    FeatureKind::Extrude(e) => {
                        let mut p = json!({ "sketch": e.sketch.0, "reverse": e.reverse, "operation": op_name(e.operation) });
                        if let Some(regions) = regions_json(&e.regions) {
                            p["regions"] = regions;
                        }
                        match e.extent {
                            ExtrudeExtent::Distance(d) => p["distance"] = json!(d),
                            ExtrudeExtent::Symmetric(d) => p["symmetric"] = json!(d),
                            ExtrudeExtent::TwoSided { forward, backward } => {
                                p["distance"] = json!(forward);
                                p["backward"] = json!(backward);
                            }
                            ExtrudeExtent::ThroughAll => p["through_all"] = json!(true),
                        }
                        if let Some(taper) = e.taper {
                            p["taper"] = json!(taper);
                        }
                        ("model.extrude", p)
                    }
                    FeatureKind::Revolve(r) => {
                        let mut p = json!({ "sketch": r.sketch.0, "operation": op_name(r.operation) });
                        if let Some(regions) = regions_json(&r.regions) {
                            p["regions"] = regions;
                        }
                        p["axis"] = match r.axis {
                            AxisRef::Origin(a) => json!(format!("{a:?}").to_lowercase()),
                            AxisRef::SketchLine(l) => json!(l.0),
                            AxisRef::Work(w) => json!({ "work": w.0 }),
                        };
                        match r.angle {
                            RevolveAngle::Full => {}
                            RevolveAngle::Angle(a) => p["angle"] = json!(a),
                            RevolveAngle::Symmetric(a) => {
                                p["angle"] = json!(a);
                                p["symmetric"] = json!(true);
                            }
                        }
                        ("model.revolve", p)
                    }
                    FeatureKind::Fillet(f) => ("model.fillet", json!({ "edges": f.edges, "radius": f.radius })),
                    FeatureKind::Chamfer(c) => {
                        let p = match &c.size {
                            ChamferSize::Equal(d) => json!({ "edges": c.edges, "distance": d }),
                            ChamferSize::TwoDistances { d1, d2, reference } => {
                                json!({ "edges": c.edges, "distance": d1, "distance2": d2, "reference": reference })
                            }
                            ChamferSize::DistanceAngle { distance, angle, reference } => {
                                json!({ "edges": c.edges, "distance": distance, "angle": angle, "reference": reference })
                            }
                        };
                        ("model.chamfer", p)
                    }
                    FeatureKind::Shell(s) => ("model.shell", json!({ "remove": s.remove, "thickness": s.thickness, "outside": s.outside })),
                    FeatureKind::Hole(h) => {
                        let mut p = json!({
                            "sketch": h.sketch.0,
                            "points": h.points.iter().map(|e| e.0).collect::<Vec<_>>(),
                            "diameter": h.diameter,
                            "reverse": h.reverse,
                        });
                        match h.extent {
                            HoleExtent::Distance(d) => p["depth"] = json!(d),
                            HoleExtent::ThroughAll => p["through_all"] = json!(true),
                        }
                        match h.tip_angle {
                            Some(a) => p["tip_angle"] = json!(a),
                            None => p["flat_bottom"] = json!(true),
                        }
                        match h.kind {
                            HoleType::Simple => {}
                            HoleType::Counterbore { diameter, depth } => {
                                p["type"] = json!("counterbore");
                                p["counterbore_diameter"] = json!(diameter);
                                p["counterbore_depth"] = json!(depth);
                            }
                            HoleType::Countersink { diameter, angle } => {
                                p["type"] = json!("countersink");
                                p["countersink_diameter"] = json!(diameter);
                                p["countersink_angle"] = json!(angle);
                            }
                        }
                        ("model.hole", p)
                    }
                    FeatureKind::PatternRect(r) => {
                        let mut p = json!({
                            "features": r.features.iter().map(|f| f.0).collect::<Vec<_>>(),
                            "direction": direction_json(&r.dir1),
                            "count": r.count1,
                            "spacing": r.spacing1,
                            "reverse": r.reverse1,
                        });
                        if let Some(d2) = &r.dir2 {
                            p["direction2"] = direction_json(d2);
                            p["count2"] = json!(r.count2);
                            p["spacing2"] = json!(r.spacing2);
                            p["reverse2"] = json!(r.reverse2);
                        }
                        ("model.pattern.rect", p)
                    }
                    FeatureKind::PatternCircular(c) => {
                        let p = json!({
                            "features": c.features.iter().map(|f| f.0).collect::<Vec<_>>(),
                            "axis": crate::work::axis_json(&c.axis),
                            "count": c.count,
                            "angle": c.angle,
                            "reverse": c.reverse,
                        });
                        ("model.pattern.circular", p)
                    }
                    FeatureKind::Mirror(m) => {
                        let mut p = json!({ "features": m.features.iter().map(|f| f.0).collect::<Vec<_>>() });
                        match &m.plane {
                            PlaneRef::Origin(o) => p["plane"] = json!(format!("{o:?}").to_lowercase()),
                            PlaneRef::Face(f) => p["face"] = json!(f),
                            PlaneRef::Work(id) => p["work_plane"] = json!(id.0),
                        }
                        ("model.mirror", p)
                    }
                    k @ (FeatureKind::WorkPlane(_) | FeatureKind::WorkAxis(_) | FeatureKind::WorkPoint(_)) => {
                        let Some(params) = crate::work::work_params(k) else { return false };
                        params
                    }
                    FeatureKind::Rib(r) => {
                        let mut p = json!({
                            "sketch": r.sketch.0,
                            "lines": r.lines.iter().map(|e| e.0).collect::<Vec<_>>(),
                            "thickness": r.thickness,
                            "flip": r.flip,
                        });
                        if let RibExtent::Distance(d) = r.extent {
                            p["distance"] = json!(d);
                        }
                        ("model.rib", p)
                    }
                    // Added whole: their commands take the same fields as the feature.
                    FeatureKind::Sweep(_)
                    | FeatureKind::Coil(_)
                    | FeatureKind::Loft(_)
                    | FeatureKind::Draft(_)
                    | FeatureKind::Split(_)
                    | FeatureKind::Combine(_)
                    | FeatureKind::Thread(_) => ("feature.add", json!({ "kind": kind_json })),
                    FeatureKind::Sketch { .. } => return false,
                };
                self.exec_status(params.0, params.1)
            }
        };
        r.is_some()
    }

    pub(crate) fn panels(&mut self, ui: &mut Ui) {
        let Some(mut panel) = self.panel.take() else { return };
        // OK, Cancel or Apply from the properties panel, the mini-toolbar or the radial menu.
        let request = self.panel_request.take();
        let mut keep = request != Some(PanelRequest::Cancel);
        let commit = matches!(request, Some(PanelRequest::Ok | PanelRequest::Apply));
        let again = request == Some(PanelRequest::Apply);
        let mut reopen: Option<&'static str> = None;
        let ctx = ui.ctx().clone();
        let t = crate::theme::Tokens::of(self.chrome.theme);
        let ptrs = panel.eq_pointers();
        match &mut panel {
            Panel::Extrude(p) => {
                if commit {
                    let (editing, kind) = (p.editing, p.kind());
                    keep = !self.commit_panel(&ptrs, editing, kind);
                    if !keep && again {
                        reopen = Some("model.extrude");
                    }
                }
            }
            Panel::Revolve(p) => {
                if commit {
                    let (editing, kind) = (p.editing, p.kind());
                    keep = !self.commit_panel(&ptrs, editing, kind);
                    if !keep && again {
                        reopen = Some("model.revolve");
                    }
                }
            }
            Panel::Fillet(p) => {
                if commit {
                    if p.edges.is_empty() {
                        self.set_error("Fillet: select at least one edge".to_string());
                    } else {
                        keep = !self.commit_panel(&ptrs, p.editing, p.kind());
                        if !keep && again {
                            reopen = Some("model.fillet");
                        }
                    }
                }
            }
            Panel::Chamfer(p) => {
                if commit {
                    match p.kind() {
                        _ if p.edges.is_empty() => self.set_error("Chamfer: select at least one edge".to_string()),
                        None => self.set_error("Chamfer: pick the face the first distance is measured on".to_string()),
                        Some(kind) => {
                            keep = !self.commit_panel(&ptrs, p.editing, kind);
                            if !keep && again {
                                reopen = Some("model.chamfer");
                            }
                        }
                    }
                }
            }
            Panel::Shell(p) => {
                if commit {
                    keep = !self.commit_panel(&ptrs, p.editing, p.kind());
                    if !keep && again {
                        reopen = Some("model.shell");
                    }
                }
            }
            Panel::Pattern(p) => {
                if commit {
                    match p.kind() {
                        _ if p.features.is_empty() => self.set_error("Click at least one feature to copy (in the part or the browser)".to_string()),
                        None => self.set_error("Check the counts, spacing and angle".to_string()),
                        Some(kind) => {
                            keep = !self.commit_panel(&ptrs, p.editing, kind);
                            if !keep && again {
                                reopen = Some(p.command());
                            }
                        }
                    }
                }
            }
            Panel::Rib(p) => {
                if commit {
                    if p.lines.is_empty() {
                        self.set_error("Rib: the sketch has no open lines to make a rib from".to_string());
                    } else {
                        keep = !self.commit_panel(&ptrs, p.editing, p.kind());
                        if !keep && again {
                            reopen = Some("model.rib");
                        }
                    }
                }
            }
            Panel::Sweep(p) => {
                if commit {
                    match p.kind() {
                        None => self.set_error("Sweep: choose a sketch with lines and arcs for the path".to_string()),
                        Some(kind) => {
                            keep = !self.commit_panel(&ptrs, p.editing, kind);
                            if !keep && again {
                                reopen = Some("model.sweep");
                            }
                        }
                    }
                }
            }
            Panel::Coil(p) => {
                if commit {
                    keep = !self.commit_panel(&ptrs, p.editing, p.kind());
                    if !keep && again {
                        reopen = Some("model.coil");
                    }
                }
            }
            Panel::Loft(p) => {
                if commit {
                    match p.kind() {
                        None => self.set_error("Loft: tick two or more sections".to_string()),
                        Some(kind) => {
                            keep = !self.commit_panel(&ptrs, p.editing, kind);
                            if !keep && again {
                                reopen = Some("model.loft");
                            }
                        }
                    }
                }
            }
            Panel::Draft(p) => {
                if commit {
                    if p.faces.is_empty() {
                        self.set_error("Draft: click at least one face to tilt".to_string());
                    } else {
                        keep = !self.commit_panel(&ptrs, p.editing, p.kind());
                        if !keep && again {
                            reopen = Some("model.draft");
                        }
                    }
                }
            }
            Panel::Split(p) => {
                if commit {
                    match p.kind() {
                        None => self.set_error("Split: click the plane that cuts the part".to_string()),
                        Some(kind) => {
                            keep = !self.commit_panel(&ptrs, p.editing, kind);
                            if !keep && again {
                                reopen = Some("model.split");
                            }
                        }
                    }
                }
            }
            Panel::Combine(p) => {
                if commit {
                    match p.kind() {
                        None if p.base.is_none() => self.set_error("Combine: click the solid that stays".to_string()),
                        None => self.set_error("Combine: click at least one other solid".to_string()),
                        Some(kind) => {
                            keep = !self.commit_panel(&ptrs, p.editing, kind);
                            if !keep && again {
                                reopen = Some("model.combine");
                            }
                        }
                    }
                }
            }
            Panel::Thread(p) => {
                if commit {
                    match p.kind() {
                        None => self.set_error("Thread: click a round shaft or hole".to_string()),
                        Some(kind) => {
                            keep = !self.commit_panel(&ptrs, p.editing, kind);
                            if !keep && again {
                                reopen = Some("model.thread");
                            }
                        }
                    }
                }
            }
            Panel::Asm(p) => {
                if commit {
                    let done = self.commit_asm_panel(p);
                    keep = !done;
                    if done && again {
                        reopen = Some(match p.tool {
                            crate::asm_panel::AsmTool::Constrain => "asm.constrain",
                            crate::asm_panel::AsmTool::Joint => "asm.joint",
                            crate::asm_panel::AsmTool::Tweak => "asm.explode.tweak",
                        });
                    }
                }
            }
            // Measure has nothing to commit: OK and Cancel both close it.
            Panel::Measure(_) => {
                if commit {
                    keep = false;
                }
            }
            Panel::Work(w) => {
                if commit {
                    match w.kind() {
                        None => self.set_error(format!("{}: fill in every selection first", w.title())),
                        Some(kind) => {
                            keep = !self.commit_panel(&ptrs, w.editing, kind);
                            if !keep && again {
                                reopen = Some(w.command());
                            }
                        }
                    }
                }
            }
            Panel::Hole(p) => {
                if commit {
                    if p.points.is_empty() {
                        self.set_error("Hole: click at least one sketch point for a centre".to_string());
                    } else {
                        keep = !self.commit_panel(&ptrs, p.editing, p.kind());
                        if !keep && again {
                            reopen = Some("model.hole");
                        }
                    }
                }
            }
            Panel::Value(v) if matches!(v.what, ValueFor::Dimension { .. }) => {
                // A new dimension: the inline box sits on the dimension itself.
                let ValueFor::Dimension { sketch, constraint, at: place } = &v.what else { return };
                let (sketch, constraint, place) = (*sketch, constraint.clone(), *place);
                let unit = if constraint.is_angular() { "deg" } else { "mm" };
                let at = self.dimension_anchor(sketch, &constraint, place).unwrap_or(self.view.rect.center());
                let (ok, cancel) = inline_value(&ctx, at, &mut v.value, &mut v.equation, unit, &t);
                if cancel {
                    keep = false;
                } else if ok || commit {
                    let mut c = constraint;
                    let value = if c.is_angular() { v.value.to_radians() } else { v.value };
                    let signed = matches!(c, Constraint::HorizontalDistance { .. } | Constraint::VerticalDistance { .. });
                    c.set_value(if signed { value } else { value.abs() });
                    let mut p = json!({ "sketch": sketch.0, "constraint": serde_json::to_value(&c).unwrap_or(Value::Null) });
                    if let Some(e) = &v.equation {
                        p["equation"] = json!(e);
                    }
                    // The dimension stays where it was put down.
                    if let Some(place) = place {
                        p["at_x"] = json!(place.x);
                        p["at_y"] = json!(place.y);
                    }
                    keep = self.exec_status("sketch.constrain", p).is_none();
                }
            }
            Panel::EditDimension { sketch, constraint, value, angular, equation } => {
                // Double-clicked dimension: the same inline box.
                let (c, place) = match self.document().sketch(*sketch) {
                    Some(s) => (s.constraint(*constraint).cloned(), s.place(*constraint)),
                    None => (None, None),
                };
                let at = c.as_ref().and_then(|c| self.dimension_anchor(*sketch, c, place)).unwrap_or(self.view.rect.center());
                let mut shown = if *angular { value.to_degrees() } else { *value };
                let (ok, cancel) = inline_value(&ctx, at, &mut shown, equation, if *angular { "deg" } else { "mm" }, &t);
                *value = if *angular { shown.to_radians() } else { shown };
                if cancel {
                    keep = false;
                } else if ok || commit {
                    let p = match equation {
                        Some(e) => json!({ "sketch": sketch.0, "constraint": constraint.0, "equation": e }),
                        None => json!({ "sketch": sketch.0, "constraint": constraint.0, "value": *value }),
                    };
                    keep = self.exec_status("sketch.set_dimension", p).is_none();
                }
            }
            Panel::Value(v) => {
                let mut ok = commit;
                egui::Window::new(v.title).collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, [0.0, 160.0]).show(&ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(v.label);
                        let r = ui.add(egui::DragValue::new(&mut v.value).speed(0.1));
                        r.request_focus();
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            ok = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            ok = true;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if ok {
                    let done = match &v.what {
                        ValueFor::Fillet { sketch, point } => {
                            self.exec_status("sketch.fillet", json!({ "sketch": sketch.0, "point": point.0, "radius": v.value }))
                        }
                        ValueFor::Offset { sketch, curves } => {
                            let ids: Vec<u32> = curves.iter().map(|c| c.0).collect();
                            self.exec_status("sketch.offset", json!({ "sketch": sketch.0, "curves": ids, "distance": v.value }))
                        }
                        ValueFor::Dimension { sketch, constraint, .. } => {
                            let mut c = constraint.clone();
                            let value = if c.is_angular() { v.value.to_radians() } else { v.value };
                            let signed = matches!(c, Constraint::HorizontalDistance { .. } | Constraint::VerticalDistance { .. });
                            c.set_value(if signed { value } else { value.abs() });
                            self.exec_status(
                                "sketch.constrain",
                                json!({ "sketch": sketch.0, "constraint": serde_json::to_value(&c).unwrap_or(Value::Null) }),
                            )
                        }
                    };
                    keep = done.is_none();
                }
            }
            Panel::Rename { feature, name } => {
                let mut ok = commit;
                egui::Window::new("Rename").collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, [0.0, 160.0]).show(&ctx, |ui| {
                    let r = ui.text_edit_singleline(name);
                    r.request_focus();
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            ok = true;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if ok {
                    keep = self.exec_status("feature.rename", json!({ "feature": feature.0, "name": name.trim() })).is_none();
                }
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = false;
        }
        if keep && self.panel.is_none() {
            self.panel = Some(panel);
        } else if !keep {
            self.panel_eqs.clear();
        }
        if let Some(id) = reopen {
            self.command(id);
        }
    }
}

/// The edit box placed on a dimension: the value (selected, so typing replaces it) and a check
/// mark. Enter or the check is OK, Esc cancels. Returns `(ok, cancel)`.
fn inline_value(ctx: &egui::Context, at: egui::Pos2, value: &mut f64, eq: &mut Option<String>, unit: &str, t: &crate::theme::Tokens) -> (bool, bool) {
    let (mut ok, mut cancel) = (false, false);
    let field = egui::Id::new("tn_inline_dimension");
    egui::Area::new(egui::Id::new("tn_inline_box")).order(egui::Order::Foreground).fixed_pos(at - egui::vec2(48.0, 13.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(t.panel).inner_margin(3.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                if ui.memory(|m| m.focused().is_none()) {
                    ui.memory_mut(|m| m.request_focus(field));
                }
                ok |= crate::properties::eq_field(ui, field, value, Some(eq), unit, -1.0e6..=1.0e6, 96.0, t).entered;
                if ui.add(egui::Button::new(egui::RichText::new("✔").color(t.ok)).small()).on_hover_text("OK (Enter)").clicked() {
                    ok = true;
                }
            });
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cancel = true;
    }
    (ok, cancel)
}

fn direction_json(d: &DirectionRef) -> Value {
    match d {
        DirectionRef::Origin(a) => json!(format!("{a:?}").to_lowercase()),
        DirectionRef::Edge(e) => json!(e),
        DirectionRef::Work(id) => json!({ "work": id.0 }),
    }
}

/// The regions a feature was told to use, as its command takes them; None for the default
/// choice.
fn regions_json(sel: &RegionSel) -> Option<Value> {
    match sel {
        RegionSel::Default => None,
        RegionSel::Keys(keys) => Some(json!(keys.iter().map(|k| k.iter().map(|e| e.0).collect::<Vec<_>>()).collect::<Vec<_>>())),
    }
}

fn op_name(o: Operation) -> &'static str {
    match o {
        Operation::Join => "join",
        Operation::Cut => "cut",
        Operation::NewBody => "new_body",
        Operation::Intersect => "intersect",
    }
}
