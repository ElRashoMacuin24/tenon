//! Punch Tool: shapes from the library stamped into a flat face of the sheet, against volumes
//! worked out by hand. The sheet is 2 mm thick, bends have an inside radius of 3 mm and the K
//! factor is 0.44.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

const T: f64 = 2.0;
const R: f64 = 3.0;
const K: f64 = 0.44;

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> String {
    s.exec(id, &p, Some(k)).unwrap_err().to_string()
}

fn id(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or_else(|| panic!("no `{key}` in {v}"))
}

fn num(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or_else(|| panic!("no number `{key}` in {v}"))
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

fn failure(s: &mut Session, k: &mut OcctKernel) -> String {
    let r = run(s, k, "model.regenerate", json!({}));
    r["error"]["message"].as_str().unwrap_or_else(|| panic!("no feature failed: {r}")).to_owned()
}

fn flat(s: &mut Session, k: &mut OcctKernel) -> Value {
    run(s, k, "sheet.flat", json!({ "curves": true }))
}

fn start() -> (Session, OcctKernel) {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": T, "bend_radius": R }));
    (s, k)
}

/// A plate of `w` by `h` on the XY plane: its face feature.
fn plate(s: &mut Session, k: &mut OcctKernel, w: f64, h: f64) -> u64 {
    let sk = id(&run(s, k, "sketch.create", json!({ "plane": "xy" })), "feature");
    run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": w, "y2": h }));
    id(&run(s, k, "sheet.face", json!({ "sketch": sk })), "feature")
}

/// A sketch on the plate at height `T` (its top) with centre points at `at`.
fn points_on_top(s: &mut Session, k: &mut OcctKernel, at: &[(f64, f64)]) -> u64 {
    let wp = id(&run(s, k, "work.plane", json!({ "by": "offset", "base": "xy", "distance": T })), "feature");
    let sk = id(&run(s, k, "sketch.create", json!({ "work_plane": wp, "project_origin": false })), "feature");
    for (x, y) in at {
        run(s, k, "sketch.point", json!({ "sketch": sk, "x": x, "y": y }));
    }
    sk
}

/// The volume taken out by each shape of the library: its area by hand x the thickness.
#[test]
fn a_punch_takes_out_its_own_area_and_is_its_outline_in_the_flat_pattern() {
    let half = (4.0f64 / 6.0).acos();
    let d_area = 36.0 * PI - (36.0 * half - 4.0 * 20f64.sqrt());
    let cases: [(Value, f64, &str); 5] = [
        (json!({ "shape": "round", "diameter": 10 }), 25.0 * PI, "Round 10"),
        (json!({ "shape": "square", "side": 10 }), 100.0, "Square 10"),
        (json!({ "shape": "rectangle", "length": 20, "width": 10 }), 200.0, "Rectangle 20 x 10"),
        (json!({ "shape": "obround", "length": 24, "width": 8 }), 128.0 + 16.0 * PI, "Obround 24 x 8"),
        (json!({ "shape": "d_shape", "diameter": 12, "across": 10 }), d_area, "D-Shape 12 x 10"),
    ];
    for (shape, area, name) in cases {
        let (mut s, mut k) = start();
        plate(&mut s, &mut k, 100.0, 60.0);
        let sk = points_on_top(&mut s, &mut k, &[(30.0, 30.0), (70.0, 30.0)]);
        let mut p = shape.clone();
        p["sketch"] = json!(sk);
        p["rotation"] = json!(0.5);
        run(&mut s, &mut k, "sheet.punch", p);
        let v = volume(&mut s, &mut k);
        assert!(close(v, (6000.0 - 2.0 * area) * T, 1e-5), "{shape}: {v} {}", (6000.0 - 2.0 * area) * T);
        let f = flat(&mut s, &mut k);
        assert_eq!(f["holes"], 2, "{f}");
        assert!(close(num(&f, "area"), 6000.0 - 2.0 * area, 1e-5), "{f}");
        let marks = f["marks"].as_array().unwrap_or_else(|| panic!("no marks: {f}"));
        assert_eq!(marks.len(), 2, "{f}");
        for (m, x) in marks.iter().zip([30.0, 70.0]) {
            assert_eq!((m["name"].as_str().unwrap(), m["formed"].as_bool().unwrap()), (name, false));
            // At the sketch's points: 30 up, and 30 or 70 along.
            let (cx, cy) = (m["center"][0].as_f64().unwrap(), m["center"][1].as_f64().unwrap());
            assert!(close(cx, x, 1e-5) && close(cy, 30.0, 1e-5), "{shape}: {m}");
        }
    }
}

#[test]
fn a_rotation_turns_a_stamp_about_its_point() {
    let (mut s, mut k) = start();
    plate(&mut s, &mut k, 100.0, 60.0);
    let sk = points_on_top(&mut s, &mut k, &[(50.0, 30.0)]);
    // A slit 40 by 2, turned a quarter of the way round.
    run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk, "shape": "rectangle", "length": 40, "width": 2, "rotation": FRAC_PI_2 }));
    let f = flat(&mut s, &mut k);
    let hole = &f["inner"][0];
    // Its corners, as the flat has them: along Y now.
    let ys: Vec<f64> = hole.as_array().unwrap().iter().map(|c| c["start"]["y"].as_f64().unwrap()).collect();
    let (lo, hi) = ys.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| (l.min(*v), h.max(*v)));
    assert!(close(lo, 10.0, 1e-6) && close(hi, 50.0, 1e-6), "{hole}");
}

