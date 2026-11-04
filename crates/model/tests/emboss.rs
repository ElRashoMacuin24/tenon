//! Emboss (DEC-133): volumes worked out by hand on a flat face, on a cylinder and on a cone, raised,
//! recessed, from the sketch plane, tapered, and wrapped with the lengths kept.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI};

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn feature(v: &Value) -> u64 {
    v["feature"].as_u64().unwrap()
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    let t = run(s, k, "model.topology", json!({}));
    for b in t["bodies"].as_array().unwrap() {
        assert_eq!(b["valid"], true, "{b}");
    }
    run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

fn fails(s: &mut Session, k: &mut OcctKernel) -> String {
    let r = run(s, k, "model.regenerate", json!({}));
    r["error"]["message"].as_str().unwrap_or_else(|| panic!("it should fail: {r}")).to_owned()
}

#[track_caller]
fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} against {b}");
}

/// A block 40 x 40, 10 high, from the origin.
fn block(s: &mut Session, k: &mut OcctKernel) -> u64 {
    let sk = feature(&run(s, k, "sketch.create", json!({ "plane": "xy" })));
    run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 40 }));
    feature(&run(s, k, "model.extrude", json!({ "sketch": sk, "distance": 10 })))
}

/// A sketch on the plane `distance` above XY, with a rectangle.
fn rect_above(s: &mut Session, k: &mut OcctKernel, distance: f64, r: [f64; 4]) -> u64 {
    let wp = feature(&run(s, k, "work.plane", json!({ "base": "xy", "distance": distance })));
    let sk = feature(&run(s, k, "sketch.create", json!({ "work_plane": wp })));
    run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": r[0], "y1": r[1], "x2": r[2], "y2": r[3] }));
    sk
}

/// Lying along X through the origin, radius `r` and `len` long from x = 0.
fn shaft(s: &mut Session, k: &mut OcctKernel, r: f64, len: f64) -> u64 {
    let sk = feature(&run(s, k, "sketch.create", json!({ "plane": "yz" })));
    run(s, k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": r }));
    feature(&run(s, k, "model.extrude", json!({ "sketch": sk, "distance": len })))
}

#[test]
fn raised_and_recessed_on_a_flat_face_from_a_sketch_on_it() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let v0 = 16000.0;
    assert!((volume(&mut s, &mut k) - v0).abs() < 1e-6);
    // On the plane of the top face: 10 x 6, 2 up.
    let sk = rect_above(&mut s, &mut k, 10.0, [5.0, 5.0, 15.0, 11.0]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 2 })));
    near(volume(&mut s, &mut k), v0 + 120.0, 1e-6);
    let tree = run(&mut s, &mut k, "model.topology", json!({}));
    near(tree["bodies"][0]["bbox"]["max"][2].as_f64().unwrap(), 12.0, 1e-6);

    // Undo, suppress, delete: as a feature.
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": e }));
    near(volume(&mut s, &mut k), v0, 1e-6);
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": e, "suppressed": false }));
    near(volume(&mut s, &mut k), v0 + 120.0, 1e-6);

    // Changed to a recess, 3 deep: 10 x 6 x 3 out.
    let kind = json!({ "type": "emboss", "sketch": sk, "style": "recess", "depth": 3.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": e, "kind": kind }));
    near(volume(&mut s, &mut k), v0 - 180.0, 1e-6);
    run(&mut s, &mut k, "edit.undo", json!({}));
    near(volume(&mut s, &mut k), v0 + 120.0, 1e-6);

    // Its depth is a parameter.
    run(&mut s, &mut k, "param.add", json!({ "name": "lift", "equation": "5 mm" }));
    let kind = json!({ "type": "emboss", "sketch": sk, "style": "raise", "depth": 2.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": e, "kind": kind, "equations": { "/depth": "lift" } }));
    near(volume(&mut s, &mut k), v0 + 300.0, 1e-6);
    // Faces it made carry its name.
    let faces = run(&mut s, &mut k, "model.faces", json!({}));
    let top = faces["faces"].as_array().unwrap().iter().find(|f| f["name"] == json!({ "type": "cap", "feature": e, "end": "end" })).unwrap();
    near(top["centroid"][2].as_f64().unwrap(), 15.0, 1e-6);
    near(top["area"].as_f64().unwrap(), 60.0, 1e-6);
}

#[test]
fn seen_from_a_sketch_off_the_face_it_lands_on_the_face_and_follows_that_face_not_the_sketch() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    // The sketch 20 above the top face: raised from the face, not from the sketch.
    let sk = rect_above(&mut s, &mut k, 30.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 2 }));
    near(volume(&mut s, &mut k), 16120.0, 1e-6);
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    near(t["bodies"][0]["bbox"]["max"][2].as_f64().unwrap(), 12.0, 1e-6);

    // Looking the other way there is nothing on that side, and it says so.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 30.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 2, "reverse": true }));
    let said = fails(&mut s, &mut k);
    assert!(said.contains("no face of the part") && said.contains("flip"), "{said}");

    // From inside the block, looking up: on the top; reversed, on the bottom.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 5.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 2, "reverse": true }));
    near(volume(&mut s, &mut k), 16120.0, 1e-6);
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    near(t["bodies"][0]["bbox"]["min"][2].as_f64().unwrap(), -2.0, 1e-6);
}

