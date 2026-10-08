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

/// A sketch on the top face of extrusion `ex` with centre points at `pts`.
fn points_on_top(s: &mut Session, k: &mut OcctKernel, ex: u64, pts: &[(f64, f64)]) -> (u64, Vec<u64>) {
    let top_ref = run(s, k, "model.face_ref", json!({ "origin": top(ex) }));
    let sk = run(s, k, "sketch.create", json!({ "face": top_ref }))["feature"].as_u64().unwrap();
    let ids = pts.iter().map(|(x, y)| run(s, k, "sketch.point", json!({ "sketch": sk, "x": x, "y": y }))["point"].as_u64().unwrap()).collect();
    (sk, ids)
}

#[test]
fn holes_simple_counterbore_and_countersink() {
    let cone = |r1: f64, r2: f64, h: f64| PI * h / 3.0 * (r1 * r1 + r1 * r2 + r2 * r2);
    let cases: Vec<(Value, f64)> = vec![
        // Blind, 5 deep, with a 118 degree drill point.
        (json!({ "diameter": 6, "depth": 5 }), PI * 9.0 * 5.0 + cone(3.0, 0.0, 3.0 / (59.0f64).to_radians().tan())),
        (json!({ "diameter": 6, "depth": 5, "flat_bottom": true }), PI * 9.0 * 5.0),
        (json!({ "diameter": 6, "through_all": true }), PI * 9.0 * 10.0),
        (
            json!({ "diameter": 6, "through_all": true, "type": "counterbore", "counterbore_diameter": 10, "counterbore_depth": 3 }),
            PI * 25.0 * 3.0 + PI * 9.0 * 7.0,
        ),
        // A 90 degree countersink from 10 down to 6 is 2 deep.
        (json!({ "diameter": 6, "through_all": true, "type": "countersink", "countersink_diameter": 10 }), cone(5.0, 3.0, 2.0) + PI * 9.0 * 8.0),
    ];
    for (params, removed) in cases {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        let (_, ex, _, _) = block(&mut s, &mut k, 40.0, 20.0, 10.0);
        let (sk, pts) = points_on_top(&mut s, &mut k, ex, &[(10.0, 10.0), (30.0, 10.0)]);
        let mut p = params.clone();
        p["sketch"] = json!(sk);
        let r = run(&mut s, &mut k, "model.hole", p);
        assert_eq!(r["name"], "Hole1");
        regenerates(&mut s, &mut k);
        let v = volume(&mut s, &mut k);
        assert!(approx(v, 8000.0 - 2.0 * removed), "{params}: {v} vs {}", 8000.0 - 2.0 * removed);
        // Both points were taken by default; the hole walls are named after their points.
        let faces = run(&mut s, &mut k, "model.faces", json!({}));
        let walls: Vec<u64> = pts.iter().map(|p| (p << 8) | 2).collect();
        for w in &walls {
            let n = faces["faces"].as_array().unwrap().iter().filter(|f| f["name"]["type"] == "from" && f["name"]["source"] == *w).count();
            assert_eq!(n, 1, "{params}: wall of point {}: {faces}", w >> 8);
        }
    }
}

#[test]
fn a_hole_follows_its_point_and_keeps_its_edges() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, ex, _, _) = block(&mut s, &mut k, 40.0, 20.0, 10.0);
    let (sk, pts) = points_on_top(&mut s, &mut k, ex, &[(10.0, 10.0)]);
    let hole = run(&mut s, &mut k, "model.hole", json!({ "sketch": sk, "diameter": 6, "through_all": true }))["feature"].as_u64().unwrap();
    // Chamfer the hole's top edge: between the top face and the hole wall.
    let wall = json!({ "type": "from", "feature": hole, "source": (pts[0] << 8) | 2, "ordinal": 0 });
    let edge = run(&mut s, &mut k, "model.edge_ref", json!({ "faces": [top(ex), wall] }));
    run(&mut s, &mut k, "model.chamfer", json!({ "edges": [edge], "distance": 1.0 }));
    regenerates(&mut s, &mut k);
    let with_chamfer = volume(&mut s, &mut k);
    assert!(with_chamfer < 8000.0 - PI * 90.0, "{with_chamfer}");
    // Move the point: the hole and its chamfer go with it, nothing changes in volume.
    run(&mut s, &mut k, "sketch.drag", json!({ "sketch": sk, "point": pts[0], "x": 25.0, "y": 8.0 }));
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), with_chamfer), "{} vs {with_chamfer}", volume(&mut s, &mut k));
    let topo = run(&mut s, &mut k, "model.topology", json!({}));
    assert_eq!(topo["bodies"][0]["faces"], 6 + 2, "block, wall and chamfer cone: {topo}");
}

