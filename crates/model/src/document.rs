//! The part document: an ordered feature history.

use serde::{Deserialize, Serialize};
use tenon_geom::{Axis, Frame, Vec3};
use tenon_sketch::{EntityId, Sketch};

use crate::naming::{EdgeRef, FaceRef};

/// Stable id of a feature within its document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeatureId(pub u32);

impl std::fmt::Display for FeatureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "f{}", self.0)
    }
}

/// The three origin planes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OriginPlane {
    XY,
    YZ,
    XZ,
}

impl OriginPlane {
    /// Sketch frame of the plane. XY looks down from +Z; YZ looks from +X (sketch x = world Y);
    /// XZ looks from the front, -Y (sketch x = world X, sketch y = world Z).
    pub fn frame(self) -> Frame {
        match self {
            OriginPlane::XY => Frame::WORLD,
            OriginPlane::YZ => Frame::new(Vec3::ZERO, Vec3::X, Vec3::Y).unwrap_or(Frame::WORLD),
            OriginPlane::XZ => Frame::new(Vec3::ZERO, -Vec3::Y, Vec3::X).unwrap_or(Frame::WORLD),
        }
    }
}

/// The three origin axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OriginAxis {
    X,
    Y,
    Z,
}

impl OriginAxis {
    pub fn axis(self) -> Axis {
        let d = match self {
            OriginAxis::X => Vec3::X,
            OriginAxis::Y => Vec3::Y,
            OriginAxis::Z => Vec3::Z,
        };
        Axis::new(Vec3::ZERO, d).unwrap_or(Axis::Z)
    }
}

/// Where a sketch lies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaneRef {
    Origin(OriginPlane),
    /// A planar face of the part, by persistent reference.
    Face(FaceRef),
    /// A work plane feature.
    Work(FeatureId),
    /// One of the three planes of a user coordinate system (DEC-133).
    Ucs {
        feature: FeatureId,
        plane: OriginPlane,
    },
}

impl PlaneRef {
    /// The feature this plane comes from, if any.
    pub fn feature(&self) -> Option<FeatureId> {
        match self {
            PlaneRef::Origin(_) => None,
            PlaneRef::Face(f) => f.feature(),
            PlaneRef::Work(id) => Some(*id),
            PlaneRef::Ucs { feature, .. } => Some(*feature),
        }
    }
}

/// Which closed regions of the sketch a feature uses.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionSel {
    /// Every region at even nesting depth (outer regions with their holes).
    #[default]
    Default,
    /// Regions by key: the curves that enclose one, or one region of those the sketch's curves
    /// divide it into, by the side of each curve it is on (`tenon_sketch::RegionKey`).
    Keys(Vec<tenon_sketch::RegionKey>),
}

