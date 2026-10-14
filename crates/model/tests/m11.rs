//! M11: stress studies as commands. Where a part is held and what pushes on it are kept with
//! the part, refer to its faces by name, and are edited with undo like anything else.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;
use tenon_model::study::{LoadKind, StudyId};

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> String {
    s.exec(id, &p, Some(k)).expect_err(&format!("{id} {p} should be refused")).0
}

/// The reference of the one face of the part that looks `way`.
fn face(s: &mut Session, k: &mut OcctKernel, way: &str) -> Value {
    let found = run(s, k, "model.find", json!({ "what": "faces", "facing": way }));
    assert_eq!(found["count"], 1, "{way}: {found}");
    found["found"][0]["reference"].clone()
}

#[test]
fn a_study_says_where_a_part_is_held_and_what_pushes_on_it() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    // A bar 100 long, 20 wide and 10 thick.
    run(&mut s, &mut k, "model.box", json!({ "width": 100, "depth": 20, "height": 10 }));
    let (left, right, top) = (face(&mut s, &mut k, "left"), face(&mut s, &mut k, "right"), face(&mut s, &mut k, "top"));

    // No study yet: said so, with what makes one.
    assert!(refused(&mut s, &mut k, "study.fix", json!({ "faces": [left] })).contains("study.new makes one"));
    assert_eq!(run(&mut s, &mut k, "study.list", json!({}))["studies"], json!([]));

    // A study: held at its left end, pushed down on its right end, with a pressure on top.
    let study = run(&mut s, &mut k, "study.new", json!({}));
    assert_eq!((study["study"].as_u64(), study["name"].as_str()), (Some(1), Some("Study1")));
    assert_eq!(study["needs"].as_array().map(Vec::len), Some(2), "a support and a load: {study}");
    let fixed = run(&mut s, &mut k, "study.fix", json!({ "faces": [left] }));
    assert_eq!(
        (fixed["support"].as_u64(), fixed["supports"][0]["name"].as_str(), fixed["supports"][0]["kind"].as_str()),
        (Some(1), Some("Fixed1"), Some("fixed"))
    );
    assert_eq!(fixed["needs"].as_array().map(Vec::len), Some(1));
    let force = run(&mut s, &mut k, "study.force", json!({ "faces": [right], "newtons": 150, "direction": "down" }));
    assert_eq!(force["load"], 2);
    assert_eq!(force["loads"][0], json!({ "id": 2, "name": "Force1", "faces": 1, "type": "force", "newtons": 150.0, "direction": [0.0, 0.0, -1.0] }));
    assert_eq!(force["needs"], json!([]), "ready to be worked out");
    let pressed = run(&mut s, &mut k, "study.pressure", json!({ "faces": [top], "megapascals": 0.2, "name": "Snow" }));
    assert_eq!(pressed["loads"][1], json!({ "id": 3, "name": "Snow", "faces": 1, "type": "pressure", "megapascals": 0.2 }));
    // A force with no direction is square to its faces; a direction may be three numbers.
    let square = run(&mut s, &mut k, "study.force", json!({ "faces": [top, right], "newtons": -20 }));
    assert_eq!((square["loads"][2]["direction"].clone(), square["loads"][2]["faces"].as_u64()), (Value::Null, Some(2)));
    let slanted = run(&mut s, &mut k, "study.force", json!({ "faces": [right], "newtons": 5, "direction": [3, 0, -4] }));
    let way: Vec<f64> = slanted["loads"][3]["direction"].as_array().unwrap().iter().map(|c| c.as_f64().unwrap()).collect();
    assert!((way[0] - 0.6).abs() < 1e-12 && way[1] == 0.0 && (way[2] + 0.8).abs() < 1e-12, "kept as a direction, whatever its length: {way:?}");

    // It is in the part, as the part's file will have it.
    let st = s.document().study(StudyId(1)).unwrap();
    assert_eq!((st.supports.len(), st.loads.len()), (1, 4));
    assert!(matches!(st.loads[0].kind, LoadKind::Force { newtons, direction: Some(d) } if newtons == 150.0 && d.z == -1.0));
    s.document().validate().unwrap();

    // Each command is one undo step.
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert_eq!(s.document().study(StudyId(1)).unwrap().loads.len(), 3);
    run(&mut s, &mut k, "edit.redo", json!({}));
    assert_eq!(s.document().study(StudyId(1)).unwrap().loads.len(), 4);

    // Removed by id; the element size set and left to be chosen again; renamed.
    let fewer = run(&mut s, &mut k, "study.remove", json!({ "item": 4 }));
    assert_eq!((fewer["removed"].as_u64(), fewer["loads"].as_array().map(Vec::len)), (Some(4), Some(3)));
    assert!(refused(&mut s, &mut k, "study.remove", json!({ "item": 4 })).contains("no support or load 4"));
    assert_eq!(run(&mut s, &mut k, "study.mesh_size", json!({ "size": 2.5 }))["mesh_size"], 2.5);
    assert_eq!(run(&mut s, &mut k, "study.mesh_size", json!({ "size": null }))["mesh_size"], Value::Null);
    assert!(refused(&mut s, &mut k, "study.mesh_size", json!({ "size": 0 })).contains("more than 0"));
    assert_eq!(run(&mut s, &mut k, "study.rename", json!({ "name": " End load " }))["name"], "End load");

    // Changed in place: what is given takes the place of what it had, and the rest stays.
    let changed =
        run(&mut s, &mut k, "study.set", json!({ "item": 2, "newtons": 220, "direction": "normal", "faces": [right, top], "name": "Pull" }));
    assert_eq!(changed["item"], 2);
    assert_eq!(changed["loads"][0], json!({ "id": 2, "name": "Pull", "faces": 2, "type": "force", "newtons": 220.0, "direction": null }));
    let along = run(&mut s, &mut k, "study.set", json!({ "item": 2, "direction": "+x" }));
    assert_eq!((along["loads"][0]["direction"].clone(), along["loads"][0]["newtons"].as_f64()), (json!([1.0, 0.0, 0.0]), Some(220.0)));
    assert_eq!(run(&mut s, &mut k, "study.set", json!({ "item": 3, "megapascals": 0.35 }))["loads"][1]["megapascals"], 0.35);
    assert_eq!(run(&mut s, &mut k, "study.set", json!({ "item": 1, "faces": [left, top] }))["supports"][0]["faces"], 2);
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert_eq!(s.document().study(StudyId(1)).unwrap().supports[0].faces.len(), 1, "one undo step");
    // What is not its to have is refused in words, and it is left as it was.
    assert!(refused(&mut s, &mut k, "study.set", json!({ "item": 1, "newtons": 5 })).contains("is a support"));
    assert!(refused(&mut s, &mut k, "study.set", json!({ "item": 3, "direction": "down" })).contains("is a pressure"));
    assert!(refused(&mut s, &mut k, "study.set", json!({ "item": 2, "megapascals": 1 })).contains("is a force"));
    assert!(refused(&mut s, &mut k, "study.set", json!({ "item": 2, "faces": [], "newtons": 1 })).contains("at least one face"));
    assert!(refused(&mut s, &mut k, "study.set", json!({ "item": 9, "name": "x" })).contains("no support or load 9"));
    assert!(matches!(s.document().study(StudyId(1)).unwrap().loads[0].kind, LoadKind::Force { newtons, .. } if newtons == 220.0));

    // What is not a load is refused in words, and nothing is added.
    assert!(refused(&mut s, &mut k, "study.force", json!({ "faces": [right], "newtons": 0 })).contains("other than 0"));
    assert!(refused(&mut s, &mut k, "study.force", json!({ "faces": [], "newtons": 5 })).contains("at least one face"));
    assert!(refused(&mut s, &mut k, "study.force", json!({ "faces": [right], "newtons": 5, "direction": [0, 0, 0] })).contains("no direction"));
    assert!(refused(&mut s, &mut k, "study.pressure", json!({ "faces": [top] })).contains("megapascals"));
    assert!(refused(&mut s, &mut k, "study.fix", json!({ "faces": ["the left one"] })).contains("faces"));
    assert_eq!(s.document().study(StudyId(1)).unwrap().loads.len(), 3);

    // A second study: from then on a command says which it means.
    let second = run(&mut s, &mut k, "study.new", json!({ "name": "Twisted" }));
    assert_eq!((second["study"].as_u64(), second["name"].as_str()), (Some(2), Some("Twisted")));
    assert!(refused(&mut s, &mut k, "study.fix", json!({ "faces": [left] })).contains("several studies"));
    assert!(refused(&mut s, &mut k, "study.fix", json!({ "study": 9, "faces": [left] })).contains("no study 9"));
    run(&mut s, &mut k, "study.fix", json!({ "study": 2, "faces": [left] }));
    let list = run(&mut s, &mut k, "study.list", json!({}));
    let names: Vec<&str> = list["studies"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["End load", "Twisted"]);
    assert_eq!(list["studies"][1]["needs"].as_array().map(Vec::len), Some(1), "the second has no load yet");
    run(&mut s, &mut k, "study.delete", json!({ "study": 2 }));
    assert_eq!(s.document().studies().len(), 1);
    s.document().validate().unwrap();

    // The faces a study names are the part's own names for them: after a change to the part
    // that keeps them, they are the same faces. (The bar made longer: its right end has moved.)
    run(&mut s, &mut k, "param.set", json!({ "name": "d0", "equation": 140 }));
    let mass = run(&mut s, &mut k, "model.mass", json!({}));
    assert_eq!(mass["bodies"][0]["volume"].as_f64(), Some(140.0 * 20.0 * 10.0));
    let now_right = face(&mut s, &mut k, "right");
    let kept = serde_json::to_value(&s.document().study(StudyId(1)).unwrap().loads[0].faces[0]).unwrap();
    assert_eq!(kept["origin"], now_right["origin"], "the load is still on the right end, wherever that now is");
}

