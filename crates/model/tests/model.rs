//! Part modelling through commands against the OpenCASCADE kernel.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;
use std::time::Duration;

use serde_json::{Value, json};
use tenon_geom::tol;
use tenon_kernel::{Kernel, MeshTol};
use tenon_kernel_occt::OcctKernel;
use tenon_model::worker::{Response, Worker};
use tenon_model::{Document, FeatureStatus, Session};

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let m = run(s, k, "model.mass", json!({}));
    m["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

fn approx(a: f64, b: f64) -> bool {
    tol::rel_eq(a, b, 1e-6)
}

/// Face index of the face named `name` (e.g. the end cap of a feature).
fn face_named(s: &mut Session, k: &mut OcctKernel, name: Value) -> u64 {
    let faces = run(s, k, "model.faces", json!({}));
    faces["faces"].as_array().unwrap().iter().find(|f| f["name"] == name).unwrap_or_else(|| panic!("no face named {name}"))["face"].as_u64().unwrap()
}

/// Plate 60 x 40 x `t` with two 8 mm holes cut through from a sketch on its top face.
fn bracket(s: &mut Session, k: &mut OcctKernel, t: f64) -> (u64, u64, u64) {
    let sk1 = run(s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(s, k, "sketch.rectangle", json!({ "sketch": sk1, "x1": 0, "y1": 0, "x2": 60, "y2": 40 }));
    let ex1 = run(s, k, "model.extrude", json!({ "sketch": sk1, "distance": t }))["feature"].as_u64().unwrap();
    let top = face_named(s, k, json!({ "type": "cap", "feature": ex1, "end": "end" }));
    let face = run(s, k, "model.face_ref", json!({ "face": top }));
    let sk2 = run(s, k, "sketch.create", json!({ "face": face }))["feature"].as_u64().unwrap();
    run(s, k, "sketch.circle", json!({ "sketch": sk2, "cx": 15, "cy": 20, "r": 4 }));
    run(s, k, "sketch.circle", json!({ "sketch": sk2, "cx": 45, "cy": 20, "r": 4 }));
    run(s, k, "model.extrude", json!({ "sketch": sk2, "through_all": true, "reverse": true, "operation": "cut" }));
    (sk1, ex1, sk2)
}

#[test]
fn bracket_with_holes_regenerates_after_an_upstream_edit() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, ex1, _) = bracket(&mut s, &mut k, 8.0);
    let r = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    assert!(approx(volume(&mut s, &mut k), 60.0 * 40.0 * 8.0 - 2.0 * PI * 16.0 * 8.0));

    // Make the plate thicker. The hole sketch follows the top face; the through-all cut follows too.
    let mut kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(ex1 as u32)).unwrap().kind).unwrap();
    kind["extent"] = json!({ "distance": 12.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": ex1, "kind": kind }));
    let r = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    assert!(approx(volume(&mut s, &mut k), 60.0 * 40.0 * 12.0 - 2.0 * PI * 16.0 * 12.0));
    let topo = run(&mut s, &mut k, "model.topology", json!({}));
    assert_eq!(topo["bodies"][0]["faces"], 8, "6 plate faces + 2 hole walls: {topo}");
    assert_eq!(topo["bodies"][0]["valid"], true);

    // Undo goes back to 8 mm, redo forward to 12 mm.
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(approx(volume(&mut s, &mut k), 60.0 * 40.0 * 8.0 - 2.0 * PI * 16.0 * 8.0));
    run(&mut s, &mut k, "edit.redo", json!({}));
    assert!(approx(volume(&mut s, &mut k), 60.0 * 40.0 * 12.0 - 2.0 * PI * 16.0 * 12.0));
}

#[test]
fn sketch_dimension_edit_moves_the_hole() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, _, sk2) = bracket(&mut s, &mut k, 8.0);
    // Dimension the first circle's radius and change it.
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk2 }));
    let circle = info["entities"].as_array().unwrap().iter().find(|e| e["type"] == "circle").unwrap()["id"].as_u64().unwrap();
    let c = run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk2, "constraint": { "type": "radius", "curve": circle, "value": 4.0 } }));
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": sk2, "constraint": c["constraint"], "value": 6.0 }));
    assert!(approx(volume(&mut s, &mut k), 60.0 * 40.0 * 8.0 - PI * 36.0 * 8.0 - PI * 16.0 * 8.0));
}

