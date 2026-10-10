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

/// How many faces of the part, over all its bodies, carry the name `origin`.
fn faces_named(s: &mut Session, k: &mut OcctKernel, origin: &Value) -> usize {
    let bodies = run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().len();
    (0..bodies)
        .map(|b| run(s, k, "model.faces", json!({ "body": b }))["faces"].as_array().unwrap().iter().filter(|f| f["name"] == *origin).count())
        .sum()
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

/// A 20 x 20 block `h` tall on the XY plane, centred on the origin; returns (sketch, the
/// rectangle's lines, extrusion).
fn block(s: &mut Session, k: &mut OcctKernel, h: f64) -> (u64, Vec<Value>, u64) {
    let sk = run(s, k, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let lines = run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": -10, "y1": -10, "x2": 10, "y2": 10 }))["lines"].as_array().unwrap().clone();
    let ext = run(s, k, "model.extrude", json!({ "sketch": sk, "distance": h }))["feature"].as_u64().unwrap();
    (sk, lines, ext)
}

/// A reference to the face named `origin`.
fn face(s: &mut Session, k: &mut OcctKernel, origin: Value) -> Value {
    run(s, k, "model.face_ref", json!({ "origin": origin }))
}

fn volumes(s: &mut Session, k: &mut OcctKernel) -> Vec<f64> {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).collect()
}

/// The volume of a square frustum `h` tall between squares of sides `a` and `b`.
fn frustum(a: f64, b: f64, h: f64) -> f64 {
    h / 3.0 * (a * a + b * b + a * b)
}

#[test]
fn a_draft_tilts_its_faces_about_the_neutral_plane_and_keeps_their_names() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, lines, ext) = block(&mut s, &mut k, 10.0);
    let sides: Vec<Value> = lines.iter().map(|l| face(&mut s, &mut k, json!({ "type": "side", "feature": ext, "curve": l }))).collect();
    let angle = 5f64.to_radians();
    let draft = run(&mut s, &mut k, "model.draft", json!({ "faces": sides, "plane": "xy", "angle": angle }))["feature"].as_u64().unwrap();
    // The sides lean in as they rise from the XY plane, where they stay put: a frustum.
    let top = |h: f64, a: f64| 20.0 - 2.0 * h * a.tan();
    assert!(near(volume(&mut s, &mut k), frustum(20.0, top(10.0, angle), 10.0), 1e-9), "{}", volume(&mut s, &mut k));
    // Every face keeps its name, so later features hold them: the top is opened by a shell.
    for l in &lines {
        assert_eq!(faces_named(&mut s, &mut k, &json!({ "type": "side", "feature": ext, "curve": l })), 1);
    }
    let lid = face(&mut s, &mut k, json!({ "type": "cap", "feature": ext, "end": "end" }));
    run(&mut s, &mut k, "model.shell", json!({ "remove": [lid], "thickness": 1 }));
    let drafted = frustum(20.0, top(10.0, angle), 10.0);
    let v = volume(&mut s, &mut k);
    assert!(v > 0.2 * drafted && v < 0.5 * drafted, "a hollow frustum: {v}");
    run(&mut s, &mut k, "edit.undo", json!({}));
    // The block made taller and the draft steeper, by its parameter: the draft follows.
    let mut kind = serde_json::to_value(&s.document().features()[1].kind).unwrap();
    kind["extent"]["distance"] = json!(15.0);
    run(&mut s, &mut k, "feature.update", json!({ "feature": ext, "kind": kind }));
    run(&mut s, &mut k, "param.set", json!({ "name": "d1", "equation": "8 deg" }));
    let steep = 8f64.to_radians();
    assert!(near(volume(&mut s, &mut k), frustum(20.0, top(15.0, steep), 15.0), 1e-9));
    // The other way, the part widens as it rises.
    let mut kind = serde_json::to_value(&s.document().features()[2].kind).unwrap();
    kind["reverse"] = json!(true);
    run(&mut s, &mut k, "feature.update", json!({ "feature": draft, "kind": kind }));
    assert!(near(volume(&mut s, &mut k), frustum(20.0, 20.0 + 2.0 * 15.0 * steep.tan(), 15.0), 1e-9));
    // A face lying flat on the pull direction cannot be tilted: said in plain words.
    let lid = face(&mut s, &mut k, json!({ "type": "cap", "feature": ext, "end": "end" }));
    run(&mut s, &mut k, "model.draft", json!({ "faces": [lid], "plane": "xy", "angle": angle }));
    let m = failure(&mut s, &mut k);
    assert!(m.starts_with("The 5 degree draft could not be made on this face."), "{m}");
    run(&mut s, &mut k, "edit.undo", json!({}));
    // Angles a mould cannot use, and no plane, are refused when asked for.
    let side = face(&mut s, &mut k, json!({ "type": "side", "feature": ext, "curve": lines[0] }));
    let e = s.exec("model.draft", &json!({ "faces": [side], "plane": "xy", "angle": 1.55 }), Some(&mut k)).unwrap_err().0;
    assert!(e.contains("at most 85 degrees"), "{e}");
    let e = s.exec("model.draft", &json!({ "faces": [side], "angle": 0.1 }), Some(&mut k)).unwrap_err().0;
    assert!(e.contains("name the plane"), "{e}");
}

