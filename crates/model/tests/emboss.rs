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

#[test]
fn over_an_outside_corner_each_face_has_its_layer_and_the_corner_between_stays_open() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    // Seen at 45 degrees over the front top edge (y = 0, z = 10): a plane looking along (0, -1, 1)
    // from well off. A UCS turned about X gives it: its Z is (0, -sin 45, cos 45).
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 20, "y": -20, "z": 30, "rx": std::f64::consts::FRAC_PI_4 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "ucs_plane": { "ucs": u, "plane": "xy" } })));
    let half = 5.0 * FRAC_1_SQRT_2;
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": -5, "y1": -half, "x2": 5, "y2": half }));
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 }));
    // Square across the edge, each layer is what lies within the strip of half width a from the
    // edge: integral from 0 to d of (a sqrt 2 - w) dw, a sqrt 2 being 5; twice that, 10 long.
    near(volume(&mut s, &mut k), 16000.0 + 2.0 * (5.0 - 0.5) * 10.0, 1e-6);
    // It stands 1 up and 1 out from the block, and no further: the corner itself stays open.
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    near(t["bodies"][0]["bbox"]["max"][2].as_f64().unwrap(), 11.0, 1e-6);
    near(t["bodies"][0]["bbox"]["min"][1].as_f64().unwrap(), -1.0, 1e-6);
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
fn wrapped_it_lies_where_the_sketch_says_and_reads_as_it_does() {
    let (r, d) = (10.0f64, 2.0f64);
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    shaft(&mut s, &mut k, r, 50.0);
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, r, 0.0] }));
    // 12 along the shaft from x = 10, and from 2 to 10 to the +Y side of the plane over the axis.
    let sk = rect_above(&mut s, &mut k, 15.0, [10.0, 2.0, 22.0, 10.0]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d, "wrap": face })));
    volume(&mut s, &mut k);
    let faces = run(&mut s, &mut k, "model.faces", json!({}));
    let top = faces["faces"].as_array().unwrap().iter().find(|f| f["name"] == json!({ "type": "cap", "feature": e, "end": "end" })).unwrap();
    // Its top: 12 long, and 8 / r radians of the round at r + d: from 0.2 to 1.0 rad round from the
    // top toward +Y (as the sketch has it from above), about x = 16.
    near(top["area"].as_f64().unwrap(), 12.0 * 0.8 * (r + d), 1e-3);
    let c = &top["centroid"];
    near(c[0].as_f64().unwrap(), 16.0, 1e-3);
    // The middle of the arc is 0.6 rad round; the centroid of an arc of half angle 0.4 lies at
    // r sin(0.4) / 0.4 along that.
    let along = (r + d) * 0.4f64.sin() / 0.4;
    near(c[1].as_f64().unwrap(), along * 0.6f64.sin(), 1e-3);
    near(c[2].as_f64().unwrap(), along * 0.6f64.cos(), 1e-3);

    // With the sketch's plane looking at the shaft from below instead, and the part seen from
    // there: on the underside, the same way round the axis from the sketch's Y as it looks.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    shaft(&mut s, &mut k, r, 50.0);
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, r, 0.0] }));
    let sk = rect_above(&mut s, &mut k, -15.0, [10.0, 2.0, 22.0, 10.0]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d, "wrap": face })));
    volume(&mut s, &mut k);
    let faces = run(&mut s, &mut k, "model.faces", json!({}));
    let top = faces["faces"].as_array().unwrap().iter().find(|f| f["name"] == json!({ "type": "cap", "feature": e, "end": "end" })).unwrap();
    let c = &top["centroid"];
    near(c[1].as_f64().unwrap(), along * 0.6f64.sin(), 1e-3);
    near(c[2].as_f64().unwrap(), -along * 0.6f64.cos(), 1e-3);
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
fn on_a_sphere_and_on_a_freeform_face_it_follows_them_too() {
    // On a sphere, a disc seen from above: between the spheres R and R + d within the round of
    // radius a. Each by the integral of 2 pi rho sqrt(S^2 - rho^2) from 0 to a.
    let (r, d, a) = (20.0f64, 2.0f64, 6.0f64);
    let cap = |s: f64| 2.0 * PI / 3.0 * (s.powi(3) - (s * s - a * a).powf(1.5));
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.sphere", json!({ "diameter": 2.0 * r }));
    let v0 = 4.0 / 3.0 * PI * r.powi(3);
    near(volume(&mut s, &mut k), v0, 1e-2);
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 30 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp })));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": a }));
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d })));
    near(volume(&mut s, &mut k), v0 + cap(r + d) - cap(r), 1e-2);
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    near(t["bodies"][0]["bbox"]["max"][2].as_f64().unwrap(), r + d, 1e-6);
    // Recessed.
    let kind = json!({ "type": "emboss", "sketch": sk, "style": "recess", "depth": d });
    run(&mut s, &mut k, "feature.update", json!({ "feature": e, "kind": kind }));
    near(volume(&mut s, &mut k), v0 - (cap(r) - cap(r - d)), 1e-2);

    // On a smooth blend between two circles, whose face is a spline: about the area seen times
    // the depth, and a whole solid.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let low = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": low, "cx": 0, "cy": 0, "r": 20 }));
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 30 })));
    let high = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp })));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": high, "cx": 0, "cy": 0, "r": 12 }));
    run(&mut s, &mut k, "model.loft", json!({ "sections": [low, high] }));
    let v0 = volume(&mut s, &mut k);
    let front = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xz", "distance": 40 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": front })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": -3, "y1": 10, "x2": 3, "y2": 20 }));
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 }));
    let v = volume(&mut s, &mut k) - v0;
    assert!(v > 60.0 * 0.9 && v < 60.0 * 1.3, "60 mm^2 seen, 1 up, on a slope: {v}");
}