#[test]
fn bad_holes_are_refused_or_reported() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, ex, _, _) = block(&mut s, &mut k, 40.0, 20.0, 10.0);
    let (sk, pts) = points_on_top(&mut s, &mut k, ex, &[(10.0, 10.0)]);
    let before = s.document().clone();
    for bad in [
        json!({ "sketch": sk, "diameter": -1, "depth": 5 }),
        json!({ "sketch": sk, "diameter": 6, "depth": 5, "type": "counterbore", "counterbore_diameter": 4, "counterbore_depth": 1 }),
        json!({ "sketch": sk, "diameter": 6, "depth": 5, "type": "counterbore", "counterbore_diameter": 10, "counterbore_depth": 6 }),
        json!({ "sketch": sk, "diameter": 6, "depth": 5, "type": "countersink", "countersink_diameter": 30 }),
        json!({ "sketch": sk, "diameter": 6, "depth": 5, "points": [] }),
        json!({ "sketch": sk, "diameter": 6, "depth": 5, "points": [999] }),
        json!({ "sketch": sk, "diameter": 6, "depth": 5, "type": "slot" }),
    ] {
        assert!(s.exec("model.hole", &bad, Some(&mut k)).is_err(), "{bad}");
    }
    assert_eq!(s.document(), &before);
    // Deleting the centre point breaks the hole with a clear message.
    let hole = run(&mut s, &mut k, "model.hole", json!({ "sketch": sk, "diameter": 6, "depth": 5 }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.delete", json!({ "sketch": sk, "entities": [pts[0]] }));
    let r = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert_eq!(r["error"]["feature"], hole, "{r}");
    assert!(r["error"]["message"].as_str().unwrap().contains("no longer exists"), "{r}");
}

/// A block `w` x `d` x `h` with its corner at (x0, y0, 0).
fn block_at(s: &mut Session, k: &mut OcctKernel, x0: f64, y0: f64, w: f64, d: f64, h: f64) -> u64 {
    let sk = run(s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": x0, "y1": y0, "x2": x0 + w, "y2": y0 + d }));
    run(s, k, "model.extrude", json!({ "sketch": sk, "distance": h }))["feature"].as_u64().unwrap()
}

/// Faces named as made by feature `f`.
fn faces_of(s: &mut Session, k: &mut OcctKernel, f: u64) -> Vec<Value> {
    let faces = run(s, k, "model.faces", json!({}));
    faces["faces"].as_array().unwrap().iter().filter(|x| x["name"]["type"] == "from" && x["name"]["feature"] == f).cloned().collect()
}

#[test]
fn rectangular_pattern_of_a_hole_follows_the_hole() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let ex = block_at(&mut s, &mut k, 0.0, 0.0, 60.0, 40.0, 10.0);
    let (sk, _) = points_on_top(&mut s, &mut k, ex, &[(10.0, 10.0)]);
    let hole = run(&mut s, &mut k, "model.hole", json!({ "sketch": sk, "diameter": 6, "through_all": true }))["feature"].as_u64().unwrap();
    let pat = run(
        &mut s,
        &mut k,
        "model.pattern.rect",
        json!({ "features": [hole], "direction": "x", "count": 3, "spacing": 20, "direction2": "y", "count2": 2, "spacing2": 20 }),
    );
    assert_eq!(pat["name"], "Rectangular Pattern1");
    let pat = pat["feature"].as_u64().unwrap();
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), 24000.0 - 6.0 * PI * 9.0 * 10.0), "{}", volume(&mut s, &mut k));
    assert_eq!(faces_of(&mut s, &mut k, pat).len(), 5, "one wall per copy");

    // A bigger hole: every copy follows.
    let mut kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(hole as u32)).unwrap().kind).unwrap();
    kind["diameter"] = json!(8.0);
    run(&mut s, &mut k, "feature.update", json!({ "feature": hole, "kind": kind }));
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), 24000.0 - 6.0 * PI * 16.0 * 10.0), "{}", volume(&mut s, &mut k));

    // A copy's edge can be referenced: chamfer the top edge of one copied hole.
    let copy_wall = faces_of(&mut s, &mut k, pat)[0]["name"].clone();
    let edge = run(&mut s, &mut k, "model.edge_ref", json!({ "faces": [top(ex), copy_wall] }));
    run(&mut s, &mut k, "model.chamfer", json!({ "edges": [edge], "distance": 1.0 }));
    regenerates(&mut s, &mut k);
    assert!(volume(&mut s, &mut k) < 24000.0 - 6.0 * PI * 16.0 * 10.0);
}

