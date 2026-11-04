//! Pattern, Mirror and Copy of an assembly's components (DEC-130), through the commands a script
//! uses: each against figures worked out by hand.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tenon_cli::Engine;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

fn kernel() -> Box<dyn Kernel> {
    Box::new(OcctKernel::new())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-patterns-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(e: &mut Engine, id: &str, p: Value) -> Value {
    e.exec(id, &p).unwrap_or_else(|err| panic!("{id}: {err}"))
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn vec3(v: &Value) -> [f64; 3] {
    [num(&v[0]), num(&v[1]), num(&v[2])]
}

/// Whether two points or directions are the same, to a millionth of a millimetre.
#[track_caller]
fn same<const N: usize>(a: [f64; N], b: [f64; N]) {
    assert!((0..N).all(|i| (a[i] - b[i]).abs() < 1e-6), "{a:?} is not {b:?}");
}

/// A box part, `x` by `y` by `z`, with a corner at its origin.
fn box_part(e: &mut Engine, name: &str, x: f64, y: f64, z: f64) {
    run(e, "file.new", json!({ "name": name }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": x, "y2": y }));
    run(e, "model.extrude", json!({ "sketch": sk, "distance": z }));
    run(e, "file.save", json!({ "path": format!("{name}.tenon") }));
}

/// A foot 40 x 20 and `t` thick (a parameter, 10) with a 10 x 10 post on its corner at the origin,
/// up to 30: 8000 + 2000 = 10000 mm^3, with its centre of mass at (17, 9, 8). It is not the same as
/// its mirror image however it is turned: a mirrored one is another part.
fn ell(e: &mut Engine) {
    run(e, "file.new", json!({ "name": "ell" }));
    run(e, "param.add", json!({ "name": "t", "equation": "10 mm" }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }));
    run(e, "model.extrude", json!({ "sketch": sk, "distance": 10, "equations": { "/extent/distance": "t" } }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 10, "y2": 10 }));
    run(e, "model.extrude", json!({ "sketch": sk, "distance": 30, "operation": "join" }));
    run(e, "document.rename", json!({ "name": "Ell" }));
    run(e, "file.save", json!({ "path": "ell.tenon" }));
}

fn component<'a>(tree: &'a Value, id: &Value) -> &'a Value {
    tree["components"].as_array().unwrap().iter().find(|c| c["id"] == *id).unwrap_or_else(|| panic!("no component {id} in {tree}"))
}

/// Where a component's part origin stands.
fn origin(e: &mut Engine, id: &Value) -> [f64; 3] {
    let tree = run(e, "asm.tree", json!({}));
    vec3(&component(&tree, id)["placement"]["origin"])
}

fn ids(v: &Value) -> Vec<Value> {
    v.as_array().unwrap().clone()
}

/// A plain assembly: a part `rail` under a free block on its top.
fn rail_and_block(e: &mut Engine) -> (Value, Value) {
    box_part(e, "rail", 100.0, 20.0, 5.0);
    box_part(e, "block", 10.0, 10.0, 10.0);
    run(e, "asm.new", json!({ "name": "Rows" }));
    let rail = run(e, "asm.insert", json!({ "path": "rail.tenon" }))["component"].clone();
    let block = run(e, "asm.insert", json!({ "path": "block.tenon", "at": [0, 0, 5] }))["component"].clone();
    (rail, block)
}

#[test]
fn a_rectangular_pattern_repeats_components_in_rows_and_follows_its_counts() {
    let dir = scratch("rect");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    let (_, block) = rail_and_block(e);
    let (x, y) = (run(e, "asm.geom", json!({ "axis": "x" })), run(e, "asm.geom", json!({ "axis": "y" })));

    // Three along X 20 apart, two rows along Y 15 apart: an element wherever i and j say.
    let made = run(
        e,
        "asm.pattern",
        json!({ "components": [block], "type": "rectangular", "direction": x, "count": 3, "spacing": 20, "direction2": y, "count2": 2, "spacing2": 15 }),
    );
    assert_eq!(made["name"], "Pattern:1");
    assert_eq!(made["elements"], 5, "six places, the source among them");
    let pattern = made["pattern"].clone();
    let elements = ids(&made["components"]);
    for (el, at) in elements.iter().zip([[20.0, 0.0], [40.0, 0.0], [0.0, 15.0], [20.0, 15.0], [40.0, 15.0]]) {
        same(origin(e, el), [at[0], at[1], 5.0]);
    }

    // The parts list counts them; the total weighs them. 16 grams at 1 g/cm^3: the rail's 10000
    // mm^3 with its middle at (50, 10, 2.5), the blocks' 6000 with theirs at (25, 12.5, 10).
    let bom = run(e, "asm.bom", json!({}));
    assert_eq!(bom["rows"][1]["quantity"], 6, "{bom}");
    assert_eq!(bom["rows"][0]["quantity"], 1);
    assert_eq!(bom["components"], 7);
    let mass = run(e, "asm.mass", json!({}));
    assert!((num(&mass["volume"]) - 16000.0).abs() < 1e-6, "{mass}");
    same(vec3(&mass["center_of_mass"]), [650000.0 / 16000.0, 175000.0 / 16000.0, 85000.0 / 16000.0]);

    // In the browser's words: one record, each element a component held by it.
    let tree = run(e, "asm.tree", json!({}));
    assert_eq!(tree["patterns"][0]["name"], "Pattern:1");
    assert_eq!(tree["patterns"][0]["type"], "rectangular");
    assert_eq!(tree["patterns"][0]["elements"][4]["at"], json!([2, 1]));
    assert_eq!(component(&tree, &elements[0])["element_of"], pattern);
    assert_eq!(component(&tree, &block)["element_of"], Value::Null, "the source is not an element");
    assert_eq!(component(&tree, &elements[0])["dof"], 0, "the rule holds it");
    assert_eq!(component(&tree, &block)["dof"], 6, "it is free to move, and they with it");

    // Four along X: two more elements. The ones there stay the same components.
    let four = run(e, "asm.edit_pattern", json!({ "pattern": pattern, "count": 4 }));
    assert_eq!(four["elements"], 7);
    same(origin(e, &elements[1]), [40.0, 0.0, 5.0]);
    let bom = run(e, "asm.bom", json!({}));
    assert_eq!(bom["rows"][1]["quantity"], 8);
    let new: Vec<Value> = ids(&four["components"]).into_iter().filter(|c| !elements.contains(c)).collect();
    assert_eq!(new.len(), 2);
    let tree = run(e, "asm.tree", json!({}));
    let at_new: Vec<[f64; 3]> = new.iter().map(|c| vec3(&component(&tree, c)["placement"]["origin"])).collect();
    assert!(at_new.iter().any(|p| (p[0] - 60.0).abs() < 1e-6 && p[1].abs() < 1e-6), "{at_new:?}");
    assert!(at_new.iter().any(|p| (p[0] - 60.0).abs() < 1e-6 && (p[1] - 15.0).abs() < 1e-6), "{at_new:?}");

    // A wider spacing moves them; no second direction, and only one row is left.
    run(e, "asm.edit_pattern", json!({ "pattern": pattern, "spacing": 25 }));
    same(origin(e, &elements[1]), [50.0, 0.0, 5.0]);
    let one_row = run(e, "asm.edit_pattern", json!({ "pattern": pattern, "direction2": null }));
    assert_eq!(one_row["elements"], 3);
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][1]["quantity"], 4);
    // (All this undoes, step by step.)
    run(e, "asm.undo", json!({}));
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][1]["quantity"], 8);
    run(e, "asm.undo", json!({}));
    same(origin(e, &elements[1]), [40.0, 0.0, 5.0]);
    run(e, "asm.undo", json!({}));
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][1]["quantity"], 6);

    // Suppressed its elements are out of the assembly (the source stays), and back they come.
    let v = |e: &mut Engine| num(&run(e, "asm.mass", json!({}))["volume"]);
    run(e, "asm.suppress_pattern", json!({ "pattern": pattern }));
    assert!((v(e) - 11000.0).abs() < 1e-6);
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][1]["quantity"], 1);
    assert_eq!(run(e, "asm.tree", json!({}))["patterns"][0]["suppressed"], true);
    let err = e.exec("asm.visible", &json!({ "component": elements[0], "visible": true })).unwrap_err();
    assert!(err.contains("suppressed"), "{err}");
    run(e, "asm.suppress_pattern", json!({ "pattern": pattern, "suppressed": false }));
    assert!((v(e) - 16000.0).abs() < 1e-6);
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][1]["quantity"], 6);

    // An element is the pattern's to place: words say so, and nothing changes.
    let before = run(e, "asm.tree", json!({}));
    for (id, p, word) in [
        ("asm.move", json!({ "component": elements[0], "by": [1, 0, 0] }), "stands where"),
        ("asm.place", json!({ "component": elements[0], "origin": [1, 0, 0] }), "stands where"),
        ("asm.ground", json!({ "component": elements[0] }), "holds it"),
        ("asm.delete", json!({ "component": elements[0] }), "count"),
        ("asm.set_row", json!({ "component": elements[0], "row": null }), "size"),
    ] {
        let err = e.exec(id, &p).unwrap_err();
        assert!(err.contains("element of Pattern:1") && err.contains(word), "{id}: {err}");
    }
    assert_eq!(run(e, "asm.tree", json!({})), before);

    // Rename, then delete: its elements go, the source stays. Kept, they stay as components of
    // their own, free to move.
    run(e, "asm.rename", json!({ "pattern": pattern, "name": "Row of blocks" }));
    assert_eq!(run(e, "asm.tree", json!({}))["patterns"][0]["name"], "Row of blocks");
    run(e, "asm.delete_pattern", json!({ "pattern": pattern }));
    let tree = run(e, "asm.tree", json!({}));
    assert_eq!((tree["components"].as_array().unwrap().len(), tree["patterns"].as_array().unwrap().len()), (2, 0));
    run(e, "asm.undo", json!({}));
    run(e, "asm.delete_pattern", json!({ "pattern": pattern, "keep_elements": true }));
    let tree = run(e, "asm.tree", json!({}));
    assert_eq!((tree["components"].as_array().unwrap().len(), tree["patterns"].as_array().unwrap().len()), (7, 0));
    assert_eq!(component(&tree, &elements[3])["dof"], 6);
    run(e, "asm.move", json!({ "component": elements[3], "by": [0, 0, 1] }));
    same(origin(e, &elements[3]), [20.0, 15.0, 6.0]);
}

#[test]
fn a_pattern_takes_new_sources_loses_them_and_repeats_what_another_repeats() {
    let dir = scratch("sources");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    box_part(e, "peg", 2.0, 2.0, 8.0);
    let (_, block) = rail_and_block(e);
    let peg = run(e, "asm.insert", json!({ "path": "peg.tenon", "at": [4, 4, 15] }))["component"].clone();
    let x = run(e, "asm.geom", json!({ "axis": "x" }));
    let first = run(e, "asm.pattern", json!({ "components": [block], "type": "rectangular", "direction": x, "count": 3, "spacing": 20 }));
    let (pattern, blocks) = (first["pattern"].clone(), ids(&first["components"]));

    // The peg as well: each element has one of it too, each 20 further on. The blocks that were
    // there are the same components.
    let both = run(e, "asm.edit_pattern", json!({ "pattern": pattern, "components": [block, peg] }));
    assert_eq!(both["elements"], 2);
    let made = ids(&both["components"]);
    assert_eq!((made.len(), &made[0], &made[2]), (4, &blocks[0], &blocks[1]));
    same(origin(e, &made[1]), [24.0, 4.0, 15.0]);
    same(origin(e, &made[3]), [44.0, 4.0, 15.0]);
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][2]["quantity"], 3);

    // A pattern of what another makes: the far peg, three more up Y from it, following it.
    let y = run(e, "asm.geom", json!({ "axis": "y" }));
    let second = run(e, "asm.pattern", json!({ "components": [made[3]], "type": "rectangular", "direction": y, "count": 4, "spacing": 5 }));
    let last = ids(&second["components"]).pop().unwrap();
    same(origin(e, &last), [44.0, 19.0, 15.0]);
    run(e, "asm.move", json!({ "component": peg, "by": [0, 0, 1] }));
    same(origin(e, &last), [44.0, 19.0, 16.0]);
    // An earlier pattern cannot lean on what a later one makes.
    let err = e.exec("asm.edit_pattern", &json!({ "pattern": pattern, "components": [block, peg, last] })).unwrap_err();
    assert!(err.contains("depends on it"), "{err}");
    // Count 2: the far block and peg go, and the second pattern, which repeated that peg, with them.
    run(e, "asm.edit_pattern", json!({ "pattern": pattern, "count": 2 }));
    let tree = run(e, "asm.tree", json!({}));
    assert_eq!(tree["patterns"].as_array().unwrap().len(), 1, "{tree}");
    assert!(tree["components"].as_array().unwrap().iter().all(|c| c["id"] != last));
    run(e, "asm.undo", json!({}));
    assert_eq!(run(e, "asm.tree", json!({}))["patterns"].as_array().unwrap().len(), 2);
    run(e, "asm.delete_pattern", json!({ "pattern": second["pattern"] }));

    // The peg no longer repeated: what stood for it goes, and it stays; the blocks stay.
    run(e, "asm.edit_pattern", json!({ "pattern": pattern, "components": [block] }));
    let tree = run(e, "asm.tree", json!({}));
    assert_eq!(component(&tree, &peg)["dof"], 6);
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][2]["quantity"], 1, "{tree}");
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][1]["quantity"], 3);
    same(origin(e, &blocks[1]), [40.0, 0.0, 5.0]);
}

