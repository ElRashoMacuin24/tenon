//! Moving, copying, turning and scaling sketch geometry; extending a curve to the next one and
//! splitting it where another crosses. Each works on a copy and commits only when it succeeds,
//! so a tool that fails leaves the sketch as it was.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{FRAC_PI_2, TAU};

use tenon_geom::{Line, Vec2, norm_angle, tol};

use crate::sketch::{SketchError, SketchResult};
use crate::solve::{GaussNewton, SketchSolver, SolveOptions};
use crate::{Constraint, EntityId, Geometry, Sketch};

fn invalid(msg: impl Into<String>) -> SketchError {
    SketchError::Invalid(msg.into())
}

/// Parameters closer than this are the same place on a curve.
const PARAM_EPS: f64 = 1e-9;
/// Most there are in a sketch pattern, the original counted.
pub const MAX_PATTERN: usize = 500;
/// Angles within this of a quarter turn count as one.
const ANGLE_EPS: f64 = 1e-9;

/// How geometry is moved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    /// Shifted by a vector.
    Shift(Vec2),
    /// Turned about a point (radians, counter-clockwise).
    Turn { about: Vec2, angle: f64 },
    /// Made larger or smaller about a point.
    Scale { about: Vec2, factor: f64 },
}

impl Motion {
    pub fn apply(&self, p: Vec2) -> Vec2 {
        match *self {
            Motion::Shift(by) => p + by,
            Motion::Turn { about, angle } => {
                let (d, (s, c)) = (p - about, angle.sin_cos());
                about + Vec2::new(d.x * c - d.y * s, d.x * s + d.y * c)
            }
            Motion::Scale { about, factor } => about + (p - about) * factor,
        }
    }

    /// How far everything turns (radians).
    fn turn(&self) -> f64 {
        match self {
            Motion::Turn { angle, .. } => *angle,
            _ => 0.0,
        }
    }

    /// How much longer every length becomes.
    fn factor(&self) -> f64 {
        match self {
            Motion::Scale { factor, .. } => *factor,
            _ => 1.0,
        }
    }

    /// Quarter turns this is, when it is a whole number of them (0 to 3); a shift or a scaling is
    /// none.
    fn quarter_turns(&self) -> Option<u8> {
        match self {
            Motion::Turn { angle, .. } => {
                let q = angle.rem_euclid(TAU) / FRAC_PI_2;
                ((q - q.round()).abs() < ANGLE_EPS).then_some((q.round() as u8) % 4)
            }
            _ => Some(0),
        }
    }

    fn check(&self) -> SketchResult<()> {
        let fine = match *self {
            Motion::Shift(by) => by.is_finite() && by.len() <= tol::MAX_SIZE,
            Motion::Turn { about, angle } => about.is_finite() && angle.is_finite(),
            Motion::Scale { about, factor } => about.is_finite() && factor.is_finite() && (1e-6..=1e6).contains(&factor),
        };
        if fine { Ok(()) } else { Err(invalid("that is not a distance, an angle or a scale to move geometry by")) }
    }
}

