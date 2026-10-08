//! M2 features through commands: fillet, chamfer and shell, with edge and face references that
//! survive upstream edits.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};
use tenon_geom::tol;
use tenon_kernel_occt::OcctKernel;
use tenon_model::{Document, Session};

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let m = run(s, k, "model.mass", json!({}));
    m["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

fn approx(a: f64, b: f64) -> bool {
    tol::rel_eq(a, b, 1e-6)
}

fn regenerates(s: &mut Session, k: &mut OcctKernel) {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
}

/// A block `w` x `d` x `h` from a dimensioned rectangle. Returns (sketch, extrusion, rectangle
/// lines [bottom, right, top, left], width dimension).
fn block(s: &mut Session, k: &mut OcctKernel, w: f64, d: f64, h: f64) -> (u64, u64, Vec<u64>, u64) {
    let sk = run(s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    let lines: Vec<u64> = run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": w, "y2": d }))["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    let width =
        run(s, k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": lines[0], "value": w } }))["constraint"]
            .as_u64()
            .unwrap();
    let ex = run(s, k, "model.extrude", json!({ "sketch": sk, "distance": h }))["feature"].as_u64().unwrap();
    (sk, ex, lines, width)
}

fn side(ex: u64, line: u64) -> Value {
    json!({ "type": "side", "feature": ex, "curve": line })
}

fn top(ex: u64) -> Value {
    json!({ "type": "cap", "feature": ex, "end": "end" })
}

#[test]
fn fillets_follow_upstream_edits() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (sk, ex, l, width) = block(&mut s, &mut k, 40.0, 20.0, 10.0);
    // The four vertical edges, each named by the two side faces it joins.
    let edges: Vec<Value> =
        (0..4).map(|i| run(&mut s, &mut k, "model.edge_ref", json!({ "faces": [side(ex, l[i]), side(ex, l[(i + 1) % 4])] }))).collect();
    let fillet = run(&mut s, &mut k, "model.fillet", json!({ "edges": edges, "radius": 3.0 }))["feature"].as_u64().unwrap();
    let rounded = |w: f64, d: f64, h: f64, r: f64| (w * d - (4.0 - PI) * r * r) * h;
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), rounded(40.0, 20.0, 10.0, 3.0)), "{}", volume(&mut s, &mut k));
    let topo = run(&mut s, &mut k, "model.topology", json!({}));
    assert_eq!(topo["bodies"][0]["faces"], 10, "6 + 4 rounds: {topo}");

    // Widen the sketch: the edges are found again by their faces, the rounds follow.
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": sk, "constraint": width, "value": 50.0 }));
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), rounded(50.0, 20.0, 10.0, 3.0)), "{}", volume(&mut s, &mut k));

    // The round faces have names: a sketch can sit on a face made by the fillet's neighbour, and
    // the top face keeps its name through the fillet.
    let faces = run(&mut s, &mut k, "model.faces", json!({}));
    let made_by_fillet = faces["faces"].as_array().unwrap().iter().filter(|f| f["name"]["type"] == "from" && f["name"]["feature"] == fillet).count();
    assert_eq!(made_by_fillet, 4, "{faces}");
    let top_ref = run(&mut s, &mut k, "model.face_ref", json!({ "origin": top(ex) }));
    let sk2 = run(&mut s, &mut k, "sketch.create", json!({ "face": top_ref }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk2, "cx": 25, "cy": 10, "r": 2 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk2, "through_all": true, "reverse": true, "operation": "cut" }));
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), rounded(50.0, 20.0, 10.0, 3.0) - PI * 4.0 * 10.0));

    // Edit the fillet radius through its definition.
    let mut kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(fillet as u32)).unwrap().kind).unwrap();
    kind["radius"] = json!(2.0);
    run(&mut s, &mut k, "feature.update", json!({ "feature": fillet, "kind": kind }));
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), rounded(50.0, 20.0, 10.0, 2.0) - PI * 4.0 * 10.0));

    // The whole document survives a file round trip.
    let text = serde_json::to_string(s.document()).unwrap();
    let back: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(&back, s.document());
}

