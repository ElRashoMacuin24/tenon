//! Geometric and dimensional constraints.
//!
//! Dimensional values are lengths in millimetres and angles in radians. Signed variants
//! (`HorizontalDistance`, `VerticalDistance`) store the signed target `b - a`; the rest store a
//! non-negative magnitude and take their orientation from the geometry when solved.

use serde::{Deserialize, Serialize};

use crate::EntityId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Constraint {
    /// Two points at the same place.
    Coincident {
        a: EntityId,
        b: EntityId,
    },
    /// A point on a line (infinite), circle or arc.
    PointOnCurve {
        point: EntityId,
        curve: EntityId,
    },
    Horizontal {
        line: EntityId,
    },
    Vertical {
        line: EntityId,
    },
    Parallel {
        a: EntityId,
        b: EntityId,
    },
    Perpendicular {
        a: EntityId,
        b: EntityId,
    },
    Collinear {
        a: EntityId,
        b: EntityId,
    },
    /// Line and circle/arc, or two circles/arcs.
    Tangent {
        a: EntityId,
        b: EntityId,
    },
    /// Two circles/arcs share a centre.
    Concentric {
        a: EntityId,
        b: EntityId,
    },
    /// Two lines of equal length, or two circles/arcs of equal radius.
    Equal {
        a: EntityId,
        b: EntityId,
    },
    /// Points `a` and `b` mirror each other across `axis` (a line).
    Symmetric {
        a: EntityId,
        b: EntityId,
        axis: EntityId,
    },
    /// `point` at the middle of `line`.
    Midpoint {
        point: EntityId,
        line: EntityId,
    },
    /// `point` does not move.
    Fix {
        point: EntityId,
    },
    /// Point-point (straight line) or point-line (perpendicular) distance.
    Distance {
        a: EntityId,
        b: EntityId,
        value: f64,
    },
    /// `b.x - a.x = value` for two points.
    HorizontalDistance {
        a: EntityId,
        b: EntityId,
        value: f64,
    },
    /// `b.y - a.y = value` for two points.
    VerticalDistance {
        a: EntityId,
        b: EntityId,
        value: f64,
    },
    Length {
        line: EntityId,
        value: f64,
    },
    /// Angle between two lines (radians, magnitude).
    Angle {
        a: EntityId,
        b: EntityId,
        value: f64,
    },
    Radius {
        curve: EntityId,
        value: f64,
    },
    Diameter {
        curve: EntityId,
        value: f64,
    },
}

impl Constraint {
    /// Entities this constraint refers to.
    pub fn refs(&self) -> Vec<EntityId> {
        use Constraint::*;
        match self {
            Coincident { a, b }
            | Parallel { a, b }
            | Perpendicular { a, b }
            | Collinear { a, b }
            | Tangent { a, b }
            | Concentric { a, b }
            | Equal { a, b }
            | Distance { a, b, .. }
            | HorizontalDistance { a, b, .. }
            | VerticalDistance { a, b, .. }
            | Angle { a, b, .. } => vec![*a, *b],
            PointOnCurve { point, curve } => vec![*point, *curve],
            Horizontal { line } | Vertical { line } | Length { line, .. } => vec![*line],
            Symmetric { a, b, axis } => vec![*a, *b, *axis],
            Midpoint { point, line } => vec![*point, *line],
            Fix { point } => vec![*point],
            Radius { curve, .. } | Diameter { curve, .. } => vec![*curve],
        }
    }

    /// Driving value of a dimensional constraint.
    pub fn value(&self) -> Option<f64> {
        use Constraint::*;
        match self {
            Distance { value, .. }
            | HorizontalDistance { value, .. }
            | VerticalDistance { value, .. }
            | Length { value, .. }
            | Angle { value, .. }
            | Radius { value, .. }
            | Diameter { value, .. } => Some(*value),
            _ => None,
        }
    }

    /// Changes the driving value; false for geometric constraints.
    pub fn set_value(&mut self, v: f64) -> bool {
        use Constraint::*;
        match self {
            Distance { value, .. }
            | HorizontalDistance { value, .. }
            | VerticalDistance { value, .. }
            | Length { value, .. }
            | Angle { value, .. }
            | Radius { value, .. }
            | Diameter { value, .. } => {
                *value = v;
                true
            }
            _ => false,
        }
    }

    pub fn is_dimensional(&self) -> bool {
        self.value().is_some()
    }

    /// Angle constraints take radians; every other dimension is a length.
    pub fn is_angular(&self) -> bool {
        matches!(self, Constraint::Angle { .. })
    }

    pub fn name(&self) -> &'static str {
        use Constraint::*;
        match self {
            Coincident { .. } => "coincident",
            PointOnCurve { .. } => "point_on_curve",
            Horizontal { .. } => "horizontal",
            Vertical { .. } => "vertical",
            Parallel { .. } => "parallel",
            Perpendicular { .. } => "perpendicular",
            Collinear { .. } => "collinear",
            Tangent { .. } => "tangent",
            Concentric { .. } => "concentric",
            Equal { .. } => "equal",
            Symmetric { .. } => "symmetric",
            Midpoint { .. } => "midpoint",
            Fix { .. } => "fix",
            Distance { .. } => "distance",
            HorizontalDistance { .. } => "horizontal_distance",
            VerticalDistance { .. } => "vertical_distance",
            Length { .. } => "length",
            Angle { .. } => "angle",
            Radius { .. } => "radius",
            Diameter { .. } => "diameter",
        }
    }

    /// The same constraint with every reference passed through `f` (used when entities are
    /// replaced, e.g. by trim or fillet).
    pub fn map_refs(&self, f: impl Fn(EntityId) -> EntityId) -> Constraint {
        use Constraint::*;
        match self.clone() {
            Coincident { a, b } => Coincident { a: f(a), b: f(b) },
            PointOnCurve { point, curve } => PointOnCurve { point: f(point), curve: f(curve) },
            Horizontal { line } => Horizontal { line: f(line) },
            Vertical { line } => Vertical { line: f(line) },
            Parallel { a, b } => Parallel { a: f(a), b: f(b) },
            Perpendicular { a, b } => Perpendicular { a: f(a), b: f(b) },
            Collinear { a, b } => Collinear { a: f(a), b: f(b) },
            Tangent { a, b } => Tangent { a: f(a), b: f(b) },
            Concentric { a, b } => Concentric { a: f(a), b: f(b) },
            Equal { a, b } => Equal { a: f(a), b: f(b) },
            Symmetric { a, b, axis } => Symmetric { a: f(a), b: f(b), axis: f(axis) },
            Midpoint { point, line } => Midpoint { point: f(point), line: f(line) },
            Fix { point } => Fix { point: f(point) },
            Distance { a, b, value } => Distance { a: f(a), b: f(b), value },
            HorizontalDistance { a, b, value } => HorizontalDistance { a: f(a), b: f(b), value },
            VerticalDistance { a, b, value } => VerticalDistance { a: f(a), b: f(b), value },
            Length { line, value } => Length { line: f(line), value },
            Angle { a, b, value } => Angle { a: f(a), b: f(b), value },
            Radius { curve, value } => Radius { curve: f(curve), value },
            Diameter { curve, value } => Diameter { curve: f(curve), value },
        }
    }
}