#[test]
fn it_follows_its_sketch_and_the_part_under_it() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let ex = block(&mut s, &mut k);
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 30 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp })));
    let lines = run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 5, "y1": 5, "x2": 15, "y2": 11 }));
    let line = lines["lines"][0].as_u64().unwrap();
    let dim = run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": line, "value": 10.0 } }))["constraint"].as_u64().unwrap();
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 2 }));
    near(volume(&mut s, &mut k), 16120.0, 1e-6);
    // The profile longer: 16 x 6 x 2.
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": sk, "constraint": dim, "value": 16.0 }));
    near(volume(&mut s, &mut k), 16000.0 + 192.0, 1e-6);
    // The part under it taller: it stands on the new top.
    let kind = json!({ "type": "extrude", "sketch": 1, "regions": "default", "extent": { "distance": 20.0 }, "reverse": false, "operation": "join" });
    run(&mut s, &mut k, "feature.update", json!({ "feature": ex, "kind": kind }));
    near(volume(&mut s, &mut k), 32000.0 + 192.0, 1e-6);
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    near(t["bodies"][0]["bbox"]["max"][2].as_f64().unwrap(), 22.0, 1e-6);
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
fn it_leaves_nothing_in_the_kernel_past_a_round_and_under_an_overhang_or_when_refused() {
    let count = |s: &Session| {
        let mut k = OcctKernel::new();
        let mut r = tenon_model::regenerate(s.document(), &mut k);
        r.release(&mut k);
        tenon_kernel::Kernel::live_shapes(&k)
    };
    // Wider than the shaft, and on past its end.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    shaft(&mut s, &mut k, 5.0, 50.0);
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, 5.0, 0.0] }));
    let sk = rect_above(&mut s, &mut k, 12.0, [10.0, -8.0, 20.0, 8.0]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 })));
    assert_eq!(count(&s), 0);
    let off = rect_above(&mut s, &mut k, 12.0, [60.0, -2.0, 70.0, 2.0]);
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": e }));
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": off, "depth": 1, "wrap": face }));
    assert!(fails(&mut s, &mut k).contains("lies off the face"));
    assert_eq!(count(&s), 0);
    // Over something and under it.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk0 = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk0, "x1": 0, "y1": 0, "x2": 40, "y2": 40 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk0, "distance": 5 }));
    let web = rect_above(&mut s, &mut k, 5.0, [15.0, 0.0, 25.0, 40.0]);
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": web, "distance": 20 }));
    let flange = rect_above(&mut s, &mut k, 25.0, [10.0, 0.0, 30.0, 40.0]);
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": flange, "distance": 5 }));
    let sk = rect_above(&mut s, &mut k, 30.0, [5.0, 5.0, 35.0, 15.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 }));
    assert_eq!(count(&s), 0);
    // Refused for what it would leave, with the part held to what it was.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 10.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 5, "style": "from_plane", "reverse": true }));
    assert!(fails(&mut s, &mut k).contains("comes loose"));
    assert_eq!(count(&s), 0);
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 8.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 5, "style": "from_plane", "reverse": true }));
    assert!(fails(&mut s, &mut k).contains("hollow"));
    assert_eq!(count(&s), 0);
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