impl Sketch {
    /// Moves `ids` (curves, with their points, or points) by `motion`, and the rest of the
    /// sketch follows by its constraints; or, with `copy`, leaves them and makes a copy.
    ///
    /// What is moved keeps its own shape: constraints within it stay, except that a turn drops
    /// (or, by quarter turns, swaps) the horizontal and vertical ones, and a scaling scales the
    /// dimensions. A copy takes the geometric constraints within it, not the dimensions.
    /// Constraints to the rest of the sketch that the motion would break refuse it.
    ///
    /// Returns the moved entities, or the copies of `ids` in the same order.
    pub fn transform(&mut self, ids: &[EntityId], motion: Motion, copy: bool) -> SketchResult<Vec<EntityId>> {
        motion.check()?;
        if ids.is_empty() {
            return Err(invalid("there is nothing to move: select geometry first"));
        }
        for id in ids {
            if self.entity(*id).is_none() {
                return Err(SketchError::NoEntity(*id));
            }
            if self.is_held_outside(*id) && !copy {
                return Err(invalid("projected geometry follows the part and cannot be moved: break its link first"));
            }
        }
        // Everything that moves: the points of the curves, the points picked themselves, and
        // any other curve all of whose points move (it moves whether it was picked or not).
        let mut points: BTreeSet<EntityId> = BTreeSet::new();
        for id in ids {
            match self.geometry(*id) {
                Some(Geometry::Point { .. }) => {
                    points.insert(*id);
                }
                Some(g) => points.extend(g.points()),
                None => {}
            }
        }
        let carried: Vec<EntityId> = self
            .entities()
            .filter(|(id, e)| {
                (e.geometry.is_curve() || e.geometry.is_text()) && (ids.contains(id) || e.geometry.points().iter().all(|p| points.contains(p)))
            })
            .map(|(id, _)| id)
            .collect();
        let moved: BTreeSet<EntityId> = points.iter().copied().chain(carried.iter().copied()).collect();
        let within = |c: &Constraint| c.refs().iter().all(|r| moved.contains(r));
        let quarters = motion.quarter_turns();
        // What a level or upright line is after the motion: itself, the other, or neither.
        let turned = |c: &Constraint| -> Option<Constraint> {
            match (c, quarters) {
                (_, Some(0 | 2)) => Some(c.clone()),
                (Constraint::Horizontal { line }, Some(_)) => Some(Constraint::Vertical { line: *line }),
                (Constraint::Vertical { line }, Some(_)) => Some(Constraint::Horizontal { line: *line }),
                _ => None,
            }
        };
        let level = |c: &Constraint| matches!(c, Constraint::Horizontal { .. } | Constraint::Vertical { .. });
        let across = |c: &Constraint| matches!(c, Constraint::HorizontalDistance { .. } | Constraint::VerticalDistance { .. });

        let mut s = self.clone();
        if copy {
            let mut map: BTreeMap<EntityId, EntityId> = BTreeMap::new();
            for p in &points {
                let at = self.point(*p).ok_or(SketchError::NoEntity(*p))?;
                let new = s.add_point(motion.apply(at))?;
                // (A copied sketch point stays one: the curves go on after.)
                if self.entity(*p).is_some_and(|e| e.sketch_point) {
                    s.set_center_point(new, false)?;
                }
                map.insert(*p, new);
            }
            for c in &carried {
                let Some(e) = self.entity(*c) else { continue };
                let at = |p: &EntityId| map.get(p).copied().ok_or(SketchError::NoEntity(*p));
                let geometry = match &e.geometry {
                    Geometry::Line { start, end } => Geometry::Line { start: at(start)?, end: at(end)? },
                    Geometry::Circle { center, radius } => Geometry::Circle { center: at(center)?, radius: radius * motion.factor() },
                    Geometry::Arc { center, start, end } => Geometry::Arc { center: at(center)?, start: at(start)?, end: at(end)? },
                    Geometry::Spline { poles, degree } => {
                        Geometry::Spline { poles: poles.iter().map(at).collect::<SketchResult<_>>()?, degree: *degree }
                    }
                    Geometry::Text { at: anchor, text, height, angle } => {
                        Geometry::Text { at: at(anchor)?, text: text.clone(), height: height * motion.factor(), angle: angle + motion.turn() }
                    }
                    Geometry::Point { .. } => continue,
                };
                let new = s.insert(geometry)?;
                s.set_construction(new, e.construction)?;
                if e.centerline {
                    s.set_centerline(new, true)?;
                }
                map.insert(*c, new);
            }
            // The copy holds its shape as the original does. (Its sizes are its own to give: a
            // dimension is not copied. Nor is being fixed in place.)
            let kept: Vec<Constraint> = self
                .constraints()
                .filter(|(id, c)| within(c) && !c.is_dimensional() && !self.is_driven(*id) && !matches!(c, Constraint::Fix { .. }))
                .filter_map(|(_, c)| if level(c) { turned(c) } else { Some(c.clone()) })
                .map(|c| c.map_refs(|r| map.get(&r).copied().unwrap_or(r)))
                .collect();
            for c in kept {
                s.push_constraint(c)?;
            }
            let out = ids.iter().filter_map(|id| map.get(id).copied()).collect();
            *self = s;
            return Ok(out);
        }

        // (A point fixed in place is held there, whatever is asked of it.)
        if self.constraints().any(|(_, c)| matches!(c, Constraint::Fix { point } if points.contains(point))) {
            return Err(invalid("the rest of the sketch cannot follow: a point of the selection is fixed, which holds the selection where it is"));
        }
        // Within what moves: level and upright lines turn with it, sizes scale with it. A
        // dimension measured level or upright cannot turn, unless by quarter turns.
        let own: Vec<(crate::ConstraintId, Constraint)> = self.constraints().filter(|(_, c)| within(c)).map(|(id, c)| (id, c.clone())).collect();
        for (id, c) in own {
            if self.is_driven(id) {
                continue;
            }
            if level(&c) {
                s.remove_constraint(id)?;
                if let Some(t) = turned(&c) {
                    s.push_constraint(t)?;
                }
            } else if across(&c) && !matches!(quarters, Some(0 | 2)) {
                return Err(invalid(
                    "a dimension measured level or upright holds the selection as it lies: delete it, or make it driven, before turning",
                ));
            } else if across(&c) && quarters == Some(2) {
                s.set_value_unsolved(id, -c.value().unwrap_or(0.0))?;
            } else if c.is_dimensional() && !c.is_angular() && motion.factor() != 1.0 {
                s.set_value_unsolved(id, c.value().unwrap_or(0.0) * motion.factor())?;
            }
        }
        let mut pin: Vec<EntityId> = Vec::new();
        for p in &points {
            let at = self.point(*p).ok_or(SketchError::NoEntity(*p))?;
            s.set_point(*p, motion.apply(at))?;
            pin.push(*p);
        }
        for c in &carried {
            if let Some(Geometry::Circle { radius, .. }) = self.geometry(*c) {
                s.set_radius(*c, radius * motion.factor())?;
                pin.push(*c);
            }
            // (Text turns and grows with what carries it.)
            if let Some(Geometry::Text { height, angle, .. }) = s.geometry_mut(*c) {
                *height *= motion.factor();
                *angle += motion.turn();
            }
        }
        GaussNewton.solve(&mut s, &SolveOptions { pin, ..Default::default() }).map_err(|e| {
            invalid(format!("the rest of the sketch cannot follow: a constraint or a dimension holds the selection where it is ({e})"))
        })?;
        *self = s;
        Ok(ids.to_vec())
    }

