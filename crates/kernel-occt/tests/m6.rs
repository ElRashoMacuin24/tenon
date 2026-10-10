//! M6 kernel operations: sweep along lines and arcs, along a helix (coils and threads), loft
//! through sections, and draft. Volumes against hand-computed values; every side face traced to
//! the profile curve it came from; refusals for bad input.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{PI, TAU};

use tenon_geom::{Frame, Vec2, Vec3};
use tenon_kernel::{
    Curve2, Curve3, Kernel, KernelError, LoftOpts, Loop, Origin, Path3, PrimitiveRole, Profile, Region, ShapeHandle, SweepOpts, SweepOrientation,
    TaggedCurve2,
};
use tenon_kernel_occt::OcctKernel;

fn volume(k: &dyn Kernel, s: ShapeHandle) -> f64 {
    k.mass_properties(s, 1.0).unwrap().volume
}

fn near(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs()
}

fn line(tag: u64, a: (f64, f64), b: (f64, f64)) -> TaggedCurve2 {
    TaggedCurve2 { tag, curve: Curve2::Line { start: Vec2::new(a.0, a.1), end: Vec2::new(b.0, b.1) } }
}

/// A square of side `s` centred on the frame's origin, its sides tagged 1 to 4.
fn square(frame: Frame, s: f64) -> Profile {
    let h = s / 2.0;
    let outer = Loop { curves: vec![line(1, (-h, -h), (h, -h)), line(2, (h, -h), (h, h)), line(3, (h, h), (-h, h)), line(4, (-h, h), (-h, -h))] };
    Profile { frame, regions: vec![Region { outer, holes: vec![] }] }
}

/// A circle of radius `r` centred on the frame's origin, tagged 7.
fn circle(frame: Frame, r: f64) -> Profile {
    let outer = Loop { curves: vec![TaggedCurve2 { tag: 7, curve: Curve2::Circle { center: Vec2::ZERO, radius: r } }] };
    Profile { frame, regions: vec![Region { outer, holes: vec![] }] }
}

fn solid() -> SweepOpts {
    SweepOpts { orientation: SweepOrientation::Frenet, solid: true }
}

/// The profile tags the faces of `op` were swept from, sorted.
fn tags(op: &tenon_kernel::Op) -> Vec<u64> {
    let mut t: Vec<u64> = op
        .history
        .generated
        .iter()
        .filter_map(|g| match g.origin {
            Origin::ProfileCurve { tag } => Some(tag),
            Origin::Input(_) => None,
        })
        .collect();
    t.sort_unstable();
    t.dedup();
    t
}

fn caps(op: &tenon_kernel::Op) -> usize {
    op.history.roles.iter().filter(|(r, _)| matches!(r, PrimitiveRole::StartCap | PrimitiveRole::EndCap)).count()
}

#[test]
fn a_sweep_along_a_bent_path_is_area_times_path_length() {
    let mut k = OcctKernel::new();
    // A Ø4 circle on the XY plane, swept up 20, round a quarter circle of radius 10, then 20 along X.
    let s = 1.0 / 2f64.sqrt();
    let path = Path3 {
        curves: vec![
            Curve3::Line { start: Vec3::ZERO, end: Vec3::new(0.0, 0.0, 20.0) },
            Curve3::Arc { start: Vec3::new(0.0, 0.0, 20.0), mid: Vec3::new(10.0 - 10.0 * s, 0.0, 20.0 + 10.0 * s), end: Vec3::new(10.0, 0.0, 30.0) },
            Curve3::Line { start: Vec3::new(10.0, 0.0, 30.0), end: Vec3::new(30.0, 0.0, 30.0) },
        ],
    };
    let op = k.sweep(&circle(Frame::WORLD, 2.0), &path, &solid()).unwrap();
    // Pappus: the profile's centre stays on the path, so area x length.
    let expected = PI * 4.0 * (20.0 + 10.0 * PI / 2.0 + 20.0);
    assert!(near(volume(&k, op.shape), expected, 1e-4), "{} vs {expected}", volume(&k, op.shape));
    assert!(k.is_valid(op.shape).unwrap());
    assert_eq!(tags(&op), [7], "the tube's wall comes from the circle");
    assert_eq!(caps(&op), 2);

    // A square straight up: a box, its four sides each from its line.
    let op = k
        .sweep(&square(Frame::WORLD, 10.0), &Path3 { curves: vec![Curve3::Line { start: Vec3::ZERO, end: Vec3::new(0.0, 0.0, 30.0) }] }, &solid())
        .unwrap();
    assert!(near(volume(&k, op.shape), 3000.0, 1e-9));
    assert_eq!(tags(&op), [1, 2, 3, 4]);
    let fixed = SweepOpts { orientation: SweepOrientation::Fixed, solid: true };
    let op = k
        .sweep(&square(Frame::WORLD, 10.0), &Path3 { curves: vec![Curve3::Line { start: Vec3::ZERO, end: Vec3::new(0.0, 0.0, 30.0) }] }, &fixed)
        .unwrap();
    assert!(near(volume(&k, op.shape), 3000.0, 1e-9));
}

