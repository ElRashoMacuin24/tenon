//! M1 kernel operations: profile faces, extrude, revolve, transform. Volumes are checked against
//! analytic values; history is checked for cap roles and for faces traced to profile curve tags.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use tenon_geom::{Aabb3, Axis, Frame, Vec2, Vec3, tol};
use tenon_kernel::{
    AngleExtent, BoolOp, Curve2, Extent, InputRef, Kernel, KernelError, Loop, Origin, PrimitiveRole, Profile, Region, ShapeHandle, SurfaceKind,
    TaggedCurve2, TopoId, TopoKind, Transform,
};
use tenon_kernel_occt::OcctKernel;

fn approx(a: f64, b: f64) -> bool {
    tol::rel_eq(a, b, tol::MEASURE_REL)
}

fn volume(k: &dyn Kernel, s: ShapeHandle) -> f64 {
    k.mass_properties(s, 1.0).unwrap().volume
}

fn line(tag: u64, a: (f64, f64), b: (f64, f64)) -> TaggedCurve2 {
    TaggedCurve2 { tag, curve: Curve2::Line { start: Vec2::new(a.0, a.1), end: Vec2::new(b.0, b.1) } }
}

/// Rectangle loop with tags `tag`..`tag + 3`: bottom, right, top, left.
fn rect(tag: u64, x0: f64, y0: f64, x1: f64, y1: f64) -> Loop {
    Loop {
        curves: vec![
            line(tag, (x0, y0), (x1, y0)),
            line(tag + 1, (x1, y0), (x1, y1)),
            line(tag + 2, (x1, y1), (x0, y1)),
            line(tag + 3, (x0, y1), (x0, y0)),
        ],
    }
}

fn profile(frame: Frame, regions: Vec<Region>) -> Profile {
    Profile { frame, regions }
}

fn one(outer: Loop) -> Vec<Region> {
    vec![Region { outer, holes: vec![] }]
}

fn normal_of(k: &dyn Kernel, s: ShapeHandle, face: TopoId) -> Vec3 {
    match k.face_info(s.face(face.index)).unwrap().surface {
        SurfaceKind::Plane { normal, .. } => normal,
        other => panic!("expected a plane, got {other:?}"),
    }
}

fn faces_from_tag(op: &tenon_kernel::Op, tag: u64) -> Vec<TopoId> {
    op.history.generated_from(Origin::ProfileCurve { tag }).filter(|t| t.kind == TopoKind::Face).collect()
}

#[test]
fn extrude_rectangle_with_caps_and_tagged_sides() {
    let mut k = OcctKernel::new();
    let p = profile(Frame::WORLD, one(rect(1, 0.0, 0.0, 40.0, 20.0)));
    let op = k.extrude(&p, &Extent::Distance(10.0), None).unwrap();
    assert!(approx(volume(&k, op.shape), 8000.0));
    assert!(k.is_valid(op.shape).unwrap());
    assert_eq!(k.topology(op.shape).unwrap().faces, 6);

    let start = op.history.role(PrimitiveRole::StartCap).unwrap();
    let end = op.history.role(PrimitiveRole::EndCap).unwrap();
    assert!(normal_of(&k, op.shape, start).near(-Vec3::Z, 1e-12));
    assert!(normal_of(&k, op.shape, end).near(Vec3::Z, 1e-12));
    assert!((k.face_info(op.shape.face(end.index)).unwrap().centroid.z - 10.0).abs() < 1e-9);

    // Every side face is traced to the sketch line it was swept from.
    for (tag, n) in [(1, -Vec3::Y), (2, Vec3::X), (3, Vec3::Y), (4, -Vec3::X)] {
        let faces = faces_from_tag(&op, tag);
        assert_eq!(faces.len(), 1, "tag {tag}");
        assert!(normal_of(&k, op.shape, faces[0]).near(n, 1e-12), "tag {tag}");
    }
}

