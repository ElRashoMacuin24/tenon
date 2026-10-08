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
}

impl PlaneRef {
    /// The feature this plane comes from, if any.
    pub fn feature(&self) -> Option<FeatureId> {
        match self {
            PlaneRef::Origin(_) => None,
            PlaneRef::Face(f) => f.feature(),
            PlaneRef::Work(id) => Some(*id),
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
    /// Regions by key (the sorted ids of their outer boundary curves).
    Keys(Vec<Vec<EntityId>>),
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisRef {
    /// A line of the profile's own sketch.
    SketchLine(EntityId),
    Origin(OriginAxis),
    /// A work axis feature.
    Work(FeatureId),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevolveAngle {
    Full,
    Angle(f64),
    Symmetric(f64),
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
}

/// Rounds edges with one radius.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fillet {
    pub edges: Vec<EdgeRef>,
    pub radius: f64,
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleExtent {
    /// Depth of the full-diameter part, from the sketch plane (the drill point is extra).
    Distance(f64),
    ThroughAll,
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
}

/// The points of a sketch a hole goes at by default: centre points, i.e. points that are not
/// construction and not on any curve.
pub fn hole_centres(sk: &Sketch) -> Vec<EntityId> {
    sk.entities().filter(|(id, e)| !e.construction && sk.point(*id).is_some() && sk.curves_at(*id).is_empty()).map(|(id, _)| id).collect()
}

/// The usual drill point angle, 118 degrees.
pub const DRILL_POINT: f64 = 118.0 * std::f64::consts::PI / 180.0;

impl Hole {
    /// Why the hole's sizes cannot make a hole, if they cannot.
    pub fn check(&self) -> Result<(), String> {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if !positive(self.diameter) {
            return Err("the hole diameter must be positive".into());
        }
        let depth = match self.extent {
            HoleExtent::Distance(d) if !positive(d) => return Err("the hole depth must be positive".into()),
            HoleExtent::Distance(d) => Some(d),
            HoleExtent::ThroughAll => None,
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

/// A direction for a pattern: an origin axis or a straight edge of the part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectionRef {
    Origin(OriginAxis),
    Edge(EdgeRef),
    /// Along a work axis.
    Work(FeatureId),
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
}

impl AxisSel {
    /// The features this axis comes from.
    pub fn features(&self) -> Vec<FeatureId> {
        match self {
            AxisSel::Origin(_) => vec![],
            AxisSel::Edge(e) => e.features(),
            AxisSel::Face(f) => f.feature().into_iter().collect(),
            AxisSel::Work(id) => vec![*id],
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
    Sketch { plane: PlaneRef, sketch: Sketch },
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
}

impl FeatureKind {
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
        }
    }
    /// Base of the default name of a new feature ("Extrusion" gives Extrusion1, Extrusion2, ...).
    pub fn default_name(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
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
        }
    }
    /// Features this one depends on.
    pub fn depends_on(&self) -> Vec<FeatureId> {
        let mut v = match self {
            FeatureKind::Sketch { plane, .. } => plane.feature().into_iter().collect(),
            FeatureKind::Extrude(e) => vec![e.sketch],
            FeatureKind::Revolve(r) => match r.axis {
                AxisRef::Work(a) => vec![r.sketch, a],
                _ => vec![r.sketch],
            },
            FeatureKind::Fillet(f) => f.edges.iter().flat_map(EdgeRef::features).collect(),
            FeatureKind::Chamfer(c) => {
                let mut v: Vec<FeatureId> = c.edges.iter().flat_map(EdgeRef::features).collect();
                if let ChamferSize::TwoDistances { reference, .. } | ChamferSize::DistanceAngle { reference, .. } = &c.size {
                    v.extend(reference.feature());
                }
                v
            }
            FeatureKind::Shell(s) => s.remove.iter().filter_map(FaceRef::feature).collect(),
            FeatureKind::Hole(h) => vec![h.sketch],
            FeatureKind::PatternRect(p) => {
                let mut v = p.features.clone();
                for d in std::iter::once(&p.dir1).chain(p.dir2.as_ref()) {
                    match d {
                        DirectionRef::Origin(_) => {}
                        DirectionRef::Edge(e) => v.extend(e.features()),
                        DirectionRef::Work(id) => v.push(*id),
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
            },
            FeatureKind::WorkAxis(w) => match w {
                WorkAxis::Along { axis } => axis.features(),
                WorkAxis::Planes { a, b } => a.feature().into_iter().chain(b.feature()).collect(),
            },
            FeatureKind::WorkPoint(w) => match w {
                WorkPoint::Center { edge } => edge.features(),
                WorkPoint::Intersection { axis, plane } => axis.features().into_iter().chain(plane.feature()).collect(),
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
    pub kind: FeatureKind,
}

/// Most features one document may hold (hostile-input cap).
pub const MAX_FEATURES: usize = 10_000;

/// A part: its features in history order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub name: String,
    features: Vec<Feature>,
    next_feature: u32,
    /// Parameter names and equations.
    #[serde(default)]
    pub(crate) params: crate::params::Parameters,
}

impl Default for Document {
    fn default() -> Self {
        Document { name: "Part1".into(), features: Vec::new(), next_feature: 0, params: Default::default() }
    }
}

impl Document {
    pub fn new(name: impl Into<String>) -> Self {
        Document { name: name.into(), ..Default::default() }
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
        self.next_feature = self.next_feature.checked_add(1).ok_or("feature ids exhausted")?;
        let id = FeatureId(self.next_feature);
        let base = kind.default_name();
        let n = (1..).find(|n| !self.features.iter().any(|f| f.name == format!("{base}{n}"))).unwrap_or(1);
        self.features.push(Feature { id, name: format!("{base}{n}"), suppressed: false, kind });
        Ok(id)
    }

    /// The document as it was before feature `id` (that feature and everything after it left out):
    /// what a feature is edited against.
    pub fn rolled_back_to(&self, id: FeatureId) -> Document {
        let n = self.index_of(id).unwrap_or(self.features.len());
        Document { name: self.name.clone(), features: self.features[..n].to_vec(), next_feature: self.next_feature, params: self.params.clone() }
    }

    /// Removes a feature that nothing depends on.
    pub fn remove(&mut self, id: FeatureId) -> Result<Feature, String> {
        let users: Vec<&str> = self.features.iter().filter(|f| f.kind.depends_on().contains(&id)).map(|f| f.name.as_str()).collect();
        if !users.is_empty() {
            return Err(format!("{} is used by {}", self.feature(id).map_or("?", |f| f.name.as_str()), users.join(", ")));
        }
        let i = self.index_of(id).ok_or_else(|| format!("{id} does not exist"))?;
        Ok(self.features.remove(i))
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

    /// Checks references and every sketch, e.g. after loading a file.
    pub fn validate(&self) -> Result<(), String> {
        if self.features.len() > MAX_FEATURES {
            return Err("too many features".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for (i, f) in self.features.iter().enumerate() {
            if f.id.0 > self.next_feature || !seen.insert(f.id) {
                return Err(format!("{} has an invalid id", f.name));
            }
            for dep in f.kind.depends_on() {
                match self.index_of(dep) {
                    Some(j) if j < i => {}
                    _ => return Err(format!("{} refers to {dep}, which does not come before it", f.name)),
                }
            }
            match &f.kind {
                FeatureKind::Sketch { sketch, .. } => sketch.validate().map_err(|e| format!("{}: {e}", f.name))?,
                FeatureKind::Extrude(e) if self.sketch(e.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, e.sketch)),
                FeatureKind::Revolve(r) if self.sketch(r.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, r.sketch)),
                FeatureKind::Hole(h) if self.sketch(h.sketch).is_none() => return Err(format!("{}: {} is not a sketch", f.name, h.sketch)),
                // Sizes are checked here too: a file must not hold a hole that cannot be built
                // without saying so. (Points that went missing are a regeneration error instead.)
                FeatureKind::Hole(h) => h.check().map_err(|e| format!("{}: {e}", f.name))?,
                FeatureKind::PatternRect(p) => p.check().map_err(|e| format!("{}: {e}", f.name))?,
                FeatureKind::PatternCircular(p) => p.check().map_err(|e| format!("{}: {e}", f.name))?,
                FeatureKind::Mirror(m) => check_sources(&m.features).map_err(|e| format!("{}: {e}", f.name))?,
                _ => {}
            }
            self.check_copies(&f.kind).map_err(|e| format!("{}: {e}", f.name))?;
        }
        Ok(())
    }
}
