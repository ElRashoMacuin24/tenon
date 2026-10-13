//! M10, helpers for whoever drives Tenon in words: finding faces and edges by what they are
//! and where, and the primitives (box, cylinder, sphere, torus) as ordinary features made in
//! one step. Against the OpenCASCADE kernel.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> String {
    s.exec(id, &p, Some(k)).expect_err(&format!("{id} {p} should be refused")).0
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let m = run(s, k, "model.mass", json!({}));
    m["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

fn found(s: &mut Session, k: &mut OcctKernel, p: Value) -> Vec<Value> {
    let r = run(s, k, "model.find", p);
    let list = r["found"].as_array().unwrap().clone();
    assert_eq!(r["count"].as_u64(), Some(list.len() as u64));
    list
}

fn references(list: &[Value]) -> Vec<Value> {
    list.iter().map(|f| f["reference"].clone()).inspect(|r| assert!(r.is_object(), "{r}")).collect()
}

fn features(s: &mut Session, k: &mut OcctKernel) -> Vec<String> {
    run(s, k, "model.tree", json!({}))["features"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_owned()).collect()
}

/// A plate 60 by 40 by 8 with a hole of 10 through it at (30, 20), made with the primitives.
fn plate(s: &mut Session, k: &mut OcctKernel) -> (u64, u64) {
    let plate = run(s, k, "model.box", json!({ "width": 60, "depth": 40, "height": 8 }));
    let hole = run(s, k, "model.cylinder", json!({ "x": 30, "y": 20, "diameter": 10, "height": 8, "operation": "cut" }));
    (plate["feature"].as_u64().unwrap(), hole["feature"].as_u64().unwrap())
}

#[test]
fn faces_and_edges_are_found_by_what_they_are_and_where() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (block, hole) = plate(&mut s, &mut k);
    let net = 60.0 * 40.0 - PI * 25.0;
    assert!(near(volume(&mut s, &mut k), net * 8.0));

    // "The top face": the flat face that looks up. It says what it is, and how to refer to it.
    let top = found(&mut s, &mut k, json!({ "what": "faces", "facing": "top" }));
    assert_eq!(top.len(), 1);
    assert_eq!((top[0]["kind"].as_str(), top[0]["normal"].clone()), (Some("plane"), json!([0.0, 0.0, 1.0])));
    assert!(near(top[0]["area"].as_f64().unwrap(), net) && near(top[0]["centroid"][2].as_f64().unwrap(), 8.0));
    assert_eq!(top[0]["name"], json!({ "type": "cap", "feature": block, "end": "end" }));
    // The same face said other ways.
    for p in [json!({ "facing": "+z" }), json!({ "facing": [0, 0, 2] }), json!({ "kind": "plane", "most": "top" }), json!({ "facing": "UP" })] {
        let mut p = p;
        p["what"] = json!("faces");
        let same = found(&mut s, &mut k, p.clone());
        assert_eq!((same.len(), &same[0]["face"]), (1, &top[0]["face"]), "{p}");
    }
    // The other five sides each by their way, and none looking askew.
    for (way, normal) in [("bottom", [0, 0, -1]), ("left", [-1, 0, 0]), ("right", [1, 0, 0]), ("front", [0, -1, 0]), ("back", [0, 1, 0])] {
        let f = found(&mut s, &mut k, json!({ "what": "faces", "facing": way }));
        assert_eq!(f.len(), 1, "{way}");
        assert_eq!(f[0]["normal"], json!(normal.map(f64::from)), "{way}");
    }
    assert!(found(&mut s, &mut k, json!({ "what": "faces", "facing": [1, 1, 0] })).is_empty());

    // "The wall of the hole": round, a hole, of that radius, made by that feature, its axis upright.
    for p in [json!({ "hole": true }), json!({ "kind": "cylinder" }), json!({ "radius": 5 }), json!({ "feature": hole }), json!({ "along": "z" })] {
        let mut p = p;
        p["what"] = json!("faces");
        let wall = found(&mut s, &mut k, p.clone());
        assert_eq!(wall.len(), 1, "{p}");
        assert_eq!(
            (wall[0]["kind"].as_str(), wall[0]["hole"].as_bool(), wall[0]["radius"].as_f64()),
            (Some("cylinder"), Some(true), Some(5.0)),
            "{p}"
        );
        assert_eq!(wall[0]["axis"][2].as_f64().map(f64::abs), Some(1.0));
    }
    assert!(found(&mut s, &mut k, json!({ "what": "faces", "hole": false })).is_empty(), "there is no shaft");
    assert!(found(&mut s, &mut k, json!({ "what": "faces", "radius": 4 })).is_empty());
    assert_eq!(found(&mut s, &mut k, json!({ "what": "faces", "feature": block })).len(), 6);
    assert_eq!(found(&mut s, &mut k, json!({ "what": "faces" })).len(), 7);
    // The largest are the top and the bottom, which are alike; the smallest flat ones are the two
    // ends; the smallest of all is the wall of the hole.
    assert_eq!(found(&mut s, &mut k, json!({ "what": "faces", "kind": "plane", "most": "largest" })).len(), 2);
    let ends = found(&mut s, &mut k, json!({ "what": "faces", "kind": "plane", "most": "smallest" }));
    assert!(ends.len() == 2 && ends.iter().all(|f| near(f["area"].as_f64().unwrap(), 320.0)), "{ends:?}");
    assert_eq!(found(&mut s, &mut k, json!({ "what": "faces", "most": "smallest" }))[0]["kind"], "cylinder");

    // "The vertical edges": the four corners of the plate, and not the seam of the hole's wall.
    let upright = found(&mut s, &mut k, json!({ "what": "edges", "kind": "line", "along": "z" }));
    assert_eq!(upright.len(), 4);
    assert!(upright.iter().all(|e| near(e["length"].as_f64().unwrap(), 8.0) && e["direction"][2].as_f64().unwrap().abs() == 1.0));
    // The rims of the hole: whole circles of its radius; the top one is the higher.
    let rims = found(&mut s, &mut k, json!({ "what": "edges", "kind": "circle" }));
    assert_eq!(rims.len(), 2);
    assert!(rims.iter().all(|e| e["radius"].as_f64() == Some(5.0) && near(e["length"].as_f64().unwrap(), 10.0 * PI)));
    let rim = found(&mut s, &mut k, json!({ "what": "edges", "radius": 5, "most": "top" }));
    assert_eq!((rim.len(), rim[0]["mid"][2].as_f64()), (1, Some(8.0)));
    // The edges round the top: those level with the highest, the rim among them.
    assert_eq!(found(&mut s, &mut k, json!({ "what": "edges", "most": "top" })).len(), 5);
    assert_eq!(found(&mut s, &mut k, json!({ "what": "edges", "kind": "line", "most": "top" })).len(), 4);
    assert_eq!(found(&mut s, &mut k, json!({ "what": "edges", "along": "x" })).len(), 4);
    assert_eq!(found(&mut s, &mut k, json!({ "what": "edges", "feature": hole })).len(), 2);
    assert_eq!(found(&mut s, &mut k, json!({ "what": "edges" })).len(), 14);
    assert!(found(&mut s, &mut k, json!({ "what": "edges", "kind": "arc" })).is_empty());

    // What is found is what the features take: the corners rounded, the top opened.
    let before = volume(&mut s, &mut k);
    run(&mut s, &mut k, "model.fillet", json!({ "edges": references(&upright), "radius": 3 }));
    assert!(near(volume(&mut s, &mut k), before - (4.0 - PI) * 9.0 * 8.0));
    let top = found(&mut s, &mut k, json!({ "what": "faces", "facing": "top" }));
    run(&mut s, &mut k, "model.shell", json!({ "thickness": 1, "remove": references(&top) }));
    assert!(run(&mut s, &mut k, "model.regenerate", json!({}))["error"].is_null());
    // A sketch on the face found, as on any face.
    let floor = found(&mut s, &mut k, json!({ "what": "faces", "facing": "top", "most": "bottom" }));
    assert!(near(floor[0]["centroid"][2].as_f64().unwrap(), 1.0), "the floor inside the shell: {floor:?}");
    run(&mut s, &mut k, "sketch.create", json!({ "face": floor[0]["reference"] }));

    // On one body of two.
    run(&mut s, &mut k, "model.box", json!({ "x": 100, "width": 10, "depth": 10, "height": 10, "operation": "new_body" }));
    assert_eq!(found(&mut s, &mut k, json!({ "what": "faces", "body": 1 })).len(), 6);
    let tops = found(&mut s, &mut k, json!({ "what": "faces", "facing": "top", "most": "top" }));
    assert_eq!((tops.len(), tops[0]["body"].as_u64()), (1, Some(1)), "the cube stands higher");

    // What cannot be meant is said.
    assert_eq!(
        refused(&mut s, &mut k, "model.find", json!({ "what": "edges", "facing": "top" })).split(':').next(),
        Some("`facing` and `hole` are said of faces")
    );
    assert!(refused(&mut s, &mut k, "model.find", json!({ "what": "corners" })).contains("`what` must be"));
    assert!(refused(&mut s, &mut k, "model.find", json!({ "what": "faces", "facing": "sideways" })).contains("`facing`"));
    assert!(refused(&mut s, &mut k, "model.find", json!({ "what": "faces", "facing": [0, 0, 0] })).contains("`facing`"));
    assert!(refused(&mut s, &mut k, "model.find", json!({})).contains("`what`"));
}