    /// The end point of line or arc `curve` nearer to `pick`: (the point, whether it is the
    /// curve's end rather than its start).
    fn end_near(&self, curve: EntityId, pick: Vec2) -> SketchResult<(EntityId, bool)> {
        let (start, end) = match self.geometry(curve) {
            Some(Geometry::Line { start, end }) | Some(Geometry::Arc { start, end, .. }) => (*start, *end),
            Some(_) => return Err(invalid("only a line or an arc has an end to extend")),
            None => return Err(SketchError::NoEntity(curve)),
        };
        let (a, b) = (self.point(start).ok_or(SketchError::NoEntity(start))?, self.point(end).ok_or(SketchError::NoEntity(end))?);
        Ok(if a.dist(pick) <= b.dist(pick) { (start, false) } else { (end, true) })
    }

    /// Lengthens line or arc `curve` at the end nearer to `pick`, along itself, to the nearest
    /// curve in its way. The end is then held on that curve.
    pub fn extend(&mut self, curve: EntityId, pick: Vec2) -> SketchResult<()> {
        if self.is_held_outside(curve) {
            return Err(invalid("projected geometry follows the part and cannot be extended: break its link first"));
        }
        let (moving, at_end) = self.end_near(curve, pick)?;
        if self.curves_at(moving).len() > 1 {
            return Err(invalid("that end is joined to another curve: there is nothing to extend"));
        }
        let target = self.carrier(curve).ok_or_else(|| invalid("only a line or an arc has an end to extend"))?;
        let arc = self.arc(curve);
        // How far past the end a place on the curve's own line or circle is (None: not past it).
        let beyond = |p: Vec2| -> Option<f64> {
            match (self.line(curve), arc) {
                (Some((a, b)), _) => {
                    let t = Line::new(a, b).param_of(p);
                    let d = if at_end { t - 1.0 } else { -t };
                    (d > PARAM_EPS).then_some(d)
                }
                (None, Some(a)) => {
                    let angle = (p - a.center).angle();
                    let d = if at_end { norm_angle(angle - a.start) - a.sweep() } else { norm_angle(a.start - angle) };
                    (d > PARAM_EPS && d < TAU - a.sweep() - PARAM_EPS).then_some(d)
                }
                _ => None,
            }
        };
        let mut best: Option<(f64, Vec2, EntityId)> = None;
        let others: Vec<EntityId> = self.entities().filter(|(id, e)| *id != curve && e.geometry.is_curve()).map(|(id, _)| id).collect();
        for other in others {
            for p in self.crossings(target, other) {
                if let Some(d) = beyond(p)
                    && best.is_none_or(|b| d < b.0)
                {
                    best = Some((d, p, other));
                }
            }
        }
        let Some((_, to, boundary)) = best else {
            return Err(invalid("there is no curve beyond that end for it to reach"));
        };
        let mut s = self.clone();
        s.set_point(moving, to)?;
        s.push_constraint(Constraint::PointOnCurve { point: moving, curve: boundary })?;
        GaussNewton
            .solve(&mut s, &SolveOptions { pin: vec![moving], ..Default::default() })
            .map_err(|e| invalid(format!("it cannot be made longer: a constraint or a dimension holds it as it is ({e})")))?;
        *self = s;
        Ok(())
    }

