//! What Emboss asks of the kernel (DEC-133): a face thickened along its normal, a flat outline
//! laid round a cone with its lengths kept, and the way a face looks. Figures worked out by hand.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_4, PI, TAU};

use tenon_geom::{Axis, Frame, Vec2, Vec3};
use tenon_kernel::{Curve2, Kernel, Loop, Op, PrimitiveRole, Profile, Region, SurfaceKind, TaggedCurve2};
use tenon_kernel_occt::OcctKernel;

fn line(tag: u64, a: (f64, f64), b: (f64, f64)) -> TaggedCurve2 {
    TaggedCurve2 { tag, curve: Curve2::Line { start: Vec2::new(a.0, a.1), end: Vec2::new(b.0, b.1) } }
}

fn rect(frame: Frame, (x0, y0): (f64, f64), (x1, y1): (f64, f64)) -> Profile {
    let curves = vec![line(1, (x0, y0), (x1, y0)), line(2, (x1, y0), (x1, y1)), line(3, (x1, y1), (x0, y1)), line(4, (x0, y1), (x0, y0))];
    Profile { frame, regions: vec![Region { outer: Loop { curves }, holes: vec![] }] }
}

/// The face of `s` on a surface `pick` says yes to.
fn face_where(k: &OcctKernel, s: tenon_kernel::ShapeHandle, pick: impl Fn(&SurfaceKind) -> bool) -> u32 {
    let n = k.topology(s).unwrap().faces;
    (0..n).find(|f| pick(&k.face_info(s.face(*f)).unwrap().surface)).unwrap()
}

#[test]
fn a_flat_face_thickens_to_a_prism_either_way() {
    let mut k = OcctKernel::new();
    let b = k.make_box(&Frame::WORLD, Vec3::new(40.0, 20.0, 10.0)).unwrap().shape;
    let top = face_where(&k, b, |s| matches!(s, SurfaceKind::Plane { normal, .. } if normal.z > 0.5));
    assert!((k.face_normal(b.face(top)).unwrap() - Vec3::Z).len() < 1e-12);

    let up = k.thicken(b.face(top), 3.0).unwrap();
    assert!(k.is_valid(up.shape).unwrap());
    assert!((k.mass_properties(up.shape, 1.0).unwrap().volume - 2400.0).abs() < 1e-6, "40 x 20 x 3");
    let bb = k.bounding_box(up.shape).unwrap().unwrap();
    assert!((bb.min.z - 10.0).abs() < 1e-6 && (bb.max.z - 13.0).abs() < 1e-6, "{bb:?}");
    // The far side is the end cap: a plane 3 up.
    let caps: Vec<_> = up.history.roles_of(PrimitiveRole::EndCap).collect();
    assert_eq!(caps.len(), 1);
    assert!((k.face_info(up.shape.face(caps[0].index)).unwrap().centroid.z - 13.0).abs() < 1e-6);

    let down = k.thicken(b.face(top), -4.0).unwrap();
    assert!((k.mass_properties(down.shape, 1.0).unwrap().volume - 3200.0).abs() < 1e-6, "40 x 20 x 4");
    let bb = k.bounding_box(down.shape).unwrap().unwrap();
    assert!((bb.min.z - 6.0).abs() < 1e-6 && (bb.max.z - 10.0).abs() < 1e-6, "{bb:?}");
}

#[test]
fn a_round_face_thickens_to_a_ring() {
    let mut k = OcctKernel::new();
    let c = k.make_cylinder(&Axis::Z, 10.0, 30.0, ).unwrap().shape;
    let side = face_where(&k, c, |s| matches!(s, SurfaceKind::Cylinder { .. }));
    let n = k.face_normal(c.face(side)).unwrap();
    assert!(n.z.abs() < 1e-9 && (n.len() - 1.0).abs() < 1e-9, "it looks out from the axis: {n:?}");

    let out = k.thicken(c.face(side), 2.0).unwrap();
    assert!(k.is_valid(out.shape).unwrap());
    let v = k.mass_properties(out.shape, 1.0).unwrap().volume;
    assert!((v - PI * (144.0 - 100.0) * 30.0).abs() < 1e-3, "a ring from 10 to 12, 30 long: {v}");
    let cap: Vec<_> = out.history.roles_of(PrimitiveRole::EndCap).collect();
    assert_eq!(cap.len(), 1);
    match k.face_info(out.shape.face(cap[0].index)).unwrap().surface {
        SurfaceKind::Cylinder { radius, .. } => assert!((radius - 12.0).abs() < 1e-9),
        other => panic!("{other:?}"),
    }

    let inward = k.thicken(c.face(side), -3.0).unwrap();
    let v = k.mass_properties(inward.shape, 1.0).unwrap().volume;
    assert!((v - PI * (100.0 - 49.0) * 30.0).abs() < 1e-3, "a ring from 7 to 10: {v}");
}