#[test]
fn a_coil_sweeps_its_profile_round_the_axis() {
    let mut k = OcctKernel::new();
    // A Ø2 circle in the XZ plane at radius 10, swept 3 turns of pitch 5 round the Z axis.
    let at = Frame::new(Vec3::new(10.0, 0.0, 0.0), Vec3::Y, Vec3::X).unwrap();
    let helix = |left| Path3 { curves: vec![Curve3::Helix { frame: Frame::WORLD, radius: 10.0, pitch: 5.0, turns: 3.0, left }] };
    for left in [false, true] {
        let op = k.sweep(&circle(at, 1.0), &helix(left), &solid()).unwrap();
        // A profile in a plane through the axis sweeps area x the distance its centre travels round
        // the axis, whatever the pitch.
        let expected = PI * 1.0 * TAU * 10.0 * 3.0;
        assert!(near(volume(&k, op.shape), expected, 1e-3), "{} vs {expected}", volume(&k, op.shape));
        assert!(k.is_valid(op.shape).unwrap());
        assert_eq!(tags(&op), [7]);
        let bb = k.bounding_box(op.shape).unwrap().unwrap();
        assert!(near(bb.max.z - bb.min.z, 15.0 + 2.0, 1e-3), "3 turns of 5, plus the profile: {bb:?}");
    }
}

#[test]
fn a_loft_between_two_squares_is_a_frustum() {
    let mut k = OcctKernel::new();
    let bottom = square(Frame::WORLD, 20.0);
    let top = square(Frame::new(Vec3::new(0.0, 0.0, 10.0), Vec3::Z, Vec3::X).unwrap(), 10.0);
    let ruled = LoftOpts { solid: true, ruled: true, closed: false };
    let op = k.loft(&[bottom.clone(), top.clone()], &ruled).unwrap();
    let expected = 10.0 / 3.0 * (400.0 + 100.0 + 200.0);
    assert!(near(volume(&k, op.shape), expected, 1e-9), "{}", volume(&k, op.shape));
    assert_eq!(tags(&op), [1, 2, 3, 4], "each side from the first section's line");
    assert_eq!(caps(&op), 2);
    // Smooth through three sections: a valid solid between the ruled one and its hull.
    let mid = square(Frame::new(Vec3::new(0.0, 0.0, 5.0), Vec3::Z, Vec3::X).unwrap(), 18.0);
    let smooth = LoftOpts { solid: true, ruled: false, closed: false };
    let op = k.loft(&[bottom, mid, top], &smooth).unwrap();
    assert!(k.is_valid(op.shape).unwrap());
    assert!(volume(&k, op.shape) > expected, "bulging through the wider middle section");
}

