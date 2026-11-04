//! 3D sketches (DEC-136): points, lines, bends and splines that lie on no plane, held by
//! constraints in a solver as a 2D sketch's are.
//!
//! The solver is [`crate::lm`]'s, as the 2D sketch's and the assembly's are: damped minimum-norm
//! Gauss-Newton, conflict found by leaving each constraint out, redundancy by the rank of the
//! Jacobian. The unknowns are three for each point. An arc has none of its own: one row keeps its
//! end as far from its centre as its start, and it goes the short way round.
//!
//! Entity ids are of the same kind as a 2D sketch's ([`EntityId`]), so a sweep's path names the
//! curves of either the same way.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use tenon_geom::{Vec3, tol};
use tenon_kernel::{Curve3, Path3};

use crate::lm;
use crate::sketch::{SketchError, SketchResult, as_pairs};
use crate::solve::{Dof, determined_columns};
use crate::{ConstraintId, EntityId, MAX_CONSTRAINTS, MAX_ENTITIES};

/// Most iterations of one solve.
const MAX_ITER: usize = 100;
/// Constraints tried one by one when looking for the cause of a conflict.
const MAX_CONFLICT_PROBES: usize = 48;
/// Weight of a dragged point: it moves least, so it ends where the pointer is when it can.
const DRAG_WEIGHT: f64 = 1e4;
/// Most points one spline goes through.
pub const MAX_SPLINE_POINTS: usize = 500;
/// Most points an edge brought in is drawn through.
pub const MAX_EDGE_POINTS: usize = 4096;
/// Pieces a span of a spline is drawn in.
const SPLINE_STEPS: usize = 24;
/// Pieces a half turn of an arc is drawn in.
const ARC_STEPS: usize = 48;

fn invalid(msg: impl Into<String>) -> SketchError {
    SketchError::Invalid(msg.into())
}

/// A finite coordinate within the modelling range.
fn valid_pos(p: Vec3) -> bool {
    tol::is_valid_coord(p.x) && tol::is_valid_coord(p.y) && tol::is_valid_coord(p.z)
}

fn dir_n(a: Vec3, b: Vec3) -> Vec3 {
    let d = b - a;
    let l = d.len();
    if l > 1e-12 { d * (1.0 / l) } else { d }
}

/// One of the three directions of space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis3 {
    X,
    Y,
    Z,
}

impl Axis3 {
    pub const ALL: [Axis3; 3] = [Axis3::X, Axis3::Y, Axis3::Z];

    pub fn of(self, p: Vec3) -> f64 {
        match self {
            Axis3::X => p.x,
            Axis3::Y => p.y,
            Axis3::Z => p.z,
        }
    }
    pub fn dir(self) -> Vec3 {
        match self {
            Axis3::X => Vec3::X,
            Axis3::Y => Vec3::Y,
            Axis3::Z => Vec3::Z,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Axis3::X => "X",
            Axis3::Y => "Y",
            Axis3::Z => "Z",
        }
    }
}

/// A curved edge of the part brought into a 3D sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeCurve {
    /// A circular edge: the point halfway along it, so that it is the arc from its start through
    /// this to its end. (A whole circle has its start and end at one place.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid: Option<Vec3>,
    /// Points along it from its start to its end, near enough to draw and pick it, and to run a
    /// sweep along an edge that is not circular.
    pub along: Vec<Vec3>,
    /// Its length, as the part has it.
    pub length: f64,
}

/// What an entity of a 3D sketch is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Geometry3 {
    Point {
        pos: Vec3,
    },
    Line {
        start: EntityId,
        end: EntityId,
    },
    /// An arc: from `start` to `end` the short way round `center`, in the plane of the three
    /// (the solver keeps `end` as far from the centre as `start`). A bend is one of these.
    Arc {
        center: EntityId,
        start: EntityId,
        end: EntityId,
    },
    /// A smooth curve through its points, in order ([`Through`]).
    Spline {
        points: Vec<EntityId>,
    },
    /// A curved edge of the part: always reference geometry. It is where the part has it.
    Edge {
        start: EntityId,
        end: EntityId,
        curve: EdgeCurve,
    },
}

impl Geometry3 {
    /// Point entities this geometry references.
    pub fn points(&self) -> Vec<EntityId> {
        match self {
            Geometry3::Point { .. } => vec![],
            Geometry3::Line { start, end } | Geometry3::Edge { start, end, .. } => vec![*start, *end],
            Geometry3::Arc { center, start, end } => vec![*center, *start, *end],
            Geometry3::Spline { points } => points.clone(),
        }
    }
    pub fn is_point(&self) -> bool {
        matches!(self, Geometry3::Point { .. })
    }
    pub fn is_curve(&self) -> bool {
        !self.is_point()
    }
    /// Where the curve starts and ends.
    pub fn ends(&self) -> Option<(EntityId, EntityId)> {
        match self {
            Geometry3::Point { .. } => None,
            Geometry3::Line { start, end } | Geometry3::Arc { start, end, .. } | Geometry3::Edge { start, end, .. } => Some((*start, *end)),
            Geometry3::Spline { points } => Some((*points.first()?, *points.last()?)),
        }
    }
    pub fn kind_name(&self) -> &'static str {
        match self {
            Geometry3::Point { .. } => "point",
            Geometry3::Line { .. } => "line",
            Geometry3::Arc { .. } => "arc",
            Geometry3::Spline { .. } => "spline",
            Geometry3::Edge { .. } => "edge",
        }
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity3 {
    pub geometry: Geometry3,
    /// Construction geometry guides other geometry but is no part of a default sweep path.
    #[serde(default, skip_serializing_if = "is_false")]
    pub construction: bool,
    /// Reference geometry: it is where something outside the sketch puts it (a vertex, a work
    /// point, an edge of the part), and the solver never moves it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub reference: bool,
}

/// A constraint or dimension of a 3D sketch. Lengths in millimetres.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Constraint3 {
    /// Two points at the same place.
    Coincident { a: EntityId, b: EntityId },
    /// `point` does not move.
    Fix { point: EntityId },
    /// A line parallel to X, Y or Z.
    Along { line: EntityId, axis: Axis3 },
    /// Two lines in one direction.
    Parallel { a: EntityId, b: EntityId },
    /// Two lines square to one another.
    Perpendicular { a: EntityId, b: EntityId },
    /// Two lines of one length, or two arcs of one radius.
    Equal { a: EntityId, b: EntityId },
    /// Two curves that meet at an end run on smoothly there: the same direction at that end.
    /// (A line and a line are not offered: that is [`Constraint3::Parallel`].)
    Tangent { a: EntityId, b: EntityId },
    /// `point` is where the two tangents at `arc`'s ends meet: the corner a bend rounds, kept so
    /// that what holds the corner goes on holding it.
    Corner { point: EntityId, arc: EntityId },
    /// The length of a line.
    Length { line: EntityId, value: f64 },
    /// How far apart two points are.
    Distance { a: EntityId, b: EntityId, value: f64 },
    /// A point's X, Y or Z.
    Coordinate { point: EntityId, axis: Axis3, value: f64 },
    /// The radius of an arc.
    Radius { arc: EntityId, value: f64 },
}

impl Constraint3 {
    /// Entities this constraint refers to.
    pub fn refs(&self) -> Vec<EntityId> {
        use Constraint3::*;
        match self {
            Coincident { a, b } | Parallel { a, b } | Perpendicular { a, b } | Equal { a, b } | Tangent { a, b } | Distance { a, b, .. } => {
                vec![*a, *b]
            }
            Fix { point } | Coordinate { point, .. } => vec![*point],
            Along { line, .. } | Length { line, .. } => vec![*line],
            Corner { point, arc } => vec![*point, *arc],
            Radius { arc, .. } => vec![*arc],
        }
    }

    /// The same constraint with every reference passed through `f`.
    pub fn map_refs(&self, f: impl Fn(EntityId) -> EntityId) -> Constraint3 {
        use Constraint3::*;
        match self.clone() {
            Coincident { a, b } => Coincident { a: f(a), b: f(b) },
            Fix { point } => Fix { point: f(point) },
            Along { line, axis } => Along { line: f(line), axis },
            Parallel { a, b } => Parallel { a: f(a), b: f(b) },
            Perpendicular { a, b } => Perpendicular { a: f(a), b: f(b) },
            Equal { a, b } => Equal { a: f(a), b: f(b) },
            Tangent { a, b } => Tangent { a: f(a), b: f(b) },
            Corner { point, arc } => Corner { point: f(point), arc: f(arc) },
            Length { line, value } => Length { line: f(line), value },
            Distance { a, b, value } => Distance { a: f(a), b: f(b), value },
            Coordinate { point, axis, value } => Coordinate { point: f(point), axis, value },
            Radius { arc, value } => Radius { arc: f(arc), value },
        }
    }

    /// The value of a dimension.
    pub fn value(&self) -> Option<f64> {
        use Constraint3::*;
        match self {
            Length { value, .. } | Distance { value, .. } | Coordinate { value, .. } | Radius { value, .. } => Some(*value),
            _ => None,
        }
    }

    /// Changes a dimension's value; false for a constraint with none.
    pub fn set_value(&mut self, v: f64) -> bool {
        use Constraint3::*;
        match self {
            Length { value, .. } | Distance { value, .. } | Coordinate { value, .. } | Radius { value, .. } => {
                *value = v;
                true
            }
            _ => false,
        }
    }

    pub fn is_dimensional(&self) -> bool {
        self.value().is_some()
    }

    pub fn name(&self) -> &'static str {
        use Constraint3::*;
        match self {
            Coincident { .. } => "coincident",
            Fix { .. } => "fix",
            Along { .. } => "along",
            Parallel { .. } => "parallel",
            Perpendicular { .. } => "perpendicular",
            Equal { .. } => "equal",
            Tangent { .. } => "tangent",
            Corner { .. } => "corner",
            Length { .. } => "length",
            Distance { .. } => "distance",
            Coordinate { .. } => "coordinate",
            Radius { .. } => "radius",
        }
    }

    /// In words, for what is said when it cannot be had.
    pub fn describe(&self) -> String {
        use Constraint3::*;
        match self {
            Coincident { a, b } => format!("{a} and {b} coincident"),
            Fix { point } => format!("{point} fixed"),
            Along { line, axis } => format!("{line} parallel to {}", axis.name()),
            Parallel { a, b } => format!("{a} parallel to {b}"),
            Perpendicular { a, b } => format!("{a} perpendicular to {b}"),
            Equal { a, b } => format!("{a} equal to {b}"),
            Tangent { a, b } => format!("{a} tangent to {b}"),
            Corner { point, arc } => format!("{point} the corner of bend {arc}"),
            Length { line, value } => format!("{line} {value} mm long"),
            Distance { a, b, value } => format!("{a} and {b} {value} mm apart"),
            Coordinate { point, axis, value } => format!("{} of {point} at {value}", axis.name()),
            Radius { arc, value } => format!("{arc} of radius {value} mm"),
        }
    }
}

/// Where a new curve's point comes from: an existing point (shared, so the curves join) or a new
/// position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointRef3 {
    New(Vec3),
    Existing(EntityId),
}

impl From<Vec3> for PointRef3 {
    fn from(p: Vec3) -> Self {
        PointRef3::New(p)
    }
}
impl From<EntityId> for PointRef3 {
    fn from(id: EntityId) -> Self {
        PointRef3::Existing(id)
    }
}

/// A bend as [`Sketch3d::add_bend`] makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bend3 {
    pub arc: EntityId,
    pub center: EntityId,
    /// Where the arc starts (on the first line) and ends (on the second).
    pub start: EntityId,
    pub end: EntityId,
    /// The corner it rounds, kept as a construction point.
    pub corner: EntityId,
    /// The radius's dimension.
    pub radius: ConstraintId,
}

