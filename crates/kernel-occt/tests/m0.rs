//! M0 acceptance tests for the OpenCASCADE backend: primitives, booleans against analytic
//! volumes, history, topology, tessellation, mass properties, STEP round trip, hostile input.
// Test helpers outside #[test] functions are not covered by clippy.toml's test exemptions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use tenon_geom::{Aabb3, Axis, Frame, Vec3, tol};
use tenon_kernel::{
    BoolOp, CancelToken, CurveKind, InputRef, Kernel, KernelError, MeshTol, PrimitiveRole, ShapeHandle, ShapeKind, SurfaceKind, TopoId,
};
use tenon_kernel_occt::OcctKernel;

fn approx(a: f64, b: f64) -> bool {
    tol::rel_eq(a, b, tol::MEASURE_REL)
}

fn volume(k: &dyn Kernel, s: ShapeHandle) -> f64 {
    k.mass_properties(s, 1.0).unwrap().volume
}

fn boxed(k: &mut dyn Kernel, origin: Vec3, size: Vec3) -> ShapeHandle {
    k.make_box(&Frame::WORLD.with_origin(origin).unwrap(), size).unwrap().shape
}

fn cylinder_z(k: &mut dyn Kernel, base: Vec3, r: f64, h: f64) -> ShapeHandle {
    k.make_cylinder(&Axis::new(base, Vec3::Z).unwrap(), r, h).unwrap().shape
}

/// The 20 x 20 x 10 plate used by several tests.
fn plate(k: &mut dyn Kernel) -> ShapeHandle {
    boxed(k, Vec3::ZERO, Vec3::new(20.0, 20.0, 10.0))
}

#[test]
fn reports_occt_8() {
    let v = OcctKernel::occt_version().unwrap();
    assert!(v.starts_with("8."), "{v}");
    assert!(OcctKernel::new().version().contains(&v));
}

#[test]
fn box_measures_and_topology() {
    let mut k = OcctKernel::new();
    let b = boxed(&mut k, Vec3::new(1.0, 2.0, 3.0), Vec3::new(10.0, 20.0, 30.0));
    let m = k.mass_properties(b, 1.0).unwrap();
    assert!(approx(m.volume, 6000.0), "{}", m.volume);
    assert!(approx(m.area, 2.0 * (200.0 + 600.0 + 300.0)), "{}", m.area);
    assert!(m.center_of_mass.near(Vec3::new(6.0, 12.0, 18.0), 1e-9));
    let bb = k.bounding_box(b).unwrap().unwrap();
    assert!(bb.near(&Aabb3::new(Vec3::new(1.0, 2.0, 3.0), Vec3::new(11.0, 22.0, 33.0)), 1e-6), "{bb:?}");
    let t = k.topology(b).unwrap();
    assert_eq!((t.kind, t.solids, t.shells, t.faces, t.edges, t.vertices), (ShapeKind::Solid, 1, 1, 6, 12, 8));
    assert!(t.face_edges.iter().all(|f| f.len() == 4));
    assert!(t.edge_faces.iter().all(|e| e.len() == 2), "every edge of a closed box joins two faces");
    assert!(t.edge_vertices.iter().all(|e| e.len() == 2));
    assert!(k.is_valid(b).unwrap());
}

#[test]
fn box_inertia_about_centre_of_mass() {
    let mut k = OcctKernel::new();
    let (a, b, c) = (10.0, 20.0, 30.0);
    let s = boxed(&mut k, Vec3::new(5.0, 5.0, 5.0), Vec3::new(a, b, c));
    let density = 7.85e-6; // steel, kg/mm^3
    let m = k.mass_properties(s, density).unwrap();
    let mass = a * b * c * density;
    assert!(approx(m.mass, mass));
    let expect = [mass * (b * b + c * c) / 12.0, mass * (a * a + c * c) / 12.0, mass * (a * a + b * b) / 12.0];
    for (i, (row, want)) in m.inertia.iter().zip(expect).enumerate() {
        assert!(approx(row[i], want), "I{i}{i} = {} expected {want}", row[i]);
        for (j, v) in row.iter().enumerate().filter(|(j, _)| *j != i) {
            assert!(v.abs() < 1e-9 * mass * 1000.0, "off-diagonal {i}{j} = {v}");
        }
    }
}