/// The area under the round of radius `r` over the strip of a plane from `y0` to `y1`, the strip
/// being taken only as far as the round reaches.
fn under_round(r: f64, y0: f64, y1: f64) -> f64 {
    let f = |y: f64| {
        let y = y.clamp(-r, r);
        0.5 * (y * (r * r - y * y).sqrt() + r * r * (y / r).asin())
    };
    f(y1) - f(y0)
}

/// A sketch on the plane `off` along the Z of a UCS turned `turn` about X, with a rectangle: the
/// sketch's y runs across a shaft along X, and it looks at the shaft from that side.
fn seen_turned(s: &mut Session, k: &mut OcctKernel, turn: f64, off: f64, r: [f64; 4]) -> u64 {
    let u = feature(&run(s, k, "work.ucs", json!({ "rx": turn })));
    let v = feature(&run(s, k, "work.ucs", json!({ "base": u, "z": off })));
    let sk = feature(&run(s, k, "sketch.create", json!({ "ucs_plane": { "ucs": v, "plane": "xy" } })));
    run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": r[0], "y1": r[1], "x2": r[2], "y2": r[3] }));
    sk
}

/// One solid, whole.
#[track_caller]
fn one_solid(s: &mut Session, k: &mut OcctKernel) {
    let t = run(s, k, "model.topology", json!({}));
    assert_eq!(t["bodies"].as_array().unwrap().len(), 1, "{t}");
    assert_eq!(t["bodies"][0]["solids"], 1, "{t}");
}

#[test]
fn a_profile_wider_than_the_round_stops_where_the_round_turns_away() {
    let (r, len, l) = (5.0, 50.0, 10.0);
    let v0 = PI * r * r * len;
    // From every side, wherever the shaft's seam falls; wider than the shaft, narrower and to one
    // side. Each is the ring as seen, whose ends run to the plane through the axis.
    for turn in [0.0f64, 10.0, 20.0, 35.0, 60.0, 90.0, 135.0, 180.0, -30.0, -100.0] {
        for (y0, y1) in [(-8.0, 8.0), (-3.0, 3.0), (-3.5, -1.0), (1.0, 3.5), (2.0, 8.0)] {
            for (style, d) in [("raise", 1.0), ("recess", 1.0), ("raise", 0.25)] {
                let (mut s, mut k) = (Session::default(), OcctKernel::new());
                shaft(&mut s, &mut k, r, len);
                let sk = seen_turned(&mut s, &mut k, turn.to_radians(), 12.0, [10.0, y0, 10.0 + l, y1]);
                run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d, "style": style }));
                let hand = if style == "raise" {
                    v0 + l * (under_round(r + d, y0, y1) - under_round(r, y0, y1))
                } else {
                    v0 - l * (under_round(r, y0, y1) - under_round(r - d, y0, y1))
                };
                let v = volume(&mut s, &mut k);
                assert!((v - hand).abs() < 1e-2, "{style} {d} at {turn} over {y0}..{y1}: {v} against {hand}");
                one_solid(&mut s, &mut k);
            }
        }
    }

    // As wide as the box round the profile comes to the round: narrow against a small shaft, and
    // deep against a large one.
    for (r, w, d) in [(5.0, 2.0, 1.0), (5.0, 2.5, 1.0), (5.0, 1.0, 1.0), (5.0, 1.0, 2.0), (5.0, 4.0, 1.0), (3.0, 1.0, 0.3), (2.0, 0.5, 0.5)] {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        shaft(&mut s, &mut k, r, 30.0);
        let sk = rect_above(&mut s, &mut k, r + 1.0, [10.0, -w, 20.0, w]);
        run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d }));
        let hand = PI * r * r * 30.0 + 10.0 * (under_round(r + d, -w, w) - under_round(r, -w, w));
        let v = volume(&mut s, &mut k);
        assert!((v - hand).abs() < 1e-2, "r {r}, {w} wide, {d} up: {v} against {hand}");
        one_solid(&mut s, &mut k);
    }
    for d in [15.0, 25.0, 40.0] {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        shaft(&mut s, &mut k, 50.0, 200.0);
        let sk = rect_above(&mut s, &mut k, 51.0, [10.0, -1.0, 20.0, 1.0]);
        run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d, "style": "recess" }));
        let hand = PI * 2500.0 * 200.0 - 10.0 * (under_round(50.0, -1.0, 1.0) - under_round(50.0 - d, -1.0, 1.0));
        near(volume(&mut s, &mut k), hand, 1e-2);
        one_solid(&mut s, &mut k);
    }

    // A tube: what looks up inside the bore lies under the shaft's top, and takes nothing.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk0 = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "yz" })));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk0, "cx": 0, "cy": 0, "r": 10 }));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk0, "cx": 0, "cy": 0, "r": 5 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk0, "distance": 50 }));
    let sk = rect_above(&mut s, &mut k, 11.0, [-5.0, -15.0, 20.0, 15.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 }));
    near(volume(&mut s, &mut k), PI * 75.0 * 50.0 + 20.0 * PI * (121.0 - 100.0) / 2.0, 1e-2);
    one_solid(&mut s, &mut k);
}