#[test]
fn pins_sliding_supports_the_parts_weight_and_copies_of_a_study() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    // A bar with a hole through it.
    run(&mut s, &mut k, "model.box", json!({ "width": 100, "depth": 20, "height": 10 }));
    run(&mut s, &mut k, "model.cylinder", json!({ "x": 50, "y": 10, "diameter": 8, "height": 10, "operation": "cut" }));
    let bore = run(&mut s, &mut k, "model.find", json!({ "what": "faces", "kind": "cylinder" }));
    assert_eq!(bore["count"], 1, "{bore}");
    let bore = bore["found"][0]["reference"].clone();
    let (bottom, left) = (face(&mut s, &mut k, "bottom"), face(&mut s, &mut k, "left"));
    run(&mut s, &mut k, "study.new", json!({}));

    // A pin in the bore: held across and along, free to turn, unless said otherwise.
    let pin = run(&mut s, &mut k, "study.pin", json!({ "faces": [bore] }));
    assert_eq!(pin["support"], 1);
    assert_eq!(pin["supports"][0], json!({ "id": 1, "name": "Pin1", "kind": "pin", "faces": 1, "radial": true, "axial": true, "tangential": false }));
    assert!(refused(&mut s, &mut k, "study.pin", json!({ "faces": [left] })).contains("a pin holds round"));
    assert!(refused(&mut s, &mut k, "study.pin", json!({ "faces": [bore], "radial": false, "axial": false })).contains("holds nothing"));
    assert!(refused(&mut s, &mut k, "study.pin", json!({ "faces": [bore], "axial": "no" })).contains("true or false"));
    let turned = run(&mut s, &mut k, "study.set", json!({ "item": 1, "axial": false, "tangential": true }));
    assert_eq!((turned["supports"][0]["axial"].clone(), turned["supports"][0]["tangential"].clone()), (json!(false), json!(true)));

    // The bar may slide on its bottom, and on the bore.
    let slide = run(&mut s, &mut k, "study.frictionless", json!({ "faces": [bottom, bore] }));
    assert_eq!(slide["supports"][1], json!({ "id": 2, "name": "Frictionless1", "kind": "frictionless", "faces": 2 }));
    assert!(refused(&mut s, &mut k, "study.set", json!({ "item": 2, "radial": true })).contains("is a support"));

    // Its own weight: down, at the Earth's pull, once.
    let weight = run(&mut s, &mut k, "study.gravity", json!({}));
    assert_eq!(weight["load"], 3);
    assert_eq!(
        weight["loads"][0],
        json!({ "id": 3, "name": "Gravity1", "faces": 0, "type": "gravity", "direction": [0.0, 0.0, -1.0], "acceleration": 9810.0 })
    );
    assert_eq!(weight["needs"], json!([]), "held and loaded");
    assert!(refused(&mut s, &mut k, "study.gravity", json!({ "direction": "left" })).contains("the part's weight already"));
    assert!(refused(&mut s, &mut k, "study.set", json!({ "item": 3, "direction": "normal" })).contains("pulls one way"));
    assert!(refused(&mut s, &mut k, "study.set", json!({ "item": 3, "faces": [left] })).contains("is the part's weight"));
    let moon = run(&mut s, &mut k, "study.set", json!({ "item": 3, "direction": "-y", "acceleration": 1620 }));
    assert_eq!((moon["loads"][0]["direction"].clone(), moon["loads"][0]["acceleration"].as_f64()), (json!([0.0, -1.0, 0.0]), Some(1620.0)));
    run(&mut s, &mut k, "study.remove", json!({ "item": 3 }));
    let told = refused(&mut s, &mut k, "study.gravity", json!({ "direction": "normal" }));
    assert!(told.contains("direction"), "the weight pulls one way, whatever the faces: {told}");
    assert!(refused(&mut s, &mut k, "study.gravity", json!({ "acceleration": 0 })).contains("more than 0"));
    run(&mut s, &mut k, "study.gravity", json!({ "direction": [0, 0, -2], "name": "Weight" }));

    // Suppressed: kept, and for now not there. With its only load suppressed the study lacks one.
    let quiet = run(&mut s, &mut k, "study.set", json!({ "item": 3, "suppressed": true }));
    assert_eq!(quiet["loads"][0]["suppressed"], true);
    assert_eq!(quiet["needs"].as_array().map(Vec::len), Some(1), "{quiet}");
    let heard = run(&mut s, &mut k, "study.set", json!({ "item": 3, "suppressed": false }));
    assert_eq!((heard["loads"][0].get("suppressed"), heard["needs"].clone()), (None, json!([])));
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(s.document().study(StudyId(1)).unwrap().loads[0].suppressed, "one undo step");
    run(&mut s, &mut k, "edit.redo", json!({}));

    // A copy of the study: the same supports and loads under another name, to change and compare.
    let copy = run(&mut s, &mut k, "study.copy", json!({}));
    assert_eq!((copy["study"].as_u64(), copy["name"].as_str(), copy["copied"].as_u64()), (Some(2), Some("Study1 copy"), Some(1)));
    assert_eq!((copy["supports"].as_array().map(Vec::len), copy["loads"].as_array().map(Vec::len)), (Some(2), Some(1)));
    let again = run(&mut s, &mut k, "study.copy", json!({ "study": 1 }));
    assert_eq!(again["name"], "Study1 copy 2");
    let named = run(&mut s, &mut k, "study.copy", json!({ "study": 2, "name": "On the Moon" }));
    assert_eq!((named["study"].as_u64(), named["name"].as_str()), (Some(4), Some("On the Moon")));
    assert!(refused(&mut s, &mut k, "study.copy", json!({ "study": 9 })).contains("no study 9"));
    // The copy is its own: changing it leaves the first as it was.
    run(&mut s, &mut k, "study.remove", json!({ "study": 2, "item": 1 }));
    assert_eq!((s.document().study(StudyId(1)).unwrap().supports.len(), s.document().study(StudyId(2)).unwrap().supports.len()), (2, 1));
    run(&mut s, &mut k, "edit.undo", json!({}));
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert_eq!(s.document().studies().len(), 3, "a copy is one undo step");
    s.document().validate().unwrap();
}