#[test]
fn circular_pattern_full_and_partial() {
    for (angle, count, copies) in [(None, 4, 4.0), (Some(PI / 2.0), 3, 3.0)] {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        let ex = block_at(&mut s, &mut k, -20.0, -20.0, 40.0, 40.0, 10.0);
        // A boss 3 mm in radius, 5 tall, at x = 15 on the top face.
        let top_ref = run(&mut s, &mut k, "model.face_ref", json!({ "origin": top(ex) }));
        let sk = run(&mut s, &mut k, "sketch.create", json!({ "face": top_ref }))["feature"].as_u64().unwrap();
        run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 15, "cy": 0, "r": 3 }));
        let boss = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5 }))["feature"].as_u64().unwrap();
        let mut p = json!({ "features": [boss], "axis": "z", "count": count });
        if let Some(a) = angle {
            p["angle"] = json!(a);
        }
        let pat = run(&mut s, &mut k, "model.pattern.circular", p)["feature"].as_u64().unwrap();
        regenerates(&mut s, &mut k);
        let v = volume(&mut s, &mut k);
        assert!(approx(v, 16000.0 + copies * PI * 9.0 * 5.0), "{angle:?}: {v}");
        // Where the copies' round walls are (their centroids lie on the boss axes).
        let at = |x: f64, y: f64, s: &mut Session, k: &mut OcctKernel| {
            faces_of(s, k, pat)
                .iter()
                .any(|f| (f["centroid"][0].as_f64().unwrap() - x).abs() < 1e-6 && (f["centroid"][1].as_f64().unwrap() - y).abs() < 1e-6)
        };
        let r45 = 15.0 * std::f64::consts::FRAC_1_SQRT_2;
        if angle.is_none() {
            assert!(at(0.0, 15.0, &mut s, &mut k) && at(-15.0, 0.0, &mut s, &mut k) && at(0.0, -15.0, &mut s, &mut k), "quarter turns");
        } else {
            assert!(at(r45, r45, &mut s, &mut k) && at(0.0, 15.0, &mut s, &mut k), "45 and 90 degrees");
        }
    }
}

#[test]
fn mirror_a_cut_and_a_boss_across_an_origin_plane() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let ex = block_at(&mut s, &mut k, -20.0, 0.0, 40.0, 20.0, 10.0);
    let (sk, _) = points_on_top(&mut s, &mut k, ex, &[(10.0, 10.0)]);
    let hole = run(&mut s, &mut k, "model.hole", json!({ "sketch": sk, "diameter": 6, "through_all": true }))["feature"].as_u64().unwrap();
    let mirror = run(&mut s, &mut k, "model.mirror", json!({ "features": [hole], "plane": "yz" }));
    assert_eq!(mirror["name"], "Mirror1");
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), 8000.0 - 2.0 * PI * 9.0 * 10.0), "{}", volume(&mut s, &mut k));
    // The mirrored hole is at x = -10.
    let walls = faces_of(&mut s, &mut k, mirror["feature"].as_u64().unwrap());
    assert_eq!(walls.len(), 1, "{walls:?}");
    assert!((walls[0]["centroid"][0].as_f64().unwrap() + 10.0).abs() < 1e-6, "{}", walls[0]);
}

