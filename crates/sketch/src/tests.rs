use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use tenon_geom::{Frame, Vec2, norm_angle};

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
    assert_eq!(ring.holes.iter().map(|h| h.iter().map(|p| p.curve).collect::<Vec<_>>()).collect::<Vec<_>>(), vec![vec![hole]]);
    assert!((ring.area - (1600.0 - 25.0 * PI)).abs() < 0.5, "{}", ring.area);
    assert!((disk.area - 25.0 * PI).abs() < 0.5);
    assert!(ring.contains(v(2.0, 2.0)) && !ring.contains(v(20.0, 20.0)) && disk.contains(v(20.0, 20.0)));
    let d = default_regions(&s, &r);
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

/// The curve tags of a kernel loop, in order.
fn tags(l: &tenon_kernel::Loop) -> Vec<u64> {
    l.curves.iter().map(|c| c.tag).collect()
}

#[test]
fn a_line_through_a_circle_gives_two_halves_and_both_together_the_circle() {
    use tenon_kernel::Curve2;
    let mut s = Sketch::new();
    let c = s.add_circle(v(0.0, 0.0), 10.0).unwrap();
    // Left to right, sticking out both sides: its ends bound nothing.
    let l = s.add_line(v(-15.0, 0.0), v(15.0, 0.0)).unwrap();
    let all = regions(&s);
    assert_eq!(all.len(), 2, "{all:?}");
    let upper = all.iter().find(|r| r.contains(v(0.0, 5.0))).unwrap();
    let lower = all.iter().find(|r| r.contains(v(0.0, -5.0))).unwrap();
    for half in [upper, lower] {
        assert_eq!(half.key, vec![c, l]);
        assert!((half.area - 50.0 * PI).abs() < 0.5, "{}", half.area);
        assert_eq!((half.depth, half.nth), (0, 0));
        assert!(half.contains(half.inside));
    }
    // Told apart by the side of the line they are on: its left is above it.
    assert_eq!((upper.left.clone(), upper.right.clone()), (vec![c, l], vec![]));
    assert_eq!((lower.left.clone(), lower.right.clone()), (vec![c], vec![l]));
    assert_eq!(key_of(&s, &all, upper), RegionKey::Sided { left: vec![c, l], right: vec![], nth: 0 });
    assert_eq!(find(&s, &all, &key_of(&s, &all, lower)), vec![lower]);
    // The circle named alone is all of it, however it is divided.
    assert_eq!(find(&s, &all, &RegionKey::Curves(vec![c])).len(), 2);
    assert!(find(&s, &all, &RegionKey::Curves(vec![l])).is_empty());
    assert!(find(&s, &all, &RegionKey::Sided { left: vec![l], right: vec![c], nth: 0 }).is_empty());

    // One half: half the circle and the stretch of line across it.
    let p = profile(&s, Frame::WORLD, &[upper]);
    assert_eq!(p.regions.len(), 1);
    let curves = &p.regions[0].outer.curves;
    assert_eq!(curves.len(), 2, "{curves:?}");
    let arc = curves.iter().find(|x| x.tag == u64::from(c.0)).unwrap();
    let Curve2::Arc { radius, start_angle, end_angle, .. } = arc.curve else { panic!("{arc:?}") };
    assert!(close(radius, 10.0) && close(start_angle, 0.0) && close(end_angle, PI), "{arc:?}");
    let chord = curves.iter().find(|x| x.tag == u64::from(l.0)).unwrap();
    let Curve2::Line { start, end } = chord.curve else { panic!("{chord:?}") };
    assert!(start.near(v(-10.0, 0.0), 1e-9) && end.near(v(10.0, 0.0), 1e-9), "{chord:?}");
    // Both halves, or what is used when nothing is picked: the circle whole, and no line.
    for both in [vec![upper, lower], vec![lower, upper], default_regions(&s, &all)] {
        let p = profile(&s, Frame::WORLD, &both);
        assert_eq!(p.regions.len(), 1);
        assert!(p.regions[0].holes.is_empty());
        assert_eq!(p.regions[0].outer.curves, vec![tenon_kernel::TaggedCurve2 { tag: u64::from(c.0), curve: s.curve2(c).unwrap() }]);
        assert_eq!(outlines(&s, &both).len(), 1);
    }
}

#[test]
fn a_rectangle_divided_by_a_line_resting_on_its_sides() {
    use tenon_kernel::Curve2;
    let mut s = Sketch::new();
    let r = s.add_rectangle(v(0.0, 0.0), v(40.0, 20.0)).unwrap();
    // From the middle of one long side to the middle of the other: it shares no point with them.
    let d = s.add_line(v(20.0, 0.0), v(20.0, 20.0)).unwrap();
    let all = regions(&s);
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all.iter().all(|x| (x.area - 400.0).abs() < 1e-9 && x.depth == 0));
    let left = all.iter().find(|x| x.contains(v(5.0, 5.0))).unwrap();
    let right = all.iter().find(|x| x.contains(v(35.0, 5.0))).unwrap();
    // Each half has a side the other has not, so its curves alone name it.
    assert_ne!(left.key, right.key);
    assert_eq!(key_of(&s, &all, left), RegionKey::Curves(left.key.clone()));
    assert_eq!(find(&s, &all, &RegionKey::Curves(right.key.clone())), vec![right]);
    // The rectangle's own four sides name both halves: what they enclose, whole.
    assert_eq!(find(&s, &all, &RegionKey::Curves(r.to_vec())).len(), 2);

    // One half: the long sides are used as far as the divider.
    let p = profile(&s, Frame::WORLD, &[left]);
    let curves = &p.regions[0].outer.curves;
    assert_eq!(curves.len(), 4, "{curves:?}");
    let lengths: Vec<f64> = curves
        .iter()
        .map(|c| match c.curve {
            Curve2::Line { start, end } => start.dist(end),
            _ => panic!("{c:?}"),
        })
        .collect();
    assert!(lengths.iter().all(|x| close(*x, 20.0)), "{lengths:?}");
    assert!(curves.iter().any(|c| c.tag == u64::from(d.0)));
    // Both: the four sides whole, as if the divider were not there.
    let p = profile(&s, Frame::WORLD, &[left, right]);
    assert_eq!(p.regions.len(), 1);
    let mut whole = tags(&p.regions[0].outer);
    whole.sort_unstable();
    assert_eq!(whole, r.iter().map(|x| u64::from(x.0)).collect::<Vec<_>>());
    for c in &p.regions[0].outer.curves {
        assert_eq!(Some(c.curve.clone()), s.curve2(EntityId(c.tag as u32)), "a side is the sketch's own line again");
    }
}

