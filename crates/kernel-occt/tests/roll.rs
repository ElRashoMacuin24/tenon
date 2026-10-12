//! A flat outline rolled round an axis: what a bend of a sheet metal part is when its end is
//! not cut square (a corner relief that reaches into the bend, a cut across it).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};

use tenon_geom::{Frame, Vec2, Vec3};
use tenon_kernel::{
    BoolOp, Curve2, CurveKind, Kernel, KernelError, Loop, Op, Origin, PrimitiveRole, Profile, Region, SurfaceKind, TaggedCurve2, TopoKind,
};
use tenon_kernel_occt::OcctKernel;

/// Inside radius 3, 2 thick, the neutral surface 0.44 of the way through.
const R: f64 = 3.0;
const T: f64 = 2.0;
const RHO: f64 = R + 0.44 * T;

fn line(tag: u64, a: (f64, f64), b: (f64, f64)) -> TaggedCurve2 {
    TaggedCurve2 { tag, curve: Curve2::Line { start: Vec2::new(a.0, a.1), end: Vec2::new(b.0, b.1) } }
}

/// The axis through (1, 2, 3) along Y; the outline starts straight below it and turns towards -X.
fn frame() -> Frame {
    Frame::new(Vec3::new(1.0, 2.0, 3.0), Vec3::Y, -Vec3::Z).unwrap()
}

fn rolled(k: &mut OcctKernel, curves: Vec<TaggedCurve2>) -> Op {
    let profile = Profile { frame: frame(), regions: vec![Region { outer: Loop { curves }, holes: vec![] }] };
    k.roll(&profile, RHO, R, R + T).unwrap()
}

/// The faces a tag made.
fn faces_of(op: &Op, tag: u64) -> Vec<u32> {
    op.history
        .generated
        .iter()
        .filter(|g| g.origin == Origin::ProfileCurve { tag })
        .flat_map(|g| g.result.iter().filter(|t| t.kind == TopoKind::Face).map(|t| t.index))
        .collect()
}

/// Where a point of the solid is in the flat outline: how far round the axis (as length of the
/// neutral surface), how far along it, and its radius.
fn flat(p: Vec3) -> (f64, f64, f64) {
    let d = p - Vec3::new(1.0, 2.0, 3.0);
    // Angle 0 is -Z; it turns towards Y x (-Z) = -X.
    let angle = (-d.x).atan2(-d.z);
    (angle * RHO, d.y, (d.x * d.x + d.z * d.z).sqrt())
}

/// The volume of sheet a flat area becomes: every strip of it is an arc of the ring.
fn volume_of(area: f64) -> f64 {
    area * ((R + T) * (R + T) - R * R) / (2.0 * RHO)
}

#[test]
fn a_rectangle_rolls_into_a_bend_with_square_ends() {
    let mut k = OcctKernel::new();
    // A quarter turn, 40 long.
    let w = FRAC_PI_2 * RHO;
    let op = rolled(
        &mut k,
        vec![line(1, (0.0, 0.0), (w, 0.0)), line(2, (w, 0.0), (w, 40.0)), line(3, (w, 40.0), (0.0, 40.0)), line(4, (0.0, 40.0), (0.0, 0.0))],
    );
    let s = op.shape;
    assert!(k.is_valid(s).unwrap());
    let topo = k.topology(s).unwrap();
    assert_eq!((topo.solids, topo.faces), (1, 6));
    let volume = k.mass_properties(s, 1.0).unwrap().volume;
    assert!((volume - 160.0 * PI).abs() < 1e-6, "a quarter of a ring 3 to 5, 40 long: {volume}");
    let b = k.bounding_box(s).unwrap().unwrap();
    assert!(b.min.near(Vec3::new(-4.0, 2.0, -2.0), 1e-6) && b.max.near(Vec3::new(1.0, 42.0, 3.0), 1e-6), "{b:?}");

    // The two round faces are the caps, the inner one first; the four sides are flat and are
    // told apart by the tags of the outline.
    let radius = |f: u32| match k.face_info(s.face(f)).unwrap().surface {
        SurfaceKind::Cylinder { radius, .. } => Some(radius),
        _ => None,
    };
    let caps = |role| op.history.roles_of(role).map(|t| radius(t.index)).collect::<Vec<_>>();
    assert_eq!(caps(PrimitiveRole::StartCap), vec![Some(3.0)]);
    assert_eq!(caps(PrimitiveRole::EndCap), vec![Some(5.0)]);
    let normal = |tag: u64| -> Vec3 {
        let faces = faces_of(&op, tag);
        assert_eq!(faces.len(), 1, "tag {tag}");
        match k.face_info(s.face(faces[0])).unwrap().surface {
            SurfaceKind::Plane { normal, .. } => normal,
            other => panic!("tag {tag}: {other:?}"),
        }
    };
    // The ends are square to the axis; the side the outline starts on holds the axis and the
    // direction it starts in (straight down), the side it ends on is a quarter turn on.
    assert!(normal(1).cross(Vec3::Y).len() < 1e-9 && normal(3).cross(Vec3::Y).len() < 1e-9);
    assert!(normal(4).cross(Vec3::X).len() < 1e-9, "{:?}", normal(4));
    assert!(normal(2).cross(Vec3::Z).len() < 1e-9, "{:?}", normal(2));
    // Every edge is a line or an arc of a circle: nothing is approximated.
    for e in 0..topo.edges {
        assert!(matches!(k.edge_info(s.edge(e)).unwrap().curve, CurveKind::Line { .. } | CurveKind::Circle { .. }), "edge {e}");
    }
    k.release(s);
}