    /// Divides `curve` (a line, an arc or a circle) where another curve crosses it, at the
    /// crossing nearest to `pick`. The two pieces share a point held on the crossing curve; a
    /// line's pieces stay in line. Returns the new piece. (A circle needs two crossings: it
    /// becomes two arcs.)
    pub fn split(&mut self, curve: EntityId, pick: Vec2) -> SketchResult<EntityId> {
        if self.is_held_outside(curve) {
            return Err(invalid("projected geometry follows the part and cannot be split: break its link first"));
        }
        let target = self.carrier(curve).ok_or_else(|| invalid("only lines, arcs and circles can be split"))?;
        let arc = self.arc(curve);
        let line = self.line(curve);
        let param = |p: Vec2| -> f64 {
            match (line, arc) {
                (Some((a, b)), _) => Line::new(a, b).param_of(p),
                (None, Some(a)) => norm_angle((p - a.center).angle() - a.start),
                _ => norm_angle((p - self.circle(curve).map_or(Vec2::ZERO, |c| c.0)).angle()),
            }
        };
        let span = match (line, arc) {
            (Some(_), _) => 1.0,
            (None, Some(a)) => a.sweep(),
            _ => TAU,
        };
        let whole = line.is_none() && arc.is_none();
        let mut cuts: Vec<(f64, Vec2, EntityId)> = Vec::new();
        let others: Vec<EntityId> = self.entities().filter(|(id, e)| *id != curve && e.geometry.is_curve()).map(|(id, _)| id).collect();
        for other in others {
            for p in self.crossings(target, other) {
                let t = param(p);
                let inside = whole || (t > PARAM_EPS && t < span - PARAM_EPS);
                if inside && !cuts.iter().any(|(u, _, _)| (u - t).abs() < PARAM_EPS) {
                    cuts.push((t, p, other));
                }
            }
        }
        let tp = param(pick);
        // (Round a circle, the far side of 0 is near.)
        let away = |t: f64| if whole { (t - tp).abs().min(TAU - (t - tp).abs()) } else { (t - tp).abs() };
        cuts.sort_by(|a, b| away(a.0).total_cmp(&away(b.0)));
        let Some(&(_, at, cutter)) = cuts.first() else {
            return Err(invalid("nothing crosses it to split it at"));
        };
        let mut s = self.clone();
        let p = s.add_point(at)?;
        let new = match s.geometry(curve).cloned() {
            Some(Geometry::Line { start, end }) => {
                // A length it was given is now the two pieces' together: from end to end.
                let lengths: Vec<(crate::ConstraintId, f64)> = s
                    .constraints()
                    .filter_map(|(id, c)| match c {
                        Constraint::Length { line, value } if *line == curve => Some((id, *value)),
                        _ => None,
                    })
                    .collect();
                for (id, value) in lengths {
                    s.replace_constraint(id, Constraint::Distance { a: start, b: end, value })?;
                }
                // What compared the whole line with something else no longer says anything of a piece.
                let gone: Vec<crate::ConstraintId> = s
                    .constraints()
                    .filter(|(_, c)| {
                        matches!(c, Constraint::Equal { a, b } if *a == curve || *b == curve)
                            || matches!(c, Constraint::Midpoint { line, .. } if *line == curve)
                    })
                    .map(|(id, _)| id)
                    .collect();
                for id in gone {
                    s.remove_constraint(id)?;
                }
                s.replace_point(curve, end, p);
                let new = s.insert(Geometry::Line { start: p, end })?;
                let level: Vec<Constraint> = s
                    .constraints()
                    .filter(|(_, c)| matches!(c, Constraint::Horizontal { line } | Constraint::Vertical { line } if *line == curve))
                    .map(|(_, c)| c.map_refs(|r| if r == curve { new } else { r }))
                    .collect();
                if level.is_empty() {
                    s.push_constraint(Constraint::Parallel { a: curve, b: new })?;
                }
                for c in level {
                    s.push_constraint(c)?;
                }
                new
            }
            Some(Geometry::Arc { center, end, .. }) => {
                s.replace_point(curve, end, p);
                s.insert(Geometry::Arc { center, start: p, end })?
            }
            Some(Geometry::Circle { center, .. }) => {
                let Some(&(_, at2, cutter2)) = cuts.get(1) else {
                    return Err(invalid("a circle is split where two curves cross it (or one crosses it twice): there is only one crossing"));
                };
                let q = s.add_point(at2)?;
                // Counter-clockwise from the first crossing to the second, and on round.
                if let Some(g) = s.geometry_mut(curve) {
                    *g = Geometry::Arc { center, start: p, end: q };
                }
                s.push_constraint(Constraint::PointOnCurve { point: q, curve: cutter2 })?;
                s.insert(Geometry::Arc { center, start: q, end: p })?
            }
            _ => return Err(invalid("only lines, arcs and circles can be split")),
        };
        let e = self.entity(curve).ok_or(SketchError::NoEntity(curve))?;
        s.set_construction(new, e.construction)?;
        s.push_constraint(Constraint::PointOnCurve { point: p, curve: cutter })?;
        s.validate()?;
        *self = s;
        Ok(new)
    }