/// An arc in space: the short way round `center` from `start` to `end`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc3 {
    pub center: Vec3,
    pub radius: f64,
    /// From the centre to the start, of unit length.
    pub u: Vec3,
    /// In the arc's plane, a quarter turn on from `u` the way the arc goes, of unit length.
    pub v: Vec3,
    /// How far round it goes (radians), more than 0 and less than a half turn.
    pub sweep: f64,
}

impl Arc3 {
    pub fn new(center: Vec3, start: Vec3, end: Vec3) -> Option<Arc3> {
        let (a, b) = (start - center, end - center);
        let radius = a.len();
        if radius <= tol::MIN_SIZE {
            return None;
        }
        let u = a * (1.0 / radius);
        let n = a.cross(b);
        if n.len() <= 1e-12 * radius * radius {
            return None;
        }
        let v = n.cross(a);
        let v = v * (1.0 / v.len());
        Some(Arc3 { center, radius, u, v, sweep: b.dot(v).atan2(b.dot(u)) })
    }
    /// The point `t` of the way round it, from 0 to 1.
    pub fn at(&self, t: f64) -> Vec3 {
        let (s, c) = (self.sweep * t).sin_cos();
        self.center + (self.u * c + self.v * s) * self.radius
    }
    pub fn length(&self) -> f64 {
        self.radius * self.sweep
    }
}

/// Gauss-Legendre points and weights on 0 to 1, five of them: exact for a polynomial of degree 9.
const GAUSS: [(f64, f64); 5] = [
    (0.046_910_077_030_668, 0.118_463_442_528_095),
    (0.230_765_344_947_158, 0.239_314_335_249_683),
    (0.5, 0.284_444_444_444_444),
    (0.769_234_655_052_842, 0.239_314_335_249_683),
    (0.953_089_922_969_332, 0.118_463_442_528_095),
];

/// A smooth curve through points: a cubic in each span between two points, parameter the distance
/// between them, with no sudden change of direction or of curvature at the points and none of
/// curvature at its ends (the natural cubic spline).
#[derive(Clone, Debug, PartialEq)]
pub struct Through {
    points: Vec<Vec3>,
    /// The direction and speed at each point.
    tangents: Vec<Vec3>,
    /// How long each span's parameter runs.
    spans: Vec<f64>,
}

impl Through {
    /// None unless there are two points or more, each apart from the next.
    pub fn new(points: &[Vec3]) -> Option<Through> {
        let n = points.len();
        if n < 2 {
            return None;
        }
        let spans: Vec<f64> = points.windows(2).map(|w| w[0].dist(w[1])).collect();
        if spans.iter().any(|h| *h <= tol::MIN_SIZE) {
            return None;
        }
        // The tridiagonal system of the natural spline's tangents, by elimination (it is
        // diagonally dominant, so it needs no pivots).
        let mut low = vec![0.0; n];
        let mut diag = vec![0.0; n];
        let mut up = vec![0.0; n];
        let mut rhs = vec![Vec3::ZERO; n];
        diag[0] = 2.0;
        up[0] = 1.0;
        rhs[0] = (points[1] - points[0]) * (3.0 / spans[0]);
        for i in 1..n - 1 {
            let (h0, h1) = (spans[i - 1], spans[i]);
            low[i] = h1;
            diag[i] = 2.0 * (h0 + h1);
            up[i] = h0;
            rhs[i] = (points[i] - points[i - 1]) * (3.0 * h1 / h0) + (points[i + 1] - points[i]) * (3.0 * h0 / h1);
        }
        low[n - 1] = 1.0;
        diag[n - 1] = 2.0;
        rhs[n - 1] = (points[n - 1] - points[n - 2]) * (3.0 / spans[n - 2]);
        for i in 1..n {
            let f = low[i] / diag[i - 1];
            diag[i] -= f * up[i - 1];
            rhs[i] = rhs[i] - rhs[i - 1] * f;
        }
        let mut tangents = vec![Vec3::ZERO; n];
        tangents[n - 1] = rhs[n - 1] * (1.0 / diag[n - 1]);
        for i in (0..n - 1).rev() {
            tangents[i] = (rhs[i] - tangents[i + 1] * up[i]) * (1.0 / diag[i]);
        }
        Some(Through { points: points.to_vec(), tangents, spans })
    }

    /// The four control points of span `i`.
    fn bezier(&self, i: usize) -> [Vec3; 4] {
        let h = self.spans[i] / 3.0;
        [self.points[i], self.points[i] + self.tangents[i] * h, self.points[i + 1] - self.tangents[i + 1] * h, self.points[i + 1]]
    }

    /// Where span `i` is at `t` from 0 to 1.
    fn point(&self, i: usize, t: f64) -> Vec3 {
        let [a, b, c, d] = self.bezier(i);
        let s = 1.0 - t;
        a * (s * s * s) + b * (3.0 * s * s * t) + c * (3.0 * s * t * t) + d * (t * t * t)
    }

    /// How fast span `i` goes at `t`.
    fn speed(&self, i: usize, t: f64) -> f64 {
        let [a, b, c, d] = self.bezier(i);
        let s = 1.0 - t;
        ((b - a) * (3.0 * s * s) + (c - b) * (6.0 * s * t) + (d - c) * (3.0 * t * t)).len()
    }

    /// The unit direction it sets off in.
    pub fn start_tangent(&self) -> Vec3 {
        self.tangents.first().map_or(Vec3::ZERO, |t| t.normalized())
    }

    /// The unit direction it ends in.
    pub fn end_tangent(&self) -> Vec3 {
        self.tangents.last().map_or(Vec3::ZERO, |t| t.normalized())
    }

    /// Its length: each span in four parts, each by five Gauss-Legendre points.
    pub fn length(&self) -> f64 {
        let mut sum = 0.0;
        for i in 0..self.spans.len() {
            for part in 0..4 {
                for (at, weight) in GAUSS {
                    let t = (f64::from(part) + at) / 4.0;
                    sum += self.speed(i, t) * weight / 4.0;
                }
            }
        }
        sum
    }

    /// Points along it, near enough to draw and pick it.
    pub fn polyline(&self) -> Vec<Vec3> {
        let mut out = Vec::with_capacity(self.spans.len() * SPLINE_STEPS + 1);
        for i in 0..self.spans.len() {
            for k in 0..SPLINE_STEPS {
                out.push(self.point(i, k as f64 / SPLINE_STEPS as f64));
            }
        }
        out.extend(self.points.last().copied());
        out
    }

    /// The same curve as a cubic B-spline: its poles and its whole knot vector. (Each knot inside
    /// comes once: the curve is smooth to its curvature there, and the kernel's spline says so.)
    pub fn b_spline(&self) -> (Vec<Vec3>, Vec<f64>) {
        let n = self.spans.len();
        let mut knots = vec![0.0; 4];
        let mut at = 0.0;
        for (i, h) in self.spans.iter().enumerate() {
            at += h;
            if i + 1 < n {
                knots.push(at);
            }
        }
        knots.extend([at; 4]);
        let mut poles = Vec::with_capacity(n + 3);
        poles.extend(self.points.first().copied());
        let first = self.bezier(0);
        poles.push(first[1]);
        // The pole between two spans is on the line of the second's first two inner control
        // points, as far beyond the first as the spans' lengths say.
        for i in 1..n {
            let [_, b, c, _] = self.bezier(i);
            poles.push(b - (c - b) * (self.spans[i - 1] / self.spans[i]));
        }
        let last = self.bezier(n - 1);
        poles.push(last[2]);
        poles.extend(self.points.last().copied());
        (poles, knots)
    }
}

/// A 3D sketch. Coordinates are millimetres in the part's own axes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sketch3d {
    #[serde(with = "as_pairs")]
    entities: BTreeMap<EntityId, Entity3>,
    #[serde(with = "as_pairs")]
    constraints: BTreeMap<ConstraintId, Constraint3>,
    next_entity: u32,
    next_constraint: u32,
}

impl Sketch3d {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- reading ----------------------------------------------------------------------------

    pub fn entities(&self) -> impl Iterator<Item = (EntityId, &Entity3)> {
        self.entities.iter().map(|(k, v)| (*k, v))
    }
    pub fn entity(&self, id: EntityId) -> Option<&Entity3> {
        self.entities.get(&id)
    }
    pub fn constraints(&self) -> impl Iterator<Item = (ConstraintId, &Constraint3)> {
        self.constraints.iter().map(|(k, v)| (*k, v))
    }
    pub fn constraint(&self, id: ConstraintId) -> Option<&Constraint3> {
        self.constraints.get(&id)
    }
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
    pub fn geometry(&self, id: EntityId) -> Option<&Geometry3> {
        self.entities.get(&id).map(|e| &e.geometry)
    }
    pub fn point(&self, id: EntityId) -> Option<Vec3> {
        match self.geometry(id)? {
            Geometry3::Point { pos } => Some(*pos),
            _ => None,
        }
    }
    pub fn is_point(&self, id: EntityId) -> bool {
        self.point(id).is_some()
    }
    pub fn is_line(&self, id: EntityId) -> bool {
        matches!(self.geometry(id), Some(Geometry3::Line { .. }))
    }
    pub fn is_arc(&self, id: EntityId) -> bool {
        matches!(self.geometry(id), Some(Geometry3::Arc { .. }))
    }
    pub fn is_curve(&self, id: EntityId) -> bool {
        self.geometry(id).is_some_and(Geometry3::is_curve)
    }
    pub fn is_reference(&self, id: EntityId) -> bool {
        self.entity(id).is_some_and(|e| e.reference)
    }
    pub fn line(&self, id: EntityId) -> Option<(Vec3, Vec3)> {
        match self.geometry(id)? {
            Geometry3::Line { start, end } => Some((self.point(*start)?, self.point(*end)?)),
            _ => None,
        }
    }
    pub fn arc(&self, id: EntityId) -> Option<Arc3> {
        match self.geometry(id)? {
            Geometry3::Arc { center, start, end } => Arc3::new(self.point(*center)?, self.point(*start)?, self.point(*end)?),
            _ => None,
        }
    }
    pub fn spline(&self, id: EntityId) -> Option<Through> {
        match self.geometry(id)? {
            Geometry3::Spline { points } => {
                let pts: Option<Vec<Vec3>> = points.iter().map(|p| self.point(*p)).collect();
                Through::new(&pts?)
            }
            _ => None,
        }
    }
    /// Curves that use point `p`.
    pub fn curves_at(&self, p: EntityId) -> Vec<EntityId> {
        self.entities.iter().filter(|(_, e)| e.geometry.points().contains(&p)).map(|(id, _)| *id).collect()
    }
    /// Constraints that refer to `id`.
    pub fn constraints_on(&self, id: EntityId) -> Vec<ConstraintId> {
        self.constraints.iter().filter(|(_, c)| c.refs().contains(&id)).map(|(cid, _)| *cid).collect()
    }
    /// Where the sketch reaches: its points, and the points along the edges brought in.
    pub fn extent(&self) -> Vec<Vec3> {
        let along = self.entities.values().filter_map(|e| match &e.geometry {
            Geometry3::Edge { curve, .. } => Some(curve.along.iter().copied()),
            _ => None,
        });
        self.entities.keys().filter_map(|id| self.point(*id)).chain(along.flatten()).collect()
    }

    /// Points along a curve (the one place of a point), near enough to draw and pick it.
    pub fn polyline(&self, id: EntityId) -> Vec<Vec3> {
        match self.geometry(id) {
            Some(Geometry3::Point { pos }) => vec![*pos],
            Some(Geometry3::Line { .. }) => self.line(id).map_or_else(Vec::new, |(a, b)| vec![a, b]),
            Some(Geometry3::Arc { .. }) => self.arc(id).map_or_else(Vec::new, |a| {
                let n = ((a.sweep / std::f64::consts::PI * ARC_STEPS as f64).ceil() as usize).max(2);
                (0..=n).map(|k| a.at(k as f64 / n as f64)).collect()
            }),
            Some(Geometry3::Spline { .. }) => self.spline(id).map_or_else(Vec::new, |s| s.polyline()),
            Some(Geometry3::Edge { curve, .. }) => curve.along.clone(),
            None => Vec::new(),
        }
    }

