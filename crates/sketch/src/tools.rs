//! Sketch editing tools: fillet, trim, offset, mirror. Each works on a copy and commits only
//! when it succeeds, so a failed tool leaves the sketch unchanged.

use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

use tenon_geom::{Circle, Line, Vec2, circle_circle, line_circle, line_line_infinite, norm_angle, tol};

use crate::sketch::{SketchError, SketchResult};
use crate::{Constraint, EntityId, Geometry, Sketch};

fn invalid(msg: impl Into<String>) -> SketchError {
    SketchError::Invalid(msg.into())
}

/// Parameters closer than this are the same split point.
const PARAM_EPS: f64 = 1e-9;

/// A curve that trim and offset understand.
#[derive(Clone, Copy, Debug)]
enum Carrier {
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
}

impl Sketch {
    /// Rounds the corner where exactly two lines meet at `corner` with a tangent arc. Adds tangent
    /// and radius constraints. Returns the arc.
    pub fn fillet(&mut self, corner: EntityId, radius: f64) -> SketchResult<EntityId> {
        let c = self.point(corner).ok_or(SketchError::NoEntity(corner))?;
        if !tol::is_valid_size(radius) {
            return Err(invalid(format!("fillet radius {radius} is out of range")));
        }
        let lines: Vec<EntityId> = self.curves_at(corner);
        let [l1, l2] = lines.as_slice() else {
            return Err(invalid("a fillet needs exactly two lines meeting at the point"));
        };
        let (l1, l2) = (*l1, *l2);
        let other = |s: &Sketch, l: EntityId| -> Option<Vec2> {
            match s.geometry(l)? {
                Geometry::Line { start, end } => s.point(if *start == corner { *end } else { *start }),
                _ => None,
            }
        };
        let (Some(o1), Some(o2)) = (other(self, l1), other(self, l2)) else {
            return Err(invalid("a fillet needs exactly two lines meeting at the point"));
        };
        let (u1, u2) = ((o1 - c).normalized(), (o2 - c).normalized());
        let theta = u1.dot(u2).clamp(-1.0, 1.0).acos();
        if !(1e-6..PI - 1e-6).contains(&theta) {
            return Err(invalid("the lines are parallel"));
        }
        let t = radius / (theta / 2.0).tan();
        if t >= c.dist(o1) - tol::MIN_SIZE || t >= c.dist(o2) - tol::MIN_SIZE {
            return Err(invalid(format!("radius {radius} is too large for these lines")));
        }
        let (t1, t2) = (c + u1 * t, c + u2 * t);
        let center = c + (u1 + u2).normalized() * (radius / (theta / 2.0).sin());

        let mut s = self.clone();
        let (p1, p2) = (s.add_point(t1)?, s.add_point(t2)?);
        s.replace_point(l1, corner, p1);
        s.replace_point(l2, corner, p2);
        let cp = s.add_point(center)?;
        let (start, end) = if (t1 - center).cross(t2 - center) > 0.0 { (p1, p2) } else { (p2, p1) };
        let arc = s.insert(Geometry::Arc { center: cp, start, end })?;
        s.drop_if_unused(corner);
        s.push_constraint(Constraint::Tangent { a: l1, b: arc })?;
        s.push_constraint(Constraint::Tangent { a: l2, b: arc })?;
        s.push_constraint(Constraint::Radius { curve: arc, value: radius })?;
        s.solve()?;
        *self = s;
        Ok(arc)
    }

    fn carrier(&self, id: EntityId) -> Option<Carrier> {
        if let Some((a, b)) = self.line(id) {
            return Some(Carrier::Line(a, b));
        }
        self.circle(id).map(|(c, r)| Carrier::Circle(c, r))
    }