/// How a feature's solid combines with the part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// A new, separate body.
    NewBody,
    /// Add material to the current body (a new body when there is none).
    #[default]
    Join,
    /// Remove material from the current body.
    Cut,
    /// Keep only the overlap with the current body.
    Intersect,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtrudeExtent {
    Distance(f64),
    /// Total distance, centred on the sketch plane.
    Symmetric(f64),
    TwoSided {
        forward: f64,
        backward: f64,
    },
    /// Through the whole current body (for cuts).
    ThroughAll,
    /// As far as a plane: a planar face, a work plane or an origin plane, on whichever side of
    /// the sketch it is (DEC-104). At an angle to the sketch, the end is cut along the plane.
    To(PlaneRef),
    /// As far as the part's next faces (DEC-109): joined, up to where it meets the part; cut,
    /// through the material it starts in, to where that ends.
    ToNext,
    /// From one plane to another, wherever the sketch is (DEC-111): a flat face, a work plane or
    /// an origin plane each; at an angle, the ends are cut along them.
    Between {
        from: PlaneRef,
        to: PlaneRef,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Extrude {
    pub sketch: FeatureId,
    #[serde(default)]
    pub regions: RegionSel,
    pub extent: ExtrudeExtent,
    /// Go against the sketch normal.
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub operation: Operation,
    /// Radians the sides lean in as they leave the sketch (negative: lean out). Without it, or at
    /// zero, the sides are straight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<f64>,
}

impl Extrude {
    pub fn check(&self) -> Result<(), String> {
        let taper = self.taper.unwrap_or(0.0);
        if !(taper.is_finite() && taper.abs() <= MAX_DRAFT_ANGLE) {
            return Err("the taper must be at most 85 degrees either way".into());
        }
        if taper != 0.0 && !matches!(self.extent, ExtrudeExtent::Distance(_) | ExtrudeExtent::ThroughAll | ExtrudeExtent::To(_)) {
            return Err(
                "A tapered extrusion goes one way from its sketch: use one distance or Through All, or taper each side as an extrusion of its own."
                    .into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisRef {
    /// A line of the profile's own sketch.
    SketchLine(EntityId),
    Origin(OriginAxis),
    /// A work axis feature.
    Work(FeatureId),
    /// One of the three axes of a user coordinate system (DEC-133).
    Ucs {
        feature: FeatureId,
        axis: OriginAxis,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevolveAngle {
    Full,
    Angle(f64),
    Symmetric(f64),
    /// Two angles: `forward` the default way, `backward` the other (Inventor's Asymmetric,
    /// DEC-108).
    TwoSided {
        forward: f64,
        backward: f64,
    },
    /// Round to a plane through the axis (DEC-112): a flat face (to the half of the plane it is
    /// on) or a work or origin plane (to where the profile first meets it).
    To(PlaneRef),
    /// Round to the part's next faces (DEC-112), as Extrude's To Next.
    ToNext,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Revolve {
    pub sketch: FeatureId,
    #[serde(default)]
    pub regions: RegionSel,
    pub axis: AxisRef,
    pub angle: RevolveAngle,
    #[serde(default)]
    pub operation: Operation,
    /// Turned the other way round the axis (Inventor's Flipped, DEC-108).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reverse: bool,
}

/// Rounds edges: with one radius, a radius that varies along each edge, or the edges between
/// two sets of faces.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Fillet {
    pub edges: Vec<EdgeRef>,
    /// The radius; with `radii`, the radius at the start of each edge.
    pub radius: f64,
    /// Variable radius (DEC-122): the radius further along each edge (or each run of edges that
    /// carry on smoothly into one another), `at` from 0 at its start to 1 at its end, in order.
    /// Between them the radius changes evenly (one further point) or smoothly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub radii: Vec<RadiusAt>,
    /// Face fillet (DEC-122): two sets of faces; what is rounded is every edge between a face of
    /// one and a face of the other, found again whenever the part is rebuilt (`edges` is empty).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faces: Option<[Vec<FaceRef>; 2]>,
}

/// A variable fillet's radius at a place along its edges (DEC-122).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RadiusAt {
    /// From 0 at the start of the edge to 1 at its end.
    pub at: f64,
    pub radius: f64,
}

impl Fillet {
    /// Why it cannot be a fillet, if it cannot.
    pub fn check(&self) -> Result<(), String> {
        let ok = |r: f64| r.is_finite() && r > 0.0;
        if !ok(self.radius) || self.radii.iter().any(|r| !ok(r.radius)) {
            return Err("a fillet's radii must be positive".into());
        }
        if self.radii.iter().any(|r| !(r.at > 0.0 && r.at <= 1.0)) || self.radii.windows(2).any(|w| w[1].at <= w[0].at) {
            return Err("the places of a variable fillet's radii must be in order along the edge, after its start and up to its end (0 to 1)".into());
        }
        match &self.faces {
            Some([a, b]) if a.is_empty() || b.is_empty() => Err("a face fillet needs a face in each of its two sets".into()),
            Some(_) if !self.edges.is_empty() => Err("a face fillet rounds the edges between its faces: give faces or edges, not both".into()),
            None if self.edges.is_empty() => Err("select at least one edge".into()),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChamferSize {
    /// The same distance on both faces.
    Equal(f64),
    /// `d1` on the `reference` face, `d2` on the other.
    TwoDistances { d1: f64, d2: f64, reference: FaceRef },
    /// `distance` on the `reference` face, then `angle` (radians) from it.
    DistanceAngle { distance: f64, angle: f64, reference: FaceRef },
}

/// Bevels edges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chamfer {
    pub edges: Vec<EdgeRef>,
    pub size: ChamferSize,
}

/// Hollows the body, leaving walls of `thickness` and removing the `remove` faces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shell {
    #[serde(default)]
    pub remove: Vec<FaceRef>,
    pub thickness: f64,
    /// Grow the wall outwards instead of inwards.
    #[serde(default)]
    pub outside: bool,
}

/// The shape of a hole's top.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleType {
    Simple,
    Counterbore {
        diameter: f64,
        depth: f64,
    },
    /// `angle` is the included angle of the countersink (radians).
    Countersink {
        diameter: f64,
        angle: f64,
    },
    /// A shallow seat, as Inventor's: the hole's depth is measured from its floor (DEC-105).
    /// Zero deep, it is only where the hole starts.
    Spotface {
        diameter: f64,
        depth: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleExtent {
    /// Depth of the full-diameter part, from the sketch plane (the drill point is extra).
    Distance(f64),
    ThroughAll,
    /// As far as a plane: each hole along its own axis to where that meets the plane (DEC-105).
    To(PlaneRef),
}

/// Holes drilled at points of a sketch, against the sketch normal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hole {
    pub sketch: FeatureId,
    /// The sketch points at the hole centres.
    pub points: Vec<EntityId>,
    pub diameter: f64,
    pub kind: HoleType,
    pub extent: HoleExtent,
    /// Included angle of the drill point (radians) for blind holes; `None` for a flat bottom.
    #[serde(default)]
    pub tip_angle: Option<f64>,
    /// Drill along the sketch normal instead.
    #[serde(default)]
    pub reverse: bool,
    /// Tapped: a thread on each hole's wall, from its mouth (DEC-114).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tapped: Option<Tapping>,
}

/// A tapped hole's thread: cosmetic, on each hole's wall from its mouth, as Thread puts one on
/// (DEC-114).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tapping {
    /// Millimetres from one turn to the next; without it, the ISO coarse pitch for the hole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f64>,
    /// What it is called on drawings; without it, named from its size and pitch ("M8x1.25").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub designation: Option<String>,
    #[serde(default)]
    pub length: ThreadLength,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub left: bool,
}

/// The points of a sketch a hole goes at by default: centre points, i.e. the sketch's own points that are not
/// construction, not on any curve and not marked as sketch points (DEC-117).
pub fn hole_centres(sk: &Sketch) -> Vec<EntityId> {
    sk.entities().map(|(id, _)| id).filter(|id| sk.is_center_point(*id)).collect()
}

/// What a thread may be called on drawings.
fn designation_ok(d: &str) -> bool {
    !d.trim().is_empty() && d.chars().count() <= 64
}

const BAD_DESIGNATION: &str = "the thread designation must be 1 to 64 characters";

/// The usual drill point angle, 118 degrees.
pub const DRILL_POINT: f64 = 118.0 * std::f64::consts::PI / 180.0;

impl Hole {
    /// Why the hole's sizes cannot make a hole, if they cannot.
    pub fn check(&self) -> Result<(), String> {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if self.tapped.as_ref().and_then(|t| t.designation.as_ref()).is_some_and(|d| !designation_ok(d)) {
            return Err(BAD_DESIGNATION.into());
        }
        if !positive(self.diameter) {
            return Err("the hole diameter must be positive".into());
        }
        let depth = match self.extent {
            HoleExtent::Distance(d) if !positive(d) => return Err("the hole depth must be positive".into()),
            HoleExtent::Distance(d) => Some(d),
            HoleExtent::ThroughAll | HoleExtent::To(_) => None,
        };
        match self.kind {
            HoleType::Simple => {}
            HoleType::Counterbore { diameter, depth: cb } => {
                if !positive(diameter) || diameter <= self.diameter {
                    return Err("the counterbore must be wider than the hole".into());
                }
                if !positive(cb) || depth.is_some_and(|d| cb >= d) {
                    return Err("the counterbore depth must be positive and less than the hole depth".into());
                }
            }
            HoleType::Countersink { diameter, angle } => {
                if !positive(diameter) || diameter <= self.diameter {
                    return Err("the countersink must be wider than the hole".into());
                }
                if !(angle.is_finite() && angle > 0.0 && angle < std::f64::consts::PI) {
                    return Err("the countersink angle must be between 0 and 180 degrees".into());
                }
                let sink = (diameter - self.diameter) / 2.0 / (angle / 2.0).tan();
                if depth.is_some_and(|d| sink >= d) {
                    return Err("the countersink is deeper than the hole".into());
                }
            }
            // (The hole's depth is from the spotface's floor, so any depth of spotface goes.)
            HoleType::Spotface { diameter, depth: sf } => {
                if !positive(diameter) || diameter <= self.diameter {
                    return Err("the spotface must be wider than the hole".into());
                }
                if !(sf.is_finite() && sf >= 0.0) {
                    return Err("the spotface depth must be zero or more".into());
                }
            }
        }
        if let Some(a) = self.tip_angle
            && !(a.is_finite() && a > 0.0 && a < std::f64::consts::PI)
        {
            return Err("the drill point angle must be between 0 and 180 degrees".into());
        }
        if self.points.is_empty() {
            return Err("the hole has no centre points".into());
        }
        Ok(())
    }
}

/// How far a rib grows from its lines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RibExtent {
    /// Until it meets the part.
    ToNext,
    Distance(f64),
}

/// A thin wall from open lines of a sketch: in the sketch plane, `thickness` thick (half on each
/// side of the plane), growing from each line toward the part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rib {
    pub sketch: FeatureId,
    pub lines: Vec<EntityId>,
    pub thickness: f64,
    pub extent: RibExtent,
    /// Grow away from the part instead.
    #[serde(default)]
    pub flip: bool,
}

impl Rib {
    pub fn check(&self) -> Result<(), String> {
        if !(self.thickness.is_finite() && self.thickness > 0.0) {
            return Err("the rib thickness must be positive".into());
        }
        if let RibExtent::Distance(d) = self.extent
            && !(d.is_finite() && d > 0.0)
        {
            return Err("the rib distance must be positive".into());
        }
        if self.lines.is_empty() {
            return Err("the rib needs at least one sketch line".into());
        }
        Ok(())
    }
}

/// The lines a rib uses by default: the sketch's lines that are not construction and not part
/// of a closed profile.
pub fn open_lines(sk: &Sketch) -> Vec<EntityId> {
    let closed: std::collections::BTreeSet<EntityId> = tenon_sketch::regions(sk).iter().flat_map(|r| r.key.iter().copied()).collect();
    sk.entities().filter(|(id, e)| !e.construction && sk.is_line(*id) && !closed.contains(id)).map(|(id, _)| id).collect()
}

/// Lines and arcs of a sketch, or lines, bends and splines of a 3D sketch, joined end to end: a
/// sweep's path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchCurves {
    pub sketch: FeatureId,
    pub curves: Vec<EntityId>,
}

/// A profile swept along a path drawn in another sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sweep {
    /// The profile's sketch.
    pub sketch: FeatureId,
    #[serde(default)]
    pub regions: RegionSel,
    pub path: SketchCurves,
    /// Keep the profile's orientation as it is, instead of turning it with the path.
    #[serde(default)]
    pub fixed: bool,
    #[serde(default)]
    pub operation: Operation,
}

/// Most curves a sweep's path names.
pub const MAX_SWEEP_CURVES: usize = 1000;

impl Sweep {
    pub fn check(&self) -> Result<(), String> {
        if self.path.curves.is_empty() || self.path.curves.len() > MAX_SWEEP_CURVES {
            return Err(format!("the sweep path needs 1 to {MAX_SWEEP_CURVES} lines and arcs"));
        }
        Ok(())
    }
}

/// A profile swept round an axis along a helix: springs, coils, threads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coil {
    pub sketch: FeatureId,
    #[serde(default)]
    pub regions: RegionSel,
    pub axis: AxisRef,
    /// How far the coil advances along the axis in one turn (mm).
    pub pitch: f64,
    pub turns: f64,
    /// Left-handed: turning clockwise when seen going along the axis.
    #[serde(default)]
    pub left: bool,
    #[serde(default)]
    pub operation: Operation,
}

impl Coil {
    pub fn check(&self) -> Result<(), String> {
        if !tenon_geom::tol::is_valid_size(self.pitch) {
            return Err("the coil pitch must be positive".into());
        }
        if !(self.turns.is_finite() && self.turns > 1e-6 && self.turns <= 10_000.0) {
            return Err("the coil needs 0 to 10000 turns".into());
        }
        Ok(())
    }
}

/// A solid through the profiles of two or more sketches, in order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Loft {
    pub sections: Vec<FeatureId>,
    /// Flat sides between sections instead of smooth ones.
    #[serde(default)]
    pub ruled: bool,
    #[serde(default)]
    pub operation: Operation,
}

impl Loft {
    pub fn check(&self) -> Result<(), String> {
        if self.sections.len() < 2 || self.sections.len() > 100 {
            return Err("a loft needs 2 to 100 section sketches".into());
        }
        let unique: std::collections::BTreeSet<_> = self.sections.iter().collect();
        if unique.len() != self.sections.len() {
            return Err("a sketch is a loft section only once".into());
        }
        Ok(())
    }
}

/// Tilts faces of a body about a neutral plane, so a moulded or cast part comes out of its tool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    pub faces: Vec<FaceRef>,
    /// The neutral plane: where it crosses a face, the face stays put. The part is pulled along
    /// its normal.
    pub plane: PlaneRef,
    /// Radians from the pull direction.
    pub angle: f64,
    /// Tilt the other way (material is removed above the plane instead of added).
    #[serde(default)]
    pub reverse: bool,
}

/// Most a draft may tilt its faces.
pub const MAX_DRAFT_ANGLE: f64 = 1.5; // just under 86 degrees

impl Draft {
    pub fn check(&self) -> Result<(), String> {
        if self.faces.is_empty() || self.faces.len() > 1000 {
            return Err("a draft needs 1 to 1000 faces".into());
        }
        if !(self.angle.is_finite() && self.angle > 0.0 && self.angle <= MAX_DRAFT_ANGLE) {
            return Err("the draft angle must be more than 0 and at most 85 degrees".into());
        }
        Ok(())
    }
}

/// Which pieces of a split body stay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitKeep {
    /// Both, as two bodies.
    #[default]
    Both,
    /// The piece on the side the plane's normal points to.
    Front,
    /// The piece on the other side.
    Back,
}

/// Cuts bodies along a plane, into two bodies or keeping one side.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Split {
    pub plane: PlaneRef,
    #[serde(default)]
    pub keep: SplitKeep,
    /// A face of the one body to split. Without it, every body the plane crosses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<FaceRef>,
}

/// Joins bodies to a base body, cuts them from it or keeps what they share with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Combine {
    /// A face of the body that stays and changes.
    pub base: FaceRef,
    /// A face of each of the other bodies.
    pub tools: Vec<FaceRef>,
    /// Join, cut or intersect.
    #[serde(default)]
    pub operation: Operation,
    /// Leave the other bodies in the part instead of using them up.
    #[serde(default)]
    pub keep_tools: bool,
}

impl Combine {
    pub fn check(&self) -> Result<(), String> {
        if self.tools.is_empty() || self.tools.len() > 100 {
            return Err("Combine needs 1 to 100 other bodies".into());
        }
        if self.operation == Operation::NewBody {
            return Err("Combine joins, cuts or intersects: it cannot make a new body".into());
        }
        Ok(())
    }
}

/// How far a thread runs along its face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadLength {
    /// The whole face.
    #[default]
    Full,
    /// This far from the end it starts at.
    Distance(f64),
}