    /// The length of a curve: a line's and an arc's exactly, a spline's by adding up along it
    /// (Gauss-Legendre), an edge's as the part has it. None for what is no curve.
    pub fn length(&self, id: EntityId) -> Option<f64> {
        match self.geometry(id)? {
            Geometry3::Point { .. } => None,
            Geometry3::Line { .. } => self.line(id).map(|(a, b)| a.dist(b)),
            Geometry3::Arc { .. } => self.arc(id).map(|a| a.length()),
            Geometry3::Spline { .. } => self.spline(id).map(|s| s.length()),
            Geometry3::Edge { curve, .. } => Some(curve.length),
        }
    }

    /// The unit direction of curve `id` at its end `at`, from its start towards its end.
    pub fn direction_at(&self, id: EntityId, at: EntityId) -> Option<Vec3> {
        let (start, end) = self.geometry(id)?.ends()?;
        if at != start && at != end {
            return None;
        }
        match self.geometry(id)? {
            Geometry3::Point { .. } => None,
            Geometry3::Line { .. } => self.line(id).map(|(a, b)| dir_n(a, b)),
            Geometry3::Arc { .. } => {
                let a = self.arc(id)?;
                let k = if at == start { 0.0 } else { a.sweep };
                let (s, c) = f64::sin_cos(k);
                Some(a.v * c - a.u * s)
            }
            Geometry3::Spline { .. } => {
                let s = self.spline(id)?;
                Some(if at == start { s.start_tangent() } else { s.end_tangent() })
            }
            Geometry3::Edge { curve, .. } => {
                let n = curve.along.len();
                let (p, q) = if at == start {
                    (*curve.along.first()?, *curve.along.get(1)?)
                } else {
                    (*curve.along.get(n.checked_sub(2)?)?, *curve.along.last()?)
                };
                Some(dir_n(p, q))
            }
        }
    }

    /// Where two curves join, end to end: an end of `a` and an end of `b` that are one point or
    /// are held together (coincident).
    pub fn joined_ends(&self, a: EntityId, b: EntityId) -> Option<(EntityId, EntityId)> {
        let ends = |id: EntityId| -> Vec<EntityId> {
            match self.geometry(id).and_then(Geometry3::ends) {
                Some((s, e)) => vec![s, e],
                None => Vec::new(),
            }
        };
        let held = |p: EntityId, q: EntityId| {
            p == q || self.constraints.values().any(|c| matches!(c, Constraint3::Coincident { a, b } if (*a, *b) == (p, q) || (*a, *b) == (q, p)))
        };
        let of_b = ends(b);
        ends(a).into_iter().find_map(|p| of_b.iter().find(|q| held(p, **q)).map(|q| (p, *q)))
    }

    // ---- creating geometry ------------------------------------------------------------------

    fn insert(&mut self, geometry: Geometry3) -> SketchResult<EntityId> {
        if self.entities.len() >= MAX_ENTITIES {
            return Err(SketchError::TooMany);
        }
        self.next_entity = self.next_entity.checked_add(1).ok_or(SketchError::TooMany)?;
        let id = EntityId(self.next_entity);
        self.entities.insert(id, Entity3 { geometry, construction: false, reference: false });
        Ok(id)
    }

    pub fn add_point(&mut self, pos: Vec3) -> SketchResult<EntityId> {
        if !valid_pos(pos) {
            return Err(invalid(format!("a point at ({}, {}, {}) is out of range", pos.x, pos.y, pos.z)));
        }
        self.insert(Geometry3::Point { pos })
    }

    fn peek(&self, p: PointRef3) -> SketchResult<Vec3> {
        match p {
            PointRef3::New(pos) if valid_pos(pos) => Ok(pos),
            PointRef3::New(pos) => Err(invalid(format!("a point at ({}, {}, {}) is out of range", pos.x, pos.y, pos.z))),
            PointRef3::Existing(id) => self.point(id).ok_or_else(|| invalid(format!("{id} is not a point"))),
        }
    }

    /// An existing point, or a new one.
    pub fn resolve(&mut self, p: PointRef3) -> SketchResult<EntityId> {
        match p {
            PointRef3::New(pos) => self.add_point(pos),
            PointRef3::Existing(id) if self.is_point(id) => Ok(id),
            PointRef3::Existing(id) => Err(invalid(format!("{id} is not a point"))),
        }
    }

    pub fn add_line(&mut self, a: impl Into<PointRef3>, b: impl Into<PointRef3>) -> SketchResult<EntityId> {
        let (a, b) = (a.into(), b.into());
        // (Checked before any point is made, so a line refused leaves nothing behind.)
        if a == b || self.peek(a)?.dist(self.peek(b)?) <= tol::MIN_SIZE {
            return Err(invalid("a line needs two points apart from one another: this one has both ends at one place"));
        }
        let (a, b) = (self.resolve(a)?, self.resolve(b)?);
        self.insert(Geometry3::Line { start: a, end: b })
    }

    /// A spline through two points or more, in order.
    pub fn add_spline(&mut self, points: &[PointRef3]) -> SketchResult<EntityId> {
        if points.len() < 2 || points.len() > MAX_SPLINE_POINTS {
            return Err(invalid(format!("a spline goes through 2 to {MAX_SPLINE_POINTS} points, not {}", points.len())));
        }
        let at: Vec<Vec3> = points.iter().map(|p| self.peek(*p)).collect::<SketchResult<_>>()?;
        if Through::new(&at).is_none() {
            return Err(invalid("a spline needs each of its points apart from the next: two of these are at one place"));
        }
        let ids = points.iter().map(|p| self.resolve(*p)).collect::<SketchResult<Vec<_>>>()?;
        self.insert(Geometry3::Spline { points: ids })
    }

    /// An edge of the part as reference geometry, from `start` to `end`. Its points are reference
    /// geometry too.
    pub fn add_edge(&mut self, start: Vec3, end: Vec3, curve: EdgeCurve) -> SketchResult<EntityId> {
        check_edge(&curve)?;
        let mut s = self.clone();
        let a = s.add_point(start)?;
        let b = if start.dist(end) <= tol::MIN_SIZE { a } else { s.add_point(end)? };
        let id = s.insert(Geometry3::Edge { start: a, end: b, curve })?;
        for e in [a, b, id] {
            s.set_reference(e, true)?;
        }
        *self = s;
        Ok(id)
    }

    /// A straight edge of the part as a reference line.
    pub fn add_edge_line(&mut self, start: Vec3, end: Vec3) -> SketchResult<EntityId> {
        let mut s = self.clone();
        let id = s.add_line(start, end)?;
        if let Some((a, b)) = s.geometry(id).and_then(Geometry3::ends) {
            for e in [a, b, id] {
                s.set_reference(e, true)?;
            }
        }
        *self = s;
        Ok(id)
    }

    /// A reference point (where a vertex or a work point of the part is).
    pub fn add_reference_point(&mut self, at: Vec3) -> SketchResult<EntityId> {
        let id = self.add_point(at)?;
        self.set_reference(id, true)?;
        Ok(id)
    }

    pub fn set_construction(&mut self, id: EntityId, construction: bool) -> SketchResult<()> {
        self.entities.get_mut(&id).ok_or(SketchError::NoEntity(id))?.construction = construction;
        Ok(())
    }

    /// Marks an entity as reference geometry (or as the sketch's own again).
    pub fn set_reference(&mut self, id: EntityId, reference: bool) -> SketchResult<()> {
        self.entities.get_mut(&id).ok_or(SketchError::NoEntity(id))?.reference = reference;
        Ok(())
    }

    /// Moves a point without solving.
    pub fn set_point(&mut self, id: EntityId, pos: Vec3) -> SketchResult<()> {
        if !valid_pos(pos) {
            return Err(invalid(format!("a point at ({}, {}, {}) is out of range", pos.x, pos.y, pos.z)));
        }
        match self.entities.get_mut(&id).map(|e| &mut e.geometry) {
            Some(Geometry3::Point { pos: p }) => {
                *p = pos;
                Ok(())
            }
            Some(_) => Err(invalid(format!("{id} is not a point"))),
            None => Err(SketchError::NoEntity(id)),
        }
    }

    /// Gives an edge brought in the curve the part has for it now, without solving.
    pub fn set_edge_curve(&mut self, id: EntityId, new: EdgeCurve) -> SketchResult<()> {
        check_edge(&new)?;
        match self.entities.get_mut(&id).map(|e| &mut e.geometry) {
            Some(Geometry3::Edge { curve, .. }) => {
                *curve = new;
                Ok(())
            }
            Some(_) => Err(invalid(format!("{id} is not an edge brought in"))),
            None => Err(SketchError::NoEntity(id)),
        }
    }

    /// Puts `new` in place of `old` in every curve and constraint that has it, and takes `old` away.
    fn merge_points(&mut self, keep: EntityId, old: EntityId) {
        for e in self.entities.values_mut() {
            match &mut e.geometry {
                Geometry3::Line { start, end } | Geometry3::Edge { start, end, .. } => {
                    for p in [start, end] {
                        if *p == old {
                            *p = keep;
                        }
                    }
                }
                Geometry3::Arc { center, start, end } => {
                    for p in [center, start, end] {
                        if *p == old {
                            *p = keep;
                        }
                    }
                }
                Geometry3::Spline { points } => points.iter_mut().filter(|p| **p == old).for_each(|p| *p = keep),
                Geometry3::Point { .. } => {}
            }
        }
        let map = |r: EntityId| if r == old { keep } else { r };
        for c in self.constraints.values_mut() {
            *c = c.map_refs(map);
        }
        // (What has come to be between one point and itself says nothing now.)
        self.constraints.retain(|_, c| !matches!(c, Constraint3::Coincident { a, b } if a == b));
        self.entities.remove(&old);
    }