#[test]
fn patterns_refuse_what_they_cannot_copy() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, ex, l, _) = block(&mut s, &mut k, 40.0, 20.0, 10.0);
    let edge = run(&mut s, &mut k, "model.edge_ref", json!({ "faces": [side(ex, l[0]), side(ex, l[1])] }));
    let fillet = run(&mut s, &mut k, "model.fillet", json!({ "edges": [edge], "radius": 2.0 }))["feature"].as_u64().unwrap();
    let before = s.document().clone();
    for bad in [
        json!({ "features": [fillet], "direction": "x", "count": 2, "spacing": 5 }),
        json!({ "features": [], "direction": "x", "count": 2, "spacing": 5 }),
        json!({ "features": [ex], "direction": "x", "count": 1, "spacing": 5 }),
        json!({ "features": [ex], "direction": "x", "count": 2, "spacing": -5 }),
        json!({ "features": [ex], "direction": "w", "count": 2, "spacing": 5 }),
        json!({ "features": [ex], "direction": "x", "count": 100000, "spacing": 5 }),
        json!({ "features": [99], "direction": "x", "count": 2, "spacing": 5 }),
    ] {
        assert!(s.exec("model.pattern.rect", &bad, Some(&mut k)).is_err(), "{bad}");
    }
    assert!(s.exec("model.pattern.circular", &json!({ "features": [ex], "axis": "z", "count": 1 }), Some(&mut k)).is_err());
    assert!(s.exec("model.mirror", &json!({ "features": [fillet], "plane": "xy" }), Some(&mut k)).is_err());
    assert_eq!(s.document(), &before);
    // Copying a suppressed feature is reported at regeneration.
    let pat = run(&mut s, &mut k, "model.pattern.rect", json!({ "features": [ex], "direction": "z", "count": 2, "spacing": 10 }))["feature"]
        .as_u64()
        .unwrap();
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), 2.0 * 8000.0 - (4.0 - PI) * 10.0), "the fillet is not copied: {}", volume(&mut s, &mut k));
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": fillet }));
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": ex }));
    let r = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert_eq!(r["error"]["feature"], pat, "{r}");
    assert!(r["error"]["message"].as_str().is_some_and(|m| m.contains("Extrusion1 has no result to copy")), "{r}");
}

fn work(s: &mut Session, k: &mut OcctKernel, id: u64) -> Value {
    let w = run(s, k, "model.work", json!({}));
    w["work"].as_array().unwrap().iter().find(|x| x["feature"] == id).cloned().unwrap_or_else(|| panic!("no work feature {id}: {w}"))
}

fn v3(v: &Value) -> [f64; 3] {
    [v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap()]
}

fn near3(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6)
}

