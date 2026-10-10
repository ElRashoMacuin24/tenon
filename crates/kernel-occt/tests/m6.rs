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