#[test]
fn a_dimple_adds_what_its_dome_is_more_than_the_flat_by_hand_and_is_a_mark_in_the_flat_pattern() {
    let (mut s, mut k) = start();
    plate(&mut s, &mut k, 100.0, 60.0);
    let sk = points_on_top(&mut s, &mut k, &[(50.0, 30.0)]);
    let f = id(&run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk, "shape": "dimple", "diameter": 20, "depth": 3 })), "feature");
    // The dome between two spheres about one centre, standing on a plate, adds pi H^2 t to it:
    // worked out from the two caps (docs/decisions.md, DEC-138).
    let v = volume(&mut s, &mut k);
    assert!(close(v, 6000.0 * T + PI * 9.0 * T, 1e-5), "{v} {}", 6000.0 * T + PI * 9.0 * T);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    // Its top stands 3 above the plate's top; nothing of it goes below the plate's underside.
    let b = run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0]["bbox"].clone();
    let z = |key: &str| b[key][2].as_f64().unwrap();
    assert!(close(z("max"), T + 3.0, 1e-5) && close(z("min"), 0.0, 1e-5), "{b}");
    // Flat: no hole, the whole plate, a centre mark and a bend line round its rim, and its name.
    let p = flat(&mut s, &mut k);
    assert_eq!(p["holes"], 0, "{p}");
    assert!(close(num(&p, "area"), 6000.0, 1e-6), "{p}");
    let marks = p["marks"].as_array().unwrap();
    assert_eq!(marks.len(), 1, "{p}");
    let m = &marks[0];
    assert_eq!((m["name"].as_str().unwrap(), m["formed"].as_bool().unwrap()), ("Dimple 20 x 3", true));
    assert!(close(m["center"][0].as_f64().unwrap(), 50.0, 1e-5) && close(m["center"][1].as_f64().unwrap(), 30.0, 1e-5), "{m}");
    assert!(m["lines"].as_array().unwrap().len() > 8, "a bend line round the rim: {m}");
    // Stamped out of the other side instead: it stands 3 under the underside.
    let mut kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(f as u32)).unwrap().kind).unwrap();
    kind["flip"] = json!(true);
    run(&mut s, &mut k, "feature.update", json!({ "feature": f, "kind": kind }));
    assert!(close(volume(&mut s, &mut k), 6000.0 * T + PI * 9.0 * T, 1e-5));
    let b = run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0]["bbox"].clone();
    let z = |key: &str| b[key][2].as_f64().unwrap();
    assert!(close(z("min"), -3.0, 1e-5) && close(z("max"), T, 1e-5), "{b}");
}

