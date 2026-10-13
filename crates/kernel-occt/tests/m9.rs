//! M9: what the backend keeps from one feature to the next. One test, in a test program of its
//! own: it counts the work done since the program started, which other tests would add to.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use tenon_geom::{Axis, Frame, Vec3};
use tenon_kernel::{BoolOp, Kernel, MeshTol, ShapeHandle};
use tenon_kernel_occt::OcctKernel;

fn hole(k: &mut dyn Kernel, plate: ShapeHandle, x: f64, y: f64) -> ShapeHandle {
    let drill = k.make_cylinder(&Axis::new(Vec3::new(x, y, -1.0), Vec3::Z).unwrap(), 2.0, 12.0).unwrap().shape;
    k.boolean(BoolOp::Cut, plate, &[drill]).unwrap().shape
}

/// After a feature, only the faces it made or changed are summed and meshed again: the rest
/// keep their share of the volume and their triangles. This is most of why an edit of a part
/// with many features costs what the edit changes, not what the part is.
#[test]
fn after_a_feature_only_what_it_changed_is_summed_and_meshed_again() {
    let mut k = OcctKernel::new();
    // A plate 200 by 100 by 10 with holes in two rows: 6 flat faces and one round one a hole.
    let mut plate = k.make_box(&Frame::WORLD, Vec3::new(200.0, 100.0, 10.0)).unwrap().shape;
    for i in 0..10 {
        plate = hole(&mut k, plate, 15.0 + 18.0 * f64::from(i), 30.0);
    }
    let faces = k.topology(plate).unwrap().faces;
    assert_eq!(faces, 16);
    let volume = |k: &OcctKernel, s: ShapeHandle| k.mass_properties(s, 1.0).unwrap().volume;
    let exact = |holes: f64| 200.0 * 100.0 * 10.0 - holes * std::f64::consts::PI * 4.0 * 10.0;

    // Measured and meshed for the first time: every face is summed, and every face is meshed.
    let start = OcctKernel::counters();
    assert!((volume(&k, plate) - exact(10.0)).abs() < 1e-6 * exact(10.0));
    let shown = k.tessellate(plate, &MeshTol::default()).unwrap();
    let first = OcctKernel::counters().since(&start);
    assert_eq!((first.faces_summed, first.sums_kept), (16, 0), "{first:?}");
    assert_eq!((first.faces_meshed, first.meshes_kept), (16, 0), "{first:?}");

    // The same again: nothing is summed, nothing is meshed.
    let before = OcctKernel::counters();
    assert_eq!(volume(&k, plate), volume(&k, plate));
    assert_eq!(k.tessellate(plate, &MeshTol::default()).unwrap(), shown);
    let again = OcctKernel::counters().since(&before);
    assert_eq!((again.faces_summed, again.sums_kept), (0, 32), "{again:?}");
    assert_eq!((again.faces_meshed, again.meshes_kept), (0, 16), "{again:?}");

    // One more hole. It changes the top and the bottom of the plate and adds its own wall:
    // those three are summed and meshed; the four sides and the ten other holes are not.
    let before = OcctKernel::counters();
    let more = hole(&mut k, plate, 100.0, 70.0);
    assert_eq!(k.topology(more).unwrap().faces, 17);
    assert!((volume(&k, more) - exact(11.0)).abs() < 1e-6 * exact(11.0));
    let mesh = k.tessellate(more, &MeshTol::default()).unwrap();
    let after = OcctKernel::counters().since(&before);
    assert_eq!((after.faces_summed, after.sums_kept), (3, 14), "{after:?}");
    assert_eq!((after.faces_meshed, after.meshes_kept), (3, 14), "{after:?}");
    // What was kept is right: the mesh is closed and holds the part's volume, less what its
    // flat sides round the holes leave out.
    let kept = mesh.signed_volume() / exact(11.0);
    assert!(kept > 1.0 && kept < 1.001, "{kept}");
    assert_eq!(mesh.faces.len(), 17);

    // A far mesh is made apart: it neither uses the display's triangles nor disturbs them.
    let before = OcctKernel::counters();
    let far = k.tessellate(more, &MeshTol::far(224.0)).unwrap();
    assert!(far.triangle_count() * 2 < mesh.triangle_count(), "{} of {}", far.triangle_count(), mesh.triangle_count());
    assert_eq!(k.tessellate(more, &MeshTol::default()).unwrap(), mesh);
    let apart = OcctKernel::counters().since(&before);
    assert_eq!((apart.faces_meshed, apart.meshes_kept), (17, 17), "{apart:?}");

    // A part moved far enough that its volume is reckoned from another point is summed afresh,
    // and comes to the same.
    let moved = k.transform(more, &tenon_kernel::Transform::Translate(Vec3::new(5000.0, 0.0, 0.0))).unwrap().shape;
    let before = OcctKernel::counters();
    assert!((volume(&k, moved) - exact(11.0)).abs() < 1e-6 * exact(11.0));
    assert_eq!(OcctKernel::counters().since(&before).faces_summed, 17);
}