#[test]
fn chamfers_meet_at_the_corners() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, ex, l, _) = block(&mut s, &mut k, 30.0, 30.0, 20.0);
    let edges: Vec<Value> = (0..4).map(|i| run(&mut s, &mut k, "model.edge_ref", json!({ "faces": [top(ex), side(ex, l[i])] }))).collect();
    run(&mut s, &mut k, "model.chamfer", json!({ "edges": edges, "distance": 2.0 }));
    regenerates(&mut s, &mut k);
    // Four 2 x 2 triangular prisms along the top edges, overlapping in a d^3/3 piece at each corner.
    let expected = 30.0 * 30.0 * 20.0 - 4.0 * (2.0 * 2.0 / 2.0 * 30.0) + 4.0 * 8.0 / 3.0;
    assert!(approx(volume(&mut s, &mut k), expected), "{} vs {expected}", volume(&mut s, &mut k));
}

#[test]
fn shell_follows_its_open_face() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, ex, _, _) = block(&mut s, &mut k, 30.0, 30.0, 20.0);
    let open = run(&mut s, &mut k, "model.face_ref", json!({ "origin": top(ex) }));
    run(&mut s, &mut k, "model.shell", json!({ "remove": [open], "thickness": 2.0 }));
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), 30.0 * 30.0 * 20.0 - 26.0 * 26.0 * 18.0), "{}", volume(&mut s, &mut k));
    // Taller block: the open face is still the top.
    let mut kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(ex as u32)).unwrap().kind).unwrap();
    kind["extent"] = json!({ "distance": 25.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": ex, "kind": kind }));
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), 30.0 * 30.0 * 25.0 - 26.0 * 26.0 * 23.0), "{}", volume(&mut s, &mut k));
}

#[test]
fn a_lost_edge_breaks_the_fillet_with_a_clear_message() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (sk, ex, l, _) = block(&mut s, &mut k, 40.0, 20.0, 10.0);
    let edge = run(&mut s, &mut k, "model.edge_ref", json!({ "faces": [side(ex, l[0]), side(ex, l[1])] }));
    let fillet = run(&mut s, &mut k, "model.fillet", json!({ "edges": [edge], "radius": 2.0 }))["feature"].as_u64().unwrap();
    regenerates(&mut s, &mut k);
    // Replace the bottom line: the face it swept, and so the edge, are gone.
    run(&mut s, &mut k, "sketch.delete", json!({ "sketch": sk, "entities": [l[0]] }));
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }));
    let ents = info["entities"].as_array().unwrap();
    let loose: Vec<u64> = ents
        .iter()
        .filter(|e| e["type"] == "point" && e["construction"] == false)
        .map(|e| e["id"].as_u64().unwrap())
        .filter(|p| ents.iter().filter(|e| e["type"] == "line" && (e["start"] == *p || e["end"] == *p)).count() == 1)
        .collect();
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "start": loose[0], "end": loose[1] }));
    let r = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert_eq!(r["error"]["feature"], fillet, "{r}");
    let msg = r["error"]["message"].as_str().unwrap();
    assert!(msg.contains("no longer exists") && msg.contains("Extrusion1"), "{msg}");
}

#[test]
fn edges_are_listed_with_the_faces_they_join() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, ex, l, _) = block(&mut s, &mut k, 40.0, 20.0, 10.0);
    let edges = run(&mut s, &mut k, "model.edges", json!({}));
    let list = edges["edges"].as_array().unwrap();
    assert_eq!(list.len(), 12);
    assert!(list.iter().all(|e| e["faces"].is_array()), "every edge of a block joins two named faces: {edges}");
    // Ambiguity is refused: there is exactly one edge between two adjacent sides, none between
    // opposite ones.
    let opposite = s.exec("model.edge_ref", &json!({ "faces": [side(ex, l[0]), side(ex, l[2])] }), Some(&mut k));
    assert!(opposite.unwrap_err().0.contains("no edge"));
    // Bad input to the features is refused without changing the document.
    let before = s.document().clone();
    assert!(s.exec("model.fillet", &json!({ "edges": [], "radius": 1.0 }), Some(&mut k)).is_err());
    assert!(s.exec("model.shell", &json!({ "thickness": "x" }), Some(&mut k)).is_err());
    assert_eq!(s.document(), &before);
}
