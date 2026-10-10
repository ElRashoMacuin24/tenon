//! M6 features through commands: sweep, coil and loft. Each: its volume against a hand-computed
//! one, its faces named so later features can hold them (persistent naming), an upstream edit it
//! follows, and a failure that says what to do.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

fn near(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs()
}

/// The failure message, when the part does not regenerate.
fn failure(s: &mut Session, k: &mut OcctKernel) -> String {
    let r = run(s, k, "model.regenerate", json!({}));
    r["error"]["message"].as_str().unwrap_or_else(|| panic!("no failure: {r}")).to_owned()
}

/// How many faces of the part carry the name `origin`.
fn faces_named(s: &mut Session, k: &mut OcctKernel, origin: &Value) -> usize {
    run(s, k, "model.faces", json!({}))["faces"].as_array().unwrap().iter().filter(|f| f["name"] == *origin).count()
}

/// Some face carries the name `origin`.
fn named(s: &mut Session, k: &mut OcctKernel, origin: Value) -> bool {
    faces_named(s, k, &origin) > 0
}

/// A circle of radius `r` at (`x`, `y`) of a new sketch on `plane`; returns (sketch, circle).
fn circle(s: &mut Session, k: &mut OcctKernel, plane: &str, x: f64, y: f64, r: f64) -> (u64, u64) {
    let sk = run(s, k, "sketch.create", json!({ "plane": plane, "project_origin": false }))["feature"].as_u64().unwrap();
    let c = run(s, k, "sketch.circle", json!({ "sketch": sk, "cx": x, "cy": y, "r": r }))["circle"].as_u64().unwrap();
    (sk, c)
}

#[test]
fn a_sweep_follows_its_path_and_names_its_faces() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (profile, ring) = circle(&mut s, &mut k, "xy", 0.0, 0.0, 2.0);
    // The path on the XZ plane: 20 up, a quarter turn of radius 10, then 20 along.
    let path = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz", "project_origin": false }))["feature"].clone();
    let up = run(&mut s, &mut k, "sketch.line", json!({ "sketch": path, "x1": 0, "y1": 0, "x2": 0, "y2": 20 }));
    let corner = run(&mut s, &mut k, "sketch.point", json!({ "sketch": path, "x": 10, "y": 30 }))["point"].clone();
    run(&mut s, &mut k, "sketch.arc", json!({ "sketch": path, "cx": 10, "cy": 20, "start": corner, "end": up["end"] }));
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": path, "start": corner, "x2": 30, "y2": 30 }));
    let sweep = run(&mut s, &mut k, "model.sweep", json!({ "sketch": profile, "path_sketch": path }))["feature"].as_u64().unwrap();
    // Pappus: the circle's centre runs along the path, so the volume is its area times the length.
    let length = 20.0 + 10.0 * PI / 2.0 + 20.0;
    assert!(near(volume(&mut s, &mut k), PI * 4.0 * length, 1e-4), "{}", volume(&mut s, &mut k));
    // The wall is named after the circle (three faces: straight, bent, straight; a reference picks
    // one by where it is), the ends after the sweep: later features can hold them.
    assert_eq!(faces_named(&mut s, &mut k, &json!({ "type": "side", "feature": sweep, "curve": ring })), 3);
    assert!(named(&mut s, &mut k, json!({ "type": "cap", "feature": sweep, "end": "start" })));
    assert!(named(&mut s, &mut k, json!({ "type": "cap", "feature": sweep, "end": "end" })));
    // The profile made smaller: the sweep follows, and its wall keeps its name.
    let r = run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": profile, "constraint": { "type": "radius", "curve": ring, "value": 2.0 } }));
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": profile, "constraint": r["constraint"], "value": 1.5 }));
    assert!(near(volume(&mut s, &mut k), PI * 2.25 * length, 1e-4));
    assert!(named(&mut s, &mut k, json!({ "type": "side", "feature": sweep, "curve": ring })));
    // A profile drawn in the path's own plane would sweep into a sheet: refused, saying why.
    let (flat, _) = circle(&mut s, &mut k, "xz", 0.0, 0.0, 1.0);
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": flat, "path_sketch": path, "operation": "new_body" }));
    assert!(failure(&mut s, &mut k).starts_with("The profile lies along the path"));
    run(&mut s, &mut k, "edit.undo", json!({}));
    // A path whose curves do not meet: refused when built, saying so.
    let gap = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz", "project_origin": false }))["feature"].clone();
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": gap, "x1": 0, "y1": 0, "x2": 0, "y2": 10 }));
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": gap, "x1": 5, "y1": 12, "x2": 5, "y2": 20 }));
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": profile, "path_sketch": gap, "operation": "new_body" }));
    assert!(failure(&mut s, &mut k).contains("do not join end to end"));
}