#[test]
fn extents_place_the_solid_correctly() {
    let mut k = OcctKernel::new();
    let p = profile(Frame::WORLD, one(rect(1, 0.0, 0.0, 10.0, 10.0)));
    let bbox = |k: &OcctKernel, s| k.bounding_box(s).unwrap().unwrap();
    let down = k.extrude(&p, &Extent::Distance(-4.0), None).unwrap().shape;
    assert!(bbox(&k, down).near(&Aabb3::new(Vec3::new(0.0, 0.0, -4.0), Vec3::new(10.0, 10.0, 0.0)), 1e-9));
    let sym = k.extrude(&p, &Extent::Symmetric(6.0), None).unwrap().shape;
    assert!(bbox(&k, sym).near(&Aabb3::new(Vec3::new(0.0, 0.0, -3.0), Vec3::new(10.0, 10.0, 3.0)), 1e-9));
    let two = k.extrude(&p, &Extent::TwoSided { forward: 5.0, backward: 2.0 }, None).unwrap().shape;
    assert!(bbox(&k, two).near(&Aabb3::new(Vec3::new(0.0, 0.0, -2.0), Vec3::new(10.0, 10.0, 5.0)), 1e-9));
    assert!(approx(volume(&k, two), 700.0));
}

#[test]
fn profile_on_a_tilted_frame() {
    let mut k = OcctKernel::new();
    // Sketch on the XZ plane (normal -Y, as seen from the front), offset 5 along its normal.
    let frame = Frame::new(Vec3::new(0.0, -5.0, 0.0), -Vec3::Y, Vec3::X).unwrap();
    let op = k.extrude(&profile(frame, one(rect(1, 0.0, 0.0, 10.0, 20.0))), &Extent::Distance(3.0), None).unwrap();
    let b = k.bounding_box(op.shape).unwrap().unwrap();
    assert!(b.near(&Aabb3::new(Vec3::new(0.0, -8.0, 0.0), Vec3::new(10.0, -5.0, 20.0)), 1e-9), "{b:?}");
    assert!(approx(volume(&k, op.shape), 600.0));
}

#[test]
fn region_with_a_circular_hole() {
    let mut k = OcctKernel::new();
    let hole = Loop { curves: vec![TaggedCurve2 { tag: 10, curve: Curve2::Circle { center: Vec2::new(20.0, 20.0), radius: 5.0 } }] };
    let p = profile(Frame::WORLD, vec![Region { outer: rect(1, 0.0, 0.0, 40.0, 40.0), holes: vec![hole] }]);
    let op = k.extrude(&p, &Extent::Distance(10.0), None).unwrap();
    assert!(approx(volume(&k, op.shape), 16_000.0 - 250.0 * PI));
    assert!(k.is_valid(op.shape).unwrap());
    let wall = faces_from_tag(&op, 10);
    assert_eq!(wall.len(), 1);
    assert!(
        matches!(k.face_info(op.shape.face(wall[0].index)).unwrap().surface, SurfaceKind::Cylinder { radius, .. } if (radius - 5.0).abs() < 1e-9)
    );
}

#[test]
fn slot_profile_of_lines_and_arcs() {
    let mut k = OcctKernel::new();
    let arc = |tag, cx: f64, a0: f64, a1: f64| TaggedCurve2 {
        tag,
        curve: Curve2::Arc { center: Vec2::new(cx, 0.0), radius: 5.0, start_angle: a0, end_angle: a1 },
    };
    let slot = Loop {
        curves: vec![
            line(1, (0.0, -5.0), (20.0, -5.0)),
            arc(2, 20.0, -FRAC_PI_2, FRAC_PI_2),
            line(3, (20.0, 5.0), (0.0, 5.0)),
            arc(4, 0.0, FRAC_PI_2, 1.5 * PI),
        ],
    };
    let op = k.extrude(&profile(Frame::WORLD, one(slot)), &Extent::Distance(2.0), None).unwrap();
    assert!(approx(volume(&k, op.shape), (200.0 + 25.0 * PI) * 2.0));
    for tag in 1..=4 {
        assert_eq!(faces_from_tag(&op, tag).len(), 1, "tag {tag}");
    }
}

#[test]
fn spline_closed_by_a_line() {
    let mut k = OcctKernel::new();
    let spline = TaggedCurve2 {
        tag: 1,
        curve: Curve2::BSpline { poles: vec![Vec2::new(0.0, 0.0), Vec2::new(3.0, 8.0), Vec2::new(7.0, 8.0), Vec2::new(10.0, 0.0)], degree: 3 },
    };
    let lp = Loop { curves: vec![spline, line(2, (10.0, 0.0), (0.0, 0.0))] };
    let face = k.make_face(&profile(Frame::WORLD, one(lp.clone()))).unwrap();
    // Four poles and degree 3 make a single cubic Bezier: y(t) = 24 t (1 - t) and
    // x'(t) = 9 + 6t - 6t^2. Area = integral of y x' dt over [0, 1] = 216/6 + 144/30 = 40.8.
    let area = k.mass_properties(face.shape, 1.0).unwrap().area;
    assert!((area - 40.8).abs() < 1e-6, "{area}");
    assert_eq!(face.history.generated_from(Origin::ProfileCurve { tag: 1 }).count(), 1, "the spline edge is tagged");
    let op = k.extrude(&profile(Frame::WORLD, one(lp)), &Extent::Distance(1.0), None).unwrap();
    assert!((volume(&k, op.shape) - 40.8).abs() < 1e-6);
}

