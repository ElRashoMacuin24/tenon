//! Parameters and equations through commands: named dimensions and feature values, user
//! parameters, dependency order, cycles, renames and units.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::{Document, Session};

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let m = run(s, k, "model.mass", json!({}));
    m["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * (1.0 + b.abs())
}

fn param(s: &mut Session, k: &mut OcctKernel, name: &str) -> Value {
    let l = run(s, k, "param.list", json!({}));
    l["model"]
        .as_array()
        .unwrap()
        .iter()
        .chain(l["user"].as_array().unwrap())
        .find(|p| p["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("no {name}: {l}"))
}

/// A 40 x 20 x 10 block: the width dimension and the extrusion distance, with their names.
fn block(s: &mut Session, k: &mut OcctKernel) -> (u64, String, String) {
    let sk = run(s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    let lines = run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }))["lines"].clone();
    let w = run(s, k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": lines[0], "value": 40 } }));
    let width = w["name"].as_str().unwrap().to_owned();
    run(s, k, "model.extrude", json!({ "sketch": sk, "distance": 10 }));
    let l = run(s, k, "param.list", json!({}));
    let dist = l["model"].as_array().unwrap().iter().find(|p| p["of"].as_str().unwrap().contains("Extrusion1")).unwrap()["name"]
        .as_str()
        .unwrap()
        .to_owned();
    (sk, width, dist)
}

#[test]
fn dimensions_and_feature_values_are_named_and_driven_by_equations() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, width, dist) = block(&mut s, &mut k);
    assert_eq!((width.as_str(), dist.as_str()), ("d0", "d1"), "Inventor-style names in creation order");
    assert_eq!(param(&mut s, &mut k, "d0")["value"], 40.0);
    assert_eq!(param(&mut s, &mut k, "d1")["unit"], "mm");

    // A user parameter drives the width; the depth follows the width.
    run(&mut s, &mut k, "param.add", json!({ "name": "plate", "equation": "60 mm", "comment": "overall width" }));
    run(&mut s, &mut k, "param.set", json!({ "name": "d0", "equation": "plate" }));
    run(&mut s, &mut k, "param.set", json!({ "name": "d1", "equation": "d0 / 6" }));
    assert!(close(volume(&mut s, &mut k), 60.0 * 20.0 * 10.0), "{}", volume(&mut s, &mut k));
    run(&mut s, &mut k, "param.set", json!({ "name": "plate", "equation": "90" }));
    assert!(close(volume(&mut s, &mut k), 90.0 * 20.0 * 15.0), "{}", volume(&mut s, &mut k));
    assert_eq!(param(&mut s, &mut k, "d1")["equation"], "d0 / 6");

    // One undo step per change: undo brings back the 60 wide plate.
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(close(volume(&mut s, &mut k), 60.0 * 20.0 * 10.0));
    run(&mut s, &mut k, "edit.redo", json!({}));

    // Cycles and unknown names are refused and change nothing.
    let before = s.document().clone();
    let e = s.exec("param.set", &json!({ "name": "plate", "equation": "d1 * 2" }), Some(&mut k)).unwrap_err();
    assert!(e.0.contains("circle"), "{e:?}");
    let e = s.exec("param.set", &json!({ "name": "d1", "equation": "depth + 1" }), Some(&mut k)).unwrap_err();
    assert!(e.0.contains("no parameter `depth`"), "{e:?}");
    let e = s.exec("param.set", &json!({ "name": "d1", "equation": "d0 /" }), Some(&mut k)).unwrap_err();
    assert!(e.0.contains("d1"), "{e:?}");
    assert_eq!(s.document(), &before);

    // Renaming a parameter rewrites the equations that use it.
    run(&mut s, &mut k, "param.rename", json!({ "name": "d0", "to": "width" }));
    assert_eq!(param(&mut s, &mut k, "d1")["equation"], "width / 6");
    assert!(s.exec("param.rename", &json!({ "name": "width", "to": "plate" }), Some(&mut k)).is_err(), "taken");
    assert!(s.exec("param.rename", &json!({ "name": "width", "to": "2x" }), Some(&mut k)).is_err(), "not a name");

    // A user parameter in use cannot be deleted; a dimension set directly drops its equation.
    assert!(s.exec("param.delete", &json!({ "name": "plate" }), Some(&mut k)).unwrap_err().0.contains("used by width"));
    let t = param(&mut s, &mut k, "width")["target"].clone();
    let (sk, c) = (t["sketch"].as_u64().unwrap(), t["constraint"].as_u64().unwrap());
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": sk, "constraint": c, "value": 30 }));
    assert!(param(&mut s, &mut k, "width")["equation"].is_null());
    assert!(close(volume(&mut s, &mut k), 30.0 * 20.0 * 5.0), "the depth follows: {}", volume(&mut s, &mut k));
    run(&mut s, &mut k, "param.delete", json!({ "name": "plate" }));

    // The parameters survive a file round trip.
    let text = serde_json::to_string(s.document()).unwrap();
    let back: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(&back, s.document());
}

