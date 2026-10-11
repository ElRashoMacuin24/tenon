//! The part shown in a sketch: edges projected onto the sketch plane as reference geometry
//! (DEC-055). The sketch's own geometry is constrained and dimensioned to it, and it bounds
//! profiles like any curve.
//!
//! A projection is found again at every regeneration by the name of its edge, and the reference
//! geometry is put where the edge now is; the rest of the sketch follows by its constraints.
//! Nothing here needs a kernel: an edge comes as its curve, its ends and a point halfway along.

use std::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};
use tenon_geom::{Frame, Vec2, Vec3};
use tenon_kernel::CurveKind;
use tenon_sketch::{EntityId, Geometry, Sketch};

use crate::naming::EdgeRef;

/// Closer than this (mm) is the same place.
const SAME: f64 = 1e-7;
/// Reference geometry further than this (mm) from where its edge is has moved. (Coarser than
/// the nine decimals a file keeps, so reading a file back moves nothing.)
const MOVED: f64 = 2e-9;
/// A direction within this of the plane's normal (or of lying in the plane) counts as that.
const SQUARE: f64 = 1e-9;

/// An edge of the part projected into a sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    /// The sketch entity that stands for the edge: a line, a circle, an arc, or a point (a
    /// straight edge seen end on).
    pub entity: EntityId,
    pub edge: EdgeRef,
}

/// An edge as the kernel (or a scene) describes it.
#[derive(Clone, Copy, Debug)]
pub struct EdgeGeom<'a> {
    pub curve: &'a CurveKind,
    pub ends: [Vec3; 2],
    /// A point on the edge between its ends (which way round an arc goes).
    pub mid: Vec3,
}

/// What an edge is on a sketch plane, in sketch coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Projected {
    /// A straight edge seen end on.
    Point {
        at: Vec2,
    },
    Line {
        start: Vec2,
        end: Vec2,
    },
    Circle {
        center: Vec2,
        radius: f64,
    },
    /// Counter-clockwise from `start` to `end`.
    Arc {
        center: Vec2,
        start: Vec2,
        end: Vec2,
    },
}

impl Projected {
    fn kind(&self) -> &'static str {
        match self {
            Projected::Point { .. } => "point",
            Projected::Line { .. } => "line",
            Projected::Circle { .. } => "circle",
            Projected::Arc { .. } => "arc",
        }
    }
}

/// "a line", "an arc".
fn a_kind(kind: &str) -> String {
    format!("{} {kind}", if kind.starts_with('a') { "an" } else { "a" })
}

/// How far round from angle `from` to angle `to`, counter-clockwise, in [0, 2 pi).
fn sweep(from: f64, to: f64) -> f64 {
    (to - from).rem_euclid(TAU)
}

