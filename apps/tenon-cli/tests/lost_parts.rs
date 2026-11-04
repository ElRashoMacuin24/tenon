//! A part that is not there, or does not build, in what is made of it. Alone, a part whose Derive has
//! lost its file says so in red. In an assembly and a drawing it was silent: the component was simply
//! not there, and every report read as if all were well.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use tenon_cli::Engine;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

fn engine(name: &str) -> (Engine, PathBuf) {
    let dir = std::env::temp_dir().join(format!("tenon-lost-parts-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    (Engine::new(Box::new(OcctKernel::new()) as Box<dyn Kernel>, dir.clone()), dir)
}

fn run(e: &mut Engine, id: &str, p: Value) -> Value {
    e.exec(id, &p).unwrap_or_else(|err| panic!("{id}: {err}"))
}

fn refused(e: &mut Engine, id: &str, p: Value) -> String {
    e.exec(id, &p).expect_err(id)
}

/// `marked.tenon` derives `shaft.tenon`. It is placed in `h.tenonasm`, and both are drawn in
/// `p.tenondrw`, with a parts list. Then `shaft.tenon` goes.
fn a_derive_that_loses_its_file(name: &str) -> (Engine, PathBuf) {
    let (mut e, dir) = engine(name);
    run(&mut e, "file.new", json!({ "name": "Shaft" }));
    run(&mut e, "model.cylinder", json!({ "diameter": 30, "height": 100 }));
    run(&mut e, "file.save", json!({ "path": "shaft.tenon" }));
    run(&mut e, "file.new", json!({ "name": "Marked" }));
    run(&mut e, "model.derive", json!({ "path": "shaft.tenon" }));
    run(&mut e, "file.save", json!({ "path": "marked.tenon" }));
    run(&mut e, "asm.new", json!({ "name": "H" }));
    run(&mut e, "asm.insert", json!({ "path": "marked.tenon", "grounded": true }));
    run(&mut e, "asm.save", json!({ "path": "h.tenonasm" }));
    run(&mut e, "drw.new", json!({ "name": "P", "size": "C" }));
    run(&mut e, "drw.view.base", json!({ "model": "marked.tenon", "orientation": "top", "scale": 1, "at": [150, 200] }));
    run(&mut e, "drw.view.base", json!({ "model": "h.tenonasm", "orientation": "top", "scale": 1, "at": [350, 200] }));
    run(&mut e, "drw.save", json!({ "path": "p.tenondrw" }));
    std::fs::remove_file(dir.join("shaft.tenon")).unwrap();
    (e, dir)
}

#[test]
fn a_part_that_does_not_build_says_so_in_an_assembly() {
    let (mut e, _dir) = a_derive_that_loses_its_file("asm");
    let opened = run(&mut e, "asm.open", json!({ "path": "h.tenonasm" }));
    assert_eq!(opened["missing"], json!([]), "the file itself is there: {opened}");
    let said = opened["failing"][0].as_str().unwrap_or_default();
    assert!(said.contains("marked.tenon") && said.contains("shaft.tenon"), "{opened}");
    // The tree, the parts list, the mass, interference and export name it, where they said 0 and none.
    let tree = run(&mut e, "asm.tree", json!({}));
    assert!(tree["components"][0]["failing"].as_str().unwrap().contains("shaft.tenon"), "{tree}");
    let bom = run(&mut e, "asm.bom", json!({}));
    assert!(bom["rows"][0]["failing"].as_str().unwrap().contains("shaft.tenon"), "{bom}");
    for (id, p) in [("asm.mass", json!({})), ("asm.interference", json!({})), ("asm.export_step", json!({ "path": "h.step" }))] {
        let why = refused(&mut e, id, p);
        assert!(why.contains("does not build") && why.contains("shaft.tenon"), "{id}: {why}");
    }
}

#[test]
fn a_part_that_does_not_build_says_so_on_the_views_of_it() {
    let (mut e, _dir) = a_derive_that_loses_its_file("drw");
    let opened = run(&mut e, "drw.open", json!({ "path": "p.tenondrw" }));
    assert!(opened["failing"][0].as_str().unwrap().contains("shaft.tenon"), "{opened}");
    let info = run(&mut e, "drw.info", json!({}));
    for v in info["views"].as_array().unwrap() {
        let why = v["error"].as_str().unwrap_or_else(|| panic!("no word from {v}"));
        assert!(why.contains("does not build") && why.contains("shaft.tenon"), "{why}");
    }
}

/// An assembly with a part file gone, and a drawing of it: the assembly alone said so, and its
/// drawing left the components out with no word, while its parts list still listed them.
#[test]
fn a_part_file_gone_is_said_by_a_drawing_of_its_assembly() {
    let (mut e, dir) = engine("gone");
    for (name, width) in [("plate", 60), ("wedge", 20)] {
        run(&mut e, "file.new", json!({ "name": name }));
        run(&mut e, "model.box", json!({ "width": width, "depth": 20, "height": 10 }));
        run(&mut e, "file.save", json!({ "path": format!("{name}.tenon") }));
    }
    run(&mut e, "asm.new", json!({ "name": "G" }));
    run(&mut e, "asm.insert", json!({ "path": "plate.tenon", "grounded": true }));
    run(&mut e, "asm.insert", json!({ "path": "wedge.tenon", "at": [0, 0, 20] }));
    run(&mut e, "asm.save", json!({ "path": "g.tenonasm" }));
    run(&mut e, "drw.new", json!({ "name": "D", "size": "C" }));
    let view = run(&mut e, "drw.view.base", json!({ "model": "g.tenonasm", "orientation": "front", "scale": 1, "at": [200, 200] }));
    let list = run(&mut e, "drw.parts_list", json!({ "view": view["view"] }));
    run(&mut e, "drw.save", json!({ "path": "g.tenondrw" }));
    assert!(!list.is_null());
    std::fs::remove_file(dir.join("wedge.tenon")).unwrap();

    let opened = run(&mut e, "drw.open", json!({ "path": "g.tenondrw" }));
    let missing = opened["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 1, "{opened}");
    assert!(missing[0].as_str().unwrap().contains("wedge.tenon"), "{opened}");
    let info = run(&mut e, "drw.info", json!({}));
    let view = &info["views"][0];
    assert!(view["error"].as_str().unwrap_or_else(|| panic!("no word from {view}")).contains("wedge.tenon"), "{view}");
    assert!(view["visible_curves"].as_u64().unwrap() > 0, "what is there is still drawn: {view}");
}

/// When the component a balloon points at goes from the assembly, the balloon goes from the sheet; and
/// `drw.info` says so of it, as it does of a dimension on the same component.
#[test]
fn a_balloon_on_a_component_that_has_gone_says_so() {
    let (mut e, _dir) = engine("balloon");
    for (name, width) in [("plate", 60), ("wedge", 20)] {
        run(&mut e, "file.new", json!({ "name": name }));
        run(&mut e, "model.box", json!({ "width": width, "depth": 20, "height": 10 }));
        run(&mut e, "file.save", json!({ "path": format!("{name}.tenon") }));
    }
    run(&mut e, "asm.new", json!({ "name": "G" }));
    run(&mut e, "asm.insert", json!({ "path": "plate.tenon", "grounded": true }));
    let wedge = run(&mut e, "asm.insert", json!({ "path": "wedge.tenon", "at": [0, 0, 20] }))["component"].clone();
    run(&mut e, "asm.save", json!({ "path": "g.tenonasm" }));
    run(&mut e, "drw.new", json!({ "name": "D", "size": "C" }));
    let view = run(&mut e, "drw.view.base", json!({ "model": "g.tenonasm", "orientation": "front", "scale": 1, "at": [200, 200] }))["view"].clone();
    run(&mut e, "drw.balloon", json!({ "view": view, "component": wedge, "at": [300, 300] }));
    run(&mut e, "drw.save", json!({ "path": "g.tenondrw" }));
    let info = run(&mut e, "drw.info", json!({}));
    let balloon = info["annotations"].as_array().unwrap().iter().find(|a| a["type"] == "balloon").unwrap();
    assert!(balloon["problem"].is_null(), "{balloon}");
    run(&mut e, "asm.open", json!({ "path": "g.tenonasm" }));
    run(&mut e, "asm.delete", json!({ "component": wedge }));
    run(&mut e, "asm.save", json!({ "path": "g.tenonasm" }));
    run(&mut e, "drw.open", json!({ "path": "g.tenondrw" }));
    let info = run(&mut e, "drw.info", json!({}));
    let balloon = info["annotations"].as_array().unwrap().iter().find(|a| a["type"] == "balloon").unwrap();
    assert_eq!(balloon["problem"], "the component is not in the model", "{balloon}");
}