#[test]
fn across_a_step_each_face_has_its_own_layer() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    // A step: x from 20 on stands 5 higher.
    let sk = rect_above(&mut s, &mut k, 10.0, [20.0, 0.0, 40.0, 40.0]);
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5 }));
    let v0 = 16000.0 + 4000.0;
    near(volume(&mut s, &mut k), v0, 1e-6);
    // Across the step, from above: 10 on each level, 6 wide, 2 up: and only what looks up. The
    // wall of the step is left bare (it gets no layer of its own), but what stands next to it
    // on the low side reaches only 2 up.
    let sk2 = rect_above(&mut s, &mut k, 30.0, [10.0, 5.0, 30.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk2, "depth": 2 }));
    near(volume(&mut s, &mut k), v0 + 20.0 * 6.0 * 2.0, 1e-6);
}

/// The part of the ring between `r0` and `r1` that a strip |x| <= a of a plane above the axis
/// shows, per unit length: the integral across the strip of sqrt(r1^2 - x^2) - sqrt(r0^2 - x^2).
fn seen_ring(r0: f64, r1: f64, a: f64) -> f64 {
    let area = |r: f64| a * (r * r - a * a).sqrt() + r * r * (a / r).asin();
    area(r1) - area(r0)
}

#[test]
fn on_a_cylinder_it_follows_the_round_with_its_sides_along_the_sketch_normal() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (r, d, a, len) = (10.0, 2.0, 3.0, 12.0);
    shaft(&mut s, &mut k, r, 50.0);
    let v0 = PI * r * r * 50.0;
    near(volume(&mut s, &mut k), v0, 1e-3);
    // On the plane above the axis, 12 along the shaft and 6 across it: seen on the top.
    let sk = rect_above(&mut s, &mut k, 15.0, [10.0, -a, 10.0 + len, a]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d })));
    let raised = seen_ring(r, r + d, a) * len;
    near(volume(&mut s, &mut k), v0 + raised, 1e-3);
    // The top is the round at r + d: at x = 0 it stands r + d high.
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    near(t["bodies"][0]["bbox"]["max"][2].as_f64().unwrap(), r + d, 1e-6);
    // Only the top: nothing on the underside.
    near(t["bodies"][0]["bbox"]["min"][2].as_f64().unwrap(), -r, 1e-6);

    // Recessed the same: the ring from r - d to r.
    let kind = json!({ "type": "emboss", "sketch": sk, "style": "recess", "depth": d });
    run(&mut s, &mut k, "feature.update", json!({ "feature": e, "kind": kind }));
    near(volume(&mut s, &mut k), v0 - seen_ring(r - d, r, a) * len, 1e-3);
    // The shaft's far side is untouched by what was seen from above.
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    near(t["bodies"][0]["bbox"]["min"][2].as_f64().unwrap(), -r, 1e-6);
}