#[test]
fn two_regions_extrude_together() {
    let mut k = OcctKernel::new();
    let regions =
        vec![Region { outer: rect(1, 0.0, 0.0, 10.0, 10.0), holes: vec![] }, Region { outer: rect(5, 20.0, 0.0, 30.0, 10.0), holes: vec![] }];
    let op = k.extrude(&profile(Frame::WORLD, regions), &Extent::Distance(1.0), None).unwrap();
    assert!(approx(volume(&k, op.shape), 200.0));
    assert_eq!(k.topology(op.shape).unwrap().solids, 2);
    assert_eq!(op.history.roles_of(PrimitiveRole::StartCap).count(), 2);
}

#[test]
fn revolve_full_partial_symmetric() {
    let mut k = OcctKernel::new();
    // Rectangle x 10..20, y 0..10 revolved about the world Y axis: a ring.
    let p = profile(Frame::WORLD, one(rect(1, 10.0, 0.0, 20.0, 10.0)));
    let axis = Axis::new(Vec3::ZERO, Vec3::Y).unwrap();
    let ring = PI * (20.0 * 20.0 - 10.0 * 10.0) * 10.0;
    let full = k.revolve(&p, &axis, &AngleExtent::Full).unwrap();
    assert!(approx(volume(&k, full.shape), ring), "{}", volume(&k, full.shape));
    assert!(full.history.role(PrimitiveRole::StartCap).is_none(), "a full revolution has no caps");
    assert!(k.is_valid(full.shape).unwrap());

    let quarter = k.revolve(&p, &axis, &AngleExtent::Angle(FRAC_PI_2)).unwrap();
    assert!(approx(volume(&k, quarter.shape), ring / 4.0));
    assert!(quarter.history.role(PrimitiveRole::StartCap).is_some() && quarter.history.role(PrimitiveRole::EndCap).is_some());
    // The outer wall comes from the rectangle's right edge (tag 2, x = 20).
    let outer = faces_from_tag(&quarter, 2);
    assert_eq!(outer.len(), 1);
    assert!(
        matches!(k.face_info(quarter.shape.face(outer[0].index)).unwrap().surface, SurfaceKind::Cylinder { radius, .. } if (radius - 20.0).abs() < 1e-9)
    );

    // Symmetric: half on each side of the sketch plane.
    let sym = k.revolve(&p, &axis, &AngleExtent::Symmetric(FRAC_PI_2)).unwrap();
    assert!(approx(volume(&k, sym.shape), ring / 4.0));
    let b = k.bounding_box(sym.shape).unwrap().unwrap();
    assert!((b.max.z + b.min.z).abs() < 1e-6, "symmetric about the sketch plane: {b:?}");
}

#[test]
fn revolve_through_the_axis_is_rejected() {
    let mut k = OcctKernel::new();
    let p = profile(Frame::WORLD, one(rect(1, -5.0, 0.0, 5.0, 10.0)));
    let r = k.revolve(&p, &Axis::new(Vec3::ZERO, Vec3::Y).unwrap(), &AngleExtent::Full);
    assert!(r.is_err(), "a profile crossing the axis must not produce a solid");
}

