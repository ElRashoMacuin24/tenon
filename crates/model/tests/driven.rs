//! Driven dimensions through commands: a dimension that follows its sketch instead of setting it.
//! It is taken where a driving one would be one too many, has a name equations can read, and
//! cannot itself be set.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> String {
    match s.exec(id, &p, Some(k)) {
        Err(e) => e.0,
        Ok(v) => panic!("{id} {p} was not refused: {v}"),
    }
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    run(s, k, "model.mass", json!({}))["bodies"][0]["volume"].as_f64().unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1.0)
}

fn param(s: &mut Session, k: &mut OcctKernel, name: &str) -> Value {
    let list = run(s, k, "param.list", json!({}));
    list["model"].as_array().unwrap().iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("no {name} in {list}")).clone()
}

#[test]
fn a_driven_dimension_is_taken_where_one_more_would_be_too_many_and_equations_read_it() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    // A 40 x 20 rectangle, fully held: a fixed corner, a width (d0) and a height (d1).
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let r = run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }));
    let (lines, corners) = (r["lines"].clone(), r["corners"].clone());
    run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "fix", "point": corners[0] } }));
    let w = run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": lines[0], "value": 40.0 } }));
    let h = run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": lines[1], "value": 20.0 } }));
    assert_eq!((w["name"].as_str(), h["name"].as_str(), w["driven"].as_bool()), (Some("d0"), Some("d1"), Some(false)));
    assert_eq!(run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }))["dof"], 0);

    // The opposite side's length would be one too many: refused, saying what to do instead.
    let top = json!({ "type": "length", "line": lines[2], "value": 40.0 });
    let m = refused(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk, "constraint": top }));
    assert!(m.starts_with("the sketch is already held here") && m.contains("driven dimension"), "{m}");
    // Asked for as driven it is taken, reads what the sketch measures and has a name of its own.
    let top = run(
        &mut s,
        &mut k,
        "sketch.constrain",
        json!({ "sketch": sk, "constraint": { "type": "length", "line": lines[2], "value": 7.0 }, "driven": true }),
    );
    assert_eq!((top["name"].as_str(), top["driven"].as_bool(), top["value"].as_f64()), (Some("d2"), Some(true), Some(40.0)));
    // "auto" makes a dimension driven only when it has to be: the diagonal here.
    let diagonal = json!({ "type": "distance", "a": corners[0], "b": corners[2], "value": 1.0 });
    let diag = run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk, "constraint": diagonal, "driven": "auto" }));
    assert_eq!((diag["name"].as_str(), diag["driven"].as_bool()), (Some("d3"), Some(true)));
    assert!(near(diag["value"].as_f64().unwrap(), 2000.0_f64.sqrt()));
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }));
    assert_eq!(info["dof"], 0);
    let marked: Vec<&Value> = info["constraints"].as_array().unwrap().iter().filter(|c| c["driven"] == true).collect();
    assert_eq!(marked.len(), 2, "{info}");
    let p = param(&mut s, &mut k, "d3");
    assert!(p["driven"] == true && p["equation"].is_null() && near(p["value"].as_f64().unwrap(), 2000.0_f64.sqrt()), "{p}");
    assert_eq!(param(&mut s, &mut k, "d0")["driven"], false);

    // It cannot be set: not as a dimension, not as a parameter, not by an equation, not by a table.
    let e = refused(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": sk, "constraint": diag["constraint"], "value": 50.0 }));
    assert!(e.contains("driven dimension") && e.contains("cannot be given a value"), "{e}");
    for equation in [json!(50), json!("d0 + 10")] {
        let e = refused(&mut s, &mut k, "param.set", json!({ "name": "d3", "equation": equation }));
        assert!(e.contains("driven dimension"), "{e}");
    }
    let e = refused(
        &mut s,
        &mut k,
        "sketch.constrain",
        json!({ "sketch": sk, "constraint": top["constraint"].clone(), "driven": true, "equation": "d0" }),
    );
    assert!(!e.is_empty());
    let e = refused(&mut s, &mut k, "table.create", json!({ "columns": ["d0", "d3"] }));
    assert!(e.contains("d3 is a driven dimension"), "{e}");

    // An equation reads it: the block is half its base's diagonal high, and follows the width.
    let ext = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5, "equations": { "/extent/distance": "d3 / 2" } }));
    assert!(near(volume(&mut s, &mut k), 800.0 * 2000.0_f64.sqrt() / 2.0), "{}", volume(&mut s, &mut k));
    run(&mut s, &mut k, "param.set", json!({ "name": "d0", "equation": 30 }));
    assert!(near(param(&mut s, &mut k, "d3")["value"].as_f64().unwrap(), 1300.0_f64.sqrt()));
    assert!(near(param(&mut s, &mut k, "d2")["value"].as_f64().unwrap(), 30.0));
    assert!(near(volume(&mut s, &mut k), 600.0 * 1300.0_f64.sqrt() / 2.0), "{}", volume(&mut s, &mut k));
    // Through a chain: a user parameter sets the width, and the height of the block follows the
    // diagonal that follows the width, all in the one edit.
    run(&mut s, &mut k, "param.add", json!({ "name": "W", "equation": "50 mm" }));
    run(&mut s, &mut k, "param.set", json!({ "name": "d0", "equation": "W" }));
    assert!(near(volume(&mut s, &mut k), 1000.0 * 2900.0_f64.sqrt() / 2.0), "{}", volume(&mut s, &mut k));
    run(&mut s, &mut k, "param.set", json!({ "name": "W", "equation": "30 mm" }));
    assert!(near(volume(&mut s, &mut k), 600.0 * 1300.0_f64.sqrt() / 2.0), "{}", volume(&mut s, &mut k));
    // But a dimension of the same sketch cannot follow it: the diagonal would chase itself.
    let e = refused(&mut s, &mut k, "param.set", json!({ "name": "d1", "equation": "d3 / 2" }));
    assert!(e.contains("`d3` is a driven dimension of a sketch this would change"), "{e}");
    assert!(near(volume(&mut s, &mut k), 600.0 * 1300.0_f64.sqrt() / 2.0), "nothing changed");

    // Driven and driving change places. The diagonal cannot drive while the height does.
    let e = refused(&mut s, &mut k, "sketch.set_driven", json!({ "sketch": sk, "constraint": diag["constraint"], "driven": false }));
    assert!(e.starts_with("the sketch is held without this dimension"), "{e}");
    run(&mut s, &mut k, "sketch.set_driven", json!({ "sketch": sk, "constraint": h["constraint"], "driven": true }));
    assert_eq!(param(&mut s, &mut k, "d1")["driven"], true);
    let now = run(&mut s, &mut k, "sketch.set_driven", json!({ "sketch": sk, "constraint": diag["constraint"], "driven": false }));
    assert_eq!(now["driven"], false);
    // A 30-40-50 triangle: the diagonal set to 50, the height reads 40.
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": sk, "constraint": diag["constraint"], "value": 50.0 }));
    assert!(near(param(&mut s, &mut k, "d1")["value"].as_f64().unwrap(), 40.0));
    assert!(near(volume(&mut s, &mut k), 30.0 * 40.0 * 25.0), "{}", volume(&mut s, &mut k));
    // Each step is undone as one.
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(near(param(&mut s, &mut k, "d1")["value"].as_f64().unwrap(), 20.0));
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert_eq!(param(&mut s, &mut k, "d3")["driven"], true);
    // A dimension with an equation, made driven, loses the equation (nothing sets it now).
    run(&mut s, &mut k, "sketch.set_driven", json!({ "sketch": sk, "constraint": w["constraint"], "driven": true }));
    let d0 = param(&mut s, &mut k, "d0");
    assert!(d0["driven"] == true && d0["equation"].is_null(), "{d0}");
    let _ = ext;
}