#[test]
fn a_part_in_a_size_is_mirrored_and_patterned_in_that_size_and_follows_it() {
    let dir = scratch("sizes");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    run(e, "file.new", json!({ "name": "Pin" }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 10, "y2": 4 }));
    run(e, "model.extrude", json!({ "sketch": sk, "distance": 20 }));
    run(e, "table.create", json!({ "columns": ["d0"], "row": "short" }));
    run(e, "table.add_row", json!({ "name": "long", "values": { "d0": 50 } }));
    run(e, "file.save", json!({ "path": "pin.tenon" }));
    run(e, "asm.new", json!({ "name": "Sizes" }));
    let pin = run(e, "asm.insert", json!({ "path": "pin.tenon", "at": [30, 0, 0], "row": "long" }))["component"].clone();
    let (yz, y) = (run(e, "asm.geom", json!({ "plane": "yz" })), run(e, "asm.geom", json!({ "axis": "y" })));
    let m = run(e, "asm.mirror", json!({ "components": [pin], "plane": yz }));
    run(e, "asm.pattern", json!({ "components": [pin], "type": "rectangular", "direction": y, "count": 2, "spacing": 10 }));
    let rows = |e: &mut Engine| {
        let bom = run(e, "asm.bom", json!({}));
        bom["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (r["part"].as_str().unwrap().to_owned(), r["name"].as_str().unwrap().to_owned(), r["quantity"].as_u64().unwrap(), num(&r["volume"]))
            })
            .collect::<Vec<_>>()
    };
    // 10 x 4 x 50 = 2000 mm^3: the mirror image of that size, on a row of its own.
    let r = rows(e);
    assert_eq!((r[0].0.as_str(), r[0].1.as_str(), r[0].2), ("pin (long)", "Pin, long", 2));
    assert_eq!((r[1].0.as_str(), r[1].1.as_str(), r[1].2), ("pin (long) (mirrored)", "Pin, long, mirrored", 1));
    assert!((r[1].3 - 2000.0).abs() < 1e-6, "{r:?}");
    // The source at the file's own size: everything that repeats it follows. 10 x 4 x 20 = 800.
    run(e, "asm.set_row", json!({ "component": pin, "row": null }));
    let r = rows(e);
    assert_eq!((r[0].0.as_str(), r[0].2), ("pin", 2), "{r:?}");
    assert_eq!((r[1].0.as_str(), r[1].2), ("pin (mirrored)", 1), "{r:?}");
    assert!((r[1].3 - 800.0).abs() < 1e-6, "{r:?}");
    let image = ids(&m["components"])[0].clone();
    assert_eq!(component(&run(e, "asm.tree", json!({})), &image)["row"], Value::Null);
    // Saved and opened: the same.
    run(e, "asm.save", json!({ "path": "sizes.tenonasm" }));
    run(e, "asm.open", json!({ "path": "sizes.tenonasm" }));
    assert_eq!(rows(e)[1].0, "pin (mirrored)");
}

#[test]
fn a_pattern_follows_what_it_depends_on_and_what_is_held_to_an_element_follows_it() {
    let dir = scratch("follow");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    let (rail, block) = rail_and_block(e);
    run(e, "asm.ground", json!({ "component": rail, "grounded": false }));
    // Along the rail's own X axis, which turns with it.
    let along = run(e, "asm.geom", json!({ "component": rail, "axis": "x" }));
    let made = run(e, "asm.pattern", json!({ "components": [block], "type": "rectangular", "direction": along, "count": 3, "spacing": 20 }));
    let (pattern, elements) = (made["pattern"].clone(), ids(&made["components"]));

    // The source moves, and they with it.
    run(e, "asm.move", json!({ "component": block, "by": [3, 4, 5] }));
    same(origin(e, &elements[0]), [23.0, 4.0, 10.0]);
    same(origin(e, &elements[1]), [43.0, 4.0, 10.0]);

    // The rail is turned a quarter turn about its own Z: the rows now run along Y.
    run(e, "asm.move", json!({ "component": rail, "turn": { "axis": "z", "angle": FRAC_PI_2 } }));
    same(origin(e, &block), [3.0, 4.0, 10.0]);
    same(origin(e, &elements[0]), [3.0, 24.0, 10.0]);
    same(origin(e, &elements[1]), [3.0, 44.0, 10.0]);
    // Undone, it all goes back where it was.
    run(e, "asm.undo", json!({}));
    same(origin(e, &elements[1]), [43.0, 4.0, 10.0]);
    // Its direction given another way: a rule works from what it is told.
    let y = run(e, "asm.geom", json!({ "axis": "y" }));
    run(e, "asm.edit_pattern", json!({ "pattern": pattern, "direction": y }));
    same(origin(e, &elements[1]), [3.0, 44.0, 10.0]);
}

#[test]
fn an_element_can_be_constrained_to_and_a_relationship_on_it_is_kept_while_it_is_there() {
    let dir = scratch("held");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    box_part(e, "cap", 10.0, 10.0, 2.0);
    let (_, block) = rail_and_block(e);
    let x = run(e, "asm.geom", json!({ "axis": "x" }));
    let made = run(e, "asm.pattern", json!({ "components": [block], "type": "rectangular", "direction": x, "count": 3, "spacing": 20 }));
    let (pattern, elements) = (made["pattern"].clone(), ids(&made["components"]));
    let cap = run(e, "asm.insert", json!({ "path": "cap.tenon", "at": [0, 50, 0] }))["component"].clone();

    // On the last element, flush and turned as it is: the cap's origin stands where the element's does.
    let (a, b) =
        (run(e, "asm.geom", json!({ "component": elements[1], "origin": true })), run(e, "asm.geom", json!({ "component": cap, "origin": true })));
    run(e, "asm.joint", json!({ "type": "rigid", "a": a, "b": b, "flip": true }));
    same(origin(e, &cap), [40.0, 0.0, 5.0]);
    assert_eq!(run(e, "asm.dof", json!({}))["dof"], 6, "the block's six: the cap is held, by the element");

    // Whatever moves the element moves the cap: the source, the spacing.
    run(e, "asm.move", json!({ "component": block, "by": [1, 2, 3] }));
    same(origin(e, &elements[1]), [41.0, 2.0, 8.0]);
    same(origin(e, &cap), [41.0, 2.0, 8.0]);
    run(e, "asm.edit_pattern", json!({ "pattern": pattern, "spacing": 30 }));
    same(origin(e, &cap), [61.0, 2.0, 8.0]);

    // A count that takes the element away takes what was held to it with it, as the panel warns.
    run(e, "asm.edit_pattern", json!({ "pattern": pattern, "count": 2 }));
    let tree = run(e, "asm.tree", json!({}));
    assert_eq!(tree["relationships"].as_array().unwrap().len(), 0, "{tree}");
    assert_eq!(component(&tree, &cap)["dof"], 6);

    // Held to its own source it cannot settle: refused, in words, and nothing changes.
    let before = run(e, "asm.tree", json!({}));
    let (a, b) =
        (run(e, "asm.geom", json!({ "component": elements[0], "origin": true })), run(e, "asm.geom", json!({ "component": block, "origin": true })));
    let err = e.exec("asm.joint", &json!({ "type": "rigid", "a": a, "b": b, "flip": true })).unwrap_err();
    assert!(err.contains("Rigid:"), "{err}");
    assert_eq!(run(e, "asm.tree", json!({})), before);

    // What a pattern takes its direction from goes: the pattern goes, its elements stay, free.
    let rail = component(&before, &json!(1))["id"].clone();
    let along = run(e, "asm.geom", json!({ "component": rail, "axis": "y" }));
    let more = run(e, "asm.pattern", json!({ "components": [cap], "type": "rectangular", "direction": along, "count": 2, "spacing": 12 }));
    let second = ids(&more["components"])[0].clone();
    run(e, "asm.delete", json!({ "component": rail }));
    let tree = run(e, "asm.tree", json!({}));
    assert_eq!(tree["patterns"].as_array().unwrap().len(), 1, "only the first: {tree}");
    assert_eq!(component(&tree, &second)["element_of"], Value::Null);
    assert_eq!(component(&tree, &second)["dof"], 6);
    // What a pattern repeats goes: what stands for it goes too.
    run(e, "asm.delete", json!({ "component": block }));
    let tree = run(e, "asm.tree", json!({}));
    assert_eq!(tree["patterns"].as_array().unwrap().len(), 0, "{tree}");
    assert_eq!(tree["components"].as_array().unwrap().len(), 2, "the cap and its copy: {tree}");
}