#[test]
fn a_louver_is_a_flap_bent_up_along_its_hinge_by_hand_and_a_mark_and_a_bend_line_in_the_flat_pattern() {
    let (mut s, mut k) = start();
    plate(&mut s, &mut k, 100.0, 60.0);
    let sk = points_on_top(&mut s, &mut k, &[(50.0, 30.0)]);
    run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk, "shape": "louver", "length": 30, "width": 15, "height": 5 }));
    // The flap is 15 long laid flat and bends by a turn with allowance turn x (3 + 0.88), then
    // straight; its volume is its middle's length x 2 x 30; the opening took 15 x 30 x 2.
    let (turn, straight) = tenon_model::sheet::punch::louver_flap(15.0, 5.0, T, R, K).unwrap();
    assert!(close(straight + turn * (R + K * T), 15.0, 1e-9));
    let flap = 30.0 * T * (straight + turn * (R + T / 2.0));
    let v = volume(&mut s, &mut k);
    assert!(close(v, 6000.0 * T - 450.0 * T + flap, 1e-5), "{v} {}", 6000.0 * T - 450.0 * T + flap);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    // The lip's top is 5 above the plate's top.
    let b = run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0]["bbox"].clone();
    assert!(close(b["max"][2].as_f64().unwrap(), T + 5.0, 1e-5), "{b}");
    // Flat: the whole plate, a mark at its middle, and a bend line along its hinge, 30 long.
    let p = flat(&mut s, &mut k);
    assert_eq!(p["holes"], 0, "{p}");
    assert!(close(num(&p, "area"), 6000.0, 1e-6), "{p}");
    let m = &p["marks"][0];
    assert_eq!(m["name"].as_str().unwrap(), "Louver 30 x 15 x 5");
    assert!(close(m["center"][0].as_f64().unwrap(), 50.0, 1e-5) && close(m["center"][1].as_f64().unwrap(), 30.0, 1e-5), "{m}");
    let lines = m["lines"].as_array().unwrap();
    let long: f64 = lines
        .iter()
        .map(|l| {
            let (a, b) = (&l[0], &l[1]);
            ((a[0].as_f64().unwrap() - b[0].as_f64().unwrap()).powi(2) + (a[1].as_f64().unwrap() - b[1].as_f64().unwrap()).powi(2)).sqrt()
        })
        .sum();
    assert!(close(long, 30.0, 1e-5), "{m}: {long}");
    // No bend of the flat pattern for it: the press makes that in one stroke.
    assert_eq!(p["bends"].as_array().unwrap().len(), 0, "{p}");
    // Too high for what it has to bend with: said.
    let (mut s, mut k) = start();
    plate(&mut s, &mut k, 100.0, 60.0);
    let sk = points_on_top(&mut s, &mut k, &[(50.0, 30.0)]);
    run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk, "shape": "louver", "length": 30, "width": 15, "height": 50 }));
    assert!(failure(&mut s, &mut k).contains("stands at most"));
}

#[test]
fn a_punch_stays_on_one_flat_face_and_says_so() {
    let (mut s, mut k) = start();
    plate(&mut s, &mut k, 100.0, 60.0);
    // Too near the edge: over it.
    let sk = points_on_top(&mut s, &mut k, &[(97.0, 30.0)]);
    run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk, "shape": "round", "diameter": 10 }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("goes over the edge of its face"), "{e}");
    run(&mut s, &mut k, "edit.undo", json!({}));
    // A sketch with no centre points.
    let sk2 = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy", "project_origin": false })), "feature");
    let e = refused(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk2 }));
    assert!(e.contains("no centre points"), "{e}");
    // Off the sheet altogether.
    run(&mut s, &mut k, "sketch.point", json!({ "sketch": sk2, "x": 300, "y": 300 }));
    run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk2, "shape": "round", "diameter": 10 }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("not on a flat side of the sheet"), "{e}");
    run(&mut s, &mut k, "edit.undo", json!({}));
    // A size this shape has not got; a shape there is not.
    let e = refused(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk2, "shape": "round", "side": 10 }));
    assert!(e.contains("has no `side`"), "{e}");
    let e = refused(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk2, "shape": "star" }));
    assert!(e.contains("round | square") && e.contains("star"), "{e}");
    // A sketch that is not a sheet's own plane: the flat side must be in the sketch's plane.
    let up = id(&run(&mut s, &mut k, "work.plane", json!({ "by": "offset", "base": "xy", "distance": 30 })), "feature");
    let above = id(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": up, "project_origin": false })), "feature");
    run(&mut s, &mut k, "sketch.point", json!({ "sketch": above, "x": 50, "y": 30 }));
    run(&mut s, &mut k, "sheet.punch", json!({ "sketch": above }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("not on a flat side of the sheet"), "{e}");
}

/// A plate with a flange: a punch across the bend is refused in words, and clear of it goes in.
#[test]
fn a_punch_across_a_bend_is_refused_in_words() {
    let (mut s, mut k) = start();
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    let r = run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 100, "y2": 60 }));
    let face = id(&run(&mut s, &mut k, "sheet.face", json!({ "sketch": sk })), "feature");
    let right = r["lines"][1].as_u64().unwrap();
    let edge = run(
        &mut s,
        &mut k,
        "model.edge_ref",
        json!({ "faces": [{ "type": "cap", "feature": face, "end": "end" }, { "type": "side", "feature": face, "curve": right }] }),
    );
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "angle": FRAC_PI_2, "height": 30, "position": "inside" }));
    let before = volume(&mut s, &mut k);
    // The flat of the top runs to x = 95: a stamp of radius 5 at 92 runs over onto the bend.
    let up = points_on_top(&mut s, &mut k, &[(92.0, 30.0)]);
    run(&mut s, &mut k, "sheet.punch", json!({ "sketch": up, "shape": "round", "diameter": 10 }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("goes across a bend") && e.contains("Cut Across Bend"), "{e}");
    run(&mut s, &mut k, "edit.undo", json!({}));
    let clear = points_on_top(&mut s, &mut k, &[(50.0, 30.0)]);
    run(&mut s, &mut k, "sheet.punch", json!({ "sketch": clear, "shape": "round", "diameter": 10 }));
    assert!(close(volume(&mut s, &mut k), before - 25.0 * PI * T, 1e-5));
    let p = flat(&mut s, &mut k);
    assert_eq!(p["holes"], 1, "{p}");
    assert_eq!(p["marks"].as_array().unwrap().len(), 1, "{p}");
}