#[test]
fn a_coil_winds_round_its_axis_and_says_when_its_turns_collide() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    // A Ø2 circle 10 from the Z axis, in a plane through the axis.
    let (profile, ring) = circle(&mut s, &mut k, "xz", 10.0, 0.0, 1.0);
    let coil = run(&mut s, &mut k, "model.coil", json!({ "sketch": profile, "axis": "z", "pitch": 5, "turns": 3 }))["feature"].as_u64().unwrap();
    // The circle's area times the distance its centre goes round the axis.
    let per_turn = PI * 1.0 * 2.0 * PI * 10.0;
    assert!(near(volume(&mut s, &mut k), 3.0 * per_turn, 1e-3), "{}", volume(&mut s, &mut k));
    assert!(named(&mut s, &mut k, json!({ "type": "side", "feature": coil, "curve": ring })));
    assert!(named(&mut s, &mut k, json!({ "type": "cap", "feature": coil, "end": "end" })));
    // More turns, by its parameter; left-handed: the same volume.
    let mut kind = serde_json::to_value(&s.document().features().last().unwrap().kind).unwrap();
    kind["turns"] = json!(4.5);
    kind["left"] = json!(true);
    run(&mut s, &mut k, "feature.update", json!({ "feature": coil, "kind": kind }));
    assert!(near(volume(&mut s, &mut k), 4.5 * per_turn, 1e-3));
    // Turns closer than the profile is tall run into each other: said in plain words.
    kind["pitch"] = json!(1.0);
    run(&mut s, &mut k, "feature.update", json!({ "feature": coil, "kind": kind }));
    let m = failure(&mut s, &mut k);
    assert!(
        m.starts_with("The coil's turns run into each other: the pitch (1 mm) must be more than the profile's height along the axis (2 mm)."),
        "{m}"
    );
    // A profile on the axis cannot coil.
    let (on_axis, _) = circle(&mut s, &mut k, "xz", 0.0, 0.0, 1.0);
    let e = s.exec("model.coil", &json!({ "sketch": on_axis, "axis": "z", "pitch": 5, "turns": 0 }), Some(&mut k)).unwrap_err().0;
    assert!(e.contains("turns"), "{e}");
}