#[test]
fn a_study_is_checked_against_the_part_as_it_now_is() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.box", json!({ "width": 100, "depth": 20, "height": 10 }));
    let hole = run(&mut s, &mut k, "model.cylinder", json!({ "x": 50, "y": 10, "diameter": 8, "height": 10, "operation": "cut" }));
    let bore = run(&mut s, &mut k, "model.find", json!({ "what": "faces", "kind": "cylinder" }))["found"][0]["reference"].clone();
    let (left, right) = (face(&mut s, &mut k, "left"), face(&mut s, &mut k, "right"));
    run(&mut s, &mut k, "study.new", json!({}));
    let needs = |c: &Value| c["needs"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_owned()).collect::<Vec<_>>();

    // Nothing in it, and a part of nothing in particular.
    let empty = run(&mut s, &mut k, "study.check", json!({}));
    assert_eq!(empty["ready"], false);
    let said = needs(&empty);
    assert!(said.iter().any(|n| n.starts_with("a support")) && said.iter().any(|n| n.starts_with("a load")), "{said:?}");
    assert!(said.iter().any(|n| n.contains("a material that says how strong it is (the part has no material)")), "{said:?}");

    // Held, loaded, of steel: ready.
    run(&mut s, &mut k, "study.pin", json!({ "faces": [bore] }));
    run(&mut s, &mut k, "study.fix", json!({ "faces": [left] }));
    run(&mut s, &mut k, "study.force", json!({ "faces": [right, bore], "newtons": 100, "direction": "down" }));
    run(&mut s, &mut k, "document.material", json!({ "name": "Oak" }));
    let wooden = needs(&run(&mut s, &mut k, "study.check", json!({})));
    assert!(matches!(wooden.as_slice(), [one] if one.contains("Oak does not say")), "{wooden:?}");
    run(&mut s, &mut k, "document.material", json!({ "name": "Steel, mild" }));
    let ready = run(&mut s, &mut k, "study.check", json!({}));
    assert_eq!((ready["ready"].clone(), ready["needs"].clone(), ready["lost"].clone()), (json!(true), json!([]), json!([])));

    // The hole taken out of the part: the pin's face and one of the force's two are gone, and
    // the check says whose. (The study keeps them: the hole may come back.)
    let cut = hole["feature"].as_u64().expect("the id of the feature that cuts the hole");
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": cut, "suppressed": true }));
    let holed = run(&mut s, &mut k, "study.check", json!({}));
    assert_eq!(holed["ready"], false);
    assert_eq!(holed["lost"], json!([{ "item": 1, "name": "Pin1", "lost": 1, "of": 1 }, { "item": 3, "name": "Force1", "lost": 1, "of": 2 }]));
    let said = needs(&holed);
    assert!(said.iter().any(|n| n.starts_with("Pin1: its face is no longer on the part")), "{said:?}");
    assert!(said.iter().any(|n| n.starts_with("Force1: 1 of its 2 faces are no longer on the part")), "{said:?}");
    // A suppressed support is not in the study, and its lost face is not held against it.
    run(&mut s, &mut k, "study.set", json!({ "item": 1, "suppressed": true }));
    let quiet = run(&mut s, &mut k, "study.check", json!({}));
    assert_eq!(quiet["lost"].as_array().map(Vec::len), Some(1), "{quiet}");
    // The force given its faces again: ready.
    run(&mut s, &mut k, "study.set", json!({ "item": 3, "faces": [right] }));
    assert_eq!(run(&mut s, &mut k, "study.check", json!({}))["ready"], true);
    // The hole back: the pin's face is there again, by its name.
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": cut, "suppressed": false }));
    run(&mut s, &mut k, "study.set", json!({ "item": 1, "suppressed": false }));
    assert_eq!(run(&mut s, &mut k, "study.check", json!({}))["ready"], true);

    // A second solid: a study is of one.
    run(&mut s, &mut k, "model.box", json!({ "x": 200, "width": 10, "depth": 10, "height": 10, "operation": "new_body" }));
    let two = needs(&run(&mut s, &mut k, "study.check", json!({})));
    assert!(matches!(two.as_slice(), [one] if one.contains("one solid: the part has 2")), "{two:?}");
}