/// A sketch's points moved, a stamp goes with them; taken back, it comes back.
#[test]
fn a_punch_follows_its_sketch_and_is_taken_back_and_suppressed() {
    let (mut s, mut k) = start();
    plate(&mut s, &mut k, 100.0, 60.0);
    let sk = points_on_top(&mut s, &mut k, &[(30.0, 30.0)]);
    let f = id(&run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk, "diameter": 10 })), "feature");
    let cut = 6000.0 * T - 25.0 * PI * T;
    assert!(close(volume(&mut s, &mut k), cut, 1e-5));
    // Another point in the sketch: another stamp, when none are named.
    run(&mut s, &mut k, "sketch.point", json!({ "sketch": sk, "x": 70, "y": 30 }));
    assert!(close(volume(&mut s, &mut k), 6000.0 * T - 50.0 * PI * T, 1e-5));
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": f, "suppressed": true }));
    assert!(close(volume(&mut s, &mut k), 6000.0 * T, 1e-5));
    let kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(f as u32)).unwrap().kind).unwrap();
    assert_eq!(kind, json!({ "type": "punch", "sketch": sk, "shape": { "round": { "diameter": 10.0 } } }), "what a file says of it");
}

/// A dome or a flap over a hole would fill the hole in without a word. That is refused, whether the
/// hole is in the middle of it or off to one side, and the part stays as it was. A cut may take a
/// hole with it: what goes is its own area and what was left of the hole, by hand.
#[test]
fn a_formed_stamp_will_not_close_a_hole_and_a_cut_may_take_one() {
    let with_a_hole = |at: (f64, f64)| {
        let (mut s, mut k) = start();
        plate(&mut s, &mut k, 200.0, 100.0);
        let sk = points_on_top(&mut s, &mut k, &[at]);
        run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk, "shape": "round", "diameter": 4 }));
        (s, k)
    };
    let holed = 40_000.0 - PI * 4.0 * T;
    for (shape, at) in [
        (json!({ "shape": "dimple", "diameter": 30, "depth": 3 }), (50.0, 50.001)),
        (json!({ "shape": "dimple", "diameter": 30, "depth": 3 }), (58.0, 50.0)),
        (json!({ "shape": "louver", "length": 30, "width": 15, "height": 5 }), (50.0, 50.0)),
        // (A hole that lies across the rim of the stamp.)
        (json!({ "shape": "louver", "length": 30, "width": 15, "height": 5 }), (65.0, 50.0)),
    ] {
        let (mut s, mut k) = with_a_hole((50.0, 50.0));
        assert!(close(volume(&mut s, &mut k), holed, 1e-5));
        let sk = points_on_top(&mut s, &mut k, &[at]);
        let mut p = shape.clone();
        p["sketch"] = json!(sk);
        run(&mut s, &mut k, "sheet.punch", p);
        let e = failure(&mut s, &mut k);
        assert!((e.contains("would close the hole") && e.contains("Move it clear")) || e.contains("clear of its edges and holes"), "{shape}: {e}");
        let left = run(&mut s, &mut k, "model.mass", json!({}));
        assert!(close(left["bodies"][0]["volume"].as_f64().unwrap(), holed, 1e-5), "{shape}: {left}");
    }
    // Clear of it, it goes on: the dome's rim is sqrt(15^2 + 2 x 2 x 3) = 15.39 from its point.
    let (mut s, mut k) = with_a_hole((50.0, 50.0));
    let sk = points_on_top(&mut s, &mut k, &[(50.0 + 2.0 + 15.5, 50.0)]);
    run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk, "shape": "dimple", "diameter": 30, "depth": 3 }));
    assert!(close(volume(&mut s, &mut k), holed + PI * 9.0 * T, 1e-5), "{}", volume(&mut s, &mut k));
    // A round cut that takes the hole: its own area, all of the hole inside it.
    let (mut s, mut k) = with_a_hole((55.0, 50.0));
    let sk = points_on_top(&mut s, &mut k, &[(55.0, 50.0)]);
    run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk, "shape": "round", "diameter": 30 }));
    assert!(close(volume(&mut s, &mut k), 40_000.0 - PI * 225.0 * T, 1e-5), "{}", volume(&mut s, &mut k));
    let p = flat(&mut s, &mut k);
    assert_eq!(p["holes"], 1, "{p}");
    assert!(close(num(&p, "area"), 20_000.0 - PI * 225.0, 1e-5), "{p}");
}