#[test]
fn a_circular_pattern_spreads_components_round_an_axis() {
    let dir = scratch("round");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    box_part(e, "bolt", 4.0, 4.0, 10.0);
    box_part(e, "hub", 20.0, 20.0, 5.0);
    run(e, "asm.new", json!({ "name": "Round" }));
    run(e, "asm.insert", json!({ "path": "hub.tenon" }));
    let bolt = run(e, "asm.insert", json!({ "path": "bolt.tenon", "at": [30, 0, 0] }))["component"].clone();
    let z = run(e, "asm.geom", json!({ "axis": "z" }));

    // Six, a whole turn: 60 degrees apart, each turned with the turn.
    let made = run(e, "asm.pattern", json!({ "components": [bolt], "type": "circular", "axis": z, "count": 6 }));
    let (pattern, elements) = (made["pattern"].clone(), ids(&made["components"]));
    assert_eq!(elements.len(), 5);
    let tree = run(e, "asm.tree", json!({}));
    for (k, el) in elements.iter().enumerate() {
        let a = (k + 1) as f64 * PI / 3.0;
        let f = &component(&tree, el)["placement"];
        same(vec3(&f["origin"]), [30.0 * a.cos(), 30.0 * a.sin(), 0.0]);
        same(vec3(&f["x"]), [a.cos(), a.sin(), 0.0]);
        same(vec3(&f["z"]), [0.0, 0.0, 1.0]);
    }
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][1]["quantity"], 6);
    // Their volumes are six bolts' (the hub's 2000 and 160 mm^3 each) wherever they stand.
    assert!((num(&run(e, "asm.mass", json!({}))["volume"]) - (2000.0 + 6.0 * 160.0)).abs() < 1e-6);

    // Part of a turn: four over 90 degrees are 30 apart, from the first to the last.
    let q = run(e, "asm.edit_pattern", json!({ "pattern": pattern, "count": 4, "angle": FRAC_PI_2 }));
    assert_eq!(q["elements"], 3);
    let last = ids(&q["components"]).pop().unwrap();
    same(origin(e, &last), [0.0, 30.0, 0.0]);
    let middle = ids(&q["components"])[0].clone();
    same(origin(e, &middle), [30.0 * (PI / 6.0).cos(), 30.0 * (PI / 6.0).sin(), 0.0]);
    // The other way round.
    run(e, "asm.edit_pattern", json!({ "pattern": pattern, "reverse": true }));
    same(origin(e, &last), [0.0, -30.0, 0.0]);
    // What cannot be an axis or a plane is told in words.
    let xy = run(e, "asm.geom", json!({ "plane": "xy" }));
    let wrong = e.exec("asm.edit_pattern", &json!({ "pattern": pattern, "axis": xy })).unwrap_err();
    assert!(wrong.contains("axis"), "{wrong}");
    let mirror_wrong = e.exec("asm.mirror", &json!({ "components": [bolt], "plane": z })).unwrap_err();
    assert!(mirror_wrong.contains("flat"), "{mirror_wrong}");

    // Hostile numbers are refused, and nothing changes; what means nothing for it too.
    let before = run(e, "asm.tree", json!({}));
    for bad in [json!({ "spacing": 5 }), json!({ "plane": xy }), json!({ "direction": z })] {
        let mut p = json!({ "pattern": pattern });
        p.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        let err = e.exec("asm.edit_pattern", &p).unwrap_err();
        assert!(err.contains("means nothing"), "{err}");
    }
    let err = e
        .exec("asm.pattern", &json!({ "components": [bolt], "type": "rectangular", "direction": z, "count": 2, "spacing": 5, "axis": z }))
        .unwrap_err();
    assert!(err.contains("means nothing"), "{err}");
    let err = e.exec("asm.pattern", &json!({ "components": [bolt], "type": "rectangular", "direction": z, "count": 5000, "spacing": 5, "direction2": z, "count2": 5000, "spacing2": 5 })).unwrap_err();
    assert!(err.contains("more than 5000 components"), "{err}");
    let err =
        e.exec("asm.pattern", &json!({ "components": [bolt], "type": "rectangular", "direction": z, "count": 5000, "spacing": 5 })).unwrap_err();
    assert!(err.contains("more than 5000 components"), "with what is there already: {err}");
    let err = e.exec("asm.pattern", &json!({ "components": [bolt], "type": "rectangular", "direction": z, "count": 3, "spacing": 0 })).unwrap_err();
    assert!(err.contains("spacing must be positive"), "{err}");
    let err = e
        .exec("asm.pattern", &json!({ "components": [bolt], "type": "rectangular", "direction": z, "count": 3, "spacing": 5, "count2": 3 }))
        .unwrap_err();
    assert!(err.contains("direction2"), "{err}");
    for bad in [json!({ "count": 0 }), json!({ "count": 1 }), json!({ "count": 100_000 }), json!({ "angle": 0 }), json!({ "angle": 7 })] {
        let mut p = json!({ "pattern": pattern });
        p.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert!(e.exec("asm.edit_pattern", &p).is_err(), "{p}");
    }
    assert!(e.exec("asm.pattern", &json!({ "components": [], "type": "circular", "axis": z, "count": 3 })).is_err());
    assert!(e.exec("asm.pattern", &json!({ "components": [999], "type": "circular", "axis": z, "count": 3 })).is_err());
    assert!(e.exec("asm.edit_pattern", &json!({ "pattern": 77, "count": 3 })).is_err());
    assert!(e.exec("asm.delete_pattern", &json!({ "pattern": 77 })).is_err());
    assert_eq!(run(e, "asm.tree", json!({})), before);
}

/// The solids of a STEP file, each with its volume, centre of mass and box.
fn solids_of(path: &Path) -> Vec<(f64, [f64; 3], [f64; 3], [f64; 3])> {
    let mut k = OcctKernel::new();
    let shapes = k.import_step(&std::fs::read(path).unwrap()).unwrap();
    shapes
        .iter()
        .map(|s| {
            let m = k.mass_properties(*s, 1.0).unwrap();
            let b = k.bounding_box(*s).unwrap().unwrap();
            let v = |p: tenon_geom::Vec3| [p.x, p.y, p.z];
            (m.volume, v(m.center_of_mass), v(b.min), v(b.max))
        })
        .collect()
}

/// A mirrored L at x -100 to -140 from one at x 100 to 140, mirrored in the assembly's YZ plane.
fn mirrored_ell(e: &mut Engine) -> (Value, Value, Value) {
    ell(e);
    run(e, "asm.new", json!({ "name": "Pair" }));
    let source = run(e, "asm.insert", json!({ "path": "ell.tenon", "at": [100, 0, 0], "grounded": false }))["component"].clone();
    let plane = run(e, "asm.geom", json!({ "plane": "yz" }));
    let made = run(e, "asm.mirror", json!({ "components": [source], "plane": plane }));
    assert_eq!(made["name"], "Mirror:1");
    let element = ids(&made["components"])[0].clone();
    (source, element, made["pattern"].clone())
}

#[test]
fn a_mirrored_occurrence_holds_its_part_reflected_wherever_it_is_read() {
    let dir = scratch("mirror");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    let (source, element, _) = mirrored_ell(e);

    // The same part file; reflected; at a rigid frame that with the part's XY plane reflected
    // comes to the source's image: origin (-100, 0, 0), X and Z each the other way.
    let tree = run(e, "asm.tree", json!({}));
    let el = component(&tree, &element);
    assert_eq!(el["mirrored"], true);
    assert_eq!(el["part"], component(&tree, &source)["part"]);
    assert_eq!(el["name"], "ell:2");
    same(vec3(&el["placement"]["origin"]), [-100.0, 0.0, 0.0]);
    same(vec3(&el["placement"]["x"]), [-1.0, 0.0, 0.0]);
    same(vec3(&el["placement"]["y"]), [0.0, 1.0, 0.0]);
    same(vec3(&el["placement"]["z"]), [0.0, 0.0, -1.0]);

    // A row of its own in the parts list, as a mirrored part, of the same volume and mass.
    let bom = run(e, "asm.bom", json!({}));
    assert_eq!(bom["parts"], 2, "{bom}");
    assert_eq!(
        (&bom["rows"][1]["part"], &bom["rows"][1]["name"], &bom["rows"][1]["mirrored"]),
        (&json!("ell (mirrored)"), &json!("Ell, mirrored"), &json!(true))
    );
    assert_eq!((&bom["rows"][0]["quantity"], &bom["rows"][1]["quantity"], &bom["rows"][0]["mirrored"]), (&json!(1), &json!(1), &json!(false)));
    assert!((num(&bom["rows"][1]["volume"]) - 10000.0).abs() < 1e-6, "{bom}");
    run(e, "asm.export_bom", json!({ "path": "pair.csv" }));
    let csv = std::fs::read_to_string(dir.join("pair.csv")).unwrap();
    assert!(csv.lines().nth(2).unwrap().starts_with("2,ell (mirrored),\"Ell, mirrored\",1,"), "{csv}");

    // Weighed: the two centres of mass (117, 9, 8) and (-117, 9, 8), half way between.
    let mass = run(e, "asm.mass", json!({}));
    assert!((num(&mass["volume"]) - 20000.0).abs() < 1e-6);
    same(vec3(&mass["center_of_mass"]), [0.0, 9.0, 8.0]);

    // Exported to STEP: a solid whose centre and box are the image of the source's. The post, at
    // x 100 to 110 on the source, stands at -110 to -100: an unreflected part at that frame would
    // have its centre of mass at z = -8, and one turned half round its y = 11.
    run(e, "asm.export_step", json!({ "path": "pair.step" }));
    let solids = solids_of(&dir.join("pair.step"));
    assert_eq!(solids.len(), 2);
    let image = solids.iter().find(|s| s.1[0] < 0.0).unwrap();
    assert!((image.0 - 10000.0).abs() < 1e-3, "{image:?}");
    same(image.1, [-117.0, 9.0, 8.0]);
    same(image.2, [-140.0, 0.0, 0.0]);
    same(image.3, [-100.0, 20.0, 30.0]);

    // Exported as a mesh: closed, its faces wound outward, so that the volume it encloses comes
    // out positive, the two L's together, and every normal written agrees with its winding.
    let r = run(e, "asm.export_stl", json!({ "path": "pair.stl" }));
    assert_eq!(r["watertight"], true, "{r}");
    let data = std::fs::read(dir.join("pair.stl")).unwrap();
    let (mut signed, mut left) = (0.0, 0.0);
    for t in data[84..].as_chunks::<50>().0 {
        let f = |i: usize| f32::from_le_bytes(t[i..i + 4].try_into().unwrap()) as f64;
        let (a, b, c) = ([f(12), f(16), f(20)], [f(24), f(28), f(32)], [f(36), f(40), f(44)]);
        let n = [f(0), f(4), f(8)];
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let w = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        assert!(n[0] * w[0] + n[1] * w[1] + n[2] * w[2] >= 0.0, "{n:?} against {w:?}");
        let tetra = (a[0] * w[0] + a[1] * w[1] + a[2] * w[2]) / 6.0;
        signed += tetra;
        if a[0] < 0.0 {
            left += tetra;
        }
    }
    assert!((signed - 20000.0).abs() < 1e-1, "{signed}");
    assert!((left - 10000.0).abs() < 1e-1, "the mirrored one alone: {left}");
    let r = run(e, "asm.export_3mf", json!({ "path": "pair.3mf" }));
    assert_eq!((&r["objects"], &r["items"], &r["watertight"]), (&json!(2), &json!(2), &json!(true)), "{r}");
}

#[test]
fn a_mirrored_occurrence_is_checked_for_interference_where_it_truly_stands() {
    let dir = scratch("mirror-clash");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    box_part(e, "probe", 10.0, 20.0, 10.0);
    let (_, element, _) = mirrored_ell(e);
    assert_eq!(run(e, "asm.interference", json!({}))["count"], 0);
    // Over x -105 to -95, y 0 to 20, z 15 to 25: through the mirrored post (x -110 to -100, y 0 to
    // 10) by 5 mm, so 5 x 10 x 10 = 500 mm^3. An unreflected part at that frame would hang from z 0
    // to -30 and miss.
    run(e, "asm.insert", json!({ "path": "probe.tenon", "at": [-105, 0, 15] }));
    let clash = run(e, "asm.interference", json!({}));
    assert_eq!(clash["count"], 1, "{clash}");
    assert!((num(&clash["volume"]) - 500.0).abs() < 1e-3, "{clash}");
    let name = component(&run(e, "asm.tree", json!({})), &element)["name"].clone();
    assert_eq!(clash["clashes"][0]["a"], name);
}

