//! The part document: an ordered feature history.

use serde::{Deserialize, Serialize};
use tenon_geom::{Axis, Frame, Vec3};
use tenon_sketch::{EntityId, Sketch};

use crate::naming::FaceRef;

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeatureKind {
    Sketch { plane: PlaneRef, sketch: Sketch },
    Extrude(Extrude),
    Revolve(Revolve),
}

impl FeatureKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude(_) => "Extrude",
            FeatureKind::Revolve(_) => "Revolve",
        }
    }
    /// Base of the default name of a new feature ("Extrusion" gives Extrusion1, Extrusion2, ...).
    pub fn default_name(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude(_) => "Extrusion",
            FeatureKind::Revolve(_) => "Revolution",
        }
    }
    /// Features this one depends on.
    pub fn depends_on(&self) -> Vec<FeatureId> {
        match self {
            FeatureKind::Sketch { plane: PlaneRef::Face(f), .. } => f.feature().into_iter().collect(),
            FeatureKind::Sketch { .. } => vec![],
            FeatureKind::Extrude(e) => vec![e.sketch],
            FeatureKind::Revolve(r) => vec![r.sketch],
        }
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
                _ => {}
            }
        }
        Ok(())
    }
}