#[test]
fn a_box_a_cylinder_a_sphere_and_a_torus_are_ordinary_features_made_in_one_step() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());

    // A box: a sketch and its extrusion, the sketch held in every direction by its dimensions.
    let made = run(&mut s, &mut k, "model.box", json!({ "width": 60, "depth": 40, "height": 8 }));
    assert_eq!((made["sketch_name"].as_str(), made["name"].as_str()), (Some("Sketch1"), Some("Extrusion1")));
    assert_eq!(features(&mut s, &mut k), ["Sketch1", "Extrusion1"]);
    assert_eq!(run(&mut s, &mut k, "sketch.info", json!({ "sketch": made["sketch"] }))["dof"], 0);
    assert!(near(volume(&mut s, &mut k), 60.0 * 40.0 * 8.0));
    // One step: undone, nothing of it is left; done again, all of it is back.
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(features(&mut s, &mut k).is_empty());
    assert!(!s.can_undo());
    run(&mut s, &mut k, "edit.redo", json!({}));
    assert_eq!(features(&mut s, &mut k), ["Sketch1", "Extrusion1"]);
    // Its sizes are parameters like any other: the width changed, the part follows.
    let params = run(&mut s, &mut k, "param.list", json!({}));
    let names: Vec<&str> = params["model"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["d0", "d1", "d2"], "{params}");
    run(&mut s, &mut k, "param.set", json!({ "name": "d0", "equation": 80 }));
    assert!(near(volume(&mut s, &mut k), 80.0 * 40.0 * 8.0));
    run(&mut s, &mut k, "edit.undo", json!({}));

    // A cylinder cut through it, and one joined on its top face.
    let hole = run(&mut s, &mut k, "model.cylinder", json!({ "x": 30, "y": 20, "diameter": 10, "height": 30, "operation": "cut" }));
    assert_eq!(run(&mut s, &mut k, "sketch.info", json!({ "sketch": hole["sketch"] }))["dof"], 0);
    let with_hole = (60.0 * 40.0 - PI * 25.0) * 8.0;
    assert!(near(volume(&mut s, &mut k), with_hole));
    let top = run(&mut s, &mut k, "model.find", json!({ "what": "faces", "facing": "top" }))["found"][0]["reference"].clone();
    let boss = run(&mut s, &mut k, "model.cylinder", json!({ "face": top, "x": 10, "y": 10, "diameter": 6, "height": 5 }));
    assert!(near(volume(&mut s, &mut k), with_hole + PI * 9.0 * 5.0));
    assert_eq!(features(&mut s, &mut k).len(), 6);
    let high = run(&mut s, &mut k, "model.find", json!({ "what": "faces", "facing": "top", "most": "top" }));
    assert_eq!(high["found"][0]["name"], json!({ "type": "cap", "feature": boss["feature"], "end": "end" }));
    assert!(near(high["found"][0]["centroid"][2].as_f64().unwrap(), 13.0));

    // A box about its middle, somewhere else, the other way from its plane.
    let mut s = Session::default();
    run(
        &mut s,
        &mut k,
        "model.box",
        json!({ "plane": "xz", "x": 10, "y": 5, "centered": true, "width": 20, "depth": 10, "height": 4, "reverse": true }),
    );
    assert!(near(volume(&mut s, &mut k), 800.0));
    let sketch = run(&mut s, &mut k, "sketch.info", json!({ "sketch": 1 }));
    assert_eq!(sketch["dof"], 0);
    let corners: Vec<(f64, f64)> = sketch["entities"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "point" && e["construction"] == false)
        .map(|e| (e["pos"]["x"].as_f64().unwrap(), e["pos"]["y"].as_f64().unwrap()))
        .collect();
    for corner in [(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)] {
        assert!(corners.iter().any(|c| near(c.0, corner.0) && near(c.1, corner.1)), "{corner:?} in {corners:?}");
    }

    // A sphere and a torus: a circle turned about a centre line, their sketches held too.
    let mut s = Session::default();
    let ball = run(&mut s, &mut k, "model.sphere", json!({ "diameter": 20 }));
    assert_eq!(
        (ball["name"].as_str(), run(&mut s, &mut k, "sketch.info", json!({ "sketch": ball["sketch"] }))["dof"].as_u64()),
        (Some("Revolution1"), Some(0))
    );
    assert!(near(volume(&mut s, &mut k), 4.0 / 3.0 * PI * 1000.0));
    // (The kernel makes the ball of several faces: each is of the sphere, and none is a hole.)
    let round = found(&mut s, &mut k, json!({ "what": "faces" }));
    assert!(
        !round.is_empty() && round.iter().all(|f| f["kind"] == "sphere" && f["radius"].as_f64() == Some(10.0) && f["hole"] == false),
        "{round:?}"
    );
    let mut s = Session::default();
    let ring = run(&mut s, &mut k, "model.torus", json!({ "x": 5, "y": 3, "diameter": 40, "section": 10 }));
    assert_eq!(run(&mut s, &mut k, "sketch.info", json!({ "sketch": ring["sketch"] }))["dof"], 0);
    assert!(near(volume(&mut s, &mut k), 2.0 * PI * PI * 20.0 * 25.0));
    let m = run(&mut s, &mut k, "model.mass", json!({}));
    let centre: Vec<f64> = m["bodies"][0]["center_of_mass"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((centre[0] - 5.0).abs() < 1e-6 && (centre[1] - 3.0).abs() < 1e-6 && centre[2].abs() < 1e-6, "{centre:?}");
    // A sphere taken out of a block.
    let mut s = Session::default();
    run(&mut s, &mut k, "model.box", json!({ "centered": true, "width": 30, "depth": 30, "height": 15 }));
    run(&mut s, &mut k, "model.sphere", json!({ "plane": "xz", "diameter": 20, "operation": "cut" }));
    assert!(near(volume(&mut s, &mut k), 30.0 * 30.0 * 15.0 - 2.0 / 3.0 * PI * 1000.0));

    // What cannot be made leaves nothing behind: no sketch without its feature, nothing to undo.
    let mut s = Session::default();
    for (id, p, why) in [
        ("model.box", json!({ "width": 10, "depth": 10, "height": 0 }), "`height` must be more than 0"),
        ("model.cylinder", json!({ "diameter": -4, "height": 5 }), "`diameter` must be more than 0"),
        ("model.torus", json!({ "diameter": 10, "section": 10 }), "must be less than its `diameter`"),
        ("model.box", json!({ "work_plane": 9, "width": 10, "depth": 10, "height": 5 }), "is not a work plane"),
        ("model.sphere", json!({ "diameter": 10, "operation": "cut" }), ""),
    ] {
        let told = s.exec(id, &p, Some(&mut k));
        match told {
            Err(e) => assert!(e.0.contains(why), "{id} {p}: {e}"),
            // (A cut out of nothing is made, and says when the part is rebuilt that it took nothing.)
            Ok(_) => {
                assert!(why.is_empty(), "{id} {p} should be refused");
                assert!(!run(&mut s, &mut k, "model.regenerate", json!({}))["error"].is_null());
                run(&mut s, &mut k, "edit.undo", json!({}));
            }
        }
        assert!(features(&mut s, &mut k).is_empty(), "{id} {p} left {:?}", features(&mut s, &mut k));
        assert!(!s.can_undo(), "{id} {p}");
    }
}