/// A formed stamp is a mark in the flat pattern, so a cut through it has no outline there: that
/// flat pattern is refused, not drawn without the hole. A cut clear of it is in the pattern.
#[test]
fn a_cut_through_a_formed_stamp_is_refused_in_the_flat_pattern_and_one_clear_of_it_is_there() {
    let cut_at = |s: &mut Session, k: &mut OcctKernel, x: f64, y: f64, r: f64| {
        let c = id(&run(s, k, "sketch.create", json!({ "plane": "xy" })), "feature");
        run(s, k, "sketch.circle", json!({ "sketch": c, "cx": x, "cy": y, "r": r }));
        run(s, k, "model.extrude", json!({ "sketch": c, "distance": 50, "operation": "cut" }));
    };
    let stamped = |shape: Value| {
        let (mut s, mut k) = start();
        plate(&mut s, &mut k, 200.0, 100.0);
        let sk = points_on_top(&mut s, &mut k, &[(50.0, 50.0)]);
        let mut p = shape;
        p["sketch"] = json!(sk);
        run(&mut s, &mut k, "sheet.punch", p);
        assert!(run(&mut s, &mut k, "model.regenerate", json!({}))["error"].is_null());
        (s, k)
    };
    let dimple = json!({ "shape": "dimple", "diameter": 40, "depth": 8 });
    let louver = json!({ "shape": "louver", "length": 30, "width": 15, "height": 5 });
    // Through the top of the dome; through its rim; through the flap.
    for (shape, x, y, name) in [
        (dimple.clone(), 50.0, 50.0, "Dimple 40 x 8"),
        (dimple.clone(), 70.0, 50.0, "Dimple 40 x 8"),
        (louver.clone(), 52.0, 50.0, "Louver 30 x 15 x 5"),
    ] {
        let (mut s, mut k) = stamped(shape);
        let before = volume(&mut s, &mut k);
        cut_at(&mut s, &mut k, x, y, 5.0);
        assert!(volume(&mut s, &mut k) < before - 1.0, "the cut took something");
        let e = refused(&mut s, &mut k, "sheet.flat", json!({}));
        assert!(e.contains(name) && e.contains("cut since it was stamped"), "{e}");
    }
    // Clear of it: its hole is in the pattern, and the mark is still its mark.
    let (mut s, mut k) = stamped(dimple);
    cut_at(&mut s, &mut k, 150.0, 50.0, 5.0);
    assert!(close(volume(&mut s, &mut k), 40_000.0 + PI * 64.0 * T - PI * 25.0 * T, 1e-4));
    let p = flat(&mut s, &mut k);
    assert_eq!(p["holes"], 1, "{p}");
    assert!(close(num(&p, "area"), 20_000.0 - PI * 25.0, 1e-5), "{p}");
    assert_eq!(p["marks"].as_array().unwrap().len(), 1, "{p}");
}

