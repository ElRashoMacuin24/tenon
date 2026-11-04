//! A 3D sketch in the part (DEC-136): what ties its reference geometry to the part. The sketch
//! itself is `tenon_sketch::Sketch3d`.
//!
//! A vertex, a work point or an edge brought into the sketch is found again at every regeneration
//! and the reference geometry put where it now is; the rest of the sketch follows by its
//! constraints. So it is with an edge projected into a 2D sketch (`project.rs`), without the
//! plane.

use serde::{Deserialize, Serialize};
use tenon_geom::{Vec3, tol};
use tenon_kernel::{CurveKind, EdgeInfo};
use tenon_sketch::{EdgeCurve, EntityId, Geometry3, Sketch3d};

use crate::FeatureId;
use crate::document::PointSel;
use crate::naming::EdgeRef;

/// Closer than this (mm) is the same place. (As for projected geometry: coarser than the nine
/// decimals a file keeps, so reading a file back moves nothing.)
const MOVED: f64 = 2e-9;
/// How many pieces a circular edge is drawn in.
const ROUND_PIECES: usize = 64;

/// What reference geometry of a 3D sketch stands for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkTo {
    /// A point: a vertex of the part, a work point, the origin, a point of another sketch.
    Point(PointSel),
    /// An edge of the part.
    Edge(EdgeRef),
}

impl LinkTo {
    /// The features it comes from.
    pub fn features(&self) -> Vec<FeatureId> {
        match self {
            LinkTo::Point(p) => p.features(),
            LinkTo::Edge(e) => e.features(),
        }
    }
}

/// Reference geometry of a 3D sketch and what it stands for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link3 {
    pub entity: EntityId,
    pub to: LinkTo,
}

/// What a link has come to: where to put its reference geometry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Found3 {
    Point { at: Vec3 },
    Line { start: Vec3, end: Vec3 },
    Curve { start: Vec3, end: Vec3, curve: EdgeCurve },
}

/// Points round a circle about `center` with this `normal`, from `start` by `sweep`.
fn round_points(center: Vec3, normal: Vec3, start: Vec3, sweep: f64) -> Vec<Vec3> {
    let r = start - center;
    let n = normal.normalized();
    (0..=ROUND_PIECES)
        .map(|i| {
            let (s, c) = (sweep * i as f64 / ROUND_PIECES as f64).sin_cos();
            center + r * c + n.cross(r) * s
        })
        .collect()
}

/// What an edge the kernel describes is, as a 3D sketch has it. `along` is asked for only when the
/// edge is neither straight nor circular: the points of its mesh, from its start to its end.
pub fn edge_found(info: &EdgeInfo, along: impl FnOnce() -> Result<Vec<Vec3>, String>) -> Result<Found3, String> {
    if info.degenerate {
        return Err("the edge has collapsed to a point".into());
    }
    match &info.curve {
        CurveKind::Line { .. } => Ok(Found3::Line { start: info.start, end: info.end }),
        CurveKind::Circle { axis, .. } => {
            let center = axis.origin() + axis.dir() * (info.start - axis.origin()).dot(axis.dir());
            // Which way round, and how far: the point halfway says.
            let (a, m, b) = (info.start - center, info.mid - center, info.end - center);
            let whole = info.start.dist(info.end) <= tol::MIN_SIZE;
            let forward = axis.dir().dot(a.cross(m)) >= 0.0;
            let normal = if forward { axis.dir() } else { -axis.dir() };
            let mut sweep = if whole { std::f64::consts::TAU } else { normal.dot(a.cross(b)).atan2(a.dot(b)) };
            if sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
            let curve = EdgeCurve { mid: Some(info.mid), along: round_points(center, normal, info.start, sweep), length: info.length };
            Ok(Found3::Curve { start: info.start, end: info.end, curve })
        }
        _ => {
            let along = along()?;
            if along.len() < 2 {
                return Err("the edge has no points to show it by".into());
            }
            Ok(Found3::Curve { start: info.start, end: info.end, curve: EdgeCurve { mid: None, along, length: info.length } })
        }
    }
}