#[test]
fn a_work_plane_offset_from_a_face_carries_a_sketch_and_follows_it() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, ex, _, _) = block(&mut s, &mut k, 40.0, 20.0, 10.0);
    // 4 below the top face, inside the block.
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "origin": top(ex) }));
    let wp = run(&mut s, &mut k, "work.plane", json!({ "base": face, "distance": -4 }));
    assert_eq!(wp["name"], "Work Plane1");
    let wp = wp["feature"].as_u64().unwrap();
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 15, "y1": 5, "x2": 25, "y2": 15 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "symmetric": 2, "operation": "cut" }));
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), 8000.0 - 200.0), "{}", volume(&mut s, &mut k));
    assert!(near3(v3(&work(&mut s, &mut k, wp)["origin"]), [0.0, 0.0, 6.0]), "{}", work(&mut s, &mut k, wp));
    // A taller block: the plane moves with the top face.
    let mut kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(ex as u32)).unwrap().kind).unwrap();
    kind["extent"] = json!({ "distance": 14.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": ex, "kind": kind }));
    regenerates(&mut s, &mut k);
    assert!(near3(v3(&work(&mut s, &mut k, wp)["origin"]), [0.0, 0.0, 10.0]));
    assert!(approx(volume(&mut s, &mut k), 40.0 * 20.0 * 14.0 - 200.0));
}

#[test]
fn angled_and_mid_planes_mirror_like_origin_planes() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let ex = block_at(&mut s, &mut k, 0.0, 0.0, 40.0, 20.0, 10.0);
    let (sk, _) = points_on_top(&mut s, &mut k, ex, &[(10.0, 10.0)]);
    let hole = run(&mut s, &mut k, "model.hole", json!({ "sketch": sk, "diameter": 4, "through_all": true }))["feature"].as_u64().unwrap();
    // Midplane between the two end faces (x = 0 and x = 40) is x = 20: the mirrored hole is at 30.
    let faces = run(&mut s, &mut k, "model.faces", json!({}));
    let at_x = |x: f64| {
        faces["faces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| (f["centroid"][0].as_f64().unwrap() - x).abs() < 1e-9 && f["name"]["type"] == "side")
            .unwrap()["name"]
            .clone()
    };

    let (left, right) = (at_x(0.0), at_x(40.0));
    let a = run(&mut s, &mut k, "model.face_ref", json!({ "origin": left }));
    let b = run(&mut s, &mut k, "model.face_ref", json!({ "origin": right }));
    let mid = run(&mut s, &mut k, "work.plane", json!({ "by": "midplane", "a": a, "b": b }))["feature"].as_u64().unwrap();
    let m = run(&mut s, &mut k, "model.mirror", json!({ "features": [hole], "work_plane": mid }))["feature"].as_u64().unwrap();
    regenerates(&mut s, &mut k);
    assert!((v3(&work(&mut s, &mut k, mid)["origin"])[0] - 20.0).abs() < 1e-6, "{}", work(&mut s, &mut k, mid));
    let walls = faces_of(&mut s, &mut k, m);
    assert!((walls[0]["centroid"][0].as_f64().unwrap() - 30.0).abs() < 1e-6, "{walls:?}");
    // XZ turned 90 degrees about Z is YZ.
    let ang = run(&mut s, &mut k, "work.plane", json!({ "by": "angle", "base": "xz", "axis": "z", "angle": PI / 2.0 }))["feature"].as_u64().unwrap();
    regenerates(&mut s, &mut k);
    let n = v3(&work(&mut s, &mut k, ang)["normal"]);
    assert!((n[0].abs() - 1.0).abs() < 1e-9 && n[1].abs() < 1e-9 && n[2].abs() < 1e-9, "{n:?}");
    // An axis not in the base plane is refused at regeneration.
    let bad = run(&mut s, &mut k, "work.plane", json!({ "by": "angle", "base": "xy", "axis": "z", "angle": 1.0 }))["feature"].as_u64().unwrap();
    let r = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert_eq!(r["error"]["feature"], bad);
    assert!(r["error"]["message"].as_str().unwrap().contains("must lie in the base plane"));
}

#[test]
fn work_axes_and_points() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let ex = block_at(&mut s, &mut k, -20.0, -20.0, 40.0, 40.0, 10.0);
    // Where XZ and YZ meet is the Z axis.
    let ax = run(&mut s, &mut k, "work.axis", json!({ "a": "xz", "b": "yz" }))["feature"].as_u64().unwrap();
    // A hole at (12, 0), patterned 3 times about the work axis.
    let (sk, pts) = points_on_top(&mut s, &mut k, ex, &[(12.0, 0.0)]);
    let hole = run(&mut s, &mut k, "model.hole", json!({ "sketch": sk, "diameter": 4, "through_all": true }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "model.pattern.circular", json!({ "features": [hole], "axis": { "work": ax }, "count": 3 }));
    // The centre of the hole's top edge.
    let wall = json!({ "type": "from", "feature": hole, "source": (pts[0] << 8) | 2, "ordinal": 0 });
    let rim = run(&mut s, &mut k, "model.edge_ref", json!({ "faces": [top(ex), wall] }));
    let centre = run(&mut s, &mut k, "work.point", json!({ "edge": rim }))["feature"].as_u64().unwrap();
    regenerates(&mut s, &mut k);
    let w = work(&mut s, &mut k, ax);
    let d = v3(&w["direction"]);
    assert!(d[0].abs() < 1e-9 && d[1].abs() < 1e-9 && (d[2].abs() - 1.0).abs() < 1e-9, "{w}");
    assert!(approx(volume(&mut s, &mut k), 16000.0 - 3.0 * PI * 4.0 * 10.0), "{}", volume(&mut s, &mut k));
    // A point where the axis meets a plane 7 above XY, and the centre of the hole's top edge.
    let wp = run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 7 }))["feature"].as_u64().unwrap();
    let pt = run(&mut s, &mut k, "work.point", json!({ "axis": { "work": ax }, "plane": { "work": wp } }))["feature"].as_u64().unwrap();

    regenerates(&mut s, &mut k);
    assert!(near3(v3(&work(&mut s, &mut k, pt)["point"]), [0.0, 0.0, 7.0]));
    assert!(near3(v3(&work(&mut s, &mut k, centre)["point"]), [12.0, 0.0, 10.0]), "{}", work(&mut s, &mut k, centre));
    // Wrong kinds are refused.
    assert!(s.exec("work.point", &json!({ "axis": { "work": wp }, "plane": "xy" }), Some(&mut k)).is_err(), "a plane is not an axis");
    assert!(s.exec("sketch.create", &json!({ "work_plane": ax }), Some(&mut k)).is_err(), "an axis is not a plane");
    assert!(s.exec("work.axis", &json!({ "a": "xy", "b": { "work": 99 } }), Some(&mut k)).is_err());
    // Parallel planes have no line.
    let par = run(&mut s, &mut k, "work.axis", json!({ "a": "xy", "b": { "work": wp } }))["feature"].as_u64().unwrap();
    let r = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert_eq!(r["error"]["feature"], par, "{r}");
}

