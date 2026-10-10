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

/// The threads on the part.
fn threads(s: &mut Session, k: &mut OcctKernel) -> Vec<Value> {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    run(s, k, "model.threads", json!({}))["threads"].as_array().unwrap().clone()
}

#[test]
fn a_thread_marks_its_face_or_cuts_its_groove_and_follows_the_diameter() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    // A shaft of diameter 8, 20 long.
    let (sk, ring) = circle(&mut s, &mut k, "xy", 0.0, 0.0, 4.0);
    let shaft = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 20 }))["feature"].as_u64().unwrap();
    let wall = json!({ "type": "side", "feature": shaft, "curve": ring });
    let plain = PI * 16.0 * 20.0;
    let on = face(&mut s, &mut k, wall.clone());
    let thread = run(&mut s, &mut k, "model.thread", json!({ "face": on }))["feature"].as_u64().unwrap();
    // Cosmetic: the part is the same cylinder, and the thread is on record, sized from its face.
    assert!(near(volume(&mut s, &mut k), plain, 1e-9));
    let t = threads(&mut s, &mut k);
    assert_eq!(t.len(), 1);
    assert_eq!((t[0]["designation"].as_str(), t[0]["pitch"].as_f64(), t[0]["diameter"].as_f64()), (Some("M8x1.25"), Some(1.25), Some(8.0)));
    assert_eq!((t[0]["internal"].as_bool(), t[0]["modelled"].as_bool(), t[0]["length"].as_f64()), (Some(false), Some(false), Some(20.0)));
    // Both ends of this shaft are open: the thread starts at the top and runs down.
    assert_eq!((t[0]["start"][2].as_f64(), t[0]["direction"][2].as_f64()), (Some(20.0), Some(-1.0)));
    // The shaft made thicker: the thread is the next size up without being told.
    let r = run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "radius", "curve": ring, "value": 4.0 } }));
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": sk, "constraint": r["constraint"], "value": 5.0 }));
    assert_eq!(threads(&mut s, &mut k)[0]["designation"], "M10x1.5");
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": sk, "constraint": r["constraint"], "value": 4.0 }));
    // Modelled, 10 long from the free end: a groove of the basic profile, eight turns of it.
    let at = s.document().features().iter().position(|f| u64::from(f.id.0) == thread).unwrap();
    let mut kind = serde_json::to_value(&s.document().features()[at].kind).unwrap();
    kind["modelled"] = json!(true);
    kind["length"] = json!({ "distance": 10.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": thread, "kind": kind }));
    // Its section is a trapezoid 7/8 of the pitch wide at the shaft and 1/4 at the root, 0.5413
    // of the pitch deep; each turn removes that area times the way its centre goes round.
    let (p, depth) = (1.25, 0.625 * 0.866_025_403_784_438_6 * 1.25);
    let area = (0.875 + 0.25) / 2.0 * p * depth;
    let centre = 4.0 - depth * (0.875 + 2.0 * 0.25) / (3.0 * (0.875 + 0.25));
    let per_turn = area * 2.0 * PI * centre;
    let removed = plain - volume(&mut s, &mut k);
    assert!(removed > 7.0 * per_turn && removed < 9.0 * per_turn, "about eight turns: {removed} vs {per_turn} a turn");
    // What is left of the shaft's wall keeps its name, and the groove's root is named after the
    // thread: later features can hold both.
    assert!(named(&mut s, &mut k, wall.clone()));
    assert!(named(&mut s, &mut k, json!({ "type": "side", "feature": thread, "curve": 3 })));
    let t = threads(&mut s, &mut k);
    assert_eq!((t[0]["modelled"].as_bool(), t[0]["length"].as_f64()), (Some(true), Some(10.0)));
    // Left-handed removes the same; a longer shaft keeps its ten millimetres of thread.
    kind["left"] = json!(true);
    run(&mut s, &mut k, "feature.update", json!({ "feature": thread, "kind": kind }));
    let left = plain - volume(&mut s, &mut k);
    assert!(near(left, removed, 1e-3), "{left} vs {removed}");
    let mut longer = serde_json::to_value(&s.document().features()[1].kind).unwrap();
    longer["extent"]["distance"] = json!(30.0);
    run(&mut s, &mut k, "feature.update", json!({ "feature": shaft, "kind": longer }));
    let on_longer = PI * 16.0 * 30.0 - volume(&mut s, &mut k);
    assert!(near(on_longer, removed, 1e-3), "{on_longer} vs {removed}");
    // The whole length: the groove runs out of both free ends.
    kind["length"] = json!("full");
    run(&mut s, &mut k, "feature.update", json!({ "feature": thread, "kind": kind }));
    let all = PI * 16.0 * 30.0 - volume(&mut s, &mut k);
    assert!(near(all, 24.0 * per_turn, 0.02), "thirty millimetres are twenty-four turns: {all} vs {}", 24.0 * per_turn);

    // Failures say what to do: a thread longer than its face, a pitch too coarse for the shaft,
    // a face that is not round.
    kind["length"] = json!({ "distance": 50.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": thread, "kind": kind }));
    let m = failure(&mut s, &mut k);
    assert!(m.starts_with("The thread is 50 mm long but its face is only 30 mm long"), "{m}");
    kind["length"] = json!("full");
    kind["pitch"] = json!(6.0);
    run(&mut s, &mut k, "feature.update", json!({ "feature": thread, "kind": kind }));
    let m = failure(&mut s, &mut k);
    assert!(m.starts_with("A 6 mm pitch is too coarse for a 8 mm shaft"), "{m}");
    kind["pitch"] = Value::Null;
    run(&mut s, &mut k, "feature.update", json!({ "feature": thread, "kind": kind }));
    let end = face(&mut s, &mut k, json!({ "type": "cap", "feature": shaft, "end": "end" }));
    run(&mut s, &mut k, "model.thread", json!({ "face": end }));
    assert!(failure(&mut s, &mut k).starts_with("A thread goes on a cylindrical face"));
    let e = s.exec("model.thread", &json!({ "face": end, "pitch": 0.0 }), Some(&mut k)).unwrap_err().0;
    assert!(e.contains("0.05 to 50 mm"), "{e}");
}

#[test]
fn a_thread_in_a_hole_is_sized_from_its_tap_drill_and_cut_outwards() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k, 10.0);
    // A 6.8 hole through the block: the drill for M8.
    let (sk, ring) = circle(&mut s, &mut k, "xy", 0.0, 0.0, 3.4);
    let hole = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 10, "operation": "cut" }))["feature"].as_u64().unwrap();
    let drilled = 4000.0 - PI * 3.4 * 3.4 * 10.0;
    assert!(near(volume(&mut s, &mut k), drilled, 1e-9));
    let wall = face(&mut s, &mut k, json!({ "type": "side", "feature": hole, "curve": ring }));
    let thread = run(&mut s, &mut k, "model.thread", json!({ "face": wall, "modelled": true }))["feature"].as_u64().unwrap();
    let t = threads(&mut s, &mut k);
    assert_eq!((t[0]["designation"].as_str(), t[0]["internal"].as_bool()), (Some("M8x1.25"), Some(true)));
    assert_eq!(t[0]["diameter"].as_f64(), Some(8.0), "the nominal size its tap drill is for");
    // The groove is cut into the wall, outwards: 3/4 of the pitch wide at the hole, 1/8 at its
    // root, eight turns through the block.
    let (p, depth) = (1.25, 0.625 * 0.866_025_403_784_438_6 * 1.25);
    let area = (0.75 + 0.125) / 2.0 * p * depth;
    let centre = 3.4 + depth * (0.75 + 2.0 * 0.125) / (3.0 * (0.75 + 0.125));
    let removed = drilled - volume(&mut s, &mut k);
    assert!(near(removed, 8.0 * area * 2.0 * PI * centre, 0.02), "{removed}");
    assert!(named(&mut s, &mut k, json!({ "type": "side", "feature": thread, "curve": 2 })));
    // A blind hole: the groove stops at the bottom instead of cutting on into the block.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k, 20.0);
    let top = run(&mut s, &mut k, "work.plane", json!({ "by": "offset", "base": "xy", "distance": 20 }))["feature"].clone();
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "work_plane": top, "project_origin": false }))["feature"].as_u64().unwrap();
    let ring = run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 3.4 }))["circle"].as_u64().unwrap();
    let hole = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 10, "reverse": true, "operation": "cut" }))["feature"]
        .as_u64()
        .unwrap();
    let blind = 8000.0 - PI * 3.4 * 3.4 * 10.0;
    assert!(near(volume(&mut s, &mut k), blind, 1e-9), "{}", volume(&mut s, &mut k));
    let wall = face(&mut s, &mut k, json!({ "type": "side", "feature": hole, "curve": ring }));
    run(&mut s, &mut k, "model.thread", json!({ "face": wall, "modelled": true }));
    let removed = blind - volume(&mut s, &mut k);
    let per_turn = area * 2.0 * PI * centre;
    assert!(
        removed > 6.5 * per_turn && removed < 8.0 * per_turn,
        "it runs in at the mouth and stops short of the bottom: {removed} vs {per_turn} a turn"
    );
    // A thread starts at its face's open end: this hole's mouth, at the top.
    let t = threads(&mut s, &mut k);
    assert_eq!((t[0]["start"][2].as_f64(), t[0]["direction"][2].as_f64()), (Some(20.0), Some(-1.0)));
    // A hole drilled up from underneath opens downwards, and its thread starts there.
    let (under, small) = circle(&mut s, &mut k, "xy", 6.0, 6.0, 1.25);
    let up = run(&mut s, &mut k, "model.extrude", json!({ "sketch": under, "distance": 6, "operation": "cut" }))["feature"].as_u64().unwrap();
    let wall = face(&mut s, &mut k, json!({ "type": "side", "feature": up, "curve": small }));
    run(&mut s, &mut k, "model.thread", json!({ "face": wall }));
    let t = threads(&mut s, &mut k);
    assert_eq!((t[1]["designation"].as_str(), t[1]["direction"][2].as_f64()), (Some("M3x0.5"), Some(1.0)));
    assert!(t[1]["start"][2].as_f64().unwrap().abs() < 1e-9);
    // Told to start from the other end, it does.
    let last = s.document().features().last().unwrap().clone();
    let mut kind = serde_json::to_value(&last.kind).unwrap();
    kind["reverse"] = json!(true);
    run(&mut s, &mut k, "feature.update", json!({ "feature": last.id.0, "kind": kind }));
    assert!((threads(&mut s, &mut k)[1]["start"][2].as_f64().unwrap() - 6.0).abs() < 1e-9);
}