#[test]
fn a_loft_joins_its_sections_in_order() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let square = |s: &mut Session, k: &mut OcctKernel, sketch: &Value, h: f64| {
        run(s, k, "sketch.rectangle", json!({ "sketch": sketch, "x1": -h, "y1": -h, "x2": h, "y2": h }))["lines"].clone()
    };
    let bottom = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].clone();
    let lines = square(&mut s, &mut k, &bottom, 10.0);
    let up = run(&mut s, &mut k, "work.plane", json!({ "by": "offset", "base": "xy", "distance": 10 }))["feature"].clone();
    let top = run(&mut s, &mut k, "sketch.create", json!({ "work_plane": up, "project_origin": false }))["feature"].clone();
    square(&mut s, &mut k, &top, 5.0);
    let loft = run(&mut s, &mut k, "model.loft", json!({ "sections": [bottom, top], "ruled": true }))["feature"].as_u64().unwrap();
    // A ruled loft between squares of 20 and 10, 10 apart: a frustum.
    let frustum = 10.0 / 3.0 * (400.0 + 100.0 + 200.0);
    assert!(near(volume(&mut s, &mut k), frustum, 1e-9), "{}", volume(&mut s, &mut k));
    for line in lines.as_array().unwrap() {
        assert!(named(&mut s, &mut k, json!({ "type": "side", "feature": loft, "curve": line })), "each side from the first section");
    }
    assert!(named(&mut s, &mut k, json!({ "type": "cap", "feature": loft, "end": "start" })));
    // Smooth through a wider middle section: it bulges.
    let mid_plane = run(&mut s, &mut k, "work.plane", json!({ "by": "offset", "base": "xy", "distance": 5 }))["feature"].clone();
    let mid = run(&mut s, &mut k, "sketch.create", json!({ "work_plane": mid_plane, "project_origin": false }))["feature"].clone();
    square(&mut s, &mut k, &mid, 9.0);
    let mut kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(u32::try_from(loft).unwrap())).unwrap().kind).unwrap();
    kind["sections"] = json!([bottom, mid, top]);
    kind["ruled"] = json!(false);
    let e = s.exec("feature.update", &json!({ "feature": loft, "kind": kind }), Some(&mut k)).unwrap_err().0;
    assert!(e.contains("does not come before it"), "the middle sketch comes after the loft: {e}");
    run(&mut s, &mut k, "feature.move", json!({ "feature": loft }));
    run(&mut s, &mut k, "feature.update", json!({ "feature": loft, "kind": kind }));
    assert!(volume(&mut s, &mut k) > frustum);
    // A section without a closed profile: the part says which.
    let open = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz", "project_origin": false }))["feature"].clone();
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": open, "x1": 0, "y1": 0, "x2": 5, "y2": 5 }));
    run(&mut s, &mut k, "model.loft", json!({ "sections": [bottom, open], "operation": "new_body" }));
    let m = failure(&mut s, &mut k);
    assert!(m.contains("loft section 2") && m.contains("no closed profile"), "{m}");
}

/// The app regenerates on a worker thread, with cancellation: each new feature finishes there too.
#[test]
fn sweeps_coils_and_lofts_regenerate_on_the_worker() {
    use std::time::Duration;
    use tenon_kernel::Kernel;
    use tenon_model::worker::{Response, Worker};
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (coil, _) = circle(&mut s, &mut k, "xz", 10.0, 0.0, 1.0);
    run(&mut s, &mut k, "model.coil", json!({ "sketch": coil, "axis": "z", "pitch": 5, "turns": 3 }));
    let (profile, _) = circle(&mut s, &mut k, "xy", 30.0, 0.0, 2.0);
    let path = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz", "project_origin": false }))["feature"].clone();
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": path, "x1": 30, "y1": 0, "x2": 30, "y2": 20 }));
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": profile, "path_sketch": path, "operation": "new_body" }));
    let (low, _) = circle(&mut s, &mut k, "xy", -30.0, 0.0, 4.0);
    let up = run(&mut s, &mut k, "work.plane", json!({ "by": "offset", "base": "xy", "distance": 10 }))["feature"].clone();
    let high = run(&mut s, &mut k, "sketch.create", json!({ "work_plane": up, "project_origin": false }))["feature"].clone();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": high, "cx": -30, "cy": 0, "r": 2 }));
    run(&mut s, &mut k, "model.loft", json!({ "sections": [low, high], "operation": "new_body" }));
    let w = Worker::spawn(|| Box::new(OcctKernel::new()) as Box<dyn Kernel>, tenon_kernel::MeshTol::default(), None).unwrap();
    w.preview(1, s.document().clone());
    match w.recv_timeout(Duration::from_secs(30)) {
        Some(Response::Scene { revision: 1, scene, .. }) => {
            assert_eq!(scene.bodies.len(), 3, "{:?}", scene.status);
            assert!(scene.bodies.iter().all(|b| b.mesh.triangle_count() > 50));
        }
        other => panic!("{other:?}"),
    }
}