#[test]
fn on_a_ball_a_cone_a_ring_and_a_freeform_face_it_stops_where_they_turn_away() {
    // A ball from each side and in each quarter of its round: between the balls of radius 10 and
    // 11 (raised) or 9 and 10 (recessed) over the rectangle.
    let over_ball = |big: f64, small: f64, a: (f64, f64), b: (f64, f64)| {
        let (mut sum, n) = (0.0, 400);
        for i in 0..n {
            for j in 0..n {
                let (x, y) = (a.0 + (a.1 - a.0) * (f64::from(i) + 0.5) / f64::from(n), b.0 + (b.1 - b.0) * (f64::from(j) + 0.5) / f64::from(n));
                sum += (big * big - x * x - y * y).sqrt() - (small * small - x * x - y * y).sqrt();
            }
        }
        sum * (a.1 - a.0) * (b.1 - b.0) / f64::from(n * n)
    };
    let (r, v0) = (10.0, 4.0 / 3.0 * PI * 1000.0);
    for base in ["yz", "xz", "xy"] {
        for quarter in [[2.0, 2.0, 5.0, 5.0], [-5.0, 2.0, -2.0, 5.0], [-5.0, -5.0, -2.0, -2.0], [2.0, -5.0, 5.0, -2.0]] {
            for style in ["raise", "recess"] {
                let (mut s, mut k) = (Session::default(), OcctKernel::new());
                run(&mut s, &mut k, "model.sphere", json!({ "diameter": 2.0 * r }));
                let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": base, "distance": 12 })));
                let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp })));
                run(
                    &mut s,
                    &mut k,
                    "sketch.rectangle",
                    json!({ "sketch": sk, "x1": quarter[0], "y1": quarter[1], "x2": quarter[2], "y2": quarter[3] }),
                );
                run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1, "style": style }));
                let (across, up) = ((quarter[0], quarter[2]), (quarter[1], quarter[3]));
                let hand = if style == "raise" { v0 + over_ball(11.0, r, across, up) } else { v0 - over_ball(r, 9.0, across, up) };
                let v = volume(&mut s, &mut k);
                assert!((v - hand).abs() < 1e-2, "{style} off {base} over {quarter:?}: {v} against {hand}");
                one_solid(&mut s, &mut k);
            }
        }
    }
    // All it shows from one side: half a shell.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.sphere", json!({ "diameter": 2.0 * r }));
    let sk = rect_above(&mut s, &mut k, 12.0, [-20.0, -20.0, 20.0, 20.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 }));
    near(volume(&mut s, &mut k), v0 + 2.0 / 3.0 * PI * (1331.0 - 1000.0), 1e-2);
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    near(t["bodies"][0]["bbox"]["min"][2].as_f64().unwrap(), -10.0, 1e-6);

    // A cone from the side, its radius 20 at z = 0 going in by 1 in 3. What a depth along its
    // normal comes to across a level: half the ring whose radius is more by d / cos(a), a being
    // the angle it leans; so over z from 10 to 20, whose radius has the mean 15.
    let cone = |s: &mut Session, k: &mut OcctKernel| {
        let sk0 = feature(&run(s, k, "sketch.create", json!({ "plane": "xz" })));
        let a = run(s, k, "sketch.line", json!({ "sketch": sk0, "x1": 0, "y1": 0, "x2": 20, "y2": 0 }));
        let b = run(s, k, "sketch.line", json!({ "sketch": sk0, "start": a["end"], "x2": 10, "y2": 30 }));
        let c = run(s, k, "sketch.line", json!({ "sketch": sk0, "start": b["end"], "x2": 0, "y2": 30 }));
        run(s, k, "sketch.line", json!({ "sketch": sk0, "start": c["end"], "end": a["start"] }));
        run(s, k, "model.revolve", json!({ "sketch": sk0, "axis": "z" }));
    };
    let d = 0.5;
    let wider = d * 10f64.sqrt() / 3.0;
    let hand = PI / 2.0 * (2.0 * 15.0 * wider * 10.0 + wider * wider * 10.0);
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    cone(&mut s, &mut k);
    let v0 = volume(&mut s, &mut k);
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xz", "distance": 30 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": -30, "y1": 10, "x2": 30, "y2": 20 }));
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d }));
    near(volume(&mut s, &mut k) - v0, hand, 1e-2);
    one_solid(&mut s, &mut k);

    // The same cone as a loft through three circles, whose face is a spline: the same, near enough.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let mut sections = Vec::new();
    for (z, r) in [(0.0, 20.0), (15.0, 15.0), (30.0, 10.0)] {
        let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": z })));
        let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp })));
        run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": r }));
        sections.push(sk);
    }
    run(&mut s, &mut k, "model.loft", json!({ "sections": sections }));
    let v0 = volume(&mut s, &mut k);
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xz", "distance": 30 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": -30, "y1": 10, "x2": 30, "y2": 20 }));
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d })));
    let v = volume(&mut s, &mut k) - v0;
    assert!((v - hand).abs() < 5e-2, "{v} against {hand}");
    one_solid(&mut s, &mut k);
    // Recessed: the ring inside it.
    let kind = json!({ "type": "emboss", "sketch": sk, "style": "recess", "depth": d });
    run(&mut s, &mut k, "feature.update", json!({ "feature": e, "kind": kind }));
    let v = v0 - volume(&mut s, &mut k);
    let hand = PI / 2.0 * (2.0 * 15.0 * wider * 10.0 - wider * wider * 10.0);
    assert!((v - hand).abs() < 5e-2, "{v} against {hand}");

    // A ring seen from the side, over its near limb. Each surface is a graph over the sketch's
    // plane, so the layer is the difference of the two heights, summed over the rectangle.
    let (big, small, d) = (20.0, 5.0, 1.0);
    let height = |r: f64, y: f64, z: f64| ((big + (r * r - z * z).sqrt()).powi(2) - y * y).sqrt();
    let (mut sum, n) = (0.0, 400);
    for i in 0..n {
        for j in 0..n {
            let (y, z) = (16.0 + 8.0 * (f64::from(i) + 0.5) / f64::from(n), -2.0 + 4.0 * (f64::from(j) + 0.5) / f64::from(n));
            sum += height(small + d, y, z) - height(small, y, z);
        }
    }
    let hand = sum * 8.0 * 4.0 / f64::from(n * n);
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.torus", json!({ "plane": "xz", "diameter": 2.0 * big, "section": 2.0 * small }));
    let v0 = volume(&mut s, &mut k);
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "yz", "distance": 40 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 16, "y1": -2, "x2": 24, "y2": 2 }));
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d }));
    let v = volume(&mut s, &mut k) - v0;
    assert!((v - hand).abs() < 1e-2, "{v} against {hand}");
    one_solid(&mut s, &mut k);
}