    /// Rounds the corner at point `corner`, where two lines meet, by an arc of `radius` tangent to
    /// both: the lines end where the arc begins, and the corner stays as a construction point
    /// held where the tangents meet, so what held it goes on holding it. A dimension of the
    /// length of either line becomes the distance from its far end to the corner.
    pub fn add_bend(&mut self, corner: EntityId, radius: f64) -> SketchResult<Bend3> {
        if !tol::is_valid_size(radius) {
            return Err(invalid(format!("a bend's radius must be more than 0, not {radius}")));
        }
        if !self.is_point(corner) {
            return Err(invalid(format!("{corner} is not a point: click the corner where two lines meet")));
        }
        let mut s = self.clone();
        // Lines that end at it, or at a point held to it: those points become the one point.
        let held: Vec<EntityId> = s
            .constraints
            .values()
            .filter_map(|c| match c {
                Constraint3::Coincident { a, b } if *a == corner => Some(*b),
                Constraint3::Coincident { a, b } if *b == corner => Some(*a),
                _ => None,
            })
            .collect();
        for h in held {
            s.merge_points(corner, h);
        }
        let curves = s.curves_at(corner);
        let [a, b] = curves.as_slice() else {
            return Err(invalid(format!("A bend rounds a corner between two lines, and {} curves meet at {corner}.", curves.len())));
        };
        let (a, b) = (*a, *b);
        if !s.is_line(a) || !s.is_line(b) {
            return Err(invalid("A bend rounds a corner between two lines: one of the curves there is not a line."));
        }
        if s.is_reference(a) || s.is_reference(b) || s.is_reference(corner) {
            return Err(invalid("A bend cannot change an edge of the part: draw lines of the sketch's own to round the corner of."));
        }
        let far = |sk: &Sketch3d, line: EntityId| -> Option<EntityId> {
            let (p, q) = sk.geometry(line)?.ends()?;
            Some(if p == corner { q } else { p })
        };
        let (far_a, far_b) = (
            far(&s, a).ok_or_else(|| invalid("internal: a line without ends"))?,
            far(&s, b).ok_or_else(|| invalid("internal: a line without ends"))?,
        );
        let at = |sk: &Sketch3d, id: EntityId| sk.point(id).ok_or_else(|| invalid("internal: a line without points"));
        let (pc, pa, pb) = (at(&s, corner)?, at(&s, far_a)?, at(&s, far_b)?);
        let (da, db) = (dir_n(pc, pa), dir_n(pc, pb));
        let (la, lb) = (pc.dist(pa), pc.dist(pb));
        // The angle between the lines at the corner.
        let inside = da.cross(db).len().atan2(da.dot(db));
        if inside < 1e-6 || inside > std::f64::consts::PI - 1e-6 {
            return Err(invalid("The two lines lie along one line: there is no corner to round."));
        }
        let back = radius / (inside / 2.0).tan();
        let room = la.min(lb);
        if back >= room - tol::MIN_SIZE {
            let most = room * (inside / 2.0).tan();
            return Err(invalid(format!(
                "A bend of radius {radius} mm needs {back:.3} mm of each line and the shorter is {room:.3} mm long: the largest radius that fits is just under {most:.3} mm."
            )));
        }
        let center = pc + (da + db).normalized() * (radius / (inside / 2.0).sin());
        let (ps, pe) = (pc + da * back, pc + db * back);
        let (start, end, cp) = (s.add_point(ps)?, s.add_point(pe)?, s.add_point(center)?);
        for (line, end_of_it) in [(a, start), (b, end)] {
            if let Some(Geometry3::Line { start: p, end: q }) = s.entities.get_mut(&line).map(|e| &mut e.geometry) {
                if *p == corner {
                    *p = end_of_it;
                } else if *q == corner {
                    *q = end_of_it;
                }
            }
        }
        // A length that was up to the corner stays up to the corner.
        for (line, far_end) in [(a, far_a), (b, far_b)] {
            for c in s.constraints.values_mut() {
                if let Constraint3::Length { line: l, value } = c
                    && *l == line
                {
                    *c = Constraint3::Distance { a: far_end, b: corner, value: *value };
                }
            }
        }
        let arc = s.insert(Geometry3::Arc { center: cp, start, end })?;
        s.set_construction(corner, true)?;
        s.set_construction(cp, true)?;
        s.push_constraint(Constraint3::Corner { point: corner, arc })?;
        s.push_constraint(Constraint3::Tangent { a, b: arc })?;
        s.push_constraint(Constraint3::Tangent { a: b, b: arc })?;
        let dim = s.push_constraint(Constraint3::Radius { arc, value: radius })?;
        s.solve().map_err(|e| invalid(format!("The bend cannot be had with what holds the sketch: {}", words_of(&s, &e))))?;
        *self = s;
        Ok(Bend3 { arc, center: cp, start, end, corner, radius: dim })
    }

    // ---- constraints ------------------------------------------------------------------------

    /// Checks that a constraint's references have the right types and its value is sane.
    pub fn check_constraint(&self, c: &Constraint3) -> SketchResult<()> {
        use Constraint3::*;
        for r in c.refs() {
            if self.entity(r).is_none() {
                return Err(SketchError::NoEntity(r));
            }
        }
        let (pt, ln, ar) = (|i| self.is_point(i), |i| self.is_line(i), |i| self.is_arc(i));
        let wrong = |what: &str| Err(invalid(what.to_owned()));
        match c {
            Coincident { a, b } if !(pt(*a) && pt(*b) && a != b) => return wrong("Coincident takes two points."),
            Fix { point } if !pt(*point) => return wrong("Fix takes a point."),
            Along { line, .. } if !ln(*line) => return wrong("Parallel to X, Y or Z takes a line."),
            Parallel { a, b } if !(ln(*a) && ln(*b) && a != b) => return wrong("Parallel takes two lines."),
            Perpendicular { a, b } if !(ln(*a) && ln(*b) && a != b) => return wrong("Perpendicular takes two lines."),
            Equal { a, b } if !(a != b && ((ln(*a) && ln(*b)) || (ar(*a) && ar(*b)))) => return wrong("Equal takes two lines, or two arcs."),
            Tangent { a, b } => {
                let curved = |i: EntityId| self.is_curve(i) && !ln(i);
                if a == b || !(self.is_curve(*a) && self.is_curve(*b)) || !(curved(*a) || curved(*b)) {
                    return wrong("Tangent takes a curve and the curve it runs on from: two lines are Parallel.");
                }
                if self.joined_ends(*a, *b).is_none() {
                    return wrong("Tangent needs the two curves to meet at an end: make their ends coincident first.");
                }
            }
            Corner { point, arc } if !(pt(*point) && ar(*arc)) => return wrong("A bend's corner is a point and its arc."),
            Length { line, .. } if !ln(*line) => return wrong("A length is of a line."),
            Distance { a, b, .. } if !(pt(*a) && pt(*b) && a != b) => return wrong("A distance is between two points."),
            Coordinate { point, .. } if !pt(*point) => return wrong("A coordinate is of a point."),
            Radius { arc, .. } if !ar(*arc) => return wrong("A radius is of an arc."),
            _ => {}
        }
        if let Some(v) = c.value() {
            let fine = match c {
                Coordinate { .. } => tol::is_valid_coord(v),
                _ => tol::is_valid_size(v),
            };
            if !fine {
                return Err(invalid(format!("{} cannot be {v}", c.name())));
            }
        }
        Ok(())
    }

    /// Adds a constraint without solving (the geometry is assumed to satisfy it).
    pub fn push_constraint(&mut self, c: Constraint3) -> SketchResult<ConstraintId> {
        self.check_constraint(&c)?;
        if self.constraints.len() >= MAX_CONSTRAINTS {
            return Err(SketchError::TooMany);
        }
        self.next_constraint = self.next_constraint.checked_add(1).ok_or(SketchError::TooMany)?;
        let id = ConstraintId(self.next_constraint);
        self.constraints.insert(id, c);
        Ok(id)
    }

    /// Adds a constraint and solves. It is refused, in words and with the sketch unchanged, when
    /// it conflicts with the others or adds nothing to them: over-constraining is not let happen.
    pub fn add_constraint(&mut self, c: Constraint3) -> SketchResult<ConstraintId> {
        let mut trial = self.clone();
        let said = c.describe();
        let id = trial.push_constraint(c)?;
        let others = |sk: &Sketch3d, ids: &[ConstraintId]| -> String {
            let named: Vec<String> = ids.iter().filter(|i| **i != id).filter_map(|i| sk.constraint(*i)).map(Constraint3::describe).collect();
            if named.is_empty() { String::new() } else { format!(" It conflicts with: {}.", named.join("; ")) }
        };
        match trial.solve() {
            Ok(()) => {}
            Err(SketchError::Conflict(ids)) => {
                return Err(invalid(format!(
                    "This would over-constrain the sketch: {said} cannot be had with what already holds it.{} Delete one of them, or change its value.",
                    others(&trial, &ids)
                )));
            }
            Err(e) => return Err(e),
        }
        if let Err(SketchError::Redundant(ids)) = trial.check_redundant(id) {
            let named: Vec<String> = ids.iter().filter_map(|i| trial.constraint(*i)).map(Constraint3::describe).collect();
            let held = if named.is_empty() { "what is there".to_owned() } else { named.join("; ") };
            return Err(invalid(format!(
                "This would over-constrain the sketch: {said} is already so, by {held}. Delete one of them first if it is to be held this way."
            )));
        }
        *self = trial;
        Ok(id)
    }

    pub fn remove_constraint(&mut self, id: ConstraintId) -> SketchResult<Constraint3> {
        self.constraints.remove(&id).ok_or(SketchError::NoConstraint(id))
    }

    /// Changes a dimension and solves; unchanged on failure.
    pub fn set_dimension(&mut self, id: ConstraintId, value: f64) -> SketchResult<()> {
        self.set_dimensions(&[(id, value)])
    }

    /// Changes several dimensions and solves once; unchanged on failure.
    pub fn set_dimensions(&mut self, values: &[(ConstraintId, f64)]) -> SketchResult<()> {
        let mut trial = self.clone();
        for (id, value) in values {
            let c = trial.constraints.get_mut(id).ok_or(SketchError::NoConstraint(*id))?;
            if !c.set_value(*value) {
                return Err(invalid(format!("{id} is not a dimension")));
            }
            let c = c.clone();
            trial.check_constraint(&c)?;
        }
        // A bend's radius changed: what it rounds stays where it is, if that can be.
        let held: Vec<EntityId> = values
            .iter()
            .filter_map(|(id, _)| match trial.constraint(*id) {
                Some(Constraint3::Radius { arc, .. }) => Some(trial.bend_holds(*arc)),
                _ => None,
            })
            .flatten()
            .collect();
        let pinned = !held.is_empty() && {
            let mut still = trial.clone();
            let fine = still.solve_pinned(&held).is_ok();
            if fine {
                trial = still;
            }
            fine
        };
        if !pinned {
            trial.solve().map_err(|e| invalid(format!("The sketch cannot take that value: {}", words_of(&trial, &e))))?;
        }
        *self = trial;
        Ok(())
    }

    /// The points a bend depends on: its corner, and the far ends of the lines tangent to it.
    fn bend_holds(&self, arc: EntityId) -> Vec<EntityId> {
        let mut out = Vec::new();
        for c in self.constraints.values() {
            match c {
                Constraint3::Corner { point, arc: a } if *a == arc => out.push(*point),
                Constraint3::Tangent { a, b } if *a == arc || *b == arc => {
                    let line = if *a == arc { *b } else { *a };
                    let Some((p, q)) = self.geometry(line).and_then(Geometry3::ends) else { continue };
                    let far = if self.joined_ends(line, arc).is_some_and(|(at, _)| at == p) { q } else { p };
                    out.push(far);
                }
                _ => {}
            }
        }
        out
    }

    // ---- solving ----------------------------------------------------------------------------

    /// Solves every constraint and writes the geometry back; unchanged on failure.
    pub fn solve(&mut self) -> SketchResult<()> {
        self.solve_with(None, &[])
    }

    /// Moves a point towards `target`, keeping every constraint; unchanged on failure.
    pub fn drag(&mut self, point: EntityId, target: Vec3) -> SketchResult<()> {
        self.solve_with(Some((point, target)), &[])
    }

    /// Solves, with `pin` held as they are for this solve.
    pub fn solve_pinned(&mut self, pin: &[EntityId]) -> SketchResult<()> {
        self.solve_with(None, pin)
    }