#[test]
fn moments_bearing_loads_remote_forces_turning_and_edges_by_command() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.box", json!({ "width": 100, "depth": 20, "height": 10 }));
    let hole = run(&mut s, &mut k, "model.cylinder", json!({ "x": 50, "y": 10, "diameter": 8, "height": 10, "operation": "cut" }));
    let bore = run(&mut s, &mut k, "model.find", json!({ "what": "faces", "kind": "cylinder" }))["found"][0]["reference"].clone();
    let (left, right) = (face(&mut s, &mut k, "left"), face(&mut s, &mut k, "right"));
    // The edge along y at the top of the right end, by where its middle is.
    let edges = run(&mut s, &mut k, "model.find", json!({ "what": "edges", "along": "y" }));
    let top_right = edges["found"].as_array().unwrap().iter().find(|e| e["mid"] == json!([100.0, 10.0, 10.0])).expect("the edge").clone();
    let top_right = top_right["reference"].clone();
    run(&mut s, &mut k, "study.new", json!({}));

    // Held along an edge, pushed along another.
    let held = run(&mut s, &mut k, "study.fix", json!({ "edges": [top_right] }));
    assert_eq!(held["supports"][0], json!({ "id": 1, "name": "Fixed1", "kind": "fixed", "faces": 0, "edges": 1 }));
    assert!(refused(&mut s, &mut k, "study.fix", json!({ "faces": [left], "edges": [top_right] })).contains("not on both"));
    assert!(refused(&mut s, &mut k, "study.fix", json!({})).contains("at least one face, or edge"));
    let pushed = run(&mut s, &mut k, "study.force", json!({ "edges": [top_right], "newtons": 40, "direction": "down" }));
    assert_eq!(
        pushed["loads"][0],
        json!({ "id": 2, "name": "Force1", "faces": 0, "edges": 1, "type": "force", "newtons": 40.0, "direction": [0.0, 0.0, -1.0] })
    );
    assert!(refused(&mut s, &mut k, "study.force", json!({ "edges": [top_right], "newtons": 40 })).contains("needs a direction"));
    assert!(refused(&mut s, &mut k, "study.pressure", json!({ "faces": [right], "edges": [top_right], "megapascals": 1 })).contains("edges"));
    // Given a face, the support is on the face.
    let moved = run(&mut s, &mut k, "study.set", json!({ "item": 1, "faces": [left] }));
    assert_eq!(moved["supports"][0], json!({ "id": 1, "name": "Fixed1", "kind": "fixed", "faces": 1 }));

    // A moment, about its face's own axis or one that is said.
    let turn = run(&mut s, &mut k, "study.moment", json!({ "faces": [right], "newton_millimetres": 2500 }));
    assert_eq!(turn["loads"][1], json!({ "id": 3, "name": "Moment1", "faces": 1, "type": "moment", "newton_millimetres": 2500.0, "axis": null }));
    let about = run(&mut s, &mut k, "study.set", json!({ "item": 3, "axis": "+y", "newton_millimetres": -100 }));
    assert_eq!((about["loads"][1]["axis"].clone(), about["loads"][1]["newton_millimetres"].as_f64()), (json!([0.0, 1.0, 0.0]), Some(-100.0)));
    assert_eq!(run(&mut s, &mut k, "study.set", json!({ "item": 3, "axis": "own" }))["loads"][1]["axis"], Value::Null);
    assert!(refused(&mut s, &mut k, "study.moment", json!({ "faces": [right], "newton_millimetres": 0 })).contains("other than 0"));

    // A bearing load in the bore; on a flat face it is refused.
    let press = run(&mut s, &mut k, "study.bearing", json!({ "faces": [bore], "newtons": 300, "direction": "front" }));
    assert_eq!(
        press["loads"][2],
        json!({ "id": 4, "name": "Bearing1", "faces": 1, "type": "bearing", "newtons": 300.0, "direction": [0.0, -1.0, 0.0] })
    );
    assert!(
        refused(&mut s, &mut k, "study.bearing", json!({ "faces": [right], "newtons": 300, "direction": "front" }))
            .contains("round (cylindrical) faces")
    );
    assert!(refused(&mut s, &mut k, "study.bearing", json!({ "faces": [bore], "newtons": 300 })).contains("direction"));

    // A force that acts at a point beyond the end.
    let far = run(&mut s, &mut k, "study.remote_force", json!({ "faces": [right], "newtons": 80, "direction": "down", "point": [150, 10, 5] }));
    let remote = json!({ "id": 5, "name": "RemoteForce1", "faces": 1, "type": "remote_force", "newtons": 80.0, "direction": [0.0, 0.0, -1.0], "point": [150.0, 10.0, 5.0] });
    assert_eq!(far["loads"][3], remote);
    assert!(refused(&mut s, &mut k, "study.remote_force", json!({ "faces": [right], "newtons": 80, "direction": "down" })).contains("point"));
    let nearer = run(&mut s, &mut k, "study.set", json!({ "item": 5, "point": [120, 10, 5] }));
    assert_eq!(nearer["loads"][3]["point"], json!([120.0, 10.0, 5.0]));

    // The part's turning: about z through the origin unless said; once.
    let spin = run(&mut s, &mut k, "study.body", json!({ "rpm": 3000 }));
    assert_eq!(
        spin["loads"][4],
        json!({ "id": 6, "name": "Body1", "faces": 0, "type": "body", "axis": [0.0, 0.0, 1.0], "point": [0.0, 0.0, 0.0], "rpm": 3000.0 })
    );
    assert!(refused(&mut s, &mut k, "study.body", json!({ "rpm": 10 })).contains("the part's turning already (Body1)"));
    let other = run(&mut s, &mut k, "study.set", json!({ "item": 6, "axis": "x", "point": [0, 10, 5], "rpm": 1500 }));
    assert_eq!((other["loads"][4]["axis"].clone(), other["loads"][4]["rpm"].as_f64()), (json!([1.0, 0.0, 0.0]), Some(1500.0)));
    assert!(refused(&mut s, &mut k, "study.set", json!({ "item": 6, "newtons": 5 })).contains("is the part's turning"));
    run(&mut s, &mut k, "study.gravity", json!({}));
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert_eq!(s.document().study(StudyId(1)).unwrap().loads.len(), 5, "one undo step each");
    s.document().validate().unwrap();

    // Held against the part: an edge that the part no longer has is said, like a face.
    run(&mut s, &mut k, "document.material", json!({ "name": "Steel, mild" }));
    run(&mut s, &mut k, "study.set", json!({ "item": 2, "suppressed": true }));
    assert_eq!(run(&mut s, &mut k, "study.check", json!({}))["ready"], true);
    run(&mut s, &mut k, "study.set", json!({ "item": 2, "suppressed": false }));
    // (The bar made shorter by a cut across its end: the edge at x = 100 is gone.)
    run(&mut s, &mut k, "model.box", json!({ "x": 90, "width": 10, "depth": 20, "height": 10, "operation": "cut" }));
    let cut = run(&mut s, &mut k, "study.check", json!({}));
    let said: Vec<&str> = cut["needs"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
    assert!(said.iter().any(|n| n.starts_with("Force1: its edge is no longer on the part. Give it its edges again")), "{said:?}");
    assert!(cut["lost"].as_array().unwrap().iter().any(|l| l["name"] == "Force1" && l["lost"] == 1 && l["of"] == 1), "{cut}");
    let _ = hole;
}

