//! User coordinate systems (DEC-133): where one stands, worked out by hand, and its planes, axes
//! and origin used where work planes, axes and points are.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};

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
    regenerates(s, k);
    run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

fn regenerates(s: &mut Session, k: &mut OcctKernel) {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * (1.0 + b.abs())
}

/// Where a UCS stands: origin, x, y, z.
fn frame_of(s: &mut Session, k: &mut OcctKernel, id: u64) -> [[f64; 3]; 4] {
    let w = run(s, k, "model.work", json!({}));
    let u = w["work"].as_array().unwrap().iter().find(|g| g["feature"].as_u64() == Some(id)).unwrap_or_else(|| panic!("no {id} in {w}")).clone();
    assert_eq!(u["type"], "ucs");
    let v = |key: &str| {
        let a = u[key].as_array().unwrap();
        [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()]
    };
    [v("origin"), v("x"), v("y"), v("z")]
}

#[track_caller]
fn stands(f: [[f64; 3]; 4], want: [[f64; 3]; 4]) {
    for (a, b) in f.iter().flatten().zip(want.iter().flatten()) {
        assert!((a - b).abs() < 1e-9, "{f:?}, wanted {want:?}");
    }
}

/// The box round the only body: [min, max].
fn extent(s: &mut Session, k: &mut OcctKernel) -> [[f64; 3]; 2] {
    regenerates(s, k);
    let t = run(s, k, "model.topology", json!({}));
    let bodies = t["bodies"].as_array().unwrap();
    assert_eq!(bodies.len(), 1, "{t}");
    let v = |key: &str| {
        let a = bodies[0]["bbox"][key].as_array().unwrap();
        [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()]
    };
    [v("min"), v("max")]
}

#[track_caller]
fn reaches(b: [[f64; 3]; 2], want: [[f64; 3]; 2]) {
    for (a, b) in b.iter().flatten().zip(want.iter().flatten()) {
        assert!((a - b).abs() < 1e-5, "{b:?}, wanted {want:?}");
    }
}

/// A block from (x0, y0) sized w by d, h high on the XY plane.
fn block(s: &mut Session, k: &mut OcctKernel, x0: f64, y0: f64, w: f64, d: f64, h: f64) -> u64 {
    let sk = feature(&run(s, k, "sketch.create", json!({ "plane": "xy" })));
    run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": x0, "y1": y0, "x2": x0 + w, "y2": y0 + d }));
    feature(&run(s, k, "model.extrude", json!({ "sketch": sk, "distance": h })))
}