/// The edge as it looks from square on to the plane of `frame`.
pub fn project(edge: &EdgeGeom, frame: &Frame) -> Result<Projected, String> {
    let flat = |p: Vec3| {
        let l = frame.to_local(p);
        Vec2::new(l.x, l.y)
    };
    let [a, b] = edge.ends;
    match edge.curve {
        CurveKind::Line { .. } => {
            let (p, q) = (flat(a), flat(b));
            Ok(if p.dist(q) < SAME { Projected::Point { at: p } } else { Projected::Line { start: p, end: q } })
        }
        CurveKind::Circle { axis, radius } => {
            let (n, c3, r) = (frame.z(), axis.origin(), *radius);
            let along = axis.dir().dot(n).abs();
            let closed = a.dist(b) < SAME;
            if along > 1.0 - SQUARE {
                // Seen square on: a circle, or an arc that goes the way its middle point says.
                let c = flat(c3);
                if closed {
                    return Ok(Projected::Circle { center: c, radius: r });
                }
                let (s, e, m) = (flat(a), flat(b), flat(edge.mid));
                let ang = |p: Vec2| (p - c).angle();
                let ccw = sweep(ang(s), ang(m)) < sweep(ang(s), ang(e));
                Ok(if ccw { Projected::Arc { center: c, start: s, end: e } } else { Projected::Arc { center: c, start: e, end: s } })
            } else if along < SQUARE {
                // Seen edge on: a straight line as long as the edge is wide.
                let u = axis.dir().cross(n).normalized();
                let v = axis.dir().cross(u);
                let (lo, hi) = if closed {
                    (-r, r)
                } else {
                    // Where the edge is along `u`: at its ends, and at the circle's two extremes
                    // when it passes them.
                    let ang = |p: Vec3| (p - c3).dot(v).atan2((p - c3).dot(u));
                    let t = |p: Vec3| (p - c3).dot(u);
                    let (sa, ea, ma) = (ang(a), ang(b), ang(edge.mid));
                    let ccw = sweep(sa, ma) < sweep(sa, ea);
                    let (from, span) = if ccw { (sa, sweep(sa, ea)) } else { (ea, sweep(ea, sa)) };
                    let passes = |at: f64| sweep(from, at) <= span;
                    let (mut lo, mut hi) = (t(a).min(t(b)), t(a).max(t(b)));
                    if passes(0.0) {
                        hi = r;
                    }
                    if passes(PI) {
                        lo = -r;
                    }
                    (lo, hi)
                };
                let (p, q) = (flat(c3 + u * lo), flat(c3 + u * hi));
                Ok(if p.dist(q) < SAME { Projected::Point { at: p } } else { Projected::Line { start: p, end: q } })
            } else {
                Err("it is a circle seen at an angle, which would be an ellipse: sketches do not have those yet".into())
            }
        }
        _ => Err("only straight and circular edges can be projected yet".into()),
    }
}

/// The points of a curve, by role: a line's start and end, a circle's centre, an arc's centre,
/// start and end.
fn points_of(sketch: &Sketch, entity: EntityId) -> Vec<EntityId> {
    sketch.geometry(entity).map(Geometry::points).unwrap_or_default()
}

/// Adds `p` to the sketch as reference geometry. Returns the entity that stands for the edge.
pub fn add(sketch: &mut Sketch, p: &Projected) -> Result<EntityId, String> {
    let err = |e: tenon_sketch::SketchError| e.to_string();
    let entity = match *p {
        Projected::Point { at } => sketch.add_point(at),
        Projected::Line { start, end } => sketch.add_line(start, end),
        Projected::Circle { center, radius } => sketch.add_circle(center, radius),
        Projected::Arc { center, start, end } => sketch.add_arc(center, start, end),
    }
    .map_err(err)?;
    for id in points_of(sketch, entity).into_iter().chain([entity]) {
        sketch.set_reference(id, true).map_err(err)?;
    }
    Ok(entity)
}

/// Puts reference entity `entity` where `p` is. True when anything moved; an error when the
/// edge is no longer the kind of curve the entity is.
pub fn place(sketch: &mut Sketch, entity: EntityId, p: &Projected) -> Result<bool, String> {
    let err = |e: tenon_sketch::SketchError| e.to_string();
    let was = sketch.geometry(entity).cloned().ok_or("the projected geometry is gone")?;
    let mut moved = false;
    let mut put = |sketch: &mut Sketch, id: EntityId, at: Vec2| -> Result<(), String> {
        if sketch.point(id).is_none_or(|now| now.dist(at) > MOVED) {
            sketch.set_point(id, at).map_err(err)?;
            moved = true;
        }
        Ok(())
    };
    match (was, *p) {
        (Geometry::Point { .. }, Projected::Point { at }) => put(sketch, entity, at)?,
        (Geometry::Line { start, end }, Projected::Line { start: a, end: b }) => {
            // Kept the way round it was, so what is attached to an end stays at that end.
            let (now_a, now_b) = (sketch.point(start), sketch.point(end));
            let swap = match (now_a, now_b) {
                (Some(x), Some(y)) => x.dist(b) + y.dist(a) < x.dist(a) + y.dist(b),
                _ => false,
            };
            let (a, b) = if swap { (b, a) } else { (a, b) };
            put(sketch, start, a)?;
            put(sketch, end, b)?;
        }
        (Geometry::Circle { center, radius: was_r }, Projected::Circle { center: c, radius }) => {
            put(sketch, center, c)?;
            if (was_r - radius).abs() > MOVED {
                sketch.set_radius(entity, radius).map_err(err)?;
                moved = true;
            }
        }
        (Geometry::Arc { center, start, end }, Projected::Arc { center: c, start: s, end: e }) => {
            put(sketch, center, c)?;
            put(sketch, start, s)?;
            put(sketch, end, e)?;
        }
        (was, now) => return Err(format!("the edge is now {}, and was {} when it was projected", a_kind(now.kind()), a_kind(was.kind_name()))),
    }
    Ok(moved)
}