fn order(s: &Session) -> Vec<String> {
    s.document().features().iter().map(|f| f.name.clone()).collect()
}

#[test]
fn end_of_part_rolls_back_and_new_features_go_above_it() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let ex = block_at(&mut s, &mut k, 0.0, 0.0, 40.0, 20.0, 10.0);
    let (sk, _) = points_on_top(&mut s, &mut k, ex, &[(10.0, 10.0)]);
    let hole = run(&mut s, &mut k, "model.hole", json!({ "sketch": sk, "diameter": 6, "through_all": true }))["feature"].as_u64().unwrap();
    regenerates(&mut s, &mut k);
    let drilled = 8000.0 - PI * 9.0 * 10.0;
    assert!(approx(volume(&mut s, &mut k), drilled));

    // The marker above the hole's sketch: the hole is rolled back.
    let r = run(&mut s, &mut k, "feature.end_of_part", json!({ "before": sk }));
    assert_eq!(r["computed"], 2, "sketch and extrusion");
    let reg = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert!(reg["error"].is_null(), "{reg}");
    let status = |reg: &Value, id: u64| reg["features"].as_array().unwrap().iter().find(|f| f["id"] == id).unwrap()["status"]["state"].clone();
    assert_eq!(status(&reg, hole), "rolled_back");
    assert!(approx(volume(&mut s, &mut k), 8000.0));

    // A new feature goes in above the marker.
    let wp = run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 5 }))["feature"].as_u64().unwrap();
    assert_eq!(order(&s), ["Sketch1", "Extrusion1", "Work Plane1", "Sketch2", "Hole1"]);
    let reg = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert_eq!(status(&reg, wp), "ok");

    // Back to the end: everything is computed. Undo puts the marker back.
    run(&mut s, &mut k, "feature.end_of_part", json!({}));
    assert!(approx(volume(&mut s, &mut k), drilled));
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(approx(volume(&mut s, &mut k), 8000.0));
    run(&mut s, &mut k, "edit.redo", json!({}));

    // Deleting the feature the marker sits above moves it down one.
    run(&mut s, &mut k, "feature.end_of_part", json!({ "before": wp }));
    run(&mut s, &mut k, "feature.delete", json!({ "feature": wp }));
    assert_eq!(s.document().end_before().map(|f| f.0 as u64), Some(sk));
    let text = serde_json::to_string(s.document()).unwrap();
    let back: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(&back, s.document(), "the marker is saved");
}

#[test]
fn features_reorder_with_their_sketches_but_not_before_what_they_use() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let ex = block_at(&mut s, &mut k, 0.0, 0.0, 40.0, 20.0, 10.0);
    // A second, separate block from its own sketch.
    let sk2 = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk2, "x1": 50, "y1": 0, "x2": 60, "y2": 10 }));
    let ex2 = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk2, "distance": 5, "operation": "new_body" }))["feature"].as_u64().unwrap();
    let wp = run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 5 }))["feature"].as_u64().unwrap();
    assert_eq!(order(&s), ["Sketch1", "Extrusion1", "Sketch2", "Extrusion2", "Work Plane1"]);

    // Extrusion2 to the top: its sketch goes along.
    run(&mut s, &mut k, "feature.move", json!({ "feature": ex2, "before": 1 }));
    assert_eq!(order(&s), ["Sketch2", "Extrusion2", "Sketch1", "Extrusion1", "Work Plane1"]);
    regenerates(&mut s, &mut k);
    assert!(approx(volume(&mut s, &mut k), 8000.0 + 500.0));
    // The work plane to just after Extrusion2.
    run(&mut s, &mut k, "feature.move", json!({ "feature": wp, "before": 1 }));
    assert_eq!(order(&s), ["Sketch2", "Extrusion2", "Work Plane1", "Sketch1", "Extrusion1"]);
    // To the end.
    run(&mut s, &mut k, "feature.move", json!({ "feature": wp }));
    assert_eq!(order(&s), ["Sketch2", "Extrusion2", "Sketch1", "Extrusion1", "Work Plane1"]);

    // A hole on Extrusion1's top cannot go above Extrusion1.
    let (sk3, _) = points_on_top(&mut s, &mut k, ex, &[(10.0, 10.0)]);
    let hole = run(&mut s, &mut k, "model.hole", json!({ "sketch": sk3, "diameter": 4, "through_all": true }))["feature"].as_u64().unwrap();
    let before = s.document().clone();
    let e = s.exec("feature.move", &json!({ "feature": hole, "before": ex }), Some(&mut k)).unwrap_err();
    assert!(e.0.contains("Hole1 cannot go there"), "{e:?}");
    assert_eq!(s.document(), &before);
    let _ = sk3;
}