#[test]
fn overlapping_circles_give_a_lens_and_two_crescents() {
    let mut s = Sketch::new();
    let a = s.add_circle(v(0.0, 0.0), 10.0).unwrap();
    let b = s.add_circle(v(10.0, 0.0), 10.0).unwrap();
    let all = regions(&s);
    assert_eq!(all.len(), 3, "{all:?}");
    let lens = all.iter().find(|r| r.contains(v(5.0, 0.0))).unwrap();
    let first = all.iter().find(|r| r.contains(v(-5.0, 0.0))).unwrap();
    let second = all.iter().find(|r| r.contains(v(15.0, 0.0))).unwrap();
    // 2 r^2 acos(d / 2r) - d/2 sqrt(4 r^2 - d^2)
    let overlap = 200.0 * 0.5_f64.acos() - 5.0 * 300.0_f64.sqrt();
    assert!((lens.area - overlap).abs() < 0.5 && (first.area - (100.0 * PI - overlap)).abs() < 0.5, "{} {}", lens.area, first.area);
    // All three are bounded by both circles: inside both, or inside one and outside the other.
    assert!(all.iter().all(|r| r.key == vec![a, b] && r.nth == 0));
    assert_eq!((lens.left.clone(), lens.right.clone()), (vec![a, b], vec![]));
    assert_eq!((first.left.clone(), first.right.clone()), (vec![a], vec![b]));
    assert_eq!((second.left.clone(), second.right.clone()), (vec![b], vec![a]));
    for r in &all {
        assert_eq!(find(&s, &all, &key_of(&s, &all, r)), vec![r]);
    }
    // Everything together is the outline of both: the far arc of each circle, nothing between.
    let p = profile(&s, Frame::WORLD, &default_regions(&s, &all));
    assert_eq!(p.regions.len(), 1);
    assert_eq!(p.regions[0].outer.curves.len(), 2, "{:?}", p.regions[0].outer);
    // The lens and one crescent are that circle whole.
    let p = profile(&s, Frame::WORLD, &[lens, first]);
    assert_eq!(p.regions[0].outer.curves, vec![tenon_kernel::TaggedCurve2 { tag: u64::from(a.0), curve: s.curve2(a).unwrap() }]);
}

#[test]
fn touching_curves_meet_at_one_place_and_leave_no_sliver() {
    // A circle resting on a line, and one a hair into it.
    for dip in [0.0, 1e-9, -1e-9] {
        let mut s = Sketch::new();
        s.add_circle(v(0.0, 10.0 - dip), 10.0).unwrap();
        s.add_line(v(-20.0, 0.0), v(20.0, 0.0)).unwrap();
        let all = regions(&s);
        assert_eq!(all.len(), 1, "dip {dip}: {all:?}");
        assert!((all[0].area - 100.0 * PI).abs() < 0.5);
    }
    // A circle inside a square, touching all four sides: the disc and the four corners.
    let mut s = Sketch::new();
    s.add_rectangle(v(-10.0, -10.0), v(10.0, 10.0)).unwrap();
    let c = s.add_circle(v(0.0, 0.0), 10.0).unwrap();
    let all = regions(&s);
    assert_eq!(all.len(), 5, "{all:?}");
    let corner = 100.0 - 25.0 * PI;
    assert_eq!(all.iter().filter(|r| (r.area - corner).abs() < 0.2).count(), 4);
    let disc = all.iter().find(|r| r.contains(v(0.0, 0.0))).unwrap();
    assert_eq!((disc.key.clone(), disc.right.clone()), (vec![c], vec![]));
    // Two circles touching from outside stay two discs.
    let mut s = Sketch::new();
    s.add_circle(v(0.0, 0.0), 5.0).unwrap();
    s.add_circle(v(8.0, 0.0), 3.0).unwrap();
    let all = regions(&s);
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all.iter().all(|r| r.depth == 0));
}

#[test]
fn regions_alike_in_curves_and_sides_are_counted_along_a_curve() {
    let mut s = Sketch::new();
    // Two level lines across two circles about one centre: between the lines and between the
    // circles there is a region each side, with the same four curves on the same sides.
    let big = s.add_circle(v(0.0, 0.0), 10.0).unwrap();
    let small = s.add_circle(v(0.0, 0.0), 3.0).unwrap();
    let top = s.add_line(v(-15.0, 1.0), v(15.0, 1.0)).unwrap();
    let bottom = s.add_line(v(-15.0, -1.0), v(15.0, -1.0)).unwrap();
    let all = regions(&s);
    let mut key = vec![big, small, top, bottom];
    key.sort();
    let twins: Vec<&SketchRegion> = all.iter().filter(|r| r.key == key).collect();
    assert_eq!(twins.len(), 2, "{all:?}");
    assert_eq!((twins[0].left.clone(), twins[0].right.clone()), (twins[1].left.clone(), twins[1].right.clone()));
    assert_eq!((twins[0].nth, twins[1].nth), (0, 1));
    let (west, east) = (all.iter().find(|r| r.contains(v(-6.0, 0.0))).unwrap(), all.iter().find(|r| r.contains(v(6.0, 0.0))).unwrap());
    assert_ne!(west.nth, east.nth);
    for r in [west, east] {
        let k = key_of(&s, &all, r);
        assert!(matches!(&k, RegionKey::Sided { nth, .. } if *nth == r.nth), "{k:?}");
        assert_eq!(find(&s, &all, &k), vec![r]);
        assert_eq!(profile(&s, Frame::WORLD, &[r]).regions[0].outer.curves.len(), 4);
    }
    // The numbering holds when a size changes: the same region is found again.
    let before = key_of(&s, &all, east);
    let Geometry::Circle { .. } = s.geometry(small).cloned().unwrap() else { panic!() };
    s.add_constraint(Constraint::Radius { curve: small, value: 4.0 }).unwrap();
    s.solve().unwrap();
    let again = regions(&s);
    let found = find(&s, &again, &before);
    assert_eq!(found.len(), 1);
    assert!(found[0].contains(v(6.0, 0.0)) && !found[0].contains(v(-6.0, 0.0)));
}

#[test]
fn a_plain_key_keeps_its_holes_and_splines_are_not_divided() {
    // A plate with a hole, and a line through the hole only: the plate named by its sides is
    // still the plate with its hole, not the discs in it.
    let mut s = Sketch::new();
    let r = s.add_rectangle(v(0.0, 0.0), v(40.0, 40.0)).unwrap();
    let hole = s.add_circle(v(20.0, 20.0), 5.0).unwrap();
    s.add_line(v(14.0, 20.0), v(26.0, 20.0)).unwrap();
    let all = regions(&s);
    assert_eq!(all.len(), 3, "{all:?}");
    let plate = find(&s, &all, &RegionKey::Curves(r.to_vec()));
    assert_eq!(plate.len(), 1);
    assert!((plate[0].area - (1600.0 - 25.0 * PI)).abs() < 0.5 && plate[0].depth == 0);
    assert_eq!(plate[0].holes.len(), 1);
    assert_eq!(find(&s, &all, &RegionKey::Curves(vec![hole])).len(), 2, "both halves of the disc in the hole");
    // The profile of the plate: its hole is the circle whole, though the line cuts the circle.
    let p = profile(&s, Frame::WORLD, &plate);
    assert_eq!(tags(&p.regions[0].holes[0]), vec![u64::from(hole.0)]);
    assert_eq!(p.regions[0].holes[0].curves[0].curve, s.curve2(hole).unwrap());
    // The plate and the discs together: no hole.
    let p = profile(&s, Frame::WORLD, &all.iter().collect::<Vec<_>>());
    assert_eq!(p.regions.len(), 1);
    assert!(p.regions[0].holes.is_empty() && p.regions[0].outer.curves.len() == 4);

    // A spline joins at its ends only: a line across it does not divide what it bounds.
    let mut s = Sketch::new();
    let poles = [v(0.0, 0.0).into(), v(10.0, 15.0).into(), v(20.0, 15.0).into(), v(30.0, 0.0).into()];
    let sp = s.add_spline(&poles, 3).unwrap();
    let Geometry::Spline { poles: ids, .. } = s.geometry(sp).cloned().unwrap() else { panic!() };
    let chord = s.add_line(ids[3], ids[0]).unwrap();
    s.add_line(v(15.0, -5.0), v(15.0, 25.0)).unwrap();
    let all = regions(&s);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].key, vec![sp, chord]);
    let p = profile(&s, Frame::WORLD, &[&all[0]]);
    let curves = &p.regions[0].outer.curves;
    assert_eq!(curves.len(), 2, "the chord is one line again: {curves:?}");
    assert!(curves.iter().all(|c| Some(c.curve.clone()) == s.curve2(EntityId(c.tag as u32))));
}