#[test]
fn box_face_roles_match_geometry() {
    let mut k = OcctKernel::new();
    let op = k.make_box(&Frame::WORLD, Vec3::new(10.0, 20.0, 30.0)).unwrap();
    let expect = [
        (PrimitiveRole::BoxXMin, -Vec3::X),
        (PrimitiveRole::BoxXMax, Vec3::X),
        (PrimitiveRole::BoxYMin, -Vec3::Y),
        (PrimitiveRole::BoxYMax, Vec3::Y),
        (PrimitiveRole::BoxZMin, -Vec3::Z),
        (PrimitiveRole::BoxZMax, Vec3::Z),
    ];
    assert_eq!(op.history.roles.len(), 6);
    for (role, normal) in expect {
        let id = op.history.role(role).unwrap();
        let info = k.face_info(op.shape.face(id.index)).unwrap();
        match info.surface {
            SurfaceKind::Plane { normal: n, .. } => assert!(n.near(normal, 1e-12), "{role:?}: {n:?}"),
            other => panic!("{role:?} is {other:?}"),
        }
    }
}

#[test]
fn cylinder_measures_roles_and_geometry() {
    let mut k = OcctKernel::new();
    let op = k.make_cylinder(&Axis::new(Vec3::new(1.0, 1.0, 0.0), Vec3::Z).unwrap(), 5.0, 10.0).unwrap();
    assert!(approx(volume(&k, op.shape), PI * 25.0 * 10.0));
    let t = k.topology(op.shape).unwrap();
    assert_eq!((t.faces, t.edges, t.vertices), (3, 3, 2), "lateral + 2 caps; 2 circles + seam");
    let lateral = op.history.role(PrimitiveRole::Lateral).unwrap();
    match k.face_info(op.shape.face(lateral.index)).unwrap().surface {
        SurfaceKind::Cylinder { axis, radius } => {
            assert!((radius - 5.0).abs() < 1e-12);
            assert!(axis.dir().near(Vec3::Z, 1e-12) || axis.dir().near(-Vec3::Z, 1e-12));
        }
        other => panic!("lateral face is {other:?}"),
    }
    let top = op.history.role(PrimitiveRole::Top).unwrap();
    let info = k.face_info(op.shape.face(top.index)).unwrap();
    assert!(approx(info.area, PI * 25.0));
    assert!(info.centroid.near(Vec3::new(1.0, 1.0, 10.0), 1e-9));
    let circles = (0..t.edges).filter(|&e| matches!(k.edge_info(op.shape.edge(e)).unwrap().curve, CurveKind::Circle { .. })).count();
    assert_eq!(circles, 2);
}

#[test]
fn other_primitives() {
    let mut k = OcctKernel::new();
    let s = k.make_sphere(Vec3::new(1.0, 2.0, 3.0), 4.0).unwrap().shape;
    assert!(approx(volume(&k, s), 4.0 / 3.0 * PI * 64.0));
    let cone = k.make_cone(&Axis::Z, 5.0, 0.0, 9.0).unwrap();
    assert!(approx(volume(&k, cone.shape), PI * 25.0 * 9.0 / 3.0));
    assert!(cone.history.role(PrimitiveRole::Top).is_none(), "a pointed cone has no top cap");
    let frustum = k.make_cone(&Axis::Z, 4.0, 2.0, 3.0).unwrap().shape;
    assert!(approx(volume(&k, frustum), PI * 3.0 / 3.0 * (16.0 + 8.0 + 4.0)));
    let torus = k.make_torus(&Axis::Z, 10.0, 2.0).unwrap().shape;
    assert!(approx(volume(&k, torus), 2.0 * PI * PI * 10.0 * 4.0));
}

#[test]
fn union_with_partly_overlapping_cylinder() {
    // Cylinder r=4 from z=5 to z=15 on a 20x20x10 plate: half of it is inside.
    let mut k = OcctKernel::new();
    let p = plate(&mut k);
    let c = cylinder_z(&mut k, Vec3::new(10.0, 10.0, 5.0), 4.0, 10.0);
    let u = k.boolean(BoolOp::Union, p, &[c]).unwrap();
    assert!(approx(volume(&k, u.shape), 4000.0 + PI * 16.0 * 5.0));
    assert!(k.is_valid(u.shape).unwrap());
}