/// Stamps of one punch that overlap are each marked at their own point, however little is left of
/// the outline of each.
#[test]
fn stamps_that_overlap_are_marked_at_their_own_points() {
    let centres = |shape: Value, points: &[(f64, f64)]| {
        let (mut s, mut k) = start();
        plate(&mut s, &mut k, 200.0, 100.0);
        let sk = points_on_top(&mut s, &mut k, points);
        let mut p = shape;
        p["sketch"] = json!(sk);
        run(&mut s, &mut k, "sheet.punch", p);
        let v = volume(&mut s, &mut k);
        let f = flat(&mut s, &mut k);
        let marks: Vec<(f64, f64)> =
            f["marks"].as_array().unwrap().iter().map(|m| (m["center"][0].as_f64().unwrap(), m["center"][1].as_f64().unwrap())).collect();
        (v, marks)
    };
    // Two of 20 across, 10 apart: two circles less the lens they share. The lens: 2 r^2 acos(d / 2r)
    // - (d / 2) sqrt(4 r^2 - d^2), r = 10, d = 10.
    let lens = 200.0 * (0.5f64).acos() - 5.0 * 300f64.sqrt();
    let (v, marks) = centres(json!({ "shape": "round", "diameter": 20 }), &[(50.0, 50.0), (60.0, 50.0)]);
    assert!(close(v, 40_000.0 - (2.0 * PI * 100.0 - lens) * T, 1e-5), "{v}");
    assert!(
        close(marks[0].0, 50.0, 1e-6) && close(marks[0].1, 50.0, 1e-6) && close(marks[1].0, 60.0, 1e-6) && close(marks[1].1, 50.0, 1e-6),
        "{marks:?}"
    );
    // Squares that overlap, where the middle of what is left is no shape's middle.
    let (v, marks) = centres(json!({ "shape": "square", "side": 20 }), &[(50.0, 50.0), (60.0, 55.0)]);
    assert!(close(v, 40_000.0 - (800.0 - 10.0 * 15.0) * T, 1e-5), "{v}");
    assert!(
        close(marks[0].0, 50.0, 1e-6) && close(marks[0].1, 50.0, 1e-6) && close(marks[1].0, 60.0, 1e-6) && close(marks[1].1, 55.0, 1e-6),
        "{marks:?}"
    );
}

/// Low, a flap as long as its opening runs into the far side of it, wherever it is on the sheet.
/// Its lip's underside must be up off the face by then: by hand, height - t cos(turn) comes to 0.
#[test]
fn a_louver_too_low_to_clear_its_opening_says_how_high_it_must_be() {
    let stamp = |height: f64| {
        let (mut s, mut k) = start();
        plate(&mut s, &mut k, 200.0, 100.0);
        let sk = points_on_top(&mut s, &mut k, &[(100.0, 50.0)]);
        run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk, "shape": "louver", "length": 30, "width": 15, "height": height }));
        (s, k)
    };
    for low in [0.1, 1.0, 1.9] {
        let (mut s, mut k) = stamp(low);
        let e = failure(&mut s, &mut k);
        assert!(e.contains("at least 1.986 mm") && e.contains("make it higher") && !e.contains("smaller"), "{low}: {e}");
    }
    assert!(tenon_model::sheet::punch::louver_flap(15.0, 1.9, T, R, K).is_err());
    // What it says is what it takes: the lip's underside comes to the plane of the face there.
    let (turn, _) = tenon_model::sheet::punch::louver_flap(15.0, 1.986, T, R, K).unwrap();
    assert!((1.986 - T * turn.cos()).abs() < 0.006, "{}", 1.986 - T * turn.cos());
    for height in [1.986, 2.0, 3.0] {
        let (mut s, mut k) = stamp(height);
        let (turn, straight) = tenon_model::sheet::punch::louver_flap(15.0, height, T, R, K).unwrap();
        let v = volume(&mut s, &mut k);
        let want = 20_000.0 * T - 450.0 * T + 30.0 * T * (straight + turn * (R + T / 2.0));
        assert!(close(v, want, 1e-5), "{height}: {v} {want}");
        let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
        assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    }
    // On a sheet thin against its bends the same words have the same reason.
    let e = tenon_model::sheet::punch::louver_flap(15.0, 0.1, 0.5, 3.0, 0.44).unwrap_err();
    assert!(e.contains("at least") && e.contains("higher"), "{e}");
}