/// A screw thread on a cylindrical face: a shaft (an external thread) or a hole (an internal
/// one).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thread {
    pub face: FaceRef,
    /// Millimetres from one turn to the next. Without it, the ISO coarse pitch for the face's
    /// diameter, which follows the face when its size changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f64>,
    /// What the thread is called on drawings ("M8x1.25", "G1/4"). Without it, named from its
    /// diameter and pitch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub designation: Option<String>,
    #[serde(default)]
    pub length: ThreadLength,
    /// Start from the other end of the face. (A thread starts at its face's open end: a bolt's
    /// tip, a hole's mouth. With both ends open, or neither, it starts at the end further along
    /// the axis taken with its largest part positive: the top, for an upright part.)
    #[serde(default)]
    pub reverse: bool,
    /// Left-handed.
    #[serde(default)]
    pub left: bool,
    /// Cut the thread's groove into the part. Otherwise the thread is cosmetic: the face is
    /// recorded and shown as threaded and stays a plain cylinder, which is quick and is enough
    /// for drawings and for parts that are tapped or bought.
    #[serde(default)]
    pub modelled: bool,
}

impl Thread {
    pub fn check(&self) -> Result<(), String> {
        if let Some(p) = self.pitch
            && !(p.is_finite() && (0.05..=50.0).contains(&p))
        {
            return Err("the thread pitch must be from 0.05 to 50 mm".into());
        }
        if let ThreadLength::Distance(d) = self.length
            && !(d.is_finite() && d > 0.0)
        {
            return Err("the thread length must be more than 0".into());
        }
        if self.designation.as_ref().is_some_and(|d| !designation_ok(d)) {
            return Err(BAD_DESIGNATION.into());
        }
        Ok(())
    }
}

/// How an emboss meets the part (DEC-133).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbossStyle {
    /// Raised from the faces the profile is seen on, by the depth.
    #[default]
    Raise,
    /// Recessed into them, by the depth.
    Recess,
    /// From the sketch plane: material added on one side of it and taken away on the other, each
    /// as far as the depth.
    FromPlane,
}

/// A closed profile raised from or recessed into the part's faces, following them where they
/// curve, or laid round a cylinder or a cone with its lengths kept (DEC-133).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Emboss {
    pub sketch: FeatureId,
    #[serde(default)]
    pub regions: RegionSel,
    #[serde(default)]
    pub style: EmbossStyle,
    /// How far from the faces, or from the sketch plane.
    pub depth: f64,
    /// The faces that look against the sketch's normal, not along it; from the plane, the sides
    /// changed over.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reverse: bool,
    /// Radians the sides lean, so that what is raised narrows going up and what is recessed
    /// narrows going down (negative: the other way). Not with `wrap`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<f64>,
    /// Wrap to Face: the cylinder or cone the profile is laid round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrap: Option<FaceRef>,
}

impl Emboss {
    pub fn check(&self) -> Result<(), String> {
        if !tenon_geom::tol::is_valid_size(self.depth) {
            return Err("the emboss depth must be more than 0".into());
        }
        let taper = self.taper.unwrap_or(0.0);
        if !(taper.is_finite() && taper.abs() <= MAX_DRAFT_ANGLE) {
            return Err("the taper must be at most 85 degrees either way".into());
        }
        if self.wrap.is_some() && self.style == EmbossStyle::FromPlane {
            return Err("Wrap to Face raises or recesses the profile on the surface: it does not go with From Plane.".into());
        }
        if self.wrap.is_some() && taper != 0.0 {
            return Err("A profile wrapped round a face has straight sides here: take the taper off, or leave Wrap to Face.".into());
        }
        Ok(())
    }
}

/// The solid of another part file brought into this one (DEC-133). Nothing of the other file's
/// contents is in this one: its solid is rebuilt from the file each time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Derive {
    /// The other part's file. In a file, its path relative to that file's folder with `/`
    /// separators; in memory, its full path, as an assembly holds its parts' (DEC-024).
    pub path: String,
    /// Scaled by this about the part's origin.
    #[serde(default = "unit_scale")]
    pub scale: f64,
    /// Then mirrored in this plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<PlaneRef>,
    /// Joined to the part, or a new solid.
    #[serde(default)]
    pub operation: Operation,
}

fn unit_scale() -> f64 {
    1.0
}

impl Derive {
    /// Whether what it names can be a file: what a file is held to when read.
    pub fn check_path(&self) -> Result<(), String> {
        if self.path.trim().is_empty() || self.path.len() > 4096 || self.path.contains('\0') {
            return Err("a derived part needs the path of a part file".into());
        }
        Ok(())
    }

    /// Why it cannot be built, if it cannot.
    pub fn check(&self) -> Result<(), String> {
        self.check_path()?;
        if !(self.scale.is_finite() && (1e-3..=1e3).contains(&self.scale)) {
            return Err("the scale of a derived part must be from 0.001 to 1000".into());
        }
        if !matches!(self.operation, Operation::Join | Operation::NewBody) {
            return Err("a derived part is joined to the part or made a new solid".into());
        }
        Ok(())
    }

    /// The file's name, for messages.
    pub fn file_name(&self) -> String {
        self.path.rsplit(['/', '\\']).next().unwrap_or(&self.path).to_owned()
    }
}

/// What has been read of the part files a document's features derive from. It is given to the
/// document from outside (this crate does not read files), travels with it to wherever it is
/// rebuilt, and is never written to a file.
#[derive(Clone, Debug, Default)]
pub struct Links {
    /// The document's own file, in full, when it has one.
    pub home: Option<String>,
    /// By the path as the feature holds it.
    pub sources: std::collections::BTreeMap<String, Source>,
}

/// Two documents are the same part wherever their files are.
impl PartialEq for Links {
    fn eq(&self, other: &Links) -> bool {
        self.sources == other.sources
    }
}

/// What became of reading a part file.
#[derive(Clone, Debug)]
pub struct Source {
    /// The part, with what it derives from read too; or what is wrong, in words.
    pub document: Result<std::sync::Arc<Document>, String>,
    /// Changes when the file or anything it derives from does.
    pub stamp: u64,
}

impl PartialEq for Source {
    fn eq(&self, other: &Source) -> bool {
        self.stamp == other.stamp
    }
}

/// A direction for a pattern: an origin axis or a straight edge of the part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectionRef {
    Origin(OriginAxis),
    Edge(EdgeRef),
    /// Along a work axis.
    Work(FeatureId),
    /// Along one of the three axes of a user coordinate system (DEC-133).
    Ucs {
        feature: FeatureId,
        axis: OriginAxis,
    },
}

/// An axis to turn about: an origin axis, a straight or circular edge, a cylindrical or conical
/// face, or a work axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisSel {
    Origin(OriginAxis),
    Edge(EdgeRef),
    Face(FaceRef),
    Work(FeatureId),
    /// One of the three axes of a user coordinate system (DEC-133).
    Ucs {
        feature: FeatureId,
        axis: OriginAxis,
    },
    /// Along a line of a 3D sketch (DEC-136).
    Sketch3dLine {
        sketch: FeatureId,
        line: EntityId,
    },
}

impl AxisSel {
    /// The features this axis comes from.
    pub fn features(&self) -> Vec<FeatureId> {
        match self {
            AxisSel::Origin(_) => vec![],
            AxisSel::Edge(e) => e.features(),
            AxisSel::Face(f) => f.feature().into_iter().collect(),
            AxisSel::Work(id) | AxisSel::Ucs { feature: id, .. } => vec![*id],
            AxisSel::Sketch3dLine { sketch, .. } => vec![*sketch],
        }
    }
}

/// A work plane: construction geometry to sketch on, mirror across, and so on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum WorkPlane {
    /// `base` moved `distance` along its normal.
    Offset { base: PlaneRef, distance: f64 },
    /// Through `axis` (which lies in `base`), turned `angle` radians from `base`.
    Angle { base: PlaneRef, axis: AxisSel, angle: f64 },
    /// Halfway between two parallel planes.
    Midplane { a: PlaneRef, b: PlaneRef },
    /// Through three points (DEC-116): from the first toward the second is its X, the third on
    /// its positive Y side.
    ThreePoints { points: [PointSel; 3] },
    /// Through the end `end` of a curve of a 3D sketch, square to the curve there (DEC-136): where
    /// a sweep's profile is drawn.
    SquareToCurve { sketch: FeatureId, curve: EntityId, end: EntityId },
}

/// A point a work feature goes through (DEC-116).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointSel {
    /// The part's origin.
    Origin,
    /// A work point.
    Work(FeatureId),
    /// The centre of a circular edge.
    Center(EdgeRef),
    /// A point of a sketch.
    SketchPoint { sketch: FeatureId, point: EntityId },
    /// The origin of a user coordinate system (DEC-133).
    Ucs(FeatureId),
    /// A point of a 3D sketch (DEC-136).
    Sketch3dPoint { sketch: FeatureId, point: EntityId },
    /// A corner of the part: an end of an edge, 0 where the edge starts and 1 where it ends, as
    /// the kernel had it when it was chosen (DEC-136). The kernel may later name the ends the
    /// other way round, so `towards` says which is meant: the way from the edge's other end to
    /// this one, as it was then.
    Vertex {
        edge: EdgeRef,
        end: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        towards: Option<Vec3>,
    },
}

impl PointSel {
    /// The features it comes from.
    pub fn features(&self) -> Vec<FeatureId> {
        match self {
            PointSel::Origin => Vec::new(),
            PointSel::Work(id) | PointSel::Ucs(id) => vec![*id],
            PointSel::Center(e) | PointSel::Vertex { edge: e, .. } => e.features(),
            PointSel::SketchPoint { sketch, .. } | PointSel::Sketch3dPoint { sketch, .. } => vec![*sketch],
        }
    }

    /// Which end of an edge from `start` to `stop` a vertex is, and which way that lies from the
    /// other: where `towards` says, if it says, else by `end`.
    pub fn end_of_edge(start: Vec3, stop: Vec3, end: u8, towards: Option<Vec3>) -> (Vec3, Vec3) {
        let (this, other) = if end == 0 { (start, stop) } else { (stop, start) };
        let (this, other) = match towards {
            Some(t) if (this - other).dot(t) < 0.0 => (other, this),
            _ => (this, other),
        };
        (this, this - other)
    }
}

