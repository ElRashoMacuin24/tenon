//! Rip, Unfold and Refold in files: records of their own, with only what is not the usual written
//! out, read back to the same bytes; and the part they make comes back the same.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use tenon_io::project;
use tenon_kernel_occt::OcctKernel;
use tenon_model::{FeatureKind, Session};

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{}", r["error"]);
    run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

/// A channel: a plate with a flange on each of two opposite edges.
fn channel(s: &mut Session, k: &mut OcctKernel) {
    run(s, k, "sheet.defaults", json!({ "thickness": 2, "bend_radius": 3, "gap": 1 }));
    let sk = run(s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 40 }));
    run(s, k, "sheet.face", json!({ "sketch": sk }));
    let (a, b) = (run(s, k, "model.edge_ref", json!({ "at": [60, 20, 2] })), run(s, k, "model.edge_ref", json!({ "at": [0, 20, 2] })));
    run(s, k, "sheet.flange", json!({ "edges": [a, b], "height": 20 }));
}

/// A point of the inside of a bend of the channel, half way round.
fn bend_at(right: bool) -> [f64; 3] {
    let c = std::f64::consts::FRAC_PI_4;
    let x = 3.0 * c.sin();
    [if right { 55.0 + x } else { 5.0 - x }, 20.0, 5.0 - 3.0 * c.cos()]
}

#[test]
fn rips_unfolds_and_refolds_are_records_read_back_to_the_same_bytes() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    channel(&mut s, &mut k);
    let top = run(&mut s, &mut k, "model.face_ref", json!({ "at": [30, 20, 2] }));
    // A rip of each way, and with a gap and an edge of its own.
    let edge = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [30, 0, 2] }));
    run(&mut s, &mut k, "sheet.rip", json!({ "face": top.clone(), "at": [20, 10, 2] }));
    let top = run(&mut s, &mut k, "model.face_ref", json!({ "at": [40, 20, 2] }));
    run(&mut s, &mut k, "sheet.rip", json!({ "face": top.clone(), "at": [40, 10, 2], "along": edge, "gap": 0.5 }));
    run(&mut s, &mut k, "sheet.rip", json!({ "face": top.clone(), "way": "between", "from": [50, 20, 2], "to": [50, 30, 2], "gap": 0.75 }));
    let wall = run(&mut s, &mut k, "model.face_ref", json!({ "at": [58, 20, 12] }));
    run(&mut s, &mut k, "sheet.rip", json!({ "face": wall, "way": "face" }));
    // An unfold of every bend, one of a bend named, and a refold of each sort. (Whether these make
    // a part that holds together is no matter here: only how they are written.)
    run(&mut s, &mut k, "sheet.unfold", json!({ "face": top.clone() }));
    let bend = top.clone();
    let one = run(&mut s, &mut k, "sheet.unfold", json!({ "face": top.clone(), "bends": [bend.clone()] }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": one }));
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": one, "bends": [bend] }));

    let extra = serde_json::Map::new();
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let records =
        |kind: &str| -> Vec<String> { text.split(&format!("type = \"{kind}\"")).skip(1).map(|c| c.split("[[").next().unwrap().to_owned()).collect() };

    let rips = records("rip");
    assert_eq!(rips.len(), 4, "{text}");
    assert!(rips[0].contains("way = { point = { at = ") && !rips[0].contains("gap") && !rips[0].contains("along"), "{}", rips[0]);
    assert!(rips[1].contains("along = ") && rips[1].contains("gap = 0.5"), "{}", rips[1]);
    assert!(rips[2].contains("way = { between = { from = ") && rips[2].contains(" to = ") && rips[2].contains("gap = 0.75"), "{}", rips[2]);
    assert!(rips[3].contains("way = \"face\"") && !rips[3].contains("gap"), "{}", rips[3]);
    for r in &rips {
        assert!(r.contains("face = "), "{r}");
    }
    let unfolds = records("unfold");
    assert_eq!(unfolds.len(), 2);
    assert!(unfolds[0].contains("face = ") && !unfolds[0].contains("bends"), "{}", unfolds[0]);
    assert!(unfolds[1].contains("bends = "), "{}", unfolds[1]);
    let refolds = records("refold");
    assert_eq!(refolds.len(), 2);
    assert!(refolds[0].contains(&format!("unfold = {one}")) && !refolds[0].contains("bends"), "{}", refolds[0]);
    assert!(refolds[1].contains("bends = "), "{}", refolds[1]);

    // Read back it is the same part, and saves as the same bytes.
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(back.features().len(), s.document().features().len());
    for (a, b) in back.features().iter().zip(s.document().features()) {
        assert_eq!(a.kind, b.kind);
    }
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes);
    back.validate().unwrap();
    assert!(back.features().iter().any(|f| matches!(f.kind, FeatureKind::Rip(_))));
    // What a file says of a bad gap, or of a whole face with a gap of its own, is refused with its line.
    let bad = text.replace("gap = 0.5", "gap = -1.0");
    let e = project::from_bytes(bad.as_bytes()).err().expect("a gap that cannot be").to_string();
    assert!(e.contains("gap of a rip"), "{e}");
    let bad = text.replace("way = \"face\"", "way = \"face\"\ngap = 1.0");
    let e = project::from_bytes(bad.as_bytes()).err().expect("a gap a whole face has not").to_string();
    assert!(e.contains("no gap of its own"), "{e}");
    let bad = text.replace("way = \"face\"", "way = \"slit\"");
    assert!(project::from_bytes(bad.as_bytes()).is_err());
}

#[test]
fn an_unfolded_and_refolded_part_comes_back_from_its_file_the_same() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    channel(&mut s, &mut k);
    let top = run(&mut s, &mut k, "model.face_ref", json!({ "at": [30, 20, 2] }));
    let right = run(&mut s, &mut k, "model.face_ref", json!({ "at": bend_at(true) }));
    let left = run(&mut s, &mut k, "model.face_ref", json!({ "at": bend_at(false) }));
    let unfold = run(&mut s, &mut k, "sheet.unfold", json!({ "face": top.clone(), "bends": [right, left] }))["feature"].as_u64().unwrap();
    let on = run(&mut s, &mut k, "model.face_ref", json!({ "at": [30, 20, 2] }));
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "face": on }))["feature"].as_u64().unwrap();
    let frame = s.regen(&mut k).sketch_frames[&tenon_model::FeatureId(sk as u32)];
    let at = frame.to_local(tenon_geom::Vec3::new(58.0, 20.0, 2.0));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": at.x, "cy": at.y, "r": 5 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5, "reverse": true, "operation": "cut" }));
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold }));
    let v = volume(&mut s, &mut k);
    let extra = serde_json::Map::new();
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let (back, _) = project::from_bytes(&bytes).unwrap();
    let mut t = Session::default();
    t.replace_document(back, None);
    let again = volume(&mut t, &mut k);
    assert!((again - v).abs() < 1e-9 * v, "{again} {v}");
    let (fa, fb) = (run(&mut s, &mut k, "sheet.flat", json!({})), run(&mut t, &mut k, "sheet.flat", json!({})));
    assert_eq!(fa["area"], fb["area"]);
}