#[test]
fn a_ucs_is_moved_then_turned_from_the_origin_or_from_another_ucs() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    // Moved (10, 20, 30); a quarter turn about its own Z takes its X to world Y and its Y to -X.
    let a = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 10, "y": 20, "z": 30, "rz": FRAC_PI_2 })));
    stands(frame_of(&mut s, &mut k, a), [[10.0, 20.0, 30.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
    // From that one: 5 along its X is 5 along world Y; 7 along its Y is 7 along -X.
    let b = feature(&run(&mut s, &mut k, "work.ucs", json!({ "base": a, "x": 5, "y": 7 })));
    stands(frame_of(&mut s, &mut k, b), [[3.0, 25.0, 30.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
    // Turned about X, then about the Y that gives (world Z), by hand: X to world Y, Y to world Z,
    // Z to world X.
    let c = feature(&run(&mut s, &mut k, "work.ucs", json!({ "rx": FRAC_PI_2, "ry": FRAC_PI_2 })));
    stands(frame_of(&mut s, &mut k, c), [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]]);
    // A change to the first moves what is measured from it.
    let def = run(&mut s, &mut k, "model.tree", json!({}));
    assert!(def["features"].as_array().unwrap().iter().any(|f| f["name"] == "UCS3"), "{def}");
    let kind = json!({ "type": "ucs", "by": "offset", "x": 110.0, "y": 20.0, "z": 30.0, "rx": 0.0, "ry": 0.0, "rz": FRAC_PI_2 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": a, "kind": kind }));
    stands(frame_of(&mut s, &mut k, b), [[103.0, 25.0, 30.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
    // Undo puts it back.
    run(&mut s, &mut k, "edit.undo", json!({}));
    stands(frame_of(&mut s, &mut k, b), [[3.0, 25.0, 30.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
    // Its offsets take parameters.
    run(&mut s, &mut k, "param.add", json!({ "name": "reach", "equation": "50 mm" }));
    run(&mut s, &mut k, "feature.update", json!({ "feature": a, "kind": kind, "equations": { "/x": "reach * 2" } }));
    stands(frame_of(&mut s, &mut k, a), [[100.0, 20.0, 30.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
    run(&mut s, &mut k, "param.set", json!({ "name": "reach", "equation": "60 mm" }));
    stands(frame_of(&mut s, &mut k, a), [[120.0, 20.0, 30.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
}

#[test]
fn a_ucs_goes_through_three_points_and_follows_them() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let ex = block(&mut s, &mut k, 0.0, 0.0, 40.0, 20.0, 10.0);
    // Two corners of the top by the ends of its edges, and a sketch point.
    let edges = run(&mut s, &mut k, "model.edges", json!({}));
    let on = |want: [f64; 3]| {
        let here = |key: &str, e: &Value| e[key].as_array().unwrap().iter().zip(want).all(|(a, b)| (a.as_f64().unwrap() - b).abs() < 1e-9);
        edges["edges"].as_array().unwrap().iter().find(|e| here("start", e) || here("end", e)).unwrap().clone()
    };
    let corner = |want: [f64; 3], s: &mut Session, k: &mut OcctKernel| {
        let e = on(want);
        let r = run(s, k, "model.edge_ref", json!({ "body": 0, "edge": e["edge"] }));
        // (Which end, by the way it lies from the edge's middle.)
        let mid = &r["fingerprint"]["mid"];
        let from_mid = |key: &str, at: f64| at - mid[key].as_f64().unwrap();
        let towards = json!({ "x": from_mid("x", want[0]), "y": from_mid("y", want[1]), "z": from_mid("z", want[2]) });
        json!({ "vertex": { "edge": r, "end": 0, "towards": towards } })
    };
    let (o, x, y) = (corner([40.0, 0.0, 10.0], &mut s, &mut k), corner([40.0, 20.0, 10.0], &mut s, &mut k), corner([0.0, 0.0, 10.0], &mut s, &mut k));
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "by": "three_points", "points": [o, x, y] })));
    // From the first toward the second is its X (world Y); the third lies on its +Y side (world
    // -X), so its Z is world Z.
    stands(frame_of(&mut s, &mut k, u), [[40.0, 0.0, 10.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
    // With the block taller, the corners go up, and the UCS with them.
    let kind = json!({ "type": "extrude", "sketch": 1, "regions": "default", "extent": { "distance": 25.0 }, "reverse": false, "operation": "join" });
    run(&mut s, &mut k, "feature.update", json!({ "feature": ex, "kind": kind }));
    stands(frame_of(&mut s, &mut k, u), [[40.0, 0.0, 25.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);

    // Three points in a line are refused in words when the part is rebuilt, not mistaken for one.
    let e = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert!(e["error"].is_null());
    let line = json!({ "by": "three_points", "points": ["origin", { "ucs": u }, { "ucs": u }] });
    run(&mut s, &mut k, "work.ucs", line);
    let e = run(&mut s, &mut k, "model.regenerate", json!({}));
    let said = e["error"]["message"].as_str().unwrap();
    assert!(said.contains("three points are in one line"), "{said}");
}

#[test]
fn a_sketch_goes_on_each_plane_of_a_ucs_as_on_the_origin_planes() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    // At (100, 0, 0), turned a quarter about X: its Y is world Z and its Z is world -Y.
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 100, "rx": FRAC_PI_2 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "ucs_plane": { "ucs": u, "plane": "xy" } })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 10, "y2": 20 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5 }));
    // 10 along world X from 100, 20 up world Z, 5 along world -Y.
    reaches(extent(&mut s, &mut k), [[100.0, -5.0, 0.0], [110.0, 0.0, 20.0]]);
    assert!(near(volume(&mut s, &mut k), 1000.0));

    // On its YZ plane, a sketch's x is the UCS's Y and its normal the UCS's X, as on the origin's.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 100, "rx": FRAC_PI_2 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "ucs_plane": { "ucs": u, "plane": "yz" } })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 10, "y2": 20 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5 }));
    // Sketch x: UCS Y = world Z, 10. Sketch y: UCS Z = world -Y, 20. Up the UCS's X: world X, 5.
    reaches(extent(&mut s, &mut k), [[100.0, -20.0, 0.0], [105.0, 0.0, 10.0]]);

    // Only a UCS has such planes.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 3 })));
    let e = s.exec("sketch.create", &json!({ "ucs_plane": { "ucs": wp, "plane": "xy" } }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("is not a UCS"), "{e}");
}

#[test]
fn extrude_to_a_mirror_a_pattern_and_a_revolve_take_its_planes_and_axes() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 50, "z": 25 })));
    // Extrude To its XY plane: 25 high.
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 10, "y2": 10 }));
    let ex = feature(&run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "to": { "ucs": u, "plane": "xy" } })));
    assert!(near(volume(&mut s, &mut k), 2500.0));
    reaches(extent(&mut s, &mut k), [[0.0, 0.0, 0.0], [10.0, 10.0, 25.0]]);

    // Mirrored across its YZ plane, at x = 50: the copy from 90 to 100.
    run(&mut s, &mut k, "model.mirror", json!({ "features": [ex], "ucs_plane": { "ucs": u, "plane": "yz" } }));
    assert!(near(volume(&mut s, &mut k), 5000.0));
    reaches(extent(&mut s, &mut k), [[0.0, 0.0, 0.0], [100.0, 10.0, 25.0]]);

    // In a row along its Y, every 30: three more, the block and its mirror image each.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "rz": FRAC_PI_2 })));
    let ex = block(&mut s, &mut k, 0.0, 0.0, 10.0, 10.0, 5.0);
    // Its X is world Y: a row along +Y.
    run(&mut s, &mut k, "model.pattern.rect", json!({ "features": [ex], "direction": { "ucs": u, "axis": "x" }, "count": 3, "spacing": 30 }));
    assert!(near(volume(&mut s, &mut k), 1500.0));
    reaches(extent(&mut s, &mut k), [[0.0, 0.0, 0.0], [10.0, 70.0, 5.0]]);

    // Round its Z, which stands at (100, 0): four about there, a quarter apart.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 100 })));
    let ex = block(&mut s, &mut k, 110.0, -5.0, 10.0, 10.0, 5.0);
    run(&mut s, &mut k, "model.pattern.circular", json!({ "features": [ex], "axis": { "ucs": u, "axis": "z" }, "count": 4 }));
    assert!(near(volume(&mut s, &mut k), 2000.0));
    reaches(extent(&mut s, &mut k), [[80.0, -20.0, 0.0], [120.0, 20.0, 5.0]]);

    // A rectangle turned about its Y: a ring. The UCS's Y is world Y at x = 100; the rectangle
    // from 10 to 20 off it, 10 high: pi (20^2 - 10^2) x 10.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 100 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 110, "y1": 0, "x2": 120, "y2": 10 }));
    run(&mut s, &mut k, "model.revolve", json!({ "sketch": sk, "axis": { "ucs": u, "axis": "y" } }));
    assert!(near(volume(&mut s, &mut k), PI * 300.0 * 10.0));
}

#[test]
fn work_features_are_built_on_a_ucs_and_it_is_measured_to() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k, 0.0, 0.0, 40.0, 20.0, 10.0);
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 50, "y": 10, "z": 30, "rz": FRAC_PI_2 })));
    // A work plane off its XY, a work axis along its Z, a work point where they meet.
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": { "ucs": u, "plane": "xy" }, "distance": -5 })));
    let ax = feature(&run(&mut s, &mut k, "work.axis", json!({ "axis": { "ucs": u, "axis": "z" } })));
    let pt = feature(&run(&mut s, &mut k, "work.point", json!({ "axis": { "work": ax }, "plane": { "work": wp } })));
    let w = run(&mut s, &mut k, "model.work", json!({}));
    let p = w["work"].as_array().unwrap().iter().find(|g| g["feature"].as_u64() == Some(pt)).unwrap()["point"].clone();
    assert_eq!(p, json!([50.0, 10.0, 25.0]));

    // Measure: from its origin to the block's top face, 20 up (z = 30 against z = 10), and across
    // to the block's right side.
    let top = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": 2, "end": "end" } }));
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": { "ucs": u }, "b": { "face": top } }));
    let got = |m: &Value, label: &str| m["values"].as_array().unwrap().iter().find(|v| v["label"] == label).map(|v| v["value"].as_f64().unwrap());
    // The nearest point of the top is its corner side: 10 along X from x = 40, none along Y, 20 down.
    assert!(near(got(&m, "Distance").unwrap(), (100.0f64 + 400.0).sqrt()), "{m}");
    assert!(near(got(&m, "dX").unwrap(), 10.0) && near(got(&m, "dZ").unwrap(), 20.0), "{m}");
    // Its XY plane and that face are parallel, 20 apart.
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": { "ucs": u, "plane": "xy" }, "b": { "face": top } }));
    assert!(near(got(&m, "Distance").unwrap(), 20.0) && near(got(&m, "Angle").unwrap(), 0.0), "{m}");
    // Its Z axis and that face: a line square to a plane, 90 degrees; it passes the top 10 off.
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": { "ucs": u, "axis": "z" }, "b": { "face": top } }));
    assert!(near(got(&m, "Distance").unwrap(), 10.0) && near(got(&m, "Angle").unwrap(), 90.0), "{m}");
    // From its X axis (world Y, at x = 50, z = 30) to the origin's XY work plane offset 0: plain
    // geometry both, 30 apart; and to a work point.
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": { "ucs": u, "axis": "x" }, "b": { "work": pt } }));
    assert!(near(got(&m, "Distance").unwrap(), 5.0), "{m}");
    // Its origin alone says where it is.
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": { "ucs": u } }));
    assert!(near(got(&m, "X").unwrap(), 50.0) && near(got(&m, "Z").unwrap(), 30.0), "{m}");
    // An axis or a plane alone, having no size: a point of it, and the way it goes.
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": { "ucs": u, "axis": "z" } }));
    assert!(near(got(&m, "X").unwrap(), 50.0) && near(got(&m, "Along Z").unwrap(), 1.0), "{m}");
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": { "ucs": u, "plane": "xy" } }));
    assert!(near(got(&m, "Z").unwrap(), 30.0) && near(got(&m, "Normal Z").unwrap(), 1.0), "{m}");
    // So does a work plane or axis, and with a second pick the figures are the ones they were.
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "yz", "distance": 7 })));
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": { "work": wp } }));
    assert!(near(got(&m, "X").unwrap(), 7.0) && near(got(&m, "Normal X").unwrap().abs(), 1.0), "{m}");
    let m = run(&mut s, &mut k, "model.measure", json!({ "a": { "work": wp }, "b": { "ucs": u } }));
    assert!(near(got(&m, "Distance").unwrap(), 43.0) && got(&m, "Normal X").is_none(), "{m}");
}

