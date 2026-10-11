//! Dimensions of one sketch that change together are written together: the sketch is solved once
//! at its new size. One at a time it would pass through sizes it cannot have (a triangle growing
//! from 3-4-5 to 30-40-50 would be 30-4-5 on the way) and fail though the result is sound.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use tenon_model::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.exec(id, &p, None).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, id: &str, p: Value) -> String {
    match s.exec(id, &p, None) {
        Err(e) => e.0,
        Ok(v) => panic!("{id} {p} was not refused: {v}"),
    }
}

/// A triangle with its three sides dimensioned 3, 4 and 5 (d0, d1, d2), one corner fixed and the
/// first side level. Returns the sketch.
fn triangle(s: &mut Session) -> u64 {
    let sk = run(s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let a = run(s, "sketch.line", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 3, "y2": 0 }));
    let b = run(s, "sketch.line", json!({ "sketch": sk, "start": a["end"], "x2": 3, "y2": 4 }));
    let c = run(s, "sketch.line", json!({ "sketch": sk, "start": b["end"], "end": a["start"] }));
    run(s, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "fix", "point": a["start"] } }));
    run(s, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "horizontal", "line": a["line"] } }));
    for (line, len) in [(&a, 3.0), (&b, 4.0), (&c, 5.0)] {
        run(s, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": line["line"], "value": len } }));
    }
    assert_eq!(run(s, "sketch.info", json!({ "sketch": sk }))["dof"], 0);
    sk
}

/// The lengths of the sketch's three lines, in order.
fn sides(s: &mut Session, sk: u64) -> Vec<f64> {
    let info = run(s, "sketch.info", json!({ "sketch": sk }));
    let at = |id: &Value| {
        let p = info["entities"].as_array().unwrap().iter().find(|e| e["id"] == *id).unwrap()["pos"].clone();
        (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap())
    };
    info["entities"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "line")
        .map(|l| {
            let ((x1, y1), (x2, y2)) = (at(&l["start"]), at(&l["end"]));
            (x2 - x1).hypot(y2 - y1)
        })
        .collect()
}

fn near(got: &[f64], want: [f64; 3]) -> bool {
    got.len() == 3 && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-7)
}

#[test]
fn one_parameter_scales_three_dimensions_of_a_sketch_at_once() {
    let mut s = Session::default();
    let sk = triangle(&mut s);
    run(&mut s, "param.add", json!({ "name": "k", "equation": "1", "unit": "ul" }));
    for (name, eq) in [("d0", "3 * k"), ("d1", "4 * k"), ("d2", "5 * k")] {
        run(&mut s, "param.set", json!({ "name": name, "equation": eq }));
    }
    // Ten times the size: 30 alone against 4 and 5 is no triangle, all three together are.
    run(&mut s, "param.set", json!({ "name": "k", "equation": "10" }));
    assert!(near(&sides(&mut s, sk), [30.0, 40.0, 50.0]), "{:?}", sides(&mut s, sk));
    // And back down, where 3 alone against 40 and 50 is none either.
    run(&mut s, "param.set", json!({ "name": "k", "equation": "1" }));
    assert!(near(&sides(&mut s, sk), [3.0, 4.0, 5.0]), "{:?}", sides(&mut s, sk));
    // Through a chain of parameters, in one step that undo takes back whole.
    run(&mut s, "param.add", json!({ "name": "scale", "equation": "2", "unit": "ul" }));
    run(&mut s, "param.set", json!({ "name": "k", "equation": "scale * 4" }));
    assert!(near(&sides(&mut s, sk), [24.0, 32.0, 40.0]), "{:?}", sides(&mut s, sk));
    run(&mut s, "edit.undo", json!({}));
    assert!(near(&sides(&mut s, sk), [3.0, 4.0, 5.0]));
    // Sizes that make no triangle are refused naming the dimensions, and nothing changes.
    run(&mut s, "param.set", json!({ "name": "d2", "equation": "5 * k ^ 3" }));
    let e = refused(&mut s, "param.set", json!({ "name": "k", "equation": "2" }));
    assert!(e.contains("d0 = 6") && e.contains("d1 = 8") && e.contains("d2 = 40"), "{e}");
    assert!(near(&sides(&mut s, sk), [3.0, 4.0, 5.0]), "{:?}", sides(&mut s, sk));
}

#[test]
fn a_design_table_row_sets_the_dimensions_of_a_sketch_at_once() {
    let mut s = Session::default();
    let sk = triangle(&mut s);
    run(&mut s, "table.create", json!({ "columns": ["d0", "d1", "d2"], "row": "small" }));
    run(&mut s, "table.add_row", json!({ "name": "large", "values": { "d0": 30, "d1": 40, "d2": 50 } }));
    run(&mut s, "table.add_row", json!({ "name": "flat", "values": { "d0": 1, "d1": 1, "d2": 50 } }));
    run(&mut s, "table.activate", json!({ "row": "large" }));
    assert!(near(&sides(&mut s, sk), [30.0, 40.0, 50.0]), "{:?}", sides(&mut s, sk));
    run(&mut s, "table.activate", json!({ "row": "small" }));
    assert!(near(&sides(&mut s, sk), [3.0, 4.0, 5.0]));
    // A row that is no triangle: refused, naming the three, and the part stays at its row.
    let e = refused(&mut s, "table.activate", json!({ "row": "flat" }));
    assert!(e.contains("d0 = 1, d1 = 1, d2 = 50"), "{e}");
    assert_eq!(run(&mut s, "table.show", json!({}))["table"]["active"], "small");
    assert!(near(&sides(&mut s, sk), [3.0, 4.0, 5.0]));
}
