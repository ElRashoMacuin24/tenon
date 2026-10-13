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
