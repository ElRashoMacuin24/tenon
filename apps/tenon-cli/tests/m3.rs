//! M3 demo: the pivot assembly script (four part files, joints and constraints, degrees of
//! freedom, interference, parts list, exploded view, editing a part in place) and the files it
//! writes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;
use std::path::PathBuf;

use serde_json::json;
use tenon_cli::Engine;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

fn kernel() -> Box<dyn Kernel> {
    Box::new(OcctKernel::new())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-cli-m3-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_m3_demo_script_builds_a_verified_pivot_assembly() {
    // The script checks every placement, the degrees of freedom, interference, the parts list,
    // the exploded positions and the volume before and after the arm is edited in place; here we
    // check the files it writes and that the saved assembly reopens elsewhere.
    let dir = scratch("pivot");
    let r = tenon_cli::demo_m3(kernel(), &dir).unwrap();
    assert_eq!(r.json["ok"], true, "{}", r.text);
    let expected = 47680.0 + 344.0 * PI;

    // The STEP file holds every component where it is placed.
    let mut k = OcctKernel::new();
    let info = tenon_cli::info(&mut k, &dir.join("pivot.step")).unwrap();
    let total: f64 = info.json["shapes"].as_array().unwrap().iter().map(|s| s["volume_mm3"].as_f64().unwrap()).sum();
    assert!((total - expected).abs() < 1e-6 * expected, "STEP volume {total} vs {expected}");
    for png in ["pivot.png", "pivot-exploded.png"] {
        assert!(std::fs::read(dir.join(png)).unwrap().starts_with(&[0x89, b'P', b'N', b'G']), "{png}");
    }
    let csv = std::fs::read_to_string(dir.join("pivot-bom.csv")).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "Item,Part,Name,Quantity,Volume (mm^3),Material,Mass (g)");
    // (No material: a pin weighs as much water would.)
    assert_eq!(lines[3], "3,pin,Pin,2,1445.133,,1.445", "{csv}");
    assert_eq!(lines.len(), 5);

    // Part paths are stored relative to the assembly: the folder can move.
    let moved = scratch("moved");
    for f in ["pivot.tenonasm", "base.tenon", "arm.tenon", "pin.tenon"] {
        std::fs::copy(dir.join(f), moved.join(f)).unwrap();
    }
    let mut e = Engine::new(kernel(), &moved);
    let opened = e.exec("asm.open", &json!({ "path": "pivot.tenonasm" })).unwrap();
    // block.tenon was not copied: it is reported missing, the rest opens.
    let missing = opened["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 1, "{opened}");
    assert!(missing[0].as_str().unwrap().contains("block.tenon"));
    let tree = e.exec("asm.tree", &json!({})).unwrap();
    assert!(tree["components"][4]["missing"].is_string());
    assert!(tree["components"][1]["part"].as_str().unwrap().starts_with(moved.to_str().unwrap()));
    let update = e.exec("asm.update", &json!({})).unwrap();
    assert_eq!(update["failing"][0]["name"], "Slider:1", "{update}");
    let tree = e.exec("asm.tree", &json!({})).unwrap();
    assert!(tree["relationships"][4]["failing"].as_str().unwrap().contains("missing"), "{tree}");
    // The other components keep their places.
    assert!((tree["components"][2]["placement"]["origin"][2].as_f64().unwrap() - 18.0).abs() < 1e-6);
}

#[test]
fn assembly_edits_undo_and_conflicts_are_refused() {
    let dir = scratch("undo");
    let r = tenon_cli::demo_m3(kernel(), &dir).unwrap();
    assert_eq!(r.json["ok"], true, "{}", r.text);
    let mut e = Engine::new(kernel(), &dir);
    e.exec("asm.open", &json!({ "path": "pivot.tenonasm" })).unwrap();
    let block = |e: &mut Engine| e.exec("asm.tree", &json!({})).unwrap()["components"][4]["placement"]["origin"][0].as_f64().unwrap();
    assert!((block(&mut e) - 30.0).abs() < 1e-6);
    e.exec("asm.move", &json!({ "component": 5, "by": [12, 0, 0] })).unwrap();
    assert!((block(&mut e) - 42.0).abs() < 1e-6);
    e.exec("asm.undo", &json!({})).unwrap();
    assert!((block(&mut e) - 30.0).abs() < 1e-6);
    e.exec("asm.redo", &json!({})).unwrap();
    assert!((block(&mut e) - 42.0).abs() < 1e-6);

    // Pinning the block to the base's front face as well as the slider: the block's front face
    // flush with the base's right face cannot hold with the slider. Refused; nothing changes.
    let base_right = e.exec("asm.geom", &json!({ "component": 1, "face": { "type": "side", "feature": 2, "curve": 7 } })).unwrap();
    let block_front = e.exec("asm.geom", &json!({ "component": 5, "face": { "type": "side", "feature": 2, "curve": 6 } })).unwrap();
    let before = e.exec("asm.tree", &json!({})).unwrap();
    let err = e.exec("asm.constrain", &json!({ "type": "flush", "a": base_right, "b": block_front })).unwrap_err();
    assert!(err.contains("Flush:1") || err.contains("conflicts"), "{err}");
    assert_eq!(e.exec("asm.tree", &json!({})).unwrap(), before);

    // A grounded component does not move; the arm joined to it cannot be dragged off its pin.
    let err = e.exec("asm.move", &json!({ "component": 1, "by": [1, 0, 0] })).unwrap_err();
    assert!(err.contains("grounded"), "{err}");
    let arm = e.exec("asm.move", &json!({ "component": 2, "by": [5, 5, 5] })).unwrap();
    assert!((arm["placement"]["origin"][0].as_f64().unwrap() - 28.0).abs() < 1e-6, "{arm}");

    // Suppressing the angle frees the arm to turn about its pin again.
    e.exec("asm.suppress", &json!({ "relationship": 2 })).unwrap();
    let dof = e.exec("asm.dof", &json!({})).unwrap();
    assert_eq!(dof["components"][1]["dof"], 1);
    e.exec("asm.move", &json!({ "component": 2, "turn": { "axis": "z", "angle": 0.5 } })).unwrap();
    // It turned about the pin (the joint), not its middle: its hole stays on the base's hole.
    let tree = e.exec("asm.tree", &json!({})).unwrap();
    let f = &tree["components"][1]["placement"];
    let v = |k: &str, i: usize| f[k][i].as_f64().unwrap();
    let hole = [v("origin", 0) + 8.0 * v("x", 0) + 8.0 * v("y", 0), v("origin", 1) + 8.0 * v("x", 1) + 8.0 * v("y", 1)];
    assert!((hole[0] - 20.0).abs() < 1e-6 && (hole[1] - 20.0).abs() < 1e-6, "{hole:?}");
    // Editing the angle drives it.
    e.exec("asm.suppress", &json!({ "relationship": 2, "suppressed": false })).unwrap();
    e.exec("asm.edit_relationship", &json!({ "relationship": 2, "angle": PI })).unwrap();
    let tree = e.exec("asm.tree", &json!({})).unwrap();
    assert!((tree["components"][1]["placement"]["x"][0].as_f64().unwrap() + 1.0).abs() < 1e-6, "{tree}");
    // Deleting a component takes its relationships with it.
    e.exec("asm.delete", &json!({ "component": 3 })).unwrap();
    let tree = e.exec("asm.tree", &json!({})).unwrap();
    assert_eq!(tree["components"].as_array().unwrap().len(), 4);
    assert!(tree["relationships"].as_array().unwrap().iter().all(|r| r["name"] != "Insert:1"));
}