#[test]
fn regeneration_resumes_from_the_feature_being_edited() {
    use tenon_kernel::Kernel;
    use tenon_model::{FeatureId, RegenCache, regenerate, regenerate_with, scene};
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let ex = block_at(&mut s, &mut k, 0.0, 0.0, 60.0, 40.0, 10.0);
    // A few holes, a fillet, then the boss whose height we keep changing.
    let (sk, _) = points_on_top(&mut s, &mut k, ex, &[(10.0, 10.0), (50.0, 10.0), (10.0, 30.0)]);
    run(&mut s, &mut k, "model.hole", json!({ "sketch": sk, "diameter": 6, "through_all": true }));
    let top_ref = run(&mut s, &mut k, "model.face_ref", json!({ "origin": top(ex) }));
    let sk2 = run(&mut s, &mut k, "sketch.create", json!({ "face": top_ref }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk2, "cx": 40, "cy": 25, "r": 5 }));
    let boss = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk2, "distance": 5 }))["feature"].as_u64().unwrap();
    let boss_index = s.document().index_of(FeatureId(boss as u32)).unwrap();
    let mut doc = s.document().clone();
    let mut cache = RegenCache::default();
    let check = |doc: &Document, k: &mut OcctKernel, cache: &mut RegenCache| {
        let mut cached = regenerate_with(doc, k, Some(cache));
        let mut fresh = regenerate(doc, k);
        let (a, b) = (scene(&cached, k, &Default::default()).unwrap(), scene(&fresh, k, &Default::default()).unwrap());
        assert_eq!(a.bodies.len(), b.bodies.len());
        for (x, y) in a.bodies.iter().zip(&b.bodies) {
            assert!(approx(x.volume, y.volume), "{} vs {}", x.volume, y.volume);
            assert_eq!(x.faces.iter().map(|f| f.0).collect::<Vec<_>>(), y.faces.iter().map(|f| f.0).collect::<Vec<_>>(), "same names");
        }
        assert_eq!(cached.status, fresh.status);
        cached.release(k);
        fresh.release(k);
    };
    let set_height = |doc: &mut Document, h: f64| {
        let f = doc.feature_mut(FeatureId(boss as u32)).unwrap();
        let mut kind = serde_json::to_value(&f.kind).unwrap();
        kind["extent"] = json!({ "distance": h });
        f.kind = serde_json::from_value(kind).unwrap();
    };
    check(&doc, &mut k, &mut cache);
    assert_eq!(cache.resumed_at, 0, "the first run computes everything");
    // Drag the boss height: the second run leaves a checkpoint before the boss, later runs start there.
    for h in [6.0, 7.0, 8.0, 9.0] {
        set_height(&mut doc, h);
        check(&doc, &mut k, &mut cache);
    }
    assert_eq!(cache.resumed_at, boss_index, "only the boss is recomputed");
    // An earlier change (the hole diameter) starts from scratch again.
    let hole = doc.features().iter().find(|f| f.name == "Hole1").unwrap().id;
    let f = doc.feature_mut(hole).unwrap();
    let mut kind = serde_json::to_value(&f.kind).unwrap();
    kind["diameter"] = json!(4.0);
    f.kind = serde_json::from_value(kind).unwrap();
    check(&doc, &mut k, &mut cache);
    assert!(cache.resumed_at < boss_index);
    // Nothing leaks.
    cache.release(&mut k);
    let mut r = s.regen(&mut k).clone();
    let _ = &mut r;
    s.replace_document(Document::default(), Some(&mut k));
    assert_eq!(k.live_shapes(), 0, "every shape released");
}