#[test]
fn a_lost_face_reference_breaks_the_feature_clearly() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk1 = run(&mut s, &mut k, "sketch.create", json!({}))["feature"].as_u64().unwrap();
    let lines = run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk1, "x1": 0, "y1": 0, "x2": 20, "y2": 10 }));
    let ex1 = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk1, "distance": 5 }))["feature"].as_u64().unwrap();
    // Sketch on the side face swept from the rectangle's bottom line.
    let bottom = lines["lines"][0].as_u64().unwrap();
    let side = face_named(&mut s, &mut k, json!({ "type": "side", "feature": ex1, "curve": bottom }));
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "face": side }));
    let sk2 = run(&mut s, &mut k, "sketch.create", json!({ "face": face }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk2, "cx": 10, "cy": 2.5, "r": 1 }));
    let ex2 = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk2, "distance": 3 }))["feature"].as_u64().unwrap();
    let ok = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert!(ok["error"].is_null(), "{ok}");

    // Replace the rectangle's bottom line: the side face it swept is gone.
    run(&mut s, &mut k, "sketch.delete", json!({ "sketch": sk1, "entities": [bottom] }));
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk1 }));
    let pts: Vec<u64> = info["entities"].as_array().unwrap().iter().filter(|e| e["type"] == "point").map(|e| e["id"].as_u64().unwrap()).collect();
    // Close the profile again with a new line between the two loose corners.
    let loose: Vec<u64> = pts
        .iter()
        .copied()
        .filter(|p| info["entities"].as_array().unwrap().iter().filter(|e| e["type"] == "line" && (e["start"] == *p || e["end"] == *p)).count() == 1)
        .collect();
    assert_eq!(loose.len(), 2, "{info}");
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk1, "start": loose[0], "end": loose[1] }));
    let r = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert_eq!(r["error"]["feature"], sk2, "{r}");
    assert!(r["error"]["message"].as_str().unwrap().contains("no longer exists"), "{r}");
    let status = |f: u64| r["features"].as_array().unwrap().iter().find(|x| x["id"] == f).unwrap()["status"]["state"].clone();
    assert_eq!(status(ex1), "ok");
    assert_eq!(status(ex2), "not_computed");
}

#[test]
fn revolve_about_an_origin_axis() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 10, "y1": 0, "x2": 20, "y2": 10 }));
    run(&mut s, &mut k, "model.revolve", json!({ "sketch": sk, "axis": "z" }));
    assert!(approx(volume(&mut s, &mut k), PI * 300.0 * 10.0));
    let topo = run(&mut s, &mut k, "model.topology", json!({}));
    let bb = &topo["bodies"][0]["bbox"];
    assert!((bb["max"][2].as_f64().unwrap() - 10.0).abs() < 1e-6 && (bb["max"][0].as_f64().unwrap() - 20.0).abs() < 1e-6, "{bb}");
}