/// A work axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum WorkAxis {
    /// Along an edge, the axis of a cylindrical face, or an origin axis.
    Along { axis: AxisSel },
    /// Where two planes meet.
    Planes { a: PlaneRef, b: PlaneRef },
}

/// A work point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum WorkPoint {
    /// The centre of a circular edge.
    Center { edge: EdgeRef },
    /// Where an axis meets a plane.
    Intersection { axis: AxisSel, plane: PlaneRef },
    /// At a point: a point of a 3D sketch, of a sketch or a vertex (DEC-136).
    At { point: PointSel },
}

/// A user coordinate system (DEC-133): an origin with X, Y and Z directions, whose three planes,
/// three axes and origin serve wherever work planes, axes and points do.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum Ucs {
    /// Moved along the axes of the part's origin or of another UCS (`base`), then turned `rx`
    /// radians about its own X, `ry` about the Y that gives and `rz` about the Z that gives.
    Offset {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<FeatureId>,
        #[serde(default)]
        x: f64,
        #[serde(default)]
        y: f64,
        #[serde(default)]
        z: f64,
        #[serde(default)]
        rx: f64,
        #[serde(default)]
        ry: f64,
        #[serde(default)]
        rz: f64,
    },
    /// Through three points: its origin, a point on its positive X, and a point on the positive
    /// Y side of its XY plane.
    ThreePoints { points: Box<[PointSel; 3]> },
}

impl Ucs {
    /// Why its numbers cannot place one, if they cannot.
    pub fn check(&self) -> Result<(), String> {
        if let Ucs::Offset { x, y, z, rx, ry, rz, .. } = self {
            if [x, y, z].iter().any(|v| !tenon_geom::tol::is_valid_coord(**v)) {
                return Err("a UCS's offsets must be numbers within range".into());
            }
            if [rx, ry, rz].iter().any(|v| !(v.is_finite() && v.abs() <= 2.0 * std::f64::consts::TAU)) {
                return Err("a UCS's turns must be numbers of at most two whole turns either way".into());
            }
        }
        Ok(())
    }
}

/// Copies of features in rows and columns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RectPattern {
    pub features: Vec<FeatureId>,
    pub dir1: DirectionRef,
    pub count1: u32,
    pub spacing1: f64,
    #[serde(default)]
    pub reverse1: bool,
    /// A second direction (`count2` 1 for a single row).
    #[serde(default)]
    pub dir2: Option<DirectionRef>,
    #[serde(default = "one")]
    pub count2: u32,
    #[serde(default)]
    pub spacing2: f64,
    #[serde(default)]
    pub reverse2: bool,
}

fn one() -> u32 {
    1
}

/// Copies of features around an axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CircPattern {
    pub features: Vec<FeatureId>,
    pub axis: AxisSel,
    pub count: u32,
    /// Total angle (radians): a full turn spaces the copies evenly, less puts the last copy at
    /// the end of the angle.
    pub angle: f64,
    #[serde(default)]
    pub reverse: bool,
}

/// Mirror images of features across a plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mirror {
    pub features: Vec<FeatureId>,
    pub plane: PlaneRef,
}

/// Most copies one pattern may make (hostile-input cap).
pub const MAX_COPIES: u32 = 10_000;

impl RectPattern {
    pub fn check(&self) -> Result<(), String> {
        check_sources(&self.features)?;
        if self.count1 < 1 || self.count2 < 1 || self.count1.saturating_mul(self.count2) > MAX_COPIES {
            return Err(format!("the counts must be at least 1 and make at most {MAX_COPIES} copies"));
        }
        if self.count1.saturating_mul(self.count2) < 2 {
            return Err("a pattern needs at least two occurrences".into());
        }
        for (count, spacing) in [(self.count1, self.spacing1), (self.count2, self.spacing2)] {
            if count > 1 && !(spacing.is_finite() && spacing > 0.0) {
                return Err("the spacing must be positive".into());
            }
        }
        if self.count2 > 1 && self.dir2.is_none() {
            return Err("a second count needs a second direction".into());
        }
        Ok(())
    }
}

impl CircPattern {
    pub fn check(&self) -> Result<(), String> {
        check_sources(&self.features)?;
        if self.count < 2 || self.count > MAX_COPIES {
            return Err(format!("the count must be 2 to {MAX_COPIES}"));
        }
        if !(self.angle.is_finite() && self.angle > 0.0 && self.angle <= std::f64::consts::TAU + 1e-9) {
            return Err("the angle must be more than 0 and at most 360 degrees".into());
        }
        Ok(())
    }

    /// The turn from one copy to the next (radians).
    pub fn step(&self) -> f64 {
        let full = (self.angle - std::f64::consts::TAU).abs() < 1e-9;
        let step = if full { self.angle / f64::from(self.count) } else { self.angle / f64::from(self.count.saturating_sub(1).max(1)) };
        if self.reverse { -step } else { step }
    }
}