#[test]
fn a_split_cuts_the_part_along_a_plane_and_names_both_cut_faces() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k, 10.0);
    let plane = run(&mut s, &mut k, "work.plane", json!({ "by": "offset", "base": "xy", "distance": 4 }))["feature"].as_u64().unwrap();
    let split = run(&mut s, &mut k, "model.split", json!({ "work_plane": plane }))["feature"].as_u64().unwrap();
    // Two bodies: the piece behind the plane stays where the body was, the one in front is new.
    let v = volumes(&mut s, &mut k);
    assert!(v.len() == 2 && near(v[0], 1600.0, 1e-9) && near(v[1], 2400.0, 1e-9), "{v:?}");
    // The cut is two faces in one place, named apart: the front piece's and the back piece's.
    let (front_cut, back_cut) =
        (json!({ "type": "cap", "feature": split, "end": "start" }), json!({ "type": "cap", "feature": split, "end": "end" }));
    assert_eq!((faces_named(&mut s, &mut k, &front_cut), faces_named(&mut s, &mut k, &back_cut)), (1, 1));
    // A later feature holds one: the front piece is hollowed, open at its cut face.
    let open = face(&mut s, &mut k, front_cut.clone());
    run(&mut s, &mut k, "model.shell", json!({ "remove": [open], "thickness": 1 }));
    let v = volumes(&mut s, &mut k);
    assert!(near(v[0], 1600.0, 1e-9) && near(v[1], 2400.0 - 18.0 * 18.0 * 5.0, 1e-9), "{v:?}");
    // The plane moved: both pieces and the shell follow.
    run(&mut s, &mut k, "param.set", json!({ "name": "d1", "equation": "5 mm" }));
    let v = volumes(&mut s, &mut k);
    assert!(near(v[0], 2000.0, 1e-9) && near(v[1], 2000.0 - 18.0 * 18.0 * 4.0, 1e-9), "{v:?}");
    // Keeping only the front leaves one body, still hollowed.
    let at = s.document().features().iter().position(|f| u64::from(f.id.0) == split).unwrap();
    let mut kind = serde_json::to_value(&s.document().features()[at].kind).unwrap();
    kind["keep"] = json!("front");
    run(&mut s, &mut k, "feature.update", json!({ "feature": split, "kind": kind }));
    let v = volumes(&mut s, &mut k);
    assert!(v.len() == 1 && near(v[0], 2000.0 - 18.0 * 18.0 * 4.0, 1e-9), "{v:?}");
    // Keeping only the back, the shell's face is gone: it says which.
    kind["keep"] = json!("back");
    run(&mut s, &mut k, "feature.update", json!({ "feature": split, "kind": kind }));
    let m = failure(&mut s, &mut k);
    assert!(m.contains("no longer exists") && m.contains("start face of Split1"), "{m}");
    // A plane that passes the part by splits nothing, and says so.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k, 10.0);
    let far = run(&mut s, &mut k, "work.plane", json!({ "by": "offset", "base": "xy", "distance": 50 }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "model.split", json!({ "work_plane": far }));
    assert!(failure(&mut s, &mut k).starts_with("The split plane does not pass through the part"));
}