#[test]
fn a_material_says_how_it_answers_to_a_load() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    // No material: nothing to check stress against.
    assert_eq!(run(&mut s, &mut k, "study.list", json!({}))["strength"], Value::Null);
    // From the library: its typical stiffness and strength come with it, and are kept in the part.
    let steel = run(&mut s, &mut k, "document.material", json!({ "name": "Steel, mild" }));
    assert_eq!(steel["strength"], json!({ "youngs_modulus": 200.0, "poisson": 0.29, "yield_strength": 250.0 }));
    assert_eq!(steel["material"]["strength"], steel["strength"]);
    let list = run(&mut s, &mut k, "study.list", json!({}));
    assert_eq!((list["material"].as_str(), list["strength"]["yield_strength"].as_f64()), (Some("Steel, mild"), Some(250.0)));
    // A stronger grade of it: one value given in place of the library's.
    let strong = run(&mut s, &mut k, "document.material", json!({ "name": "Steel, mild", "yield_strength": 355 }));
    assert_eq!(strong["strength"], json!({ "youngs_modulus": 200.0, "poisson": 0.29, "yield_strength": 355.0 }));
    // Wood has none (its strength follows its grain); nor has a material of one's own until
    // all three are said.
    assert_eq!(run(&mut s, &mut k, "document.material", json!({ "name": "Oak" }))["strength"], Value::Null);
    let own = run(&mut s, &mut k, "document.material", json!({ "name": "Carbon PETG", "density": 1.3 }));
    assert_eq!(own["strength"], Value::Null);
    let told = refused(&mut s, &mut k, "document.material", json!({ "name": "Carbon PETG", "density": 1.3, "youngs_modulus": 6 }));
    assert!(told.contains("takes all three"), "{told}");
    let own = run(
        &mut s,
        &mut k,
        "document.material",
        json!({ "name": "Carbon PETG", "density": 1.3, "youngs_modulus": 6, "poisson": 0.35, "yield_strength": 55 }),
    );
    assert_eq!(own["strength"], json!({ "youngs_modulus": 6.0, "poisson": 0.35, "yield_strength": 55.0 }));
    assert!(refused(&mut s, &mut k, "document.material", json!({ "name": "Steel, mild", "poisson": 0.5 })).contains("Poisson"));
    // The library says what it knows of each.
    let library = run(&mut s, &mut k, "document.materials", json!({}))["library"].clone();
    let of = |name: &str| library.as_array().unwrap().iter().find(|m| m["name"] == name).unwrap().clone();
    assert_eq!(of("Aluminium 6061")["strength"]["youngs_modulus"], 69.0);
    assert!(of("Pine").get("strength").is_none());
}