fn check_sources(features: &[FeatureId]) -> Result<(), String> {
    if features.is_empty() {
        return Err("choose at least one feature to copy".into());
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeatureKind {
    Sketch {
        plane: PlaneRef,
        sketch: Sketch,
        /// Edges of the part shown in the sketch as reference geometry (DEC-055).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projections: Vec<crate::project::Projection>,
    },
    /// A sketch on no plane (DEC-136).
    Sketch3d {
        sketch: tenon_sketch::Sketch3d,
        /// What its reference geometry stands for: vertices, work points, edges of the part.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        links: Vec<crate::sketch3d::Link3>,
    },
    Extrude(Extrude),
    Revolve(Revolve),
    Fillet(Fillet),
    Chamfer(Chamfer),
    Shell(Shell),
    Hole(Hole),
    PatternRect(RectPattern),
    PatternCircular(CircPattern),
    Mirror(Mirror),
    WorkPlane(WorkPlane),
    WorkAxis(WorkAxis),
    WorkPoint(WorkPoint),
    /// A user coordinate system.
    Ucs(Ucs),
    Rib(Rib),
    Sweep(Sweep),
    Coil(Coil),
    Loft(Loft),
    Draft(Draft),
    Split(Split),
    Combine(Combine),
    Thread(Thread),
    Emboss(Emboss),
    Derive(Derive),
    /// A flat piece of sheet metal (see `sheet.rs`).
    SheetFace(crate::sheet::Face),
    /// Walls bent up from edges of the sheet.
    Flange(crate::sheet::Flange),
    /// An edge of the sheet folded back on itself.
    Hem(crate::sheet::Hem),
    /// A strip of sheet along an open profile.
    ContourFlange(crate::sheet::ContourFlange),
    /// The corner between two faces of the sheet closed.
    CornerSeam(crate::sheet::CornerSeam),
    /// A square corner between two walls of the sheet opened along its edge.
    CornerRip(crate::sheet::CornerRip),
    /// A profile cut out of the sheet round its bends.
    CutAcrossBend(crate::sheet::CutAcrossBend),
    /// A profile cut out of the sheet where it is seen on it, square to every face.
    CutNormal(crate::sheet::CutNormal),
    /// Two faces of the sheet that do not touch, joined by a bend.
    Bend(crate::sheet::BendBetween),
    /// Sharp edges between faces of the sheet made bends.
    SharpBend(crate::sheet::SharpBend),
    /// A face of the sheet folded along a line drawn across it.
    Fold(crate::sheet::Fold),
}

impl FeatureKind {
    /// Whether its sizes can make a feature. A rebuild asks this first, and says so on the feature.
    pub fn check_sizes(&self) -> Result<(), String> {
        match self {
            FeatureKind::Extrude(e) => e.check(),
            FeatureKind::Hole(h) => h.check(),
            FeatureKind::Rib(r) => r.check(),
            FeatureKind::Sweep(s) => s.check(),
            FeatureKind::Coil(c) => c.check(),
            FeatureKind::Loft(l) => l.check(),
            FeatureKind::Draft(d) => d.check(),
            FeatureKind::Combine(c) => c.check(),
            FeatureKind::Thread(t) => t.check(),
            FeatureKind::SheetFace(s) => s.check(),
            FeatureKind::Flange(f) => f.check(),
            FeatureKind::Hem(h) => h.check(),
            FeatureKind::ContourFlange(c) => c.check(),
            FeatureKind::CornerSeam(c) => c.check(),
            FeatureKind::CornerRip(c) => c.check(),
            FeatureKind::CutAcrossBend(c) => c.check(),
            FeatureKind::CutNormal(c) => c.check(),
            FeatureKind::Bend(b) => b.check(),
            FeatureKind::SharpBend(b) => b.check(),
            FeatureKind::Fold(f) => f.check(),
            FeatureKind::PatternRect(p) => p.check(),
            FeatureKind::PatternCircular(p) => p.check(),
            FeatureKind::Mirror(m) => check_sources(&m.features),
            FeatureKind::Ucs(u) => u.check(),
            FeatureKind::Emboss(e) => e.check(),
            FeatureKind::Derive(d) => d.check(),
            _ => Ok(()),
        }
    }

    /// The features this one copies (patterns and mirrors).
    pub fn copies(&self) -> &[FeatureId] {
        match self {
            FeatureKind::PatternRect(p) => &p.features,
            FeatureKind::PatternCircular(p) => &p.features,
            FeatureKind::Mirror(m) => &m.features,
            _ => &[],
        }
    }
    /// True for features whose solid adds or removes material, and so can be copied.
    pub fn has_tool(&self) -> bool {
        matches!(
            self,
            FeatureKind::Extrude(_)
                | FeatureKind::Revolve(_)
                | FeatureKind::Hole(_)
                | FeatureKind::Rib(_)
                | FeatureKind::Sweep(_)
                | FeatureKind::Coil(_)
                | FeatureKind::Loft(_)
                | FeatureKind::Emboss(_)
                | FeatureKind::Derive(_)
                | FeatureKind::SheetFace(_)
                | FeatureKind::PatternRect(_)
                | FeatureKind::PatternCircular(_)
                | FeatureKind::Mirror(_)
        )
    }
}

impl FeatureKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Sketch3d { .. } => "3D Sketch",
            FeatureKind::Extrude(_) => "Extrude",
            FeatureKind::Revolve(_) => "Revolve",
            FeatureKind::Fillet(_) => "Fillet",
            FeatureKind::Chamfer(_) => "Chamfer",
            FeatureKind::Shell(_) => "Shell",
            FeatureKind::Hole(_) => "Hole",
            FeatureKind::PatternRect(_) => "Rectangular Pattern",
            FeatureKind::PatternCircular(_) => "Circular Pattern",
            FeatureKind::Mirror(_) => "Mirror",
            FeatureKind::WorkPlane(_) => "Work Plane",
            FeatureKind::WorkAxis(_) => "Work Axis",
            FeatureKind::WorkPoint(_) => "Work Point",
            FeatureKind::Ucs(_) => "UCS",
            FeatureKind::Rib(_) => "Rib",
            FeatureKind::Sweep(_) => "Sweep",
            FeatureKind::Coil(_) => "Coil",
            FeatureKind::Loft(_) => "Loft",
            FeatureKind::Draft(_) => "Draft",
            FeatureKind::Split(_) => "Split",
            FeatureKind::Combine(_) => "Combine",
            FeatureKind::Thread(_) => "Thread",
            FeatureKind::Emboss(_) => "Emboss",
            FeatureKind::Derive(_) => "Derive",
            FeatureKind::SheetFace(_) => "Face",
            FeatureKind::Flange(_) => "Flange",
            FeatureKind::Hem(_) => "Hem",
            FeatureKind::ContourFlange(_) => "Contour Flange",
            FeatureKind::CornerSeam(_) => "Corner Seam",
            FeatureKind::CornerRip(_) => "Corner Rip",
            FeatureKind::CutAcrossBend(_) | FeatureKind::CutNormal(_) => "Cut",
            FeatureKind::Bend(_) | FeatureKind::SharpBend(_) => "Bend",
            FeatureKind::Fold(_) => "Fold",
        }
    }
    /// Base of the default name of a new feature ("Extrusion" gives Extrusion1, Extrusion2, ...).
    pub fn default_name(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Sketch3d { .. } => "3D Sketch",
            FeatureKind::Extrude(_) => "Extrusion",
            FeatureKind::Revolve(_) => "Revolution",
            FeatureKind::Fillet(_) => "Fillet",
            FeatureKind::Chamfer(_) => "Chamfer",
            FeatureKind::Shell(_) => "Shell",
            FeatureKind::Hole(_) => "Hole",
            FeatureKind::PatternRect(_) => "Rectangular Pattern",
            FeatureKind::PatternCircular(_) => "Circular Pattern",
            FeatureKind::Mirror(_) => "Mirror",
            FeatureKind::WorkPlane(_) => "Work Plane",
            FeatureKind::WorkAxis(_) => "Work Axis",
            FeatureKind::WorkPoint(_) => "Work Point",
            FeatureKind::Ucs(_) => "UCS",
            FeatureKind::Rib(_) => "Rib",
            FeatureKind::Sweep(_) => "Sweep",
            FeatureKind::Coil(_) => "Coil",
            FeatureKind::Loft(_) => "Loft",
            FeatureKind::Draft(_) => "Draft",
            FeatureKind::Split(_) => "Split",
            FeatureKind::Combine(_) => "Combine",
            FeatureKind::Thread(_) => "Thread",
            FeatureKind::Emboss(_) => "Emboss",
            FeatureKind::Derive(_) => "Derive",
            FeatureKind::SheetFace(_) => "Face",
            FeatureKind::Flange(_) => "Flange",
            FeatureKind::Hem(_) => "Hem",
            FeatureKind::ContourFlange(_) => "Contour Flange",
            FeatureKind::CornerSeam(_) => "Corner Seam",
            FeatureKind::CornerRip(_) => "Corner Rip",
            FeatureKind::CutAcrossBend(_) | FeatureKind::CutNormal(_) => "Cut",
            FeatureKind::Bend(_) | FeatureKind::SharpBend(_) => "Bend",
            FeatureKind::Fold(_) => "Fold",
        }
    }
    /// Features this one depends on.
    pub fn depends_on(&self) -> Vec<FeatureId> {
        let mut v = match self {
            FeatureKind::Sketch { plane, projections, .. } => {
                plane.feature().into_iter().chain(projections.iter().flat_map(|p| p.edge.features())).collect()
            }
            FeatureKind::Sketch3d { links, .. } => links.iter().flat_map(|l| l.to.features()).collect(),
            FeatureKind::Extrude(e) => match &e.extent {
                ExtrudeExtent::To(plane) => std::iter::once(e.sketch).chain(plane.feature()).collect(),
                ExtrudeExtent::Between { from, to } => std::iter::once(e.sketch).chain(from.feature()).chain(to.feature()).collect(),
                _ => vec![e.sketch],
            },
            FeatureKind::Revolve(r) => {
                let mut v = vec![r.sketch];
                if let AxisRef::Work(a) | AxisRef::Ucs { feature: a, .. } = r.axis {
                    v.push(a);
                }
                if let RevolveAngle::To(plane) = &r.angle {
                    v.extend(plane.feature());
                }
                v
            }
            FeatureKind::Fillet(f) => {
                f.edges.iter().flat_map(EdgeRef::features).chain(f.faces.iter().flatten().flatten().filter_map(FaceRef::feature)).collect()
            }
            FeatureKind::Chamfer(c) => {
                let mut v: Vec<FeatureId> = c.edges.iter().flat_map(EdgeRef::features).collect();
                if let ChamferSize::TwoDistances { reference, .. } | ChamferSize::DistanceAngle { reference, .. } = &c.size {
                    v.extend(reference.feature());
                }
                v
            }
            FeatureKind::Shell(s) => s.remove.iter().filter_map(FaceRef::feature).collect(),
            FeatureKind::Hole(h) => match &h.extent {
                HoleExtent::To(plane) => std::iter::once(h.sketch).chain(plane.feature()).collect(),
                _ => vec![h.sketch],
            },
            FeatureKind::Rib(r) => vec![r.sketch],
            FeatureKind::Sweep(s) => vec![s.sketch, s.path.sketch],
            FeatureKind::Coil(c) => match c.axis {
                AxisRef::Work(a) | AxisRef::Ucs { feature: a, .. } => vec![c.sketch, a],
                _ => vec![c.sketch],
            },
            FeatureKind::Loft(l) => l.sections.clone(),
            FeatureKind::Draft(d) => d.faces.iter().filter_map(FaceRef::feature).chain(d.plane.feature()).collect(),
            FeatureKind::Split(s) => s.plane.feature().into_iter().chain(s.body.as_ref().and_then(FaceRef::feature)).collect(),
            FeatureKind::Combine(c) => std::iter::once(&c.base).chain(&c.tools).filter_map(FaceRef::feature).collect(),
            FeatureKind::Thread(t) => t.face.feature().into_iter().collect(),
            FeatureKind::Emboss(e) => std::iter::once(e.sketch).chain(e.wrap.as_ref().and_then(FaceRef::feature)).collect(),
            FeatureKind::Derive(d) => d.mirror.as_ref().and_then(PlaneRef::feature).into_iter().collect(),
            FeatureKind::SheetFace(f) => vec![f.sketch],
            FeatureKind::Flange(f) => f.edges.iter().flat_map(EdgeRef::features).collect(),
            FeatureKind::Hem(h) => h.edges.iter().flat_map(EdgeRef::features).collect(),
            FeatureKind::ContourFlange(c) => std::iter::once(c.sketch).chain(c.edges.iter().flat_map(EdgeRef::features)).collect(),
            FeatureKind::CornerSeam(c) => c.edges.iter().flat_map(EdgeRef::features).collect(),
            FeatureKind::CornerRip(c) => c.edges.iter().flat_map(EdgeRef::features).collect(),
            FeatureKind::CutAcrossBend(c) => vec![c.sketch],
            FeatureKind::CutNormal(c) => vec![c.sketch],
            FeatureKind::Bend(b) => b.edges.iter().flat_map(EdgeRef::features).collect(),
            FeatureKind::SharpBend(b) => b.edges.iter().flat_map(EdgeRef::features).collect(),
            FeatureKind::Fold(f) => vec![f.sketch],
            FeatureKind::PatternRect(p) => {
                let mut v = p.features.clone();
                for d in std::iter::once(&p.dir1).chain(p.dir2.as_ref()) {
                    match d {
                        DirectionRef::Origin(_) => {}
                        DirectionRef::Edge(e) => v.extend(e.features()),
                        DirectionRef::Work(id) | DirectionRef::Ucs { feature: id, .. } => v.push(*id),
                    }
                }
                v
            }
            FeatureKind::PatternCircular(p) => {
                let mut v = p.features.clone();
                v.extend(p.axis.features());
                v
            }
            FeatureKind::Mirror(m) => {
                let mut v = m.features.clone();
                v.extend(m.plane.feature());
                v
            }
            FeatureKind::WorkPlane(w) => match w {
                WorkPlane::Offset { base, .. } => base.feature().into_iter().collect(),
                WorkPlane::Angle { base, axis, .. } => base.feature().into_iter().chain(axis.features()).collect(),
                WorkPlane::Midplane { a, b } => a.feature().into_iter().chain(b.feature()).collect(),
                WorkPlane::ThreePoints { points } => points.iter().flat_map(PointSel::features).collect(),
                WorkPlane::SquareToCurve { sketch, .. } => vec![*sketch],
            },
            FeatureKind::WorkAxis(w) => match w {
                WorkAxis::Along { axis } => axis.features(),
                WorkAxis::Planes { a, b } => a.feature().into_iter().chain(b.feature()).collect(),
            },
            FeatureKind::WorkPoint(w) => match w {
                WorkPoint::Center { edge } => edge.features(),
                WorkPoint::Intersection { axis, plane } => axis.features().into_iter().chain(plane.feature()).collect(),
                WorkPoint::At { point } => point.features(),
            },
            FeatureKind::Ucs(u) => match u {
                Ucs::Offset { base, .. } => base.iter().copied().collect(),
                Ucs::ThreePoints { points } => points.iter().flat_map(PointSel::features).collect(),
            },
        };
        v.sort();
        v.dedup();
        v
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    #[serde(default)]
    pub suppressed: bool,
    /// Shown or hidden in the view by the user (DEC-106); None: as the rule has it (a sketch
    /// shows until a feature uses it, a work feature shows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    pub kind: FeatureKind,
}