#[test]
fn a_patch_of_a_cylinder_cut_by_a_box_thickens_along_the_radius() {
    let mut k = OcctKernel::new();
    let c = k.make_cylinder(&Axis::Z, 10.0, 30.0).unwrap().shape;
    // What of the cylinder lies within a box from y = 0 out: half of it.
    let half = k.make_box(&Frame::new(Vec3::new(-20.0, 0.0, -1.0), Vec3::Z, Vec3::X).unwrap(), Vec3::new(40.0, 20.0, 32.0)).unwrap().shape;
    let piece = k.boolean(tenon_kernel::BoolOp::Intersect, c, &[half]).unwrap().shape;
    let side = face_where(&k, piece, |s| matches!(s, SurfaceKind::Cylinder { .. }));
    let out = k.thicken(piece.face(side), 2.0).unwrap();
    assert!(k.is_valid(out.shape).unwrap());
    let v = k.mass_properties(out.shape, 1.0).unwrap().volume;
    assert!((v - 0.5 * PI * 44.0 * 30.0).abs() < 1e-3, "half a ring from 10 to 12: {v}");
    assert!(k.bounding_box(out.shape).unwrap().unwrap().min.y.abs() < 1e-6, "its sides stay on the plane through the axis");
}

/// The cone with its apex at the origin widening up Z, half angle 45 degrees.
fn cone_frame() -> Frame {
    Frame::new(Vec3::ZERO, Vec3::Z, Vec3::X).unwrap()
}

fn rolled(k: &mut OcctKernel, p: &Profile, inner: f64, outer: f64) -> Op {
    k.roll_cone(p, FRAC_PI_4, inner, outer).unwrap()
}

#[test]
fn an_outline_laid_round_a_cone_keeps_its_area_and_its_lengths() {
    let mut k = OcctKernel::new();
    // 8 across, from 30 to 40 from the apex; straight out from the cone by up to 1.
    let p = rect(cone_frame(), (-4.0, 30.0), (4.0, 40.0));
    let op = rolled(&mut k, &p, 0.0, 1.0);
    assert!(k.is_valid(op.shape).unwrap());
    let topo = k.topology(op.shape).unwrap();
    assert_eq!((topo.solids, topo.faces), (1, 6));

    // The face on the cone itself is the outline, its area kept: 8 x 10, to the mesh of the
    // spline through its edges.
    let starts: Vec<_> = op.history.roles_of(PrimitiveRole::StartCap).collect();
    assert_eq!(starts.len(), 1);
    let on_cone = k.face_info(op.shape.face(starts[0].index)).unwrap();
    assert!((on_cone.area - 80.0).abs() < 1e-3, "{}", on_cone.area);
    assert!(matches!(on_cone.surface, SurfaceKind::Cone { .. }), "{:?}", on_cone.surface);

    // What it holds, by hand. An element dA of the cone at distance rho from the apex, carried
    // t along the normal, is dA (1 + t cot(a) / rho) wide (the round grows, the length along the
    // cone does not), so the solid from 0 to d is the integral of d + d^2 cot(a) / (2 rho) dA.
    let (mut sum, n) = (0.0, 400);
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (-4.0 + 8.0 * (f64::from(i) + 0.5) / f64::from(n), 30.0 + 10.0 * (f64::from(j) + 0.5) / f64::from(n));
            sum += (1.0 + 1.0 / (2.0 * x.hypot(y))) * (8.0 / f64::from(n)) * (10.0 / f64::from(n));
        }
    }
    let v = k.mass_properties(op.shape, 1.0).unwrap().volume;
    assert!((v - sum).abs() < 1e-3, "{v} against {sum}");

    // The end of the outline nearest the apex: a point straight out from the apex at x = 0 is on
    // the cone's generator along X, 45 degrees up.
    let bb = k.bounding_box(op.shape).unwrap().unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    assert!((bb.min.z - 30.0 * s * (30.0f64 / 30.0)).abs() < 1.0, "{bb:?}");
    assert!(bb.max.x > 40.0 * s - 1e-6 && bb.max.x < 41.0 * s + 1.0, "{bb:?}");
}