#[test]
fn equations_on_new_dimensions_angles_and_counts() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (sk, _, _) = block(&mut s, &mut k);
    // A new dimension given as an equation: the height of the rectangle is half its width.
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }));
    let right = info["entities"].as_array().unwrap().iter().filter(|e| e["type"] == "line").nth(1).unwrap()["id"].clone();
    let h = run(
        &mut s,
        &mut k,
        "sketch.constrain",
        json!({ "sketch": sk, "constraint": { "type": "length", "line": right, "value": 1 }, "equation": "d0 / 2" }),
    );
    assert_eq!(h["name"], "d2");
    assert!(close(volume(&mut s, &mut k), 40.0 * 20.0 * 10.0), "the 20 was already half of 40: {}", volume(&mut s, &mut k));
    run(&mut s, &mut k, "param.set", json!({ "name": "d0", "equation": "50" }));
    assert!(close(volume(&mut s, &mut k), 50.0 * 25.0 * 10.0), "{}", volume(&mut s, &mut k));

    // Angles are in degrees: a countersink angle of 45 * 2 is 90 degrees.
    let top = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": 2, "end": "end" } }));
    let sk2 = run(&mut s, &mut k, "sketch.create", json!({ "face": top }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.point", json!({ "sketch": sk2, "x": 10, "y": 10 }));
    let hole = run(
        &mut s,
        &mut k,
        "model.hole",
        json!({ "sketch": sk2, "diameter": 6, "through_all": true, "type": "countersink", "countersink_diameter": 10, "countersink_angle": 1.0 }),
    )["feature"]
        .as_u64()
        .unwrap();
    let l = run(&mut s, &mut k, "param.list", json!({}));
    let angle = l["model"].as_array().unwrap().iter().find(|p| p["of"].as_str().unwrap().contains("countersink angle")).unwrap().clone();
    assert_eq!(angle["unit"], "deg");
    run(&mut s, &mut k, "param.set", json!({ "name": angle["name"], "equation": "45 * 2" }));
    let kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(hole as u32)).unwrap().kind).unwrap();
    assert!(close(kind["kind"]["countersink"]["angle"].as_f64().unwrap(), PI / 2.0), "{kind}");

    // Counts must be whole numbers.
    let pat = run(&mut s, &mut k, "model.pattern.rect", json!({ "features": [hole], "direction": "x", "count": 2, "spacing": 15 }))["feature"]
        .as_u64()
        .unwrap();
    let l = run(&mut s, &mut k, "param.list", json!({}));
    let count = l["model"].as_array().unwrap().iter().find(|p| p["of"] == "Rectangular Pattern1 count1").unwrap()["name"].clone();
    run(&mut s, &mut k, "param.set", json!({ "name": count, "equation": "1 + 2" }));
    let kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(pat as u32)).unwrap().kind).unwrap();
    assert_eq!(kind["count1"], 3);
    assert!(s.exec("param.set", &json!({ "name": count, "equation": "2.5" }), Some(&mut k)).unwrap_err().0.contains("whole number"));

    // feature.update with equations drives a feature value in the same step.
    let mut kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(pat as u32)).unwrap().kind).unwrap();
    kind["spacing1"] = json!(12.0);
    run(&mut s, &mut k, "feature.update", json!({ "feature": pat, "kind": kind, "equations": { "/spacing1": "d0 / 5" } }));
    let kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(pat as u32)).unwrap().kind).unwrap();
    assert!(close(kind["spacing1"].as_f64().unwrap(), 10.0), "{kind}");
}

#[test]
fn files_without_parameters_get_names_when_opened() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let mut v = serde_json::to_value(s.document()).unwrap();
    v.as_object_mut().unwrap().remove("params");
    let old: Document = serde_json::from_value(v).unwrap();
    assert!(old.parameters().model.is_empty());
    let mut s2 = Session::default();
    s2.replace_document(old, None);
    assert_eq!(s2.document().parameters().model.len(), 2);
    assert!(!s2.is_dirty());
}