#[test]
fn from_the_plane_material_is_added_on_one_side_and_taken_on_the_other() {
    // The plane above the top face: stands up to it.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 12.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 5, "style": "from_plane" }));
    near(volume(&mut s, &mut k), 16000.0 + 60.0 * 2.0, 1e-6);

    // The plane below it: goes down to it.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 8.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 5, "style": "from_plane" }));
    near(volume(&mut s, &mut k), 16000.0 - 60.0 * 2.0, 1e-6);

    // A plane through the block at a slope, by 1 in 10 across x, through the top at x = 20: on
    // the one side a wedge added, on the other a wedge taken. A square 10 x 10 centred on the
    // crossing: 5 x 10 of each, each 5 long rising to 0.5: 5 x 0.5 / 2 x 10 = 12.5 each.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let tilt = (0.1f64).atan();
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 20, "y": 20, "z": 10, "ry": -tilt })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "ucs_plane": { "ucs": u, "plane": "xy" } })));
    let half = 5.0 / tilt.cos();
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": -half, "y1": -5, "x2": half, "y2": 5 }));
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 5, "style": "from_plane" }));
    let v = volume(&mut s, &mut k);
    near(v, 16000.0, 1e-3);
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    near(t["bodies"][0]["bbox"]["max"][2].as_f64().unwrap(), 10.5, 1e-6);
    let m = run(&mut s, &mut k, "model.mass", json!({}));
    // What moved went from the low side to the high side, which a moment shows. Square to the
    // sloping plane, each wedge is a triangle: h = 5 / cos t along the plane, h tan t deep at its
    // end, 10 wide; its middle is 2h/3 along and h tan t / 3 down, which is 5 (2/3 + tan^2 t / 3)
    // along x from the crossing.
    let (tan, h) = (0.1f64, 5.0 / tilt.cos());
    let wedge = h * h * tan / 2.0 * 10.0;
    let moment = 2.0 * wedge * 5.0 * (2.0 / 3.0 + tan * tan / 3.0);
    let cx = m["bodies"][0]["center_of_mass"][0].as_f64().unwrap();
    near((cx - 20.0).abs(), moment / 16000.0, 1e-9);

    // Too little depth to reach: refused, in words.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 12.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1, "style": "from_plane" }));
    let said = fails(&mut s, &mut k);
    assert!(said.contains("does not reach the part") && said.contains("more depth"), "{said}");
}

#[test]
fn a_taper_narrows_what_is_raised_going_up_and_what_is_recessed_going_down() {
    // 10 x 6, 2 up on the top, 10 degrees: a frustum. Top 10 - 2 2 tan 10 by 6 - 2 2 tan 10.
    let t = 10f64.to_radians();
    let shrink = 2.0 * 2.0 * t.tan();
    let prismatoid = |a: f64, b: f64, h: f64, s: f64| {
        let (a2, b2) = (a - s, b - s);
        h / 6.0 * (a * b + a2 * b2 + 4.0 * (a + a2) / 2.0 * (b + b2) / 2.0)
    };
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 10.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 2, "taper": t }));
    near(volume(&mut s, &mut k), 16000.0 + prismatoid(10.0, 6.0, 2.0, shrink), 1e-3);

    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 10.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 2, "taper": t, "style": "recess" }));
    near(volume(&mut s, &mut k), 16000.0 - prismatoid(10.0, 6.0, 2.0, shrink), 1e-3);

    // From a plane 2 above the face, what is added widens from the plane to the part.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 12.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 5, "taper": t, "style": "from_plane" }));
    near(volume(&mut s, &mut k), 16000.0 + prismatoid(10.0 + shrink, 6.0 + shrink, 2.0, shrink), 1e-3);

    // A wrapped profile has straight sides.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    shaft(&mut s, &mut k, 10.0, 50.0);
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, 10.0, 0.0] }));
    let sk = rect_above(&mut s, &mut k, 15.0, [10.0, -3.0, 22.0, 3.0]);
    let e = s.exec("model.emboss", &json!({ "sketch": sk, "depth": 2, "taper": t, "wrap": face }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("straight sides"), "{e}");
}