/// What became of a sketch's projections at a regeneration.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    /// The sketch with its reference geometry where the part now has it, solved again.
    pub sketch: Sketch,
    /// Projections whose edge was not found or cannot be shown: the entity, and why. Their
    /// geometry stays where it was.
    pub lost: Vec<(EntityId, String)>,
    /// Anything moved.
    pub moved: bool,
}

/// Finds every projection of `sketch` again with `find` (an edge reference to what the edge is
/// on the sketch plane), puts the reference geometry there and solves the sketch.
pub fn resolve(sketch: &Sketch, projections: &[Projection], mut find: impl FnMut(&EdgeRef) -> Result<Projected, String>) -> Result<Resolved, String> {
    let mut out = Resolved { sketch: sketch.clone(), lost: Vec::new(), moved: false };
    for p in projections {
        match find(&p.edge).and_then(|at| place(&mut out.sketch, p.entity, &at)) {
            Ok(moved) => out.moved |= moved,
            Err(why) => out.lost.push((p.entity, why)),
        }
    }
    if out.moved {
        out.sketch.solve().map_err(|e| format!("its projected geometry moved with the part and the sketch cannot follow: {e}"))?;
    }
    Ok(out)
}

/// Brings `sketch` up to `resolved` (the same sketch as a regeneration left it): its reference
/// geometry is put where `resolved` has it and the sketch solved. True when anything changed;
/// on an error the sketch is as it was.
pub fn follow(sketch: &mut Sketch, resolved: &Sketch) -> Result<bool, String> {
    let err = |e: tenon_sketch::SketchError| e.to_string();
    let mut next = sketch.clone();
    let mut moved = false;
    let refs: Vec<EntityId> = next.entities().filter(|(_, e)| e.reference).map(|(id, _)| id).collect();
    for id in refs {
        match (next.geometry(id).cloned(), resolved.geometry(id)) {
            (Some(Geometry::Point { pos }), Some(Geometry::Point { pos: at })) if pos.dist(*at) > MOVED => {
                next.set_point(id, *at).map_err(err)?;
                moved = true;
            }
            (Some(Geometry::Circle { radius, .. }), Some(Geometry::Circle { radius: r, .. })) if (radius - r).abs() > MOVED => {
                next.set_radius(id, *r).map_err(err)?;
                moved = true;
            }
            _ => {}
        }
    }
    if moved {
        next.solve().map_err(err)?;
        *sketch = next;
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use tenon_geom::Axis;

    use super::*;

    fn v3(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    /// The top of a block 10 high, seen from above: X to the right, Y up.
    fn top() -> Frame {
        Frame::new(v3(0.0, 0.0, 10.0), Vec3::Z, Vec3::X).unwrap()
    }

    #[test]
    fn straight_edges_are_lines_or_points_seen_end_on() {
        let line = CurveKind::Line { origin: v3(0.0, 0.0, 0.0), dir: Vec3::X };
        // An edge of the bottom of the block, 10 below the plane: where it is, seen from above.
        let e = EdgeGeom { curve: &line, ends: [v3(0.0, 5.0, 0.0), v3(40.0, 5.0, 0.0)], mid: v3(20.0, 5.0, 0.0) };
        assert_eq!(project(&e, &top()).unwrap(), Projected::Line { start: Vec2::new(0.0, 5.0), end: Vec2::new(40.0, 5.0) });
        // An upright edge is a point.
        let up = EdgeGeom { curve: &line, ends: [v3(40.0, 20.0, 0.0), v3(40.0, 20.0, 10.0)], mid: v3(40.0, 20.0, 5.0) };
        assert_eq!(project(&up, &top()).unwrap(), Projected::Point { at: Vec2::new(40.0, 20.0) });
    }

    #[test]
    fn round_edges_are_circles_arcs_lines_or_refused() {
        let hole = CurveKind::Circle { axis: Axis::new(v3(20.0, 10.0, 0.0), Vec3::Z).unwrap(), radius: 6.0 };
        let rim = v3(26.0, 10.0, 0.0);
        let whole = EdgeGeom { curve: &hole, ends: [rim, rim], mid: v3(14.0, 10.0, 0.0) };
        assert_eq!(project(&whole, &top()).unwrap(), Projected::Circle { center: Vec2::new(20.0, 10.0), radius: 6.0 });
        // A half of it, from the east point to the west: by the north or by the south, as the
        // point between them says, whichever way the edge itself runs.
        let (east, west, north, south) = (rim, v3(14.0, 10.0, 0.0), v3(20.0, 16.0, 0.0), v3(20.0, 4.0, 0.0));
        let (e2, w2, c2) = (Vec2::new(26.0, 10.0), Vec2::new(14.0, 10.0), Vec2::new(20.0, 10.0));
        let half = |ends: [Vec3; 2], mid: Vec3| project(&EdgeGeom { curve: &hole, ends, mid }, &top()).unwrap();
        assert_eq!(half([east, west], north), Projected::Arc { center: c2, start: e2, end: w2 });
        assert_eq!(half([west, east], north), Projected::Arc { center: c2, start: e2, end: w2 });
        assert_eq!(half([east, west], south), Projected::Arc { center: c2, start: w2, end: e2 });
        assert_eq!(half([west, east], south), Projected::Arc { center: c2, start: w2, end: e2 });
        // From below the plane's normal is the other way, and so is counter-clockwise.
        let below = Frame::new(v3(0.0, 0.0, 0.0), -Vec3::Z, Vec3::X).unwrap();
        let p = project(&EdgeGeom { curve: &hole, ends: [east, west], mid: north }, &below).unwrap();
        let Projected::Arc { start, end, center } = p else { panic!("{p:?}") };
        let north2 = {
            let l = below.to_local(north);
            Vec2::new(l.x, l.y)
        };
        let ang = |q: Vec2| (q - center).angle();
        assert!(sweep(ang(start), ang(north2)) < sweep(ang(start), ang(end)), "the arc still passes the north point: {p:?}");

        // Seen edge on (from the front), the whole circle is a line as long as it is wide.
        let front = Frame::new(Vec3::ZERO, -Vec3::Y, Vec3::X).unwrap();
        assert_eq!(project(&whole, &front).unwrap(), Projected::Line { start: Vec2::new(14.0, 0.0), end: Vec2::new(26.0, 0.0) });
        // The northern half reaches both extremes; a quarter from north to east only its own.
        assert_eq!(half_on(&hole, [east, west], north, &front), Projected::Line { start: Vec2::new(14.0, 0.0), end: Vec2::new(26.0, 0.0) });
        let ne = v3(20.0 + 6.0 * 0.5f64.sqrt(), 10.0 + 6.0 * 0.5f64.sqrt(), 0.0);
        let Projected::Line { start: p, end: q } = half_on(&hole, [north, east], ne, &front) else { panic!() };
        assert!(p.near(Vec2::new(20.0, 0.0), 1e-9) && q.near(Vec2::new(26.0, 0.0), 1e-9), "{p:?} {q:?}");
        // At an angle it would be an ellipse.
        let tilted = Frame::new(Vec3::ZERO, v3(0.0, 1.0, 1.0), Vec3::X).unwrap();
        assert!(project(&whole, &tilted).unwrap_err().contains("ellipse"));
        let spline = CurveKind::BSpline;
        assert!(project(&EdgeGeom { curve: &spline, ends: [east, west], mid: north }, &top()).unwrap_err().contains("straight and circular"));
    }

    fn half_on(curve: &CurveKind, ends: [Vec3; 2], mid: Vec3, frame: &Frame) -> Projected {
        project(&EdgeGeom { curve, ends, mid }, frame).unwrap()
    }

    #[test]
    fn reference_geometry_is_added_moved_and_followed() {
        let mut s = Sketch::new();
        let own = s.add_circle(Vec2::new(5.0, 5.0), 2.0).unwrap();
        let Geometry::Circle { center, .. } = s.geometry(own).cloned().unwrap() else { panic!() };
        let edge = add(&mut s, &Projected::Line { start: Vec2::new(0.0, 0.0), end: Vec2::new(40.0, 0.0) }).unwrap();
        let hole = add(&mut s, &Projected::Circle { center: Vec2::new(30.0, 10.0), radius: 3.0 }).unwrap();
        let arc = add(&mut s, &Projected::Arc { center: Vec2::new(0.0, 20.0), start: Vec2::new(5.0, 20.0), end: Vec2::new(0.0, 25.0) }).unwrap();
        let dot = add(&mut s, &Projected::Point { at: Vec2::new(40.0, 20.0) }).unwrap();
        assert_eq!(s.entities().filter(|(_, e)| e.reference).count(), 3 + 2 + 4 + 1);
        assert_eq!(s.dof().unwrap().dof, 3, "only the sketch's own circle is free");
        s.add_constraint(tenon_sketch::Constraint::Distance { a: center, b: edge, value: 8.0 }).unwrap();
        assert!((s.point(center).unwrap().y - 8.0).abs() < 1e-9);

        // The edge is found 5 further up; the circle is larger. What was the same stays unmoved.
        let before = s.clone();
        assert!(!place(&mut s, dot, &Projected::Point { at: Vec2::new(40.0, 20.0) }).unwrap());
        assert!(place(&mut s, edge, &Projected::Line { start: Vec2::new(40.0, 5.0), end: Vec2::new(0.0, 5.0) }).unwrap());
        assert_eq!(s.line(edge).unwrap(), (Vec2::new(0.0, 5.0), Vec2::new(40.0, 5.0)), "the same way round as before");
        assert!(place(&mut s, hole, &Projected::Circle { center: Vec2::new(30.0, 10.0), radius: 4.5 }).unwrap());
        assert_eq!(s.circle(hole).unwrap().1, 4.5);
        // An edge that is another kind of curve now is not followed.
        let e = place(&mut s, arc, &Projected::Line { start: Vec2::ZERO, end: Vec2::new(1.0, 0.0) }).unwrap_err();
        assert!(e.contains("is now a line") && e.contains("was an arc"), "{e}");

        // `resolve` does the same for a list, solves, and says what it lost.
        let pick = |i: u32| {
            EdgeRef::new(
                crate::FaceOrigin::Cap { feature: crate::FeatureId(i), end: crate::CapEnd::Start },
                crate::FaceOrigin::Cap { feature: crate::FeatureId(i), end: crate::CapEnd::End },
                crate::EdgeFingerprint { mid: Vec3::ZERO, length: 0.0 },
            )
        };
        let list =
            vec![Projection { entity: edge, edge: pick(1) }, Projection { entity: hole, edge: pick(2) }, Projection { entity: arc, edge: pick(3) }];
        let r = resolve(&before, &list, |e| match e.faces[0].feature().0 {
            1 => Ok(Projected::Line { start: Vec2::new(0.0, 5.0), end: Vec2::new(40.0, 5.0) }),
            2 => Ok(Projected::Circle { center: Vec2::new(30.0, 10.0), radius: 3.0 }),
            _ => Err("no edge joins those faces any more".into()),
        })
        .unwrap();
        assert!(r.moved && r.lost.len() == 1 && r.lost[0].0 == arc && r.lost[0].1.contains("no edge"), "{:?}", r.lost);
        assert!((r.sketch.point(center).unwrap().y - 13.0).abs() < 1e-9, "the circle is still 8 from the edge: {:?}", r.sketch.point(center));
        assert_eq!(r.sketch.arc(arc).map(|a| a.radius), before.arc(arc).map(|a| a.radius), "what was lost stays where it was");
        // The document's sketch follows what regeneration found.
        let mut doc = before.clone();
        assert!(follow(&mut doc, &r.sketch).unwrap());
        assert_eq!(doc, r.sketch);
        assert!(!follow(&mut doc, &r.sketch).unwrap());
    }
}