#[test]
fn what_lies_under_an_overhang_is_not_seen() {
    // An I section along Y: a base 40 wide, a web from x = 15 to 25, a flange from x = 10 to 30.
    let ibeam = |s: &mut Session, k: &mut OcctKernel| {
        let sk = feature(&run(s, k, "sketch.create", json!({ "plane": "xy" })));
        run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 40 }));
        run(s, k, "model.extrude", json!({ "sketch": sk, "distance": 5 }));
        let web = rect_above(s, k, 5.0, [15.0, 0.0, 25.0, 40.0]);
        run(s, k, "model.extrude", json!({ "sketch": web, "distance": 20 }));
        let flange = rect_above(s, k, 25.0, [10.0, 0.0, 30.0, 40.0]);
        run(s, k, "model.extrude", json!({ "sketch": flange, "distance": 5 }));
    };
    let v0 = 40.0 * 40.0 * 5.0 + 10.0 * 40.0 * 20.0 + 20.0 * 40.0 * 5.0;
    // On the flange alone, inside it and with the box round it past its edge: the base under it
    // takes nothing.
    for x in [(14.0, 26.0), (12.0, 28.0), (10.5, 29.5)] {
        for (style, sign) in [("raise", 1.0), ("recess", -1.0)] {
            let (mut s, mut k) = (Session::default(), OcctKernel::new());
            ibeam(&mut s, &mut k);
            let sk = rect_above(&mut s, &mut k, 30.0, [x.0, 5.0, x.1, 15.0]);
            run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1, "style": style }));
            near(volume(&mut s, &mut k), v0 + sign * (x.1 - x.0) * 10.0, 1e-6);
            one_solid(&mut s, &mut k);
        }
    }
    // Right across: the flange, and the base where it shows past the flange, each 10 long.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    ibeam(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 30.0, [5.0, 5.0, 35.0, 15.0]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 })));
    near(volume(&mut s, &mut k), v0 + (20.0 + 5.0 + 5.0) * 10.0, 1e-6);
    let kind = json!({ "type": "emboss", "sketch": sk, "style": "recess", "depth": 1.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": e, "kind": kind }));
    near(volume(&mut s, &mut k), v0 - (20.0 + 5.0 + 5.0) * 10.0, 1e-6);
    // A recess as deep as the flange and more goes on down under the flange, where it was seen.
    let kind = json!({ "type": "emboss", "sketch": sk, "style": "recess", "depth": 3.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": e, "kind": kind }));
    near(volume(&mut s, &mut k), v0 - (20.0 + 5.0 + 5.0) * 10.0 * 3.0, 1e-6);

    // A round thing over a floor: a shaft of radius 5 on a rib, 8 over a plate. The shaft's top half
    // and the plate past the shaft's round take a layer; the plate under the shaft does not.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let plate = rect_above(&mut s, &mut k, -10.0, [0.0, -20.0, 50.0, 20.0]);
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": plate, "distance": 2 }));
    let rib = rect_above(&mut s, &mut k, -8.0, [0.0, -1.0, 50.0, 1.0]);
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": rib, "distance": 8 }));
    let sk0 = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "yz" })));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk0, "cx": 0, "cy": 0, "r": 5 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk0, "distance": 50 }));
    let v0 = volume(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 6.0, [10.0, -10.0, 20.0, 10.0]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 })));
    let plate_layer = 2.0 * (10.0 - 5.0) * 10.0;
    near(volume(&mut s, &mut k), v0 + 10.0 * PI * (36.0 - 25.0) / 2.0 + plate_layer, 1e-2);
    let kind = json!({ "type": "emboss", "sketch": sk, "style": "recess", "depth": 1.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": e, "kind": kind }));
    near(volume(&mut s, &mut k), v0 - 10.0 * PI * (25.0 - 16.0) / 2.0 - plate_layer, 1e-2);
}

#[test]
fn from_the_plane_what_would_come_loose_or_shut_a_hollow_is_refused() {
    // The plane on the top face, flipped: added above the face, taken below it, the two touching
    // only round the profile's edge.
    for r in [[5.0, 5.0, 15.0, 11.0], [10.0, 10.0, 30.0, 30.0]] {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        block(&mut s, &mut k);
        let sk = rect_above(&mut s, &mut k, 10.0, r);
        run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 5, "style": "from_plane", "reverse": true }));
        let said = fails(&mut s, &mut k);
        assert!(said.contains("comes loose") && said.contains("Flip"), "{said}");
        // The part stays as it was, one solid.
        let t = run(&mut s, &mut k, "model.topology", json!({}));
        assert_eq!(t["bodies"][0]["solids"], 1, "{t}");
        near(run(&mut s, &mut k, "model.mass", json!({}))["bodies"][0]["volume"].as_f64().unwrap(), 16000.0, 1e-6);
    }
    // Not flipped, it changes nothing, and says so.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 10.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 5, "style": "from_plane" }));
    let said = fails(&mut s, &mut k);
    assert!(said.contains("Nothing is added") && said.contains("off the face"), "{said}");
    // The plane inside the block, flipped: a plug on top of a closed space.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 8.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 5, "style": "from_plane", "reverse": true }));
    let said = fails(&mut s, &mut k);
    assert!(said.contains("hollow") && said.contains("flip"), "{said}");
    near(run(&mut s, &mut k, "model.mass", json!({}))["bodies"][0]["volume"].as_f64().unwrap(), 16000.0, 1e-6);
    // The plane under the block, flipped: it looks down from under it, and is a land there. What
    // reaches the part goes up from it to the part's underside, on the far side of the plane.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, -2.0, [5.0, 5.0, 15.0, 11.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 5, "style": "from_plane", "reverse": true }));
    near(volume(&mut s, &mut k), 16000.0 + 60.0 * 2.0, 1e-6);
    one_solid(&mut s, &mut k);
}