    /// Points where `other` crosses `target`'s carrier, restricted to `other`'s extent.
    fn crossings(&self, target: Carrier, other: EntityId) -> Vec<Vec2> {
        let Some(oc) = self.carrier(other) else { return vec![] };
        let on_other = |p: Vec2| -> bool {
            match self.geometry(other) {
                Some(Geometry::Arc { .. }) => self.arc(other).is_some_and(|a| a.contains_angle((p - a.center).angle())),
                _ => true,
            }
        };
        let pts: Vec<Vec2> = match (target, oc) {
            (Carrier::Line(a, b), Carrier::Line(c, d)) => line_line_infinite(a, b, c, d)
                .filter(|(_, _, u)| (-PARAM_EPS..=1.0 + PARAM_EPS).contains(u))
                .map(|(p, _, _)| vec![p])
                .unwrap_or_default(),
            (Carrier::Line(a, b), Carrier::Circle(c, r)) => line_circle(&Line::new(a, b), &Circle::new(c, r)).into_iter().map(|(p, _)| p).collect(),
            (Carrier::Circle(c, r), Carrier::Line(a, b)) => line_circle(&Line::new(a, b), &Circle::new(c, r))
                .into_iter()
                .filter(|(_, t)| (-PARAM_EPS..=1.0 + PARAM_EPS).contains(t))
                .map(|(p, _)| p)
                .collect(),
            (Carrier::Circle(c1, r1), Carrier::Circle(c2, r2)) => circle_circle(&Circle::new(c1, r1), &Circle::new(c2, r2)),
        };
        pts.into_iter().filter(|p| on_other(*p)).collect()
    }

    /// Deletes the piece of `curve` (line, arc or circle) between the intersections around `pick`.
    /// Without intersections the whole curve is deleted. New end points stay attached to the curve
    /// that cut them (point-on-curve constraints).
    pub fn trim(&mut self, curve: EntityId, pick: Vec2) -> SketchResult<()> {
        let target = self.carrier(curve).ok_or_else(|| invalid("only lines, arcs and circles can be trimmed"))?;
        // Parameter of a point along the target: line 0..1, arc 0..sweep from its start, circle angle.
        let arc = self.arc(curve);
        let param = |p: Vec2| -> f64 {
            match target {
                Carrier::Line(a, b) => Line::new(a, b).param_of(p),
                Carrier::Circle(c, _) => match arc {
                    Some(a) => norm_angle((p - c).angle() - a.start),
                    None => norm_angle((p - c).angle()),
                },
            }
        };
        let span = match (target, arc) {
            (Carrier::Line(..), _) => 1.0,
            (_, Some(a)) => a.sweep(),
            _ => TAU,
        };
        let mut cuts: Vec<(f64, EntityId)> = Vec::new();
        let ids: Vec<EntityId> = self.entities().filter(|(id, e)| *id != curve && e.geometry.is_curve()).map(|(id, _)| id).collect();
        for other in ids {
            for p in self.crossings(target, other) {
                let t = param(p);
                if t > PARAM_EPS && t < span - PARAM_EPS && !cuts.iter().any(|(u, _)| (u - t).abs() < PARAM_EPS) {
                    cuts.push((t, other));
                }
            }
        }
        cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let tp = param(pick).clamp(0.0, span);
        let at = |t: f64| -> Vec2 {
            match target {
                Carrier::Line(a, b) => a.lerp(b, t),
                Carrier::Circle(c, r) => Vec2::polar(c, r, arc.map_or(0.0, |a| a.start) + t),
            }
        };
        let mut s = self.clone();
        let full_circle = matches!(target, Carrier::Circle(..)) && arc.is_none();
        if full_circle {
            if cuts.len() < 2 {
                s.delete(&[curve]);
                *self = s;
                return Ok(());
            }
            // Keep the arc from the cut after `pick` round to the cut before it.
            let after = cuts.iter().find(|(t, _)| *t > tp).or_else(|| cuts.first()).copied();
            let before = cuts.iter().rev().find(|(t, _)| *t < tp).or_else(|| cuts.last()).copied();
            let (Some((ta, ca)), Some((tb, cb))) = (after, before) else { return Err(invalid("cannot trim this circle")) };
            let Some(Geometry::Circle { center, .. }) = s.geometry(curve).cloned() else { return Err(invalid("not a circle")) };
            let start = s.add_point(at(ta))?;
            let end = s.add_point(at(tb))?;
            if let Some(g) = s.geometry_mut(curve) {
                *g = Geometry::Arc { center, start, end };
            }
            s.push_constraint(Constraint::PointOnCurve { point: start, curve: ca })?;
            s.push_constraint(Constraint::PointOnCurve { point: end, curve: cb })?;
            *self = s;
            return Ok(());
        }
        let lo = cuts.iter().rev().find(|(t, _)| *t < tp).copied();
        let hi = cuts.iter().find(|(t, _)| *t > tp).copied();
        let (first, last) = match s.geometry(curve) {
            Some(Geometry::Line { start, end }) | Some(Geometry::Arc { start, end, .. }) => (*start, *end),
            _ => return Err(invalid("cannot trim this curve")),
        };
        match (lo, hi) {
            (None, None) => {
                s.delete(&[curve]);
            }
            (None, Some((th, ch))) => {
                let p = s.add_point(at(th))?;
                s.replace_point(curve, first, p);
                s.push_constraint(Constraint::PointOnCurve { point: p, curve: ch })?;
                s.drop_if_unused(first);
            }
            (Some((tl, cl)), None) => {
                let p = s.add_point(at(tl))?;
                s.replace_point(curve, last, p);
                s.push_constraint(Constraint::PointOnCurve { point: p, curve: cl })?;
                s.drop_if_unused(last);
            }
            (Some((tl, cl)), Some((th, ch))) => {
                let (pl, ph) = (s.add_point(at(tl))?, s.add_point(at(th))?);
                // The curve keeps [start, lo]; a new curve takes [hi, end].
                let rest = match s.geometry(curve).cloned() {
                    Some(Geometry::Line { .. }) => Geometry::Line { start: ph, end: last },
                    Some(Geometry::Arc { center, .. }) => Geometry::Arc { center, start: ph, end: last },
                    _ => return Err(invalid("cannot trim this curve")),
                };
                s.replace_point(curve, last, pl);
                let new = s.insert(rest)?;
                // A horizontal/vertical line stays so on both pieces.
                let copies: Vec<Constraint> = s
                    .constraints()
                    .filter(|(_, c)| matches!(c, Constraint::Horizontal { line } | Constraint::Vertical { line } if *line == curve))
                    .map(|(_, c)| c.map_refs(|r| if r == curve { new } else { r }))
                    .collect();
                for c in copies {
                    s.push_constraint(c)?;
                }
                s.push_constraint(Constraint::PointOnCurve { point: pl, curve: cl })?;
                s.push_constraint(Constraint::PointOnCurve { point: ph, curve: ch })?;
            }
        }
        *self = s;
        Ok(())
    }