#[test]
fn what_uses_a_ucs_says_so_when_it_is_gone_and_it_can_be_suppressed_and_deleted() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "z": 5 })));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "ucs_plane": { "ucs": u, "plane": "xy" } })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 10, "y2": 10 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 2 }));
    assert!(near(volume(&mut s, &mut k), 200.0));
    let e = s.exec("feature.delete", &json!({ "feature": u }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("is used by"), "{e}");
    // Suppressed, what stands on it fails and says what is wrong; brought back, all is well.
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": u }));
    let r = run(&mut s, &mut k, "model.regenerate", json!({}));
    let said = r["error"]["message"].as_str().unwrap();
    assert!(said.contains("UCS1") && said.contains("suppressed"), "{said}");
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": u, "suppressed": false }));
    assert!(near(volume(&mut s, &mut k), 200.0));
    // Deleted with what uses it.
    run(&mut s, &mut k, "feature.delete", json!({ "feature": u, "dependents": true }));
    assert!(s.document().features().is_empty());
    // A UCS measured from something that is not one is refused.
    let wp = feature(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 3 })));
    let e = s.exec("work.ucs", &json!({ "base": wp }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("is not a UCS"), "{e}");
    // A number that is no number is refused.
    let e = s.exec("work.ucs", &json!({ "rx": 1000.0 }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("turns"), "{e}");
}