    /// Copies of `ids` in rows: `n1` along `d1` (the original counted) `s1` apart, and when
    /// `second` is given that row repeated `n2` times along `d2`, `s2` apart. The copies are
    /// geometry of their own, each holding its shape as [`Sketch::transform`] copies do; they do
    /// not follow the original afterwards. Returns the copies.
    pub fn pattern_rect(&mut self, ids: &[EntityId], d1: Vec2, n1: u32, s1: f64, second: Option<(Vec2, u32, f64)>) -> SketchResult<Vec<EntityId>> {
        let (d2, n2, s2) = second.unwrap_or((Vec2::new(0.0, 1.0), 1, 0.0));
        let unit = |d: Vec2| (d.is_finite() && d.len() > 1e-9).then(|| d.normalized());
        let (Some(u1), Some(u2)) = (unit(d1), unit(d2)) else { return Err(invalid("a pattern needs a direction")) };
        let total = u64::from(n1) * u64::from(n2);
        if n1 == 0 || n2 == 0 || total < 2 {
            return Err(invalid("a pattern needs at least two in all, the original counted"));
        }
        if total > MAX_PATTERN as u64 {
            return Err(invalid(format!("a sketch pattern makes at most {MAX_PATTERN}")));
        }
        if !(s1.is_finite() && s2.is_finite()) || (n1 > 1 && s1.abs() < tol::MIN_SIZE) || (n2 > 1 && s2.abs() < tol::MIN_SIZE) {
            return Err(invalid("the spacing of a pattern must be a distance other than zero"));
        }
        let mut s = self.clone();
        let mut out = Vec::new();
        for j in 0..n2 {
            for i in 0..n1 {
                if i == 0 && j == 0 {
                    continue;
                }
                let by = u1 * (f64::from(i) * s1) + u2 * (f64::from(j) * s2);
                out.extend(s.transform(ids, Motion::Shift(by), true)?);
            }
        }
        *self = s;
        Ok(out)
    }