#[test]
fn wrapped_round_a_cylinder_its_length_is_kept_along_the_round() {
    let (r, d, w, len) = (10.0, 2.0, 16.0, 12.0);
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    shaft(&mut s, &mut k, r, 50.0);
    let v0 = PI * r * r * 50.0;
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, 10.0, 0.0] }));
    // A plane above the axis: 12 along the shaft, 16 across, which is more than a seen strip of
    // this shaft would be useful for: laid round, it reaches 16 / 10 radians.
    let sk = rect_above(&mut s, &mut k, 15.0, [10.0, -w / 2.0, 10.0 + len, w / 2.0]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d, "wrap": face })));
    // Each slice along the axis is the ring from r to r + d over the angle w / r.
    let raised = len * (w / r) * ((r + d) * (r + d) - r * r) / 2.0;
    near(volume(&mut s, &mut k), v0 + raised, 1e-2);
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    let top = t["bodies"][0]["bbox"]["max"][2].as_f64().unwrap();
    near(top, r + d, 1e-6);
    // It goes round: its ends lie at w / (2 r) either side of the top, at y = (r + d) sin 0.8.
    let wide = t["bodies"][0]["bbox"]["max"][1].as_f64().unwrap();
    assert!(wide >= r - 1e-6, "the shaft itself is {r} wide: {wide}");

    // Recessed.
    let kind = json!({ "type": "emboss", "sketch": sk, "style": "recess", "depth": d, "wrap": face });
    run(&mut s, &mut k, "feature.update", json!({ "feature": e, "kind": kind }));
    let cut = len * (w / r) * (r * r - (r - d) * (r - d)) / 2.0;
    near(volume(&mut s, &mut k), v0 - cut, 1e-2);

    // A disc laid round it: as a rectangle's, by the area laid.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    shaft(&mut s, &mut k, r, 50.0);
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, 10.0, 0.0] }));
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 15 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp })));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 25, "cy": 0, "r": 5 }));
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1, "wrap": face }));
    let raised = PI * 25.0 * ((11.0f64 * 11.0 - 100.0) / (2.0 * 10.0));
    near(volume(&mut s, &mut k), v0 + raised, 1e-2);

    // A ring: the hole stays open.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    shaft(&mut s, &mut k, r, 50.0);
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, 10.0, 0.0] }));
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 15 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp })));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 25, "cy": 0, "r": 5 }));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 25, "cy": 0, "r": 2 }));
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1, "wrap": face }));
    let raised = PI * (25.0 - 4.0) * ((11.0f64 * 11.0 - 100.0) / (2.0 * 10.0));
    near(volume(&mut s, &mut k), v0 + raised, 1e-2);
}

#[test]
fn on_the_far_side_of_a_cylinder_and_inside_a_hole() {
    let (r, d, w, len) = (10.0, 2.0, 8.0, 12.0);
    // Reversed: on the side of the shaft away from the sketch.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    shaft(&mut s, &mut k, r, 50.0);
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, 10.0, 0.0] }));
    let sk = rect_above(&mut s, &mut k, 15.0, [10.0, -w / 2.0, 10.0 + len, w / 2.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d, "wrap": face, "reverse": true }));
    let v0 = PI * r * r * 50.0;
    near(volume(&mut s, &mut k), v0 + len * (w / r) * ((r + d) * (r + d) - r * r) / 2.0, 1e-2);
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    near(t["bodies"][0]["bbox"]["min"][2].as_f64().unwrap(), -(r + d), 1e-6);
    near(t["bodies"][0]["bbox"]["max"][2].as_f64().unwrap(), r, 1e-6);

    // In the wall of a bore: recessed goes out into the wall. A tube 30 outside, 20 bore.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk0 = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "yz" })));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk0, "cx": 0, "cy": 0, "r": 15 }));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk0, "cx": 0, "cy": 0, "r": 10 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk0, "distance": 50 }));
    let v0 = PI * (225.0 - 100.0) * 50.0;
    near(volume(&mut s, &mut k), v0, 1e-3);
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, 10.0, 0.0] }));
    let sk = rect_above(&mut s, &mut k, 5.0, [10.0, -w / 2.0, 10.0 + len, w / 2.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d, "wrap": face, "style": "recess" }));
    near(volume(&mut s, &mut k), v0 - len * (w / 10.0) * (144.0 - 100.0) / 2.0, 1e-2);
    // Raised, it stands in from the wall.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk0 = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "yz" })));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk0, "cx": 0, "cy": 0, "r": 15 }));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk0, "cx": 0, "cy": 0, "r": 10 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk0, "distance": 50 }));
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, 10.0, 0.0] }));
    let sk = rect_above(&mut s, &mut k, 5.0, [10.0, -w / 2.0, 10.0 + len, w / 2.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d, "wrap": face, "style": "raise" }));
    near(volume(&mut s, &mut k), v0 + len * (w / 10.0) * (100.0 - 64.0) / 2.0, 1e-2);
}