/// Timing, not a check: `cargo test --release -p tenon-model --test m2 regeneration_speed -- --ignored --nocapture`.
#[test]
#[ignore]
fn regeneration_speed() {
    use std::time::Instant;
    use tenon_model::{FeatureId, RegenCache, regenerate, regenerate_with};
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let ex = block_at(&mut s, &mut k, 0.0, 0.0, 200.0, 100.0, 10.0);
    // 19 sketches of holes and 19 holes on the top face, then a boss.
    for i in 0..19 {
        let (sk, _) = points_on_top(&mut s, &mut k, ex, &[(10.0 + 10.0 * f64::from(i), 20.0), (10.0 + 10.0 * f64::from(i), 80.0)]);
        run(&mut s, &mut k, "model.hole", json!({ "sketch": sk, "diameter": 4, "through_all": true }));
    }
    let top_ref = run(&mut s, &mut k, "model.face_ref", json!({ "origin": top(ex) }));
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "face": top_ref }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 100, "cy": 50, "r": 8 }));
    let boss = run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5 }))["feature"].as_u64().unwrap();
    let mut doc = s.document().clone();
    let n = doc.features().len();
    let set = |doc: &mut Document, h: f64| {
        let f = doc.feature_mut(FeatureId(boss as u32)).unwrap();
        let mut kind = serde_json::to_value(&f.kind).unwrap();
        kind["extent"] = json!({ "distance": h });
        f.kind = serde_json::from_value(kind).unwrap();
    };
    let t = Instant::now();
    for i in 0..5 {
        set(&mut doc, 5.0 + f64::from(i));
        regenerate(&doc, &mut k).release(&mut k);
    }
    let full = t.elapsed().as_secs_f64() * 1000.0 / 5.0;
    let mut cache = RegenCache::default();
    regenerate_with(&doc, &mut k, Some(&mut cache)).release(&mut k);
    set(&mut doc, 20.0);
    regenerate_with(&doc, &mut k, Some(&mut cache)).release(&mut k);
    let t = Instant::now();
    for i in 0..5 {
        set(&mut doc, 10.0 + f64::from(i));
        regenerate_with(&doc, &mut k, Some(&mut cache)).release(&mut k);
    }
    let resumed = t.elapsed().as_secs_f64() * 1000.0 / 5.0;
    println!("{n} features: full regeneration {full:.1} ms, resumed at {} {resumed:.1} ms ({:.0}x)", cache.resumed_at, full / resumed);
    cache.release(&mut k);
}

#[test]
fn measure_areas_lengths_distances_and_angles() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, ex, l, _) = block(&mut s, &mut k, 40.0, 20.0, 10.0);
    let (sk, pts) = points_on_top(&mut s, &mut k, ex, &[(10.0, 10.0)]);
    let hole = run(&mut s, &mut k, "model.hole", json!({ "sketch": sk, "diameter": 6, "through_all": true }))["feature"].as_u64().unwrap();
    let face = |s: &mut Session, k: &mut OcctKernel, origin: Value| json!({ "face": run(s, k, "model.face_ref", json!({ "origin": origin })) });
    let value = |m: &Value, label: &str| m["values"].as_array().unwrap().iter().find(|v| v["label"] == label).map(|v| v["value"].as_f64().unwrap());
    let top_face = face(&mut s, &mut k, top(ex));
    let bottom = face(&mut s, &mut k, json!({ "type": "cap", "feature": ex, "end": "start" }));
    let front = face(&mut s, &mut k, side(ex, l[0]));
    let wall = face(&mut s, &mut k, json!({ "type": "from", "feature": hole, "source": (pts[0] << 8) | 2, "ordinal": 0 }));

    let m = run(&mut s, &mut k, "model.measure", json!({ "a": top_face }));
    assert!(approx(value(&m, "Area").unwrap(), 800.0 - PI * 9.0), "{m}");
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": wall }));
    assert!(approx(value(&m, "Diameter").unwrap(), 6.0), "{m}");
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": top_face, "b": bottom }));
    assert!(approx(value(&m, "Distance").unwrap(), 10.0) && value(&m, "Angle").unwrap().abs() < 1e-9, "{m}");
    assert!(approx(value(&m, "dZ").unwrap(), 10.0));
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": top_face, "b": front }));
    assert!(value(&m, "Distance").unwrap().abs() < 1e-9 && approx(value(&m, "Angle").unwrap(), 90.0), "{m}");
    // An edge: the front-right vertical edge is 10 long; from the hole wall it is the nearest
    // corner distance minus the radius.
    let edge = json!({ "edge": run(&mut s, &mut k, "model.edge_ref", json!({ "faces": [side(ex, l[0]), side(ex, l[1])] })) });
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": edge }));
    assert!(approx(value(&m, "Length").unwrap(), 10.0), "{m}");
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": wall, "b": edge }));
    assert!(approx(value(&m, "Distance").unwrap(), (30.0f64.powi(2) + 10.0f64.powi(2)).sqrt() - 3.0), "{m}");
    assert!(s.exec("model.measure", &json!({ "a": { "body": 0, "face": 999 } }), Some(&mut k)).is_err());
}