    /// Offsets lines, arcs and circles by `distance`. Joined curves are offset as chains whose
    /// corners meet again; positive distances go outward for closed chains and to the left of
    /// open ones. Offset arcs share the original centre. Returns the new curves.
    pub fn offset(&mut self, curves: &[EntityId], distance: f64) -> SketchResult<Vec<EntityId>> {
        if !(distance.is_finite() && tol::is_valid_size(distance.abs())) {
            return Err(invalid(format!("offset distance {distance} is out of range")));
        }
        let mut s = self.clone();
        let mut created = Vec::new();
        let mut segs: Vec<EntityId> = Vec::new();
        for id in curves {
            match self.geometry(*id) {
                Some(Geometry::Circle { center, radius }) => {
                    let r = radius + distance;
                    if !tol::is_valid_size(r) {
                        return Err(invalid("the offset would collapse a circle"));
                    }
                    let c = s.insert(Geometry::Circle { center: *center, radius: r })?;
                    created.push(c);
                }
                Some(Geometry::Line { .. } | Geometry::Arc { .. }) => segs.push(*id),
                Some(_) => return Err(invalid("only lines, arcs and circles can be offset")),
                None => return Err(SketchError::NoEntity(*id)),
            }
        }
        for chain in self.chains(&segs) {
            created.extend(s.offset_chain(self, &chain, distance)?);
        }
        *self = s;
        Ok(created)
    }

    /// Endpoints (start, end) of a line or arc.
    fn ends(&self, id: EntityId) -> Option<(EntityId, EntityId)> {
        match self.geometry(id)? {
            Geometry::Line { start, end } | Geometry::Arc { start, end, .. } => Some((*start, *end)),
            _ => None,
        }
    }