/// Whether a part number or a description can be one: some text, on one line, of at most 200
/// characters.
pub fn prop_ok(text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > 200 || text.contains(['\n', '\r']) {
        Err("it is some text, on one line, of at most 200 characters".into())
    } else {
        Ok(())
    }
}

/// The longest name a file may give a document or anything in it, in bytes. Commands take 200 or
/// 256, and files are let a little more; a much longer one is no name, and takes more of the
/// graphics card to draw than it has.
pub const MAX_NAME_IN_FILE: usize = 1000;

/// Whether a name read from a file is one: see [`MAX_NAME_IN_FILE`].
pub fn name_in_file(what: &str, name: &str) -> Result<(), String> {
    text_in_file(&format!("the name of {what}"), name, MAX_NAME_IN_FILE)
}

/// Whether what a file says of `what` fits in `most` bytes, given what it takes to draw it.
pub fn text_in_file(what: &str, text: &str, most: usize) -> Result<(), String> {
    if text.len() > most {
        let start: String = text.chars().take(40).collect();
        return Err(format!("{what} (\"{start}...\") is {} bytes long, and may be at most {most}", text.len()));
    }
    Ok(())
}

/// Most features one document may hold (hostile-input cap).
pub const MAX_FEATURES: usize = 10_000;

/// A part: its features in history order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub name: String,
    /// The part's number, for parts lists. Without one, its file's name stands in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_number: Option<String>,
    /// What the part is, for parts lists. Without one, its name stands in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    features: Vec<Feature>,
    next_feature: u32,
    /// Parameter names and equations.
    #[serde(default)]
    pub(crate) params: crate::params::Parameters,
    /// The End of Part marker sits just before this feature: it and the features after it are
    /// rolled back (not computed). `None`: after the last feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end_before: Option<FeatureId>,
    /// What the part is made of: its density gives the part its mass, its colour the part's.
    /// Without one the part weighs a gram per cubic centimetre and has the standard colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material: Option<crate::materials::Material>,
    /// A colour of the part's own ("#rrggbb"), shown in place of its material's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    appearance: Option<String>,
    /// The part's sizes, when it comes in several (see `table.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) table: Option<crate::table::DesignTable>,
    /// The sheet metal rule, when the part is a sheet metal part (see `sheet.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sheet: Option<Box<crate::sheet::SheetMetal>>,
    /// Stress studies of the part: where it is held and what pushes on it (see `study.rs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) studies: Vec<crate::study::Study>,
    /// The part as its mirror image in its own XY plane: how an assembly's mirrored occurrence
    /// has it (DEC-130). Worked out from the part file, which never says it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    reflected: bool,
    /// What has been read of the part files its features derive from: never in a file.
    #[serde(skip)]
    links: std::sync::Arc<Links>,
}

impl Default for Document {
    fn default() -> Self {
        Document {
            name: "Part1".into(),
            part_number: None,
            description: None,
            features: Vec::new(),
            next_feature: 0,
            params: Default::default(),
            end_before: None,
            material: None,
            appearance: None,
            table: None,
            sheet: None,
            studies: Vec::new(),
            reflected: false,
            links: Default::default(),
        }
    }
}

impl Document {
    /// Whether the part is built as its mirror image in its own XY plane (DEC-130).
    pub fn reflected(&self) -> bool {
        self.reflected
    }

    /// Builds the part as its mirror image in its own XY plane, or as itself again.
    pub fn set_reflected(&mut self, on: bool) {
        self.reflected = on;
    }
}

/// What deleting a feature takes with it (see [`Document::deletion`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Deletion {
    /// The features that use it, and those that use them in turn, in the tree's order. A
    /// feature cannot be deleted without them.
    pub dependents: Vec<FeatureId>,
    /// The sketches it and its dependents are made from that nothing else uses, in the tree's
    /// order: Inventor deletes them with it unless told not to.
    pub consumed: Vec<FeatureId>,
}

impl Document {
    pub fn new(name: impl Into<String>) -> Self {
        Document { name: name.into(), ..Default::default() }
    }
    /// What the part is made of, if that has been said.
    pub fn material(&self) -> Option<&crate::materials::Material> {
        self.material.as_ref()
    }
    /// How the part's material answers to a load, when it has a material that says.
    pub fn strength(&self) -> Option<crate::materials::Strength> {
        self.material.as_ref().and_then(crate::materials::Material::strength)
    }
    /// The part's stress studies.
    pub fn studies(&self) -> &[crate::study::Study] {
        &self.studies
    }
    pub fn study(&self, id: crate::study::StudyId) -> Option<&crate::study::Study> {
        self.studies.iter().find(|s| s.id == id)
    }
    pub(crate) fn study_mut(&mut self, id: crate::study::StudyId) -> Option<&mut crate::study::Study> {
        self.studies.iter_mut().find(|s| s.id == id)
    }
    /// A new, empty study, named as said or "Study1", "Study2"...
    pub(crate) fn add_study(&mut self, name: Option<&str>) -> Result<crate::study::StudyId, String> {
        if self.studies.len() >= crate::study::MAX_STUDIES {
            return Err(format!("a part has at most {} studies", crate::study::MAX_STUDIES));
        }
        let id = crate::study::StudyId(self.studies.iter().map(|s| s.id.0).max().map_or(1, |m| m.saturating_add(1)));
        let name = match name {
            Some(n) => n.trim().to_owned(),
            None => (1..).map(|i| format!("Study{i}")).find(|n| !self.studies.iter().any(|s| &s.name == n)).unwrap_or_default(),
        };
        let study = crate::study::Study::new(id, name);
        study.check()?;
        self.studies.push(study);
        Ok(id)
    }
    /// The study `id` again under another name ("Name copy", "Name copy 2" ... unless one is
    /// said). Returns the copy's id.
    pub(crate) fn copy_study(&mut self, id: crate::study::StudyId, name: Option<&str>) -> Result<crate::study::StudyId, String> {
        if self.studies.len() >= crate::study::MAX_STUDIES {
            return Err(format!("a part has at most {} studies", crate::study::MAX_STUDIES));
        }
        let Some(from) = self.study(id) else { return Err(format!("there is no study {}", id.0)) };
        let taken = |n: &str| self.studies.iter().any(|s| s.name == n);
        let name = match name {
            Some(n) => n.trim().to_owned(),
            None => {
                let base: String = from.name.chars().take(56).collect();
                std::iter::once(format!("{base} copy")).chain((2..).map(|i| format!("{base} copy {i}"))).find(|n| !taken(n)).unwrap_or_default()
            }
        };
        let new = crate::study::StudyId(self.studies.iter().map(|s| s.id.0).max().map_or(1, |m| m.saturating_add(1)));
        let copy = from.copied(new, name);
        copy.check()?;
        self.studies.push(copy);
        Ok(new)
    }