#[test]
fn wrapped_round_a_cone_its_area_is_kept() {
    // A cone: radius 40 at the bottom to 10 at z = 30: half angle 45 degrees, apex at z = 40.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk0 = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" })));
    let a = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk0, "x1": 0, "y1": 0, "x2": 40, "y2": 0 }));
    let b = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk0, "start": a["end"], "x2": 10, "y2": 30 }));
    let c = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk0, "start": b["end"], "x2": 0, "y2": 30 }));
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk0, "start": c["end"], "end": a["start"] }));
    run(&mut s, &mut k, "model.revolve", json!({ "sketch": sk0, "axis": "z" }));
    // Frustum volume: pi h / 3 (R^2 + R r + r^2).
    let v0 = PI * 30.0 / 3.0 * (1600.0 + 400.0 + 100.0);
    near(volume(&mut s, &mut k), v0, 1e-2);
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, 0.0, 15.0] }));
    // A plane that touches the cone along its generator in the XZ plane on the +X side: looks
    // out along (1, 0, 1) / sqrt 2. The middle of the generator is at (25, 0, 15).
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 25, "z": 15, "ry": FRAC_PI_2 / 2.0 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "ucs_plane": { "ucs": u, "plane": "xy" } })));
    // Its local X runs down the slope (1, 0, -1) / sqrt 2; Y along world Y. 10 x 6, centred.
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": -5, "y1": -3, "x2": 5, "y2": 3 }));
    let depth = 0.5;
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": depth, "wrap": face }));
    // As in the kernel's test: d + d^2 cot(a) / (2 rho) over the area, rho from the apex in the
    // sketch's plane. The middle of the generator is 15 sqrt 2 + ... from the apex at z = 40:
    // (25, 15) to (0, 40) is 25 sqrt 2.
    let rho0 = 25.0 * 2f64.sqrt();
    let (mut sum, n) = (0.0, 600);
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (-3.0 + 6.0 * (f64::from(i) + 0.5) / f64::from(n), -5.0 + 10.0 * (f64::from(j) + 0.5) / f64::from(n));
            sum += (depth + depth * depth / (2.0 * x.hypot(y + rho0))) * (6.0 / f64::from(n)) * (10.0 / f64::from(n));
        }
    }
    near(volume(&mut s, &mut k), v0 + sum, 2e-2);
    // Still on the cone: nothing stands off it by more than the depth, at the outline's lowest.
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    let reach = t["bodies"][0]["bbox"]["max"][0].as_f64().unwrap();
    assert!(reach <= 40.0 + 1e-6, "{reach}");
    let _ = FRAC_1_SQRT_2;
}

