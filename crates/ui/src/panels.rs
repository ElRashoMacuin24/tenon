//! Feature and value dialogs. Extrude and revolve preview live: while their panel is open the
//! workbench regenerates the document with the pending feature; OK commits it as a command.

use egui::{Align2, Ui};
use serde_json::{Value, json};
use tenon_geom::{Frame, Vec2};
use tenon_model::{
    AxisRef, AxisSel, Chamfer, ChamferSize, CircPattern, DRILL_POINT, DirectionRef, Document, EdgeRef, Extrude, ExtrudeExtent, FaceRef, FeatureId,
    FeatureKind, Fillet, Fingerprint, Hole, HoleExtent, HoleType, Mirror, Operation, OriginAxis, OriginPlane, PlaneRef, RadiusAt, RectPattern,
    RegionSel, Revolve, RevolveAngle, Rib, RibExtent, Shell, hole_centres, open_lines,
};
use tenon_sketch::{Constraint, ConstraintId, EntityId};

use crate::viewport::Pick;
use crate::workbench::{Mode, Workbench};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtentChoice {
    Distance,
    ThroughAll,
    /// As far as a face or plane (DEC-104).
    To,
    /// As far as the part's next faces (DEC-109).
    ToNext,
    /// From one face or plane to another (DEC-111).
    Between,
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
    /// With To: the face or plane it goes as far as; with Between, the one it ends on.
    pub to: Option<PlaneRef>,
    /// With Between: the face or plane it starts from, and whether a pick goes there next.
    pub from: Option<PlaneRef>,
    pub pick_from: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AxisChoice {
    Origin(OriginAxis),
    Line(EntityId),
    Work(FeatureId),
}

/// Where a revolution ends besides an angle or a full turn (DEC-112).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RevTerm {
    Angle,
    To,
    ToNext,
}