#[test]
fn cut_through_hole_with_history() {
    let mut k = OcctKernel::new();
    let plate_op = k.make_box(&Frame::WORLD, Vec3::new(20.0, 20.0, 10.0)).unwrap();
    let c = cylinder_z(&mut k, Vec3::new(10.0, 10.0, -1.0), 4.0, 12.0);
    let cut = k.boolean(BoolOp::Cut, plate_op.shape, &[c]).unwrap();
    assert!(approx(volume(&k, cut.shape), 4000.0 - PI * 16.0 * 10.0));
    let t = k.topology(cut.shape).unwrap();
    assert_eq!(t.kind, ShapeKind::Solid, "a single-solid boolean result is the solid, not a compound");
    assert_eq!(t.faces, 7, "six plate faces plus the hole wall");
    assert!(k.is_valid(cut.shape).unwrap());

    // The plate's top face survives (modified: it gained a hole) as exactly one result face.
    let top_in = plate_op.history.role(PrimitiveRole::BoxZMax).unwrap();
    let image = cut.history.image_of(InputRef { input: 0, id: top_in }).unwrap();
    assert_eq!(image.len(), 1);
    let top_out = k.face_info(cut.shape.face(image[0].index)).unwrap();
    assert!(approx(top_out.area, 400.0 - PI * 16.0), "{}", top_out.area);
    // A side face is untouched and keeps its area.
    let side_in = plate_op.history.role(PrimitiveRole::BoxXMin).unwrap();
    let side = cut.history.image_of(InputRef { input: 0, id: side_in }).unwrap();
    assert_eq!(side.len(), 1);
    assert!(approx(k.face_info(cut.shape.face(side[0].index)).unwrap().area, 200.0));
    // The tool's caps are outside the plate and are deleted; its wall becomes the hole.
    let tool_faces = k.topology(c).unwrap().faces;
    let tool_images: Vec<_> = (0..tool_faces).map(|f| cut.history.image_of(InputRef { input: 1, id: TopoId::face(f) }).unwrap().len()).collect();
    assert_eq!(tool_images.iter().filter(|&&n| n == 0).count(), 2, "{tool_images:?}");
    assert_eq!(tool_images.iter().filter(|&&n| n == 1).count(), 1, "{tool_images:?}");
}

#[test]
fn intersect() {
    let mut k = OcctKernel::new();
    let p = plate(&mut k);
    let c = cylinder_z(&mut k, Vec3::new(10.0, 10.0, -5.0), 4.0, 30.0);
    let i = k.boolean(BoolOp::Intersect, p, &[c]).unwrap();
    assert!(approx(volume(&k, i.shape), PI * 16.0 * 10.0));
}

/// Deterministic pseudo-random numbers (xorshift) so property tests are reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

#[test]
fn inclusion_exclusion_on_random_boxes_and_cylinders() {
    // vol(A ∪ B) + vol(A ∩ B) = vol(A) + vol(B), and vol(A − B) = vol(A) − vol(A ∩ B).
    let mut k = OcctKernel::new();
    let mut rng = Rng(0x7e70_1234_5678_9abc);
    for case in 0..12 {
        let size = Vec3::new(rng.range(5.0, 30.0), rng.range(5.0, 30.0), rng.range(5.0, 30.0));
        let a = boxed(&mut k, Vec3::ZERO, size);
        let base = Vec3::new(rng.range(-5.0, 35.0), rng.range(-5.0, 35.0), rng.range(-10.0, 20.0));
        let b = cylinder_z(&mut k, base, rng.range(1.0, 10.0), rng.range(2.0, 30.0));
        let (va, vb) = (volume(&k, a), volume(&k, b));
        let u = k.boolean(BoolOp::Union, a, &[b]).unwrap().shape;
        let i = k.boolean(BoolOp::Intersect, a, &[b]).unwrap().shape;
        let d = k.boolean(BoolOp::Cut, a, &[b]).unwrap().shape;
        let (vu, vi, vd) = (volume(&k, u), volume(&k, i), volume(&k, d));
        let scale = va + vb;
        assert!((vu + vi - scale).abs() <= 1e-6 * scale, "case {case}: {vu} + {vi} != {va} + {vb}");
        assert!((vd - (va - vi)).abs() <= 1e-6 * scale, "case {case}: {vd} != {va} - {vi}");
        for s in [a, b, u, i, d] {
            k.release(s);
        }
    }
    assert_eq!(k.live_shapes(), 0);
}

#[test]
fn step_round_trip_preserves_volume_and_topology() {
    let mut k = OcctKernel::new();
    let p = plate(&mut k);
    let c = cylinder_z(&mut k, Vec3::new(10.0, 10.0, -1.0), 4.0, 12.0);
    let cut = k.boolean(BoolOp::Cut, p, &[c]).unwrap().shape;
    let data = k.export_step(&[cut]).unwrap();
    let text = String::from_utf8_lossy(&data);
    assert!(text.starts_with("ISO-10303-21;"), "{}", &text[..text.len().min(60)]);

    let mut k2 = OcctKernel::new();
    let shapes = k2.import_step(&data).unwrap();
    assert_eq!(shapes.len(), 1);
    let (v0, v1) = (volume(&k, cut), volume(&k2, shapes[0]));
    assert!(approx(v0, v1), "volume before {v0}, after {v1}");
    let (t0, t1) = (k.topology(cut).unwrap(), k2.topology(shapes[0]).unwrap());
    assert_eq!((t0.faces, t0.edges, t0.vertices), (t1.faces, t1.edges, t1.vertices));
    assert!(k2.bounding_box(shapes[0]).unwrap().unwrap().near(&k.bounding_box(cut).unwrap().unwrap(), 1e-6));
}