    fn solve_with(&mut self, drag: Option<(EntityId, Vec3)>, pin: &[EntityId]) -> SketchResult<()> {
        let (point_at, x) = {
            let mut sys = System3::new(self)?;
            for id in pin {
                if let Some(&o) = sys.point_at.get(id) {
                    sys.lock(o);
                }
            }
            let mut x0 = sys.x0.clone();
            let mut weights = vec![1.0; x0.len()];
            let mut pinned = None;
            if let Some((id, target)) = drag {
                let o = *sys.point_at.get(&id).ok_or_else(|| invalid(format!("{id} is not a point")))?;
                if !valid_pos(target) {
                    return Err(invalid("a point cannot be moved out of range"));
                }
                // A point that is held (fixed, or reference geometry) is not for dragging.
                if sys.locked.get(o).copied().unwrap_or(false) {
                    return Ok(());
                }
                x0[o] = target.x;
                x0[o + 1] = target.y;
                x0[o + 2] = target.z;
                for j in o..o + 3 {
                    weights[j] = DRAG_WEIGHT;
                }
                // It goes exactly where the pointer is whenever the constraints allow: first
                // solve with it pinned there, and only if that fails let it trail behind.
                let mut held = sys.shallow();
                held.lock(o);
                pinned = held.run(&x0, &weights, None).ok().filter(|r| r.converged);
            }
            let o = match pinned {
                Some(o) => o,
                None => sys.run(&x0, &weights, None)?,
            };
            if !o.converged {
                return Err(SketchError::Conflict(sys.conflicts(&x0, &weights, &o.x)));
            }
            (sys.point_at.clone(), o.x)
        };
        let mut next = self.clone();
        for (id, &o) in &point_at {
            if let (Some(&px), Some(&py), Some(&pz)) = (x.get(o), x.get(o + 1), x.get(o + 2)) {
                next.set_point(*id, Vec3::new(px, py, pz))?;
            }
        }
        // (A solver is content to have a line's ends at one place: nothing then is left of it.)
        if let Some(gone) = next.entities.keys().copied().find(|id| next.collapsed(*id)) {
            let on: Vec<ConstraintId> = next.constraints_on(gone);
            return Err(SketchError::Conflict(on));
        }
        *self = next;
        Ok(())
    }

    /// A curve with nothing left of it.
    fn collapsed(&self, id: EntityId) -> bool {
        match self.geometry(id) {
            Some(Geometry3::Line { .. }) => self.line(id).is_some_and(|(a, b)| a.dist(b) <= tol::MIN_SIZE),
            Some(Geometry3::Arc { .. }) => self.arc(id).is_none(),
            Some(Geometry3::Spline { .. }) => self.spline(id).is_none(),
            _ => false,
        }
    }

    /// Degrees of freedom at the current geometry.
    pub fn dof(&self) -> SketchResult<Dof> {
        let sys = System3::new(self)?;
        let (jac, rows, free) = sys.jacobian(&sys.x0);
        let rank = lm::rank(&jac, rows, free.len());
        let det = determined_columns(&jac, rows, free.len());
        let mut fixed = sys.locked.clone();
        for (k, &j) in free.iter().enumerate() {
            if det.get(k).copied().unwrap_or(false)
                && let Some(f) = fixed.get_mut(j)
            {
                *f = true;
            }
        }
        let point_fixed = |id: &EntityId| sys.point_at.get(id).is_some_and(|o| (*o..*o + 3).all(|j| fixed.get(j).copied().unwrap_or(false)));
        let mut fully = BTreeSet::new();
        for (id, e) in self.entities() {
            let done = match &e.geometry {
                Geometry3::Point { .. } => point_fixed(&id),
                g => g.points().iter().all(point_fixed),
            };
            if done {
                fully.insert(id);
            }
        }
        Ok(Dof { dof: free.len().saturating_sub(rank), fully_constrained: fully })
    }

    /// Error if `new` adds no independent equation to the others.
    pub fn check_redundant(&self, new: ConstraintId) -> SketchResult<()> {
        let sys = System3::new(self)?;
        // (A Fix adds no rows: it is redundant when something already holds its point whole.)
        if let Some(Constraint3::Fix { point }) = self.constraint(new) {
            let mut without = self.clone();
            without.constraints.remove(&new);
            if without.dof()?.fully_constrained.contains(point) {
                return Err(SketchError::Redundant(self.constraints_on(*point).into_iter().filter(|c| *c != new).collect()));
            }
            return Ok(());
        }
        let Some(pos) = sys.built.iter().position(|b| b.id == new) else { return Ok(()) };
        let nb = &sys.built[pos];
        let free = sys.free();
        let rows_all = sys.rows(None);
        let rows_old = rows_all - nb.rows;
        let f_old = |x: &[f64], out: &mut Vec<f64>| sys.eval(x, out, Some(pos));
        let j_old = lm::jacobian(&f_old, &sys.x0, &free, rows_old);
        let (j_all, _, _) = sys.jacobian(&sys.x0);
        let (r_old, r_all) = (lm::rank(&j_old, rows_old, free.len()), lm::rank(&j_all, rows_all, free.len()));
        if r_all < r_old + nb.rows {
            let mine = sys.touched(&nb.c);
            let related = sys.built.iter().filter(|b| b.id != new && sys.touched(&b.c).iter().any(|r| mine.contains(r))).map(|b| b.id).collect();
            return Err(SketchError::Redundant(related));
        }
        Ok(())
    }

    // ---- deleting ---------------------------------------------------------------------------

    /// Deletes entities. Deleting a point deletes the curves that use it; points left unused by
    /// any curve after deleting a curve go too, and constraints on anything that goes. A bend
    /// deleted gives its corner back: its two lines meet at the corner again. Returns what went.
    pub fn delete(&mut self, ids: &[EntityId]) -> BTreeSet<EntityId> {
        let mut gone: BTreeSet<EntityId> = BTreeSet::new();
        let mut rest = Vec::new();
        for id in ids {
            match self.unbend(*id) {
                Some(went) => gone.extend(went),
                None => rest.push(*id),
            }
        }
        let mut now: BTreeSet<EntityId> = rest.iter().copied().filter(|i| self.entities.contains_key(i)).collect();
        let dependent: Vec<EntityId> =
            self.entities.iter().filter(|(_, e)| e.geometry.points().iter().any(|p| now.contains(p))).map(|(id, _)| *id).collect();
        now.extend(dependent);
        let candidates: BTreeSet<EntityId> = now.iter().filter_map(|id| self.geometry(*id)).flat_map(Geometry3::points).collect();
        for p in candidates {
            let still_used = self.entities.iter().any(|(id, e)| !now.contains(id) && e.geometry.points().contains(&p));
            if !still_used {
                now.insert(p);
            }
        }
        for id in &now {
            self.entities.remove(id);
        }
        self.constraints.retain(|_, c| c.refs().iter().all(|r| !now.contains(r)));
        gone.extend(now);
        gone
    }

    /// If `arc` is a bend: takes it away and joins its two lines at the corner again. Returns
    /// what went.
    fn unbend(&mut self, arc: EntityId) -> Option<BTreeSet<EntityId>> {
        let Geometry3::Arc { center, start, end } = self.geometry(arc)?.clone() else { return None };
        let corner = self.constraints.values().find_map(|c| match c {
            Constraint3::Corner { point, arc: a } if *a == arc => Some(*point),
            _ => None,
        })?;
        let lines: Vec<(EntityId, EntityId)> =
            [start, end].into_iter().flat_map(|p| self.curves_at(p).into_iter().filter(|c| *c != arc).map(move |c| (c, p))).collect();
        for (line, at) in lines {
            if let Some(Geometry3::Line { start: p, end: q }) = self.entities.get_mut(&line).map(|e| &mut e.geometry) {
                if *p == at {
                    *p = corner;
                } else if *q == at {
                    *q = corner;
                }
            }
        }
        let gone: BTreeSet<EntityId> = [arc, center, start, end].into_iter().collect();
        for id in &gone {
            self.entities.remove(id);
        }
        self.constraints.retain(|_, c| c.refs().iter().all(|r| !gone.contains(r)));
        if let Some(e) = self.entities.get_mut(&corner) {
            e.construction = false;
        }
        Some(gone)
    }

    // ---- a path -----------------------------------------------------------------------------

    /// The curves in the order they join, each with whether it runs from its start to its end,
    /// from a free end (an open path) or anywhere (a closed one).
    pub fn chain(&self, curves: &[EntityId]) -> Result<Vec<(EntityId, bool)>, String> {
        // Each point stands for all those held coincident with it.
        let mut stands: HashMap<EntityId, EntityId> = HashMap::new();
        fn root(stands: &HashMap<EntityId, EntityId>, mut p: EntityId) -> EntityId {
            while let Some(q) = stands.get(&p) {
                p = *q;
            }
            p
        }
        for c in self.constraints.values() {
            if let Constraint3::Coincident { a, b } = c {
                let (ra, rb) = (root(&stands, *a), root(&stands, *b));
                if ra != rb {
                    stands.insert(ra, rb);
                }
            }
        }
        let mut segs = Vec::new();
        for id in curves {
            match self.geometry(*id).and_then(Geometry3::ends) {
                Some((s, e)) => segs.push((*id, root(&stands, s), root(&stands, e))),
                None => return Err(format!("the path's {id} is no curve of the sketch")),
            }
        }
        if segs.is_empty() {
            return Err("the path is empty".into());
        }
        let ends = |p: EntityId| segs.iter().map(|s| usize::from(s.1 == p) + usize::from(s.2 == p)).sum::<usize>();
        let first = segs.iter().position(|s| ends(s.1) == 1 || ends(s.2) == 1).unwrap_or(0);
        let (s0, e0) = (segs[first].1, segs[first].2);
        let mut at = if ends(e0) == 1 && ends(s0) != 1 { e0 } else { s0 };
        let mut left = segs.clone();
        let mut chain = Vec::with_capacity(segs.len());
        while !left.is_empty() {
            let Some(i) = left.iter().position(|s| s.1 == at || s.2 == at) else {
                return Err("the curves of the path do not join end to end: make them meet, or choose fewer".into());
            };
            let (id, a, b) = left.remove(i);
            let forward = a == at;
            at = if forward { b } else { a };
            chain.push((id, forward));
        }
        Ok(chain)
    }

    /// One curve as the kernel has it, from its start to its end.
    fn pieces(&self, id: EntityId) -> Option<Vec<Curve3>> {
        Some(match self.geometry(id)? {
            Geometry3::Point { .. } => return None,
            Geometry3::Line { .. } => {
                let (start, end) = self.line(id)?;
                vec![Curve3::Line { start, end }]
            }
            Geometry3::Arc { .. } => {
                let a = self.arc(id)?;
                vec![Curve3::Arc { start: a.at(0.0), mid: a.at(0.5), end: a.at(1.0) }]
            }
            Geometry3::Spline { .. } => {
                let (poles, knots) = self.spline(id)?.b_spline();
                vec![Curve3::BSpline { poles, knots }]
            }
            Geometry3::Edge { start, end, curve } => {
                let (s, e) = (self.point(*start)?, self.point(*end)?);
                match curve.mid {
                    // A whole circle: two half circles.
                    Some(mid) if s.dist(e) <= tol::MIN_SIZE => {
                        let n = curve.along.len().saturating_sub(1);
                        let (q1, q3) = (*curve.along.get(n / 4)?, *curve.along.get(3 * n / 4)?);
                        vec![Curve3::Arc { start: s, mid: q1, end: mid }, Curve3::Arc { start: mid, mid: q3, end: s }]
                    }
                    Some(mid) => vec![Curve3::Arc { start: s, mid, end: e }],
                    // Any other curve: a spline through the points along it.
                    None => {
                        let (poles, knots) = Through::new(&curve.along)?.b_spline();
                        vec![Curve3::BSpline { poles, knots }]
                    }
                }
            }
        })
    }

    /// A sweep's path: `curves`, joined end to end, starting at whichever end of the chain is
    /// nearer `near` (the profile).
    pub fn path(&self, curves: &[EntityId], near: Vec3) -> Result<Path3, String> {
        let mut out: Vec<Curve3> = Vec::new();
        for (id, forward) in self.chain(curves)? {
            let pieces = self.pieces(id).ok_or_else(|| format!("{id} cannot be part of a path: it has collapsed"))?;
            if forward {
                out.extend(pieces);
            } else {
                out.extend(pieces.into_iter().rev().map(reversed));
            }
        }
        let start_of = |c: &Curve3| match c {
            Curve3::Line { start, .. } | Curve3::Arc { start, .. } => Some(*start),
            Curve3::BSpline { poles, .. } => poles.first().copied(),
            Curve3::Helix { .. } => None,
        };
        let end_of = |c: &Curve3| match c {
            Curve3::Line { end, .. } | Curve3::Arc { end, .. } => Some(*end),
            Curve3::BSpline { poles, .. } => poles.last().copied(),
            Curve3::Helix { .. } => None,
        };
        let (Some(head), Some(tail)) = (out.first().and_then(start_of), out.last().and_then(end_of)) else {
            return Err("the path is empty".into());
        };
        if tail.dist(near) < head.dist(near) {
            out.reverse();
            out = out.into_iter().map(reversed).collect();
        }
        Ok(Path3 { curves: out })
    }