#[test]
fn a_line_along_the_generator_stays_on_it_and_one_across_goes_round() {
    let mut k = OcctKernel::new();
    // x = 0 along the generator over X; the far end at y = 40 is at z = 40 sin 45 degrees, r the same.
    let p = rect(cone_frame(), (0.0, 30.0), (TAU * 0.2 * (45.0f64).to_radians().sin() * 40.0, 40.0));
    let op = rolled(&mut k, &p, 0.0, 0.5);
    assert!(k.is_valid(op.shape).unwrap());
    let topo = k.topology(op.shape).unwrap();
    // The curve from (0, 30) to (0, 40) (tag 4) made a side that holds the generator: its points
    // all have y = 0, x = z there.
    let side = op.history.generated_from(tenon_kernel::Origin::ProfileCurve { tag: 4 }).find(|t| t.kind == tenon_kernel::TopoKind::Face).unwrap();
    let info = k.face_info(op.shape.face(side.index)).unwrap();
    assert!(info.centroid.y.abs() < 1e-3, "{:?}", info.centroid);
    assert_eq!(topo.faces, 6);
}

#[test]
fn what_cannot_be_laid_round_a_cone_is_refused() {
    let mut k = OcctKernel::new();
    let on_apex = rect(cone_frame(), (-4.0, 0.0), (4.0, 10.0));
    assert!(k.roll_cone(&on_apex, FRAC_PI_4, 0.0, 1.0).is_err(), "it reaches the apex");
    let too_far = rect(cone_frame(), (-200.0, 10.0), (200.0, 40.0));
    assert!(k.roll_cone(&too_far, 0.2, 0.0, 1.0).is_err(), "it goes more than once round");
    let good = rect(cone_frame(), (-4.0, 30.0), (4.0, 40.0));
    assert!(k.roll_cone(&good, 0.0, 0.0, 1.0).is_err(), "no half angle");
    assert!(k.roll_cone(&good, FRAC_PI_4, 1.0, 1.0).is_err(), "no thickness");
    assert!(k.roll_cone(&good, f64::NAN, 0.0, 1.0).is_err());
    assert_eq!(k.live_shapes(), 0, "nothing is left behind");
    // A circle is two arcs, a whole ellipse two ellipse arcs: the caller splits them.
    let whole = TaggedCurve2 { tag: 1, curve: Curve2::Circle { center: Vec2::new(0.0, 30.0), radius: 3.0 } };
    let p = Profile { frame: cone_frame(), regions: vec![Region { outer: Loop { curves: vec![whole] }, holes: vec![] }] };
    assert!(k.roll_cone(&p, FRAC_PI_4, 0.0, 1.0).is_err());
    let half = |tag: u64, start_angle: f64, end_angle: f64| TaggedCurve2 { tag, curve: Curve2::Arc { center: Vec2::new(0.0, 30.0), radius: 3.0, start_angle, end_angle } };
    let p = Profile { frame: cone_frame(), regions: vec![Region { outer: Loop { curves: vec![half(1, 0.0, PI), half(2, PI, TAU)] }, holes: vec![] }] };
    let op = k.roll_cone(&p, FRAC_PI_4, 0.0, 1.0).unwrap();
    assert!(k.is_valid(op.shape).unwrap());
    // What the disc holds: as a rectangle's, the integral of 1 + 1 / (2 rho) over the disc. (Not
    // the area of the face: the sum of a face's area is plain, and off where its rim curves.)
    let (mut sum, n) = (0.0, 800);
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (-3.0 + 6.0 * (f64::from(i) + 0.5) / f64::from(n), -3.0 + 6.0 * (f64::from(j) + 0.5) / f64::from(n));
            if x.hypot(y) <= 3.0 {
                sum += (1.0 + 1.0 / (2.0 * x.hypot(y + 30.0))) * (6.0 / f64::from(n)).powi(2);
            }
        }
    }
    let v = k.mass_properties(op.shape, 1.0).unwrap().volume;
    assert!((v - sum).abs() < 1e-3 * sum, "{v} against {sum}");
}