#[test]
fn a_draft_tilts_the_sides_about_the_neutral_plane() {
    let mut k = OcctKernel::new();
    let block = k
        .sweep(&square(Frame::WORLD, 20.0), &Path3 { curves: vec![Curve3::Line { start: Vec3::ZERO, end: Vec3::new(0.0, 0.0, 10.0) }] }, &solid())
        .unwrap();
    let topo = k.topology(block.shape).unwrap();
    // The four side faces: their normals are horizontal.
    let sides: Vec<_> = (0..topo.faces)
        .map(|i| block.shape.face(i))
        .filter(|f| match k.face_info(*f).unwrap().surface {
            tenon_kernel::SurfaceKind::Plane { normal, .. } => normal.z.abs() < 1e-9,
            _ => false,
        })
        .collect();
    assert_eq!(sides.len(), 4);
    let angle = 5f64.to_radians();
    let op = k.draft(block.shape, &sides, Vec3::Z, angle, &Frame::WORLD).unwrap();
    assert!(k.is_valid(op.shape).unwrap());
    // The bottom stays 20 x 20 (on the neutral plane); the top shrinks or grows by 2 h tan(a).
    let d = 2.0 * 10.0 * angle.tan();
    let frustum = |top: f64| 10.0 / 3.0 * (400.0 + top * top + 20.0 * top);
    let v = volume(&k, op.shape);
    assert!(near(v, frustum(20.0 - d), 1e-6) || near(v, frustum(20.0 + d), 1e-6), "{v}");
    assert!(!op.history.images.is_empty(), "the faces keep their history");
}

#[test]
fn bad_sweeps_lofts_and_drafts_are_refused() {
    let mut k = OcctKernel::new();
    let p = square(Frame::WORLD, 10.0);
    let bad = |e: KernelError| matches!(e, KernelError::InvalidInput(_) | KernelError::OperationFailed { .. });
    assert!(bad(k.sweep(&p, &Path3 { curves: vec![] }, &solid()).unwrap_err()));
    let helix = Curve3::Helix { frame: Frame::WORLD, radius: 10.0, pitch: 5.0, turns: 0.0, left: false };
    assert!(bad(k.sweep(&p, &Path3 { curves: vec![helix.clone()] }, &solid()).unwrap_err()));
    let mixed = Path3 { curves: vec![Curve3::Line { start: Vec3::ZERO, end: Vec3::Z }, helix] };
    assert!(bad(k.sweep(&p, &mixed, &solid()).unwrap_err()));
    // Path curves that do not meet.
    let apart = Path3 {
        curves: vec![
            Curve3::Line { start: Vec3::ZERO, end: Vec3::Z },
            Curve3::Line { start: Vec3::new(5.0, 0.0, 1.0), end: Vec3::new(5.0, 0.0, 9.0) },
        ],
    };
    assert!(bad(k.sweep(&p, &apart, &solid()).unwrap_err()));
    assert!(bad(k.loft(std::slice::from_ref(&p), &LoftOpts { solid: true, ruled: true, closed: false }).unwrap_err()));
    let block = k.sweep(&p, &Path3 { curves: vec![Curve3::Line { start: Vec3::ZERO, end: Vec3::new(0.0, 0.0, 10.0) }] }, &solid()).unwrap();
    let f = block.shape.face(0);
    assert!(bad(k.draft(block.shape, &[f], Vec3::Z, 0.0, &Frame::WORLD).unwrap_err()));
    assert!(bad(k.draft(block.shape, &[f], Vec3::ZERO, 0.1, &Frame::WORLD).unwrap_err()));
    assert!(bad(k.draft(block.shape, &[], Vec3::Z, 0.1, &Frame::WORLD).unwrap_err()));
}

/// Freeform faces are meshed to a looser tolerance inside than along their edges: a long coil
/// used to come out at half a million triangles and take seconds to show.
#[test]
fn a_long_coil_meshes_without_excess_triangles() {
    let mut k = OcctKernel::new();
    let at = Frame::new(Vec3::new(40.0, 0.0, 0.0), Vec3::Y, Vec3::X).unwrap();
    let helix = Path3 { curves: vec![Curve3::Helix { frame: Frame::WORLD, radius: 40.0, pitch: 10.0, turns: 5.0, left: false }] };
    let op = k.sweep(&circle(at, 1.5), &helix, &solid()).unwrap();
    let m = k.tessellate(op.shape, &tenon_kernel::MeshTol::default()).unwrap();
    assert!(m.triangle_count() < 150_000, "{} triangles", m.triangle_count());
}

