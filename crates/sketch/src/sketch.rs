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
            None => {}
        }
        out
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
                Geometry::Spline { poles, .. } => poles.iter_mut().for_each(swap),
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
        self.entities.insert(id, Entity { geometry, construction: false });
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
        use Constraint::*;
        for r in c.refs() {
            if self.entity(r).is_none() {
                return Err(SketchError::NoEntity(r));
            }
        }
        let (pt, ln, rd) = (|i| self.is_point(i), |i| self.is_line(i), |i| self.is_round(i));
        let ok = match c {
            Coincident { a, b } => pt(*a) && pt(*b) && a != b,
            PointOnCurve { point, curve } => pt(*point) && (ln(*curve) || rd(*curve)),
            Horizontal { line } | Vertical { line } => ln(*line),
            Parallel { a, b } | Perpendicular { a, b } | Collinear { a, b } => ln(*a) && ln(*b) && a != b,
            Tangent { a, b } => a != b && ((ln(*a) && rd(*b)) || (rd(*a) && ln(*b)) || (rd(*a) && rd(*b))),
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
        };
        if !ok {
            return Err(invalid(format!("{} cannot apply to these entities", c.name())));
        }
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

    pub fn remove_constraint(&mut self, id: ConstraintId) -> SketchResult<Constraint> {
        self.places.remove(&id);
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

    /// Forgets the places of dimensions that are gone.
    fn drop_stale_places(&mut self) {
        let constraints = &self.constraints;
        self.places.retain(|id, _| constraints.contains_key(id));
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
            self.check_constraint(c).map_err(|e| invalid(format!("{id}: {e}")))?;
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