#[test]
fn a_tapered_extrusion_leans_its_sides_in_or_out_from_its_sketch() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let lines = run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": -10, "y1": -10, "x2": 10, "y2": 10 }))["lines"]
        .as_array()
        .unwrap()
        .clone();
    let angle = 5f64.to_radians();
    let ext = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 10, "taper": angle }))["feature"].as_u64().unwrap();
    // The sketch's square at the bottom, a smaller one at the top.
    let top = |h: f64, a: f64| 20.0 - 2.0 * h * a.tan();
    assert!(near(volume(&mut s, &mut k), frustum(20.0, top(10.0, angle), 10.0), 1e-9), "{}", volume(&mut s, &mut k));
    // Its faces are named as a straight extrusion's are.
    for l in &lines {
        assert_eq!(faces_named(&mut s, &mut k, &json!({ "type": "side", "feature": ext, "curve": l })), 1);
    }
    assert!(named(&mut s, &mut k, json!({ "type": "cap", "feature": ext, "end": "start" })));
    assert!(named(&mut s, &mut k, json!({ "type": "cap", "feature": ext, "end": "end" })));
    // The taper is a parameter, in degrees; negative leans the sides out.
    run(&mut s, &mut k, "param.set", json!({ "name": "d1", "equation": "8 deg" }));
    assert!(near(volume(&mut s, &mut k), frustum(20.0, top(10.0, 8f64.to_radians()), 10.0), 1e-9));
    run(&mut s, &mut k, "param.set", json!({ "name": "d1", "equation": "-8 deg" }));
    assert!(near(volume(&mut s, &mut k), frustum(20.0, top(10.0, -(8f64.to_radians())), 10.0), 1e-9));
    // A tapered pocket: a round hole 10 deep that narrows from 4 to 4 - 10 tan(5 deg).
    run(&mut s, &mut k, "param.set", json!({ "name": "d1", "equation": 0.0 }));
    assert!(near(volume(&mut s, &mut k), 4000.0, 1e-9));
    let (hole, _) = circle(&mut s, &mut k, "xy", 0.0, 0.0, 4.0);
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": hole, "distance": 10, "taper": angle, "operation": "cut" }));
    let (r1, r2) = (4.0, 4.0 - 10.0 * angle.tan());
    let cone = PI * 10.0 / 3.0 * (r1 * r1 + r1 * r2 + r2 * r2);
    assert!(near(volume(&mut s, &mut k), 4000.0 - cone, 1e-9), "{}", volume(&mut s, &mut k));
    // Too steep for its height, the sides would cross: said in plain words.
    run(&mut s, &mut k, "param.set", json!({ "name": "d1", "equation": "80 deg" }));
    let m = failure(&mut s, &mut k);
    assert!(m.starts_with("The extrusion could not be tapered by 80 degrees."), "{m}");
    // A taper goes one way from the sketch.
    let e = s.exec("model.extrude", &json!({ "sketch": sk, "symmetric": 10, "taper": angle }), Some(&mut k)).unwrap_err().0;
    assert!(e.starts_with("A tapered extrusion goes one way"), "{e}");
    let e = s.exec("model.extrude", &json!({ "sketch": sk, "distance": 10, "taper": 1.55 }), Some(&mut k)).unwrap_err().0;
    assert!(e.contains("at most 85 degrees"), "{e}");
}

