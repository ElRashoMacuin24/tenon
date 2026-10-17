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
    /// An ellipse (DEC-120) about `center`: `major` is at the end of one axis and `minor` at
    /// the end of the other (the solver keeps the two square to one another). Either may be the
    /// longer; the other axis is a quarter turn counter-clockwise from the first.
    Ellipse {
        center: EntityId,
        major: EntityId,
        minor: EntityId,
    },
    /// Clamped uniform B-spline over control points.
    Spline {
        poles: Vec<EntityId>,
        degree: u32,
    },
    /// Text (DEC-102): its first line's baseline starts at point `at`; its capital letters are
    /// `height` tall; it is turned `angle` radians about `at`. Lines are parted by a new line. Its
    /// letters are outlined by the font (see `text.rs`) and are profiles of their own.
    Text {
        at: EntityId,
        text: String,
        height: f64,
        #[serde(default, skip_serializing_if = "is_zero")]
        angle: f64,
    },
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl Geometry {
    /// Point entities this geometry references.
    pub fn points(&self) -> Vec<EntityId> {
        match self {
            Geometry::Point { .. } => vec![],
            Geometry::Line { start, end } => vec![*start, *end],
            Geometry::Circle { center, .. } => vec![*center],
            Geometry::Arc { center, start, end } => vec![*center, *start, *end],
            Geometry::Ellipse { center, major, minor } => vec![*center, *major, *minor],
            Geometry::Spline { poles, .. } => poles.clone(),
            Geometry::Text { at, .. } => vec![*at],
        }
    }
    pub fn is_point(&self) -> bool {
        matches!(self, Geometry::Point { .. })
    }
    /// A line, circle, arc or spline. (Text is no curve: its letters are outlined apart.)
    pub fn is_curve(&self) -> bool {
        !self.is_point() && !self.is_text()
    }
    pub fn is_text(&self) -> bool {
        matches!(self, Geometry::Text { .. })
    }
    pub fn kind_name(&self) -> &'static str {
        match self {
            Geometry::Point { .. } => "point",
            Geometry::Line { .. } => "line",
            Geometry::Circle { .. } => "circle",
            Geometry::Arc { .. } => "arc",
            Geometry::Ellipse { .. } => "ellipse",
            Geometry::Spline { .. } => "spline",
            Geometry::Text { .. } => "text",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub geometry: Geometry,
    /// Construction geometry guides other geometry but never forms profiles.
    #[serde(default)]
    pub construction: bool,
    /// A line marked as a centre line: the axis a revolution turns about unless told otherwise,
    /// and the line a dimension to it measures across (a diameter). It still bounds profiles.
    #[serde(default, skip_serializing_if = "is_false")]
    pub centerline: bool,
    /// Reference geometry: it is where something outside the sketch puts it (an edge of the part
    /// projected onto the sketch plane) and the solver never moves it. Other geometry can be
    /// constrained and dimensioned to it, and it bounds profiles like any curve.
    #[serde(default, skip_serializing_if = "is_false")]
    pub reference: bool,
    /// A point on its own marked as a sketch point rather than a centre point: a hole does not
    /// go at it unless it is picked (DEC-117). Points on their own are centre points otherwise.
    #[serde(default, skip_serializing_if = "is_false")]
    pub sketch_point: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
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