#[test]
fn a_mirror_follows_its_plane_its_source_and_its_part() {
    let dir = scratch("mirror-follow");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    box_part(e, "wall", 2.0, 50.0, 50.0);
    ell(e);
    run(e, "asm.new", json!({ "name": "Against a wall" }));
    let wall = run(e, "asm.insert", json!({ "path": "wall.tenon", "at": [0, 0, 0] }))["component"].clone();
    let source = run(e, "asm.insert", json!({ "path": "ell.tenon", "at": [20, 0, 0] }))["component"].clone();
    run(e, "asm.ground", json!({ "component": wall, "grounded": false }));

    // In the wall's flat face at x = 2, picked as a flat face is: the source's origin 18 from it,
    // so the mirrored one's at x = -16.
    let flat = (0..6)
        .map(|i| run(e, "asm.geom", json!({ "component": wall, "face": { "body": 0, "index": i } })))
        .find(|t| {
            let f = t["geom"]["face"]["fingerprint"].clone();
            t["kind"] == "plane" && (num(&f["centroid"]["x"]) - 2.0).abs() < 1e-6
        })
        .unwrap();
    let made = run(e, "asm.mirror", json!({ "components": [source], "plane": flat }));
    let element = ids(&made["components"])[0].clone();
    same(origin(e, &element), [-16.0, 0.0, 0.0]);

    // The source moves away by 5: its image the other way by 5.
    run(e, "asm.move", json!({ "component": source, "by": [5, 0, 0] }));
    same(origin(e, &element), [-21.0, 0.0, 0.0]);
    // The wall moves by 3: the image by twice that.
    run(e, "asm.move", json!({ "component": wall, "by": [3, 0, 0] }));
    same(origin(e, &element), [-15.0, 0.0, 0.0]);

    // The part is changed: t from 10 to 12 makes the foot 40 x 20 x 12 and the post above it 10 x
    // 10 x 18, so 11400 mm^3, and its image the same.
    run(e, "asm.edit_part", json!({ "component": source, "run": "param.set", "with": { "name": "t", "equation": "12" } }));
    let bom = run(e, "asm.bom", json!({}));
    let rows = bom["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3, "{bom}");
    for r in &rows[1..] {
        assert!((num(&r["volume"]) - 11400.0).abs() < 1e-6, "{r}");
    }

    // An edit on the mirrored occurrence is an edit of its part, which both follow.
    run(e, "asm.edit_part", json!({ "component": element, "run": "param.set", "with": { "name": "t", "equation": "10" } }));
    let bom = run(e, "asm.bom", json!({}));
    assert!((num(&bom["rows"][2]["volume"]) - 10000.0).abs() < 1e-6, "{bom}");

    // Mirrored again in the same plane: an occurrence of the part itself, not of its mirror image
    // (and in the file's words a part it already lists).
    let wall_flat = run(e, "asm.tree", json!({}))["patterns"][0].clone();
    assert_eq!(wall_flat["type"], "mirror");
    let plane = run(e, "asm.geom", json!({ "plane": "yz" }));
    let twice = run(e, "asm.mirror", json!({ "components": [element], "plane": plane }));
    let back = ids(&twice["components"])[0].clone();
    let tree = run(e, "asm.tree", json!({}));
    assert_eq!(component(&tree, &back)["mirrored"], false);
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][1]["quantity"], 2, "the part's own row");
    same(vec3(&component(&tree, &back)["placement"]["x"]), [1.0, 0.0, 0.0]);
    same(origin(e, &back), [15.0, 0.0, 0.0]);

    // The mirror's plane changed for another: it goes on from there.
    run(e, "asm.edit_pattern", json!({ "pattern": made["pattern"], "plane": plane }));
    same(origin(e, &element), [-25.0, 0.0, 0.0]);

    // Told what means nothing for a mirror, it says so.
    let err = e.exec("asm.edit_pattern", &json!({ "pattern": made["pattern"], "count": 3 })).unwrap_err();
    assert!(err.contains("means nothing"), "{err}");

    // Suppressed, the image is out, and the mirrored row with it.
    run(e, "asm.delete_pattern", json!({ "pattern": twice["pattern"] }));
    run(e, "asm.suppress_pattern", json!({ "pattern": made["pattern"] }));
    let bom = run(e, "asm.bom", json!({}));
    assert_eq!(bom["parts"], 2, "{bom}");
    run(e, "asm.suppress_pattern", json!({ "pattern": made["pattern"], "suppressed": false }));
    assert_eq!(run(e, "asm.bom", json!({}))["parts"], 3);
    // Its source deleted, the image goes with it.
    run(e, "asm.delete", json!({ "component": source }));
    let tree = run(e, "asm.tree", json!({}));
    assert_eq!((tree["components"].as_array().unwrap().len(), tree["patterns"].as_array().unwrap().len()), (1, 0), "{tree}");
    assert_eq!(run(e, "asm.bom", json!({}))["parts"], 1);
    run(e, "asm.undo", json!({}));
    // Made independent, it stays reflected, a mirrored part and free to move; its source can go.
    run(e, "asm.delete_pattern", json!({ "pattern": made["pattern"], "keep_elements": true }));
    let tree = run(e, "asm.tree", json!({}));
    assert_eq!(
        (&component(&tree, &element)["mirrored"], &component(&tree, &element)["element_of"], &component(&tree, &element)["dof"]),
        (&json!(true), &Value::Null, &json!(6))
    );
    run(e, "asm.delete", json!({ "component": source }));
    let bom = run(e, "asm.bom", json!({}));
    assert_eq!(bom["rows"][1]["part"], "ell (mirrored)", "{bom}");
    assert!((num(&bom["rows"][1]["volume"]) - 10000.0).abs() < 1e-6, "{bom}");
}

#[test]
fn a_symmetric_part_is_placed_again_in_the_mirrored_position_without_reflecting() {
    let dir = scratch("reuse");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    box_part(e, "cube", 10.0, 10.0, 10.0);
    box_part(e, "bar", 10.0, 30.0, 4.0);
    ell(e);
    run(e, "asm.new", json!({ "name": "Symmetric" }));
    run(e, "asm.insert", json!({ "path": "bar.tenon", "at": [0, 100, 0] }));
    let cube = run(e, "asm.insert", json!({ "path": "cube.tenon", "at": [50, 0, 0] }))["component"].clone();
    let bar = run(e, "asm.insert", json!({ "path": "bar.tenon", "at": [20, 10, 0] }))["component"].clone();
    let ell_c = run(e, "asm.insert", json!({ "path": "ell.tenon", "at": [100, 0, 0] }))["component"].clone();
    let plane = run(e, "asm.geom", json!({ "plane": "yz" }));

    // The L is the same as its mirror image about no middle plane of it: refused, in words.
    let err = e.exec("asm.mirror", &json!({ "components": [ell_c], "plane": plane, "reuse": [ell_c] })).unwrap_err();
    assert!(err.contains("not the same as its mirror image") && err.contains("ell:1"), "{err}");
    let err = e.exec("asm.mirror", &json!({ "components": [cube], "plane": plane, "reuse": [bar] })).unwrap_err();
    assert!(err.contains("reuse"), "{err}");

    // The cube, from x 50 to 60, is the same part again at x -60 to -50, not turned. The L beside
    // it in the same mirror is reflected, with a row of its own.
    let made = run(e, "asm.mirror", json!({ "components": [cube, bar, ell_c], "plane": plane, "reuse": [cube, bar] }));
    let el = ids(&made["components"]);
    let tree = run(e, "asm.tree", json!({}));
    let c = component(&tree, &el[0]);
    assert_eq!(c["mirrored"], false);
    assert_eq!(c["part"], component(&tree, &cube)["part"]);
    same(vec3(&c["placement"]["origin"]), [-60.0, 0.0, 0.0]);
    same(vec3(&c["placement"]["x"]), [1.0, 0.0, 0.0]);
    same(vec3(&c["placement"]["z"]), [0.0, 0.0, 1.0]);
    // The bar, x 20 to 30, y 10 to 40: x -30 to -20, the same way up.
    same(vec3(&component(&tree, &el[1])["placement"]["origin"]), [-30.0, 10.0, 0.0]);
    assert_eq!(component(&tree, &el[2])["mirrored"], true);

    // The same part, so the parts list counts it in the same row.
    let bom = run(e, "asm.bom", json!({}));
    let row = |part: &str| bom["rows"].as_array().unwrap().iter().find(|r| r["part"] == part).unwrap_or_else(|| panic!("{bom}")).clone();
    assert_eq!(row("cube")["quantity"], 2);
    assert_eq!(row("bar")["quantity"], 3, "the one placed first, its source and its image");
    assert_eq!(row("ell")["quantity"], 1);
    assert_eq!(row("ell (mirrored)")["quantity"], 1);

    // In the XZ plane: y goes to -y, so the cube from y 0 to 10 stands from -10 to 0, not turned.
    let xz = run(e, "asm.geom", json!({ "plane": "xz" }));
    let other = run(e, "asm.mirror", json!({ "components": [cube], "plane": xz, "reuse": [cube] }));
    let image = ids(&other["components"])[0].clone();
    let f = component(&run(e, "asm.tree", json!({})), &image)["placement"].clone();
    same(vec3(&f["origin"]), [50.0, -10.0, 0.0]);
    same(vec3(&f["y"]), [0.0, 1.0, 0.0]);
    // It follows the source as a mirrored one does.
    run(e, "asm.move", json!({ "component": cube, "by": [1, 2, 3] }));
    same(origin(e, &image), [51.0, -12.0, 3.0]);
    same(origin(e, &el[0]), [-61.0, 2.0, 3.0]);
}

#[test]
fn copies_are_independent_occurrences_beside_what_they_are_copied_from_with_their_relationships() {
    let dir = scratch("copy");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    box_part(e, "pin", 4.0, 4.0, 12.0);
    ell(e);
    run(e, "asm.new", json!({ "name": "Copies" }));
    let foot = run(e, "asm.insert", json!({ "path": "ell.tenon" }))["component"].clone();
    let pin = run(e, "asm.insert", json!({ "path": "pin.tenon", "at": [20, 0, 0] }))["component"].clone();
    // A pin flush on the foot's top, its sides flush with the foot's XZ plane: two of its six left.
    let top = |e: &mut Engine, c: &Value| run(e, "asm.geom", json!({ "component": c, "plane": "xy" }));
    let (a, b) = (top(e, &foot), top(e, &pin));
    run(e, "asm.constrain", json!({ "type": "flush", "a": a, "b": b, "offset": 10 }));
    let side = |e: &mut Engine, c: &Value| run(e, "asm.geom", json!({ "component": c, "plane": "xz" }));
    let (a, b) = (side(e, &foot), side(e, &pin));
    run(e, "asm.constrain", json!({ "type": "flush", "a": a, "b": b }));
    let before = run(e, "asm.tree", json!({}));
    let pin_at = vec3(&component(&before, &pin)["placement"]["origin"]);

    // Beside them by default: what is copied is 40 wide and a little more than 12 + 10 tall, so
    // the copies go by its width and a gap (a tenth of its diagonal, at least 5).
    let r = run(e, "asm.copy", json!({ "components": [foot, pin] }));
    let by = vec3(&r["by"]);
    assert!(by[0] > 40.0 && by[1] == 0.0 && by[2] == 0.0, "{by:?}");
    let (foot2, pin2) = (r["components"][0].clone(), r["components"][1].clone());
    assert_eq!(r["names"], json!(["ell:2", "pin:2"]));
    assert_eq!(r["relationships"].as_array().unwrap().len(), 2, "the two between them came too");
    assert_eq!(r["files"].as_array().unwrap().len(), 0);
    let tree = run(e, "asm.tree", json!({}));
    assert_eq!(tree["relationships"].as_array().unwrap().len(), 4);
    assert_eq!(tree["relationships"][3]["name"], "Flush:4");
    same(vec3(&component(&tree, &foot2)["placement"]["origin"]), by);
    same(vec3(&component(&tree, &pin2)["placement"]["origin"]), [pin_at[0] + by[0], pin_at[1], pin_at[2]]);
    assert_eq!(component(&tree, &foot2)["grounded"], false, "the original is grounded; a copy is there to be moved");
    assert_eq!(component(&tree, &foot2)["element_of"], Value::Null);
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][0]["quantity"], 2);
    // They do not lie on top of what they are copied from.
    assert_eq!(run(e, "asm.interference", json!({}))["count"], 0);
    assert!((num(&run(e, "asm.mass", json!({}))["volume"]) - 2.0 * (10000.0 + 192.0)).abs() < 1e-6);

    // The two constraints hold between the copies: move one up, the other follows, still 10 above
    // it and flush; the originals do not. It is an independent copy.
    run(e, "asm.move", json!({ "component": foot2, "by": [0, 0, 7] }));
    let tree = run(e, "asm.tree", json!({}));
    let (f, q) = (vec3(&component(&tree, &foot2)["placement"]["origin"]), vec3(&component(&tree, &pin2)["placement"]["origin"]));
    // (A drag is solved so that the dragged one moves as little as it can from where it was put,
    // not to the last digit: a hundredth of a millimetre here.)
    assert!(f[2] > 6.5, "{f:?}");
    assert!((q[1] - f[1]).abs() < 1e-2 && (q[2] - f[2] - 10.0).abs() < 1e-2, "{q:?} against {f:?}");
    same(vec3(&component(&tree, &pin)["placement"]["origin"]), pin_at);
    run(e, "asm.undo", json!({}));
    // Named offset: exactly where it is told; relationships to what is not copied stay behind.
    let r = run(e, "asm.copy", json!({ "components": [pin], "by": [0, 0, 50] }));
    assert_eq!(r["relationships"].as_array().unwrap().len(), 0, "the foot is not copied");
    let lone = r["components"][0].clone();
    same(vec3(&component(&run(e, "asm.tree", json!({})), &lone)["placement"]["origin"]), [pin_at[0], pin_at[1], pin_at[2] + 50.0]);
    assert_eq!(component(&run(e, "asm.tree", json!({})), &lone)["dof"], 6);
    // One undo takes back the whole copy.
    run(e, "asm.undo", json!({}));
    assert_eq!(run(e, "asm.tree", json!({}))["components"].as_array().unwrap().len(), 4);
    assert!(e.exec("asm.copy", &json!({ "components": [] })).is_err());
    assert!(e.exec("asm.copy", &json!({ "components": [99] })).is_err());
    let err = e.exec("asm.copy", &json!({ "components": [pin], "new_files": true })).unwrap_err();
    assert!(err.contains("folder"), "{err}");

    // An element copied is a component of its own, out of the pattern; a mirrored one stays so.
    let plane = run(e, "asm.geom", json!({ "plane": "yz" }));
    let m = run(e, "asm.mirror", json!({ "components": [pin2], "plane": plane }));
    let image = ids(&m["components"])[0].clone();
    let c = run(e, "asm.copy", json!({ "components": [image], "by": [0, 100, 0] }));
    let tree = run(e, "asm.tree", json!({}));
    let copy = component(&tree, &c["components"][0]);
    assert_eq!((&copy["mirrored"], &copy["element_of"], &copy["dof"]), (&json!(true), &Value::Null, &json!(6)), "{copy}");
    run(e, "asm.move", json!({ "component": c["components"][0], "by": [1, 0, 0] }));
    assert_eq!(run(e, "asm.bom", json!({}))["rows"].as_array().unwrap().iter().find(|r| r["mirrored"] == true).unwrap()["quantity"], 2);
}

