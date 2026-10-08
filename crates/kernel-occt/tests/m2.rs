//! M2 kernel operations against analytic values: fillet, chamfer, shell, and their history.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use tenon_geom::{Frame, Vec3, tol};
use tenon_kernel::{ChamferSpec, EdgeId, FaceId, InputRef, Kernel, Origin, ShapeHandle, SurfaceKind, TopoId, TopoKind};
use tenon_kernel_occt::OcctKernel;

fn cube(k: &mut OcctKernel) -> ShapeHandle {
    k.make_box(&Frame::WORLD, Vec3::new(20.0, 20.0, 20.0)).unwrap().shape
}

fn volume(k: &OcctKernel, s: ShapeHandle) -> f64 {
    k.mass_properties(s, 1.0).unwrap().volume
}

fn close(a: f64, b: f64) -> bool {
    tol::rel_eq(a, b, 1e-6)
}

/// The face of `s` whose plane has normal `n`.
fn face_with_normal(k: &OcctKernel, s: ShapeHandle, n: Vec3) -> FaceId {
    let faces = k.topology(s).unwrap().faces;
    (0..faces)
        .map(|i| s.face(i))
        .find(|f| matches!(k.face_info(*f).unwrap().surface, SurfaceKind::Plane { normal, .. } if normal.near(n, 1e-9)))
        .unwrap()
}

#[test]
fn fillet_one_edge_and_its_history() {
    let mut k = OcctKernel::new();
    let b = cube(&mut k);
    let op = k.fillet(b, &[b.edge(0)], 2.0).unwrap();
    let v = volume(&k, op.shape);
    let expected = 8000.0 - (4.0 - PI) * 20.0;
    assert!(close(v, expected), "{v} vs {expected}");
    assert!(k.is_valid(op.shape).unwrap());
    assert_eq!(k.topology(op.shape).unwrap().faces, 7);
    // The rounded face is generated from the input edge; it is a cylinder of radius 2.
    let made: Vec<TopoId> =
        op.history.generated_from(Origin::Input(InputRef { input: 0, id: TopoId::edge(0) })).filter(|t| t.kind == TopoKind::Face).collect();
    assert_eq!(made.len(), 1, "{:?}", op.history.generated);
    assert!(
        matches!(k.face_info(op.shape.face(made[0].index)).unwrap().surface, SurfaceKind::Cylinder { radius, .. } if (radius - 2.0).abs() < 1e-9)
    );
    // Every original face survives (modified or not).
    for f in 0..6 {
        assert!(!op.history.image_of(InputRef { input: 0, id: TopoId::face(f) }).unwrap().is_empty(), "face {f}");
    }
}

#[test]
fn fillet_every_edge_of_a_cube() {
    let mut k = OcctKernel::new();
    let b = cube(&mut k);
    let edges: Vec<EdgeId> = (0..12).map(|i| b.edge(i)).collect();
    let op = k.fillet(b, &edges, 2.0).unwrap();
    // Core, face slabs, edge quarter-cylinders and corner eighth-spheres.
    let (a, r) = (16.0f64, 2.0f64);
    let expected = a.powi(3) + 6.0 * a * a * r + 12.0 * a * PI * r * r / 4.0 + 4.0 / 3.0 * PI * r.powi(3);
    let v = volume(&k, op.shape);
    assert!(close(v, expected), "{v} vs {expected}");
    assert_eq!(k.topology(op.shape).unwrap().faces, 26);
    // The corner blends come from the cube's vertices.
    let from_vertices =
        op.history.generated.iter().filter(|g| matches!(g.origin, Origin::Input(InputRef { id: TopoId { kind: TopoKind::Vertex, .. }, .. }))).count();
    assert_eq!(from_vertices, 8, "{:?}", op.history.generated);
}

#[test]
fn chamfers_equal_and_unequal() {
    let mut k = OcctKernel::new();
    let b = cube(&mut k);
    let op = k.chamfer(b, &[b.edge(0)], &ChamferSpec::Equal(2.0)).unwrap();
    assert!(close(volume(&k, op.shape), 8000.0 - 2.0 * 2.0 / 2.0 * 20.0));
    let made = op.history.generated_from(Origin::Input(InputRef { input: 0, id: TopoId::edge(0) })).filter(|t| t.kind == TopoKind::Face).count();
    assert_eq!(made, 1);
    // Two distances: a 2 x 3 right triangle off the edge.
    let t = k.topology(b).unwrap();
    let reference = t.edge_faces[0][0];
    let op = k.chamfer(b, &[b.edge(0)], &ChamferSpec::TwoDistances { d1: 2.0, d2: 3.0, reference: b.face(reference) }).unwrap();
    assert!(close(volume(&k, op.shape), 8000.0 - 3.0 * 20.0), "{}", volume(&k, op.shape));
    assert!(k.is_valid(op.shape).unwrap());
}

#[test]
fn shell_with_an_open_top() {
    let mut k = OcctKernel::new();
    let b = cube(&mut k);
    let top = face_with_normal(&k, b, Vec3::Z);
    let op = k.shell(b, &[top], 2.0).unwrap();
    let expected = 8000.0 - 16.0 * 16.0 * 18.0;
    let v = volume(&k, op.shape);
    assert!(close(v, expected), "{v} vs {expected}");
    assert!(k.is_valid(op.shape).unwrap());
    // The inner walls come from the outer ones (OCCT reports the open face as modified into the
    // rim left around the opening).
    let inner =
        op.history.generated.iter().filter(|g| matches!(g.origin, Origin::Input(InputRef { id: TopoId { kind: TopoKind::Face, .. }, .. }))).count();
    assert!(inner >= 5, "inner walls generated from faces: {:?}", op.history.generated);
}

#[test]
fn bad_fillets_chamfers_and_shells_are_errors() {
    let mut k = OcctKernel::new();
    let b = cube(&mut k);
    let other = cube(&mut k);
    assert!(k.fillet(b, &[], 1.0).is_err(), "no edges");
    assert!(k.fillet(b, &[other.edge(0)], 1.0).is_err(), "edge of another shape");
    assert!(k.fillet(b, &[b.edge(99)], 1.0).is_err(), "edge out of range");
    assert!(k.fillet(b, &[b.edge(0)], -1.0).is_err());
    let all: Vec<EdgeId> = (0..12).map(|i| b.edge(i)).collect();
    assert!(k.fillet(b, &all, 11.0).is_err(), "radius larger than the part");
    assert!(k.chamfer(b, &[b.edge(0)], &ChamferSpec::Equal(f64::NAN)).is_err());
    assert!(k.shell(b, &[other.face(0)], 1.0).is_err());
    assert!(k.shell(b, &[b.face(0)], 0.0).is_err());
    // Failures leave nothing behind.
    assert_eq!(k.live_shapes(), 2);
}