#[test]
fn text_is_raised_on_a_flat_face_and_on_a_round_one_and_wrapped() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 10 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp })));
    run(&mut s, &mut k, "sketch.text", json!({ "sketch": sk, "x": 5, "y": 15, "text": "TO", "height": 10 }));
    let flat = feature(&run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 1 })));
    let by_extrude = volume(&mut s, &mut k);
    assert!(by_extrude > 16000.0 + 10.0, "{by_extrude}");
    run(&mut s, &mut k, "feature.delete", json!({ "feature": flat }));
    // On a flat face, raised is the same solid as an extrusion from the face.
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 }));
    near(volume(&mut s, &mut k), by_extrude, 1e-3);

    // Round a shaft: raised, seen on it; and laid round it. The area of the letters is what is
    // kept: raised by d it is about area x d (1 + d / (2 r)).
    let area = by_extrude - 16000.0;
    let (r, d) = (20.0, 0.5);
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    shaft(&mut s, &mut k, r, 60.0);
    let v0 = PI * r * r * 60.0;
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [30.0, 20.0, 0.0] }));
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 25 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp })));
    run(&mut s, &mut k, "sketch.text", json!({ "sketch": sk, "x": 5, "y": 15, "text": "TO", "height": 10 }));
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d, "wrap": face })));
    near(volume(&mut s, &mut k), v0 + area * d * (1.0 + d / (2.0 * r)), 1e-1);
    // Seen on it (a profile off the axis, so that it follows the round as it is).
    let kind = json!({ "type": "emboss", "sketch": sk, "depth": d });
    run(&mut s, &mut k, "feature.update", json!({ "feature": e, "kind": kind }));
    let seen = volume(&mut s, &mut k) - v0;
    assert!(seen > area * d * 0.9 && seen < area * d * 1.3, "{seen} against about {}", area * d);
}

#[test]
fn it_is_copied_by_a_pattern_and_a_mirror_and_its_faces_are_built_on() {
    // Mirrored in the middle of the block: another of them.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "yz", "distance": 20 })));
    let sk = rect_above(&mut s, &mut k, 10.0, [5.0, 5.0, 15.0, 11.0]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 2 })));
    run(&mut s, &mut k, "model.mirror", json!({ "features": [e], "work_plane": wp }));
    near(volume(&mut s, &mut k), 16000.0 + 240.0, 1e-6);

    // From the plane, what it adds and what it takes both go with it: one side of a plane in
    // the block goes down to it (recessed 2), copied twice more along X by 30... but a plane is
    // copied with it, all at 8 up.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk0 = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk0, "x1": 0, "y1": 0, "x2": 100, "y2": 40 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk0, "distance": 10 }));
    let sk = rect_above(&mut s, &mut k, 8.0, [5.0, 5.0, 15.0, 11.0]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 5, "style": "from_plane" })));
    run(&mut s, &mut k, "model.pattern.rect", json!({ "features": [e], "direction": "x", "count": 3, "spacing": 30 }));
    near(volume(&mut s, &mut k), 40000.0 - 3.0 * 120.0, 1e-6);

    // Wrapped text round a shaft, four times about its axis: four times what one adds.
    let (r, d) = (20.0, 1.0);
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    shaft(&mut s, &mut k, r, 60.0);
    let v0 = PI * r * r * 60.0;
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [30.0, r, 0.0] }));
    let sk = rect_above(&mut s, &mut k, 25.0, [10.0, -3.0, 20.0, 3.0]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d, "wrap": face })));
    let one = 10.0 * (6.0 / r) * ((r + d) * (r + d) - r * r) / 2.0;
    near(volume(&mut s, &mut k), v0 + one, 1e-2);
    run(&mut s, &mut k, "model.pattern.circular", json!({ "features": [e], "axis": "x", "count": 4 }));
    near(volume(&mut s, &mut k), v0 + 4.0 * one, 1e-2);

    // Its top and its sides can be picked and hold on: a chamfer round the top of what is raised
    // follows when it is made deeper. A sketch goes on the top.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 10.0, [5.0, 5.0, 15.0, 11.0]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 2 })));
    near(volume(&mut s, &mut k), 16120.0, 1e-6);
    let edge = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [10.0, 5.0, 12.0] }));
    assert_eq!(edge["faces"].as_array().unwrap().len(), 2);
    let ch = feature(&run(&mut s, &mut k, "model.chamfer", json!({ "edges": [edge], "distance": 0.5 })));
    near(volume(&mut s, &mut k), 16120.0 - 0.5 * 0.5 * 0.5 * 10.0, 1e-6);
    let kind = json!({ "type": "emboss", "sketch": sk, "depth": 4.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": e, "kind": kind }));
    near(volume(&mut s, &mut k), 16240.0 - 0.5 * 0.5 * 0.5 * 10.0, 1e-6);
    let _ = ch;
    let top = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": e, "end": "end" } }));
    let sk2 = feature(&run(&mut s, &mut k, "sketch.create", json!({ "face": top })));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk2, "cx": 10, "cy": 8, "r": 1 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk2, "distance": 3 }));
    near(volume(&mut s, &mut k), 16240.0 - 0.5 * 0.5 * 0.5 * 10.0 + PI * 3.0, 1e-6);
}

