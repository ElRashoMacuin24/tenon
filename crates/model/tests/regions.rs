//! Profiles from sketches whose curves cross: a line through a circle gives two halves, either of
//! which a feature can use; both together are the circle again. Volumes against hand-computed
//! ones, face names that later features can hold, an upstream edit, and what is said when a
//! region is gone.
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

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1.0)
}

fn failure(s: &mut Session, k: &mut OcctKernel) -> String {
    let r = run(s, k, "model.regenerate", json!({}));
    r["error"]["message"].as_str().unwrap_or_else(|| panic!("no failure: {r}")).to_owned()
}

/// The names of the part's faces (one body).
fn face_names(s: &mut Session, k: &mut OcctKernel) -> Vec<Value> {
    run(s, k, "model.faces", json!({ "body": 0 }))["faces"].as_array().unwrap().iter().map(|f| f["name"].clone()).collect()
}

/// The sketch's regions as `sketch.info` lists them.
fn regions(s: &mut Session, k: &mut OcctKernel, sketch: u64) -> Vec<Value> {
    run(s, k, "sketch.info", json!({ "sketch": sketch }))["regions"].as_array().unwrap().clone()
}

/// The region holding the sketch point (`x`, `y`): the one whose own inside point is nearest.
fn region_at(all: &[Value], x: f64, y: f64) -> Value {
    let d = |r: &Value| (r["inside"][0].as_f64().unwrap() - x).hypot(r["inside"][1].as_f64().unwrap() - y);
    all.iter().min_by(|a, b| d(a).total_cmp(&d(b))).unwrap().clone()
}

#[test]
fn a_circle_divided_by_a_line_is_used_whole_or_by_halves() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let c = run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 10 }))["circle"].as_u64().unwrap();
    let l = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": -15, "y1": 0, "x2": 15, "y2": 0 }))["line"].as_u64().unwrap();
    let radius = run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "radius", "curve": c, "value": 10.0 } }));

    // Two regions, each half the disc, told apart by the side of the line they are on.
    let all = regions(&mut s, &mut k, sk);
    assert_eq!(all.len(), 2, "{all:?}");
    let (upper, lower) = (region_at(&all, 0.0, 5.0), region_at(&all, 0.0, -5.0));
    assert_eq!(upper["select"], json!({ "left": [c, l] }));
    assert_eq!(lower["select"], json!({ "left": [c], "right": [l] }));

    // Nothing picked: the whole disc, as one cylinder whose wall is one face named after the
    // circle. The line through it leaves no trace.
    let whole = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5 }))["feature"].as_u64().unwrap();
    assert!(near(volume(&mut s, &mut k), 100.0 * PI * 5.0), "{}", volume(&mut s, &mut k));
    let names = face_names(&mut s, &mut k);
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(names.contains(&json!({ "type": "side", "feature": whole, "curve": c })));
    // Both halves picked are the same solid.
    run(&mut s, &mut k, "edit.undo", json!({}));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5, "regions": [upper["select"], lower["select"]] }));
    assert!(near(volume(&mut s, &mut k), 100.0 * PI * 5.0));
    assert_eq!(face_names(&mut s, &mut k).len(), 3);
    run(&mut s, &mut k, "edit.undo", json!({}));
    // So is the circle named alone: what it encloses, however it is divided.
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5, "regions": [[c]] }));
    assert!(near(volume(&mut s, &mut k), 100.0 * PI * 5.0));
    run(&mut s, &mut k, "edit.undo", json!({}));

    // One half: half the volume, with a round wall named after the circle and a flat one named
    // after the line.
    let half =
        run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5, "regions": [upper["select"]] }))["feature"].as_u64().unwrap();
    assert!(near(volume(&mut s, &mut k), 50.0 * PI * 5.0), "{}", volume(&mut s, &mut k));
    let names = face_names(&mut s, &mut k);
    assert_eq!(names.len(), 4, "{names:?}");
    let (round, flat) = (json!({ "type": "side", "feature": half, "curve": c }), json!({ "type": "side", "feature": half, "curve": l }));
    assert!(names.contains(&round) && names.contains(&flat), "{names:?}");
    // It is the upper half: its centre of mass is above the line, 4 r / 3 pi from it.
    let com = run(&mut s, &mut k, "model.mass", json!({}))["bodies"][0]["center_of_mass"].clone();
    assert!(near(com[1].as_f64().unwrap(), 40.0 / (3.0 * PI)) && com[0].as_f64().unwrap().abs() < 1e-9, "{com}");
    // A later feature holds the edge round the top of the round wall by the two faces' names.
    let edge = run(&mut s, &mut k, "model.edge_ref", json!({ "faces": [round, { "type": "cap", "feature": half, "end": "end" }] }));
    run(&mut s, &mut k, "model.fillet", json!({ "edges": [edge], "radius": 1 }));
    let filleted = volume(&mut s, &mut k);
    assert!(filleted < 50.0 * PI * 5.0 && filleted > 50.0 * PI * 5.0 - 10.0, "{filleted}");
    // The circle made larger: the same half is still the one used, and the fillet still holds
    // (its edge is found again by the faces it joins).
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": sk, "constraint": radius["constraint"], "value": 12.0 }));
    let larger = volume(&mut s, &mut k);
    assert!(larger < 72.0 * PI * 5.0 && larger > 72.0 * PI * 5.0 - 10.0, "{larger}");
    let com = run(&mut s, &mut k, "model.mass", json!({}))["bodies"][0]["center_of_mass"].clone();
    assert!(com[1].as_f64().unwrap() > 4.0, "{com}");

    // The other half joins it as a second feature: the disc again, in two heights.
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 2, "regions": [lower["select"]] }));
    assert!(volume(&mut s, &mut k) > larger + 72.0 * PI * 2.0 - 1e-6);
    run(&mut s, &mut k, "edit.undo", json!({}));

    // The line deleted: the half is no longer a region of the sketch, and the feature says so
    // rather than taking the whole circle.
    run(&mut s, &mut k, "sketch.delete", json!({ "sketch": sk, "entities": [l] }));
    assert_eq!(failure(&mut s, &mut k), "a selected profile region no longer exists in the sketch");
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(near(volume(&mut s, &mut k), larger));
    // A region that never was is refused the same way.
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5, "regions": [{ "left": [l], "right": [c] }], "operation": "new_body" }));
    assert_eq!(failure(&mut s, &mut k), "a selected profile region no longer exists in the sketch");
}

