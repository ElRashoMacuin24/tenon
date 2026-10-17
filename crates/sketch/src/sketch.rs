//! The sketch container: entities, constraints and the editing API.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use tenon_geom::{Arc, Spline, Vec2, tol};
use tenon_kernel::Curve2;

use crate::entity::valid_pos;
use crate::{Constraint, ConstraintId, Entity, EntityId, Geometry, PointRef};

/// Most entities a sketch may hold (hostile-input cap).
pub const MAX_ENTITIES: usize = 20_000;
/// Most constraints a sketch may hold.
pub const MAX_CONSTRAINTS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SketchError {
    #[error("no entity {0}")]
    NoEntity(EntityId),
    #[error("no constraint {0}")]
    NoConstraint(ConstraintId),
    #[error("{0}")]
    Invalid(String),
    #[error("the sketch is full")]
    TooMany,
    #[error("the constraint conflicts with {0:?}")]
    Conflict(Vec<ConstraintId>),
    #[error("the constraint is redundant with {0:?}")]
    Redundant(Vec<ConstraintId>),
    #[error("the sketch is too large to solve in one system")]
    TooLarge,
}

pub type SketchResult<T> = Result<T, SketchError>;

/// A slot as [`Sketch::add_slot`] makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// The two straight sides.
    pub lines: [EntityId; 2],
    /// The round ends: the one about the first centre, then the other.
    pub arcs: [EntityId; 2],
    /// The centre points of the ends.
    pub centers: [EntityId; 2],
}

/// An arc slot as [`Sketch::add_arc_slot`] makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArcSlot {
    /// The two curved sides about the centre: the outer, then the inner.
    pub sides: [EntityId; 2],
    /// The round ends: the one at the start, then the one at the end.
    pub ends: [EntityId; 2],
    /// The centre point of the sides, then those of the ends (the start's first).
    pub centers: [EntityId; 3],
}

/// Whether a text can be one: some words of at most [`crate::text::MAX_CHARS`] characters, a
/// height in range and a finite angle.
fn check_text(text: &str, height: f64, angle: f64) -> SketchResult<()> {
    if text.trim().is_empty() {
        return Err(invalid("a text needs some words"));
    }
    if text.chars().count() > crate::text::MAX_CHARS {
        return Err(invalid(format!("a text has at most {} characters", crate::text::MAX_CHARS)));
    }
    if !(height.is_finite() && (0.01..=10_000.0).contains(&height)) {
        return Err(invalid(format!("a text's height must be from 0.01 to 10000 mm, not {height}")));
    }
    if !angle.is_finite() {
        return Err(invalid("a text's angle must be a number"));
    }
    Ok(())
}

fn invalid(msg: impl Into<String>) -> SketchError {
    SketchError::Invalid(msg.into())
}

/// Serialises an id-keyed map as a list of `[id, value]` pairs. JSON object keys are strings,
/// which serde cannot turn back into integer ids inside internally tagged enums (the document's
/// feature list), so a plain list is used.
mod as_pairs {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer, K: Serialize, V: Serialize>(m: &BTreeMap<K, V>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(m.iter())
    }

    pub fn deserialize<'de, D: Deserializer<'de>, K: Deserialize<'de> + Ord, V: Deserialize<'de>>(d: D) -> Result<BTreeMap<K, V>, D::Error> {
        Ok(Vec::<(K, V)>::deserialize(d)?.into_iter().collect())
    }
}

/// A 2D sketch. Coordinates are millimetres in the sketch plane.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    #[serde(with = "as_pairs")]
    entities: BTreeMap<EntityId, Entity>,
    #[serde(with = "as_pairs")]
    constraints: BTreeMap<ConstraintId, Constraint>,
    next_entity: u32,
    next_constraint: u32,
    /// Where each dimension's value sits in the sketch, for those placed by hand. A dimension
    /// without one is drawn beside its geometry.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "as_pairs")]
    places: BTreeMap<ConstraintId, Vec2>,
    /// Dimensions that follow the sketch instead of driving it (drawn in parentheses). The
    /// solver leaves them out; their values are measured again after every change.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    driven: BTreeSet<ConstraintId>,
}

impl Sketch {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- reading ----------------------------------------------------------------------------

