use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use tenon_geom::{Frame, Vec2};

use crate::*;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-7
}

fn corners(s: &Sketch, lines: &[EntityId]) -> Vec<Vec2> {
    lines.iter().map(|l| s.line(*l).unwrap().0).collect()
}

/// 40 x 20 rectangle at the origin, fully constrained (width, height, fixed corner).
fn fixed_rect() -> (Sketch, [EntityId; 4], ConstraintId, ConstraintId) {
    let mut s = Sketch::new();
    let l = s.add_rectangle(v(1.0, 2.0), v(30.0, 25.0)).unwrap();
    let Geometry::Line { start: p0, end: p1 } = s.geometry(l[0]).cloned().unwrap() else { panic!() };
    let Geometry::Line { end: p2, .. } = s.geometry(l[1]).cloned().unwrap() else { panic!() };
    s.add_constraint(Constraint::Fix { point: p0 }).unwrap();
    s.set_point(p0, v(0.0, 0.0)).unwrap();
    s.solve().unwrap();
    let w = s.add_constraint(Constraint::HorizontalDistance { a: p0, b: p1, value: 40.0 }).unwrap();
    let h = s.add_constraint(Constraint::VerticalDistance { a: p1, b: p2, value: 20.0 }).unwrap();
    (s, l, w, h)
}

#[test]
fn rectangle_dof_counts_down_to_zero() {
    let mut s = Sketch::new();
    let l = s.add_rectangle(v(0.0, 0.0), v(10.0, 5.0)).unwrap();
    assert_eq!(s.constraint_count(), 4);
    assert_eq!(s.dof().unwrap().dof, 4, "8 point coordinates - 4 horizontal/vertical");
    let (s, _, _, _) = fixed_rect();
    let d = s.dof().unwrap();
    assert_eq!(d.dof, 0);
    for line in l {
        assert!(d.fully_constrained.contains(&line), "{line}");
    }
}

#[test]
fn dimensions_drive_geometry() {
    let (mut s, l, w, h) = fixed_rect();
    let c = corners(&s, &l);
    assert!(c[0].near(v(0.0, 0.0), 1e-7) && c[2].near(v(40.0, 20.0), 1e-7), "{c:?}");
    s.set_dimension(w, 55.0).unwrap();
    s.set_dimension(h, 12.5).unwrap();
    let c = corners(&s, &l);
    assert!(c[0].near(v(0.0, 0.0), 1e-7), "the fixed corner stays");
    assert!(c[2].near(v(55.0, 12.5), 1e-7), "{c:?}");
    assert!(s.set_dimension(w, f64::NAN).is_err());
    assert!(corners(&s, &l)[2].near(v(55.0, 12.5), 1e-7), "a rejected edit changes nothing");
}

