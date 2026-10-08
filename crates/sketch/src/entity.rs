//! Sketch entities. Points are first-class: lines, arcs and splines reference point entities, so
//! joined curves share a point instead of needing a coincidence constraint. Ids are stable for the
//! life of the sketch and become the kernel's profile curve tags (persistent naming).

use serde::{Deserialize, Serialize};
use tenon_geom::{Vec2, tol};

/// Stable id of an entity within its sketch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(pub u32);

/// Stable id of a constraint within its sketch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConstraintId(pub u32);

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "e{}", self.0)
    }
}
impl std::fmt::Display for ConstraintId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "c{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Geometry {
    Point {
        pos: Vec2,
    },
    Line {
        start: EntityId,
        end: EntityId,
    },
    Circle {
        center: EntityId,
        radius: f64,
    },
    /// Counter-clockwise from `start` to `end`; the radius is `|start - center|` (the solver keeps
    /// `end` on the same circle).
    Arc {
        center: EntityId,
        start: EntityId,
        end: EntityId,
    },
    /// Clamped uniform B-spline over control points.
    Spline {
        poles: Vec<EntityId>,
        degree: u32,
    },
}

impl Geometry {
    /// Point entities this geometry references.
    pub fn points(&self) -> Vec<EntityId> {
        match self {
            Geometry::Point { .. } => vec![],
            Geometry::Line { start, end } => vec![*start, *end],
            Geometry::Circle { center, .. } => vec![*center],
            Geometry::Arc { center, start, end } => vec![*center, *start, *end],
            Geometry::Spline { poles, .. } => poles.clone(),
        }
    }
    pub fn is_point(&self) -> bool {
        matches!(self, Geometry::Point { .. })
    }
    pub fn is_curve(&self) -> bool {
        !self.is_point()
    }
    pub fn kind_name(&self) -> &'static str {
        match self {
            Geometry::Point { .. } => "point",
            Geometry::Line { .. } => "line",
            Geometry::Circle { .. } => "circle",
            Geometry::Arc { .. } => "arc",
            Geometry::Spline { .. } => "spline",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub geometry: Geometry,
    /// Construction geometry guides other geometry but never forms profiles.
    #[serde(default)]
    pub construction: bool,
}

/// Where a new curve's point comes from: an existing point (shared, so the curves join) or a new
/// position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointRef {
    New(Vec2),
    Existing(EntityId),
}

impl From<Vec2> for PointRef {
    fn from(p: Vec2) -> Self {
        PointRef::New(p)
    }
}
impl From<EntityId> for PointRef {
    fn from(id: EntityId) -> Self {
        PointRef::Existing(id)
    }
}

/// A finite sketch coordinate within the modelling range.
pub(crate) fn valid_pos(p: Vec2) -> bool {
    tol::is_valid_coord(p.x) && tol::is_valid_coord(p.y)
}