    // ---- integrity --------------------------------------------------------------------------

    /// Checks references and values, e.g. after loading a file.
    pub fn validate(&self) -> SketchResult<()> {
        if self.entities.len() > MAX_ENTITIES || self.constraints.len() > MAX_CONSTRAINTS {
            return Err(SketchError::TooMany);
        }
        for (id, e) in &self.entities {
            if id.0 > self.next_entity {
                return Err(invalid(format!("{id} is beyond the id counter")));
            }
            match &e.geometry {
                Geometry3::Point { pos } if !valid_pos(*pos) => return Err(invalid(format!("{id} has an invalid position"))),
                Geometry3::Spline { points } if points.len() < 2 || points.len() > MAX_SPLINE_POINTS => {
                    return Err(invalid(format!("{id} is a spline of {} points: it takes 2 to {MAX_SPLINE_POINTS}", points.len())));
                }
                Geometry3::Edge { curve, .. } => check_edge(curve).map_err(|err| invalid(format!("{id}: {err}")))?,
                _ => {}
            }
            for p in e.geometry.points() {
                if !self.is_point(p) {
                    return Err(invalid(format!("{id} refers to {p}, which is not a point")));
                }
            }
        }
        for (id, c) in &self.constraints {
            if id.0 > self.next_constraint {
                return Err(invalid(format!("{id} is beyond the id counter")));
            }
            self.check_constraint(c).map_err(|e| invalid(format!("{id}: {e}")))?;
        }
        Ok(())
    }
}

fn check_edge(curve: &EdgeCurve) -> SketchResult<()> {
    if curve.along.len() < 2 || curve.along.len() > MAX_EDGE_POINTS {
        return Err(invalid(format!("an edge is drawn through 2 to {MAX_EDGE_POINTS} points, not {}", curve.along.len())));
    }
    let fine = curve.along.iter().chain(curve.mid.iter()).all(|p| valid_pos(*p));
    if !fine || !(curve.length.is_finite() && curve.length >= 0.0) {
        return Err(invalid("an edge has a point or a length out of range"));
    }
    Ok(())
}

/// A path curve run the other way.
fn reversed(c: Curve3) -> Curve3 {
    match c {
        Curve3::Line { start, end } => Curve3::Line { start: end, end: start },
        Curve3::Arc { start, mid, end } => Curve3::Arc { start: end, mid, end: start },
        Curve3::BSpline { mut poles, knots } => {
            poles.reverse();
            let last = knots.last().copied().unwrap_or(0.0);
            let first = knots.first().copied().unwrap_or(0.0);
            Curve3::BSpline { poles, knots: knots.iter().rev().map(|k| first + last - k).collect() }
        }
        h @ Curve3::Helix { .. } => h,
    }
}

/// What a solve said, in words.
fn words_of(sk: &Sketch3d, e: &SketchError) -> String {
    match e {
        SketchError::Conflict(ids) | SketchError::Redundant(ids) => {
            let named: Vec<String> = ids.iter().filter_map(|i| sk.constraint(*i)).map(Constraint3::describe).collect();
            if named.is_empty() { "the constraints cannot all be met".to_owned() } else { format!("these cannot all be met: {}", named.join("; ")) }
        }
        e => e.to_string(),
    }
}

/// A constraint prepared for evaluation.
#[derive(Clone, Debug)]
struct Built {
    id: ConstraintId,
    c: Constraint3,
    rows: usize,
    /// For Tangent: the ends where the curves join.
    pair: Option<(EntityId, EntityId)>,
    /// For rows square to a direction: the world axis furthest from it as things stood, which
    /// gives the two directions square to it.
    away: Vec3,
}

/// Parameter layout and residual system of one sketch.
struct System3<'s> {
    sketch: &'s Sketch3d,
    point_at: HashMap<EntityId, usize>,
    x0: Vec<f64>,
    arcs: Vec<(EntityId, EntityId, EntityId)>,
    built: Vec<Built>,
    locked: Vec<bool>,
}

/// How far `v` is from lying along `u`, as two numbers (`away` is not along `u`).
fn square_rows(u: Vec3, v: Vec3, away: Vec3) -> [f64; 2] {
    let e1 = u.cross(away).normalized();
    let e2 = u.cross(e1);
    [v.dot(e1), v.dot(e2)]
}

/// The world axis furthest from `u`.
fn axis_away_from(u: Vec3) -> Vec3 {
    let (x, y, z) = (u.x.abs(), u.y.abs(), u.z.abs());
    if x <= y && x <= z {
        Vec3::X
    } else if y <= z {
        Vec3::Y
    } else {
        Vec3::Z
    }
}

impl<'s> System3<'s> {
    fn new(sketch: &'s Sketch3d) -> SketchResult<Self> {
        let mut sys = System3 { sketch, point_at: HashMap::new(), x0: Vec::new(), arcs: Vec::new(), built: Vec::new(), locked: Vec::new() };
        for (id, e) in sketch.entities() {
            match &e.geometry {
                Geometry3::Point { pos } => {
                    sys.point_at.insert(id, sys.x0.len());
                    sys.x0.extend([pos.x, pos.y, pos.z]);
                }
                Geometry3::Arc { center, start, end } => sys.arcs.push((*center, *start, *end)),
                _ => {}
            }
        }
        sys.locked = vec![false; sys.x0.len()];
        for (id, e) in sketch.entities().filter(|(_, e)| e.reference && e.geometry.is_point()) {
            let _ = e;
            if let Some(&o) = sys.point_at.get(&id) {
                sys.lock(o);
            }
        }
        for (id, c) in sketch.constraints() {
            sketch.check_constraint(c)?;
            if let Constraint3::Fix { point } = c {
                if let Some(&o) = sys.point_at.get(point) {
                    sys.lock(o);
                }
                continue;
            }
            let b = sys.build(id, c.clone());
            sys.built.push(b);
        }
        Ok(sys)
    }

    fn lock(&mut self, o: usize) {
        for j in o..o + 3 {
            if let Some(l) = self.locked.get_mut(j) {
                *l = true;
            }
        }
    }