#[test]
fn region_keys_are_written_as_a_list_or_by_side() {
    let plain: RegionKey = serde_json::from_str("[5, 6, 7]").unwrap();
    assert_eq!(plain, RegionKey::Curves(vec![EntityId(5), EntityId(6), EntityId(7)]));
    assert_eq!(serde_json::to_string(&plain).unwrap(), "[5,6,7]");
    let sided = RegionKey::Sided { left: vec![EntityId(3)], right: vec![EntityId(9)], nth: 0 };
    assert_eq!(serde_json::to_string(&sided).unwrap(), r#"{"left":[3],"right":[9]}"#);
    assert_eq!(serde_json::from_str::<RegionKey>(r#"{"left":[3],"right":[9]}"#).unwrap(), sided);
    let second = RegionKey::Sided { left: vec![EntityId(3)], right: vec![], nth: 1 };
    assert_eq!(serde_json::to_string(&second).unwrap(), r#"{"left":[3],"nth":1}"#);
    assert_eq!(serde_json::from_str::<RegionKey>(r#"{"left":[3],"nth":1}"#).unwrap(), second);
    assert_eq!(second.curves(), vec![EntityId(3)]);
    assert!(serde_json::from_str::<RegionKey>(r#"{"left":[3],"inside":[4]}"#).is_err());
    assert!(serde_json::from_str::<RegionKey>("7").is_err());
}

#[test]
fn a_driven_dimension_follows_the_sketch_and_holds_nothing() {
    let (mut s, l, w, h) = fixed_rect();
    assert_eq!(s.dof().unwrap().dof, 0);
    // The rectangle is fully held: its top side's length, or its diagonal, would be one too many.
    let top = Constraint::Length { line: l[2], value: 40.0 };
    assert!(matches!(s.add_constraint(top.clone()), Err(SketchError::Redundant(_))));
    let (p0, p2) = (s.line(l[0]).unwrap().0, s.line(l[1]).unwrap().1);
    let Geometry::Line { start: a, .. } = s.geometry(l[0]).cloned().unwrap() else { panic!() };
    let Geometry::Line { end: c, .. } = s.geometry(l[1]).cloned().unwrap() else { panic!() };
    assert!(p0.near(v(0.0, 0.0), 1e-9) && p2.near(v(40.0, 20.0), 1e-9));
    // As driven dimensions they are taken: they read what the sketch measures, whatever value
    // they were given, and leave its freedom as it was.
    let top = s.add_driven(Constraint::Length { line: l[2], value: 999.0 }).unwrap();
    let diagonal = s.add_driven(Constraint::Distance { a, b: c, value: 1.0 }).unwrap();
    assert!(s.is_driven(top) && s.is_driven(diagonal) && !s.is_driven(w));
    assert_eq!(s.driven().collect::<Vec<_>>(), vec![top, diagonal]);
    assert!(close(s.constraint(top).unwrap().value().unwrap(), 40.0));
    assert!(close(s.constraint(diagonal).unwrap().value().unwrap(), 2000.0_f64.sqrt()));
    assert_eq!(s.dof().unwrap().dof, 0);
    // The width changed: they follow.
    s.set_dimension(w, 30.0).unwrap();
    assert!(close(s.constraint(top).unwrap().value().unwrap(), 30.0));
    assert!(close(s.constraint(diagonal).unwrap().value().unwrap(), 1300.0_f64.sqrt()));
    // A driven dimension cannot be given a value, and only a dimension can be driven.
    let e = s.set_dimension(top, 50.0).unwrap_err().to_string();
    assert!(e.contains("driven dimension") && e.contains("cannot be given a value"), "{e}");
    assert!(close(s.constraint(top).unwrap().value().unwrap(), 30.0));
    assert!(s.add_driven(Constraint::Horizontal { line: l[0] }).is_err());
    assert!(s.set_driven(ConstraintId(999), true).is_err());

    // Driving and driven change places: the height becomes driven, which frees the sketch by
    // one; then the diagonal can drive, and the height reads what follows from it.
    assert!(matches!(s.set_driven(diagonal, false), Err(SketchError::Redundant(_))), "the sketch is held without it");
    assert!(s.is_driven(diagonal));
    s.set_driven(h, true).unwrap();
    assert_eq!(s.dof().unwrap().dof, 1);
    s.set_driven(diagonal, false).unwrap();
    assert_eq!(s.dof().unwrap().dof, 0);
    s.set_dimension(diagonal, 50.0).unwrap();
    assert!(close(s.constraint(h).unwrap().value().unwrap(), 40.0), "30, 40, 50: {:?}", s.constraint(h));
    assert!(s.set_dimension(h, 10.0).is_err());

    // It is saved with the sketch, and goes when its dimension goes.
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains(&format!("\"driven\":[{},{}]", h.0.min(top.0), h.0.max(top.0))), "{json}");
    let back: Sketch = serde_json::from_str(&json).unwrap();
    back.validate().unwrap();
    assert_eq!(back, s);
    s.remove_constraint(top).unwrap();
    assert_eq!(s.driven().collect::<Vec<_>>(), vec![h]);
    s.delete(&[l[1]]);
    assert!(s.driven().all(|id| s.constraint(id).is_some()));
    // A sketch without driven dimensions is written as it always was.
    let (plain, ..) = fixed_rect();
    assert!(!serde_json::to_string(&plain).unwrap().contains("driven"));
    // A driven mark on what is no dimension, or on nothing, is refused when read.
    let geometric = plain.constraints().find(|(_, c)| !c.is_dimensional()).unwrap().0;
    for bad in [geometric.0, 9999] {
        let mut v = serde_json::to_value(&plain).unwrap();
        v["driven"] = serde_json::json!([bad]);
        let broken: Sketch = serde_json::from_value(v).unwrap();
        assert!(broken.validate().unwrap_err().to_string().contains("marked driven"), "{bad}");
    }
}

#[test]
fn reference_geometry_stays_where_it_is_put_and_the_rest_follows_it() {
    // A reference line along y = 10 (an edge of the part, say) and a reference circle; the
    // sketch's own point is dimensioned 4 from the line and its circle made equal to the other.
    let mut s = Sketch::new();
    let edge = s.add_line(v(0.0, 10.0), v(30.0, 10.0)).unwrap();
    let Geometry::Line { start, end } = s.geometry(edge).cloned().unwrap() else { panic!() };
    let hole = s.add_circle(v(50.0, 0.0), 6.0).unwrap();
    let Geometry::Circle { center, .. } = s.geometry(hole).cloned().unwrap() else { panic!() };
    for e in [edge, start, end, hole, center] {
        s.set_reference(e, true).unwrap();
    }
    assert!(s.is_reference(edge) && s.is_held_outside(edge));
    // On its own it has no freedom at all: nothing in it is the sketch's to move.
    assert_eq!(s.dof().unwrap().dof, 0);
    let own = s.add_circle(v(5.0, 3.0), 2.0).unwrap();
    let Geometry::Circle { center: c, .. } = s.geometry(own).cloned().unwrap() else { panic!() };
    assert_eq!(s.dof().unwrap().dof, 3);
    s.add_constraint(Constraint::Distance { a: c, b: edge, value: 4.0 }).unwrap();
    s.add_constraint(Constraint::Equal { a: own, b: hole }).unwrap();
    assert_eq!(s.dof().unwrap().dof, 1, "it may still slide along the edge");
    assert!(close(s.point(c).unwrap().y, 6.0) && close(s.circle(own).unwrap().1, 6.0), "{:?}", s.circle(own));
    assert!(s.line(edge).unwrap().0.near(v(0.0, 10.0), 1e-12) && close(s.circle(hole).unwrap().1, 6.0));
    // Dragging a reference point moves nothing; dragging the sketch's own keeps to the edge.
    s.drag(end, v(60.0, 40.0)).unwrap();
    assert!(s.line(edge).unwrap().1.near(v(30.0, 10.0), 1e-12));
    s.drag(c, v(20.0, -8.0)).unwrap();
    assert!(close(s.point(c).unwrap().x, 20.0) && close(s.point(c).unwrap().y, 6.0), "{:?}", s.point(c));
    // The part changes: the edge is put elsewhere and the radius is another; the rest follows.
    s.set_point(start, v(0.0, 25.0)).unwrap();
    s.set_point(end, v(30.0, 25.0)).unwrap();
    s.set_radius(hole, 9.0).unwrap();
    s.solve().unwrap();
    assert!(close(s.point(c).unwrap().y, 21.0) && close(s.circle(own).unwrap().1, 9.0), "{:?}", s.circle(own));
    // The sketch's own tools leave it alone.
    let cross = s.add_line(v(10.0, 0.0), v(10.0, 40.0)).unwrap();
    let e = s.trim(edge, v(20.0, 25.0)).unwrap_err().to_string();
    assert!(e.contains("projected geometry") && e.contains("break its link"), "{e}");
    s.trim(cross, v(10.0, 30.0)).unwrap();
    // It is written only where it is so, and read back.
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(json.matches("\"reference\":true").count(), 5, "{json}");
    assert!(!json.contains("\"reference\":false") && !json.contains("centerline"));
    let back: Sketch = serde_json::from_str(&json).unwrap();
    back.validate().unwrap();
    assert_eq!(back, s);
    // Its link broken, it is the sketch's own: free to move, and to trim.
    for e in [edge, start, end] {
        s.set_reference(e, false).unwrap();
    }
    assert!(!s.is_held_outside(edge));
    assert!(s.dof().unwrap().dof >= 3);
}

#[test]
fn a_dimension_to_a_centre_line_is_a_diameter() {
    // Half the section of a stepped shaft, drawn above its axis.
    let mut s = Sketch::new();
    let axis = s.add_line(v(0.0, 0.0), v(60.0, 0.0)).unwrap();
    let Geometry::Line { start: o, end: far } = s.geometry(axis).cloned().unwrap() else { panic!() };
    s.add_constraint(Constraint::Fix { point: o }).unwrap();
    s.add_constraint(Constraint::Horizontal { line: axis }).unwrap();
    s.add_constraint(Constraint::Length { line: axis, value: 60.0 }).unwrap();
    s.set_centerline(axis, true).unwrap();
    assert!(s.is_centerline(axis) && !s.entity(axis).unwrap().construction);
    let top = s.add_line(v(0.0, 9.0), v(60.0, 9.0)).unwrap();
    let Geometry::Line { start: a, end: b } = s.geometry(top).cloned().unwrap() else { panic!() };
    s.add_constraint(Constraint::Horizontal { line: top }).unwrap();
    let d = s.add_constraint(Constraint::LinearDiameter { point: a, axis, value: 20.0 }).unwrap();
    assert!(close(s.point(a).unwrap().y, 10.0) && close(s.point(b).unwrap().y, 10.0), "{:?}", s.line(top));
    s.set_dimension(d, 32.0).unwrap();
    assert!(close(s.point(a).unwrap().y, 16.0), "{:?}", s.point(a));
    // It stays on its side of the axis, and reads twice the distance.
    assert!(close(s.measure(s.constraint(d).unwrap()).unwrap(), 32.0));
    assert_eq!(s.constraint(d).unwrap().name(), "linear_diameter");
    assert!(s.constraint(d).unwrap().is_dimensional() && !s.constraint(d).unwrap().is_angular());
    // A second one on the same point is one too many; as a driven dimension it reads the size.
    let again = Constraint::LinearDiameter { point: b, axis, value: 32.0 };
    assert!(matches!(s.add_constraint(again.clone()), Err(SketchError::Redundant(_))));
    let read = s.add_driven(again).unwrap();
    assert!(close(s.constraint(read).unwrap().value().unwrap(), 32.0));
    // Only a line is a centre line, and a diameter is from a point to a line.
    let round = s.add_circle(v(5.0, 5.0), 1.0).unwrap();
    assert!(s.set_centerline(round, true).unwrap_err().to_string().contains("only a line"));
    assert!(s.add_constraint(Constraint::LinearDiameter { point: a, axis: round, value: 4.0 }).is_err());
    assert!(s.add_constraint(Constraint::LinearDiameter { point: a, axis, value: -4.0 }).is_err());
    // The centre line still bounds a profile (a half section closed along its axis).
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(json.matches("\"centerline\":true").count(), 1, "{json}");
    let back: Sketch = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
    let _ = far;
}

#[test]
fn what_the_sketch_draws_itself_is_the_default_profile_before_what_it_shows_of_the_part() {
    // The outline of a 40 x 20 face shown as reference geometry, and a circle drawn on it.
    let mut s = Sketch::new();
    let outline = s.add_rectangle(v(0.0, 0.0), v(40.0, 20.0)).unwrap();
    for l in outline {
        let Geometry::Line { start, end } = s.geometry(l).cloned().unwrap() else { panic!() };
        for e in [l, start, end] {
            s.set_reference(e, true).unwrap();
        }
    }
    // Nothing of its own: the default is the outline, the face itself.
    let all = regions(&s);
    let d = default_regions(&s, &all);
    assert!(d.len() == 1 && close(d[0].area, 800.0), "{d:?}");
    // A circle of its own: that is the default, not the face with a hole in it.
    let hole = s.add_circle(v(10.0, 10.0), 3.0).unwrap();
    let all = regions(&s);
    assert_eq!(all.len(), 2);
    let d = default_regions(&s, &all);
    assert!(d.len() == 1 && d[0].key == vec![hole], "{d:?}");
    // A second circle inside the first: a ring of its own, as in any sketch.
    let inner = s.add_circle(v(10.0, 10.0), 1.0).unwrap();
    let all = regions(&s);
    let d = default_regions(&s, &all);
    assert!(d.len() == 1 && d[0].key == vec![hole] && d[0].holes.len() == 1, "{d:?}");
    s.delete(&[inner]);
    // A circle that crosses the outline is cut in two by it; both pieces are meant.
    let over = s.add_circle(v(40.0, 10.0), 4.0).unwrap();
    let all = regions(&s);
    let d = default_regions(&s, &all);
    let areas: f64 = d.iter().filter(|r| r.key.contains(&over)).map(|r| r.area).sum();
    assert!((areas - PI * 16.0).abs() < 0.3, "the whole circle, both sides of the edge (areas are of outlines drawn in chords): {areas}");
    assert!(d.iter().all(|r| r.key.contains(&hole) || r.key.contains(&over)), "{d:?}");
    // A line of its own from edge to edge closes nothing by itself: everything, then.
    s.delete(&[hole, over]);
    s.add_line(v(30.0, 0.0), v(40.0, 10.0)).unwrap();
    let all = regions(&s);
    assert_eq!(all.len(), 2);
    let d = default_regions(&s, &all);
    assert!(close(d.iter().map(|r| r.area).sum::<f64>(), 800.0), "{d:?}");
}

/// A w x h rectangle with its lower left corner at `at`, its width and height dimensioned.
/// Returns its lines (bottom, right, top, left) and the two dimensions.
fn sized_rect(s: &mut Sketch, at: Vec2, w: f64, h: f64) -> ([EntityId; 4], ConstraintId, ConstraintId) {
    let l = s.add_rectangle(at, at + v(w, h)).unwrap();
    let width = s.add_constraint(Constraint::Length { line: l[0], value: w }).unwrap();
    let height = s.add_constraint(Constraint::Length { line: l[1], value: h }).unwrap();
    (l, width, height)
}

fn count(s: &Sketch, f: impl Fn(&Constraint) -> bool) -> usize {
    s.constraints().filter(|(_, c)| f(c)).count()
}

#[test]
fn geometry_is_moved_turned_and_scaled_and_what_is_joined_to_it_follows() {
    let mut s = Sketch::new();
    let (l, width, height) = sized_rect(&mut s, v(0.0, 0.0), 10.0, 6.0);
    // A line of its own from the rectangle's far corner out to a point held in place.
    let Geometry::Line { end: far, .. } = s.geometry(l[1]).cloned().unwrap() else { panic!() };
    let stay = s.add_line(far, v(30.0, 20.0)).unwrap();
    let Geometry::Line { end: anchor, .. } = s.geometry(stay).cloned().unwrap() else { panic!() };
    s.add_constraint(Constraint::Fix { point: anchor }).unwrap();

    // Shifted by (5, 3): every corner goes with it, its sizes hold, and the joined line
    // stretches to follow while its held end stays.
    let before = corners(&s, &l);
    assert_eq!(s.transform(&l, Motion::Shift(v(5.0, 3.0)), false).unwrap(), l.to_vec());
    for (was, now) in before.iter().zip(corners(&s, &l)) {
        assert!(now.near(*was + v(5.0, 3.0), 1e-9), "{was:?} -> {now:?}");
    }
    assert!(s.point(anchor).unwrap().near(v(30.0, 20.0), 1e-12) && s.point(far).unwrap().near(v(15.0, 9.0), 1e-9));
    assert!(close(s.constraint(width).unwrap().value().unwrap(), 10.0));

    // A quarter turn about its lower left corner: the level sides are upright now and the
    // upright ones level, and it is still 10 by 6.
    let about = corners(&s, &l)[0];
    s.transform(&l, Motion::Turn { about, angle: FRAC_PI_2 }, false).unwrap();
    assert!(matches!(s.line(l[0]), Some((a, b)) if close(a.x, b.x) && close((b.y - a.y).abs(), 10.0)), "{:?}", s.line(l[0]));
    assert_eq!((count(&s, |c| matches!(c, Constraint::Horizontal { .. })), count(&s, |c| matches!(c, Constraint::Vertical { .. }))), (2, 2));
    assert!(count(&s, |c| matches!(c, Constraint::Vertical { line } if *line == l[0])) == 1, "the bottom side is the upright one now");
    // Turned 30 degrees it is neither: those constraints go, the sizes stay.
    s.transform(&l, Motion::Turn { about, angle: PI / 6.0 }, false).unwrap();
    assert_eq!(count(&s, |c| matches!(c, Constraint::Horizontal { .. } | Constraint::Vertical { .. })), 0);
    let (a, b) = s.line(l[0]).unwrap();
    assert!(close(a.dist(b), 10.0) && close((b - a).angle(), FRAC_PI_2 + PI / 6.0), "{a:?} {b:?}");

    // Twice the size about that corner: 20 by 12, and the dimensions say so.
    s.transform(&l, Motion::Scale { about, factor: 2.0 }, false).unwrap();
    let (a, b) = s.line(l[0]).unwrap();
    assert!(close(a.dist(b), 20.0), "{a:?} {b:?}");
    assert!(close(s.constraint(width).unwrap().value().unwrap(), 20.0) && close(s.constraint(height).unwrap().value().unwrap(), 12.0));
    assert!(s.point(anchor).unwrap().near(v(30.0, 20.0), 1e-12));

    // What is held where it is cannot be moved, and says so; the sketch is as it was.
    let held = s.clone();
    let e = s.transform(&[stay], Motion::Shift(v(1.0, 0.0)), false).unwrap_err().to_string();
    assert!(e.contains("holds the selection where it is"), "{e}");
    assert_eq!(s, held);
    // Nor what does not exist, nothing at all, or by what is no number.
    assert!(s.transform(&[], Motion::Shift(v(1.0, 0.0)), false).unwrap_err().to_string().contains("nothing to move"));
    assert!(matches!(s.transform(&[EntityId(999)], Motion::Shift(v(1.0, 0.0)), false), Err(SketchError::NoEntity(_))));
    assert!(s.transform(&l, Motion::Scale { about, factor: 0.0 }, false).is_err());
    assert!(s.transform(&l, Motion::Shift(v(f64::NAN, 0.0)), false).is_err());
    assert_eq!(s, held);

    // A width measured level cannot be turned out of level; by half a turn it can.
    let mut t = Sketch::new();
    let r = t.add_rectangle(v(0.0, 0.0), v(10.0, 6.0)).unwrap();
    let Geometry::Line { start: p0, end: p1 } = t.geometry(r[0]).cloned().unwrap() else { panic!() };
    let across = t.add_constraint(Constraint::HorizontalDistance { a: p0, b: p1, value: 10.0 }).unwrap();
    let e = t.transform(&r, Motion::Turn { about: v(0.0, 0.0), angle: 0.3 }, false).unwrap_err().to_string();
    assert!(e.contains("measured level or upright"), "{e}");
    t.transform(&r, Motion::Turn { about: v(0.0, 0.0), angle: PI }, false).unwrap();
    assert!(close(t.constraint(across).unwrap().value().unwrap(), -10.0) && t.point(p1).unwrap().near(v(-10.0, 0.0), 1e-9));
    // Reference geometry is the part's to move.
    t.set_reference(r[0], true).unwrap();
    assert!(t.transform(&[r[0]], Motion::Shift(v(1.0, 0.0)), false).unwrap_err().to_string().contains("projected geometry"));
}

#[test]
fn a_copy_keeps_its_shape_and_not_its_sizes() {
    let mut s = Sketch::new();
    let (l, ..) = sized_rect(&mut s, v(0.0, 0.0), 10.0, 6.0);
    let hole = s.add_circle(v(5.0, 3.0), 2.0).unwrap();
    s.set_construction(hole, true).unwrap();
    let (entities, constraints) = (s.entity_count(), s.constraint_count());
    let original = s.clone();
    // A copy 20 to the right: four new lines on four new corners, a new circle on a new centre.
    let made = s.transform(&[l[0], l[1], l[2], l[3], hole], Motion::Shift(v(20.0, 0.0)), true).unwrap();
    assert_eq!(made.len(), 5);
    assert_eq!(s.entity_count(), entities + 4 + 4 + 2);
    assert!(made.iter().all(|m| !l.contains(m) && *m != hole));
    let at: Vec<Vec2> = made[..4].iter().map(|m| s.line(*m).unwrap().0).collect();
    for (was, now) in corners(&original, &l).iter().zip(&at) {
        assert!(now.near(*was + v(20.0, 0.0), 1e-12));
    }
    assert!(s.circle(made[4]).unwrap().0.near(v(25.0, 3.0), 1e-12) && s.entity(made[4]).unwrap().construction);
    // It is level and upright like the original (four more such constraints) and its corners
    // are shared; but its width and height are its own to give.
    assert_eq!(s.constraint_count(), constraints + 4);
    assert_eq!(count(&s, |c| c.is_dimensional()), 2);
    assert_eq!(s.dof().unwrap().dof, original.dof().unwrap().dof + 4 + 3);
    // The original is as it was.
    for (id, e) in original.entities() {
        assert_eq!(s.entity(id), Some(e));
    }
    // A copy turned 45 degrees is neither level nor upright: its lines carry no such constraint.
    let turned = s.transform(&l, Motion::Turn { about: v(0.0, 0.0), angle: FRAC_PI_4 }, true).unwrap();
    assert_eq!(count(&s, |c| matches!(c, Constraint::Horizontal { line } | Constraint::Vertical { line } if turned.contains(line))), 0);
    let (a, b) = s.line(turned[0]).unwrap();
    assert!(close((b - a).angle(), FRAC_PI_4) && close(a.dist(b), 10.0));
    // A copy at twice the size.
    let big = s.transform(&[hole], Motion::Scale { about: v(5.0, 3.0), factor: 2.0 }, true).unwrap();
    assert!(close(s.circle(big[0]).unwrap().1, 4.0));
    s.validate().unwrap();
}

#[test]
fn a_curve_is_extended_to_the_next_one_in_its_way() {
    let mut s = Sketch::new();
    let wall = s.add_line(v(20.0, -10.0), v(20.0, 10.0)).unwrap();
    let far = s.add_line(v(40.0, -10.0), v(40.0, 10.0)).unwrap();
    let short = s.add_line(v(0.0, 0.0), v(8.0, 2.0)).unwrap();
    let Geometry::Line { start, end } = s.geometry(short).cloned().unwrap() else { panic!() };
    // Clicked near its right-hand end: that end goes on along the line to the first wall, not
    // the one behind it, and is held there.
    s.extend(short, v(7.0, 2.0)).unwrap();
    assert!(s.point(end).unwrap().near(v(20.0, 5.0), 1e-9), "{:?}", s.point(end));
    assert!(s.point(start).unwrap().near(v(0.0, 0.0), 1e-9));
    assert_eq!(count(&s, |c| matches!(c, Constraint::PointOnCurve { point, curve } if *point == end && *curve == wall)), 1);
    // Nothing lies beyond its other end.
    let e = s.extend(short, v(1.0, 0.0)).unwrap_err().to_string();
    assert!(e.contains("no curve beyond that end"), "{e}");
    // An arc goes on round its circle: a quarter circle about (60, 0) from 3 o'clock to
    // 12 o'clock reaches the line x = 54 at (54, 8).
    let arc = s.add_arc(v(60.0, 0.0), v(70.0, 0.0), v(60.0, 10.0)).unwrap();
    let Geometry::Arc { end: arc_end, .. } = s.geometry(arc).cloned().unwrap() else { panic!() };
    let stop = s.add_line(v(54.0, -20.0), v(54.0, 20.0)).unwrap();
    s.extend(arc, v(60.0, 10.0)).unwrap();
    assert!(s.point(arc_end).unwrap().near(v(54.0, 8.0), 1e-9), "{:?}", s.point(arc_end));
    assert!(close(s.arc(arc).unwrap().radius, 10.0));
    // A line whose length is given cannot be made longer, and says why; a circle has no end;
    // an end joined to another curve is not one to extend.
    let fixed = s.add_line(v(0.0, 30.0), v(5.0, 30.0)).unwrap();
    s.add_constraint(Constraint::Length { line: fixed, value: 5.0 }).unwrap();
    let Geometry::Line { start: f0, .. } = s.geometry(fixed).cloned().unwrap() else { panic!() };
    s.add_constraint(Constraint::Fix { point: f0 }).unwrap();
    let roof = s.add_line(v(15.0, 25.0), v(15.0, 35.0)).unwrap();
    let before = s.clone();
    assert!(s.extend(fixed, v(5.0, 30.0)).unwrap_err().to_string().contains("holds it as it is"));
    assert_eq!(s, before);
    let ring = s.add_circle(v(0.0, -40.0), 3.0).unwrap();
    assert!(s.extend(ring, v(3.0, -40.0)).unwrap_err().to_string().contains("only a line or an arc"));
    let joined = s.add_line(end, v(25.0, 30.0)).unwrap();
    assert!(s.extend(joined, v(20.0, 5.0)).unwrap_err().to_string().contains("joined to another curve"));
    let _ = (far, stop, roof);
}

#[test]
fn curves_are_split_where_another_crosses_them() {
    let mut s = Sketch::new();
    let long = s.add_line(v(0.0, 0.0), v(30.0, 0.0)).unwrap();
    s.add_constraint(Constraint::Horizontal { line: long }).unwrap();
    let length = s.add_constraint(Constraint::Length { line: long, value: 30.0 }).unwrap();
    let cut = s.add_line(v(10.0, -5.0), v(10.0, 5.0)).unwrap();
    let Geometry::Line { start, end } = s.geometry(long).cloned().unwrap() else { panic!() };
    // Split near the crossing: two lines in a row sharing a point held on the crossing line.
    let rest = s.split(long, v(12.0, 0.0)).unwrap();
    let ((a, b), (c, d)) = (s.line(long).unwrap(), s.line(rest).unwrap());
    assert!(a.near(v(0.0, 0.0), 1e-9) && b.near(v(10.0, 0.0), 1e-9) && c.near(v(10.0, 0.0), 1e-9) && d.near(v(30.0, 0.0), 1e-9));
    let Geometry::Line { start: shared, .. } = s.geometry(rest).cloned().unwrap() else { panic!() };
    assert_eq!(s.curves_at(shared).len(), 2);
    assert_eq!(count(&s, |c| matches!(c, Constraint::PointOnCurve { point, curve } if *point == shared && *curve == cut)), 1);
    // Both pieces are level, and the 30 it was long is now from end to end.
    assert_eq!(count(&s, |c| matches!(c, Constraint::Horizontal { .. })), 2);
    assert_eq!(s.constraint(length), Some(&Constraint::Distance { a: start, b: end, value: 30.0 }));
    s.solve().unwrap();
    s.set_dimension(length, 40.0).unwrap();
    assert!(close(s.point(start).unwrap().dist(s.point(end).unwrap()), 40.0));

    // A sloping line's pieces stay in line by being parallel.
    let slope = s.add_line(v(0.0, 20.0), v(20.0, 30.0)).unwrap();
    let cross = s.add_line(v(10.0, 15.0), v(10.0, 40.0)).unwrap();
    let rest = s.split(slope, v(10.0, 25.0)).unwrap();
    assert_eq!(count(&s, |c| matches!(c, Constraint::Parallel { a, b } if *a == slope && *b == rest)), 1);
    assert!(s.line(rest).unwrap().0.near(v(10.0, 25.0), 1e-9));
    // An arc is split into two arcs about one centre.
    let arc = s.add_arc(v(60.0, 0.0), v(70.0, 0.0), v(50.0, 0.0)).unwrap();
    let up = s.add_line(v(60.0, 0.0), v(60.0, 20.0)).unwrap();
    let other = s.split(arc, v(60.0, 10.0)).unwrap();
    let (first, second) = (s.arc(arc).unwrap(), s.arc(other).unwrap());
    assert!(close(first.sweep(), FRAC_PI_2) && close(second.sweep(), FRAC_PI_2) && first.center.near(second.center, 1e-12));
    // A circle becomes two arcs where a line crosses it twice; crossed once it cannot be split.
    let ring = s.add_circle(v(100.0, 0.0), 5.0).unwrap();
    let through = s.add_line(v(90.0, 0.0), v(110.0, 0.0)).unwrap();
    let half = s.split(ring, v(105.0, 0.5)).unwrap();
    assert!(close(s.arc(ring).unwrap().sweep(), PI) && close(s.arc(half).unwrap().sweep(), PI));
    let lone = s.add_circle(v(100.0, 50.0), 5.0).unwrap();
    let tangent = s.add_line(v(90.0, 55.0), v(110.0, 55.0)).unwrap();
    let before = s.clone();
    assert!(s.split(lone, v(100.0, 55.0)).unwrap_err().to_string().contains("only one crossing"));
    // Nothing crosses a line off on its own.
    let free = s.add_line(v(200.0, 0.0), v(210.0, 0.0)).unwrap();
    assert!(s.split(free, v(205.0, 0.0)).unwrap_err().to_string().contains("nothing crosses it"));
    s.delete(&[free]);
    assert_eq!(s.entity_count(), before.entity_count());
    s.validate().unwrap();
    let _ = (cross, up, through, tangent);
}

#[test]
fn sketch_patterns_make_rows_and_rings_of_copies() {
    let mut s = Sketch::new();
    let hole = s.add_circle(v(5.0, 5.0), 2.0).unwrap();
    // Three across, 10 apart, in two rows 8 apart: five copies.
    let made = s.pattern_rect(&[hole], v(1.0, 0.0), 3, 10.0, Some((v(0.0, 1.0), 2, 8.0))).unwrap();
    assert_eq!(made.len(), 5);
    let mut at: Vec<(i64, i64)> = made.iter().map(|c| s.circle(*c).unwrap().0).map(|p| (p.x.round() as i64, p.y.round() as i64)).collect();
    at.sort_unstable();
    assert_eq!(at, [(5, 13), (15, 5), (15, 13), (25, 5), (25, 13)]);
    assert!(made.iter().all(|c| close(s.circle(*c).unwrap().1, 2.0)));
    // The direction is only a direction: its length does not matter; along a slope too.
    let row = s.pattern_rect(&[hole], v(3.0, 4.0), 2, 10.0, None).unwrap();
    assert!(s.circle(row[0]).unwrap().0.near(v(11.0, 13.0), 1e-9));
    // Six round the origin: 60 degrees apart, all 5 sqrt 2 from it. Four over 90 degrees are
    // 30 apart, the last a quarter turn from the first.
    let ring = s.pattern_circular(&[hole], v(0.0, 0.0), 6, TAU).unwrap();
    assert_eq!(ring.len(), 5);
    for (i, c) in ring.iter().enumerate() {
        let p = s.circle(*c).unwrap().0;
        assert!(close(p.len(), 50f64.sqrt()) && close(norm_angle(p.angle() - FRAC_PI_4), (i + 1) as f64 * PI / 3.0), "{i}: {p:?}");
    }
    let fan = s.pattern_circular(&[hole], v(0.0, 0.0), 4, FRAC_PI_2).unwrap();
    assert!(s.circle(fan[2]).unwrap().0.near(v(-5.0, 5.0), 1e-9), "{:?}", s.circle(fan[2]));
    // A rectangle's copies are rectangles: level, upright and joined at their corners.
    let l = s.add_rectangle(v(40.0, 0.0), v(46.0, 4.0)).unwrap();
    let before = s.constraint_count();
    let copies = s.pattern_rect(&l, v(1.0, 0.0), 3, 10.0, None).unwrap();
    assert_eq!((copies.len(), s.constraint_count()), (8, before + 8));
    // What is no pattern is refused, and nothing is made.
    let n = s.entity_count();
    for bad in [
        s.clone().pattern_rect(&[hole], v(1.0, 0.0), 1, 10.0, None),
        s.clone().pattern_rect(&[hole], v(0.0, 0.0), 3, 10.0, None),
        s.clone().pattern_rect(&[hole], v(1.0, 0.0), 3, 0.0, None),
        s.clone().pattern_rect(&[hole], v(1.0, 0.0), 100, 1.0, Some((v(0.0, 1.0), 100, 1.0))),
        s.clone().pattern_circular(&[hole], v(0.0, 0.0), 1, TAU),
        s.clone().pattern_circular(&[hole], v(0.0, 0.0), 4, 0.0),
        s.clone().pattern_circular(&[hole], v(0.0, 0.0), 4, 7.0),
    ] {
        assert!(bad.is_err());
    }
    assert_eq!(s.entity_count(), n);
    s.validate().unwrap();
}

#[test]
fn slots_and_rectangles_about_a_centre_or_at_an_angle_hold_their_shape() {
    // A slot 30 between centres and 8 across, level.
    let mut s = Sketch::new();
    let slot = s.add_slot(v(0.0, 0.0), v(30.0, 0.0), 8.0).unwrap();
    s.solve().unwrap();
    assert_eq!(s.dof().unwrap().dof, 5, "where it is, which way it lies, how long and how wide");
    let all = regions(&s);
    let area = 30.0 * 8.0 + PI * 16.0;
    assert!(all.len() == 1 && (all[0].area - area).abs() < 0.3, "{:?}", all.iter().map(|r| r.area).collect::<Vec<_>>());
    // Held by one end, the distance between its centres and a diameter, with one side level:
    // nothing is left to move. Made 40 long and 12 wide it is still a slot.
    s.add_constraint(Constraint::Fix { point: slot.centers[0] }).unwrap();
    let length = s.add_constraint(Constraint::Distance { a: slot.centers[0], b: slot.centers[1], value: 30.0 }).unwrap();
    let width = s.add_constraint(Constraint::Diameter { curve: slot.arcs[0], value: 8.0 }).unwrap();
    s.add_constraint(Constraint::Horizontal { line: slot.lines[0] }).unwrap();
    assert_eq!(s.dof().unwrap().dof, 0);
    s.set_dimension(length, 40.0).unwrap();
    s.set_dimension(width, 12.0).unwrap();
    let (a, b) = s.line(slot.lines[1]).unwrap();
    assert!(close(a.dist(b), 40.0) && close(a.y, 6.0) && close(b.y, 6.0), "{a:?} {b:?}");
    assert!(close(s.arc(slot.arcs[1]).unwrap().radius, 6.0) && close(s.arc(slot.arcs[1]).unwrap().sweep(), PI));
    assert!(s.add_slot(v(0.0, 0.0), v(0.0, 0.0), 8.0).is_err() && s.add_slot(v(0.0, 0.0), v(9.0, 0.0), 0.0).is_err());
    // A slot at an angle is as good.
    let mut t = Sketch::new();
    let tilted = t.add_slot(v(0.0, 0.0), v(30.0, 40.0), 10.0).unwrap();
    t.solve().unwrap();
    let (a, b) = t.line(tilted.lines[0]).unwrap();
    assert!(close(a.dist(b), 50.0) && close((b - a).angle(), (4.0f64).atan2(3.0)), "{a:?} {b:?}");

    // A rectangle about its centre: sized from the middle, its centre stays the centre.
    let mut r = Sketch::new();
    let (l, centre) = r.add_rectangle_center(v(10.0, 5.0), v(16.0, 9.0)).unwrap();
    r.solve().unwrap();
    assert!(corners(&r, &l)[0].near(v(4.0, 1.0), 1e-9) && r.point(centre).unwrap().near(v(10.0, 5.0), 1e-9));
    r.add_constraint(Constraint::Fix { point: centre }).unwrap();
    let w = r.add_constraint(Constraint::Length { line: l[0], value: 12.0 }).unwrap();
    r.add_constraint(Constraint::Length { line: l[1], value: 8.0 }).unwrap();
    assert_eq!(r.dof().unwrap().dof, 0);
    r.set_dimension(w, 20.0).unwrap();
    let c = corners(&r, &l);
    assert!(c[0].near(v(0.0, 1.0), 1e-7) && c[2].near(v(20.0, 9.0), 1e-7), "it grows both ways from its centre: {c:?}");
    let outline = regions(&r);
    assert!(outline.len() == 1 && close(outline[0].area, 160.0), "the diagonal is construction: one profile");

    // A rectangle at an angle: its first side from (0, 0) to (30, 40), 10 deep to the left.
    let mut q = Sketch::new();
    let l = q.add_rectangle_3(v(0.0, 0.0), v(30.0, 40.0), v(-8.0, 6.0)).unwrap();
    q.solve().unwrap();
    assert_eq!(q.dof().unwrap().dof, 5);
    let c = corners(&q, &l);
    assert!(
        c[0].near(v(0.0, 0.0), 1e-9) && c[1].near(v(30.0, 40.0), 1e-9) && c[2].near(v(22.0, 46.0), 1e-9) && c[3].near(v(-8.0, 6.0), 1e-9),
        "{c:?}"
    );
    // Dragged by a corner it stays square.
    let Geometry::Line { end: far, .. } = q.geometry(l[1]).cloned().unwrap() else { panic!() };
    q.drag(far, v(40.0, 60.0)).unwrap();
    let c = corners(&q, &l);
    assert!(close((c[1] - c[0]).dot(c[2] - c[1]), 0.0) && close((c[3] - c[0]).dot(c[1] - c[0]), 0.0), "{c:?}");
    assert!(q.add_rectangle_3(v(0.0, 0.0), v(0.0, 0.0), v(1.0, 1.0)).is_err() && q.add_rectangle_3(v(0.0, 0.0), v(5.0, 0.0), v(9.0, 0.0)).is_err());
}

#[test]
fn a_tangent_arc_carries_on_from_the_end_of_a_line_or_an_arc() {
    let mut s = Sketch::new();
    let line = s.add_line(v(0.0, 0.0), v(10.0, 0.0)).unwrap();
    let Geometry::Line { start, end } = s.geometry(line).cloned().unwrap() else { panic!() };
    // From the right-hand end, round to a point up and to the right: a quarter circle of
    // radius 5 about (10, 5), turning left.
    let up = s.add_arc_tangent(end, v(15.0, 5.0)).unwrap();
    let a = s.arc(up).unwrap();
    assert!(a.center.near(v(10.0, 5.0), 1e-9) && close(a.radius, 5.0) && close(a.sweep(), FRAC_PI_2), "{a:?}");
    assert!(a.start_point().near(v(10.0, 0.0), 1e-9) && s.curves_at(end).len() == 2);
    // Turning right from the other end (it heads left there) it runs clockwise: about (0, 4).
    let down = s.add_arc_tangent(start, v(-4.0, 4.0)).unwrap();
    let b = s.arc(down).unwrap();
    assert!(b.center.near(v(0.0, 4.0), 1e-9) && close(b.sweep(), FRAC_PI_2) && b.end_point().near(v(0.0, 0.0), 1e-9), "{b:?}");
    // It stays tangent when the line is turned: the line's end dragged, the arc's centre is
    // still square to the line through the shared point.
    s.solve().unwrap();
    s.drag(start, v(0.0, 3.0)).unwrap();
    let ((p, q), a) = (s.line(line).unwrap(), s.arc(up).unwrap());
    assert!(close((q - p).normalized().dot((a.center - q).normalized()), 0.0), "{p:?} {q:?} {a:?}");
    // And on from the arc's own free end, the same way round.
    let Geometry::Arc { end: tip, .. } = s.geometry(up).cloned().unwrap() else { panic!() };
    let on = s.add_arc_tangent(tip, s.point(tip).unwrap() + v(-3.0, 6.0)).unwrap();
    assert!(s.arc(on).is_some());
    // Straight ahead is a line, and a point that ends nothing starts no arc.
    let mut t = Sketch::new();
    let l = t.add_line(v(0.0, 0.0), v(10.0, 0.0)).unwrap();
    let Geometry::Line { end, .. } = t.geometry(l).cloned().unwrap() else { panic!() };
    assert!(t.add_arc_tangent(end, v(20.0, 0.0)).unwrap_err().to_string().contains("that is a line"));
    let lone = t.add_point(v(5.0, 5.0)).unwrap();
    assert!(t.add_arc_tangent(lone, v(9.0, 9.0)).unwrap_err().to_string().contains("end point of a line or an arc"));
}

#[test]
fn text_moves_turns_grows_and_is_copied_with_its_point() {
    let mut s = Sketch::new();
    let t = s.add_text(v(1.0, 2.0), "AB", 5.0, 0.0).unwrap();
    let Some(Geometry::Text { at, .. }) = s.geometry(t).cloned() else { panic!() };
    assert_eq!(s.texts().collect::<Vec<_>>(), [t]);
    let letters = s.text_contours(t).len();
    assert_eq!(letters, 5, "an A and its hole, a B and its two");
    // Moved, its point goes with it; turned and scaled, it turns and grows.
    s.transform(&[t], Motion::Shift(v(3.0, 4.0)), false).unwrap();
    assert!(s.point(at).unwrap().near(v(4.0, 6.0), 1e-9));
    s.transform(&[t], Motion::Turn { about: v(0.0, 6.0), angle: FRAC_PI_2 }, false).unwrap();
    s.transform(&[t], Motion::Scale { about: v(0.0, 6.0), factor: 2.0 }, false).unwrap();
    assert!(s.point(at).unwrap().near(v(0.0, 14.0), 1e-9), "{:?}", s.point(at));
    assert!(matches!(s.geometry(t), Some(Geometry::Text { height, angle, .. }) if close(*height, 10.0) && close(*angle, FRAC_PI_2)));
    // A copy is a text of its own, on a point of its own.
    let copies = s.transform(&[t], Motion::Shift(v(0.0, -20.0)), true).unwrap();
    let copy = copies.iter().copied().find(|c| s.geometry(*c).is_some_and(Geometry::is_text)).unwrap();
    assert!(matches!(s.geometry(copy), Some(Geometry::Text { at: other, height, .. }) if *other != at && close(*height, 10.0)));
    // Mirrored, a text would read backwards: it is left as it is.
    let axis = s.add_line(v(-50.0, 0.0), v(50.0, 0.0)).unwrap();
    let _ = s.mirror(&[t], axis);
    assert_eq!(s.texts().count(), 2);
    // Its words, height and angle change; what is no text is refused.
    s.set_text(t, Some("CD"), None, Some(0.0)).unwrap();
    assert!(matches!(s.geometry(t), Some(Geometry::Text { text, height, angle, .. }) if text == "CD" && close(*height, 10.0) && *angle == 0.0));
    assert!(s.set_text(t, Some(" "), None, None).is_err());
    assert!(s.set_text(t, None, Some(0.0), None).is_err());
    assert!(s.set_text(axis, Some("x"), None, None).is_err());
    assert!(s.add_text(v(0.0, 0.0), "x", f64::NAN, 0.0).is_err());
    assert!(s.add_text(v(0.0, 0.0), &"x".repeat(crate::text::MAX_CHARS + 1), 5.0, 0.0).is_err());
    // Deleted, it is gone; its letters are no regions of curves at any time.
    assert!(regions(&s).is_empty());
    assert!(s.delete(&[t]).contains(&t) && s.geometry(t).is_none() && s.text_contours(t).is_empty());
}