    /// Groups curves into chains joined at shared points: `(curve, forward)` in walking order.
    fn chains(&self, segs: &[EntityId]) -> Vec<Vec<(EntityId, bool)>> {
        let mut left: Vec<EntityId> = segs.to_vec();
        let mut out = Vec::new();
        while let Some(first) = left.first().copied() {
            left.retain(|x| *x != first);
            let mut chain = vec![(first, true)];
            // Extend forward from the end, then backward from the start.
            for forward in [true, false] {
                loop {
                    let tip = if forward {
                        chain.last().and_then(|(c, f)| self.ends(*c).map(|(a, b)| if *f { b } else { a }))
                    } else {
                        chain.first().and_then(|(c, f)| self.ends(*c).map(|(a, b)| if *f { a } else { b }))
                    };
                    let Some(tip) = tip else { break };
                    let Some(pos) = left.iter().position(|c| self.ends(*c).is_some_and(|(a, b)| a == tip || b == tip)) else { break };
                    let next = left.remove(pos);
                    let Some((a, _)) = self.ends(next) else { break };
                    if forward {
                        chain.push((next, a == tip));
                    } else {
                        chain.insert(0, (next, a != tip));
                    }
                }
            }
            out.push(chain);
        }
        out
    }

    fn offset_chain(&mut self, orig: &Sketch, chain: &[(EntityId, bool)], distance: f64) -> SketchResult<Vec<EntityId>> {
        // Walk points: start of each piece in walking order.
        let point = |id: EntityId| orig.point(id).ok_or(SketchError::NoEntity(id));
        let walk_start = |(c, f): (EntityId, bool)| orig.ends(c).map(|(a, b)| if f { a } else { b });
        let walk_end = |(c, f): (EntityId, bool)| orig.ends(c).map(|(a, b)| if f { b } else { a });
        let closed = matches!((chain.first().and_then(|x| walk_start(*x)), chain.last().and_then(|x| walk_end(*x))), (Some(a), Some(b)) if a == b);
        // Side: closed chains go outward for positive distances.
        let mut poly = Vec::new();
        for &(c, f) in chain {
            let mut pts = orig.tessellate(c, 0.1);
            if !f {
                pts.reverse();
            }
            poly.extend(pts);
        }
        let left = if closed && tenon_geom::shoelace(&poly) > 0.0 { -distance } else { distance };
        // Offset carrier and its offset end positions for each piece.
        let mut pieces = Vec::new();
        for &(c, f) in chain {
            let (s0, e0) = orig.ends(c).ok_or(SketchError::NoEntity(c))?;
            let (ws, we) = if f { (point(s0)?, point(e0)?) } else { (point(e0)?, point(s0)?) };
            match orig.geometry(c) {
                Some(Geometry::Line { .. }) => {
                    let n = (we - ws).normalized().perp() * left;
                    pieces.push((Carrier::Line(ws + n, we + n), ws + n, we + n, None));
                }
                Some(Geometry::Arc { center, .. }) => {
                    let cc = point(*center)?;
                    let r = ws.dist(cc);
                    // Walking an arc forward (counter-clockwise), its left side is the centre.
                    let nr = if f { r - left } else { r + left };
                    if !tol::is_valid_size(nr) {
                        return Err(invalid("the offset would collapse an arc"));
                    }
                    let on = |p: Vec2| cc + (p - cc).normalized() * nr;
                    pieces.push((Carrier::Circle(cc, nr), on(ws), on(we), Some(*center)));
                }
                _ => return Err(invalid("only lines and arcs can be chained")),
            }
        }
        // Corner points: where neighbouring offset carriers meet, nearest the offset ends.
        let n = pieces.len();
        let joint = |a: &(Carrier, Vec2, Vec2, Option<EntityId>), b: &(Carrier, Vec2, Vec2, Option<EntityId>)| -> Vec2 {
            let guess = a.2.mid(b.1);
            let cands: Vec<Vec2> = match (a.0, b.0) {
                (Carrier::Line(p, q), Carrier::Line(r, t)) => line_line_infinite(p, q, r, t).map(|x| vec![x.0]).unwrap_or_default(),
                (Carrier::Line(p, q), Carrier::Circle(c, r)) | (Carrier::Circle(c, r), Carrier::Line(p, q)) => {
                    line_circle(&Line::new(p, q), &Circle::new(c, r)).into_iter().map(|x| x.0).collect()
                }
                (Carrier::Circle(c1, r1), Carrier::Circle(c2, r2)) => circle_circle(&Circle::new(c1, r1), &Circle::new(c2, r2)),
            };
            cands.into_iter().min_by(|x, y| x.dist(guess).total_cmp(&y.dist(guess))).unwrap_or(guess)
        };
        let mut corners: Vec<Vec2> = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let p = if i == 0 || i == n {
                if closed && n > 1 {
                    joint(&pieces[n - 1], &pieces[0])
                } else if i == 0 {
                    pieces[0].1
                } else {
                    pieces[n - 1].2
                }
            } else {
                joint(&pieces[i - 1], &pieces[i])
            };
            corners.push(p);
        }
        let mut ids: Vec<EntityId> = corners.iter().take(if closed { n } else { n + 1 }).map(|p| self.add_point(*p)).collect::<SketchResult<_>>()?;
        if closed && let Some(first) = ids.first().copied() {
            ids.push(first);
        }
        let mut out = Vec::new();
        for (i, piece) in pieces.iter().enumerate() {
            let (a, b) = (ids[i], ids[i + 1]);
            let g = match piece.3 {
                None => Geometry::Line { start: a, end: b },
                Some(center) => {
                    // Keep the arc counter-clockwise.
                    let f = chain[i].1;
                    if f { Geometry::Arc { center, start: a, end: b } } else { Geometry::Arc { center, start: b, end: a } }
                }
            };
            out.push(self.insert(g)?);
        }
        Ok(out)
    }

    /// Mirrors entities across the line `axis`, adding symmetric constraints between original and
    /// mirrored points. Points on the axis are shared. Returns the new entities.
    pub fn mirror(&mut self, ids: &[EntityId], axis: EntityId) -> SketchResult<Vec<EntityId>> {
        let (la, lb) = self.line(axis).ok_or_else(|| invalid("the mirror axis must be a line"))?;
        let mut s = self.clone();
        let mut map: BTreeMap<EntityId, EntityId> = BTreeMap::new();
        let mut created = Vec::new();
        let mut mirror_point = |s: &mut Sketch, p: EntityId, created: &mut Vec<EntityId>| -> SketchResult<EntityId> {
            if let Some(m) = map.get(&p) {
                return Ok(*m);
            }
            let pos = self.point(p).ok_or(SketchError::NoEntity(p))?;
            let on_axis = (lb - la).normalized().cross(pos - la).abs() <= tol::LINEAR;
            let m = if on_axis {
                p
            } else {
                let m = s.add_point(pos.mirror(la, lb))?;
                s.push_constraint(Constraint::Symmetric { a: p, b: m, axis })?;
                created.push(m);
                m
            };
            map.insert(p, m);
            Ok(m)
        };
        for id in ids {
            if *id == axis {
                continue;
            }
            let e = self.entity(*id).ok_or(SketchError::NoEntity(*id))?.clone();
            let g = match &e.geometry {
                Geometry::Point { .. } => {
                    mirror_point(&mut s, *id, &mut created)?;
                    continue;
                }
                Geometry::Line { start, end } => {
                    Geometry::Line { start: mirror_point(&mut s, *start, &mut created)?, end: mirror_point(&mut s, *end, &mut created)? }
                }
                Geometry::Circle { center, radius } => Geometry::Circle { center: mirror_point(&mut s, *center, &mut created)?, radius: *radius },
                // Mirroring reverses orientation: swap the ends to stay counter-clockwise.
                Geometry::Arc { center, start, end } => Geometry::Arc {
                    center: mirror_point(&mut s, *center, &mut created)?,
                    start: mirror_point(&mut s, *end, &mut created)?,
                    end: mirror_point(&mut s, *start, &mut created)?,
                },
                Geometry::Spline { poles, degree } => Geometry::Spline {
                    poles: poles.iter().map(|p| mirror_point(&mut s, *p, &mut created)).collect::<SketchResult<_>>()?,
                    degree: *degree,
                },
            };
            let new = s.insert(g)?;
            s.set_construction(new, e.construction)?;
            if matches!(e.geometry, Geometry::Circle { .. } | Geometry::Arc { .. }) {
                s.push_constraint(Constraint::Equal { a: *id, b: new })?;
            }
            created.push(new);
        }
        *self = s;
        Ok(created)
    }
}