    /// Copies of `ids` turned about `about`: `count` in all (the original counted) over `angle`
    /// radians, a whole turn dividing evenly and less than one reaching from first to last. As
    /// with [`Sketch::pattern_rect`], the copies are geometry of their own. Returns the copies.
    pub fn pattern_circular(&mut self, ids: &[EntityId], about: Vec2, count: u32, angle: f64) -> SketchResult<Vec<EntityId>> {
        if count < 2 {
            return Err(invalid("a pattern needs at least two in all, the original counted"));
        }
        if count as usize > MAX_PATTERN {
            return Err(invalid(format!("a sketch pattern makes at most {MAX_PATTERN}")));
        }
        if !(angle.is_finite() && angle.abs() > 1e-9 && angle.abs() <= TAU + 1e-9) {
            return Err(invalid("the angle of a pattern is more than nothing and at most a whole turn"));
        }
        let whole = (angle.abs() - TAU).abs() < 1e-9;
        let step = if whole { angle / f64::from(count) } else { angle / f64::from(count - 1) };
        let mut s = self.clone();
        let mut out = Vec::new();
        for i in 1..count {
            out.extend(s.transform(ids, Motion::Turn { about, angle: step * f64::from(i) }, true)?);
        }
        *self = s;
        Ok(out)
    }

    /// An arc that carries on from the free end `from` of a line or an arc, tangent to it
    /// there, round to `to`. It shares the end point and is held tangent. Returns the arc.
    pub fn add_arc_tangent(&mut self, from: EntityId, to: Vec2) -> SketchResult<EntityId> {
        let p = self.point(from).ok_or_else(|| invalid("a tangent arc starts at the end point of a line or an arc"))?;
        let curves = self.curves_at(from);
        // The way the curve is heading as it arrives at the point.
        let leaving = |id: EntityId| -> Option<Vec2> {
            match self.geometry(id)? {
                Geometry::Line { start, end } if *end == from => Some((p - self.point(*start)?).normalized()),
                Geometry::Line { start, end } if *start == from => Some((p - self.point(*end)?).normalized()),
                Geometry::Arc { center, end, .. } if *end == from => Some((p - self.point(*center)?).normalized().perp()),
                Geometry::Arc { center, start, .. } if *start == from => Some(-(p - self.point(*center)?).normalized().perp()),
                _ => None,
            }
        };
        let Some((curve, t)) = curves.iter().find_map(|c| leaving(*c).map(|t| (*c, t))) else {
            return Err(invalid("a tangent arc starts at the end point of a line or an arc"));
        };
        let (n, d) = (t.perp(), to - p);
        let across = 2.0 * d.dot(n);
        if d.len() <= tol::MIN_SIZE || across.abs() <= 1e-9 * d.len().max(1.0) {
            return Err(invalid("the arc's end is straight ahead of where it starts (or on it): that is a line"));
        }
        let radius = d.dot(d) / across;
        let center = p + n * radius;
        let mut s = self.clone();
        // Turning left it runs counter-clockwise from the shared point; turning right, to it.
        let arc = if radius > 0.0 { s.add_arc(center, from, to)? } else { s.add_arc(center, to, from)? };
        s.push_constraint(Constraint::Tangent { a: curve, b: arc })?;
        *self = s;
        Ok(arc)
    }
}