#[test]
fn copies_in_new_part_files_are_saved_beside_the_assembly_and_write_over_nothing() {
    let dir = scratch("copy-file");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    ell(e);
    run(e, "asm.new", json!({ "name": "New parts" }));
    let foot = run(e, "asm.insert", json!({ "path": "ell.tenon" }))["component"].clone();
    let foot2 = run(e, "asm.insert", json!({ "path": "ell.tenon", "at": [0, 50, 0] }))["component"].clone();
    run(e, "asm.edit_part", json!({ "component": foot, "run": "document.properties", "with": { "part_number": "E-100" } }));
    run(e, "asm.save", json!({ "path": "new.tenonasm" }));

    // Two occurrences of one part make one new file, both copies its occurrences.
    let folder = dir.to_string_lossy().into_owned();
    let r = run(e, "asm.copy", json!({ "components": [foot, foot2], "new_files": true, "folder": folder, "by": [0, 0, 100] }));
    assert_eq!(r["files"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["names"], json!(["ell-copy:1", "ell-copy:2"]));
    assert!(r["files"][0].as_str().unwrap().ends_with("ell-copy.tenon"), "{r}");
    let (doc, _) = tenon_io::project::open(&dir.join("ell-copy.tenon")).unwrap();
    assert_eq!((doc.name.as_str(), doc.part_number.as_deref()), ("Ell Copy", None), "a new part, with no number of the old one's");

    // Its own row in the parts list; its own geometry, the same as the part's.
    let bom = run(e, "asm.bom", json!({}));
    assert_eq!(bom["rows"][1]["part"], "ell-copy", "{bom}");
    assert_eq!(bom["rows"][1]["quantity"], 2);
    assert!((num(&bom["rows"][1]["volume"]) - 10000.0).abs() < 1e-6);
    // It is independent: a change to the new part leaves the first as it was, and the reverse.
    run(e, "asm.edit_part", json!({ "component": r["components"][0], "run": "param.set", "with": { "name": "t", "equation": "12" } }));
    let bom = run(e, "asm.bom", json!({}));
    assert!((num(&bom["rows"][0]["volume"]) - 10000.0).abs() < 1e-6, "{bom}");
    assert!((num(&bom["rows"][1]["volume"]) - 11400.0).abs() < 1e-6, "{bom}");

    // Another copy finds a name no file has; the old file is what it was. Saved and opened, the
    // assembly finds them beside it.
    let bytes = std::fs::read(dir.join("ell.tenon")).unwrap();
    let copy = std::fs::read(dir.join("ell-copy.tenon")).unwrap();
    let r2 = run(e, "asm.copy", json!({ "components": [foot], "new_files": true, "folder": folder, "by": [0, 0, 200] }));
    assert!(r2["files"][0].as_str().unwrap().ends_with("ell-copy2.tenon"), "{r2}");
    assert_eq!(std::fs::read(dir.join("ell.tenon")).unwrap(), bytes);
    assert_eq!(std::fs::read(dir.join("ell-copy.tenon")).unwrap(), copy);
    run(e, "asm.save", json!({ "path": "new.tenonasm" }));
    let text = std::fs::read_to_string(dir.join("new.tenonasm")).unwrap();
    assert!(text.contains("part = \"ell-copy.tenon\"") && text.contains("part = \"ell-copy2.tenon\""), "{text}");
    run(e, "asm.open", json!({ "path": "new.tenonasm" }));
    assert_eq!(run(e, "asm.bom", json!({}))["parts"], 3);
    assert!((num(&run(e, "asm.bom", json!({}))["rows"][1]["volume"]) - 11400.0).abs() < 1e-6, "the change to the new part was saved with it");
}

/// The x of every vertical line drawn visible in a DXF, by how far from `center`.
fn visible_verticals(path: &Path, center: f64) -> Vec<f64> {
    let tags = tenon_dxf::parse(&std::fs::read(path).unwrap()).unwrap();
    let entities = tenon_dxf::sections(&tags).into_iter().find(|s| s.name == "ENTITIES").unwrap();
    let field = |r: &[tenon_dxf::Tag], code: i32| r.iter().find(|t| t.code == code).map(tenon_dxf::Tag::f64);
    let layer = |r: &[tenon_dxf::Tag]| r.iter().find(|t| t.code == 8).map(tenon_dxf::Tag::str).unwrap_or_default();
    let mut xs: Vec<f64> = tenon_dxf::records(&entities.tags)
        .into_iter()
        .filter(|(k, r)| k == "LINE" && layer(r) == "VISIBLE")
        .filter_map(|(_, r)| {
            let (x0, x1) = (field(&r, 10)?, field(&r, 11)?);
            ((x0 - x1).abs() < 1e-6).then_some(x0 - center)
        })
        .collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    xs
}

#[test]
fn drawings_and_parts_lists_of_an_assembly_with_patterns_and_mirrors_come_out_right() {
    let dir = scratch("drawing");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    box_part(e, "block", 10.0, 20.0, 10.0);
    let (source, element, _) = mirrored_ell(e);
    let _ = source;
    // Three blocks in a row from x = 50, 20 apart, and between them the L's.
    let block = run(e, "asm.insert", json!({ "path": "block.tenon", "at": [-30, 0, 0] }))["component"].clone();
    let x = run(e, "asm.geom", json!({ "axis": "x" }));
    let row = run(e, "asm.pattern", json!({ "components": [block], "type": "rectangular", "direction": x, "count": 3, "spacing": 25 }));
    let second = ids(&row["components"])[0].clone();
    run(e, "asm.save", json!({ "path": "all.tenonasm" }));

    run(e, "drw.new", json!({ "name": "All" }));
    let front = run(e, "drw.view.base", json!({ "model": "all.tenonasm", "orientation": "front", "scale": 0.5, "at": [150, 120] }))["view"].clone();
    run(e, "drw.parts_list", json!({ "view": front }));
    run(e, "drw.balloon.auto", json!({ "view": front }));
    let info = run(e, "drw.info", json!({}));
    let list = info["annotations"].as_array().unwrap().iter().find(|a| a.get("rows").is_some()).unwrap().clone();
    let rows = list["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3, "{list}");
    assert_eq!(
        (&rows[1]["part_number"], &rows[1]["description"], &rows[1]["quantity"]),
        (&json!("ell (mirrored)"), &json!("Ell, mirrored"), &json!(1))
    );
    assert_eq!((&rows[2]["part_number"], &rows[2]["quantity"]), (&json!("block"), &json!(3)));
    // Numbered as the assembly's own list is, a balloon for each row.
    let bom = run(e, "asm.bom", json!({}));
    for (b, l) in bom["rows"].as_array().unwrap().iter().zip(rows) {
        assert_eq!((&b["item"], &b["quantity"], &b["part"], &b["name"]), (&l["item"], &l["quantity"], &l["part_number"], &l["description"]));
    }
    let balloons = info["annotations"].as_array().unwrap().iter().filter(|a| a["type"] == "balloon" || a["kind"] == "balloon").count();
    assert_eq!(balloons, 3, "{info}");

    // Drawn where they stand, at half size, about the middle of the whole: the L's run x -140 to
    // -100 and 100 to 140, the post at the end towards the middle on each. The blocks are at x
    // -30 to -20, -5 to 5, 20 to 30. In the view, every one of those lines.
    run(e, "drw.export.dxf", json!({ "path": "all.dxf" }));
    let xs = visible_verticals(&dir.join("all.dxf"), 150.0);
    for x in [-140.0, -110.0, -100.0, -30.0, -20.0, -5.0, 5.0, 20.0, 30.0, 100.0, 110.0, 140.0] {
        assert!(xs.iter().any(|q| (q * 2.0 - x).abs() < 1e-2), "no line at {x} in {xs:?}");
    }
    // Where an unreflected post would have been, nothing: the far end's line is the foot's end,
    // and no line at -130.
    assert!(!xs.iter().any(|q| (q * 2.0 + 130.0).abs() < 1e-2), "{xs:?}");

    // An element is measured as any component: its edge is picked, and the width of the mirrored
    // post's top dimensioned: 10.
    let edge = run(e, "drw.pick", json!({ "view": front, "view_at": [-105.0, 30.0] }));
    assert_eq!(edge["component"], element, "{edge}");
    let d = run(e, "drw.dimension", json!({ "view": front, "type": "horizontal", "a": edge, "by": [0, 10] }));
    assert!((num(&d["value"]) - 10.0).abs() < 1e-6, "{d}");
    let edge = run(e, "drw.pick", json!({ "view": front, "view_at": [0.0, 10.0] }));
    assert_eq!(edge["component"], second, "{edge}");
    let d = run(e, "drw.dimension", json!({ "view": front, "type": "horizontal", "a": edge, "by": [0, 10] }));
    assert!((num(&d["value"]) - 10.0).abs() < 1e-6, "{d}");
}

#[test]
fn patterns_and_mirrors_are_saved_and_read_back_to_the_same_bytes() {
    let dir = scratch("file");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    box_part(e, "block", 10.0, 10.0, 10.0);
    let (source, element, mirror) = mirrored_ell(e);
    let block = run(e, "asm.insert", json!({ "path": "block.tenon", "at": [0, 50, 0] }))["component"].clone();
    let (x, y, plane) =
        (run(e, "asm.geom", json!({ "axis": "x" })), run(e, "asm.geom", json!({ "axis": "y" })), run(e, "asm.geom", json!({ "plane": "yz" })));
    let two = run(
        e,
        "asm.pattern",
        json!({ "components": [block], "type": "rectangular", "direction": x, "count": 2, "spacing": 15.5, "reverse": true, "direction2": y, "count2": 2, "spacing2": 12.25, "reverse2": true }),
    );
    let z = run(e, "asm.geom", json!({ "axis": "z" }));
    let round = run(
        e,
        "asm.pattern",
        json!({ "components": [two["components"][2]], "type": "circular", "axis": z, "count": 5, "angle": 1.25, "reverse": true }),
    );
    let again = run(e, "asm.mirror", json!({ "components": [block], "plane": plane, "reuse": [block] }));
    run(e, "asm.suppress_pattern", json!({ "pattern": again["pattern"] }));
    // And in a flat face of a component: a reference with its fingerprint, rounded as all are.
    let flat =
        (0..6).map(|i| run(e, "asm.geom", json!({ "component": block, "face": { "body": 0, "index": i } }))).find(|t| t["kind"] == "plane").unwrap();
    run(e, "asm.mirror", json!({ "components": [element], "plane": flat }));
    run(e, "asm.save", json!({ "path": "a.tenonasm" }));
    let first = std::fs::read(dir.join("a.tenonasm")).unwrap();

    // As it reads: one record to a pattern, its rule and its elements said once.
    let text = String::from_utf8(first.clone()).unwrap();
    assert!(text.contains("[[pattern]]") && text.contains("next_pattern = 6"), "{text}");
    assert!(text.contains("type = \"rectangular\"") && text.contains("type = \"circular\"") && text.contains("type = \"mirror\""), "{text}");
    assert!(text.contains("mirrored = true"), "{text}");
    assert!(!text.contains("reflected"), "the part's mirror image is worked out, never written: {text}");

    // Opened and saved again: the same bytes; and the same assembly and parts list.
    let tree = run(e, "asm.tree", json!({}));
    let bom = run(e, "asm.bom", json!({}));
    run(e, "asm.open", json!({ "path": "a.tenonasm" }));
    let without_missing = |t: &Value| {
        let mut t = t.clone();
        for c in t["components"].as_array_mut().unwrap() {
            let p = c["placement"].take();
            let round = |v: &Value| vec3(v).map(|x| (x * 1e6).round() / 1e6 + 0.0);
            c["placement"] = json!([round(&p["origin"]), round(&p["x"]), round(&p["z"])]);
        }
        t
    };
    assert_eq!(without_missing(&run(e, "asm.tree", json!({}))), without_missing(&tree));
    assert_eq!(run(e, "asm.bom", json!({}))["rows"], bom["rows"]);
    run(e, "asm.save", json!({ "path": "b.tenonasm" }));
    assert_eq!(std::fs::read(dir.join("b.tenonasm")).unwrap(), first);

    // It goes on working: a source moved moves what it left, read back or not.
    run(e, "asm.move", json!({ "component": source, "by": [0, 0, 5] }));
    same(origin(e, &element), [-100.0, 0.0, 5.0]);
    let last = ids(&round["components"]).pop().unwrap();
    run(e, "asm.edit_pattern", json!({ "pattern": round["pattern"], "count": 2 }));
    assert!(e.exec("asm.tree", &json!({})).unwrap()["components"].as_array().unwrap().iter().all(|c| c["id"] != last));
    let _ = mirror;

    // A file from before them reads as it did, and writes nothing of them.
    run(e, "asm.new", json!({ "name": "Old" }));
    run(e, "asm.insert", json!({ "path": "block.tenon" }));
    run(e, "asm.save", json!({ "path": "old.tenonasm" }));
    let old = std::fs::read_to_string(dir.join("old.tenonasm")).unwrap();
    assert!(!old.contains("pattern") && !old.contains("mirrored"), "{old}");
    run(e, "asm.open", json!({ "path": "old.tenonasm" }));
    run(e, "asm.save", json!({ "path": "old2.tenonasm" }));
    assert_eq!(std::fs::read_to_string(dir.join("old2.tenonasm")).unwrap(), old);

    // `tenon-cli diff` tells a changed pattern, and damage is told when reading.
    run(e, "asm.open", json!({ "path": "a.tenonasm" }));
    run(e, "asm.edit_pattern", json!({ "pattern": two["pattern"], "count": 3 }));
    run(e, "asm.save", json!({ "path": "c.tenonasm" }));
    let diff = tenon_io::diff::diff_files(&dir.join("a.tenonasm"), &dir.join("c.tenonasm")).unwrap();
    assert!(diff.changes.iter().any(|c| c.section == "Patterns" && c.detail.contains("count")), "{:?}", diff.changes);
    assert!(diff.changes.iter().any(|c| c.section == "Components" && c.op == '+'), "{:?}", diff.changes);
    for (from, to, says) in [
        ("next_pattern = 6", "next_pattern = 2", "bad or repeated id"),
        (
            "count = 5
",
            "count = 100000
",
            "the count must be",
        ),
        ("spacing = 15.5", "spacing = -2.0", "spacing must be positive"),
        ("sources = [", "sources = [999, ", "does not stand for"),
        ("type = \"circular\"", "type = \"spiral\"", "pattern 3 (Pattern:2)"),
    ] {
        assert!(text.contains(from), "{from}");
        let damaged = text.replacen(from, to, 1);
        std::fs::write(dir.join("bad.tenonasm"), damaged).unwrap();
        let err = e.exec("asm.open", &json!({ "path": "bad.tenonasm" })).unwrap_err();
        assert!(err.contains(says), "{from}: {err}");
    }

    // The mirror image is told apart in what is kept of a part's scene, whose name comes of these
    // bytes. A part file edited by hand to say the same is read as the part it is, and saved so.
    let (doc, _) = tenon_io::project::open(&dir.join("block.tenon")).unwrap();
    let plain = tenon_io::project::to_bytes(&doc, &serde_json::Map::new()).unwrap();
    let mut image = doc.clone();
    image.set_reflected(true);
    let bytes = tenon_io::project::to_bytes(&image, &serde_json::Map::new()).unwrap();
    assert_ne!(plain, bytes);
    assert!(!tenon_io::project::from_bytes(&plain).unwrap().0.reflected());
    assert!(!tenon_io::project::from_bytes(&bytes).unwrap().0.reflected());
    std::fs::write(dir.join("said.tenon"), &bytes).unwrap();
    run(e, "file.open", json!({ "path": "said.tenon" }));
    run(e, "file.save", json!({ "path": "said2.tenon" }));
    assert_eq!(std::fs::read(dir.join("said2.tenon")).unwrap(), plain);
    run(e, "asm.new", json!({ "name": "Said" }));
    run(e, "asm.insert", json!({ "path": "said.tenon" }));
    same(vec3(&run(e, "asm.mass", json!({}))["center_of_mass"]), [5.0, 5.0, 5.0]);
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][0]["mirrored"], false);
}