/// Reference geometry for `at`, new in the sketch.
pub fn add(sketch: &mut Sketch3d, at: &Found3) -> Result<EntityId, String> {
    match at {
        Found3::Point { at } => sketch.add_reference_point(*at),
        Found3::Line { start, end } => sketch.add_edge_line(*start, *end),
        Found3::Curve { start, end, curve } => sketch.add_edge(*start, *end, curve.clone()),
    }
    .map_err(|e| e.to_string())
}

/// Puts reference geometry `entity` where `at` says. True when that moved it; it stays a line, a
/// curve or a point, or is an error.
pub fn place(sketch: &mut Sketch3d, entity: EntityId, at: &Found3) -> Result<bool, String> {
    fn put(sketch: &mut Sketch3d, p: EntityId, to: Vec3) -> Result<bool, String> {
        let was = sketch.point(p).ok_or("its point is gone from the sketch")?;
        if was.dist(to) > MOVED {
            sketch.set_point(p, to).map_err(|e| e.to_string())?;
            return Ok(true);
        }
        Ok(false)
    }
    // Which end is which is kept from time to time: the kernel may name them the other way round.
    let swap = |sketch: &Sketch3d, a: EntityId, start: Vec3, end: Vec3| sketch.point(a).is_some_and(|was| was.dist(end) < was.dist(start));
    match (sketch.geometry(entity).cloned(), at) {
        (Some(Geometry3::Point { .. }), Found3::Point { at }) => put(sketch, entity, *at),
        (Some(Geometry3::Line { start, end }), Found3::Line { start: s, end: e }) => {
            let (s, e) = if swap(sketch, start, *s, *e) { (*e, *s) } else { (*s, *e) };
            let moved = put(sketch, start, s)?;
            Ok(put(sketch, end, e)? || moved)
        }
        (Some(Geometry3::Edge { start, end, curve: old }), Found3::Curve { start: s, end: e, curve }) => {
            let (s, e, curve) = if start != end && swap(sketch, start, *s, *e) {
                let mut turned = curve.clone();
                turned.along.reverse();
                (*e, *s, turned)
            } else {
                (*s, *e, curve.clone())
            };
            let mut moved = put(sketch, start, s)?;
            moved |= put(sketch, end, e)?;
            let same = old.along.len() == curve.along.len()
                && old.along.iter().zip(&curve.along).all(|(a, b)| a.dist(*b) <= MOVED)
                && (old.length - curve.length).abs() <= MOVED;
            if !same {
                sketch.set_edge_curve(entity, curve).map_err(|e| e.to_string())?;
                moved = true;
            }
            Ok(moved)
        }
        (None, _) => Err("it is gone from the sketch".into()),
        _ => Err("the edge has changed its kind: it was straight and is now curved, or the other way".into()),
    }
}