#[test]
fn a_round_bite_out_of_the_end_is_the_same_circle_when_the_bend_is_laid_flat() {
    let mut k = OcctKernel::new();
    // The quarter-turn bend again, with a disc of radius 4 centred just below its end, in the
    // middle of its width, taken out: the bend now ends in an arc.
    let w = FRAC_PI_2 * RHO;
    let (centre, radius) = (Vec2::new(w / 2.0, -1.0), 4.0);
    let rise = (radius * radius - w * w / 4.0).sqrt();
    let y = centre.y + rise;
    let reach = rise.atan2(w / 2.0);
    let outline = vec![
        TaggedCurve2 { tag: 9, curve: Curve2::Arc { center: centre, radius, start_angle: reach, end_angle: PI - reach } },
        line(4, (0.0, y), (0.0, 40.0)),
        line(3, (0.0, 40.0), (w, 40.0)),
        line(2, (w, 40.0), (w, y)),
    ];
    let op = rolled(&mut k, outline);
    let s = op.shape;
    assert!(k.is_valid(s).unwrap());
    let topo = k.topology(s).unwrap();
    assert_eq!((topo.solids, topo.faces), (1, 6));
    // What the disc takes of the rectangle: the part of it above the end, between the sides.
    let h = w / 2.0;
    let bite = h * rise + radius * radius * (h / radius).asin() - w * 1.0;
    let volume = k.mass_properties(s, 1.0).unwrap().volume;
    let want = volume_of(w * 40.0 - bite);
    assert!((volume - want).abs() < 1e-5 * want, "{volume} vs {want}");

    // The cut face is neither flat nor round, and it is the one the arc made.
    let cut = faces_of(&op, 9);
    assert_eq!(cut.len(), 1);
    assert!(!matches!(k.face_info(s.face(cut[0])).unwrap().surface, SurfaceKind::Plane { .. } | SurfaceKind::Cylinder { .. }));
    // Its edges on the two round faces are the circle of the flat outline, at every radius:
    // the cut is square to the sheet all the way through.
    let mut curved = 0;
    for e in 0..topo.edges {
        let info = k.edge_info(s.edge(e)).unwrap();
        // (The cut's edges across the thickness are straight, whatever kind of curve they are
        // kept as.)
        if matches!(info.curve, CurveKind::Line { .. } | CurveKind::Circle { .. }) || info.length - info.start.dist(info.end) < 1e-9 {
            continue;
        }
        curved += 1;
        let pts = k.edge_points(s.edge(e), 0.002).unwrap();
        assert!(pts.len() > 8, "edge {e}: {} points, {info:?}", pts.len());
        for p in pts {
            let (x, along, r) = flat(p);
            assert!((r - R).abs() < 1e-6 || (r - R - T).abs() < 1e-6, "radius {r}");
            let off = (Vec2::new(x, along) - centre).len() - radius;
            assert!(off.abs() < 2e-6, "{off} at ({x}, {along})");
        }
    }
    assert_eq!(curved, 2, "one on the inside of the bend, one on the outside");

    // It joins a wall that stands on its far side as one valid solid of the two volumes.
    let wall_frame = Frame::new(Vec3::new(1.0 - R - T, 2.0 + y, 3.0), Vec3::Z, Vec3::X).unwrap();
    let wall = k.make_box(&wall_frame, Vec3::new(T, 40.0 - y, 10.0)).unwrap().shape;
    let wall_volume = T * (40.0 - y) * 10.0;
    let joined = k.boolean(BoolOp::Union, s, &[wall]).unwrap().shape;
    assert!(k.is_valid(joined).unwrap());
    assert_eq!(k.topology(joined).unwrap().solids, 1);
    let total = k.mass_properties(joined, 1.0).unwrap().volume;
    assert!((total - want - wall_volume).abs() < 1e-5 * (want + wall_volume), "{total}");
    for h in [s, wall, joined] {
        k.release(h);
    }
}