#[test]
fn wrapped_it_stays_on_the_face() {
    let (r, d, len) = (10.0, 2.0, 50.0);
    let v0 = PI * r * r * len;
    let on_shaft = |s: &mut Session, k: &mut OcctKernel, x0: f64, x1: f64, style: &str| {
        shaft(s, k, r, len);
        let face = run(s, k, "model.face_ref", json!({ "at": [25.0, r, 0.0] }));
        let sk = rect_above(s, k, 15.0, [x0, -5.0, x1, 5.0]);
        feature(&run(s, k, "model.emboss", json!({ "sketch": sk, "depth": d, "wrap": face, "style": style })))
    };
    let raised = |x: f64| x * (10.0 / r) * ((r + d) * (r + d) - r * r) / 2.0;
    let recessed = |x: f64| x * (10.0 / r) * (r * r - (r - d) * (r - d)) / 2.0;
    // Half past the end: as a raise from a face, it stops at the face's edge.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    on_shaft(&mut s, &mut k, 40.0, 60.0, "raise");
    near(volume(&mut s, &mut k), v0 + raised(10.0), 1e-2);
    one_solid(&mut s, &mut k);
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    near(t["bodies"][0]["bbox"]["max"][0].as_f64().unwrap(), len, 1e-6);
    // Recessed, the same.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    on_shaft(&mut s, &mut k, 40.0, 60.0, "recess");
    near(volume(&mut s, &mut k), v0 - recessed(10.0), 1e-2);
    one_solid(&mut s, &mut k);
    // Wholly off it: refused in words, the part as it was.
    for style in ["raise", "recess"] {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        on_shaft(&mut s, &mut k, 60.0, 80.0, style);
        let said = fails(&mut s, &mut k);
        assert!(said.contains("lies off the face") && said.contains("move the sketch"), "{said}");
        near(run(&mut s, &mut k, "model.mass", json!({}))["bodies"][0]["volume"].as_f64().unwrap(), v0, 1e-2);
    }
    // A hole's wall stops at its end as well: a tube 50 long, the profile half past it.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk0 = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "yz" })));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk0, "cx": 0, "cy": 0, "r": 15 }));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk0, "cx": 0, "cy": 0, "r": 10 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk0, "distance": 50 }));
    let v0 = PI * (225.0 - 100.0) * 50.0;
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, 10.0, 0.0] }));
    let sk = rect_above(&mut s, &mut k, 5.0, [40.0, -5.0, 60.0, 5.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": d, "wrap": face, "style": "recess" }));
    near(volume(&mut s, &mut k), v0 - 10.0 * (10.0 / 10.0) * (144.0 - 100.0) / 2.0, 1e-2);
    one_solid(&mut s, &mut k);

    // A cone: a square round the generator half past its wide end, whose rim is at r = 40 from the
    // apex at 40 up: what is left weighs less than the whole would, and none of it stands past the
    // base.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk0 = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" })));
    let a = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk0, "x1": 0, "y1": 0, "x2": 40, "y2": 0 }));
    let b = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk0, "start": a["end"], "x2": 10, "y2": 30 }));
    let c = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk0, "start": b["end"], "x2": 0, "y2": 30 }));
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk0, "start": c["end"], "end": a["start"] }));
    run(&mut s, &mut k, "model.revolve", json!({ "sketch": sk0, "axis": "z" }));
    let v0 = PI * 30.0 / 3.0 * (1600.0 + 400.0 + 100.0);
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [25.0, 0.0, 15.0] }));
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 25, "z": 15, "ry": FRAC_PI_2 / 2.0 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "ucs_plane": { "ucs": u, "plane": "xy" } })));
    // Local X runs down the slope: the generator leaves the cone at 15 sqrt 2 from its middle.
    let past = 15.0 * 2f64.sqrt();
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": past - 5.0, "y1": -3, "x2": past + 5.0, "y2": 3 }));
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 0.5, "wrap": face }));
    let v = volume(&mut s, &mut k) - v0;
    assert!(v > 0.5 * 5.0 * 6.0 * 0.9 && v < 0.5 * 5.0 * 6.0 * 1.1, "half the 10 x 6, 0.5 up: {v}");
    let t = run(&mut s, &mut k, "model.topology", json!({}));
    near(t["bodies"][0]["bbox"]["min"][2].as_f64().unwrap(), 0.0, 1e-6);
    one_solid(&mut s, &mut k);
}