    /// Takes a study away. False when there is none of that id.
    pub(crate) fn remove_study(&mut self, id: crate::study::StudyId) -> bool {
        let before = self.studies.len();
        self.studies.retain(|s| s.id != id);
        self.studies.len() < before
    }
    /// Gives the part a material, or with `None` takes it away.
    pub fn set_material(&mut self, material: Option<crate::materials::Material>) -> Result<(), String> {
        if let Some(m) = &material {
            m.check()?;
        }
        self.material = material.map(|m| crate::materials::Material { name: m.name.trim().to_owned(), color: m.color.trim().to_lowercase(), ..m });
        Ok(())
    }
    /// The part's own colour ("#rrggbb"), when it has one in place of its material's.
    pub fn appearance(&self) -> Option<&str> {
        self.appearance.as_deref()
    }
    /// Gives the part a colour of its own, or with `None` returns it to its material's.
    pub fn set_appearance(&mut self, color: Option<&str>) -> Result<(), String> {
        match color {
            Some(c) => {
                let rgb = crate::materials::parse_color(c).ok_or_else(|| format!("`{c}` is not a colour: write it as #rrggbb"))?;
                self.appearance = Some(crate::materials::color_text(rgb));
            }
            None => self.appearance = None,
        }
        Ok(())
    }
    /// The part's density in grams per cubic centimetre: its material's, else water's.
    pub fn density(&self) -> f64 {
        self.material.as_ref().map_or(crate::materials::DEFAULT_DENSITY, |m| m.density)
    }
    /// The colour the part is shown in, when it has one: its own, else its material's.
    pub fn color(&self) -> Option<[u8; 3]> {
        self.appearance.as_deref().or(self.material.as_ref().map(|m| m.color.as_str())).and_then(crate::materials::parse_color)
    }
    /// Measures the driven dimensions of every sketch again.
    pub(crate) fn refresh_driven(&mut self) {
        for f in &mut self.features {
            if let FeatureKind::Sketch { sketch, .. } = &mut f.kind {
                sketch.refresh_driven();
            }
        }
    }
    pub fn features(&self) -> &[Feature] {
        &self.features
    }
    pub fn feature(&self, id: FeatureId) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }
    pub fn feature_mut(&mut self, id: FeatureId) -> Option<&mut Feature> {
        self.features.iter_mut().find(|f| f.id == id)
    }
    pub fn index_of(&self, id: FeatureId) -> Option<usize> {
        self.features.iter().position(|f| f.id == id)
    }
    pub fn sketch(&self, id: FeatureId) -> Option<&Sketch> {
        match &self.feature(id)?.kind {
            FeatureKind::Sketch { sketch, .. } => Some(sketch),
            _ => None,
        }
    }
    pub fn sketch_mut(&mut self, id: FeatureId) -> Option<&mut Sketch> {
        match &mut self.feature_mut(id)?.kind {
            FeatureKind::Sketch { sketch, .. } => Some(sketch),
            _ => None,
        }
    }
    /// The 3D sketch of feature `id` (DEC-136).
    pub fn sketch3d(&self, id: FeatureId) -> Option<&tenon_sketch::Sketch3d> {
        match &self.feature(id)?.kind {
            FeatureKind::Sketch3d { sketch, .. } => Some(sketch),
            _ => None,
        }
    }
    pub fn sketch3d_mut(&mut self, id: FeatureId) -> Option<&mut tenon_sketch::Sketch3d> {
        match &mut self.feature_mut(id)?.kind {
            FeatureKind::Sketch3d { sketch, .. } => Some(sketch),
            _ => None,
        }
    }
    /// What the reference geometry of 3D sketch `id` stands for.
    pub fn links3(&self, id: FeatureId) -> &[crate::sketch3d::Link3] {
        match self.feature(id).map(|f| &f.kind) {
            Some(FeatureKind::Sketch3d { links, .. }) => links,
            _ => &[],
        }
    }
    /// Sketch `id` can be a sweep's path: a sketch or a 3D sketch.
    pub fn has_curves(&self, id: FeatureId) -> bool {
        self.sketch(id).is_some() || self.sketch3d(id).is_some()
    }

    /// The curves that the sweeps running along sketch `id` name.
    pub fn sweep_paths_mut(&mut self, id: FeatureId) -> impl Iterator<Item = &mut Vec<EntityId>> {
        self.features.iter_mut().filter_map(move |f| match &mut f.kind {
            FeatureKind::Sweep(s) if s.path.sketch == id => Some(&mut s.path.curves),
            _ => None,
        })
    }

    /// The curve and the end of it that each work plane square to a curve of 3D sketch `id` names.
    pub fn square_planes_mut(&mut self, id: FeatureId) -> impl Iterator<Item = (EntityId, &mut EntityId)> {
        self.features.iter_mut().filter_map(move |f| match &mut f.kind {
            FeatureKind::WorkPlane(WorkPlane::SquareToCurve { sketch, curve, end }) if *sketch == id => Some((*curve, end)),
            _ => None,
        })
    }

    /// Refused, in words, when a feature would take its profile from what is no sketch: as the
    /// file is held to it when read ([`Document::validate`]), so said before it is saved.
    fn check_profile_sketches(&self, kind: &FeatureKind) -> Result<(), String> {
        let (profiles, path): (Vec<FeatureId>, Option<FeatureId>) = match kind {
            FeatureKind::Extrude(e) => (vec![e.sketch], None),
            FeatureKind::Revolve(r) => (vec![r.sketch], None),
            FeatureKind::Hole(h) => (vec![h.sketch], None),
            FeatureKind::Rib(r) => (vec![r.sketch], None),
            FeatureKind::Sweep(s) => (vec![s.sketch], Some(s.path.sketch)),
            FeatureKind::Coil(c) => (vec![c.sketch], None),
            FeatureKind::Loft(l) => (l.sections.clone(), None),
            FeatureKind::SheetFace(s) => (vec![s.sketch], None),
            FeatureKind::ContourFlange(c) => (vec![c.sketch], None),
            FeatureKind::CutAcrossBend(c) => (vec![c.sketch], None),
            FeatureKind::CutNormal(c) => (vec![c.sketch], None),
            FeatureKind::Fold(f) => (vec![f.sketch], None),
            _ => (Vec::new(), None),
        };
        let name = |id: FeatureId| self.feature(id).map_or_else(|| id.to_string(), |f| f.name.clone());
        for id in profiles {
            if self.sketch3d(id).is_some() {
                return Err(format!("{} is a 3D sketch, which is on no plane: draw this in a sketch on a plane.", name(id)));
            }
            if self.sketch(id).is_none() {
                return Err(format!("{} is not a sketch: give the sketch that has this in it.", name(id)));
            }
        }
        if let Some(id) = path.filter(|id| !self.has_curves(*id)) {
            return Err(format!("{} is not a sketch: a path is drawn in a sketch or a 3D sketch.", name(id)));
        }
        Ok(())
    }

    /// The edges of the part shown in sketch `id` as reference geometry.
    pub fn projections(&self, id: FeatureId) -> &[crate::project::Projection] {
        match self.feature(id).map(|f| &f.kind) {
            Some(FeatureKind::Sketch { projections, .. }) => projections,
            _ => &[],
        }
    }
    pub fn projections_mut(&mut self, id: FeatureId) -> Option<&mut Vec<crate::project::Projection>> {
        match &mut self.feature_mut(id)?.kind {
            FeatureKind::Sketch { projections, .. } => Some(projections),
            _ => None,
        }
    }
    /// Forgets projections whose geometry is gone from its sketch or is the sketch's own again
    /// (deleted, or its link broken).
    pub fn tidy_projections(&mut self) {
        for f in &mut self.features {
            match &mut f.kind {
                FeatureKind::Sketch { sketch, projections, .. } => projections.retain(|p| sketch.is_reference(p.entity)),
                FeatureKind::Sketch3d { sketch, links } => links.retain(|l| sketch.is_reference(l.entity)),
                _ => {}
            }
        }
    }

    /// Appends a feature with a generated name ("Extrusion2"). Returns its id.
    pub fn add(&mut self, kind: FeatureKind) -> Result<FeatureId, String> {
        if self.features.len() >= MAX_FEATURES {
            return Err("the document is full".into());
        }
        for dep in kind.depends_on() {
            if self.feature(dep).is_none() {
                return Err(format!("{dep} does not exist"));
            }
        }
        self.check_profile_sketches(&kind)?;
        self.next_feature = self.next_feature.checked_add(1).ok_or("feature ids exhausted")?;
        let id = FeatureId(self.next_feature);
        let base = kind.default_name();
        let n = (1..).find(|n| !self.features.iter().any(|f| f.name == format!("{base}{n}"))).unwrap_or(1);
        // New features go in just above the End of Part marker.
        let at = self.end_of_part();
        self.features.insert(at, Feature { id, name: format!("{base}{n}"), suppressed: false, visible: None, kind });
        Ok(id)
    }

    /// The document as it was before feature `id` (that feature and everything after it left out):
    /// what a feature is edited against.
    pub fn rolled_back_to(&self, id: FeatureId) -> Document {
        let n = self.index_of(id).unwrap_or(self.features.len());
        Document {
            name: self.name.clone(),
            part_number: self.part_number.clone(),
            description: self.description.clone(),
            features: self.features[..n].to_vec(),
            next_feature: self.next_feature,
            params: self.params.clone(),
            end_before: None,
            material: self.material.clone(),
            appearance: self.appearance.clone(),
            table: self.table.clone(),
            sheet: self.sheet.clone(),
            // (A part as it was part way up its history is looked at, not studied.)
            studies: Vec::new(),
            reflected: self.reflected,
            links: self.links.clone(),
        }
    }

    /// What has been read of the part files its features derive from.
    pub fn links(&self) -> &Links {
        &self.links
    }

    /// Gives it what has been read of them.
    pub fn set_links(&mut self, links: std::sync::Arc<Links>) {
        self.links = links;
    }

    /// The paths its derived features hold, each changed as `f` says (to a full path as a file is
    /// read, to one relative to the file as it is written).
    pub fn map_derived(&mut self, f: impl Fn(&str) -> String) {
        for feature in &mut self.features {
            if let FeatureKind::Derive(d) = &mut feature.kind {
                d.path = f(&d.path);
            }
        }
    }

    /// The part files its derived features name.
    pub fn derived_paths(&self) -> Vec<String> {
        let mut out: Vec<String> =
            self.features.iter().filter_map(|f| if let FeatureKind::Derive(d) = &f.kind { Some(d.path.clone()) } else { None }).collect();
        out.sort();
        out.dedup();
        out
    }

    /// Removes a feature that nothing depends on.
    /// What deleting `id` would take with it, as Inventor's Delete Features box offers it
    /// (DEC-103).
    pub fn deletion(&self, id: FeatureId) -> Deletion {
        // What uses it, and what uses that, in turn.
        let mut gone: std::collections::BTreeSet<FeatureId> = std::iter::once(id).collect();
        loop {
            let more: Vec<FeatureId> = self
                .features
                .iter()
                .filter(|f| !gone.contains(&f.id) && f.kind.depends_on().iter().any(|d| gone.contains(d)))
                .map(|f| f.id)
                .collect();
            if more.is_empty() {
                break;
            }
            gone.extend(more);
        }
        let dependents = self.features.iter().filter(|f| f.id != id && gone.contains(&f.id)).map(|f| f.id).collect();
        // The sketches those are made from, that nothing else uses.
        let consumed = self
            .features
            .iter()
            .filter(|s| !gone.contains(&s.id) && matches!(s.kind, FeatureKind::Sketch { .. } | FeatureKind::Sketch3d { .. }))
            .filter(|s| {
                let mut users = self.features.iter().filter(|u| u.kind.depends_on().contains(&s.id)).peekable();
                users.peek().is_some() && users.all(|u| gone.contains(&u.id))
            })
            .map(|s| s.id)
            .collect();
        Deletion { dependents, consumed }
    }

    /// Deletes features, the later first (what uses a feature comes after it). Refused, with
    /// nothing deleted, when one is used by a feature that stays.
    pub fn remove_all(&mut self, ids: &[FeatureId]) -> Result<Vec<Feature>, String> {
        let mut order: Vec<(usize, FeatureId)> =
            ids.iter().map(|id| self.index_of(*id).map(|i| (i, *id)).ok_or_else(|| format!("{id} does not exist"))).collect::<Result<_, _>>()?;
        order.sort_by_key(|o| std::cmp::Reverse(o.0));
        order.dedup();
        let before = self.clone();
        let mut out = Vec::new();
        for (_, id) in order {
            match self.remove(id) {
                Ok(f) => out.push(f),
                Err(e) => {
                    *self = before;
                    return Err(e);
                }
            }
        }
        out.reverse();
        Ok(out)
    }

    pub fn remove(&mut self, id: FeatureId) -> Result<Feature, String> {
        let users: Vec<&str> = self.features.iter().filter(|f| f.kind.depends_on().contains(&id)).map(|f| f.name.as_str()).collect();
        if !users.is_empty() {
            return Err(format!("{} is used by {}", self.feature(id).map_or("?", |f| f.name.as_str()), users.join(", ")));
        }
        let i = self.index_of(id).ok_or_else(|| format!("{id} does not exist"))?;
        if self.end_before == Some(id) {
            self.end_before = self.features.get(i + 1).map(|f| f.id);
        }
        Ok(self.features.remove(i))
    }

    /// How many features are computed: the index the End of Part marker sits at.
    pub fn end_of_part(&self) -> usize {
        self.end_before.and_then(|id| self.index_of(id)).unwrap_or(self.features.len())
    }

    /// The feature the End of Part marker sits just before, if it is not at the end.
    pub fn end_before(&self) -> Option<FeatureId> {
        self.end_before
    }

    /// Moves the End of Part marker to just before `before` (`None`: after the last feature).
    pub fn set_end_of_part(&mut self, before: Option<FeatureId>) -> Result<(), String> {
        if let Some(id) = before
            && self.feature(id).is_none()
        {
            return Err(format!("{id} does not exist"));
        }
        self.end_before = before;
        Ok(())
    }

    /// Moves a feature to just before `before` (`None`: to the end, above the End of Part marker
    /// if it is not there). Sketches only it uses go with it. Refused when a feature would come
    /// before one it uses.
    pub fn move_feature(&mut self, id: FeatureId, before: Option<FeatureId>) -> Result<(), String> {
        if before == Some(id) {
            return Ok(());
        }
        let name = self.feature(id).ok_or_else(|| format!("{id} does not exist"))?.name.clone();
        // The block: the sketches only this feature uses, then the feature.
        let own: Vec<FeatureId> = self
            .feature(id)
            .map(|f| f.kind.depends_on())
            .unwrap_or_default()
            .into_iter()
            .filter(|d| {
                matches!(self.feature(*d).map(|f| &f.kind), Some(FeatureKind::Sketch { .. } | FeatureKind::Sketch3d { .. }))
                    && self.features.iter().filter(|f| f.kind.depends_on().contains(d)).count() == 1
            })
            .collect();
        if before.is_some_and(|b| own.contains(&b)) {
            return Ok(());
        }
        let saved = (self.features.clone(), self.end_before);
        let mut block: Vec<Feature> = Vec::new();
        self.features.retain(|f| {
            if f.id == id || own.contains(&f.id) {
                block.push(f.clone());
                false
            } else {
                true
            }
        });
        // A marker on a moved feature stays where it was: on the next one that stays.
        if self.end_before.is_some_and(|e| e == id || own.contains(&e)) {
            let i = saved.0.iter().position(|f| Some(f.id) == self.end_before).unwrap_or(0);
            self.end_before = saved.0[i..].iter().map(|f| f.id).find(|f| *f != id && !own.contains(f));
        }
        let at = match before {
            Some(b) => self.index_of(b).ok_or_else(|| format!("{b} does not exist"))?,
            None => self.end_of_part(),
        };
        for (k, f) in block.into_iter().enumerate() {
            self.features.insert(at + k, f);
        }
        if let Err(e) = self.validate() {
            (self.features, self.end_before) = saved;
            return Err(format!("{name} cannot go there: {e}"));
        }
        Ok(())
    }

    /// A pattern or mirror copies only features that add or remove material.
    pub fn check_copies(&self, kind: &FeatureKind) -> Result<(), String> {
        for id in kind.copies() {
            match self.feature(*id) {
                Some(f) if f.kind.has_tool() => {}
                Some(f) => return Err(format!("{} cannot be copied: only features that add or remove material can", f.name)),
                None => return Err(format!("{id} does not exist")),
            }
        }
        Ok(())
    }

    /// Whether the sizes of one feature can make it, said with its name: what a new or changed
    /// feature is held to at once.
    pub fn sizes_of(&self, id: FeatureId) -> Result<(), String> {
        let Some(f) = self.feature(id) else { return Ok(()) };
        f.kind.check_sizes().map_err(|e| format!("{}: {e}", f.name))
    }

    /// Checks references, words and every sketch, e.g. after loading a file. A feature's sizes are
    /// not asked here ([`FeatureKind::check_sizes`]): a parameter can take one past what it can be,
    /// and a file saved like that opens, with the feature in error, to be put right.
    pub fn validate(&self) -> Result<(), String> {
        if self.features.len() > MAX_FEATURES {
            return Err("too many features".into());
        }
        name_in_file("the part", &self.name)?;
        for f in &self.features {
            name_in_file("a feature", &f.name)?;
            let designation = match &f.kind {
                FeatureKind::Thread(t) => t.designation.as_ref(),
                FeatureKind::Hole(h) => h.tapped.as_ref().and_then(|t| t.designation.as_ref()),
                _ => None,
            };
            if let Some(d) = designation {
                text_in_file(&format!("the thread of {}", f.name), d, MAX_NAME_IN_FILE)?;
            }
        }
        self.params_check()?;
        for (what, said) in [("part number", &self.part_number), ("description", &self.description)] {
            if let Some(text) = said {
                prop_ok(text).map_err(|e| format!("the {what}: {e}"))?;
            }
        }
        if let Some(e) = self.end_before
            && self.feature(e).is_none()
        {
            return Err(format!("the End of Part marker is before {e}, which does not exist"));
        }
        let mut seen = std::collections::BTreeMap::new();
        for (i, f) in self.features.iter().enumerate() {
            if f.id.0 == 0 || f.id.0 > self.next_feature {
                return Err(format!("{} has id {}, outside 1 to {} (next_feature)", f.name, f.id.0, self.next_feature));
            }
            // Two branches that each added a feature, merged, give both the same id.
            if let Some(first) = seen.insert(f.id, &f.name) {
                return Err(format!("two features have id {}: {first} and {}", f.id.0, f.name));
            }
            for dep in f.kind.depends_on() {
                match self.index_of(dep) {
                    Some(j) if j < i => {}
                    _ => return Err(format!("{} refers to {dep}, which does not come before it", f.name)),
                }
            }
            match &f.kind {
                FeatureKind::Sketch { sketch, projections, .. } => {
                    sketch.validate().map_err(|e| format!("{}: {e}", f.name))?;
                    let mut shown = std::collections::BTreeSet::new();
                    for p in projections {
                        if !sketch.is_reference(p.entity) {
                            return Err(format!("{}: {} is said to be a projected edge but is not reference geometry", f.name, p.entity));
                        }
                        if !shown.insert(p.entity) {
                            return Err(format!("{}: {} is projected from two edges", f.name, p.entity));
                        }
                    }
                }
                FeatureKind::Sketch3d { sketch, links } => {
                    sketch.validate().map_err(|e| format!("{}: {e}", f.name))?;
                    let mut linked = std::collections::BTreeSet::new();
                    for l in links {
                        if !sketch.is_reference(l.entity) {
                            return Err(format!("{}: {} is said to follow the part but is not reference geometry", f.name, l.entity));
                        }
                        if !linked.insert(l.entity) {
                            return Err(format!("{}: {} is said to follow two things of the part", f.name, l.entity));
                        }
                    }
                }
                FeatureKind::Extrude(e) if self.sketch(e.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, e.sketch)),
                FeatureKind::Revolve(r) if self.sketch(r.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, r.sketch)),
                // (Points that went missing are a regeneration error, as sizes are.)
                FeatureKind::Hole(h) if self.sketch(h.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, h.sketch)),
                FeatureKind::Rib(r) if self.sketch(r.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, r.sketch)),
                FeatureKind::Sweep(s) if self.sketch(s.sketch).is_none() || !self.has_curves(s.path.sketch) => {
                    return Err(format!("{}: the profile and the path must be sketches", f.name));
                }
                FeatureKind::Coil(c) if self.sketch(c.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, c.sketch)),
                FeatureKind::Loft(l) if l.sections.iter().any(|s| self.sketch(*s).is_none()) => {
                    return Err(format!("{}: every loft section must be a sketch", f.name));
                }
                FeatureKind::Emboss(e) if self.sketch(e.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, e.sketch)),
                FeatureKind::Derive(d) => d.check_path().map_err(|e| format!("{}: {e}", f.name))?,
                FeatureKind::SheetFace(s) if self.sketch(s.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, s.sketch)),
                FeatureKind::ContourFlange(c) if self.sketch(c.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, c.sketch)),
                FeatureKind::CutAcrossBend(c) if self.sketch(c.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, c.sketch)),
                FeatureKind::CutNormal(c) if self.sketch(c.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, c.sketch)),
                FeatureKind::Fold(fo) if self.sketch(fo.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, fo.sketch)),
                _ => {}
            }
            self.check_copies(&f.kind).map_err(|e| format!("{}: {e}", f.name))?;
        }
        if let Some(m) = &self.material {
            m.check()?;
        }
        if self.studies.len() > crate::study::MAX_STUDIES {
            return Err(format!("a part has at most {} studies", crate::study::MAX_STUDIES));
        }
        for (i, s) in self.studies.iter().enumerate() {
            s.check()?;
            if self.studies.iter().take(i).any(|o| o.id == s.id) {
                return Err(format!("two studies have the id {}", s.id.0));
            }
        }
        if self.appearance.as_deref().is_some_and(|c| crate::materials::parse_color(c).is_none()) {
            return Err("the part's appearance is not a colour: write it as #rrggbb".into());
        }
        self.table_check()
    }
}