    /// A copy sharing the sketch, for solving variants (with a point held).
    fn shallow(&self) -> System3<'s> {
        System3 {
            sketch: self.sketch,
            point_at: self.point_at.clone(),
            x0: self.x0.clone(),
            arcs: self.arcs.clone(),
            built: self.built.clone(),
            locked: self.locked.clone(),
        }
    }

    fn pt(&self, x: &[f64], id: EntityId) -> Option<Vec3> {
        let o = *self.point_at.get(&id)?;
        Some(Vec3::new(*x.get(o)?, *x.get(o + 1)?, *x.get(o + 2)?))
    }

    fn line(&self, x: &[f64], id: EntityId) -> Option<(Vec3, Vec3)> {
        match self.sketch.geometry(id)? {
            Geometry3::Line { start, end } => Some((self.pt(x, *start)?, self.pt(x, *end)?)),
            _ => None,
        }
    }

    /// A curve's direction at its end `at` (whichever way round).
    fn end_direction(&self, x: &[f64], id: EntityId, at: EntityId) -> Option<Vec3> {
        match self.sketch.geometry(id)? {
            Geometry3::Line { .. } => {
                let (a, b) = self.line(x, id)?;
                Some(dir_n(a, b))
            }
            Geometry3::Arc { center, start, end } => {
                let (c, s, e, j) = (self.pt(x, *center)?, self.pt(x, *start)?, self.pt(x, *end)?, self.pt(x, at)?);
                let n = (s - c).cross(e - c);
                Some(n.cross(j - c).normalized())
            }
            Geometry3::Spline { points } => {
                let pts: Vec<Vec3> = points.iter().map(|p| self.pt(x, *p)).collect::<Option<_>>()?;
                let s = Through::new(&pts)?;
                Some(if points.first() == Some(&at) { s.start_tangent() } else { s.end_tangent() })
            }
            Geometry3::Edge { start, curve, .. } => {
                // (Where the part has it: it is no unknown.)
                let from_start = *start == at;
                match (curve.along.first(), curve.along.get(1), curve.along.last(), curve.along.len().checked_sub(2).and_then(|i| curve.along.get(i)))
                {
                    (Some(a), Some(b), _, _) if from_start => Some(dir_n(*a, *b)),
                    (_, _, Some(a), Some(b)) => Some(dir_n(*b, *a)),
                    _ => None,
                }
            }
            Geometry3::Point { .. } => None,
        }
    }

    /// Points whose unknowns a constraint's equations take in.
    fn touched(&self, c: &Constraint3) -> Vec<EntityId> {
        let mut out = Vec::new();
        for r in c.refs() {
            match self.sketch.geometry(r) {
                Some(Geometry3::Point { .. }) => out.push(r),
                Some(g) => out.extend(g.points()),
                None => {}
            }
        }
        out
    }

    fn build(&self, id: ConstraintId, c: Constraint3) -> Built {
        use Constraint3::*;
        let rows = match &c {
            Coincident { .. } | Corner { .. } => 3,
            Along { .. } | Parallel { .. } | Tangent { .. } => 2,
            Fix { .. } => 0,
            _ => 1,
        };
        let x = &self.x0;
        let (mut pair, mut away) = (None, Vec3::X);
        match &c {
            Parallel { a, .. } => {
                if let Some((p, q)) = self.line(x, *a) {
                    away = axis_away_from(dir_n(p, q));
                }
            }
            Tangent { a, b } => {
                pair = self.sketch.joined_ends(*a, *b);
                if let Some(u) = pair.and_then(|(ea, _)| self.end_direction(x, *a, ea)) {
                    away = axis_away_from(u);
                }
            }
            _ => {}
        }
        Built { id, c, rows, pair, away }
    }

    /// Appends exactly `b.rows` residuals.
    fn eval_one(&self, b: &Built, x: &[f64], out: &mut Vec<f64>) {
        let start = out.len();
        let _ = self.eval_inner(b, x, out);
        out.resize(start + b.rows, 0.0);
    }

    fn eval_inner(&self, b: &Built, x: &[f64], out: &mut Vec<f64>) -> Option<()> {
        use Constraint3::*;
        let pt = |id| self.pt(x, id);
        let ln = |id| self.line(x, id);
        let radius = |id: EntityId| -> Option<f64> {
            match self.sketch.geometry(id)? {
                Geometry3::Arc { center, start, .. } => Some(pt(*center)?.dist(pt(*start)?)),
                _ => None,
            }
        };
        match &b.c {
            Coincident { a, b } => {
                let (p, q) = (pt(*a)?, pt(*b)?);
                out.extend([p.x - q.x, p.y - q.y, p.z - q.z]);
            }
            Fix { .. } => {}
            Along { line, axis } => {
                let (p, q) = ln(*line)?;
                let d = q - p;
                out.extend(match axis {
                    Axis3::X => [d.y, d.z],
                    Axis3::Y => [d.x, d.z],
                    Axis3::Z => [d.x, d.y],
                });
            }
            Parallel { a, b: bb } => {
                let ((a1, b1), (a2, b2)) = (ln(*a)?, ln(*bb)?);
                out.extend(square_rows(dir_n(a1, b1), dir_n(a2, b2), b.away));
            }
            Perpendicular { a, b } => {
                let ((a1, b1), (a2, b2)) = (ln(*a)?, ln(*b)?);
                out.push(dir_n(a1, b1).dot(dir_n(a2, b2)));
            }
            Equal { a, b } => match (ln(*a), ln(*b)) {
                (Some((a1, b1)), Some((a2, b2))) => out.push(a1.dist(b1) - a2.dist(b2)),
                _ => out.push(radius(*a)? - radius(*b)?),
            },
            Tangent { a, b: bb } => {
                let (ea, eb) = b.pair?;
                let (u, v) = (self.end_direction(x, *a, ea)?, self.end_direction(x, *bb, eb)?);
                out.extend(square_rows(u, v, b.away));
            }
            Corner { point, arc } => {
                let Geometry3::Arc { center, start, end } = self.sketch.geometry(*arc)? else { return None };
                let (p, c, s, e) = (pt(*point)?, pt(*center)?, pt(*start)?, pt(*end)?);
                // Where the tangents at the ends of an arc meet: from the centre, along the sum
                // of the two radii, by this much.
                let (ra, rb) = (s - c, e - c);
                let cos = ra.dot(rb) / (ra.len() * rb.len()).max(1e-12);
                let at = c + (ra + rb) * (1.0 / (1.0 + cos).max(1e-9));
                out.extend([p.x - at.x, p.y - at.y, p.z - at.z]);
            }
            Length { line, value } => {
                let (p, q) = ln(*line)?;
                out.push(p.dist(q) - value);
            }
            Distance { a, b, value } => out.push(pt(*a)?.dist(pt(*b)?) - value),
            Coordinate { point, axis, value } => out.push(axis.of(pt(*point)?) - value),
            Radius { arc, value } => out.push(radius(*arc)? - value),
        }
        Some(())
    }

    /// All residuals, skipping constraint index `skip`. Arc rows come first.
    fn eval(&self, x: &[f64], out: &mut Vec<f64>, skip: Option<usize>) {
        for (c, s, e) in &self.arcs {
            let v = match (self.pt(x, *c), self.pt(x, *s), self.pt(x, *e)) {
                (Some(c), Some(s), Some(e)) => e.dist(c) - s.dist(c),
                _ => 0.0,
            };
            out.push(v);
        }
        for (i, b) in self.built.iter().enumerate() {
            if Some(i) != skip {
                self.eval_one(b, x, out);
            }
        }
    }

    fn rows(&self, skip: Option<usize>) -> usize {
        self.arcs.len() + self.built.iter().enumerate().filter(|(i, _)| Some(*i) != skip).map(|(_, b)| b.rows).sum::<usize>()
    }

    fn free(&self) -> Vec<usize> {
        self.locked.iter().enumerate().filter(|(_, l)| !**l).map(|(i, _)| i).collect()
    }

    /// Convergence tolerance on residuals: relative to the sketch's size and well below the
    /// kernel's coincidence tolerance (`tol::LINEAR`), so solved joints stay joined.
    fn tolerance(&self) -> f64 {
        1e-11 * self.x0.iter().fold(1.0f64, |m, v| m.max(v.abs()))
    }

    fn run(&self, x0: &[f64], weights: &[f64], skip: Option<usize>) -> SketchResult<lm::Outcome> {
        let rows = self.rows(skip);
        let free = self.free();
        if rows > lm::MAX_ROWS || (rows as f64) * (rows as f64) * (free.len() as f64) > 4e8 {
            return Err(SketchError::TooLarge);
        }
        let f = |x: &[f64], out: &mut Vec<f64>| self.eval(x, out, skip);
        Ok(lm::solve(&f, x0, &free, weights, self.tolerance(), MAX_ITER))
    }

    /// Constraints whose removal makes the rest solvable.
    fn conflicts(&self, x0: &[f64], weights: &[f64], x_failed: &[f64]) -> Vec<ConstraintId> {
        let mut ids = Vec::new();
        if self.built.len() <= MAX_CONFLICT_PROBES {
            for (i, b) in self.built.iter().enumerate() {
                if self.run(x0, weights, Some(i)).is_ok_and(|o| o.converged) {
                    ids.push(b.id);
                }
            }
        }
        if ids.is_empty() {
            let tol = self.tolerance() * 10.0;
            for b in &self.built {
                let mut out = Vec::new();
                self.eval_one(b, x_failed, &mut out);
                if lm::max_abs(&out) > tol {
                    ids.push(b.id);
                }
            }
        }
        ids
    }

    fn jacobian(&self, x: &[f64]) -> (Vec<f64>, usize, Vec<usize>) {
        let rows = self.rows(None);
        let free = self.free();
        let f = |x: &[f64], out: &mut Vec<f64>| self.eval(x, out, None);
        (lm::jacobian(&f, x, &free, rows), rows, free)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use std::f64::consts::{FRAC_PI_2, PI};

    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    /// Three lines by way of (0,0,0), (0,0,50), (50,0,50) and (50,50,50).
    fn staircase() -> (Sketch3d, [EntityId; 3], [EntityId; 4]) {
        let mut s = Sketch3d::new();
        let p = [
            s.add_point(v(0.0, 0.0, 0.0)).unwrap(),
            s.add_point(v(0.0, 0.0, 50.0)).unwrap(),
            s.add_point(v(50.0, 0.0, 50.0)).unwrap(),
            s.add_point(v(50.0, 50.0, 50.0)).unwrap(),
        ];
        let l = [s.add_line(p[0], p[1]).unwrap(), s.add_line(p[1], p[2]).unwrap(), s.add_line(p[2], p[3]).unwrap()];
        (s, l, p)
    }

    #[test]
    fn four_points_and_three_lines_have_twelve_freedoms_and_each_constraint_takes_its_own() {
        let (mut s, l, p) = staircase();
        assert_eq!(s.dof().unwrap().dof, 12, "three each for four points");
        s.add_constraint(Constraint3::Fix { point: p[0] }).unwrap();
        assert_eq!(s.dof().unwrap().dof, 9);
        s.add_constraint(Constraint3::Along { line: l[0], axis: Axis3::Z }).unwrap();
        assert_eq!(s.dof().unwrap().dof, 7, "two for a line along an axis");
        s.add_constraint(Constraint3::Length { line: l[0], value: 30.0 }).unwrap();
        assert_eq!(s.dof().unwrap().dof, 6);
        assert!((s.point(p[1]).unwrap().z - 30.0).abs() < 1e-9, "{:?}", s.point(p[1]));
        s.add_constraint(Constraint3::Perpendicular { a: l[0], b: l[1] }).unwrap();
        assert_eq!(s.dof().unwrap().dof, 5);
        s.add_constraint(Constraint3::Coordinate { point: p[3], axis: Axis3::X, value: 7.0 }).unwrap();
        assert_eq!(s.dof().unwrap().dof, 4);
        assert!((s.point(p[3]).unwrap().x - 7.0).abs() < 1e-9);
        // Each line squared up, and the sketch held whole: none left. (Along X, the line is
        // square to the first of itself: that Perpendicular goes first, for this is the same.)
        let perpendicular = s.constraints().find(|(_, c)| matches!(c, Constraint3::Perpendicular { .. })).unwrap().0;
        s.remove_constraint(perpendicular).unwrap();
        for c in [
            Constraint3::Along { line: l[1], axis: Axis3::X },
            Constraint3::Distance { a: p[1], b: p[2], value: 20.0 },
            Constraint3::Coordinate { point: p[3], axis: Axis3::Y, value: 9.0 },
            Constraint3::Coordinate { point: p[3], axis: Axis3::Z, value: 11.0 },
        ] {
            s.add_constraint(c).unwrap();
        }
        assert_eq!(s.dof().unwrap().dof, 0, "{:#?}", s.constraints().collect::<Vec<_>>());
        let d = s.dof().unwrap();
        assert!(l.iter().all(|x| d.fully_constrained.contains(x)));
        // (It went away from the first point the way it was drawn.)
        assert!(s.point(p[2]).unwrap().dist(v(20.0, 0.0, 30.0)) < 1e-8, "{:?}", s.point(p[2]));
        assert!(s.point(p[3]).unwrap().dist(v(7.0, 9.0, 11.0)) < 1e-8, "{:?}", s.point(p[3]));
    }

    #[test]
    fn what_over_constrains_is_refused_in_words_and_leaves_the_sketch_as_it_was() {
        let (mut s, l, p) = staircase();
        s.add_constraint(Constraint3::Fix { point: p[0] }).unwrap();
        s.add_constraint(Constraint3::Along { line: l[0], axis: Axis3::Z }).unwrap();
        s.add_constraint(Constraint3::Length { line: l[0], value: 50.0 }).unwrap();
        let before = s.clone();
        // A second, different length of the line: it cannot be had.
        let e = s.add_constraint(Constraint3::Distance { a: p[0], b: p[1], value: 60.0 }).unwrap_err().to_string();
        assert!(e.starts_with("This would over-constrain the sketch"), "{e}");
        assert!(e.contains("mm long"), "it says what with: {e}");
        assert_eq!(s, before);
        // The same length again: nothing new.
        let e = s.add_constraint(Constraint3::Distance { a: p[0], b: p[1], value: 50.0 }).unwrap_err().to_string();
        assert!(e.contains("is already so"), "{e}");
        assert_eq!(s, before);
        // Along another axis too: there would be nothing left of the line.
        let e = s.add_constraint(Constraint3::Along { line: l[0], axis: Axis3::X }).unwrap_err().to_string();
        assert!(e.starts_with("This would over-constrain the sketch"), "{e}");
        assert_eq!(s, before);
        // A point fixed twice.
        let e = s.add_constraint(Constraint3::Fix { point: p[0] }).unwrap_err().to_string();
        assert!(e.contains("is already so"), "{e}");
        assert_eq!(s, before);
        // Words for things of the wrong kind.
        assert!(s.add_constraint(Constraint3::Parallel { a: p[0], b: l[0] }).unwrap_err().to_string().contains("takes two lines"));
    }

    #[test]
    fn a_bend_rounds_a_square_corner_tangent_to_both_lines_and_its_length_is_a_quarter_circle() {
        let (mut s, l, p) = staircase();
        assert_eq!(s.dof().unwrap().dof, 12);
        let b = s.add_bend(p[1], 10.0).unwrap();
        // The freedoms are as they were: the lines' four corners. The radius is held.
        assert_eq!(s.dof().unwrap().dof, 12, "{:?}", s.constraints().collect::<Vec<_>>());
        let arc = s.arc(b.arc).unwrap();
        assert!((arc.radius - 10.0).abs() < 1e-9, "{arc:?}");
        assert!((arc.sweep - FRAC_PI_2).abs() < 1e-9, "{arc:?}");
        assert!((s.length(b.arc).unwrap() - 5.0 * PI).abs() < 1e-9);
        // The lines stop 10 short of the corner, either side.
        assert!(s.line(l[0]).unwrap().1.dist(v(0.0, 0.0, 40.0)) < 1e-9);
        assert!(s.line(l[1]).unwrap().0.dist(v(10.0, 0.0, 50.0)) < 1e-9);
        assert!(s.point(b.center).unwrap().dist(v(10.0, 0.0, 40.0)) < 1e-9);
        // The corner is still there, to hold.
        assert!(s.point(b.corner).unwrap().dist(v(0.0, 0.0, 50.0)) < 1e-9);
        assert!(s.entity(b.corner).unwrap().construction);
        // A larger radius keeps the corner where it was and the lines through it.
        s.set_dimension(b.radius, 20.0).unwrap();
        assert!(s.point(b.corner).unwrap().dist(v(0.0, 0.0, 50.0)) < 1e-7, "{:?}", s.point(b.corner));
        assert!(s.line(l[0]).unwrap().1.dist(v(0.0, 0.0, 30.0)) < 1e-7, "{:?}", s.line(l[0]));
        assert!((s.length(b.arc).unwrap() - 10.0 * PI).abs() < 1e-7);
    }

    #[test]
    fn a_bend_too_large_for_its_lines_says_how_large_it_can_be() {
        let (mut s, _, p) = staircase();
        let e = s.add_bend(p[1], 60.0).unwrap_err().to_string();
        assert!(e.contains("the largest radius that fits is just under 50.000 mm"), "{e}");
        // Not at an end of two lines.
        let e = s.add_bend(p[0], 5.0).unwrap_err().to_string();
        assert!(e.contains("1 curves meet"), "{e}");
    }

    #[test]
    fn a_bend_deleted_gives_its_corner_back_and_a_length_held_stays_to_the_corner() {
        let (mut s, l, p) = staircase();
        let len = s.add_constraint(Constraint3::Length { line: l[0], value: 50.0 }).unwrap();
        s.add_constraint(Constraint3::Fix { point: p[0] }).unwrap();
        let b = s.add_bend(p[1], 10.0).unwrap();
        assert!(matches!(s.constraint(len), Some(Constraint3::Distance { value, .. }) if *value == 50.0), "{:?}", s.constraint(len));
        s.set_dimension(len, 70.0).unwrap();
        assert!((s.point(p[0]).unwrap().dist(s.point(b.corner).unwrap()) - 70.0).abs() < 1e-8);
        let gone = s.delete(&[b.arc]);
        assert_eq!(gone.len(), 4, "the arc, its centre and its two ends: {gone:?}");
        assert_eq!(s.geometry(l[0]).unwrap().ends().unwrap().1, b.corner);
        assert_eq!(s.geometry(l[1]).unwrap().ends().unwrap().0, b.corner);
        assert!(!s.entity(b.corner).unwrap().construction);
        assert_eq!(s.dof().unwrap().dof, 8, "{:?}", s.constraints().collect::<Vec<_>>());
        s.validate().unwrap();
    }

    #[test]
    fn a_bend_at_a_corner_held_only_by_coincident_joins_the_lines_first() {
        let mut s = Sketch3d::new();
        let a = s.add_line(v(0.0, 0.0, 0.0), v(30.0, 0.0, 0.0)).unwrap();
        let b = s.add_line(v(30.0, 0.0, 0.0), v(30.0, 0.0, 30.0)).unwrap();
        let (_, ea) = s.geometry(a).unwrap().ends().unwrap();
        let (sb, _) = s.geometry(b).unwrap().ends().unwrap();
        s.add_constraint(Constraint3::Coincident { a: ea, b: sb }).unwrap();
        let bend = s.add_bend(ea, 5.0).unwrap();
        assert!((s.arc(bend.arc).unwrap().sweep - FRAC_PI_2).abs() < 1e-9);
        assert!(s.point(sb).is_none(), "the point held to the corner has become it");
    }

    #[test]
    fn a_spline_goes_through_its_points_and_its_length_is_found_along_it() {
        // On a straight line: it is that line, and exactly as long.
        let mut s = Sketch3d::new();
        let pts: Vec<PointRef3> = [0.0, 10.0, 15.0, 40.0].iter().map(|t| PointRef3::New(v(*t, 2.0 * *t, -*t))).collect();
        let id = s.add_spline(&pts).unwrap();
        assert!((s.length(id).unwrap() - 40.0 * 6f64.sqrt()).abs() < 1e-9, "{}", s.length(id).unwrap());
        // Through 33 points on a quarter circle of radius 10: within a hair of its 5 pi.
        let mut q = Sketch3d::new();
        let on: Vec<PointRef3> = (0..=32)
            .map(|i| {
                let t = FRAC_PI_2 * f64::from(i) / 32.0;
                PointRef3::New(v(10.0 * t.cos(), 10.0 * t.sin(), 3.0))
            })
            .collect();
        let id = q.add_spline(&on).unwrap();
        assert!((q.length(id).unwrap() - 5.0 * PI).abs() < 1e-3, "{}", q.length(id).unwrap());
        // The curve as drawn passes through each point.
        let line = q.polyline(id);
        for (i, p) in on.iter().enumerate() {
            let PointRef3::New(at) = p else { panic!() };
            assert!(line[i * SPLINE_STEPS].dist(*at) < 1e-12);
        }
    }

    #[test]
    fn a_spline_as_a_b_spline_is_the_same_curve() {
        let t = Through::new(&[v(0.0, 0.0, 0.0), v(10.0, 5.0, 0.0), v(12.0, 20.0, 4.0), v(30.0, 20.0, 9.0), v(31.0, 2.0, 2.0)]).unwrap();
        let (poles, knots) = t.b_spline();
        assert_eq!(poles.len(), 7);
        assert_eq!(knots.len(), 11);
        // De Boor at a parameter inside each span agrees with the span's own cubic.
        let de_boor = |u: f64| -> Vec3 {
            let n = poles.len();
            let mut k = 3;
            while k + 1 < n && u >= knots[k + 1] {
                k += 1;
            }
            let mut d: Vec<Vec3> = (0..4).map(|j| poles[j + k - 3]).collect();
            for r in 1..4 {
                for j in (r..4).rev() {
                    let i = j + k - 3;
                    let a = (u - knots[i]) / (knots[i + 4 - r] - knots[i]);
                    d[j] = d[j - 1].lerp(d[j], a);
                }
            }
            d[3]
        };
        let mut from = 0.0;
        for (i, h) in t.spans.iter().enumerate() {
            for f in [0.0, 0.2, 0.5, 0.9] {
                let (a, b) = (de_boor(from + f * h), t.point(i, f));
                assert!(a.dist(b) < 1e-9, "span {i} at {f}: {a:?} and {b:?}");
            }
            from += h;
        }
    }

    #[test]
    fn a_line_tangent_to_a_spline_at_their_end_turns_the_spline() {
        let mut s = Sketch3d::new();
        let a = s.add_point(v(0.0, 0.0, 0.0)).unwrap();
        let b = s.add_point(v(0.0, 0.0, 10.0)).unwrap();
        let line = s.add_line(a, b).unwrap();
        s.add_constraint(Constraint3::Fix { point: a }).unwrap();
        s.add_constraint(Constraint3::Fix { point: b }).unwrap();
        let pts = [PointRef3::Existing(b), PointRef3::New(v(10.0, 0.0, 20.0)), PointRef3::New(v(20.0, 5.0, 20.0))];
        let spline = s.add_spline(&pts).unwrap();
        s.add_constraint(Constraint3::Tangent { a: line, b: spline }).unwrap();
        let t = s.spline(spline).unwrap().start_tangent();
        assert!(t.cross(Vec3::Z).len() < 1e-8, "{t:?}");
        // Not meeting at an end: refused, saying what to do.
        let other = s.add_line(v(50.0, 0.0, 0.0), v(60.0, 0.0, 0.0)).unwrap();
        let e = s.add_constraint(Constraint3::Tangent { a: other, b: spline }).unwrap_err().to_string();
        assert!(e.contains("make their ends coincident first"), "{e}");
    }

    #[test]
    fn reference_points_do_not_move_and_what_is_constrained_to_them_follows() {
        let mut s = Sketch3d::new();
        let r = s.add_reference_point(v(5.0, 5.0, 5.0)).unwrap();
        let p = s.add_point(v(0.0, 0.0, 0.0)).unwrap();
        let line = s.add_line(r, p).unwrap();
        s.add_constraint(Constraint3::Along { line, axis: Axis3::X }).unwrap();
        s.add_constraint(Constraint3::Length { line, value: 8.0 }).unwrap();
        assert_eq!(s.point(r), Some(v(5.0, 5.0, 5.0)));
        let q = s.point(p).unwrap();
        assert!((q.y - 5.0).abs() < 1e-9 && (q.z - 5.0).abs() < 1e-9 && ((q.x - 5.0).abs() - 8.0).abs() < 1e-9, "{q:?}");
        // The part moves the point: solved again, what is held to it comes too.
        s.set_point(r, v(5.0, 9.0, 1.0)).unwrap();
        s.solve().unwrap();
        let q = s.point(p).unwrap();
        assert!((q.y - 9.0).abs() < 1e-9 && (q.z - 1.0).abs() < 1e-9, "{q:?}");
        assert_eq!(s.dof().unwrap().dof, 0);
    }

    #[test]
    fn dragging_a_point_keeps_the_constraints() {
        let (mut s, l, p) = staircase();
        s.add_constraint(Constraint3::Along { line: l[1], axis: Axis3::X }).unwrap();
        s.drag(p[2], v(70.0, 30.0, 10.0)).unwrap();
        let (a, b) = s.line(l[1]).unwrap();
        assert!((a.y - b.y).abs() < 1e-9 && (a.z - b.z).abs() < 1e-9, "{a:?} {b:?}");
        assert!(b.dist(v(70.0, 30.0, 10.0)) < 1e-6, "it is where it was dragged to: {b:?}");
    }

    #[test]
    fn a_path_runs_end_to_end_from_the_end_nearer_the_profile() {
        let (mut s, l, p) = staircase();
        let b = s.add_bend(p[1], 10.0).unwrap();
        let curves = [l[2], b.arc, l[0], l[1]];
        let path = s.path(&curves, v(0.0, 0.0, -1.0)).unwrap();
        assert_eq!(path.curves.len(), 4);
        assert!(matches!(path.curves[0], Curve3::Line { start, .. } if start.dist(v(0.0, 0.0, 0.0)) < 1e-9));
        assert!(matches!(path.curves[1], Curve3::Arc { .. }));
        // From the other end, all of it the other way.
        let path = s.path(&curves, v(50.0, 60.0, 50.0)).unwrap();
        assert!(matches!(path.curves[0], Curve3::Line { start, .. } if start.dist(v(50.0, 50.0, 50.0)) < 1e-9));
        assert!(matches!(path.curves[3], Curve3::Line { end, .. } if end.dist(v(0.0, 0.0, 0.0)) < 1e-9));
        // Curves that do not join say so.
        let e = s.path(&[l[0], l[2]], Vec3::ZERO).unwrap_err();
        assert!(e.contains("do not join"), "{e}");
    }

    #[test]
    fn a_path_of_a_spline_run_backwards_is_its_mirror_in_parameter() {
        let t = Through::new(&[v(0.0, 0.0, 0.0), v(10.0, 5.0, 0.0), v(12.0, 20.0, 4.0)]).unwrap();
        let (poles, knots) = t.b_spline();
        let Curve3::BSpline { poles: rp, knots: rk } = reversed(Curve3::BSpline { poles: poles.clone(), knots: knots.clone() }) else { panic!() };
        assert_eq!(rp.first(), poles.last());
        assert!(rk.windows(2).all(|w| w[0] <= w[1]));
        assert!((rk[0] - knots[0]).abs() < 1e-12);
        assert!((rk.last().unwrap() - knots.last().unwrap()).abs() < 1e-12);
    }

    #[test]
    fn it_reads_back_from_json_as_it_was() {
        let (mut s, l, p) = staircase();
        s.add_constraint(Constraint3::Fix { point: p[0] }).unwrap();
        s.add_constraint(Constraint3::Along { line: l[2], axis: Axis3::Y }).unwrap();
        s.add_bend(p[1], 7.5).unwrap();
        let text = serde_json::to_string(&s).unwrap();
        let back: Sketch3d = serde_json::from_str(&text).unwrap();
        assert_eq!(back, s);
        assert_eq!(serde_json::to_string(&back).unwrap(), text);
        back.validate().unwrap();
        // A file that says what cannot be is told, not believed.
        let bad = text.replace("\"start\":", "\"start_of\":");
        assert!(serde_json::from_str::<Sketch3d>(&bad).is_err());
        let mut hostile = s.clone();
        hostile.entities.insert(
            EntityId(9999),
            Entity3 { geometry: Geometry3::Line { start: EntityId(1), end: EntityId(2) }, construction: false, reference: false },
        );
        assert!(hostile.validate().unwrap_err().to_string().contains("beyond the id counter"));
    }
}