#[test]
fn combine_joins_cuts_and_intersects_bodies_and_keeps_their_face_names() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, _, ext) = block(&mut s, &mut k, 10.0);
    // A second body: a cylinder of radius 5 standing on the block's edge, half inside it.
    let (sk, ring) = circle(&mut s, &mut k, "xy", 10.0, 0.0, 5.0);
    let post = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 10, "operation": "new_body" }))["feature"].as_u64().unwrap();
    assert_eq!(volumes(&mut s, &mut k).len(), 2);
    let base = face(&mut s, &mut k, json!({ "type": "cap", "feature": ext, "end": "end" }));
    let tool = face(&mut s, &mut k, json!({ "type": "side", "feature": post, "curve": ring }));
    let combine = run(&mut s, &mut k, "model.combine", json!({ "base": base, "tools": [tool] }))["feature"].as_u64().unwrap();
    let (block_v, half) = (4000.0, PI * 25.0 * 10.0 / 2.0);
    let v = volumes(&mut s, &mut k);
    assert!(v.len() == 1 && near(v[0], block_v + half, 1e-9), "joined into one body: {v:?}");
    // Faces of both bodies keep their names in the result.
    assert!(named(&mut s, &mut k, json!({ "type": "side", "feature": post, "curve": ring })));
    assert!(named(&mut s, &mut k, json!({ "type": "cap", "feature": ext, "end": "start" })));
    // Cut and intersect, and the other body kept.
    let at = s.document().features().iter().position(|f| u64::from(f.id.0) == combine).unwrap();
    let mut kind = serde_json::to_value(&s.document().features()[at].kind).unwrap();
    for (operation, expected) in [("cut", block_v - half), ("intersect", half)] {
        kind["operation"] = json!(operation);
        run(&mut s, &mut k, "feature.update", json!({ "feature": combine, "kind": kind }));
        let v = volumes(&mut s, &mut k);
        assert!(v.len() == 1 && near(v[0], expected, 1e-9), "{operation}: {v:?}");
    }
    kind["operation"] = json!("cut");
    kind["keep_tools"] = json!(true);
    run(&mut s, &mut k, "feature.update", json!({ "feature": combine, "kind": kind }));
    let v = volumes(&mut s, &mut k);
    assert!(v.len() == 2 && near(v[0], block_v - half, 1e-9) && near(v[1], 2.0 * half, 1e-9), "{v:?}");
    // The cylinder made smaller: the cut follows.
    let r = run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "radius", "curve": ring, "value": 5.0 } }));
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": sk, "constraint": r["constraint"], "value": 4.0 }));
    let v = volumes(&mut s, &mut k);
    assert!(near(v[0], block_v - PI * 16.0 * 10.0 / 2.0, 1e-9), "{v:?}");
    // Bodies that do not overlap have nothing in common: said in plain words.
    let (far, _) = circle(&mut s, &mut k, "xy", 100.0, 0.0, 5.0);
    let away = run(&mut s, &mut k, "model.extrude", json!({ "sketch": far, "distance": 10, "operation": "new_body" }))["feature"].as_u64().unwrap();
    let lid = face(&mut s, &mut k, json!({ "type": "cap", "feature": away, "end": "end" }));
    let base = face(&mut s, &mut k, json!({ "type": "cap", "feature": ext, "end": "start" }));
    run(&mut s, &mut k, "model.combine", json!({ "base": base, "tools": [lid], "operation": "intersect" }));
    assert!(failure(&mut s, &mut k).starts_with("The bodies do not overlap"));
    run(&mut s, &mut k, "edit.undo", json!({}));
    // A body with itself, and a new body, are refused.
    let other = face(&mut s, &mut k, json!({ "type": "cap", "feature": ext, "end": "end" }));
    run(&mut s, &mut k, "model.combine", json!({ "base": base, "tools": [other] }));
    assert!(failure(&mut s, &mut k).starts_with("A body cannot be combined with itself"));
    run(&mut s, &mut k, "edit.undo", json!({}));
    let e = s.exec("model.combine", &json!({ "base": base, "tools": [lid], "operation": "new_body" }), Some(&mut k)).unwrap_err().0;
    assert!(e.contains("cannot make a new body"), "{e}");
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