#[test]
fn it_goes_on_the_last_solid_it_falls_on_and_a_taper_is_hinged_at_the_plane() {
    // Two solids side by side, the profile across both: only the last, 3 x 10 of it, 1 up. (Hand,
    // both would be 2060.)
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let a = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": a, "x1": 0, "y1": 0, "x2": 10, "y2": 10 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": a, "distance": 10 }));
    let b = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": b, "x1": 20, "y1": 0, "x2": 30, "y2": 10 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": b, "distance": 10, "operation": "new_body" }));
    let sk = rect_above(&mut s, &mut k, 10.0, [7.0, 0.0, 23.0, 10.0]);
    let e = feature(&run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 })));
    near(volume(&mut s, &mut k), 2000.0 + 3.0 * 10.0, 1e-6);
    // A profile on the first alone, the box round it reaching the last: on the first.
    let sk2 = rect_above(&mut s, &mut k, 10.0, [3.0, 2.0, 8.0, 8.0]);
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": e }));
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk2, "depth": 7 }));
    near(volume(&mut s, &mut k), 2000.0 + 5.0 * 6.0 * 7.0, 1e-6);

    // A plane 20 above the face, a taper of 10 degrees, 2 up: hinged at the plane, its foot on the
    // face is 2 x 20 tan 10 wider than the profile, and its top 2 x 18 tan 10.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 30.0, [15.0, 15.0, 25.0, 25.0]);
    let t = 10f64.to_radians();
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 2, "taper": t }));
    let (foot, top) = (10.0 + 2.0 * 20.0 * t.tan(), 10.0 + 2.0 * 18.0 * t.tan());
    let hand = 2.0 / 3.0 * (foot * foot + top * top + foot * top);
    near(volume(&mut s, &mut k), 16000.0 + hand, 1e-3);
}