/// Brings `sketch` up to `resolved` (the same sketch as a regeneration left it): its reference
/// geometry is put where `resolved` has it and the sketch solved. True when anything changed; on
/// an error the sketch is as it was.
pub fn follow(sketch: &mut Sketch3d, resolved: &Sketch3d) -> Result<bool, String> {
    let err = |e: tenon_sketch::SketchError| e.to_string();
    let mut next = sketch.clone();
    let mut moved = false;
    let refs: Vec<EntityId> = next.entities().filter(|(_, e)| e.reference).map(|(id, _)| id).collect();
    for id in refs {
        match (next.geometry(id).cloned(), resolved.geometry(id)) {
            (Some(Geometry3::Point { pos }), Some(Geometry3::Point { pos: at })) if pos.dist(*at) > MOVED => {
                next.set_point(id, *at).map_err(err)?;
                moved = true;
            }
            (Some(Geometry3::Edge { curve, .. }), Some(Geometry3::Edge { curve: now, .. })) if curve != *now => {
                next.set_edge_curve(id, now.clone()).map_err(err)?;
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

/// The lines, arcs and splines of a sketch a sweep runs along by default: its own, not construction.
pub fn path_curves(sketch: &Sketch3d) -> Vec<EntityId> {
    sketch.entities().filter(|(_, e)| !e.construction && !e.reference && e.geometry.is_curve()).map(|(id, _)| id).collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use tenon_geom::Axis;

    use super::*;

    fn info(curve: CurveKind, start: Vec3, end: Vec3, mid: Vec3, length: f64) -> EdgeInfo {
        EdgeInfo { curve, length, start, end, mid, degenerate: false }
    }

    #[test]
    fn a_circular_edge_is_an_arc_through_its_middle_the_way_it_goes() {
        // A quarter circle of radius 10 about Z at height 5, from +X to +Y.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let axis = Axis::new(Vec3::ZERO, Vec3::Z).unwrap();
        let (a, m, b) = (Vec3::new(10.0, 0.0, 5.0), Vec3::new(10.0 * s, 10.0 * s, 5.0), Vec3::new(0.0, 10.0, 5.0));
        let found =
            edge_found(&info(CurveKind::Circle { axis, radius: 10.0 }, a, b, m, 5.0 * std::f64::consts::PI), || panic!("no points asked for"))
                .unwrap();
        let Found3::Curve { curve, .. } = &found else { panic!("{found:?}") };
        assert_eq!(curve.mid, Some(m));
        assert!(curve.along[ROUND_PIECES / 2].dist(m) < 1e-9, "{:?}", curve.along[ROUND_PIECES / 2]);
        assert!(curve.along[ROUND_PIECES].dist(b) < 1e-9);
        // The other way round about the same axis: three quarters of the circle.
        let m2 = Vec3::new(-10.0 * s, -10.0 * s, 5.0);
        let found = edge_found(&info(CurveKind::Circle { axis, radius: 10.0 }, a, b, m2, 15.0 * std::f64::consts::PI), || panic!()).unwrap();
        let Found3::Curve { curve, .. } = &found else { panic!("{found:?}") };
        assert!(curve.along[ROUND_PIECES / 2].dist(m2) < 1e-9, "{:?}", curve.along[ROUND_PIECES / 2]);
        assert!(curve.along[ROUND_PIECES].dist(b) < 1e-9);
    }

    #[test]
    fn reference_geometry_is_put_where_its_edge_is_and_keeps_its_ends() {
        let mut s = Sketch3d::new();
        let line = add(&mut s, &Found3::Line { start: Vec3::ZERO, end: Vec3::new(10.0, 0.0, 0.0) }).unwrap();
        let (a, b) = s.geometry(line).unwrap().ends().unwrap();
        // The same place: nothing moved.
        assert!(!place(&mut s, line, &Found3::Line { start: Vec3::ZERO, end: Vec3::new(10.0, 0.0, 0.0) }).unwrap());
        // Named the other way round by the kernel, and longer: each end to the place nearer it.
        assert!(place(&mut s, line, &Found3::Line { start: Vec3::new(15.0, 0.0, 0.0), end: Vec3::ZERO }).unwrap());
        assert_eq!(s.point(a), Some(Vec3::ZERO));
        assert_eq!(s.point(b), Some(Vec3::new(15.0, 0.0, 0.0)));
        // No longer straight: said, and it stays.
        let e = place(&mut s, line, &Found3::Point { at: Vec3::ZERO }).unwrap_err();
        assert!(e.contains("changed its kind"), "{e}");
    }

    #[test]
    fn what_is_held_to_reference_geometry_follows_it() {
        let mut doc_sketch = Sketch3d::new();
        let r = doc_sketch.add_reference_point(Vec3::new(1.0, 2.0, 3.0)).unwrap();
        let p = doc_sketch.add_point(Vec3::new(9.0, 2.0, 3.0)).unwrap();
        let line = doc_sketch.add_line(r, p).unwrap();
        doc_sketch.add_constraint(tenon_sketch::Constraint3::Along { line, axis: tenon_sketch::Axis3::X }).unwrap();
        let mut resolved = doc_sketch.clone();
        resolved.set_point(r, Vec3::new(1.0, 7.0, 3.0)).unwrap();
        assert!(follow(&mut doc_sketch, &resolved).unwrap());
        let q = doc_sketch.point(p).unwrap();
        assert!((q.y - 7.0).abs() < 1e-9, "{q:?}");
        assert!(!follow(&mut doc_sketch, &resolved).unwrap(), "and then there is nothing to follow");
    }
}