    pub fn entities(&self) -> impl Iterator<Item = (EntityId, &Entity)> {
        self.entities.iter().map(|(k, v)| (*k, v))
    }
    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }
    pub fn constraints(&self) -> impl Iterator<Item = (ConstraintId, &Constraint)> {
        self.constraints.iter().map(|(k, v)| (*k, v))
    }
    pub fn constraint(&self, id: ConstraintId) -> Option<&Constraint> {
        self.constraints.get(&id)
    }
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }
    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn geometry(&self, id: EntityId) -> Option<&Geometry> {
        self.entities.get(&id).map(|e| &e.geometry)
    }
    pub fn point(&self, id: EntityId) -> Option<Vec2> {
        match self.geometry(id)? {
            Geometry::Point { pos } => Some(*pos),
            _ => None,
        }
    }
    pub fn is_point(&self, id: EntityId) -> bool {
        self.point(id).is_some()
    }
    pub fn is_line(&self, id: EntityId) -> bool {
        matches!(self.geometry(id), Some(Geometry::Line { .. }))
    }
    /// Circle or arc.
    pub fn is_round(&self, id: EntityId) -> bool {
        matches!(self.geometry(id), Some(Geometry::Circle { .. } | Geometry::Arc { .. }))
    }
    pub fn line(&self, id: EntityId) -> Option<(Vec2, Vec2)> {
        match self.geometry(id)? {
            Geometry::Line { start, end } => Some((self.point(*start)?, self.point(*end)?)),
            _ => None,
        }
    }
    /// Centre and radius of a circle or arc.
    pub fn circle(&self, id: EntityId) -> Option<(Vec2, f64)> {
        match self.geometry(id)? {
            Geometry::Circle { center, radius } => Some((self.point(*center)?, *radius)),
            Geometry::Arc { center, start, .. } => {
                let c = self.point(*center)?;
                Some((c, self.point(*start)?.dist(c)))
            }
            _ => None,
        }
    }
    pub fn arc(&self, id: EntityId) -> Option<Arc> {
        match self.geometry(id)? {
            Geometry::Arc { center, start, end } => {
                let (c, s, e) = (self.point(*center)?, self.point(*start)?, self.point(*end)?);
                Some(Arc::new(c, s.dist(c), (s - c).angle(), (e - c).angle()))
            }
            _ => None,
        }
    }
    /// Where two curves join, end to end: an end of `a` and an end of `b` that are one point or
    /// are held together (coincident). Of a line or an arc the ends are its start and end, of a
    /// spline its first and last poles.
    pub fn joined_ends(&self, a: EntityId, b: EntityId) -> Option<(EntityId, EntityId)> {
        let ends = |id: EntityId| -> Vec<EntityId> {
            match self.geometry(id) {
                Some(Geometry::Line { start, end }) | Some(Geometry::Arc { start, end, .. }) => vec![*start, *end],
                Some(Geometry::Spline { poles, .. }) => poles.first().into_iter().chain(poles.last()).copied().collect(),
                _ => Vec::new(),
            }
        };
        let held = |p: EntityId, q: EntityId| {
            p == q || self.constraints().any(|(_, c)| matches!(c, Constraint::Coincident { a, b } if (*a, *b) == (p, q) || (*a, *b) == (q, p)))
        };
        let of_b = ends(b);
        ends(a).into_iter().find_map(|p| of_b.iter().find(|q| held(p, **q)).map(|q| (p, *q)))
    }

    pub fn is_ellipse(&self, id: EntityId) -> bool {
        matches!(self.geometry(id), Some(Geometry::Ellipse { .. }))
    }
    /// An ellipse's centre, the axis from it to its `major` point, and the half-length of the
    /// other axis (a quarter turn counter-clockwise from the first).
    pub fn ellipse(&self, id: EntityId) -> Option<(Vec2, Vec2, f64)> {
        match self.geometry(id)? {
            Geometry::Ellipse { center, major, minor } => {
                let c = self.point(*center)?;
                Some((c, self.point(*major)? - c, self.point(*minor)?.dist(c)))
            }
            _ => None,
        }
    }
    pub fn spline(&self, id: EntityId) -> Option<Spline> {
        match self.geometry(id)? {
            Geometry::Spline { poles, degree } => {
                let pts: Option<Vec<Vec2>> = poles.iter().map(|p| self.point(*p)).collect();
                Some(Spline::from_control(pts?, *degree as usize))
            }
            _ => None,
        }
    }

    /// The curve as the kernel sees it (arcs counter-clockwise from start to end).
    pub fn curve2(&self, id: EntityId) -> Option<Curve2> {
        Some(match self.geometry(id)? {
            Geometry::Point { .. } => return None,
            Geometry::Line { .. } => {
                let (a, b) = self.line(id)?;
                Curve2::Line { start: a, end: b }
            }
            Geometry::Circle { .. } => {
                let (c, r) = self.circle(id)?;
                Curve2::Circle { center: c, radius: r }
            }
            Geometry::Arc { .. } => {
                let a = self.arc(id)?;
                Curve2::Arc { center: a.center, radius: a.radius, start_angle: a.start, end_angle: a.end }
            }
            Geometry::Spline { poles, degree } => {
                let pts: Option<Vec<Vec2>> = poles.iter().map(|p| self.point(*p)).collect();
                Curve2::BSpline { poles: pts?, degree: *degree }
            }
            Geometry::Ellipse { .. } => {
                let (center, major, minor) = self.ellipse(id)?;
                Curve2::Ellipse { center, major, minor }
            }
            Geometry::Text { .. } => return None,
        })
    }

    /// Polyline approximation of a curve (or the single position of a point), for display,
    /// picking and region detection.
    pub fn tessellate(&self, id: EntityId, chord_tol: f64) -> Vec<Vec2> {
        let mut out = Vec::new();
        match self.geometry(id) {
            Some(Geometry::Point { pos }) => out.push(*pos),
            Some(Geometry::Line { .. }) => {
                if let Some((a, b)) = self.line(id) {
                    out.extend([a, b]);
                }
            }
            Some(Geometry::Circle { .. }) => {
                if let Some((c, r)) = self.circle(id) {
                    tenon_geom::Circle::new(c, r).tessellate(chord_tol, &mut out);
                }
            }
            Some(Geometry::Arc { .. }) => {
                if let Some(a) = self.arc(id) {
                    a.tessellate(chord_tol, &mut out);
                }
            }
            Some(Geometry::Spline { .. }) => {
                if let Some(s) = self.spline(id) {
                    out = s.tessellate(chord_tol);
                }
            }
            Some(Geometry::Ellipse { .. }) => {
                if let Some((c, major, minor)) = self.ellipse(id) {
                    out = ellipse_points(c, major, minor, 0.0, std::f64::consts::TAU, chord_tol);
                }
            }
            // (A text is many outlines, not one: see [`Sketch::text_contours`].)
            Some(Geometry::Text { .. }) | None => {}
        }
        out
    }

    /// The outlines of text entity `id`'s letters as they stand.
    pub fn text_contours(&self, id: EntityId) -> Vec<crate::text::Contour> {
        match self.geometry(id) {
            Some(Geometry::Text { at, text, height, angle }) => {
                self.point(*at).map_or_else(Vec::new, |p| crate::text::contours(text, p, *height, *angle))
            }
            _ => Vec::new(),
        }
    }

    /// The box text `id` takes, as its four corners in order round it.
    pub fn text_corners(&self, id: EntityId) -> Option<[Vec2; 4]> {
        match self.geometry(id)? {
            Geometry::Text { at, text, height, angle } => crate::text::corners(text, self.point(*at)?, *height, *angle),
            _ => None,
        }
    }

    /// Where the sketch reaches: its points, and the corners of its texts' boxes.
    pub fn extent(&self) -> Vec<Vec2> {
        let points = self.entities().filter_map(|(id, _)| self.point(id));
        points.chain(self.texts().filter_map(|t| self.text_corners(t)).flatten()).collect()
    }

    /// The texts of the sketch.
    pub fn texts(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.entities().filter(|(_, e)| e.geometry.is_text()).map(|(id, _)| id)
    }

    /// Text `text` with its first line's baseline starting at `at`, capital letters `height` tall,
    /// turned `angle` radians (DEC-102).
    pub fn add_text(&mut self, at: impl Into<PointRef>, text: &str, height: f64, angle: f64) -> SketchResult<EntityId> {
        check_text(text, height, angle)?;
        let at = self.resolve(at.into())?;
        self.insert(Geometry::Text { at, text: text.to_owned(), height, angle })
    }

    /// Changes a text's words, height or angle (what is None stays).
    pub fn set_text(&mut self, id: EntityId, text: Option<&str>, height: Option<f64>, angle: Option<f64>) -> SketchResult<()> {
        let Some(Geometry::Text { text: t, height: h, angle: a, .. }) = self.geometry(id) else {
            return Err(invalid(format!("{id} is no text")));
        };
        let (t, h, a) = (text.map_or_else(|| t.clone(), str::to_owned), height.unwrap_or(*h), angle.unwrap_or(*a));
        check_text(&t, h, a)?;
        if let Some(Geometry::Text { text, height, angle, .. }) = self.geometry_mut(id) {
            (*text, *height, *angle) = (t, h, a);
        }
        Ok(())
    }

    /// Curves that use point `p`.
    pub fn curves_at(&self, p: EntityId) -> Vec<EntityId> {
        self.entities.iter().filter(|(_, e)| e.geometry.points().contains(&p)).map(|(id, _)| *id).collect()
    }

    /// Constraints that reference `id`.
    pub fn constraints_on(&self, id: EntityId) -> Vec<ConstraintId> {
        self.constraints.iter().filter(|(_, c)| c.refs().contains(&id)).map(|(cid, _)| *cid).collect()
    }

    // ---- creating geometry ------------------------------------------------------------------

    pub(crate) fn geometry_mut(&mut self, id: EntityId) -> Option<&mut Geometry> {
        self.entities.get_mut(&id).map(|e| &mut e.geometry)
    }

    /// Replaces point `old` by `new` in curve `curve`.
    pub(crate) fn replace_point(&mut self, curve: EntityId, old: EntityId, new: EntityId) {
        if let Some(g) = self.geometry_mut(curve) {
            let swap = |p: &mut EntityId| {
                if *p == old {
                    *p = new;
                }
            };
            match g {
                Geometry::Line { start, end } => {
                    swap(start);
                    swap(end);
                }
                Geometry::Arc { center, start, end } => {
                    swap(center);
                    swap(start);
                    swap(end);
                }
                Geometry::Circle { center, .. } => swap(center),
                Geometry::Ellipse { center, major, minor } => {
                    swap(center);
                    swap(major);
                    swap(minor);
                }
                Geometry::Spline { poles, .. } => poles.iter_mut().for_each(swap),
                Geometry::Text { at, .. } => swap(at),
                Geometry::Point { .. } => {}
            }
        }
    }

    /// Removes point `p` (and constraints on it) if no curve uses it any more.
    pub(crate) fn drop_if_unused(&mut self, p: EntityId) {
        if self.is_point(p) && self.curves_at(p).is_empty() {
            self.entities.remove(&p);
            self.constraints.retain(|_, c| !c.refs().contains(&p));
            self.drop_stale_places();
        }
    }

    pub(crate) fn insert(&mut self, geometry: Geometry) -> SketchResult<EntityId> {
        if self.entities.len() >= MAX_ENTITIES {
            return Err(SketchError::TooMany);
        }
        self.next_entity = self.next_entity.checked_add(1).ok_or(SketchError::TooMany)?;
        let id = EntityId(self.next_entity);
        self.entities.insert(id, Entity { geometry, construction: false, centerline: false, reference: false, sketch_point: false });
        Ok(id)
    }

    pub fn add_point(&mut self, pos: Vec2) -> SketchResult<EntityId> {
        if !valid_pos(pos) {
            return Err(invalid(format!("point {pos:?} is not finite or out of range")));
        }
        self.insert(Geometry::Point { pos })
    }

    /// An existing point, or a new one.
    pub fn resolve(&mut self, p: PointRef) -> SketchResult<EntityId> {
        match p {
            PointRef::New(pos) => self.add_point(pos),
            PointRef::Existing(id) if self.is_point(id) => Ok(id),
            PointRef::Existing(id) => Err(invalid(format!("{id} is not a point"))),
        }
    }

    /// Position a point reference would have, without creating anything.
    fn peek(&self, p: PointRef) -> SketchResult<Vec2> {
        match p {
            PointRef::New(pos) if valid_pos(pos) => Ok(pos),
            PointRef::New(pos) => Err(invalid(format!("point {pos:?} is not finite or out of range"))),
            PointRef::Existing(id) => self.point(id).ok_or_else(|| invalid(format!("{id} is not a point"))),
        }
    }

    pub fn add_line(&mut self, a: impl Into<PointRef>, b: impl Into<PointRef>) -> SketchResult<EntityId> {
        let (a, b) = (a.into(), b.into());
        // Validate before creating points, so a refused line leaves nothing behind.
        if a == b || self.peek(a)?.dist(self.peek(b)?) <= tol::MIN_SIZE {
            return Err(invalid("a line needs two distinct points"));
        }
        let (a, b) = (self.resolve(a)?, self.resolve(b)?);
        self.insert(Geometry::Line { start: a, end: b })
    }

    pub fn add_circle(&mut self, center: impl Into<PointRef>, radius: f64) -> SketchResult<EntityId> {
        if !tol::is_valid_size(radius) {
            return Err(invalid(format!("circle radius {radius} is out of range")));
        }
        let c = self.resolve(center.into())?;
        self.insert(Geometry::Circle { center: c, radius })
    }

    /// An ellipse about `center` (DEC-120): one axis from it to `major`, the other a quarter turn
    /// counter-clockwise from that and `minor` long. Its three points are new.
    pub fn add_ellipse(&mut self, center: Vec2, major: Vec2, minor: f64) -> SketchResult<EntityId> {
        let axis = major - center;
        if !tol::is_valid_size(axis.len()) || !tol::is_valid_size(minor) {
            return Err(invalid("an ellipse needs two axes of some length"));
        }
        let mut s = self.clone();
        let (c, m) = (s.add_point(center)?, s.add_point(major)?);
        let n = s.add_point(center + axis.normalized().perp() * minor)?;
        let id = s.insert(Geometry::Ellipse { center: c, major: m, minor: n })?;
        *self = s;
        Ok(id)
    }

    /// Arc counter-clockwise from `start` to `end` about `center`. A new `end` position is moved
    /// onto the circle through `start`.
    pub fn add_arc(&mut self, center: impl Into<PointRef>, start: impl Into<PointRef>, end: impl Into<PointRef>) -> SketchResult<EntityId> {
        let (center, start, end) = (center.into(), start.into(), end.into());
        let cp = self.peek(center)?;
        let r = self.peek(start)?.dist(cp);
        if !tol::is_valid_size(r) {
            return Err(invalid("an arc needs a start point away from its centre"));
        }
        let end = match end {
            PointRef::New(p) if (p - cp).len() > tol::MIN_SIZE => PointRef::New(cp + (p - cp).normalized() * r),
            PointRef::New(_) => return Err(invalid("an arc needs an end point away from its centre")),
            e => e,
        };
        let ep = self.peek(end)?;
        if ep.dist(self.peek(start)?) <= tol::MIN_SIZE || center == start || center == end || start == end {
            return Err(invalid("an arc needs distinct centre, start and end points"));
        }
        let (c, s, e) = (self.resolve(center)?, self.resolve(start)?, self.resolve(end)?);
        self.insert(Geometry::Arc { center: c, start: s, end: e })
    }

    /// Arc through three points (start, a point on the arc, end).
    pub fn add_arc_three_point(&mut self, start: Vec2, through: Vec2, end: Vec2) -> SketchResult<EntityId> {
        let a = Arc::from_3_points(start, through, end).ok_or_else(|| invalid("the three points are collinear"))?;
        // `from_3_points` returns a counter-clockwise arc; its endpoints may be swapped.
        let (s, e) = (a.start_point(), a.end_point());
        let c = self.add_point(a.center)?;
        self.add_arc(c, s, e)
    }

    pub fn add_spline(&mut self, poles: &[PointRef], degree: u32) -> SketchResult<EntityId> {
        if !(1..=5).contains(&degree) || poles.len() <= degree as usize || poles.len() > 500 {
            return Err(invalid("a spline needs degree 1..=5 and more control points than its degree"));
        }
        for p in poles {
            self.peek(*p)?;
        }
        let ids = poles.iter().map(|p| self.resolve(*p)).collect::<SketchResult<Vec<_>>>()?;
        self.insert(Geometry::Spline { poles: ids, degree })
    }

    pub fn set_construction(&mut self, id: EntityId, construction: bool) -> SketchResult<()> {
        self.entities.get_mut(&id).ok_or(SketchError::NoEntity(id))?.construction = construction;
        Ok(())
    }

    /// Marks a line as a centre line, or as an ordinary line again.
    pub fn set_centerline(&mut self, id: EntityId, centerline: bool) -> SketchResult<()> {
        if !self.is_line(id) {
            return Err(if self.entity(id).is_some() {
                invalid(format!("{id} is not a line: only a line can be a centre line"))
            } else {
                SketchError::NoEntity(id)
            });
        }
        self.entities.get_mut(&id).ok_or(SketchError::NoEntity(id))?.centerline = centerline;
        Ok(())
    }
    pub fn is_centerline(&self, id: EntityId) -> bool {
        self.entity(id).is_some_and(|e| e.centerline)
    }

    /// Makes a point on its own a centre point (what a hole goes at unless told otherwise; a
    /// point is one when made) or a sketch point, which a hole takes only when picked (DEC-117).
    pub fn set_center_point(&mut self, id: EntityId, center: bool) -> SketchResult<()> {
        if !self.is_point(id) {
            return Err(if self.entity(id).is_some() {
                invalid(format!("{id} is not a point: only a point can be a centre point"))
            } else {
                SketchError::NoEntity(id)
            });
        }
        if !self.curves_at(id).is_empty() {
            return Err(invalid(format!("{id} is the end of a curve: only a point on its own can be a centre point")));
        }
        self.entities.get_mut(&id).ok_or(SketchError::NoEntity(id))?.sketch_point = !center;
        Ok(())
    }
    /// A point on its own that is a centre point: not construction, reference or a sketch point.
    pub fn is_center_point(&self, id: EntityId) -> bool {
        self.entity(id).is_some_and(|e| !e.construction && !e.reference && !e.sketch_point) && self.is_point(id) && self.curves_at(id).is_empty()
    }

    /// Marks an entity as reference geometry (or as the sketch's own again). A reference curve's
    /// points are reference too: mark them as well.
    pub fn set_reference(&mut self, id: EntityId, reference: bool) -> SketchResult<()> {
        self.entities.get_mut(&id).ok_or(SketchError::NoEntity(id))?.reference = reference;
        Ok(())
    }
    pub fn is_reference(&self, id: EntityId) -> bool {
        self.entity(id).is_some_and(|e| e.reference)
    }
    /// A curve or point that may not be reshaped by the sketch's own tools: reference geometry,
    /// or a curve standing on a reference point.
    pub fn is_held_outside(&self, id: EntityId) -> bool {
        self.is_reference(id) || self.geometry(id).is_some_and(|g| g.points().iter().any(|p| self.is_reference(*p)))
    }

    /// Moves a point without solving.
    pub fn set_point(&mut self, id: EntityId, pos: Vec2) -> SketchResult<()> {
        if !valid_pos(pos) {
            return Err(invalid(format!("point {pos:?} is not finite or out of range")));
        }
        match self.entities.get_mut(&id).map(|e| &mut e.geometry) {
            Some(Geometry::Point { pos: p }) => {
                *p = pos;
                Ok(())
            }
            Some(_) => Err(invalid(format!("{id} is not a point"))),
            None => Err(SketchError::NoEntity(id)),
        }
    }

    /// Sets a circle's radius without solving.
    pub fn set_radius(&mut self, id: EntityId, r: f64) -> SketchResult<()> {
        match self.entities.get_mut(&id).map(|e| &mut e.geometry) {
            Some(Geometry::Circle { radius, .. }) if tol::is_valid_size(r) => {
                *radius = r;
                Ok(())
            }
            Some(Geometry::Circle { .. }) => Err(invalid(format!("radius {r} is out of range"))),
            Some(_) => Err(invalid(format!("{id} is not a circle"))),
            None => Err(SketchError::NoEntity(id)),
        }
    }

    /// Axis-aligned rectangle from two corners: four lines (bottom, right, top, left) joined at
    /// shared corners, with horizontal and vertical constraints.
    pub fn add_rectangle(&mut self, a: Vec2, b: Vec2) -> SketchResult<[EntityId; 4]> {
        let (lo, hi) = (a.min(b), a.max(b));
        if hi.x - lo.x <= tol::MIN_SIZE || hi.y - lo.y <= tol::MIN_SIZE {
            return Err(invalid("a rectangle needs a non-zero width and height"));
        }
        let p = [self.add_point(lo)?, self.add_point(Vec2::new(hi.x, lo.y))?, self.add_point(hi)?, self.add_point(Vec2::new(lo.x, hi.y))?];
        let l = [self.add_line(p[0], p[1])?, self.add_line(p[1], p[2])?, self.add_line(p[2], p[3])?, self.add_line(p[3], p[0])?];
        for (i, line) in l.iter().enumerate() {
            let c = if i % 2 == 0 { Constraint::Horizontal { line: *line } } else { Constraint::Vertical { line: *line } };
            self.push_constraint(c)?;
        }
        Ok(l)
    }

    /// A rectangle about its centre: `center` and one `corner`. Four lines as
    /// [`Sketch::add_rectangle`] makes them, a construction diagonal, and the centre point held
    /// at the diagonal's middle, so the rectangle stays about the point it was drawn round.
    /// Returns the lines and the centre point.
    pub fn add_rectangle_center(&mut self, center: Vec2, corner: Vec2) -> SketchResult<([EntityId; 4], EntityId)> {
        let mut s = self.clone();
        let l = s.add_rectangle(center * 2.0 - corner, corner)?;
        let (Some(Geometry::Line { start: lo, .. }), Some(Geometry::Line { end: hi, .. })) = (s.geometry(l[0]).cloned(), s.geometry(l[1]).cloned())
        else {
            return Err(invalid("internal: a rectangle without corners"));
        };
        let diagonal = s.add_line(lo, hi)?;
        s.set_construction(diagonal, true)?;
        let c = s.add_point(center)?;
        s.push_constraint(Constraint::Midpoint { point: c, line: diagonal })?;
        *self = s;
        Ok((l, c))
    }

    /// A rectangle at any angle from three points: one side from `a` to `b`, and `c` on (or
    /// level with) the opposite side. Four lines joined at shared corners (the first from `a` to
    /// `b`), the second square to the first and the others parallel to those two.
    pub fn add_rectangle_3(&mut self, a: Vec2, b: Vec2, c: Vec2) -> SketchResult<[EntityId; 4]> {
        let along = b - a;
        if along.len() <= tol::MIN_SIZE {
            return Err(invalid("a rectangle needs a first side of some length"));
        }
        let n = along.normalized().perp();
        let depth = (c - a).dot(n);
        if depth.abs() <= tol::MIN_SIZE {
            return Err(invalid("a rectangle needs a non-zero width and height"));
        }
        let mut s = self.clone();
        let p = [s.add_point(a)?, s.add_point(b)?, s.add_point(b + n * depth)?, s.add_point(a + n * depth)?];
        let l = [s.add_line(p[0], p[1])?, s.add_line(p[1], p[2])?, s.add_line(p[2], p[3])?, s.add_line(p[3], p[0])?];
        s.push_constraint(Constraint::Perpendicular { a: l[0], b: l[1] })?;
        s.push_constraint(Constraint::Parallel { a: l[0], b: l[2] })?;
        s.push_constraint(Constraint::Parallel { a: l[1], b: l[3] })?;
        *self = s;
        Ok(l)
    }

    /// A straight slot: two parallel lines joined by a half circle at each end, about the
    /// centres `a` and `b`, `width` across. The lines are tangent to the arcs and the arcs equal,
    /// so it stays a slot whatever is done to it: its length is the distance between the two
    /// centre points and its width a radius or diameter of either end.
    /// Returns the two lines, the two arcs (the one about `a` first) and the two centre points.
    pub fn add_slot(&mut self, a: Vec2, b: Vec2, width: f64) -> SketchResult<Slot> {
        if a.dist(b) <= tol::MIN_SIZE {
            return Err(invalid("a slot needs two centres apart from one another"));
        }
        if !tol::is_valid_size(width) {
            return Err(invalid(format!("slot width {width} is out of range")));
        }
        let (u, r) = ((b - a).normalized(), width / 2.0);
        let n = u.perp();
        let mut s = self.clone();
        let (ca, cb) = (s.add_point(a)?, s.add_point(b)?);
        // Round the outside, counter-clockwise: along one side from a to b, round b, back, round a.
        let (a_lo, b_lo, b_hi, a_hi) = (s.add_point(a - n * r)?, s.add_point(b - n * r)?, s.add_point(b + n * r)?, s.add_point(a + n * r)?);
        let lower = s.add_line(a_lo, b_lo)?;
        let round_b = s.add_arc(cb, b_lo, b_hi)?;
        let upper = s.add_line(b_hi, a_hi)?;
        let round_a = s.add_arc(ca, a_hi, a_lo)?;
        for (line, arc) in [(lower, round_a), (lower, round_b), (upper, round_a), (upper, round_b)] {
            s.push_constraint(Constraint::Tangent { a: line, b: arc })?;
        }
        s.push_constraint(Constraint::Equal { a: round_a, b: round_b })?;
        *self = s;
        Ok(Slot { lines: [lower, upper], arcs: [round_a, round_b], centers: [ca, cb] })
    }

    /// An arc slot: two arcs about `center`, `width` apart, joined at each end by a half
    /// circle. Its middle runs counter-clockwise from `start` round to the direction of `end`
    /// (its radius is the start's). The ends are tangent to both sides, so it stays an arc slot
    /// whatever is done to it: the sides share their centre, and the ends are equal of
    /// themselves. Its radius, width and the angle between its ends are what size it.
    pub fn add_arc_slot(&mut self, center: Vec2, start: Vec2, end: Vec2, width: f64) -> SketchResult<ArcSlot> {
        let radius = start.dist(center);
        if !tol::is_valid_size(radius) || end.dist(center) <= tol::MIN_SIZE {
            return Err(invalid("an arc slot needs its start and its end away from its centre"));
        }
        if !tol::is_valid_size(width) {
            return Err(invalid(format!("slot width {width} is out of range")));
        }
        if width >= 2.0 * radius {
            return Err(invalid("an arc slot must be narrower than twice its radius: its inside would cross the centre"));
        }
        let (u0, u1) = ((start - center).normalized(), (end - center).normalized());
        if (u1 - u0).len() * radius <= width {
            return Err(invalid("the round ends of the arc slot would overlap: its ends must be further apart round the arc"));
        }
        let r = width / 2.0;
        let at = |u: Vec2, d: f64| center + u * d;
        let mut s = self.clone();
        let c = s.add_point(center)?;
        let (c0, c1) = (s.add_point(at(u0, radius))?, s.add_point(at(u1, radius))?);
        let (out0, out1) = (s.add_point(at(u0, radius + r))?, s.add_point(at(u1, radius + r))?);
        let (in0, in1) = (s.add_point(at(u0, radius - r))?, s.add_point(at(u1, radius - r))?);
        // Counter-clockwise: along the outside, round the end, back along the inside (drawn the
        // same way round its centre), round the start.
        let outer = s.add_arc(c, out0, out1)?;
        let round_end = s.add_arc(c1, out1, in1)?;
        let inner = s.add_arc(c, in0, in1)?;
        let round_start = s.add_arc(c0, in0, out0)?;
        for (side, round) in [(outer, round_start), (outer, round_end), (inner, round_start), (inner, round_end)] {
            s.push_constraint(Constraint::Tangent { a: side, b: round })?;
        }
        *self = s;
        Ok(ArcSlot { sides: [outer, inner], ends: [round_start, round_end], centers: [c, c0, c1] })
    }

    /// Regular polygon with `sides` corners, one at `vertex`: the sides plus a construction
    /// circle, with every corner on the circle and equal sides.
    pub fn add_polygon(&mut self, center: Vec2, vertex: Vec2, sides: u32) -> SketchResult<Vec<EntityId>> {
        if !(3..=64).contains(&sides) {
            return Err(invalid("a polygon needs 3 to 64 sides"));
        }
        let r = vertex.dist(center);
        if !tol::is_valid_size(r) {
            return Err(invalid("a polygon needs a vertex away from its centre"));
        }
        let c = self.add_point(center)?;
        let circle = self.add_circle(c, r)?;
        self.set_construction(circle, true)?;
        let a0 = (vertex - center).angle();
        let n = sides as usize;
        let corners = (0..n)
            .map(|i| self.add_point(Vec2::polar(center, r, a0 + std::f64::consts::TAU * i as f64 / n as f64)))
            .collect::<SketchResult<Vec<_>>>()?;
        let mut lines = Vec::with_capacity(n);
        for i in 0..n {
            lines.push(self.add_line(corners[i], corners[(i + 1) % n])?);
            self.push_constraint(Constraint::PointOnCurve { point: corners[i], curve: circle })?;
        }
        for w in lines.windows(2) {
            self.push_constraint(Constraint::Equal { a: w[0], b: w[1] })?;
        }
        Ok(lines)
    }

    // ---- constraints (no solving; see `solve.rs` for checked, solved edits) -----------------

    /// Checks that a constraint's references have the right types and its value is sane.
    pub fn check_constraint(&self, c: &Constraint) -> SketchResult<()> {
        self.check_kinds(c)?;
        use Constraint::*;
        if let Some(v) = c.value() {
            let fine = match c {
                Angle { .. } => v.is_finite() && v > 0.0 && v < std::f64::consts::TAU,
                HorizontalDistance { .. } | VerticalDistance { .. } => tol::is_valid_coord(v),
                Distance { .. } => v.is_finite() && (0.0..=tol::MAX_SIZE).contains(&v),
                _ => tol::is_valid_size(v),
            };
            if !fine {
                return Err(invalid(format!("{} value {v} is out of range", c.name())));
            }
        }
        Ok(())
    }

    /// Checks that a constraint's references exist and have the right types.
    pub fn check_kinds(&self, c: &Constraint) -> SketchResult<()> {
        use Constraint::*;
        for r in c.refs() {
            if self.entity(r).is_none() {
                return Err(SketchError::NoEntity(r));
            }
        }
        let (pt, ln, rd) = (|i| self.is_point(i), |i| self.is_line(i), |i| self.is_round(i));
        let ok = match c {
            Coincident { a, b } => pt(*a) && pt(*b) && a != b,
            PointOnCurve { point, curve } => pt(*point) && (ln(*curve) || rd(*curve) || self.is_ellipse(*curve)),
            Horizontal { line } | Vertical { line } => ln(*line),
            Parallel { a, b } | Perpendicular { a, b } | Collinear { a, b } => ln(*a) && ln(*b) && a != b,
            Tangent { a, b } => a != b && ((ln(*a) && rd(*b)) || (rd(*a) && ln(*b)) || (rd(*a) && rd(*b))),
            Smooth { a, b } => {
                let (sp, arc) = (|i| self.spline(i).is_some(), |i| matches!(self.geometry(i), Some(Geometry::Arc { .. })));
                let joins = |i| ln(i) || arc(i) || sp(i);
                a != b && (sp(*a) || sp(*b)) && joins(*a) && joins(*b) && self.joined_ends(*a, *b).is_some()
            }
            Concentric { a, b } => rd(*a) && rd(*b) && a != b,
            Equal { a, b } => a != b && ((ln(*a) && ln(*b)) || (rd(*a) && rd(*b))),
            Symmetric { a, b, axis } => pt(*a) && pt(*b) && a != b && ln(*axis),
            Midpoint { point, line } => pt(*point) && ln(*line),
            Fix { point } => pt(*point),
            Distance { a, b, .. } => a != b && ((pt(*a) && pt(*b)) || (pt(*a) && ln(*b)) || (ln(*a) && pt(*b))),
            HorizontalDistance { a, b, .. } | VerticalDistance { a, b, .. } => pt(*a) && pt(*b) && a != b,
            Length { line, .. } => ln(*line),
            Angle { a, b, .. } => ln(*a) && ln(*b) && a != b,
            Radius { curve, .. } | Diameter { curve, .. } => rd(*curve),
            LinearDiameter { point, axis, .. } => pt(*point) && ln(*axis),
        };
        if !ok {
            return Err(invalid(format!("{} cannot apply to these entities", c.name())));
        }
        Ok(())
    }

    /// Dimension `id` follows the sketch instead of driving it.
    pub fn is_driven(&self, id: ConstraintId) -> bool {
        self.driven.contains(&id)
    }

    /// The driven dimensions.
    pub fn driven(&self) -> impl Iterator<Item = ConstraintId> + '_ {
        self.driven.iter().copied()
    }

    /// Adds a dimension that follows the sketch: it shows a size without setting it, so it can
    /// be put where a driving dimension would be redundant. Its value is measured, whatever `c`
    /// holds.
    pub fn add_driven(&mut self, mut c: Constraint) -> SketchResult<ConstraintId> {
        if !c.is_dimensional() {
            return Err(invalid(format!("{} is not a dimension, so it cannot be driven", c.name())));
        }
        self.check_kinds(&c)?;
        let v = self.measure(&c).filter(|v| v.is_finite()).ok_or_else(|| invalid(format!("{} cannot be measured here", c.name())))?;
        c.set_value(v);
        if self.constraints.len() >= MAX_CONSTRAINTS {
            return Err(SketchError::TooMany);
        }
        self.next_constraint = self.next_constraint.checked_add(1).ok_or(SketchError::TooMany)?;
        let id = ConstraintId(self.next_constraint);
        self.constraints.insert(id, c);
        self.driven.insert(id);
        Ok(id)
    }

    /// Marks dimension `id` as driven without checking anything (see `set_driven` in `solve.rs`).
    pub(crate) fn mark_driven(&mut self, id: ConstraintId, driven: bool) {
        if driven {
            self.driven.insert(id);
        } else {
            self.driven.remove(&id);
        }
    }

    /// Measures every driven dimension again.
    pub fn refresh_driven(&mut self) {
        let ids: Vec<ConstraintId> = self.driven.iter().copied().collect();
        for id in ids {
            let measured = self.constraints.get(&id).and_then(|c| self.measure(c)).filter(|v| v.is_finite());
            if let (Some(v), Some(c)) = (measured, self.constraints.get_mut(&id)) {
                c.set_value(v);
            }
        }
    }

    /// Adds a constraint without solving (the geometry is assumed to satisfy it).
    pub fn push_constraint(&mut self, c: Constraint) -> SketchResult<ConstraintId> {
        self.check_constraint(&c)?;
        if self.constraints.len() >= MAX_CONSTRAINTS {
            return Err(SketchError::TooMany);
        }
        self.next_constraint = self.next_constraint.checked_add(1).ok_or(SketchError::TooMany)?;
        let id = ConstraintId(self.next_constraint);
        self.constraints.insert(id, c);
        Ok(id)
    }

    /// Changes a dimension's value without solving (a tool that moves geometry solves once at
    /// its end).
    pub(crate) fn set_value_unsolved(&mut self, id: ConstraintId, value: f64) -> SketchResult<()> {
        match self.constraints.get_mut(&id) {
            Some(c) => {
                if c.set_value(value) {
                    Ok(())
                } else {
                    Err(invalid(format!("{id} is not a dimension")))
                }
            }
            None => Err(invalid(format!("no constraint {id}"))),
        }
    }

    /// Puts `c` in place of constraint `id`, which keeps its id (and so a dimension its name and
    /// where its value is shown).
    pub(crate) fn replace_constraint(&mut self, id: ConstraintId, c: Constraint) -> SketchResult<()> {
        self.check_constraint(&c)?;
        match self.constraints.get_mut(&id) {
            Some(slot) => {
                *slot = c;
                Ok(())
            }
            None => Err(invalid(format!("no constraint {id}"))),
        }
    }

    pub fn remove_constraint(&mut self, id: ConstraintId) -> SketchResult<Constraint> {
        self.places.remove(&id);
        self.driven.remove(&id);
        self.constraints.remove(&id).ok_or(SketchError::NoConstraint(id))
    }

    pub(crate) fn constraint_mut(&mut self, id: ConstraintId) -> Option<&mut Constraint> {
        self.constraints.get_mut(&id)
    }

    /// Where dimension `id`'s value was placed, if it was placed by hand.
    pub fn place(&self, id: ConstraintId) -> Option<Vec2> {
        self.places.get(&id).copied()
    }

    /// Places dimension `id`'s value at `at` (sketch coordinates): its dimension line runs
    /// through there.
    pub fn set_place(&mut self, id: ConstraintId, at: Vec2) -> SketchResult<()> {
        match self.constraints.get(&id) {
            None => Err(SketchError::NoConstraint(id)),
            Some(c) if !c.is_dimensional() => Err(invalid(format!("{id} is not a dimension, so it has no place to be shown"))),
            Some(_) if !valid_pos(at) => Err(invalid("the dimension's place is not a valid position")),
            Some(_) => {
                self.places.insert(id, at);
                Ok(())
            }
        }
    }

    /// Forgets the places (and the driven marks) of dimensions that are gone.
    fn drop_stale_places(&mut self) {
        let constraints = &self.constraints;
        self.places.retain(|id, _| constraints.contains_key(id));
        self.driven.retain(|id| constraints.contains_key(id));
    }

    // ---- deleting ---------------------------------------------------------------------------

    /// Deletes entities. Deleting a point also deletes the curves that use it; points left
    /// unused by any curve after deleting a curve are removed too. Constraints on anything
    /// removed are removed. Returns everything removed.
    pub fn delete(&mut self, ids: &[EntityId]) -> BTreeSet<EntityId> {
        let mut gone: BTreeSet<EntityId> = ids.iter().copied().filter(|i| self.entities.contains_key(i)).collect();
        // Curves that use a deleted point.
        let dependent: Vec<EntityId> =
            self.entities.iter().filter(|(_, e)| e.geometry.points().iter().any(|p| gone.contains(p))).map(|(id, _)| *id).collect();
        gone.extend(dependent);
        // Points that only deleted curves used.
        let candidates: BTreeSet<EntityId> = gone.iter().filter_map(|id| self.geometry(*id)).flat_map(Geometry::points).collect();
        for p in candidates {
            let still_used = self.entities.iter().any(|(id, e)| !gone.contains(id) && e.geometry.points().contains(&p));
            if !still_used {
                gone.insert(p);
            }
        }
        for id in &gone {
            self.entities.remove(id);
        }
        self.constraints.retain(|_, c| c.refs().iter().all(|r| !gone.contains(r)));
        self.drop_stale_places();
        gone
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
                Geometry::Point { pos } if !valid_pos(*pos) => return Err(invalid(format!("{id} has an invalid position"))),
                Geometry::Circle { radius, .. } if !tol::is_valid_size(*radius) => return Err(invalid(format!("{id} has an invalid radius"))),
                Geometry::Spline { poles, degree } if !(1..=5).contains(degree) || poles.len() <= *degree as usize => {
                    return Err(invalid(format!("{id} is an invalid spline")));
                }
                g => {
                    for p in g.points() {
                        if !self.is_point(p) {
                            return Err(invalid(format!("{id} references {p}, which is not a point")));
                        }
                    }
                }
            }
        }
        for (id, c) in &self.constraints {
            if id.0 > self.next_constraint {
                return Err(invalid(format!("{id} is beyond the id counter")));
            }
            // (A driven dimension reads whatever the sketch measures: only its number must be one.)
            if self.driven.contains(id) {
                self.check_kinds(c).map_err(|e| invalid(format!("{id}: {e}")))?;
                if !c.is_dimensional() {
                    return Err(invalid(format!("{id} is marked driven but is not a dimension")));
                }
                if !c.value().is_some_and(f64::is_finite) {
                    return Err(invalid(format!("{id} is a driven dimension without a finite value")));
                }
            } else {
                self.check_constraint(c).map_err(|e| invalid(format!("{id}: {e}")))?;
            }
        }
        for id in &self.driven {
            match self.constraints.get(id) {
                Some(c) if c.is_dimensional() => {}
                Some(_) => return Err(invalid(format!("{id} is marked driven but is not a dimension"))),
                None => return Err(invalid(format!("{id} is marked driven but does not exist"))),
            }
        }
        for (id, at) in &self.places {
            match self.constraints.get(id) {
                Some(c) if c.is_dimensional() && valid_pos(*at) => {}
                Some(_) => return Err(invalid(format!("{id} has a place to be shown but is not a dimension, or the place is not valid"))),
                None => return Err(invalid(format!("a place is given for {id}, which does not exist"))),
            }
        }
        Ok(())
    }
}

/// Points round an ellipse from parameter `t0` to `t1`, close enough to it that no chord strays
/// more than `chord_tol` (DEC-120). The point at t is `c + major cos t + minor sin t` along the
/// axis a quarter turn counter-clockwise from `major`.
pub(crate) fn ellipse_points(c: Vec2, major: Vec2, minor: f64, t0: f64, t1: f64, chord_tol: f64) -> Vec<Vec2> {
    let (a, b) = (major.len(), minor);
    let other = major.normalized().perp() * b;
    // (A step h of the parameter strays from the curve by at most max(a, b) h^2 / 8, as round a
    // circle as big as the longer axis.)
    let r = a.max(b);
    let step = if r > chord_tol { 2.0 * (1.0 - chord_tol / r).clamp(-1.0, 1.0).acos() } else { std::f64::consts::TAU / 8.0 };
    let n = (((t1 - t0).abs() / step.max(1e-3)).ceil() as usize).clamp(8, 4096);
    (0..=n)
        .map(|i| {
            let t = t0 + (t1 - t0) * i as f64 / n as f64;
            c + major * t.cos() + other * t.sin()
        })
        .collect()
}