#[test]
fn a_plate_divided_by_a_line_keeps_its_plain_keys_and_its_holes() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let sides = run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }))["lines"].clone();
    let hole = run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 10, "cy": 10, "r": 3 }))["circle"].as_u64().unwrap();
    // A divider resting on the long sides (it shares no point with them).
    let divider = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": 20, "y1": 0, "x2": 20, "y2": 20 }))["line"].as_u64().unwrap();
    let all = regions(&mut s, &mut k, sk);
    assert_eq!(all.len(), 3, "the two halves and the disc in the hole: {all:?}");
    let (left, right) = (region_at(&all, 5.0, 3.0), region_at(&all, 30.0, 10.0));
    // Each half has a side the other has not: its curves name it, as a list.
    assert!(left["select"].is_array() && right["select"].is_array() && left["select"] != right["select"], "{left} {right}");
    assert_eq!(left["holes"], 1);

    // The four sides named as before this sketch was divided: the whole plate, with its hole,
    // as one block with six faces and the bore.
    let plate = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 4, "regions": [sides] }))["feature"].as_u64().unwrap();
    assert!(near(volume(&mut s, &mut k), (800.0 - 9.0 * PI) * 4.0), "{}", volume(&mut s, &mut k));
    let names = face_names(&mut s, &mut k);
    assert_eq!(names.len(), 7, "{names:?}");
    assert!(!names.contains(&json!({ "type": "side", "feature": plate, "curve": divider })));
    assert!(names.contains(&json!({ "type": "side", "feature": plate, "curve": hole })));
    run(&mut s, &mut k, "edit.undo", json!({}));
    // Nothing picked is the same.
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 4 }));
    assert!(near(volume(&mut s, &mut k), (800.0 - 9.0 * PI) * 4.0));
    assert_eq!(face_names(&mut s, &mut k).len(), 7);
    run(&mut s, &mut k, "edit.undo", json!({}));

    // The right half alone: 20 x 20, its left wall named after the divider.
    let half =
        run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 4, "regions": [right["select"]] }))["feature"].as_u64().unwrap();
    assert!(near(volume(&mut s, &mut k), 1600.0));
    let names = face_names(&mut s, &mut k);
    assert_eq!(names.len(), 6, "{names:?}");
    assert!(names.contains(&json!({ "type": "side", "feature": half, "curve": divider })));
    // The divider moved: the half follows.
    let end = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }))["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == divider)
        .map(|e| (e["start"].clone(), e["end"].clone()))
        .unwrap();
    for (point, y) in [(end.0, 0.0), (end.1, 20.0)] {
        run(&mut s, &mut k, "sketch.drag", json!({ "sketch": sk, "point": point, "x": 25.0, "y": y }));
    }
    assert!(near(volume(&mut s, &mut k), 15.0 * 20.0 * 4.0), "{}", volume(&mut s, &mut k));
    // The left half with its hole.
    run(&mut s, &mut k, "edit.undo", json!({}));
    run(&mut s, &mut k, "edit.undo", json!({}));
    run(&mut s, &mut k, "edit.undo", json!({}));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 4, "regions": [left["select"]] }));
    assert!(near(volume(&mut s, &mut k), (400.0 - 9.0 * PI) * 4.0), "{}", volume(&mut s, &mut k));
}