#[test]
fn conflicting_and_redundant_constraints_are_refused() {
    let (mut s, l, w, _) = fixed_rect();
    let before = s.clone();
    match s.add_constraint(Constraint::Length { line: l[0], value: 30.0 }) {
        Err(SketchError::Conflict(ids)) => assert!(ids.contains(&w), "{ids:?}"),
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert_eq!(s, before, "the sketch is unchanged after a refused constraint");
    assert!(matches!(s.add_constraint(Constraint::Horizontal { line: l[0] }), Err(SketchError::Redundant(_))));
    assert_eq!(s, before);
}

#[test]
fn drag_keeps_constraints() {
    let mut s = Sketch::new();
    let l = s.add_rectangle(v(0.0, 0.0), v(10.0, 5.0)).unwrap();
    let Geometry::Line { end: corner, .. } = s.geometry(l[1]).cloned().unwrap() else { panic!() };
    s.drag(corner, v(14.0, 9.0)).unwrap();
    assert!(s.point(corner).unwrap().near(v(14.0, 9.0), 1e-6));
    let c = corners(&s, &l);
    assert!(close(c[0].y, c[1].y) && close(c[1].x, c[2].x) && close(c[2].y, c[3].y) && close(c[3].x, c[0].x), "still a rectangle: {c:?}");

    // A free line's endpoint goes exactly where it is dragged and nothing else moves.
    let mut s = Sketch::new();
    let a = s.add_point(v(0.0, 0.0)).unwrap();
    let line = s.add_line(a, v(5.0, 0.0)).unwrap();
    let Geometry::Line { end, .. } = s.geometry(line).cloned().unwrap() else { panic!() };
    s.drag(end, v(3.0, 4.0)).unwrap();
    assert!(s.point(end).unwrap().near(v(3.0, 4.0), 1e-9) && s.point(a).unwrap().near(v(0.0, 0.0), 1e-9));
}

#[test]
fn every_geometric_constraint_solves() {
    let mut s = Sketch::new();
    let l1 = s.add_line(v(0.0, 0.0), v(10.0, 1.0)).unwrap();
    let l2 = s.add_line(v(0.0, 5.0), v(8.0, 9.0)).unwrap();
    let c1 = s.add_circle(v(20.0, 3.0), 4.0).unwrap();
    let c2 = s.add_circle(v(22.0, 4.0), 2.0).unwrap();
    let p = s.add_point(v(3.0, 3.0)).unwrap();
    s.add_constraint(Constraint::Parallel { a: l1, b: l2 }).unwrap();
    s.add_constraint(Constraint::Horizontal { line: l1 }).unwrap();
    s.add_constraint(Constraint::Equal { a: l1, b: l2 }).unwrap();
    s.add_constraint(Constraint::Concentric { a: c1, b: c2 }).unwrap();
    s.add_constraint(Constraint::Midpoint { point: p, line: l1 }).unwrap();
    let (a1, b1) = s.line(l1).unwrap();
    let (a2, b2) = s.line(l2).unwrap();
    assert!(close(a1.y, b1.y) && close(a2.y, b2.y), "parallel to a horizontal line");
    assert!(close(a1.dist(b1), a2.dist(b2)));
    assert!(s.circle(c1).unwrap().0.near(s.circle(c2).unwrap().0, 1e-7));
    assert!(s.point(p).unwrap().near(a1.mid(b1), 1e-7));

    let l3 = s.add_line(v(30.0, 0.0), v(31.0, 10.0)).unwrap();
    s.add_constraint(Constraint::Perpendicular { a: l1, b: l3 }).unwrap();
    let (a3, b3) = s.line(l3).unwrap();
    assert!(close(a3.x, b3.x), "perpendicular to a horizontal line is vertical");

    s.add_constraint(Constraint::Tangent { a: l2, b: c1 }).unwrap();
    let (c, r) = s.circle(c1).unwrap();
    let (a2, b2) = s.line(l2).unwrap();
    assert!(close((b2 - a2).normalized().cross(c - a2).abs(), r), "line tangent to circle");
    s.add_constraint(Constraint::Diameter { curve: c2, value: 3.0 }).unwrap();
    assert!(close(s.circle(c2).unwrap().1, 1.5));
    s.add_constraint(Constraint::Radius { curve: c1, value: 6.0 }).unwrap();
    assert!(close(s.circle(c1).unwrap().1, 6.0));

    let q = s.add_point(v(40.0, 40.0)).unwrap();
    s.add_constraint(Constraint::PointOnCurve { point: q, curve: c1 }).unwrap();
    let (c, r) = s.circle(c1).unwrap();
    assert!(close(s.point(q).unwrap().dist(c), r));
}

#[test]
fn symmetric_angle_distance_collinear_coincident() {
    let mut s = Sketch::new();
    let axis = s.add_line(v(0.0, -10.0), v(0.0, 10.0)).unwrap();
    s.add_constraint(Constraint::Vertical { line: axis }).unwrap();
    let a = s.add_point(v(-3.0, 1.0)).unwrap();
    let b = s.add_point(v(5.0, 2.0)).unwrap();
    s.add_constraint(Constraint::Symmetric { a, b, axis }).unwrap();
    let (pa, pb) = (s.point(a).unwrap(), s.point(b).unwrap());
    let ax = s.line(axis).unwrap().0.x;
    assert!(close(pa.y, pb.y) && close(pa.x + pb.x, 2.0 * ax), "{pa:?} {pb:?}");

    let l1 = s.add_line(v(10.0, 0.0), v(20.0, 0.0)).unwrap();
    let l2 = s.add_line(v(10.0, 0.0), v(15.0, 3.0)).unwrap();
    s.add_constraint(Constraint::Horizontal { line: l1 }).unwrap();
    s.add_constraint(Constraint::Angle { a: l1, b: l2, value: FRAC_PI_4 }).unwrap();
    let c = Constraint::Angle { a: l1, b: l2, value: 0.0 };
    assert!(close(s.measure(&c).unwrap(), FRAC_PI_4));

    let p = s.add_point(v(12.0, 7.0)).unwrap();
    s.add_constraint(Constraint::Distance { a: p, b: l1, value: 4.0 }).unwrap();
    let (la, _) = s.line(l1).unwrap();
    assert!(close((s.point(p).unwrap().y - la.y).abs(), 4.0));

    let l3 = s.add_line(v(30.0, 1.0), v(40.0, 2.0)).unwrap();
    s.add_constraint(Constraint::Collinear { a: l1, b: l3 }).unwrap();
    let (la, _) = s.line(l1).unwrap(); // the solve may move l1 too
    let (a3, b3) = s.line(l3).unwrap();
    assert!(close(a3.y, la.y) && close(b3.y, la.y), "{a3:?} {b3:?} {la:?}");

    let q1 = s.add_point(v(50.0, 50.0)).unwrap();
    let q2 = s.add_point(v(51.0, 52.0)).unwrap();
    s.add_constraint(Constraint::Coincident { a: q1, b: q2 }).unwrap();
    assert!(s.point(q1).unwrap().near(s.point(q2).unwrap(), 1e-9));
}

#[test]
fn fillet_rounds_a_corner_tangentially() {
    let mut s = Sketch::new();
    let l = s.add_rectangle(v(0.0, 0.0), v(40.0, 20.0)).unwrap();
    let Geometry::Line { end: corner, .. } = s.geometry(l[0]).cloned().unwrap() else { panic!() };
    let arc = s.fillet(corner, 5.0).unwrap();
    assert!(close(s.circle(arc).unwrap().1, 5.0));
    assert!(s.point(corner).is_none(), "the sharp corner point is gone");
    let a = s.arc(arc).unwrap();
    assert!(close(a.sweep(), FRAC_PI_2));
    assert!(a.center.near(v(35.0, 5.0), 1e-7), "{:?}", a.center);
    let r = regions(&s);
    assert_eq!(r.len(), 1);
    let expected = 800.0 - (25.0 - 25.0 * PI / 4.0);
    assert!((r[0].area - expected).abs() < 0.05, "{} vs {expected}", r[0].area);
    assert!(s.fillet(EntityId(9999), 1.0).is_err());
    let Geometry::Line { start: other, .. } = s.geometry(l[0]).cloned().unwrap() else { panic!() };
    assert!(s.fillet(other, 100.0).is_err(), "radius too large for the lines");
}

#[test]
fn trim_lines_and_circles() {
    let mut s = Sketch::new();
    let h = s.add_line(v(-10.0, 0.0), v(10.0, 0.0)).unwrap();
    let vline = s.add_line(v(0.0, -10.0), v(0.0, 10.0)).unwrap();
    s.trim(h, v(6.0, 0.1)).unwrap();
    let (a, b) = s.line(h).unwrap();
    assert!(a.near(v(-10.0, 0.0), 1e-9) && b.near(v(0.0, 0.0), 1e-9), "{a:?} {b:?}");
    // Trimming the middle of a line crossed twice splits it in two.
    let mut s2 = Sketch::new();
    let long = s2.add_line(v(0.0, 0.0), v(30.0, 0.0)).unwrap();
    s2.add_line(v(10.0, -5.0), v(10.0, 5.0)).unwrap();
    s2.add_line(v(20.0, -5.0), v(20.0, 5.0)).unwrap();
    let before = s2.entity_count();
    s2.trim(long, v(15.0, 0.0)).unwrap();
    assert!(s2.line(long).unwrap().1.near(v(10.0, 0.0), 1e-9));
    assert_eq!(s2.entity_count(), before + 3, "one new line and two new points");

    // The circle is crossed at (-5, 0) by the trimmed line and at (0, +-5) by the vertical one;
    // picking near (5, 0) removes the right half between (0, -5) and (0, 5).
    let c = s.add_circle(v(0.0, 0.0), 5.0).unwrap();
    s.trim(c, v(5.0, 0.5)).unwrap();
    let arc = s.arc(c).expect("the circle became an arc");
    assert!(close(arc.sweep(), PI), "{}", arc.sweep());
    assert!(arc.start_point().near(v(0.0, 5.0), 1e-9), "{:?}", arc.start_point());
    let lone = s.add_circle(v(100.0, 100.0), 1.0).unwrap();
    s.trim(lone, v(101.0, 100.0)).unwrap();
    assert!(s.entity(lone).is_none(), "a circle without crossings is deleted");
    assert!(s.entity(vline).is_some());
}

#[test]
fn offset_rectangle_and_circle() {
    let mut s = Sketch::new();
    let l = s.add_rectangle(v(0.0, 0.0), v(40.0, 20.0)).unwrap();
    let out = s.offset(&l, 2.0).unwrap();
    assert_eq!(out.len(), 4);
    let pts: Vec<Vec2> = out.iter().map(|id| s.line(*id).unwrap().0).collect();
    let (lo, hi) = pts.iter().fold((v(1e9, 1e9), v(-1e9, -1e9)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    assert!(lo.near(v(-2.0, -2.0), 1e-9) && hi.near(v(42.0, 22.0), 1e-9), "outward: {lo:?} {hi:?}");
    let inner = s.offset(&l, -2.0).unwrap();
    let pts: Vec<Vec2> = inner.iter().map(|id| s.line(*id).unwrap().0).collect();
    let (lo, hi) = pts.iter().fold((v(1e9, 1e9), v(-1e9, -1e9)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    assert!(lo.near(v(2.0, 2.0), 1e-9) && hi.near(v(38.0, 18.0), 1e-9), "inward: {lo:?} {hi:?}");
    let c = s.add_circle(v(100.0, 0.0), 5.0).unwrap();
    let oc = s.offset(&[c], 1.5).unwrap();
    assert!(close(s.circle(oc[0]).unwrap().1, 6.5));
    assert!(s.offset(&[c], -5.0).is_err(), "cannot collapse a circle");
    assert_eq!(regions(&s).len(), 5, "three nested rectangles (a ring each) plus two circles");
}

#[test]
fn mirror_across_a_line() {
    let mut s = Sketch::new();
    let axis = s.add_line(v(0.0, 0.0), v(0.0, 10.0)).unwrap();
    s.set_construction(axis, true).unwrap();
    let Geometry::Line { start: on_axis, .. } = s.geometry(axis).cloned().unwrap() else { panic!() };
    let l = s.add_line(on_axis, v(5.0, 3.0)).unwrap();
    let arc = s.add_arc(v(5.0, 6.0), v(8.0, 6.0), v(5.0, 9.0)).unwrap();
    let new = s.mirror(&[l, arc], axis).unwrap();
    let ml = new.iter().copied().find(|id| s.is_line(*id)).unwrap();
    let (a, b) = s.line(ml).unwrap();
    assert!(a.near(v(0.0, 0.0), 1e-9) && b.near(v(-5.0, 3.0), 1e-9));
    let Geometry::Line { start, .. } = s.geometry(ml).cloned().unwrap() else { panic!() };
    assert_eq!(start, on_axis, "points on the axis are shared");
    let ma = s.arc(new.iter().copied().find(|id| s.arc(*id).is_some()).unwrap()).unwrap();
    assert!(ma.center.near(v(-5.0, 6.0), 1e-9) && close(ma.radius, 3.0) && close(ma.sweep(), FRAC_PI_2));
    s.solve().unwrap();
}

#[test]
fn regions_with_holes_splits_and_nesting() {
    let mut s = Sketch::new();
    s.add_rectangle(v(0.0, 0.0), v(40.0, 40.0)).unwrap();
    let hole = s.add_circle(v(20.0, 20.0), 5.0).unwrap();
    let r = regions(&s);
    assert_eq!(r.len(), 2);
    let ring = r.iter().find(|x| x.depth == 0).unwrap();
    let disk = r.iter().find(|x| x.depth == 1).unwrap();
    assert_eq!(ring.holes, vec![vec![hole]]);
    assert!((ring.area - (1600.0 - 25.0 * PI)).abs() < 0.5, "{}", ring.area);
    assert!((disk.area - 25.0 * PI).abs() < 0.5);
    assert!(ring.contains(v(2.0, 2.0)) && !ring.contains(v(20.0, 20.0)) && disk.contains(v(20.0, 20.0)));
    let d = default_regions(&r);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].key, ring.key);

    // The profile carries entity ids as tags.
    let p = profile(&s, Frame::WORLD, &d);
    assert_eq!(p.regions.len(), 1);
    assert_eq!(p.regions[0].outer.curves.len(), 4);
    assert_eq!(p.regions[0].holes[0].curves[0].tag, u64::from(hole.0));

    // A line across the rectangle splits it in two; construction and dangling lines don't count.
    let mut s = Sketch::new();
    let l = s.add_rectangle(v(0.0, 0.0), v(40.0, 20.0)).unwrap();
    let Geometry::Line { start: a, .. } = s.geometry(l[0]).cloned().unwrap() else { panic!() };
    let Geometry::Line { start: c, .. } = s.geometry(l[2]).cloned().unwrap() else { panic!() };
    s.add_line(a, c).unwrap();
    let cons = s.add_circle(v(100.0, 100.0), 3.0).unwrap();
    s.set_construction(cons, true).unwrap();
    s.add_line(v(50.0, 0.0), v(60.0, 0.0)).unwrap();
    let r = regions(&s);
    assert_eq!(r.len(), 2, "{r:?}");
    assert!(r.iter().all(|x| (x.area - 400.0).abs() < 1e-6));
}

#[test]
fn polygon_and_three_point_arc() {
    let mut s = Sketch::new();
    let lines = s.add_polygon(v(0.0, 0.0), v(10.0, 0.0), 6).unwrap();
    assert_eq!(lines.len(), 6);
    s.solve().unwrap();
    let r = regions(&s);
    assert_eq!(r.len(), 1, "the construction circle is not a region");
    assert!((r[0].area - 1.5 * 3f64.sqrt() * 100.0).abs() < 1e-6);
    let arc = s.add_arc_three_point(v(20.0, 0.0), v(25.0, 5.0), v(30.0, 0.0)).unwrap();
    let a = s.arc(arc).unwrap();
    assert!(a.center.near(v(25.0, 0.0), 1e-9) && close(a.radius, 5.0));
    assert!(s.add_arc_three_point(v(0.0, 0.0), v(1.0, 1.0), v(2.0, 2.0)).is_err());
}

#[test]
fn serde_round_trip_and_validation() {
    let (s, _, _, _) = fixed_rect();
    let json = serde_json::to_string(&s).unwrap();
    let back: Sketch = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
    back.validate().unwrap();
    // A file whose line points at a missing entity is rejected by validation.
    let broken = json.replacen("\"start\":1", "\"start\":999", 1);
    assert_ne!(broken, json);
    let bad: Sketch = serde_json::from_str(&broken).unwrap();
    assert!(bad.validate().is_err());
}

#[test]
fn hostile_input_is_refused() {
    let mut s = Sketch::new();
    assert!(s.add_point(v(f64::NAN, 0.0)).is_err());
    assert!(s.add_point(v(1e300, 0.0)).is_err());
    assert!(s.add_line(v(0.0, 0.0), v(0.0, 0.0)).is_err());
    assert!(s.add_circle(v(0.0, 0.0), -1.0).is_err());
    assert!(s.add_rectangle(v(0.0, 0.0), v(0.0, 5.0)).is_err());
    assert!(s.add_polygon(v(0.0, 0.0), v(1.0, 0.0), 2).is_err());
    assert!(s.add_spline(&[v(0.0, 0.0).into(), v(1.0, 0.0).into()], 3).is_err());
    let l = s.add_line(v(0.0, 0.0), v(1.0, 0.0)).unwrap();
    assert!(s.add_constraint(Constraint::Radius { curve: l, value: 1.0 }).is_err());
    assert!(s.add_constraint(Constraint::Length { line: l, value: f64::INFINITY }).is_err());
    assert!(s.add_constraint(Constraint::Horizontal { line: EntityId(777) }).is_err());
    assert!(s.drag(l, v(1.0, 1.0)).is_err(), "only points can be dragged");
    assert!(s.trim(EntityId(777), v(0.0, 0.0)).is_err());
    assert!(s.offset(&[l], f64::NAN).is_err());
    assert!(s.mirror(&[l], EntityId(777)).is_err());
    assert_eq!(s.entity_count(), 3);
}

#[test]
fn delete_cleans_up_points_and_constraints() {
    let mut s = Sketch::new();
    let l = s.add_rectangle(v(0.0, 0.0), v(10.0, 10.0)).unwrap();
    let gone = s.delete(&[l[0]]);
    assert!(gone.contains(&l[0]));
    assert_eq!(s.constraint_count(), 3, "the bottom line's horizontal constraint went with it");
    assert_eq!(s.entity_count(), 4 + 3, "4 corner points are still used by the other lines");
    let Geometry::Line { start, .. } = s.geometry(l[1]).cloned().unwrap() else { panic!() };
    let gone = s.delete(&[start]);
    assert!(gone.contains(&l[1]), "deleting a point deletes the curves using it");
    s.validate().unwrap();
}

#[test]
fn a_dimension_keeps_the_place_it_was_put_until_it_is_gone() {
    let (mut s, lines, w, h) = fixed_rect();
    assert_eq!(s.place(w), None, "beside its geometry until it is placed");
    s.set_place(w, v(20.0, -8.0)).unwrap();
    s.set_place(h, v(48.0, 10.0)).unwrap();
    assert_eq!((s.place(w), s.place(h)), (Some(v(20.0, -8.0)), Some(v(48.0, 10.0))));
    // Only dimensions have a place, and only a real position will do.
    let level = s.constraints().find(|(_, c)| matches!(c, Constraint::Horizontal { .. })).map(|(id, _)| id).unwrap();
    assert!(matches!(s.set_place(level, v(1.0, 1.0)), Err(SketchError::Invalid(_))));
    assert!(matches!(s.set_place(ConstraintId(999), v(1.0, 1.0)), Err(SketchError::NoConstraint(_))));
    assert!(matches!(s.set_place(w, v(f64::NAN, 0.0)), Err(SketchError::Invalid(_))));
    s.validate().unwrap();
    // It is saved with the sketch, and a sketch with nothing placed says nothing of places.
    let json = serde_json::to_value(&s).unwrap();
    assert_eq!(json["places"].as_array().map(Vec::len), Some(2));
    assert_eq!(serde_json::from_value::<Sketch>(json).unwrap(), s);
    assert!(serde_json::to_value(fixed_rect().0).unwrap().get("places").is_none());
    // Editing a dimension's value leaves it where it is; removing it, or what it measures,
    // forgets the place.
    s.set_dimension(w, 50.0).unwrap();
    assert_eq!(s.place(w), Some(v(20.0, -8.0)));
    s.remove_constraint(w).unwrap();
    assert_eq!(s.place(w), None);
    let corner = s.constraint(h).unwrap().refs()[1];
    s.delete(&[corner]);
    let _ = lines;
    assert!(s.constraint(h).is_none() && s.place(h).is_none());
    s.validate().unwrap();
}
