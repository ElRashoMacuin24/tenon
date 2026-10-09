//! M4 kernel: hidden-line removal for drawing views.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use tenon_geom::{Axis, Frame, Vec2, Vec3};
use tenon_kernel::{BoolOp, HlrCurve, HlrKind, Kernel};
use tenon_kernel_occt::OcctKernel;

/// Looking at the front (from -Y): view X is world X, view Y is world Z.
fn front() -> Frame {
    Frame::new(Vec3::ZERO, -Vec3::Y, Vec3::X).unwrap()
}

fn length(c: &HlrCurve) -> f64 {
    c.points.windows(2).map(|w| w[0].dist(w[1])).sum()
}

fn total(curves: &[HlrCurve], visible: bool, kind: Option<HlrKind>) -> f64 {
    curves.iter().filter(|c| c.visible == visible && kind.is_none_or(|k| c.kind == k)).map(length).sum()
}

#[test]
fn a_box_seen_from_the_front_is_its_front_rectangle() {
    let mut k = OcctKernel::new();
    let b = k.make_box(&Frame::WORLD, Vec3::new(10.0, 20.0, 30.0)).unwrap().shape;
    let curves = k.project_edges(&[b], &front(), 0.01, true).unwrap();
    assert!((total(&curves, true, None) - 80.0).abs() < 1e-6, "{curves:?}");
    for c in curves.iter().filter(|c| c.visible) {
        for p in &c.points {
            assert!(p.x > -1e-6 && p.x < 10.0 + 1e-6 && p.y > -1e-6 && p.y < 30.0 + 1e-6, "{p:?}");
        }
    }
    // Seen from the top (from +Z), view Y is world Y: a 10 x 20 rectangle.
    let top = Frame::new(Vec3::ZERO, Vec3::Z, Vec3::X).unwrap();
    let curves = k.project_edges(&[b], &top, 0.01, false).unwrap();
    assert!((total(&curves, true, None) - 60.0).abs() < 1e-6);
    assert!(curves.iter().all(|c| c.visible), "no hidden edges asked for");
    k.release(b);
}

#[test]
fn a_cylinder_shows_its_silhouettes_and_a_hole_its_hidden_lines() {
    let mut k = OcctKernel::new();
    let cyl = k.make_cylinder(&Axis::Z, 5.0, 20.0).unwrap().shape;
    let curves = k.project_edges(&[cyl], &front(), 0.01, true).unwrap();
    // Two silhouettes of 20, and the two end circles seen edge-on as lines of 10. (A silhouette
    // that falls on the cylinder's seam comes as a sharp edge; from the side, the seam facing the
    // viewer is not drawn.)
    assert!((total(&curves, true, None) - 60.0).abs() < 1e-3, "{curves:?}");
    assert!(total(&curves, true, Some(HlrKind::Outline)) >= 20.0 - 1e-6);
    let side = Frame::new(Vec3::ZERO, Vec3::X, Vec3::Y).unwrap();
    let curves = k.project_edges(&[cyl], &side, 0.01, false).unwrap();
    assert!((total(&curves, true, None) - 60.0).abs() < 1e-3, "no seam down the middle: {curves:?}");

    // A 20 x 20 x 10 block with a 6 mm hole through it, seen from the front: the hole's walls are
    // hidden, two vertical lines at x = 10 -+ 3.
    let block = k.make_box(&Frame::WORLD, Vec3::new(20.0, 20.0, 10.0)).unwrap().shape;
    let pin = k.make_cylinder(&Axis::new(Vec3::new(10.0, 10.0, -1.0), Vec3::Z).unwrap(), 3.0, 12.0).unwrap().shape;
    let holed = k.boolean(BoolOp::Cut, block, &[pin]).unwrap().shape;
    let curves = k.project_edges(&[holed], &front(), 0.01, true).unwrap();
    let hidden: Vec<&HlrCurve> = curves.iter().filter(|c| !c.visible).collect();
    let xs: Vec<f64> = hidden.iter().flat_map(|c| c.points.iter().map(|p| p.x)).collect();
    assert!(xs.iter().any(|x| (x - 7.0).abs() < 1e-6) && xs.iter().any(|x| (x - 13.0).abs() < 1e-6), "{hidden:?}");
    // (Hidden edges behind visible ones come too; the drawing leaves those out.)
    let vertical_at = |c: &HlrCurve, x: f64| c.points.iter().all(|p| (p.x - x).abs() < 1e-6);
    let walls: f64 = hidden.iter().filter(|c| vertical_at(c, 7.0) || vertical_at(c, 13.0)).map(|c| length(c)).sum();
    assert!((walls - 20.0).abs() < 1e-3, "two hidden walls of 10: {hidden:?}");
    // Without hidden edges, none come.
    assert!(k.project_edges(&[holed], &front(), 0.01, false).unwrap().iter().all(|c| c.visible));
    // Hostile input.
    assert!(k.project_edges(&[holed], &front(), f64::NAN, true).is_err());
    let _ = Vec2::ZERO;
    for s in [cyl, block, pin, holed] {
        k.release(s);
    }
}