#[test]
fn transforms_keep_volume_and_map_faces() {
    let mut k = OcctKernel::new();
    let b = k.make_box(&Frame::WORLD, Vec3::new(1.0, 2.0, 3.0)).unwrap();
    let top = b.history.role(PrimitiveRole::BoxZMax).unwrap();
    let moved = k.transform(b.shape, &Transform::Translate(Vec3::new(10.0, 0.0, 0.0))).unwrap();
    let bb = k.bounding_box(moved.shape).unwrap().unwrap();
    assert!(bb.near(&Aabb3::new(Vec3::new(10.0, 0.0, 0.0), Vec3::new(11.0, 2.0, 3.0)), 1e-9));
    let img = moved.history.image_of(InputRef { input: 0, id: top }).unwrap();
    assert_eq!(img.len(), 1);
    assert!((k.face_info(moved.shape.face(img[0].index)).unwrap().centroid.z - 3.0).abs() < 1e-9);

    let rot = k.transform(b.shape, &Transform::Rotate { axis: Axis::Z, angle: FRAC_PI_2 }).unwrap();
    assert!(approx(volume(&k, rot.shape), 6.0));
    let mir = k.transform(b.shape, &Transform::Mirror { plane: Frame::new(Vec3::ZERO, Vec3::X, Vec3::Y).unwrap() }).unwrap();
    assert!(approx(volume(&k, mir.shape), 6.0));
    assert!(k.bounding_box(mir.shape).unwrap().unwrap().max.x <= 1e-9);
    let big = k.transform(b.shape, &Transform::Scale { center: Vec3::ZERO, factor: 2.0 }).unwrap();
    assert!(approx(volume(&k, big.shape), 48.0));
}

#[test]
fn extrude_then_cut_keeps_tags_through_the_boolean() {
    let mut k = OcctKernel::new();
    let plate = k.extrude(&profile(Frame::WORLD, one(rect(1, 0.0, 0.0, 40.0, 20.0))), &Extent::Distance(10.0), None).unwrap();
    let hole = Loop { curves: vec![TaggedCurve2 { tag: 9, curve: Curve2::Circle { center: Vec2::new(10.0, 10.0), radius: 3.0 } }] };
    let tool = k.extrude(&profile(Frame::WORLD, one(hole)), &Extent::Distance(10.0), None).unwrap();
    let cut = k.boolean(BoolOp::Cut, plate.shape, &[tool.shape]).unwrap();
    assert!(approx(volume(&k, cut.shape), 8000.0 - 90.0 * PI));
    // The plate's front face (tag 1) survives the cut as one face.
    let front = faces_from_tag(&plate, 1)[0];
    assert_eq!(cut.history.image_of(InputRef { input: 0, id: front }).unwrap().len(), 1);
}

#[test]
fn bad_profiles_are_rejected() {
    let mut k = OcctKernel::new();
    let ok = profile(Frame::WORLD, one(rect(1, 0.0, 0.0, 1.0, 1.0)));
    assert!(matches!(k.extrude(&profile(Frame::WORLD, vec![]), &Extent::Distance(1.0), None), Err(KernelError::InvalidInput(_))));
    let open = Loop { curves: vec![line(1, (0.0, 0.0), (1.0, 0.0)), line(2, (1.0, 0.0), (1.0, 1.0))] };
    assert!(k.extrude(&profile(Frame::WORLD, one(open)), &Extent::Distance(1.0), None).is_err());
    let degenerate = Loop { curves: vec![line(1, (0.0, 0.0), (0.0, 0.0))] };
    assert!(k.make_face(&profile(Frame::WORLD, one(degenerate))).is_err());
    let nan = Loop { curves: vec![line(1, (f64::NAN, 0.0), (1.0, 0.0))] };
    assert!(matches!(k.make_face(&profile(Frame::WORLD, one(nan))), Err(KernelError::InvalidInput(_))));
    let spline = Loop { curves: vec![TaggedCurve2 { tag: 1, curve: Curve2::BSpline { poles: vec![Vec2::ZERO, Vec2::new(1.0, 0.0)], degree: 3 } }] };
    assert!(matches!(k.make_face(&profile(Frame::WORLD, one(spline))), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.extrude(&ok, &Extent::Distance(0.0), None), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.extrude(&ok, &Extent::Distance(f64::INFINITY), None), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.extrude(&ok, &Extent::ThroughAll { reverse: false }, None), Err(KernelError::Unsupported(_))));
    assert!(matches!(k.extrude(&ok, &Extent::Distance(1.0), Some(0.1)), Err(KernelError::Unsupported(_))));
    assert!(matches!(k.revolve(&ok, &Axis::Z, &AngleExtent::Angle(0.0)), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.revolve(&ok, &Axis::Z, &AngleExtent::Angle(TAU * 2.0)), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.transform(ShapeHandle::from_parts(42, 0), &Transform::Translate(Vec3::X)), Err(KernelError::InvalidHandle)));
}