/// The face of a component's part that is the `end` ("start" or "end") of feature `feature`, as a
/// target.
fn cap(e: &mut Engine, component: &Value, feature: u64, end: &str) -> Value {
    run(e, "asm.geom", json!({ "component": component, "face": { "type": "cap", "feature": feature, "end": end } }))
}

/// A point of a component's part, where it stands.
fn at_world(e: &mut Engine, id: &Value, p: [f64; 3]) -> [f64; 3] {
    let tree = run(e, "asm.tree", json!({}));
    let f = &component(&tree, id)["placement"];
    let (o, x, y, z) = (vec3(&f["origin"]), vec3(&f["x"]), vec3(&f["y"]), vec3(&f["z"]));
    [0, 1, 2].map(|i| o[i] + x[i] * p[0] + y[i] * p[1] + z[i] * p[2])
}

/// A block 10 x 10 x 10 and an L at x 50 to 90, mirrored in the assembly's YZ plane to x -90 to
/// -50. The L is grounded; the blocks are free, set on it from above, one each side.
fn grounded_ell_and_image(e: &mut Engine) -> (Value, Value) {
    ell(e);
    box_part(e, "block", 10.0, 10.0, 10.0);
    run(e, "asm.new", json!({ "name": "Pair" }));
    let source = run(e, "asm.insert", json!({ "path": "ell.tenon", "at": [50, 0, 0], "grounded": true }))["component"].clone();
    let yz = run(e, "asm.geom", json!({ "plane": "yz" }));
    let made = run(e, "asm.mirror", json!({ "components": [source], "plane": yz }));
    (source, made["components"][0].clone())
}

#[test]
fn what_is_held_to_a_mirrored_occurrence_stands_as_the_image_of_what_is_held_to_its_source() {
    let dir = scratch("held-mirror");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    let (source, image) = grounded_ell_and_image(e);

    // A mate: the block's underside on the foot's top (z 10, facing up either side of a mirror in
    // YZ), so the block stands on it at z 10 to 20, not turned over and not sunk into the foot.
    let mut blocks = Vec::new();
    for (of, at) in [(&source, [80, 5, 40]), (&image, [-80, 5, 40])] {
        let block = run(e, "asm.insert", json!({ "path": "block.tenon", "at": at, "grounded": false }))["component"].clone();
        let (top, under) = (cap(e, of, 2, "end"), cap(e, &block, 2, "start"));
        run(e, "asm.constrain", json!({ "type": "mate", "a": top, "b": under }));
        same(at_world(e, &block, [0.0, 0.0, 0.0]), [f64::from(at[0]), 5.0, 10.0]);
        same(at_world(e, &block, [0.0, 0.0, 10.0]), [f64::from(at[0]), 5.0, 20.0]);
        blocks.push(block);
    }
    assert_eq!(run(e, "asm.interference", json!({}))["count"], 0);

    // Auto explode reads the same normals: each goes up by the spacing, away from its foot.
    run(e, "asm.explode.auto", json!({ "spacing": 30 }));
    let positions = run(e, "asm.explode.positions", json!({}));
    for b in &blocks {
        let row = positions["components"].as_array().unwrap().iter().find(|c| c["id"] == *b).unwrap();
        assert!((vec3(&row["origin"])[2] - 40.0).abs() < 1e-6, "{row}");
    }
    run(e, "asm.explode.clear", json!({}));

    // A flush: tops level with the foot's top, so the block at z 0 to 10, beside the foot.
    for (of, at) in [(&source, [80, 35, 40]), (&image, [-80, 35, 40])] {
        let block = run(e, "asm.insert", json!({ "path": "block.tenon", "at": at, "grounded": false }))["component"].clone();
        let (top, its_top) = (cap(e, of, 2, "end"), cap(e, &block, 2, "end"));
        run(e, "asm.constrain", json!({ "type": "flush", "a": top, "b": its_top }));
        same(at_world(e, &block, [0.0, 0.0, 0.0]), [f64::from(at[0]), 35.0, 0.0]);
        same(at_world(e, &block, [0.0, 0.0, 10.0]), [f64::from(at[0]), 35.0, 10.0]);
    }

    // A rigid joint: the underside's middle on the middle of the foot's top. That is the 40 x 20
    // top less the post's 10 x 10 at its corner: 700 mm^2, its middle at x (800 * 20 - 100 * 5) / 700
    // and y (800 * 10 - 100 * 5) / 700 of the part. The block's middle is 5 above, and its top 10.
    let (mx, my) = (15500.0 / 700.0, 7500.0 / 700.0);
    for (of, at, x) in [(&source, [80, 65, 40], 50.0 + mx), (&image, [-80, 65, 40], -50.0 - mx)] {
        let block = run(e, "asm.insert", json!({ "path": "block.tenon", "at": at, "grounded": false }))["component"].clone();
        let (top, under) = (cap(e, of, 2, "end"), cap(e, &block, 2, "start"));
        run(e, "asm.joint", json!({ "type": "rigid", "a": top, "b": under }));
        same(at_world(e, &block, [5.0, 5.0, 5.0]), [x, my, 15.0]);
        same(at_world(e, &block, [5.0, 5.0, 10.0]), [x, my, 20.0]);
        run(e, "asm.delete", json!({ "component": block }));
    }

    // A pattern along a mirrored flat face: its normal is the way out of the material, so up by
    // the spacing from each top alike.
    let free = run(e, "asm.insert", json!({ "path": "block.tenon", "at": [0, 200, 0], "grounded": false }))["component"].clone();
    for of in [&source, &image] {
        let top = cap(e, of, 2, "end");
        let made = run(e, "asm.pattern", json!({ "components": [free], "type": "rectangular", "direction": top, "count": 2, "spacing": 10 }));
        same(origin(e, &made["components"][0]), [0.0, 200.0, 10.0]);
        run(e, "asm.delete_pattern", json!({ "pattern": made["pattern"] }));
    }

    // Mirrored in the assembly's XY plane, what faces up faces down: the foot's top, on the image
    // of a source at z 5 to 15, is at z -15 facing down. The block mates to it from below, turned
    // over, its underside at z -15 and its far side at z -25.
    run(e, "asm.new", json!({ "name": "Below" }));
    let source = run(e, "asm.insert", json!({ "path": "ell.tenon", "at": [50, 0, 5], "grounded": true }))["component"].clone();
    let xy = run(e, "asm.geom", json!({ "plane": "xy" }));
    let made = run(e, "asm.mirror", json!({ "components": [source], "plane": xy }));
    let image = made["components"][0].clone();
    let block = run(e, "asm.insert", json!({ "path": "block.tenon", "at": [80, 5, -50], "grounded": false }))["component"].clone();
    let (top, under) = (cap(e, &image, 2, "end"), cap(e, &block, 2, "start"));
    run(e, "asm.constrain", json!({ "type": "mate", "a": top, "b": under }));
    assert!((at_world(e, &block, [0.0, 0.0, 0.0])[2] + 15.0).abs() < 1e-6);
    assert!((at_world(e, &block, [0.0, 0.0, 10.0])[2] + 25.0).abs() < 1e-6);
    assert_eq!(run(e, "asm.interference", json!({}))["count"], 0);
}

