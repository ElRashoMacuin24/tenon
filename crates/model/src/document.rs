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
        }
    }
    /// Features this one depends on.
    pub fn depends_on(&self) -> Vec<FeatureId> {
        let mut v = match self {
            FeatureKind::Sketch { plane: PlaneRef::Face(f), .. } => f.feature().into_iter().collect(),
            FeatureKind::Sketch { .. } => vec![],
            FeatureKind::Extrude(e) => vec![e.sketch],
            FeatureKind::Revolve(r) => vec![r.sketch],
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
}

impl Default for Document {
    fn default() -> Self {
        Document { name: "Part1".into(), features: Vec::new(), next_feature: 0 }
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
        Document { name: self.name.clone(), features: self.features[..n].to_vec(), next_feature: self.next_feature }
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
                _ => {}
            }
        }
        Ok(())
    }
}
