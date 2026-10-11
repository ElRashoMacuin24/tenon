//! The sketch's Modify and Pattern tools and its further shapes, as commands: moved, copied,
//! turned and scaled geometry, extended and split curves, patterns, slots, rectangles about a
//! centre or at an angle, and tangent arcs. Each ends in a solid whose volume says the sketch
//! is what it should be.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> String {
    s.exec(id, &p, Some(k)).unwrap_err().to_string()
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1.0)
}

fn sketch(s: &mut Session, k: &mut OcctKernel) -> u64 {
    run(s, k, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap()
}

fn regions(s: &mut Session, k: &mut OcctKernel, sk: u64) -> Vec<f64> {
    run(s, k, "sketch.info", json!({ "sketch": sk }))["regions"].as_array().unwrap().iter().map(|r| r["area"].as_f64().unwrap()).collect()
}

#[test]
fn a_plate_of_holes_is_made_by_moving_copying_and_patterning() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = sketch(&mut s, &mut k);
    let plate = run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 40 }));
    let hole = run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 3 }))["circle"].clone();
    run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "diameter", "curve": hole, "value": 6.0 } }));
    // The hole is moved in from the corner, then repeated: three across 20 apart, in two rows.
    let moved = run(&mut s, &mut k, "sketch.move", json!({ "sketch": sk, "entities": [hole], "dx": 10, "dy": 10 }));
    assert_eq!(moved["entities"], json!([hole]));
    let copies = run(
        &mut s,
        &mut k,
        "sketch.pattern.rect",
        json!({ "sketch": sk, "entities": [hole], "along": plate["lines"][0], "count": 3, "spacing": 20, "count2": 2, "spacing2": 20 }),
    );
    assert_eq!(copies["entities"].as_array().unwrap().len(), 5);
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5 }));
    assert!(near(volume(&mut s, &mut k), (2400.0 - 6.0 * 9.0 * PI) * 5.0), "{}", volume(&mut s, &mut k));
    // One step back and the pattern is gone again.
    assert!(s.undo() && s.undo());
    assert_eq!(regions(&mut s, &mut k, sk).len(), 2);
    // A copy turned a quarter about the plate's middle, and one at half the size beside it.
    let turned =
        run(&mut s, &mut k, "sketch.rotate", json!({ "sketch": sk, "entities": [hole], "cx": 30, "cy": 20, "angle": PI / 2.0, "copy": true }));
    let small = run(&mut s, &mut k, "sketch.scale", json!({ "sketch": sk, "entities": [hole], "cx": 30, "cy": 10, "factor": 0.5, "copy": true }));
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }));
    let circle = |id: &Value| info["entities"].as_array().unwrap().iter().find(|e| e["id"] == *id).unwrap().clone();
    let centre = |id: &Value| {
        let c = circle(id)["center"].clone();
        let p = info["entities"].as_array().unwrap().iter().find(|e| e["id"] == c).unwrap()["pos"].clone();
        (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap())
    };
    let (tx, ty) = centre(&turned["entities"][0]);
    assert!(near(tx, 40.0) && near(ty, 0.0), "(10, 10) a quarter turn about (30, 20): {tx} {ty}");
    let (sx, sy) = centre(&small["entities"][0]);
    assert!(near(sx, 20.0) && near(sy, 10.0) && near(circle(&small["entities"][0])["radius"].as_f64().unwrap(), 1.5), "{sx} {sy}");
    // Five round the plate's middle over a whole turn.
    let ring =
        run(&mut s, &mut k, "sketch.pattern.circular", json!({ "sketch": sk, "entities": [small["entities"][0]], "cx": 30, "cy": 20, "count": 5 }));
    assert_eq!(ring["entities"].as_array().unwrap().len(), 4);
    // What cannot be done says why, and changes nothing.
    let before = serde_json::to_value(s.document()).unwrap();
    for (id, p, why) in [
        ("sketch.move", json!({ "sketch": sk, "entities": [], "dx": 1, "dy": 0 }), "nothing to move"),
        ("sketch.move", json!({ "sketch": sk, "entities": [9999], "dx": 1, "dy": 0 }), "9999"),
        ("sketch.scale", json!({ "sketch": sk, "entities": [hole], "cx": 0, "cy": 0, "factor": 0 }), "not a distance, an angle or a scale"),
        ("sketch.pattern.rect", json!({ "sketch": sk, "entities": [hole], "count": 3, "spacing": 5 }), "give the direction"),
        ("sketch.pattern.rect", json!({ "sketch": sk, "entities": [hole], "dx": 1, "dy": 0, "count": 1, "spacing": 5 }), "at least two"),
        (
            "sketch.pattern.rect",
            json!({ "sketch": sk, "entities": [hole], "dx": 1, "dy": 0, "count": 2, "spacing": 5, "count2": 2 }),
            "needs `spacing2`",
        ),
        ("sketch.pattern.circular", json!({ "sketch": sk, "entities": [hole], "cx": 0, "cy": 0, "count": 3, "angle": 9 }), "at most a whole turn"),
    ] {
        let e = refused(&mut s, &mut k, id, p.clone());
        assert!(e.contains(why), "{id} {p}: {e}");
    }
    assert_eq!(serde_json::to_value(s.document()).unwrap(), before);
}