#[test]
fn worker_regenerates_off_thread_and_reports_the_latest() {
    let w = Worker::spawn(|| Box::new(OcctKernel::new()) as Box<dyn Kernel>, MeshTol::default(), None).unwrap();
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    bracket(&mut s, &mut k, 8.0);
    let doc = s.document().clone();
    let mut thinner = doc.clone();
    // Two quick requests: only the newest answer matters.
    w.regenerate(1, Document::default(), false);
    w.regenerate(2, doc, true);
    let mut last = None;
    while let Some(r) = w.recv_timeout(Duration::from_secs(30)) {
        match r {
            Response::Scene { revision, scene } => {
                last = Some((revision, scene));
                if revision == 2 {
                    break;
                }
            }
            other => panic!("{other:?}"),
        }
    }
    let (rev, scene) = last.unwrap();
    assert_eq!(rev, 2);
    assert_eq!(scene.bodies.len(), 1);
    assert!(scene.bodies[0].mesh.triangle_count() > 50);
    assert!(scene.status.iter().all(|(_, st)| *st == FeatureStatus::Ok));
    assert!(approx(scene.bodies[0].volume, 60.0 * 40.0 * 8.0 - 2.0 * PI * 16.0 * 8.0));
    assert!(scene.bodies[0].faces.iter().any(|(name, _)| name.is_some()), "faces carry persistent names");

    thinner.name = "renamed".into();
    w.export_step(7, thinner);
    match w.recv_timeout(Duration::from_secs(30)) {
        Some(Response::Step { request: 7, result: Ok(bytes) }) => assert!(bytes.starts_with(b"ISO-10303-21;")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn bad_commands_change_nothing() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = run(&mut s, &mut k, "sketch.create", json!({}))["feature"].as_u64().unwrap();
    let before = s.document().clone();
    let rev = s.revision();
    let bad = [
        ("sketch.line", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 0, "y2": 0 })),
        ("sketch.line", json!({ "sketch": sk, "x1": "a", "y1": 0, "x2": 1, "y2": 0 })),
        ("sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 1e300 })),
        ("sketch.circle", json!({ "sketch": 999, "cx": 0, "cy": 0, "r": 1 })),
        ("sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "radius", "curve": 1 } })),
        ("sketch.create", json!({ "plane": "abc" })),
        ("model.extrude", json!({ "sketch": 999, "distance": 5 })),
        ("model.extrude", json!({ "sketch": sk })),
        ("feature.delete", json!({ "feature": 999 })),
        ("feature.update", json!({ "feature": sk, "kind": { "type": "extrude", "sketch": sk, "extent": { "distance": 1.0 } } })),
        ("sketch.delete", json!({ "sketch": sk, "entities": [42] })),
        ("edit.undo", json!({})),
    ];
    for (id, p) in bad {
        let undo_ok = id == "edit.undo";
        let r = s.exec(id, &p, Some(&mut k));
        if undo_ok {
            // Undo is allowed to succeed (the sketch creation is undoable); redo restores it.
            if r.is_ok() {
                s.exec("edit.redo", &json!({}), Some(&mut k)).unwrap();
            }
            continue;
        }
        assert!(r.is_err(), "{id} {p} should fail");
    }
    assert_eq!(s.document(), &before);
    assert!(s.revision() >= rev);
    assert!(s.exec("no.such.command", &json!({}), None).is_err());
    assert!(s.exec("model.mass", &json!({}), None).is_err(), "geometry commands need a kernel");
    assert!(s.exec("sketch.info", &json!([1, 2]), None).is_err(), "parameters must be an object");

    // A feature in use cannot be deleted.
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 1, "y2": 1 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 1 }));
    assert!(s.exec("feature.delete", &json!({ "feature": sk }), None).unwrap_err().0.contains("used by"));
}

#[test]
fn repeated_regeneration_does_not_leak_shapes() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    bracket(&mut s, &mut k, 8.0);
    run(&mut s, &mut k, "model.regenerate", json!({}));
    let live = k.live_shapes();
    for i in 0..5 {
        run(&mut s, &mut k, "document.rename", json!({ "name": format!("P{i}") }));
        run(&mut s, &mut k, "model.regenerate", json!({}));
    }
    assert_eq!(k.live_shapes(), live, "one body's worth of shapes stays alive");
}

#[test]
fn documents_round_trip_through_json() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    bracket(&mut s, &mut k, 8.0);
    let text = serde_json::to_string(s.document()).unwrap();
    let back: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(&back, s.document());
    back.validate().unwrap();
}