/// Threads need to know a shaft from a hole: a round face says which side its material is on.
#[test]
fn a_round_face_says_whether_it_is_a_shaft_or_a_hole() {
    let mut k = OcctKernel::new();
    let up = Path3 { curves: vec![Curve3::Line { start: Vec3::ZERO, end: Vec3::new(0.0, 0.0, 10.0) }] };
    let round = |k: &OcctKernel, s: ShapeHandle| {
        let n = k.topology(s).unwrap().faces;
        let walls: Vec<bool> = (0..n)
            .filter_map(|i| k.face_info(s.face(i)).ok())
            .filter(|f| matches!(f.surface, tenon_kernel::SurfaceKind::Cylinder { .. }))
            .map(|f| f.reversed)
            .collect();
        assert_eq!(walls.len(), 1, "one round face");
        walls[0]
    };
    // An extruded circle, a revolved rectangle and a primitive cylinder are all shafts.
    let shaft = k.sweep(&circle(Frame::WORLD, 4.0), &up, &solid()).unwrap();
    assert!(!round(&k, shaft.shape));
    let axis = tenon_geom::Axis::new(Vec3::ZERO, Vec3::Z).unwrap();
    let primitive = k.make_cylinder(&axis, 4.0, 10.0).unwrap();
    assert!(!round(&k, primitive.shape));
    // Drilled through a block, the same circle is a hole; so is its mirror image.
    let block = k.sweep(&square(Frame::WORLD, 20.0), &up, &solid()).unwrap();
    let hole = k.boolean(tenon_kernel::BoolOp::Cut, block.shape, &[shaft.shape]).unwrap();
    assert!(round(&k, hole.shape));
    let plane = Frame::new(Vec3::new(30.0, 0.0, 0.0), Vec3::X, Vec3::Y).unwrap();
    let mirrored = k.transform(hole.shape, &tenon_kernel::Transform::Mirror { plane }).unwrap();
    assert!(round(&k, mirrored.shape));
    let mirrored_shaft = k.transform(shaft.shape, &tenon_kernel::Transform::Mirror { plane }).unwrap();
    assert!(!round(&k, mirrored_shaft.shape));
}

/// A helix is built a few turns at a time: as one edge, sixty turns came out at half their volume.
#[test]
fn a_coil_of_many_turns_keeps_its_volume() {
    let mut k = OcctKernel::new();
    let at = Frame::new(Vec3::new(10.0, 0.0, 0.0), Vec3::Y, Vec3::X).unwrap();
    for turns in [4.5, 60.0] {
        let helix = Path3 { curves: vec![Curve3::Helix { frame: Frame::WORLD, radius: 10.0, pitch: 5.0, turns, left: false }] };
        let op = k.sweep(&circle(at, 1.0), &helix, &solid()).unwrap();
        let expected = PI * 1.0 * TAU * 10.0 * turns;
        assert!(near(volume(&k, op.shape), expected, 1e-5), "{turns} turns: {} vs {expected}", volume(&k, op.shape));
        assert!(k.is_valid(op.shape).unwrap());
        let bb = k.bounding_box(op.shape).unwrap().unwrap();
        assert!(near(bb.max.z - bb.min.z, 5.0 * turns + 2.0, 1e-6));
    }
}