#[test]
fn a_circle_turned_about_a_line_through_it_is_a_sphere() {
    let ball = 4.0 / 3.0 * PI * 1000.0;
    for (plane, axis) in [("xy", "y"), ("xy", "x"), ("xz", "z"), ("yz", "y")] {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        let (sk, ring) = circle(&mut s, &mut k, plane, 0.0, 0.0, 10.0);
        let rev = run(&mut s, &mut k, "model.revolve", json!({ "sketch": sk, "axis": axis }))["feature"].as_u64().unwrap();
        assert!(near(volume(&mut s, &mut k), ball, 1e-9), "{plane} about {axis}: {}", volume(&mut s, &mut k));
        // The ball's face is named after the circle, so later features can hold it.
        assert!(named(&mut s, &mut k, json!({ "type": "side", "feature": rev, "curve": ring })));
    }
    // About a line drawn through the circle, off the origin; and the circle made smaller.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (sk, ring) = circle(&mut s, &mut k, "xy", 30.0, 5.0, 10.0);
    let line = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": 15, "y1": 5, "x2": 45, "y2": 5 }))["line"].as_u64().unwrap();
    run(&mut s, &mut k, "model.revolve", json!({ "sketch": sk, "axis": line }));
    assert!(near(volume(&mut s, &mut k), ball, 1e-9), "{}", volume(&mut s, &mut k));
    let r = run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "radius", "curve": ring, "value": 10.0 } }));
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": sk, "constraint": r["constraint"], "value": 5.0 }));
    assert!(near(volume(&mut s, &mut k), ball / 8.0, 1e-9));
    // Part of a turn of a profile lying across its axis has no one meaning: said in plain words.
    let at = s.document().features().len() - 1;
    let id = s.document().features()[at].id.0;
    let mut kind = serde_json::to_value(&s.document().features()[at].kind).unwrap();
    kind["angle"] = json!({ "angle": 1.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": id, "kind": kind }));
    let m = failure(&mut s, &mut k);
    assert!(m.starts_with("For part of a turn the profile must be on one side of the axis"), "{m}");
    // An axis square to the sketch makes no solid: said, where the kernel returned nothing at all.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (sk, _) = circle(&mut s, &mut k, "xz", 20.0, 0.0, 5.0);
    run(&mut s, &mut k, "model.revolve", json!({ "sketch": sk, "axis": "y" }));
    let m = failure(&mut s, &mut k);
    assert!(m.starts_with("The axis is square to the sketch"), "{m}");
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