#[test]
fn tessellation_is_closed_outward_and_grouped_by_face() {
    let mut k = OcctKernel::new();
    let b = plate(&mut k);
    let m = k.tessellate(b, &MeshTol::default()).unwrap();
    assert_eq!(m.triangle_count(), 12, "two triangles per box face");
    assert_eq!(m.faces.len(), 6);
    assert_eq!(m.edges.len(), 12);
    assert!(m.edges.iter().all(|e| e.points.len() >= 2));
    assert!((m.signed_volume() - 4000.0).abs() < 1e-3, "{}", m.signed_volume());
    assert!(m.normals.iter().all(|n| ((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() - 1.0).abs() < 1e-5));

    let c = cylinder_z(&mut k, Vec3::new(10.0, 10.0, -1.0), 4.0, 12.0);
    let cut = k.boolean(BoolOp::Cut, b, &[c]).unwrap().shape;
    let tol = MeshTol { linear: 0.001, angular: 0.1 };
    let m = k.tessellate(cut, &tol).unwrap();
    let exact = 4000.0 - PI * 160.0;
    // The hole wall is approximated by chords, so the mesh volume is close but not exact.
    assert!((m.signed_volume() - exact).abs() / exact < 1e-3, "{} vs {exact}", m.signed_volume());
    for f in &m.faces {
        assert!(f.count > 0 && f.count % 3 == 0, "face {} has no triangles", f.face);
    }
    assert_eq!(m.face_of_triangle(0), Some(m.faces[0].face));
}

#[test]
fn hostile_input_is_rejected_not_panicking() {
    let mut k = OcctKernel::new();
    for size in [
        Vec3::new(0.0, 1.0, 1.0),
        Vec3::new(-1.0, 1.0, 1.0),
        Vec3::new(f64::NAN, 1.0, 1.0),
        Vec3::new(1.0, f64::INFINITY, 1.0),
        Vec3::new(1.0, 1.0, 1e300),
    ] {
        assert!(matches!(k.make_box(&Frame::WORLD, size), Err(KernelError::InvalidInput(_))), "{size:?}");
    }
    assert!(matches!(k.make_cylinder(&Axis::Z, 0.0, 1.0), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.make_cylinder(&Axis::Z, 1.0, f64::NAN), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.make_cone(&Axis::Z, 2.0, 2.0, 1.0), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.make_cone(&Axis::Z, 0.0, 0.0, 1.0), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.make_torus(&Axis::Z, 1.0, 2.0), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.make_sphere(Vec3::new(f64::NAN, 0.0, 0.0), 1.0), Err(KernelError::InvalidInput(_))));

    let b = plate(&mut k);
    assert!(matches!(k.boolean(BoolOp::Cut, b, &[]), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.face_info(b.face(6)), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.edge_info(b.edge(u32::MAX)), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.mass_properties(b, -1.0), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.tessellate(b, &MeshTol { linear: 0.0, angular: 0.5 }), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.export_step(&[]), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.import_step(b""), Err(KernelError::InvalidInput(_))));
    assert!(matches!(k.import_step(b"this is not a STEP file\n"), Err(KernelError::Exchange(_))));
    let data = k.export_step(&[b]).unwrap();
    assert!(k.import_step(&data[..200]).is_err(), "a truncated STEP file must not import");
}

#[test]
fn released_and_foreign_handles_are_invalid() {
    let mut k = OcctKernel::new();
    let a = plate(&mut k);
    k.release(a);
    assert_eq!(k.topology(a), Err(KernelError::InvalidHandle));
    // The slot is reused with a new generation; the old handle stays invalid.
    let b = plate(&mut k);
    assert_eq!(a.index(), b.index());
    assert_ne!(a, b);
    assert_eq!(k.mass_properties(a, 1.0), Err(KernelError::InvalidHandle));
    assert!(matches!(k.boolean(BoolOp::Union, b, &[a]), Err(KernelError::InvalidHandle)));
    assert_eq!(k.topology(ShapeHandle::from_parts(99, 0)), Err(KernelError::InvalidHandle));
    k.release(a); // stale release is a no-op
    assert_eq!(k.live_shapes(), 1);
}

#[test]
fn cancellation_stops_new_operations() {
    let mut k = OcctKernel::new();
    let token = CancelToken::new();
    k.set_cancel(token.clone());
    token.cancel();
    assert_eq!(k.make_box(&Frame::WORLD, Vec3::new(1.0, 1.0, 1.0)), Err(KernelError::Cancelled));
    token.reset();
    assert!(k.make_box(&Frame::WORLD, Vec3::new(1.0, 1.0, 1.0)).is_ok());
}

#[test]
fn kernel_moves_to_a_worker_thread() {
    let mut k: Box<dyn Kernel> = Box::new(OcctKernel::new());
    let b = plate(k.as_mut());
    let v = std::thread::spawn(move || volume(k.as_ref(), b)).join().unwrap();
    assert!(approx(v, 4000.0));
}
