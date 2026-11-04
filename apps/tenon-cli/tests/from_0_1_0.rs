//! What was made in 0.1.0 stays as it was made. The files here were written by 0.1.0's own
//! `tenon-cli` (`tests/fixtures/v0.1.0`), and the figures are what it says of them.
//!
//! The faces in question are flat faces on left-handed planes: what a Mirror feature, a chamfer or a
//! part-turn Revolve leaves. 0.1.0 said such a face looked into the material; it now looks out, as
//! every other does, and a reference made in 0.1.0 holds the way it looked then (DEC-140).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tenon_cli::Engine;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

fn engine(name: &str) -> Engine {
    let dir = std::env::temp_dir().join(format!("tenon-from-0.1.0-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Engine::new(Box::new(OcctKernel::new()) as Box<dyn Kernel>, dir)
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0.1.0")
}

fn run(e: &mut Engine, id: &str, p: Value) -> Value {
    e.exec(id, &p).unwrap_or_else(|err| panic!("{id}: {err}"))
}

fn open(e: &mut Engine, file: &str) {
    run(e, "file.open", json!({ "path": fixtures().join(file).to_string_lossy() }));
}

/// The one solid's volume and centre of mass.
fn mass(e: &mut Engine) -> (f64, [f64; 3]) {
    let m = run(e, "model.mass", json!({}));
    let b = &m["bodies"][0];
    let c = &b["center_of_mass"];
    (b["volume"].as_f64().unwrap(), [c[0].as_f64().unwrap(), c[1].as_f64().unwrap(), c[2].as_f64().unwrap()])
}

/// Each file, and what 0.1.0 itself says of it. (Holes and cuts on these faces vanished, and bosses
/// floated clear of the part, when the frame followed the new normal.)
const MADE_IN_0_1_0: &[(&str, f64, [f64; 3])] = &[
    // On a face a Mirror feature made.
    ("boss_reverse.tenon", 24062.8319, [-0.1110, 14.9817, 4.9948]),
    ("cut_forward.tenon", 23937.1681, [0.0984, 15.0184, 5.0052]),
    ("hole_reverse.tenon", 23919.5680, [0.1237, 15.0235, 5.0067]),
    ("pad_reverse.tenon", 25500.0000, [-2.5000, 15.0000, 5.0000]),
    ("pad_cut_forward.tenon", 22500.0000, [2.5000, 15.0000, 5.0000]),
    // Sketches dimensioned to the face's edges.
    ("con_cut_forward.tenon", 23937.1681, [0.0984, 15.0184, 5.0052]),
    ("con_hole_reverse.tenon", 23919.5680, [0.1237, 15.0235, 5.0067]),
    // A work plane offset from one, and a split by a work plane offset from one.
    ("workplane_offset.tenon", 24018.8496, [-0.0263, 14.9876, 5.0000]),
    ("split_front.tenon", 24000.0000, [0.0000, 15.0000, 5.0000]),
    // On a chamfer's face.
    ("ch_cut_forward.tenon", 23755.2876, [19.8079, 15.0029, 9.9112]),
    ("ch_boss_reverse.tenon", 23769.4248, [19.8193, 14.9941, 9.9158]),
    // On a flat side of a quarter revolve.
    ("rv_cut_forward.tenon", 1464.6571, [7.2091, 6.7044, 5.0603]),
    ("rv_hole_reverse.tenon", 1462.5335, [7.2094, 6.7039, 5.0601]),
];

#[test]
fn parts_made_in_0_1_0_come_to_what_they_came_to_there() {
    let mut e = engine("parts");
    for (file, volume, centre) in MADE_IN_0_1_0 {
        open(&mut e, file);
        let (v, c) = mass(&mut e);
        assert!((v - volume).abs() < 1e-3, "{file}: volume {v}, and 0.1.0 said {volume}");
        assert!((0..3).all(|i| (c[i] - centre[i]).abs() < 1e-3), "{file}: centre of mass {c:?}, and 0.1.0 said {centre:?}");
        let broken = run(&mut e, "model.broken", json!({}));
        assert_eq!(broken["broken"], json!([]), "{file}: {broken}");
    }
}

#[test]
fn a_work_plane_on_such_a_face_stays_on_the_side_it_was_on() {
    let mut e = engine("plane");
    open(&mut e, "workplane_offset.tenon");
    let work = run(&mut e, "model.work", json!({}));
    let origin = &work["work"][0]["origin"];
    assert!((origin[0].as_f64().unwrap() + 35.0).abs() < 1e-6, "offset 5 from the face at x = -40, into the block: {work}");
}

/// A mate made in 0.1.0 to a face a Mirror feature made: an Update leaves its component where it
/// stands. (With the frame turned, it went to the other side and turned about.)
#[test]
fn a_mate_made_in_0_1_0_on_such_a_face_holds_where_it_stood() {
    let mut e = engine("mate");
    run(&mut e, "asm.open", json!({ "path": fixtures().join("A.tenonasm").to_string_lossy() }));
    let at = |e: &mut Engine| {
        let tree = run(e, "asm.tree", json!({}));
        let q = tree["components"].as_array().unwrap().iter().find(|c| c["name"] == "Q:1").unwrap().clone();
        let o = &q["placement"]["origin"];
        [o[0].as_f64().unwrap(), o[1].as_f64().unwrap(), o[2].as_f64().unwrap()]
    };
    let before = at(&mut e);
    assert_eq!(before, [-30.0, 10.0, 0.0]);
    let solved = run(&mut e, "asm.update", json!({}));
    assert_eq!(solved["converged"], true, "{solved}");
    let after = at(&mut e);
    assert!((0..3).all(|i| (after[i] - before[i]).abs() < 1e-6), "it moved to {after:?}");
}

/// What is made now on the same face of the same file looks out of the material, beside what was
/// made in 0.1.0 and stays as it was.
#[test]
fn a_new_sketch_on_such_a_face_looks_out_of_the_material() {
    let mut e = engine("new");
    open(&mut e, "cut_forward.tenon");
    let (before, _) = mass(&mut e);
    let face = run(&mut e, "model.face_ref", json!({ "at": [-40, 15, 5] }));
    let sk = run(&mut e, "sketch.create", json!({ "face": face }))["feature"].clone();
    run(&mut e, "sketch.circle", json!({ "sketch": sk, "cx": -8, "cy": -3, "r": 2 }));
    run(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 5 }));
    let (after, c) = mass(&mut e);
    let boss = std::f64::consts::PI * 4.0 * 5.0;
    assert!((after - before - boss).abs() < 1e-3, "a boss standing on the part: {before} to {after}");
    assert!(c[0] < 0.0984, "it stands on the far side of the face: {c:?}");
    // And on a chamfer's face.
    open(&mut e, "ch_cut_forward.tenon");
    let (before, _) = mass(&mut e);
    let face = run(&mut e, "model.face_ref", json!({ "at": [38, 25, 18] }));
    let sk = run(&mut e, "sketch.create", json!({ "face": face }))["feature"].clone();
    run(&mut e, "sketch.circle", json!({ "sketch": sk, "cx": 1, "cy": 0, "r": 0.5 }));
    run(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 2 }));
    let (after, _) = mass(&mut e);
    assert!(after > before + 1.0, "it stands out of the block's corner, and is not lost inside it: {before} to {after}");
}