#[test]
fn a_slanted_end_rolls_into_a_twisted_one_and_what_cannot_be_rolled_is_refused() {
    let mut k = OcctKernel::new();
    let w = FRAC_PI_2 * RHO;
    // One end runs from 0 at the start of the bend to 5 at its far side.
    let op = rolled(
        &mut k,
        vec![line(1, (0.0, 0.0), (w, 5.0)), line(2, (w, 5.0), (w, 40.0)), line(3, (w, 40.0), (0.0, 40.0)), line(4, (0.0, 40.0), (0.0, 0.0))],
    );
    assert!(k.is_valid(op.shape).unwrap());
    let volume = k.mass_properties(op.shape, 1.0).unwrap().volume;
    let want = volume_of(w * 40.0 - w * 5.0 / 2.0);
    assert!((volume - want).abs() < 1e-5 * want, "{volume} vs {want}");
    assert_eq!(faces_of(&op, 1).len(), 1);
    k.release(op.shape);

    let square =
        |w: f64| vec![line(1, (0.0, 0.0), (w, 0.0)), line(2, (w, 0.0), (w, 4.0)), line(3, (w, 4.0), (0.0, 4.0)), line(4, (0.0, 4.0), (0.0, 0.0))];
    let profile = |curves: Vec<TaggedCurve2>, holes: Vec<Loop>| Profile { frame: frame(), regions: vec![Region { outer: Loop { curves }, holes }] };
    // More than once round the axis; a hole; radii the wrong way round.
    let e = k.roll(&profile(square(7.0 * RHO), vec![]), RHO, R, R + T).unwrap_err();
    assert!(e.to_string().contains("less than once round"), "{e}");
    let hole = Loop { curves: vec![TaggedCurve2 { tag: 5, curve: Curve2::Circle { center: Vec2::new(1.0, 2.0), radius: 0.5 } }] };
    assert!(matches!(k.roll(&profile(square(3.0), vec![hole]), RHO, R, R + T), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.roll(&profile(square(3.0), vec![]), RHO, R + T, R), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.roll(&profile(square(3.0), vec![]), 0.0, R, R + T), Err(KernelError::InvalidInput(_))));
    assert_eq!(k.live_shapes(), 0);
}

#[test]
fn faces_in_one_plane_are_made_one_face_where_asked_and_nowhere_else() {
    let mut k = OcctKernel::new();
    // Two blocks end to end, joined: 14 faces, four of them cut in two by the joint.
    let a = k.make_box(&Frame::WORLD, Vec3::new(10.0, 4.0, 2.0)).unwrap().shape;
    let b = k.make_box(&Frame::new(Vec3::new(10.0, 0.0, 0.0), Vec3::Z, Vec3::X).unwrap(), Vec3::new(5.0, 4.0, 2.0)).unwrap().shape;
    let joined = k.boolean(BoolOp::Union, a, &[b]).unwrap().shape;
    let topo = k.topology(joined).unwrap();
    assert_eq!(topo.faces, 10, "two ends and four sides in two pieces each");
    let volume = k.mass_properties(joined, 1.0).unwrap().volume;
    // The faces of the shorter block: those whose middle is past x = 10.
    let short: Vec<_> = (0..topo.faces).filter(|f| k.face_info(joined.face(*f)).unwrap().centroid.x > 10.0).map(|f| joined.face(f)).collect();
    assert_eq!(short.len(), 5);
    let op = k.unify(joined, &short).unwrap();
    let one = op.shape;
    assert!(k.is_valid(one).unwrap());
    let after = k.topology(one).unwrap();
    assert_eq!((after.faces, after.edges, after.solids), (6, 12, 1), "a plain block again");
    assert!((k.mass_properties(one, 1.0).unwrap().volume - volume).abs() < 1e-9);
    // Every face of the two blocks says what it became: each side's two pieces, the one face.
    let images: Vec<Vec<u32>> = (0..topo.faces)
        .map(|f| {
            op.history
                .images
                .iter()
                .find(|i| i.source.id.kind == TopoKind::Face && i.source.id.index == f)
                .map_or(vec![], |i| i.result.iter().map(|r| r.index).collect())
        })
        .collect();
    assert!(images.iter().all(|i| i.len() == 1), "{images:?}");
    let mut became: Vec<u32> = images.iter().map(|i| i[0]).collect();
    became.sort_unstable();
    became.dedup();
    assert_eq!(became.len(), 6);
    // Asked of a face that has no neighbour in its plane, nothing changes.
    let end = (0..topo.faces).map(|f| joined.face(f)).find(|f| k.face_info(*f).unwrap().centroid.x < 1e-9).unwrap();
    let same = k.unify(joined, &[end]).unwrap().shape;
    assert_eq!(k.topology(same).unwrap().faces, 10);
    assert!(matches!(k.unify(joined, &[one.face(0)]), Err(KernelError::InvalidInput(_))));
    for h in [a, b, joined, one, same] {
        k.release(h);
    }
    assert_eq!(k.live_shapes(), 0);
}
