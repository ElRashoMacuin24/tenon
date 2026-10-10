//! M6, the part as a whole: what it is made of (material, appearance, mass) and the sizes it
//! comes in (the design table).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> String {
    match s.exec(id, &p, Some(k)) {
        Err(e) => e.0,
        Ok(v) => panic!("{id} {p} was not refused: {v}"),
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1.0)
}

/// A block `width` x `d1` x `d2` (40 x 20 x 10 to begin with): its width follows the user
/// parameter `width`, its depth is the dimension d1 and its height the extrusion's distance d2.
fn block(s: &mut Session, k: &mut OcctKernel) -> u64 {
    run(s, k, "param.add", json!({ "name": "width", "equation": "40 mm" }));
    let sk = run(s, k, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let lines = run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }))["lines"].clone();
    run(s, k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": lines[0], "value": 40.0 }, "equation": "width" }));
    run(s, k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": lines[1], "value": 20.0 } }));
    run(s, k, "model.extrude", json!({ "sketch": sk, "distance": 10 }))["feature"].as_u64().unwrap()
}

fn mass(s: &mut Session, k: &mut OcctKernel) -> Value {
    run(s, k, "model.mass", json!({}))
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    mass(s, k)["bodies"][0]["volume"].as_f64().unwrap()
}

#[test]
fn a_material_gives_the_part_its_mass_and_its_colour() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    // 8 cubic centimetres. With no material it weighs as water does, and has no colour of its own.
    let m = mass(&mut s, &mut k);
    assert!(near(m["bodies"][0]["volume"].as_f64().unwrap(), 8000.0));
    assert!(near(m["mass"].as_f64().unwrap(), 8.0) && m["material"].is_null() && near(m["density"].as_f64().unwrap(), 1.0), "{m}");
    assert!(s.document().color().is_none());
    // From the library, whatever the capitals: mild steel is 7.85 g/cm^3.
    let r = run(&mut s, &mut k, "document.material", json!({ "name": "steel, mild" }));
    assert_eq!(r["material"], json!({ "name": "Steel, mild", "density": 7.85, "color": "#8a9199" }));
    let m = mass(&mut s, &mut k);
    assert!(near(m["mass"].as_f64().unwrap(), 62.8) && m["material"] == "Steel, mild", "{m}");
    assert!(near(m["bodies"][0]["mass"].as_f64().unwrap(), 62.8));
    assert_eq!(s.document().color(), Some([0x8a, 0x91, 0x99]));
    // The centre of mass is the block's middle, and its inertia is in grams and millimetres: about
    // the axis along its height, m (w^2 + d^2) / 12.
    let com: Vec<f64> = m["bodies"][0]["center_of_mass"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!(near(com[0], 20.0) && near(com[1], 10.0) && near(com[2], 5.0), "{com:?}");
    let izz = m["bodies"][0]["inertia"][2][2].as_f64().unwrap();
    assert!(near(izz, 62.8 * (1600.0 + 400.0) / 12.0), "{izz}");
    // A density asked for just this once.
    assert!(near(run(&mut s, &mut k, "model.mass", json!({ "density": 2.0 }))["mass"].as_f64().unwrap(), 16.0));
    // A library material with a density of one's own (this batch of PLA is a little heavier),
    // and a material that is not in the library at all.
    run(&mut s, &mut k, "document.material", json!({ "name": "PLA", "density": 1.3 }));
    assert!(near(mass(&mut s, &mut k)["mass"].as_f64().unwrap(), 10.4));
    let r = run(&mut s, &mut k, "document.material", json!({ "name": "Casting resin", "density": 1.1, "color": "#20A0C0" }));
    assert_eq!(r["material"], json!({ "name": "Casting resin", "density": 1.1, "color": "#20a0c0" }));
    assert!(near(mass(&mut s, &mut k)["mass"].as_f64().unwrap(), 8.8));
    // A colour of the part's own goes over the material's, and comes off again.
    let r = run(&mut s, &mut k, "document.appearance", json!({ "color": "#FF8000" }));
    assert_eq!((r["appearance"].as_str(), r["color"].as_str()), (Some("#ff8000"), Some("#ff8000")));
    assert_eq!(s.document().color(), Some([255, 128, 0]));
    let r = run(&mut s, &mut k, "document.appearance", json!({ "color": null }));
    assert_eq!((r["appearance"].is_null(), r["color"].as_str()), (true, Some("#20a0c0")));
    // Each change is one step back.
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert_eq!(s.document().appearance(), Some("#ff8000"));
    run(&mut s, &mut k, "edit.undo", json!({}));
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert_eq!(s.document().material().map(|m| m.name.as_str()), Some("PLA"));
    // No material again.
    run(&mut s, &mut k, "document.material", json!({ "name": null }));
    assert!(near(mass(&mut s, &mut k)["mass"].as_f64().unwrap(), 8.0));
    // The library, and what is refused.
    let lib = run(&mut s, &mut k, "document.materials", json!({}));
    assert_eq!(lib["library"].as_array().map(Vec::len), Some(22));
    assert!(lib["library"].as_array().unwrap().iter().any(|m| m["name"] == "Aluminium 6061" && m["density"] == 2.7));
    assert!(refused(&mut s, &mut k, "document.material", json!({ "name": "Unobtainium" })).contains("not in the material library"));
    assert!(refused(&mut s, &mut k, "document.material", json!({ "name": "Mine", "density": 0 })).contains("more than 0"));
    assert!(refused(&mut s, &mut k, "document.material", json!({ "name": "Mine", "density": 2, "color": "blue" })).contains("#rrggbb"));
    assert!(refused(&mut s, &mut k, "document.appearance", json!({ "color": "red" })).contains("#rrggbb"));
    assert!(refused(&mut s, &mut k, "model.mass", json!({ "density": -1 })).contains("more than 0"));
}

/// The table's rows as (name, values).
fn rows(v: &Value) -> Vec<(String, Vec<f64>)> {
    v["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["name"].as_str().unwrap().to_owned(), r["values"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()))
        .collect()
}

#[test]
fn a_design_table_holds_the_sizes_of_one_part() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let extrusion = block(&mut s, &mut k);
    assert!(run(&mut s, &mut k, "table.show", json!({}))["table"].is_null());
    // A table of the block's three sizes: the part as it stands is its first row.
    let t = run(&mut s, &mut k, "table.create", json!({ "columns": ["width", "d1", "d2"], "row": "Small" }));
    assert_eq!(t["table"]["active"], "Small");
    assert_eq!(rows(&t), [("Small".to_owned(), vec![40.0, 20.0, 10.0])]);
    // A second size: what is not said is as the first.
    let t = run(&mut s, &mut k, "table.add_row", json!({ "name": "Large", "values": { "width": 80, "d2": 25 } }));
    assert_eq!(rows(&t)[1], ("Large".to_owned(), vec![80.0, 20.0, 25.0]));
    assert!(near(volume(&mut s, &mut k), 8000.0), "adding a row does not change the part");
    // Made active, the part is that size; and back.
    run(&mut s, &mut k, "table.activate", json!({ "row": "Large" }));
    assert!(near(volume(&mut s, &mut k), 80.0 * 20.0 * 25.0));
    run(&mut s, &mut k, "table.activate", json!({ "row": "Small" }));
    assert!(near(volume(&mut s, &mut k), 8000.0));
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(near(volume(&mut s, &mut k), 40000.0), "one step back is the size before");
    run(&mut s, &mut k, "edit.redo", json!({}));
    // The active row follows the part: an edit made the usual way is that size's from then on.
    run(&mut s, &mut k, "param.set", json!({ "name": "width", "equation": "50 mm" }));
    run(&mut s, &mut k, "param.set", json!({ "name": "d2", "equation": 12.0 }));
    let t = run(&mut s, &mut k, "table.show", json!({}));
    assert_eq!(rows(&t), [("Small".to_owned(), vec![50.0, 20.0, 12.0]), ("Large".to_owned(), vec![80.0, 20.0, 25.0])]);
    run(&mut s, &mut k, "table.activate", json!({ "row": "Large" }));
    run(&mut s, &mut k, "table.activate", json!({ "row": "Small" }));
    assert!(near(volume(&mut s, &mut k), 50.0 * 20.0 * 12.0), "and it comes back with them");
    // A value set in the table: in another row it waits there, in the active row it changes the part.
    run(&mut s, &mut k, "table.set", json!({ "row": "Large", "column": "d1", "value": 30 }));
    assert!(near(volume(&mut s, &mut k), 50.0 * 20.0 * 12.0));
    run(&mut s, &mut k, "table.set", json!({ "row": "Small", "column": "d1", "value": 15 }));
    assert!(near(volume(&mut s, &mut k), 50.0 * 15.0 * 12.0));
    // Rows are renamed and removed, but not the one the part is at.
    let t = run(&mut s, &mut k, "table.rename_row", json!({ "name": "Small", "to": "S" }));
    assert_eq!(t["table"]["active"], "S");
    assert!(refused(&mut s, &mut k, "table.remove_row", json!({ "name": "S" })).contains("make another row active first"));
    assert!(refused(&mut s, &mut k, "table.add_row", json!({ "name": "Large" })).contains("already"));
    assert!(refused(&mut s, &mut k, "table.add_row", json!({ "name": "XL", "values": { "height": 1 } })).contains("no column `height`"));
    assert!(refused(&mut s, &mut k, "table.activate", json!({ "row": "Huge" })).contains("no row `Huge`"));
    // A parameter renamed keeps its column.
    run(&mut s, &mut k, "param.rename", json!({ "name": "width", "to": "W" }));
    assert_eq!(run(&mut s, &mut k, "table.show", json!({}))["table"]["columns"], json!(["W", "d1", "d2"]));
    // One that starts to follow an equation leaves the table: the table would overwrite it.
    run(&mut s, &mut k, "param.set", json!({ "name": "d1", "equation": "W / 2" }));
    let t = run(&mut s, &mut k, "table.show", json!({}));
    assert_eq!(t["table"]["columns"], json!(["W", "d2"]));
    assert_eq!(rows(&t), [("S".to_owned(), vec![50.0, 12.0]), ("Large".to_owned(), vec![80.0, 25.0])]);
    assert!(refused(&mut s, &mut k, "table.add_column", json!({ "name": "d1" })).contains("follows an equation"));
    assert!(refused(&mut s, &mut k, "table.add_column", json!({ "name": "d0" })).contains("follows an equation"));
    assert!(refused(&mut s, &mut k, "table.add_column", json!({ "name": "nothing" })).contains("no parameter"));
    // So does one whose feature is deleted; and a table with nothing left to set is gone.
    run(&mut s, &mut k, "feature.delete", json!({ "feature": extrusion }));
    assert_eq!(run(&mut s, &mut k, "table.show", json!({}))["table"]["columns"], json!(["W"]));
    assert!(refused(&mut s, &mut k, "table.remove_column", json!({ "name": "W" })).contains("at least one parameter"));
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert_eq!(run(&mut s, &mut k, "table.show", json!({}))["table"]["columns"], json!(["W", "d2"]));
    // Only one table, of parameters that exist; deleting it leaves the part at its size.
    assert!(refused(&mut s, &mut k, "table.create", json!({ "columns": ["W"] })).contains("already"));
    run(&mut s, &mut k, "table.activate", json!({ "row": "Large" }));
    run(&mut s, &mut k, "table.delete", json!({}));
    assert!(run(&mut s, &mut k, "table.show", json!({}))["table"].is_null());
    assert!(near(volume(&mut s, &mut k), 80.0 * 40.0 * 25.0), "Large: 80 wide, half that deep, 25 high: {}", volume(&mut s, &mut k));
    assert!(refused(&mut s, &mut k, "table.activate", json!({ "row": "Large" })).contains("no design table"));
    assert!(refused(&mut s, &mut k, "table.create", json!({ "columns": ["missing"] })).contains("no parameter"));
    assert!(refused(&mut s, &mut k, "table.create", json!({ "columns": ["W", "W"] })).contains("only once"));
}