#[derive(Clone, Debug)]
pub(crate) struct RevolvePanel {
    pub editing: Option<FeatureId>,
    pub sketch: FeatureId,
    pub axis: AxisChoice,
    pub full: bool,
    pub degrees: f64,
    pub symmetric: bool,
    /// Flipped, or (with `degrees_b`) Asymmetric, as Inventor's directions (DEC-108).
    pub direction: Direction,
    pub degrees_b: f64,
    /// To a plane or the next faces instead of an angle, and the plane (DEC-112).
    pub term: RevTerm,
    pub to: Option<PlaneRef>,
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
    /// How large the elements of a stress study are.
    MeshSize {
        study: tenon_model::study::StudyId,
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
    /// The radius; varying, the radius at the start of each edge.
    pub radius: f64,
    /// Variable radius (DEC-122): the radius at the end of each edge, and any places between
    /// that the feature has (kept as they are).
    pub variable: bool,
    pub end_radius: f64,
    pub between: Vec<RadiusAt>,
    /// Face fillet (DEC-122): its two sets of faces, and which of them a click adds to.
    pub faces: Option<[Vec<FaceRef>; 2]>,
    pub set: usize,
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

/// A hole sized for a screw: a clearance hole in one of two fits, or the drill to tap it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fit {
    Close,
    Normal,
    TapDrill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Seat {
    None,
    Counterbore,
    /// A shallow seat; the hole is measured from its floor.
    Spotface,
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
    /// Termination To: as far as `to` (DEC-105).
    pub to_plane: bool,
    pub to: Option<PlaneRef>,
    pub depth: f64,
    pub flat: bool,
    pub tip_degrees: f64,
    pub reverse: bool,
    /// Sized for a screw (an index into the library's screw holes): None for a hole of one's
    /// own size.
    pub screw: Option<(usize, Fit)>,
    /// Holes placed by clicking a flat face, with no sketch drawn first (DEC-113): the face, its
    /// sketch frame, and the centres clicked on it (in that frame). On OK a sketch on the face
    /// with these points is made with the hole, in one step.
    pub face: Option<(FaceRef, Frame)>,
    pub face_points: Vec<Vec2>,
    /// Tapped (a screw's tap drill, DEC-114): the thread the whole wall, or this deep.
    pub thread_full: bool,
    pub thread_depth: f64,
}

impl HolePanel {
    /// Sizes the hole, and its seat, for its screw.
    pub(crate) fn size_for_screw(&mut self) {
        let Some((i, fit)) = self.screw else { return };
        let Some(h) = tenon_model::library::SCREW_HOLES.get(i) else { return };
        self.diameter = match fit {
            Fit::Close => h.close,
            Fit::Normal => h.normal,
            Fit::TapDrill => h.tap_drill,
        };
        match self.seat {
            Seat::None => {}
            Seat::Counterbore => (self.seat_diameter, self.bore_depth) = h.counterbore,
            // (A spotface as wide as the head's counterbore: its depth is the user's.)
            Seat::Spotface => self.seat_diameter = h.counterbore.0,
            Seat::Countersink => (self.seat_diameter, self.sink_degrees) = (h.countersink, 90.0),
        }
    }

    /// The screw a hole's size is for, if it is one of the library's.
    fn screw_of(diameter: f64) -> Option<(usize, Fit)> {
        tenon_model::library::SCREW_HOLES.iter().enumerate().find_map(|(i, h)| {
            [(h.close, Fit::Close), (h.normal, Fit::Normal), (h.tap_drill, Fit::TapDrill)]
                .into_iter()
                .find(|(d, _)| (d - diameter).abs() < 1e-9)
                .map(|(_, fit)| (i, fit))
        })
    }
    pub(crate) fn kind(&self) -> FeatureKind {
        FeatureKind::Hole(Hole {
            sketch: self.sketch,
            points: self.points.clone(),
            diameter: self.diameter,
            kind: match self.seat {
                Seat::None => HoleType::Simple,
                Seat::Counterbore => HoleType::Counterbore { diameter: self.seat_diameter, depth: self.bore_depth },
                Seat::Spotface => HoleType::Spotface { diameter: self.seat_diameter, depth: self.bore_depth },
                Seat::Countersink => HoleType::Countersink { diameter: self.seat_diameter, angle: self.sink_degrees.to_radians() },
            },
            // (To with nothing picked yet: the depth stands in, and the preview shows that.)
            extent: match (&self.to, self.to_plane, self.through) {
                (Some(plane), true, _) => HoleExtent::To(plane.clone()),
                (_, false, true) => HoleExtent::ThroughAll,
                _ => HoleExtent::Distance(self.depth),
            },
            tip_angle: if self.flat { None } else { Some(self.tip_degrees.to_radians()) },
            reverse: self.reverse,
            tapped: self.tapped().then(|| tenon_model::Tapping {
                length: if self.thread_full { tenon_model::ThreadLength::Full } else { tenon_model::ThreadLength::Distance(self.thread_depth) },
                ..Default::default()
            }),
        })
    }

    /// Tapped: sized for a screw's tap drill.
    pub(crate) fn tapped(&self) -> bool {
        matches!(self.screw, Some((_, Fit::TapDrill)))
    }

    fn from_hole(editing: Option<FeatureId>, h: &Hole) -> HolePanel {
        let (seat, seat_diameter, bore_depth, sink_degrees) = match h.kind {
            HoleType::Simple => (Seat::None, h.diameter * 1.8, h.diameter / 2.0, 90.0),
            HoleType::Counterbore { diameter, depth } => (Seat::Counterbore, diameter, depth, 90.0),
            HoleType::Countersink { diameter, angle } => (Seat::Countersink, diameter, h.diameter / 2.0, angle.to_degrees()),
            HoleType::Spotface { diameter, depth } => (Seat::Spotface, diameter, depth, 90.0),
        };
        let (through, depth) = match h.extent {
            HoleExtent::Distance(d) => (false, d),
            HoleExtent::ThroughAll | HoleExtent::To(_) => (matches!(h.extent, HoleExtent::ThroughAll), h.diameter * 2.0),
        };
        let to = match &h.extent {
            HoleExtent::To(plane) => Some(plane.clone()),
            _ => None,
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
            to_plane: to.is_some(),
            to,
            depth,
            flat: h.tip_angle.is_none(),
            tip_degrees: h.tip_angle.unwrap_or(DRILL_POINT).to_degrees(),
            reverse: h.reverse,
            screw: HolePanel::screw_of(h.diameter),
            face: None,
            face_points: Vec::new(),
            thread_full: !matches!(h.tapped.as_ref().map(|t| t.length), Some(tenon_model::ThreadLength::Distance(_))),
            thread_depth: match h.tapped.as_ref().map(|t| t.length) {
                Some(tenon_model::ThreadLength::Distance(d)) => d,
                _ => h.diameter * 1.5,
            },
        }
    }

    /// Whether its holes are placed by clicking a face.
    pub(crate) fn on_face(&self) -> bool {
        self.editing.is_none() && self.points.is_empty()
    }

    /// The document with the sketch its face clicks make, and the hole on it: the sketch's id
    /// and the hole as it would be made.
    pub(crate) fn on_face_document(&self, doc: &Document) -> Option<(Document, FeatureId, FeatureKind)> {
        let ((face, _), points) = (self.face.as_ref()?, &self.face_points);
        if points.is_empty() {
            return None;
        }
        let mut d = doc.clone();
        let mut sk = tenon_sketch::Sketch::new();
        let ids: Vec<EntityId> = points.iter().filter_map(|p| sk.add_point(*p).ok()).collect();
        let sketch = d.add(FeatureKind::Sketch { plane: PlaneRef::Face(face.clone()), sketch: sk, projections: Vec::new() }).ok()?;
        let FeatureKind::Hole(mut h) = self.kind() else { return None };
        (h.sketch, h.points) = (sketch, ids);
        let kind = FeatureKind::Hole(h);
        d.add(kind.clone()).ok()?;
        Some((d, sketch, kind))
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
    pub(crate) fn new(editing: Option<FeatureId>, edges: Vec<EdgeRef>, radius: f64) -> FilletPanel {
        FilletPanel { editing, edges, radius, variable: false, end_radius: radius * 2.0, between: Vec::new(), faces: None, set: 0 }
    }

    /// The panel for a fillet as it is.
    pub(crate) fn of(editing: Option<FeatureId>, f: Fillet) -> FilletPanel {
        let mut between = f.radii.clone();
        let end = between.pop_if(|r| r.at >= 1.0).map(|r| r.radius).or(f.radii.last().map(|r| r.radius));
        FilletPanel {
            variable: end.is_some(),
            end_radius: end.unwrap_or(f.radius * 2.0),
            between,
            faces: f.faces.clone(),
            ..FilletPanel::new(editing, f.edges, f.radius)
        }
    }

    pub(crate) fn kind(&self) -> FeatureKind {
        let radii = if self.variable {
            self.between.iter().copied().chain(std::iter::once(RadiusAt { at: 1.0, radius: self.end_radius })).collect()
        } else {
            Vec::new()
        };
        let edges = if self.faces.is_some() { Vec::new() } else { self.edges.clone() };
        FeatureKind::Fillet(Fillet { edges, radius: self.radius, radii, faces: self.faces.clone() })
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
    /// A sheet metal face.
    SheetFace(crate::sheet::FacePanel),
    /// Cut: a profile through the sheet, round its bends when asked.
    SheetCut(crate::sheet::CutPanel),
    /// A sheet metal flange.
    Flange(crate::sheet::FlangePanel),
    /// A hem along edges of the sheet.
    Hem(crate::sheet::HemPanel),
    /// A sheet along an open profile.
    ContourFlange(crate::sheet::ContourPanel),
    /// A corner seam between two faces of the sheet.
    CornerSeam(crate::sheet::SeamPanel),
    /// A bend between two faces of the sheet that do not touch.
    Bend(crate::sheet::BendPanel),
    /// A face of the sheet folded along a line drawn across it.
    Fold(crate::sheet::FoldPanel),
    /// A box, a cylinder, a sphere or a torus, set up by clicks on a plane.
    Primitive(crate::primitive::PrimitivePanel),
    /// Fixed, in the Stress Analysis environment: the faces that are held.
    Support(crate::analysis::SupportPanel),
    /// Force, Pressure or Gravity, in the Stress Analysis environment.
    Load(crate::analysis::LoadPanel),
    /// A study's name, or (with `item`) that of a support or a load of it.
    RenameStudy {
        study: tenon_model::study::StudyId,
        item: Option<u32>,
        name: String,
    },
    Value(ValuePanel),
    /// A rectangular or circular pattern of sketch geometry.
    SketchPattern(crate::sketch_tools::SketchPatternPanel),
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
    /// Delete Features: what goes with a feature, ticked as Inventor ticks it (DEC-103).
    DeleteFeatures {
        feature: FeatureId,
        /// The sketches it is made from that nothing else uses go too.
        consumed: bool,
        /// The features that use it go too (without them it cannot be deleted).
        dependents: bool,
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
            Panel::SheetFace(p) => p.editing,
            Panel::SheetCut(p) => p.editing,
            Panel::Flange(p) => p.editing,
            Panel::Hem(p) => p.editing,
            Panel::ContourFlange(p) => p.editing,
            Panel::Fold(p) => p.editing,
            Panel::CornerSeam(p) => p.editing,
            Panel::Bend(p) => p.editing,
            _ => None,
        }
    }

    /// The value fields that take equations as the panel stands: field key and the feature value
    /// (JSON pointer) it sets.
    pub(crate) fn eq_pointers(&self) -> Vec<(&'static str, &'static str)> {
        match self {
            Panel::Extrude(p) => match (p.extent, p.direction) {
                (ExtentChoice::ThroughAll | ExtentChoice::To, _) => vec![("taper", "/taper")],
                (ExtentChoice::ToNext | ExtentChoice::Between, _) => vec![],
                (_, Direction::Symmetric) => vec![("distance", "/extent/symmetric")],
                (_, Direction::Asymmetric) => vec![("distance", "/extent/two_sided/forward"), ("distance_b", "/extent/two_sided/backward")],
                _ => vec![("distance", "/extent/distance"), ("taper", "/taper")],
            },
            Panel::Revolve(p) if p.full => vec![],
            Panel::Revolve(p) => match p.direction {
                _ if p.term != RevTerm::Angle => vec![],
                _ if p.symmetric || p.direction == Direction::Symmetric => vec![("degrees", "/angle/symmetric")],
                Direction::Asymmetric => vec![("degrees", "/angle/two_sided/forward"), ("degrees_b", "/angle/two_sided/backward")],
                _ => vec![("degrees", "/angle/angle")],
            },
            Panel::Fillet(p) if p.variable && p.between.is_empty() => vec![("radius", "/radius"), ("end_radius", "/radii/0/radius")],
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
                    if !p.to_plane {
                        v.push(("depth", "/extent/distance"));
                    }
                    if !p.flat {
                        v.push(("tip_degrees", "/tip_angle"));
                    }
                }
                match p.seat {
                    Seat::None => {}
                    Seat::Counterbore => v.extend([("seat_diameter", "/kind/counterbore/diameter"), ("bore_depth", "/kind/counterbore/depth")]),
                    Seat::Spotface => v.extend([("seat_diameter", "/kind/spotface/diameter"), ("bore_depth", "/kind/spotface/depth")]),
                    Seat::Countersink => v.extend([("seat_diameter", "/kind/countersink/diameter"), ("sink_degrees", "/kind/countersink/angle")]),
                }
                if p.tapped() && !p.thread_full {
                    v.push(("thread_depth", "/tapped/length/distance"));
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
            Panel::SheetCut(p) => p.eq_pointers(),
            Panel::Flange(p) => p.eq_pointers(),
            Panel::Hem(p) => p.eq_pointers(),
            Panel::ContourFlange(p) => p.eq_pointers(),
            Panel::Fold(p) => p.eq_pointers(),
            Panel::CornerSeam(p) => p.eq_pointers(),
            Panel::Bend(p) => p.eq_pointers(),
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

/// A plane as commands take it: an origin plane's name, a face reference, or {"work": id}.
pub(crate) fn plane_json(plane: &PlaneRef) -> Value {
    match plane {
        PlaneRef::Origin(o) => json!(format!("{o:?}").to_lowercase()),
        PlaneRef::Face(f) => serde_json::to_value(f).unwrap_or(Value::Null),
        PlaneRef::Work(id) => json!({ "work": id.0 }),
    }
}

impl ExtrudePanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        let extent = match (self.extent, self.direction) {
            // (Until a plane is picked, the distance stands in: the preview shows that.)
            (ExtentChoice::To, _) => match &self.to {
                Some(plane) => ExtrudeExtent::To(plane.clone()),
                None => ExtrudeExtent::Distance(self.distance),
            },
            (ExtentChoice::ThroughAll, _) => ExtrudeExtent::ThroughAll,
            (ExtentChoice::ToNext, _) => ExtrudeExtent::ToNext,
            (ExtentChoice::Between, _) => match (&self.from, &self.to) {
                (Some(from), Some(to)) => ExtrudeExtent::Between { from: from.clone(), to: to.clone() },
                _ => ExtrudeExtent::Distance(self.distance),
            },
            (ExtentChoice::Distance, Direction::Symmetric) => ExtrudeExtent::Symmetric(self.distance),
            (ExtentChoice::Distance, Direction::Asymmetric) => ExtrudeExtent::TwoSided { forward: self.distance, backward: self.distance_b },
            (ExtentChoice::Distance, _) => ExtrudeExtent::Distance(self.distance),
        };
        let reverse = self.direction == Direction::Flipped;
        // A taper goes one way from the sketch: the two-way extents are straight.
        let one_way = matches!(extent, ExtrudeExtent::Distance(_) | ExtrudeExtent::ThroughAll | ExtrudeExtent::To(_));
        let taper = (one_way && (self.taper != 0.0 || self.keep_taper)).then(|| self.taper.to_radians());
        FeatureKind::Extrude(Extrude { sketch: self.sketch, regions: self.regions.clone(), extent, reverse, operation: self.operation, taper })
    }
}

impl RevolvePanel {
    pub(crate) fn kind(&self) -> FeatureKind {
        let angle = if !self.full && self.term == RevTerm::ToNext {
            RevolveAngle::ToNext
        } else if let (false, RevTerm::To, Some(plane)) = (self.full, self.term, &self.to) {
            RevolveAngle::To(plane.clone())
        } else if self.full {
            RevolveAngle::Full
        } else if self.symmetric || self.direction == Direction::Symmetric {
            RevolveAngle::Symmetric(self.degrees.to_radians())
        } else if self.direction == Direction::Asymmetric {
            RevolveAngle::TwoSided { forward: self.degrees.to_radians(), backward: self.degrees_b.to_radians() }
        } else {
            RevolveAngle::Angle(self.degrees.to_radians())
        };
        let reverse = !self.full && self.direction == Direction::Flipped;
        let axis = match self.axis {
            AxisChoice::Origin(a) => AxisRef::Origin(a),
            AxisChoice::Line(l) => AxisRef::SketchLine(l),
            AxisChoice::Work(w) => AxisRef::Work(w),
        };
        FeatureKind::Revolve(Revolve { sketch: self.sketch, regions: self.regions.clone(), axis, angle, operation: self.operation, reverse })
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
            Some(Panel::Primitive(p)) => self.primitive_preview(p),
            Some(Panel::Hole(p)) if !p.points.is_empty() => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Hole(p)) if !p.face_points.is_empty() => p.on_face_document(self.document()).map(|(d, _, _)| d),
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
            Some(Panel::SheetFace(p)) => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::SheetCut(p)) => with_feature(self.document(), p.editing, p.kind()),
            // With an edge picked the flange shows as it will be; further edges are picked on that.
            Some(Panel::Flange(p)) if !p.edges.is_empty() => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Flange(p)) => rolled(p.editing),
            Some(Panel::Hem(p)) if !p.edges.is_empty() => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Hem(p)) => rolled(p.editing),
            Some(Panel::ContourFlange(p)) => with_feature(self.document(), p.editing, p.kind()),
            Some(Panel::Fold(p)) => with_feature(self.document(), p.editing, p.kind()),
            // With both edges picked the bend shows as it will be.
            Some(Panel::Bend(p)) => match p.kind() {
                Some(kind) => with_feature(self.document(), p.editing, kind),
                None => rolled(p.editing),
            },
            // With both edges picked the corner shows as it will be.
            Some(Panel::CornerSeam(p)) => match p.kind() {
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
            ("fillet" | "face_fillet", Some(FeatureKind::Fillet(f))) => Panel::Fillet(FilletPanel::of(editing, f)),
            ("fillet", None) => Panel::Fillet(FilletPanel::new(editing, self.picked_edges(), 2.0)),
            ("face_fillet", None) => {
                Panel::Fillet(FilletPanel { faces: Some([Vec::new(), Vec::new()]), ..FilletPanel::new(editing, Vec::new(), 2.0) })
            }
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
            "face_fillet" => "Face Fillet: click the faces of the first set, then those of the second; the edges between them are rounded. Set the radius, then OK.",
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
        // The newest sketch with a closed profile that nothing uses yet; else the newest with
        // a profile; else the newest.
        let doc = self.document();
        let used: std::collections::BTreeSet<FeatureId> = doc.features().iter().flat_map(|f| f.kind.depends_on()).collect();
        let sketches = self.sketches();
        let newest = |want: &dyn Fn(FeatureId) -> bool| sketches.iter().rev().map(|s| s.0).find(|id| want(*id));
        newest(&|id| !used.contains(&id) && crate::sweeps::has_profile(doc, id))
            .or_else(|| newest(&|id| crate::sweeps::has_profile(doc, id)))
            .or_else(|| sketches.last().map(|s| s.0))
            .ok_or_else(|| "draw a sketch first".to_string())
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
                        ExtrudeExtent::To(_) => (ExtentChoice::To, Direction::Default, 10.0, 10.0),
                        ExtrudeExtent::ToNext => (ExtentChoice::ToNext, way, 10.0, 10.0),
                        ExtrudeExtent::Between { .. } => (ExtentChoice::Between, Direction::Default, 10.0, 10.0),
                    };
                    let (from, to) = match &e.extent {
                        ExtrudeExtent::To(plane) => (None, Some(plane.clone())),
                        ExtrudeExtent::Between { from, to } => (Some(from.clone()), Some(to.clone())),
                        _ => (None, None),
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
                        to,
                        from,
                        pick_from: false,
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
                    to: None,
                    from: None,
                    pick_from: true,
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
                        RevolveAngle::TwoSided { forward, .. } => (false, forward.to_degrees(), false),
                        RevolveAngle::To(_) | RevolveAngle::ToNext => (false, 90.0, false),
                    };
                    let (term, to) = match &r.angle {
                        RevolveAngle::To(plane) => (RevTerm::To, Some(plane.clone())),
                        RevolveAngle::ToNext => (RevTerm::ToNext, None),
                        _ => (RevTerm::Angle, None),
                    };
                    let (direction, degrees_b) = match (&r.angle, r.reverse) {
                        (RevolveAngle::TwoSided { backward, .. }, _) => (Direction::Asymmetric, backward.to_degrees()),
                        (RevolveAngle::Symmetric(_), _) => (Direction::Symmetric, 90.0),
                        (_, true) => (Direction::Flipped, 90.0),
                        _ => (Direction::Default, 90.0),
                    };
                    let axis = match r.axis {
                        AxisRef::Origin(a) => AxisChoice::Origin(a),
                        AxisRef::SketchLine(l) => AxisChoice::Line(l),
                        AxisRef::Work(w) => AxisChoice::Work(w),
                    };
                    RevolvePanel {
                        editing,
                        sketch: r.sketch,
                        axis,
                        full,
                        degrees,
                        symmetric,
                        direction,
                        degrees_b,
                        term,
                        to,
                        operation: r.operation,
                        regions: r.regions.clone(),
                    }
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
                    direction: Direction::Default,
                    degrees_b: 90.0,
                    term: RevTerm::Angle,
                    to: None,
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
                    // (With no sketch of centres the holes are placed by clicking a face.)
                    Mode::Model => self
                        .sketches()
                        .iter()
                        .rev()
                        .map(|s| s.0)
                        .find(|id| self.document().sketch(*id).is_some_and(|sk| !hole_centres(sk).is_empty()))
                        .unwrap_or(FeatureId(0)),
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
                        tapped: None,
                    },
                );
                // Drill into the part: flip when the part lies on the sketch normal's side.
                if let (Some(frame), Some(b)) = (self.sketch_frame(sketch), self.scene.bbox()) {
                    p.reverse = (b.center() - frame.origin()).dot(frame.z()) > 1e-9;
                }
                p
            }
        };
        let on_face = panel.on_face();
        self.panel = Some(Panel::Hole(panel));
        self.start_equations();
        self.set_status(if on_face {
            "Hole: click a flat face where each hole goes, set the sizes, then OK."
        } else {
            "Hole: click sketch points to add or remove centres, set the sizes, then OK."
        });
        Ok(())
    }

    /// Makes the sketch a hole placed by clicks on a face goes on, then the hole, as one step
    /// (DEC-113). Returns whether it was made.
    fn commit_hole_on_face(&mut self, ptrs: &[(&'static str, &'static str)], p: &HolePanel) -> bool {
        let Some((face, _)) = p.face.clone() else { return false };
        let mark = self.session.mark();
        let made = (|| -> Result<(), String> {
            let r = self.exec("sketch.create", json!({ "face": face, "project_origin": false }))?;
            let sketch = r["feature"].as_u64().and_then(|v| u32::try_from(v).ok()).map(FeatureId).ok_or("internal: no sketch made")?;
            let mut points = Vec::new();
            for v in &p.face_points {
                let r = self.exec("sketch.point", json!({ "sketch": sketch.0, "x": v.x, "y": v.y }))?;
                points.push(r["point"].as_u64().and_then(|v| u32::try_from(v).ok()).map(EntityId).ok_or("internal: no point made")?);
            }
            let FeatureKind::Hole(mut h) = p.kind() else { return Err("internal: not a hole".into()) };
            (h.sketch, h.points) = (sketch, points);
            if self.commit_panel(ptrs, None, FeatureKind::Hole(h)) { Ok(()) } else { Err(String::new()) }
        })();
        match made {
            Ok(()) => {
                self.session.squash_since(mark);
                true
            }
            Err(e) => {
                self.session.roll_back_to(mark);
                if !e.is_empty() {
                    self.set_error(e);
                }
                false
            }
        }
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
                            ExtrudeExtent::To(ref plane) => p["to"] = plane_json(plane),
                            ExtrudeExtent::ToNext => p["to_next"] = json!(true),
                            ExtrudeExtent::Between { ref from, ref to } => {
                                p["from"] = plane_json(from);
                                p["to"] = plane_json(to);
                            }
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
                        match &r.angle {
                            RevolveAngle::Full => {}
                            RevolveAngle::Angle(a) => p["angle"] = json!(a),
                            RevolveAngle::Symmetric(a) => {
                                p["angle"] = json!(a);
                                p["symmetric"] = json!(true);
                            }
                            RevolveAngle::TwoSided { forward, backward } => {
                                p["angle"] = json!(forward);
                                p["backward"] = json!(backward);
                            }
                            RevolveAngle::To(plane) => p["to"] = plane_json(plane),
                            RevolveAngle::ToNext => p["to_next"] = json!(true),
                        }
                        if r.reverse {
                            p["reverse"] = json!(true);
                        }
                        ("model.revolve", p)
                    }
                    FeatureKind::Fillet(f) => {
                        let mut v = json!({ "radius": f.radius });
                        match &f.faces {
                            Some(sets) => v["faces"] = json!(sets),
                            None => v["edges"] = json!(f.edges),
                        }
                        if !f.radii.is_empty() {
                            v["radii"] = json!(f.radii);
                        }
                        ("model.fillet", v)
                    }
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
                            HoleExtent::To(ref plane) => p["to"] = plane_json(plane),
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
                            HoleType::Spotface { diameter, depth } => {
                                p["type"] = json!("spotface");
                                p["spotface_diameter"] = json!(diameter);
                                p["spotface_depth"] = json!(depth);
                            }
                        }
                        if let Some(t) = &h.tapped {
                            p["tapped"] = json!(true);
                            if let tenon_model::ThreadLength::Distance(d) = t.length {
                                p["thread_length"] = json!(d);
                            }
                            if let Some(pitch) = t.pitch {
                                p["pitch"] = json!(pitch);
                            }
                            if let Some(name) = &t.designation {
                                p["designation"] = json!(name);
                            }
                            if t.left {
                                p["left"] = json!(true);
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
                    | FeatureKind::Thread(_)
                    | FeatureKind::SheetFace(_)
                    | FeatureKind::Flange(_)
                    | FeatureKind::Hem(_)
                    | FeatureKind::ContourFlange(_)
                    | FeatureKind::CornerSeam(_)
                    | FeatureKind::CornerRip(_)
                    | FeatureKind::Bend(_)
                    | FeatureKind::SharpBend(_)
                    | FeatureKind::Fold(_)
                    | FeatureKind::CutAcrossBend(_)
                    | FeatureKind::CutNormal(_) => ("feature.add", json!({ "kind": kind_json })),
                    FeatureKind::Sketch { .. } => return false,
                };
                self.exec_status(params.0, params.1)
            }
        };
        // Said in the status bar, where the panel's instruction was: what was made or changed.
        // (If its geometry then fails, the browser and the view say so.)
        if let Some(done) = &r {
            let id = done["feature"].as_u64().and_then(|v| u32::try_from(v).ok()).map(FeatureId).or(editing);
            let name = id.and_then(|id| self.document().feature(id)).or_else(|| self.document().features().last()).map(|f| f.name.clone());
            if let Some(name) = name {
                self.set_status(if editing.is_some() { format!("Changed {name}.") } else { format!("Made {name}.") });
            }
        }
        r.is_some()
    }

    pub(crate) fn panels(&mut self, ui: &mut Ui) {
        // A value a field refused this frame: said, and no OK is taken with the value from
        // before it.
        let refused = ui.ctx().data_mut(|d| d.remove_temp::<String>(egui::Id::new(crate::properties::FIELD_REFUSED)));
        if let Some(said) = &refused {
            self.set_error(said.clone());
        }
        let Some(mut panel) = self.panel.take() else { return };
        // OK, Cancel or Apply from the properties panel, the mini-toolbar or the radial menu.
        let request = match self.panel_request.take() {
            Some(PanelRequest::Ok | PanelRequest::Apply) if refused.is_some() => None,
            request => request,
        };
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
            Panel::Primitive(p) => {
                if commit {
                    match p.params() {
                        // (What it still needs is what its next click would be.)
                        None => self.set_error(p.prompt()),
                        Some(params) => {
                            keep = self.exec_status(p.shape.command(), params).is_none();
                            if !keep {
                                self.set_status(format!("{} made: a sketch and its feature, in one step.", p.shape.title()));
                                if again {
                                    reopen = Some(p.shape.command());
                                }
                            }
                        }
                    }
                }
            }
            Panel::Support(p) => {
                if commit {
                    keep = !self.commit_support(p);
                    if !keep && again {
                        reopen = Some(match p.kind {
                            tenon_model::study::SupportKind::Fixed => "study.fix",
                            tenon_model::study::SupportKind::Pin { .. } => "study.pin",
                            tenon_model::study::SupportKind::Frictionless => "study.frictionless",
                        });
                    }
                }
            }
            Panel::Load(p) => {
                if commit {
                    keep = !self.commit_load(p);
                    // (The part's weight and its turning are there once: there is no other to
                    // start.)
                    if !keep && again {
                        reopen = p.what.on_faces().then(|| p.what.command());
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
            Panel::SheetFace(p) => {
                if commit {
                    let (editing, kind) = (p.editing, p.kind());
                    keep = !self.commit_panel(&ptrs, editing, kind);
                    if !keep && again {
                        reopen = Some("sheet.face");
                    }
                }
            }
            Panel::SheetCut(p) => {
                if commit {
                    let (editing, kind) = (p.editing, p.kind());
                    keep = !self.commit_panel(&ptrs, editing, kind);
                    if !keep && again {
                        reopen = Some("sheet.cut");
                    }
                }
            }
            Panel::Flange(p) => {
                if commit {
                    if p.edges.is_empty() {
                        self.set_error("Flange: click at least one edge of the sheet".to_string());
                    } else {
                        keep = !self.commit_panel(&ptrs, p.editing, p.kind());
                        if !keep && again {
                            reopen = Some("sheet.flange");
                        }
                    }
                }
            }
            Panel::Hem(p) => {
                if commit {
                    if p.edges.is_empty() {
                        self.set_error("Hem: click at least one edge of the sheet".to_string());
                    } else {
                        keep = !self.commit_panel(&ptrs, p.editing, p.kind());
                        if !keep && again {
                            reopen = Some("sheet.hem");
                        }
                    }
                }
            }
            Panel::Bend(p) => {
                if commit {
                    match p.kind() {
                        Some(kind) => {
                            keep = !self.commit_panel(&ptrs, p.editing, kind);
                            if !keep && again {
                                reopen = Some("sheet.bend");
                            }
                        }
                        None => self.set_error("Bend: click a sharp edge between two faces, or an edge of each of two faces".to_string()),
                    }
                }
            }
            Panel::CornerSeam(p) => {
                if commit {
                    match p.kind() {
                        Some(kind) => {
                            keep = !self.commit_panel(&ptrs, p.editing, kind);
                            if !keep && again {
                                reopen = Some("sheet.corner_seam");
                            }
                        }
                        None if p.rip => self.set_error("Corner Rip: click the edge of the corner to open".to_string()),
                        None => self.set_error("Corner Seam: click the edge at the end of each of the two faces".to_string()),
                    }
                }
            }
            Panel::Fold(p) => {
                if commit {
                    keep = !self.commit_panel(&ptrs, p.editing, p.kind());
                }
            }
            Panel::ContourFlange(p) => {
                if commit {
                    keep = !self.commit_panel(&ptrs, p.editing, p.kind());
                }
            }
            Panel::Fillet(p) => {
                if commit {
                    match &p.faces {
                        Some([a, b]) if a.is_empty() || b.is_empty() => {
                            self.set_error("Face Fillet: click a face for each of the two sets".to_string());
                        }
                        None if p.edges.is_empty() => self.set_error("Fillet: select at least one edge".to_string()),
                        _ => {
                            keep = !self.commit_panel(&ptrs, p.editing, p.kind());
                            if !keep && again {
                                reopen = Some(if p.faces.is_some() { "model.face_fillet" } else { "model.fillet" });
                            }
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
                if commit && p.on_face() && !p.face_points.is_empty() {
                    // The sketch on the face, its points, and the hole: one step to undo.
                    keep = !self.commit_hole_on_face(&ptrs, p);
                    if !keep && again {
                        reopen = Some("model.hole");
                    }
                } else if commit {
                    if p.points.is_empty() {
                        self.set_error("Hole: click a flat face where it goes, or sketch points for its centres".to_string());
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
                        // The value box of the panels: what is typed takes the place of what
                        // was there, and Enter is OK. It has the keyboard as soon as it is open
                        // (given before the box is drawn: a box that finds itself with the
                        // keyboard selects what it shows, one given it afterwards does not).
                        let id = egui::Id::new("tn_value_box");
                        if ui.memory(|m| m.focused().is_none()) {
                            ui.memory_mut(|m| m.request_focus(id));
                        }
                        if crate::properties::eq_field(ui, id, &mut v.value, None, "mm", -1.0e9..=1.0e9, 90.0, &t).entered {
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
                        ValueFor::MeshSize { study } => {
                            let size = if v.value > 0.0 { json!(v.value) } else { Value::Null };
                            self.exec_status("study.mesh_size", json!({ "study": study.0, "size": size }))
                        }
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
            Panel::SketchPattern(p) => {
                keep = self.sketch_pattern_window(&ctx, p, commit) && keep;
            }
            Panel::RenameStudy { study, item, name } => {
                if name_window(&ctx, name, &mut keep) || commit {
                    let p = match item {
                        Some(item) => ("study.set", json!({ "study": study.0, "item": *item, "name": name.trim() })),
                        None => ("study.rename", json!({ "study": study.0, "name": name.trim() })),
                    };
                    keep = self.exec_status(p.0, p.1).is_none();
                }
            }
            Panel::Rename { feature, name } => {
                let ok = name_window(&ctx, name, &mut keep) || commit;
                if ok {
                    keep = self.exec_status("feature.rename", json!({ "feature": feature.0, "name": name.trim() })).is_none();
                }
            }
            Panel::DeleteFeatures { feature, consumed, dependents } => {
                let plan = self.document().deletion(*feature);
                let names = |ids: &[FeatureId]| ids.iter().map(|id| self.feature_name(*id)).collect::<Vec<_>>().join(", ");
                let (what, used, made_from) = (self.feature_name(*feature), names(&plan.dependents), names(&plan.consumed));
                let mut ok = commit;
                egui::Window::new("Delete Features").collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, [0.0, 160.0]).show(&ctx, |ui| {
                    ui.label(format!("Delete {what}, and:"));
                    ui.add_space(4.0);
                    if !plan.consumed.is_empty() {
                        crate::drawing::check(ui, consumed, "Consumed sketches and features", "tn_delete_consumed");
                        ui.label(egui::RichText::new(format!("    {made_from}")).small());
                    }
                    if !plan.dependents.is_empty() {
                        crate::drawing::check(ui, dependents, "Dependent sketches and features", "tn_delete_dependents");
                        ui.label(egui::RichText::new(format!("    {used}")).small());
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if crate::drawing::button(ui, "OK", "tn_delete_ok").clicked() {
                            ok = true;
                        }
                        if crate::drawing::button(ui, "Cancel", "tn_delete_cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if ok && keep {
                    if !*dependents && !plan.dependents.is_empty() {
                        // (Tenon keeps no feature whose parent is gone: it is deleted with it, or
                        // the parent stays.)
                        self.set_error(format!("{what} cannot go while {used} use it: tick them to delete them too, or Cancel."));
                    } else {
                        let p = json!({ "feature": feature.0, "consumed": *consumed, "dependents": *dependents });
                        match self.exec("feature.delete", p) {
                            Ok(r) => {
                                let n = r["deleted"].as_array().map_or(1, Vec::len);
                                self.set_status(if n > 1 { format!("Deleted {what} and {} more.", n - 1) } else { format!("Deleted {what}.") });
                                keep = false;
                            }
                            Err(e) => self.set_error(e),
                        }
                    }
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

/// The Rename window: a name in a box that has the keyboard, with what it shows selected, so
/// that typing replaces it. Enter or OK returns true; Cancel clears `keep`.
fn name_window(ctx: &egui::Context, name: &mut String, keep: &mut bool) -> bool {
    let mut ok = false;
    egui::Window::new("Rename").collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, [0.0, 160.0]).show(ctx, |ui| {
        let id = egui::Id::new("tn_rename_box");
        // (Given the keyboard before it is drawn, and asked whether it lost it before it is
        // given it again: else it selects nothing, and Enter does nothing.)
        if ui.memory(|m| m.focused().is_none()) {
            ui.memory_mut(|m| m.request_focus(id));
        }
        let r = ui.add(egui::TextEdit::singleline(name).id(id).desired_width(220.0));
        if r.gained_focus()
            && let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), id)
        {
            let all = egui::text_selection::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(name.chars().count()));
            state.cursor.set_char_range(Some(all));
            state.store(ui.ctx(), id);
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            ok = true;
        }
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                ok = true;
            }
            if ui.button("Cancel").clicked() {
                *keep = false;
            }
        });
    });
    ok
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
        RegionSel::Keys(keys) => Some(json!(keys)),
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