#[test]
fn what_falls_short_of_the_part_says_so_and_changes_nothing() {
    // The box round the profile reaches the block, the profile does not.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 12.0, [41.0, 5.0, 50.0, 15.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 }));
    let said = fails(&mut s, &mut k);
    assert!(said.contains("does not fall on the faces") && said.contains("move the sketch"), "{said}");
    // Over a through hole in a plate, the same.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let hole = rect_above(&mut s, &mut k, 10.0, [10.0, 10.0, 30.0, 30.0]);
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": hole, "distance": -10, "operation": "cut" }));
    let sk = rect_above(&mut s, &mut k, 12.0, [15.0, 15.0, 25.0, 25.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 5 }));
    let said = fails(&mut s, &mut k);
    assert!(said.contains("does not fall on the faces"), "{said}");
    // A shallow pocket's floor is seen: the same profile lands on it.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let hole = rect_above(&mut s, &mut k, 10.0, [10.0, 10.0, 30.0, 30.0]);
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": hole, "distance": -4, "operation": "cut" }));
    let v0 = volume(&mut s, &mut k);
    let sk = rect_above(&mut s, &mut k, 12.0, [15.0, 15.0, 25.0, 25.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 }));
    near(volume(&mut s, &mut k), v0 + 100.0, 1e-6);
    // Half on the rim, half on the floor.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    let hole = rect_above(&mut s, &mut k, 10.0, [10.0, 10.0, 30.0, 30.0]);
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": hole, "distance": -4, "operation": "cut" }));
    let sk = rect_above(&mut s, &mut k, 12.0, [5.0, 15.0, 15.0, 25.0]);
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1 }));
    near(volume(&mut s, &mut k), v0 + 100.0, 1e-6);
}