/// A modelled thread is a helical groove cut from a shaft. With the helix as one long edge, the
/// cut removed nothing or everything for some lengths (a whole number of turns passing through
/// both ends), without an error.
#[test]
fn a_groove_wound_round_a_shaft_is_cut_whatever_its_length() {
    let mut k = OcctKernel::new();
    let (p, r) = (1.25f64, 4.0f64);
    let depth = 0.625 * 0.866_025_403_784_438_6 * p;
    // The groove's section in the XZ plane (x away from the axis, y along it), starting one
    // pitch below the shaft.
    let frame = Frame::new(Vec3::new(0.0, 0.0, -p), Vec3::new(0.0, -1.0, 0.0), Vec3::X).unwrap();
    let q = [Vec2::new(r + 0.07, -0.47 * p), Vec2::new(r + 0.07, 0.47 * p), Vec2::new(r - depth, 0.125 * p), Vec2::new(r - depth, -0.125 * p)];
    // Inside the shaft it is 7/8 of the pitch wide at the surface and 1/4 at its root.
    let per_turn = (0.875 + 0.25) / 2.0 * p * depth * TAU * (r - depth * (0.875 + 0.5) / (3.0 * 1.125));
    for (h, turns) in [(10.0, 10.0), (20.0, 18.0), (10.0, 9.5)] {
        let up = Path3 { curves: vec![Curve3::Line { start: Vec3::ZERO, end: Vec3::new(0.0, 0.0, h) }] };
        let shaft = k.sweep(&circle(Frame::WORLD, r), &up, &solid()).unwrap();
        let curves = (0..4).map(|i| TaggedCurve2 { tag: i as u64 + 1, curve: Curve2::Line { start: q[i], end: q[(i + 1) % 4] } }).collect();
        let profile = Profile { frame, regions: vec![tenon_kernel::Region { outer: tenon_kernel::Loop { curves }, holes: vec![] }] };
        let centre = (q[0] + q[1] + q[2] + q[3]) * 0.25;
        let start = Frame::new(Vec3::new(0.0, 0.0, -p + centre.y), Vec3::Z, Vec3::X).unwrap();
        let helix = Path3 { curves: vec![Curve3::Helix { frame: start, radius: centre.x, pitch: p, turns, left: false }] };
        let tool = k.sweep(&profile, &helix, &SweepOpts { orientation: SweepOrientation::Binormal(Vec3::Z), solid: true }).unwrap();
        let cut = k.boolean(tenon_kernel::BoolOp::Cut, shaft.shape, &[tool.shape]).unwrap();
        assert!(k.is_valid(cut.shape).unwrap());
        let removed = volume(&k, shaft.shape) - volume(&k, cut.shape);
        // The groove runs the whole shaft: its length over the pitch in turns, to within the
        // run-in and run-out at the ends.
        assert!((removed / per_turn - h / p).abs() < 0.2, "{h} mm, {turns} turns: {} turns' worth removed", removed / per_turn);
    }
}

/// A whole turn of a profile that lies across its axis is the turn of each side, joined: a circle
/// about its diameter is a sphere. It used to be refused.
#[test]
fn a_profile_across_its_axis_turns_into_the_solid_of_both_sides() {
    let mut k = OcctKernel::new();
    let y = tenon_geom::Axis::new(Vec3::ZERO, Vec3::Y).unwrap();
    let full = tenon_kernel::AngleExtent::Full;
    // A circle about its diameter.
    let ball = k.revolve(&circle(Frame::WORLD, 10.0), &y, &full).unwrap();
    assert!(near(volume(&k, ball.shape), 4.0 / 3.0 * PI * 1000.0, 1e-9), "{}", volume(&k, ball.shape));
    assert!(k.is_valid(ball.shape).unwrap());
    assert_eq!(tags(&ball), [7], "its face is still the circle's");
    // A square about its middle line: a cylinder.
    let can = k.revolve(&square(Frame::WORLD, 20.0), &y, &full).unwrap();
    assert!(near(volume(&k, can.shape), PI * 100.0 * 20.0, 1e-9), "{}", volume(&k, can.shape));
    assert!(k.is_valid(can.shape).unwrap());
    // Sides that differ: 4 wide and 12 tall on the left of the axis, 10 wide and 5 tall on the
    // right. The turn of each, together.
    let pts = [(-4.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0), (0.0, 12.0), (-4.0, 12.0)];
    let outer = Loop { curves: (0..6).map(|i| line(i as u64 + 1, pts[i], pts[(i + 1) % 6])).collect() };
    let step = Profile { frame: Frame::WORLD, regions: vec![Region { outer, holes: vec![] }] };
    let op = k.revolve(&step, &y, &full).unwrap();
    assert!(near(volume(&k, op.shape), PI * (100.0 * 5.0 + 16.0 * 7.0), 1e-9), "{}", volume(&k, op.shape));
    assert!(k.is_valid(op.shape).unwrap());
    // Part of a turn has no such meaning: refused, saying so.
    let e = k.revolve(&circle(Frame::WORLD, 10.0), &y, &tenon_kernel::AngleExtent::Angle(1.0)).unwrap_err();
    assert!(e.to_string().contains("lies across it"), "{e}");
    // A profile wholly on one side is turned as before.
    let off = Frame::new(Vec3::new(30.0, 0.0, 0.0), Vec3::Z, Vec3::X).unwrap();
    let ring = k.revolve(&circle(off, 10.0), &y, &full).unwrap();
    assert!(near(volume(&k, ring.shape), PI * 100.0 * TAU * 30.0, 1e-9));
}