/// Only a left-handed plane is held to its reference: a sketch on the end of an extrusion follows
/// the end when the extrusion is flipped, as it always has.
#[test]
fn a_sketch_on_an_ordinary_face_still_turns_with_the_face() {
    let mut e = engine("ordinary");
    let sk = run(&mut e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(&mut e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 30 }));
    let block = run(&mut e, "model.extrude", json!({ "sketch": sk, "distance": 10 }))["feature"].clone();
    let face = run(&mut e, "model.face_ref", json!({ "at": [20, 15, 10] }));
    let on = run(&mut e, "sketch.create", json!({ "face": face }))["feature"].clone();
    run(&mut e, "sketch.circle", json!({ "sketch": on, "cx": 20, "cy": 15, "r": 2 }));
    let boss = std::f64::consts::PI * 4.0 * 5.0;
    run(&mut e, "model.extrude", json!({ "sketch": on, "distance": 5 }));
    let (v, c) = mass(&mut e);
    assert!((v - 12000.0 - boss).abs() < 1e-3 && c[2] > 5.0, "{v} {c:?}");
    let kind = json!({ "type": "extrude", "sketch": sk, "extent": { "distance": 10.0 }, "reverse": true });
    run(&mut e, "feature.update", json!({ "feature": block, "kind": kind }));
    let (v, c) = mass(&mut e);
    assert!((v - 12000.0 - boss).abs() < 1e-3, "{v}");
    assert!(c[2] < -5.0, "the boss stands on the end, now at z = -10, looking down: {c:?}");
}