#[test]
fn curves_are_extended_and_split_and_what_they_bound_is_extruded() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = sketch(&mut s, &mut k);
    // Three sides of a 30 x 20 box, open on the right, and a wall further right. The top and
    // bottom are extended to the wall: now it is a closed 40 x 20.
    let line = |s: &mut Session, k: &mut OcctKernel, p: Value| run(s, k, "sketch.line", p);
    let left = line(&mut s, &mut k, json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 0, "y2": 20 }));
    let top = line(&mut s, &mut k, json!({ "sketch": sk, "start": left["end"], "x2": 30, "y2": 20 }));
    let bottom = line(&mut s, &mut k, json!({ "sketch": sk, "start": left["start"], "x2": 30, "y2": 0 }));
    let wall = line(&mut s, &mut k, json!({ "sketch": sk, "x1": 40, "y1": 0, "x2": 40, "y2": 20 }));
    assert!(regions(&mut s, &mut k, sk).is_empty());
    run(&mut s, &mut k, "sketch.extend", json!({ "sketch": sk, "curve": top["line"], "x": 29, "y": 20 }));
    run(&mut s, &mut k, "sketch.extend", json!({ "sketch": sk, "curve": bottom["line"], "x": 29, "y": 0 }));
    let areas = regions(&mut s, &mut k, sk);
    assert!(areas.len() == 1 && near(areas[0], 800.0), "{areas:?}");
    // The wall is where they stopped: no further.
    let e = refused(&mut s, &mut k, "sketch.extend", json!({ "sketch": sk, "curve": top["line"], "x": 39, "y": 20 }));
    assert!(e.contains("joined to another curve") || e.contains("no curve beyond"), "{e}");
    // A line down the middle, and the top split where it crosses: three lines along the top
    // now... the split piece is its own curve, and the region is two.
    let mid = line(&mut s, &mut k, json!({ "sketch": sk, "x1": 15, "y1": -5, "x2": 15, "y2": 25 }));
    let rest = run(&mut s, &mut k, "sketch.split", json!({ "sketch": sk, "curve": top["line"], "x": 14, "y": 20 }));
    assert!(rest["curve"].as_u64().is_some());
    let areas = regions(&mut s, &mut k, sk);
    assert!(areas.len() == 2 && near(areas.iter().sum::<f64>(), 800.0), "{areas:?}");
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 2 }));
    assert!(near(volume(&mut s, &mut k), 1600.0), "{}", volume(&mut s, &mut k));
    let e = refused(&mut s, &mut k, "sketch.split", json!({ "sketch": sk, "curve": left["line"], "x": 0, "y": 10 }));
    assert!(e.contains("nothing crosses it"), "{e}");
    let _ = (wall, mid);
}

#[test]
fn slots_rectangles_and_tangent_arcs_are_profiles() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    // A slot three ways: by its centres, by its ends, and by its middle and one centre. Each is
    // 30 between centres and 8 across: 240 + 16 pi.
    let area = 240.0 + 16.0 * PI;
    for (by, x1, x2) in [("centers", 0.0, 30.0), ("overall", -4.0, 34.0), ("center", 15.0, 30.0)] {
        let sk = sketch(&mut s, &mut k);
        let slot = run(&mut s, &mut k, "sketch.slot", json!({ "sketch": sk, "x1": x1, "y1": 0, "x2": x2, "y2": 0, "width": 8, "by": by }));
        assert_eq!(
            (slot["lines"].as_array().unwrap().len(), slot["arcs"].as_array().unwrap().len(), slot["centers"].as_array().unwrap().len()),
            (2, 2, 2)
        );
        let x = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 3, "operation": "new_body" }));
        let bodies = run(&mut s, &mut k, "model.mass", json!({}))["bodies"].as_array().unwrap().clone();
        let v = bodies.last().unwrap()["volume"].as_f64().unwrap();
        assert!(near(v, area * 3.0), "{by}: {v} vs {}", area * 3.0);
        run(&mut s, &mut k, "feature.delete", json!({ "feature": x["feature"] }));
    }
    let sk = sketch(&mut s, &mut k);
    assert!(
        refused(&mut s, &mut k, "sketch.slot", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 5, "y2": 0, "width": 8, "by": "overall" }))
            .contains("longer than it is wide")
    );
    assert!(
        refused(&mut s, &mut k, "sketch.slot", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 5, "y2": 0, "width": 8, "by": "ends" }))
            .contains("centers, overall or center")
    );

    // A rectangle about its centre and one at an angle.
    let about = run(&mut s, &mut k, "sketch.rectangle_center", json!({ "sketch": sk, "cx": 0, "cy": 0, "x": 12, "y": 5 }));
    assert!(about["center"].as_u64().is_some());
    let areas = regions(&mut s, &mut k, sk);
    assert!(areas.len() == 1 && near(areas[0], 240.0), "{areas:?}");
    run(&mut s, &mut k, "sketch.rectangle3", json!({ "sketch": sk, "x1": 40, "y1": 0, "x2": 70, "y2": 40, "x3": 32, "y3": 6 }));
    let areas = regions(&mut s, &mut k, sk);
    assert!(areas.len() == 2 && areas.iter().any(|a| near(*a, 500.0)), "50 long and 10 deep: {areas:?}");

    // A line, a tangent arc on from its end, and a line back: half a stadium.
    let sk = sketch(&mut s, &mut k);
    let base = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 20, "y2": 0 }));
    let arc = run(&mut s, &mut k, "sketch.arc_tangent", json!({ "sketch": sk, "from": base["end"], "x": 20, "y": 10 }));
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }));
    let made = info["entities"].as_array().unwrap().iter().find(|e| e["id"] == arc["arc"]).unwrap().clone();
    assert_eq!(made["type"], "arc");
    assert!(info["constraints"].as_array().unwrap().iter().any(|c| c["type"] == "tangent"), "{info}");
    let e = refused(&mut s, &mut k, "sketch.arc_tangent", json!({ "sketch": sk, "from": base["start"], "x": -9, "y": 0 }));
    assert!(e.contains("that is a line"), "{e}");
}
