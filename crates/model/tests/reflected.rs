//! A part built as its mirror image (DEC-130): what an assembly's mirrored occurrence holds.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use tenon_geom::Vec3;
use tenon_kernel::{Mesh, MeshTol};
use tenon_kernel_occt::OcctKernel;
use tenon_model::{Document, RegenCache, Session, WorkGeom, regenerate, regenerate_with, scene};

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// A foot 40 x 20 x 10 with a 10 x 10 post on its corner at the origin up to 30 (10000 mm^3, its
/// centre of mass at (17, 9, 8)), a work plane at z = 12 and a work axis along X through (0, 5, 3).
fn chiral(k: &mut OcctKernel) -> Document {
    let mut s = Session::default();
    let sk = run(&mut s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(&mut s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }));
    run(&mut s, k, "model.extrude", json!({ "sketch": sk, "distance": 10 }));
    let sk = run(&mut s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(&mut s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 10, "y2": 10 }));
    run(&mut s, k, "model.extrude", json!({ "sketch": sk, "distance": 30, "operation": "join" }));
    run(&mut s, k, "work.plane", json!({ "base": "xy", "distance": 12 }));
    s.document().clone()
}

/// The volume a mesh encloses, as its windings say: negative for a mesh turned inside out.
fn enclosed(m: &Mesh) -> f64 {
    let at = |i: u32| {
        let p = m.positions[i as usize];
        Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
    };
    m.indices.as_chunks::<3>().0.iter().map(|t| at(t[0]).dot(at(t[1]).cross(at(t[2]))) / 6.0).sum()
}

#[test]
fn a_part_built_as_its_mirror_image_is_a_true_solid_with_its_faces_named_as_they_were() {
    let mut k = OcctKernel::new();
    let doc = chiral(&mut k);
    let plain = regenerate(&doc, &mut k);
    let plain_scene = scene(&plain, &mut k, &MeshTol::default()).unwrap();

    let mut mirrored = doc.clone();
    mirrored.set_reflected(true);
    let image = regenerate(&mirrored, &mut k);
    assert!(image.first_error().is_none(), "{:?}", image.first_error());
    assert_eq!(image.bodies.len(), 1);
    assert!(k_valid(&k, image.bodies[0].shape));
    let s = scene(&image, &mut k, &MeshTol::default()).unwrap();
    let b = &s.bodies[0];

    // By hand: the same volume; z the other way.
    assert!((b.volume - 10000.0).abs() < 1e-6, "{}", b.volume);
    assert!(b.mass.center_of_mass.near(Vec3::new(17.0, 9.0, -8.0), 1e-6), "{:?}", b.mass.center_of_mass);
    let bbox = b.bbox.unwrap();
    assert!(bbox.min.near(Vec3::new(0.0, 0.0, -30.0), 1e-6) && bbox.max.near(Vec3::new(40.0, 20.0, 0.0), 1e-6), "{bbox:?}");

    // Wound the right way: the mesh encloses the volume, not its negative, and each normal is on
    // the side its triangle's winding says. Reflected without care, both would be the other way.
    let m = &b.mesh;
    assert!((enclosed(m) - 10000.0).abs() < 1e-2, "{}", enclosed(m));
    for t in m.indices.as_chunks::<3>().0 {
        let at = |i: u32| {
            let p = m.positions[i as usize];
            Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
        };
        let w = (at(t[1]) - at(t[0])).cross(at(t[2]) - at(t[0]));
        for i in t {
            let n = m.normals[*i as usize];
            let n = Vec3::new(f64::from(n[0]), f64::from(n[1]), f64::from(n[2]));
            assert!(w.dot(n) > 0.0, "a normal against its triangle: {w:?} {n:?}");
        }
    }

    // Every face and edge is named as it was on the part, so a relationship or a dimension can
    // hold on to it, and each face is the image of the one of its name.
    assert_eq!(b.faces.len(), plain_scene.bodies[0].faces.len());
    for (name, info) in &b.faces {
        let name = name.expect("a named face");
        // (A side the foot cuts in two is two faces of one name.)
        let c = info.centroid;
        let image = |was: &tenon_kernel::FaceInfo| {
            let d = was.centroid;
            c.near(Vec3::new(d.x, d.y, -d.z), 1e-6) && (info.area - was.area).abs() < 1e-6
        };
        assert!(plain_scene.bodies[0].faces.iter().any(|(n, was)| *n == Some(name) && image(was)), "{name:?} at {c:?}");
    }
    assert_eq!(b.edges.iter().filter(|(n, _)| n.is_some()).count(), plain_scene.bodies[0].edges.iter().filter(|(n, _)| n.is_some()).count());

    // Its work plane with it.
    let WorkGeom::Plane(f) = s.work[0].1 else { panic!("{:?}", s.work) };
    assert!(f.origin().near(Vec3::new(0.0, 0.0, -12.0), 1e-9), "{f:?}");
    assert!(f.z().near(Vec3::Z, 1e-9) || f.z().near(-Vec3::Z, 1e-9), "{f:?}");

    // It is what the document says, in what is kept of a scene too; a part file never says it
    // otherwise, so what it has is as it was.
    let text = serde_json::to_value(&doc).unwrap();
    assert!(text.get("reflected").is_none(), "{text}");
    assert_eq!(serde_json::to_value(&mirrored).unwrap()["reflected"], true);
}

fn k_valid(k: &OcctKernel, s: tenon_kernel::ShapeHandle) -> bool {
    use tenon_kernel::Kernel;
    k.is_valid(s).unwrap()
}

#[test]
fn a_mirror_image_is_not_kept_in_a_checkpoint_of_the_part_it_is_built_from() {
    // Resuming from a checkpoint has to come to the same image, not reflect a reflection or take
    // up the part reflected.
    let mut k = OcctKernel::new();
    let mut doc = chiral(&mut k);
    doc.set_reflected(true);
    let mut cache = RegenCache::default();
    let first = regenerate_with(&doc, &mut k, Some(&mut cache));
    let v = |r: &tenon_model::Regen, k: &mut OcctKernel| {
        use tenon_kernel::Kernel;
        k.mass_properties(r.bodies[0].shape, 1.0).unwrap().center_of_mass
    };
    assert!(v(&first, &mut k).near(Vec3::new(17.0, 9.0, -8.0), 1e-6));
    let again = regenerate_with(&doc, &mut k, Some(&mut cache));
    assert!(cache.resumed_at > 0, "it took up from a checkpoint");
    assert!(v(&again, &mut k).near(Vec3::new(17.0, 9.0, -8.0), 1e-6), "{:?}", v(&again, &mut k));
    // And taken back it is the part again, from the same checkpoints.
    doc.set_reflected(false);
    let back = regenerate_with(&doc, &mut k, Some(&mut cache));
    assert!(v(&back, &mut k).near(Vec3::new(17.0, 9.0, 8.0), 1e-6), "{:?}", v(&back, &mut k));
}