#[test]
fn a_part_is_inserted_on_a_mirrored_occurrence_as_on_its_source() {
    let dir = scratch("insert-mirror");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    // A plate 40 x 40 x 5 with a hole of radius 4 through it, and a pin of that radius 20 long.
    run(e, "file.new", json!({ "name": "plate" }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 40 }));
    run(e, "model.extrude", json!({ "sketch": sk, "distance": 5 }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.circle", json!({ "sketch": sk, "cx": 10, "cy": 20, "r": 4 }));
    run(e, "model.extrude", json!({ "sketch": sk, "distance": 5, "operation": "cut" }));
    run(e, "file.save", json!({ "path": "plate.tenon" }));
    run(e, "file.new", json!({ "name": "pin" }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(e, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 4 }));
    run(e, "model.extrude", json!({ "sketch": sk, "distance": 20 }));
    run(e, "file.save", json!({ "path": "pin.tenon" }));

    run(e, "asm.new", json!({ "name": "Pinned" }));
    let plate = run(e, "asm.insert", json!({ "path": "plate.tenon", "at": [50, 0, 0], "grounded": true }))["component"].clone();
    let yz = run(e, "asm.geom", json!({ "plane": "yz" }));
    let image = run(e, "asm.mirror", json!({ "components": [plate], "plane": yz }))["components"][0].clone();
    // The rim on the plate's top, the far one from the part's XY plane: part z 5, or on the image
    // -5. The pin's rim at its own z 0. The pin stands on the top, at z 5 to 25, in line with the
    // hole, at x 60 on the source and -60 on the image. A rim whose axis went by a mirrored plane's
    // inward normal would send it down through the plate.
    for (of, at, x) in [(&plate, [100, 100, 30], 60.0), (&image, [-100, 100, 30], -60.0)] {
        let rims: Vec<Value> = (0..40)
            .filter_map(|i| e.exec("asm.geom", &json!({ "component": of, "edge": { "body": 0, "index": i } })).ok())
            .filter(|t| t["kind"] == "circle")
            .collect();
        let rim = rims.iter().find(|t| num(&t["geom"]["edge"]["fingerprint"]["mid"]["z"]).abs() > 1.0).unwrap().clone();
        let pin = run(e, "asm.insert", json!({ "path": "pin.tenon", "at": at, "grounded": false }))["component"].clone();
        let foot = (0..8)
            .filter_map(|i| e.exec("asm.geom", &json!({ "component": pin, "edge": { "body": 0, "index": i } })).ok())
            .find(|t| t["kind"] == "circle" && num(&t["geom"]["edge"]["fingerprint"]["mid"]["z"]).abs() < 1.0)
            .unwrap();
        run(e, "asm.constrain", json!({ "type": "insert", "a": rim, "b": foot }));
        same(at_world(e, &pin, [0.0, 0.0, 0.0]), [x, 20.0, 5.0]);
        same(at_world(e, &pin, [0.0, 0.0, 20.0]), [x, 20.0, 25.0]);
    }
}

#[test]
fn a_mirrored_occurrences_own_planes_and_axes_are_the_images_of_its_sources() {
    let dir = scratch("planes-mirror");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    ell(e);
    box_part(e, "block", 10.0, 10.0, 10.0);
    // The L, with a work plane at z 25 and a work axis along X through (0, 5, 3)
    run(e, "file.open", json!({ "path": "ell.tenon" }));
    let wp = run(e, "work.plane", json!({ "base": "xy", "distance": 25 }))["feature"].clone();
    run(e, "file.save", json!({ "path": "ell.tenon" }));
    run(e, "asm.new", json!({ "name": "Pair" }));
    let source = run(e, "asm.insert", json!({ "path": "ell.tenon", "at": [50, 0, 0], "grounded": true }))["component"].clone();
    let yz = run(e, "asm.geom", json!({ "plane": "yz" }));
    let image = run(e, "asm.mirror", json!({ "components": [source], "plane": yz }))["components"][0].clone();

    // A block's underside mated to each plane in turn. Mirrored in YZ, up stays up and X is the
    // other way, so the block lies the same way up from the plane on the two, with its middle's x
    // the other way.
    let on = |e: &mut Engine, plane: Value, x: f64| -> [f64; 3] {
        let block = run(e, "asm.insert", json!({ "path": "block.tenon", "at": [x, 60, 60], "grounded": false }))["component"].clone();
        let under = cap(e, &block, 2, "start");
        run(e, "asm.constrain", json!({ "type": "mate", "a": plane, "b": under }));
        let up = at_world(e, &block, [0.0, 0.0, 5.0]);
        let was = at_world(e, &block, [0.0, 0.0, 0.0]);
        run(e, "asm.delete", json!({ "component": block }));
        [0, 1, 2].map(|i| up[i] - was[i])
    };
    for (plane, up) in [
        (json!({ "work": wp }), [0.0, 0.0, 5.0]),
        (json!({ "plane": "xy" }), [0.0, 0.0, 5.0]),
        // (The XZ plane's normal, as a sketch on it looks, is toward -Y.)
        (json!({ "plane": "xz" }), [0.0, -5.0, 0.0]),
    ] {
        for (of, x) in [(&source, 60.0), (&image, -70.0)] {
            let mut p = plane.clone();
            p["component"] = of.clone();
            let t = run(e, "asm.geom", p);
            same(on(e, t, x), up);
        }
    }
    // The YZ plane faces +X on the source and, as the image of that, -X on the image.
    for (of, x, way) in [(&source, 60.0, 5.0), (&image, -70.0, -5.0)] {
        let t = run(e, "asm.geom", json!({ "component": of, "plane": "yz" }));
        same(on(e, t, x), [way, 0.0, 0.0]);
    }
    // The height of each block on the work plane: it stands at z 25 to 35, on the image too.
    for (of, x) in [(&source, 60.0), (&image, -70.0)] {
        let t = run(e, "asm.geom", json!({ "component": of, "work": wp }));
        let block = run(e, "asm.insert", json!({ "path": "block.tenon", "at": [x, 60, 60], "grounded": false }))["component"].clone();
        let under = cap(e, &block, 2, "start");
        run(e, "asm.constrain", json!({ "type": "mate", "a": t, "b": under }));
        assert!((at_world(e, &block, [0.0, 0.0, 0.0])[2] - 25.0).abs() < 1e-6);
        assert!((at_world(e, &block, [0.0, 0.0, 10.0])[2] - 35.0).abs() < 1e-6);
        run(e, "asm.delete", json!({ "component": block }));
    }
    // A joint on the part's origin, a point with no way of its own, takes the part's Z, and on the
    // image the image of that: up on both, so the block is turned to face it the same way on both.
    for (of, x) in [(&source, 60.0), (&image, -70.0)] {
        let a = run(e, "asm.geom", json!({ "component": of, "origin": true }));
        let block = run(e, "asm.insert", json!({ "path": "block.tenon", "at": [x, 60, 60], "grounded": false }))["component"].clone();
        let b = run(e, "asm.geom", json!({ "component": block, "origin": true }));
        run(e, "asm.joint", json!({ "type": "rigid", "a": a, "b": b }));
        let tree = run(e, "asm.tree", json!({}));
        same(vec3(&component(&tree, &block)["placement"]["z"]), [0.0, 0.0, -1.0]);
        run(e, "asm.delete", json!({ "component": block }));
    }
    // A pattern along each one's own Z axis goes up on both: the image of up, here, is up.
    let free = run(e, "asm.insert", json!({ "path": "block.tenon", "at": [0, 200, 0], "grounded": false }))["component"].clone();
    for of in [&source, &image] {
        let z = run(e, "asm.geom", json!({ "component": of, "axis": "z" }));
        let made = run(e, "asm.pattern", json!({ "components": [free], "type": "rectangular", "direction": z, "count": 2, "spacing": 10 }));
        same(origin(e, &made["components"][0]), [0.0, 200.0, 10.0]);
        run(e, "asm.delete_pattern", json!({ "pattern": made["pattern"] }));
    }
}

#[test]
fn a_source_deleted_and_brought_back_by_undo_has_its_part_again() {
    let dir = scratch("undo-source");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    let (source, _, _) = mirrored_ell(e);
    let file = dir.join("ell.tenon");
    let key = tenon_io::asm::part_key(&file);
    let weighed = |e: &mut Engine| num(&run(e, "asm.mass", json!({}))["volume"]);
    assert!((weighed(e) - 20000.0).abs() < 1e-6);

    // Deleted, the source takes its element with it; nothing is left of the part in the assembly.
    run(e, "asm.delete", json!({ "component": source }));
    assert!(run(e, "asm.tree", json!({}))["components"].as_array().unwrap().is_empty());
    assert_eq!(num(&run(e, "asm.mass", json!({}))["volume"]), 0.0);

    // Undo brings back components with a part behind them: weighed, exported, drawn from it.
    run(e, "asm.undo", json!({}));
    assert!((weighed(e) - 20000.0).abs() < 1e-6);
    run(e, "asm.export_step", json!({ "path": "back.step" }));
    assert_eq!(solids_of(&dir.join("back.step")).len(), 2);
    let bom = run(e, "asm.bom", json!({}));
    assert_eq!(bom["parts"], 2, "{bom}");

    // Redo and undo again, as often as you like.
    run(e, "asm.redo", json!({}));
    run(e, "asm.undo", json!({}));
    assert!((weighed(e) - 20000.0).abs() < 1e-6);

    // The same goes for what deleting a pattern or lowering a count closes.
    run(e, "asm.delete", json!({ "component": source }));
    assert!(e.asm.parts.contains_key(&key), "undo can still name it");

    // While undo keeps it, Place reads the file again: it may have changed since it was deleted.
    let plain = std::fs::read(&file).unwrap();
    box_part(e, "ell", 20.0, 20.0, 20.0);
    let c = run(e, "asm.insert", json!({ "path": "ell.tenon" }))["component"].clone();
    assert!((weighed(e) - 8000.0).abs() < 1e-6, "{}", weighed(e));
    run(e, "asm.delete", json!({ "component": c }));
    std::fs::write(&file, &plain).unwrap();

    // A file opened afresh has no undo to keep it for.
    box_part(e, "other", 5.0, 5.0, 5.0);
    run(e, "asm.insert", json!({ "path": "other.tenon" }));
    run(e, "asm.save", json!({ "path": "a.tenonasm" }));
    run(e, "asm.open", json!({ "path": "a.tenonasm" }));
    assert!(!e.asm.parts.contains_key(&key));
    assert!(e.asm.parts.contains_key(&tenon_io::asm::part_key(&dir.join("other.tenon"))));
}

#[test]
fn a_second_direction_along_the_first_is_refused_in_words() {
    let dir = scratch("along-one-line");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    let (_, block) = rail_and_block(e);
    let (x, y) = (run(e, "asm.geom", json!({ "axis": "x" })), run(e, "asm.geom", json!({ "axis": "y" })));
    // 3 x 2 along one line would be six components, of which some stand on others.
    let params = json!({ "components": [block], "type": "rectangular", "direction": x, "count": 3, "spacing": 20, "direction2": x, "count2": 2, "spacing2": 20 });
    let err = e.exec("asm.pattern", &params).unwrap_err();
    assert!(err.contains("across the first"), "{err}");
    assert_eq!(run(e, "asm.tree", json!({}))["components"].as_array().unwrap().len(), 2, "nothing is made");
    // The same changed to it, by an edit, stays as it was.
    let made = run(
        e,
        "asm.pattern",
        json!({ "components": [block], "type": "rectangular", "direction": x, "count": 3, "spacing": 20, "direction2": y, "count2": 2, "spacing2": 20 }),
    );
    let err = e.exec("asm.edit_pattern", &json!({ "pattern": made["pattern"], "direction2": x })).unwrap_err();
    assert!(err.contains("across the first"), "{err}");
    assert_eq!(made["elements"], 5);
    // With one in either direction it is only one row: nothing lies on anything.
    let none = run(e, "asm.edit_pattern", json!({ "pattern": made["pattern"], "direction2": x, "count2": 1 }));
    assert_eq!(none["elements"], 2);
    assert_eq!(run(e, "asm.interference", json!({}))["count"], 0);
}

#[test]
fn what_would_go_out_of_reach_is_refused_in_words() {
    let dir = scratch("reach");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    let (_, block) = rail_and_block(e);
    let x = run(e, "asm.geom", json!({ "axis": "x" }));
    let says = |err: String| {
        assert!(err.contains("more than 1000 m from the origin") && err.contains("lower the spacing or the count"), "{err}");
        assert!(!err.contains("bad placement"), "{err}");
    };
    // 4 x 400 m is 1600 m; 19 x 100 m is 1900 m.
    for (count, spacing) in [(5, 400_000.0), (20, 100_000.0)] {
        let p = json!({ "components": [block], "type": "rectangular", "direction": x, "count": count, "spacing": spacing });
        says(e.exec("asm.pattern", &p).unwrap_err());
    }
    let made = run(e, "asm.pattern", json!({ "components": [block], "type": "rectangular", "direction": x, "count": 2, "spacing": 900_000 }));
    says(e.exec("asm.edit_pattern", &json!({ "pattern": made["pattern"], "count": 3 })).unwrap_err());
    // A copy of the element, 900 m along, by 200 m more; and a move of the source that takes its
    // element as far.
    says(e.exec("asm.copy", &json!({ "components": [made["components"][0]], "by": [200_000, 0, 0] })).unwrap_err());
    says(e.exec("asm.move", &json!({ "component": block, "by": [200_000, 0, 0] })).unwrap_err());
    same(origin(e, &block), [0.0, 0.0, 5.0]);
}

#[test]
fn a_frame_at_no_axis_in_particular_is_read_back_to_the_same_bytes() {
    let dir = scratch("turned");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    box_part(e, "block", 10.0, 10.0, 10.0);
    run(e, "asm.new", json!({ "name": "Turned" }));
    let turn = json!({ "origin": [3.3, -7.1, 2.9], "z": [0.3, 0.5, 0.81], "x": [0.5, -0.3, 0.0] });
    // A plain component so; a long pattern along its own X; round an axis, seven in 2.5 radians.
    let plain = run(e, "asm.insert", json!({ "path": "block.tenon", "grounded": false }))["component"].clone();
    let mut place = turn.clone();
    place["component"] = plain.clone();
    run(e, "asm.place", place);
    let source = run(e, "asm.insert", json!({ "path": "block.tenon", "grounded": false }))["component"].clone();
    let mut place = turn.clone();
    place["component"] = source.clone();
    run(e, "asm.place", place);
    let along = run(e, "asm.geom", json!({ "component": source, "axis": "x" }));
    run(e, "asm.pattern", json!({ "components": [source], "type": "rectangular", "direction": along, "count": 40, "spacing": 500 }));
    let z = run(e, "asm.geom", json!({ "axis": "z" }));
    let round = run(e, "asm.insert", json!({ "path": "block.tenon", "at": [100, 0, 0], "grounded": false }))["component"].clone();
    run(e, "asm.pattern", json!({ "components": [round], "type": "circular", "axis": z, "count": 7, "angle": 2.5 }));
    run(e, "asm.save", json!({ "path": "a.tenonasm" }));
    let first = std::fs::read(dir.join("a.tenonasm")).unwrap();
    run(e, "asm.open", json!({ "path": "a.tenonasm" }));
    run(e, "asm.save", json!({ "path": "b.tenonasm" }));
    assert!(first == std::fs::read(dir.join("b.tenonasm")).unwrap(), "read back and written, it is not the same");
    // Solved again with nothing changed, which is what the app does when a mirror image arrives,
    // it stays the same too; that is no step to undo.
    run(e, "asm.update", json!({}));
    assert!(!e.asm.can_undo(), "solving what was read changed it");
    run(e, "asm.save", json!({ "path": "c.tenonasm" }));
    assert!(first == std::fs::read(dir.join("c.tenonasm")).unwrap(), "solved and written, it is not the same");

    // A frame that has drifted further than a file's rounding is still put right when it is read.
    let text = String::from_utf8(first).unwrap();
    let drifted = text.replacen("0.857492926", "0.857493026", 1);
    assert_ne!(drifted, text, "{text}");
    std::fs::write(dir.join("drift.tenonasm"), drifted).unwrap();
    run(e, "asm.open", json!({ "path": "drift.tenonasm" }));
    let tree = run(e, "asm.tree", json!({}));
    let c = &tree["components"][0]["placement"];
    let (x, y) = (vec3(&c["x"]), vec3(&c["y"]));
    assert!((x[0] * y[0] + x[1] * y[1] + x[2] * y[2]).abs() < 1e-12, "{c}");
    assert!(((x[0] * x[0] + x[1] * x[1] + x[2] * x[2]) - 1.0).abs() < 1e-12, "{c}");
}

#[test]
fn what_repeats_a_suppressed_patterns_elements_is_out_with_them_and_hidden_stays_hidden() {
    let dir = scratch("suppress-chain");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    box_part(e, "block", 10.0, 10.0, 10.0);
    run(e, "asm.new", json!({ "name": "Chain" }));
    let block = run(e, "asm.insert", json!({ "path": "block.tenon", "grounded": false }))["component"].clone();
    let (x, y) = (run(e, "asm.geom", json!({ "axis": "x" })), run(e, "asm.geom", json!({ "axis": "y" })));
    // Pattern 1 makes one more of the source; pattern 2 makes two more of that one.
    let one = run(e, "asm.pattern", json!({ "components": [block], "type": "rectangular", "direction": x, "count": 2, "spacing": 20 }));
    let el = one["components"][0].clone();
    let two = run(e, "asm.pattern", json!({ "components": [el], "type": "rectangular", "direction": y, "count": 3, "spacing": 20 }));
    let more = ids(&two["components"]);
    let tree_of = |e: &mut Engine, id: &Value| component(&run(e, "asm.tree", json!({})), id)["visible"].clone();
    let weighed = |e: &mut Engine| num(&run(e, "asm.mass", json!({}))["volume"]);
    assert!((weighed(e) - 4000.0).abs() < 1e-6);

    // Pattern 1 out: its element is gone, and so what repeats it. Only the source is left of four.
    run(e, "asm.suppress_pattern", json!({ "pattern": one["pattern"] }));
    assert!((weighed(e) - 1000.0).abs() < 1e-6, "{}", weighed(e));
    assert_eq!(run(e, "asm.bom", json!({}))["rows"][0]["quantity"], 1);
    for c in std::iter::once(&el).chain(&more) {
        assert_eq!(tree_of(e, c), false, "{c}");
        let err = e.exec("asm.visible", &json!({ "component": c, "visible": true })).unwrap_err();
        assert!(err.contains("suppressed"), "{err}");
    }
    assert_eq!(run(e, "asm.interference", json!({}))["count"], 0);
    run(e, "asm.export_step", json!({ "path": "one.step" }));
    assert_eq!(solids_of(&dir.join("one.step")).len(), 1);
    // Brought back, all four.
    run(e, "asm.suppress_pattern", json!({ "pattern": one["pattern"], "suppressed": false }));
    assert!((weighed(e) - 4000.0).abs() < 1e-6);

    // An element hidden on its own stays hidden through its pattern's coming and going.
    run(e, "asm.visible", json!({ "component": more[0], "visible": false }));
    assert!((weighed(e) - 3000.0).abs() < 1e-6);
    run(e, "asm.suppress_pattern", json!({ "pattern": two["pattern"] }));
    assert!((weighed(e) - 2000.0).abs() < 1e-6, "the source and pattern 1's element");
    run(e, "asm.suppress_pattern", json!({ "pattern": two["pattern"], "suppressed": false }));
    assert_eq!(tree_of(e, &more[0]), false);
    assert_eq!(tree_of(e, &more[1]), true);
    assert!((weighed(e) - 3000.0).abs() < 1e-6);
    // A count raised while it is out makes elements that come back with the rest.
    run(e, "asm.suppress_pattern", json!({ "pattern": two["pattern"] }));
    let raised = run(e, "asm.edit_pattern", json!({ "pattern": two["pattern"], "count": 4 }));
    assert_eq!(raised["elements"], 3);
    assert!((weighed(e) - 2000.0).abs() < 1e-6);
    run(e, "asm.suppress_pattern", json!({ "pattern": two["pattern"], "suppressed": false }));
    assert!((weighed(e) - 4000.0).abs() < 1e-6, "one hidden of five");

    // Made independent while out, its elements are components of their own, as they were asked to
    // be seen or not.
    run(e, "asm.suppress_pattern", json!({ "pattern": two["pattern"] }));
    run(e, "asm.delete_pattern", json!({ "pattern": two["pattern"], "keep_elements": true }));
    assert_eq!(tree_of(e, &more[0]), false);
    assert_eq!(tree_of(e, &more[1]), true);
    assert!((weighed(e) - 4000.0).abs() < 1e-6);
}

#[test]
fn a_part_file_gone_is_listed_once_however_many_ways_it_is_used() {
    let dir = scratch("gone");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    mirrored_ell(e);
    run(e, "asm.save", json!({ "path": "a.tenonasm" }));
    std::fs::remove_file(dir.join("ell.tenon")).unwrap();
    let opened = run(e, "asm.open", json!({ "path": "a.tenonasm" }));
    let missing = opened["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert!(missing[0].as_str().unwrap().contains("ell.tenon"), "{missing:?}");
    assert_eq!(opened["components"], 2);
    // Each component still says why, as ever.
    let tree = run(e, "asm.tree", json!({}));
    assert!(tree["components"].as_array().unwrap().iter().all(|c| !c["missing"].is_null()), "{tree}");
}

#[test]
fn what_a_mirror_cannot_follow_is_said_by_update_and_by_what_moves_its_source() {
    let dir = scratch("lost-plane");
    let mut e = Engine::new(kernel(), &dir);
    let e = &mut e;
    ell(e);
    box_part(e, "block", 10.0, 10.0, 10.0);
    run(e, "asm.new", json!({ "name": "Lost" }));
    let l = run(e, "asm.insert", json!({ "path": "ell.tenon", "grounded": false }))["component"].clone();
    let b = run(e, "asm.insert", json!({ "path": "block.tenon", "at": [100, 0, 0], "grounded": false }))["component"].clone();
    // The post's top, at z 30, which goes with the post.
    let top = cap(e, &l, 4, "end");
    let made = run(e, "asm.mirror", json!({ "components": [b], "plane": top }));
    let element = made["components"][0].clone();
    // All is well: nothing extra is said.
    let fine = run(e, "asm.update", json!({}));
    assert!(fine.get("patterns_failing").is_none(), "{fine}");
    assert!(run(e, "asm.move", json!({ "component": b, "by": [1, 0, 0] })).get("patterns_failing").is_none());
    let at = origin(e, &element);

    run(e, "asm.edit_part", json!({ "component": l, "run": "feature.delete", "with": { "feature": 4 } }));
    let u = run(e, "asm.update", json!({}));
    assert_eq!(u["converged"], true, "{u}");
    let failing = &u["patterns_failing"];
    assert_eq!(failing.as_array().unwrap().len(), 1, "{u}");
    assert_eq!(failing[0]["name"], "Mirror:1");
    assert!(failing[0]["message"].as_str().unwrap().contains("no longer exists"), "{u}");
    assert_eq!(failing[0]["pattern"], made["pattern"]);

    // A move of its source says so too, and the element is left where it was.
    let moved = run(e, "asm.move", json!({ "component": b, "by": [1, 0, 0] }));
    assert_eq!(moved["patterns_failing"][0]["name"], "Mirror:1", "{moved}");
    same(origin(e, &element), at);

    // A new plane mends it: nothing is said, and it stands in the new plane's image.
    let yz = run(e, "asm.geom", json!({ "plane": "yz" }));
    let mended = run(e, "asm.edit_pattern", json!({ "pattern": made["pattern"], "plane": yz }));
    assert!(mended.get("patterns_failing").is_none(), "{mended}");
    assert!(run(e, "asm.update", json!({})).get("patterns_failing").is_none());
    same(origin(e, &element), [-102.0, 0.0, 0.0]);
}