#[test]
fn it_leaves_nothing_in_the_kernel_however_it_ends() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    shaft(&mut s, &mut k, 10.0, 50.0);
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, 10.0, 0.0] }));
    let sk = rect_above(&mut s, &mut k, 15.0, [10.0, -3.0, 20.0, 3.0]);
    for p in [
        json!({ "sketch": sk, "depth": 1.0 }),
        json!({ "sketch": sk, "depth": 1.0, "style": "recess" }),
        json!({ "sketch": sk, "depth": 1.0, "style": "recess", "taper": 0.1 }),
        json!({ "sketch": sk, "depth": 7.0, "style": "from_plane" }),
        json!({ "sketch": sk, "depth": 1.0, "wrap": face }),
        json!({ "sketch": sk, "depth": 1.0, "style": "recess", "wrap": face }),
        // These fail.
        json!({ "sketch": sk, "depth": 1.0, "style": "from_plane" }),
        json!({ "sketch": sk, "depth": 1.0, "reverse": true, "wrap": { "origin": { "type": "cap", "feature": 2, "end": "end" }, "fingerprint": { "surface": "plane", "direction": { "x": 1.0, "y": 0.0, "z": 0.0 }, "centroid": { "x": 50.0, "y": 0.0, "z": 0.0 }, "area": 314.0 } } }),
    ] {
        let mut t = Session::new(s.document().clone());
        let (mut k2, q) = (OcctKernel::new(), p.clone());
        run(&mut t, &mut k2, "model.emboss", q);
        let mut r = tenon_model::regenerate(t.document(), &mut k2);
        r.release(&mut k2);
        assert_eq!(tenon_kernel::Kernel::live_shapes(&k2), 0, "{p}");
    }
    let _ = k;
}

#[test]
fn what_cannot_be_embossed_says_what_to_do() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = rect_above(&mut s, &mut k, 0.0, [0.0, 0.0, 10.0, 10.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 2 }));
    let said = fails(&mut s, &mut k);
    assert!(said.contains("no part to emboss"), "{said}");

    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let e = s.exec("model.emboss", &json!({ "sketch": 2, "depth": 2 }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("is not a sketch"), "{e}");
    let sk = rect_above(&mut s, &mut k, 10.0, [5.0, 5.0, 15.0, 11.0]);
    let e = s.exec("model.emboss", &json!({ "sketch": sk, "depth": -2 }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("depth must be more than 0"), "{e}");
    let e = s.exec("model.emboss", &json!({ "sketch": sk, "depth": 2, "style": "wavy" }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("`style` must be one of"), "{e}");
    // Wrap to Face on a flat face.
    let top = run(&mut s, &mut k, "model.face_ref", json!({ "at": [20.0, 20.0, 10.0] }));
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 2, "wrap": top }));
    let said = fails(&mut s, &mut k);
    assert!(said.contains("round a cylinder or a cone"), "{said}");
    // From Plane does not go with it.
    let e = s.exec("model.emboss", &json!({ "sketch": sk, "depth": 2, "style": "from_plane", "wrap": top }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("does not go with From Plane"), "{e}");
}
