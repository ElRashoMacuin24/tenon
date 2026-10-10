//! M6 demo: the four-fittings script (coil, threads, sweep, loft, draft, tapered extrusion, split,
//! combine, materials, design tables, and an assembly whose parts list weighs them) and the files
//! it writes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use serde_json::json;
use tenon_cli::Engine;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

fn kernel() -> Box<dyn Kernel> {
    Box::new(OcctKernel::new())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-cli-m6-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn near(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs()
}

#[test]
fn the_m6_demo_script_builds_four_verified_fittings_and_weighs_their_assembly() {
    // The script checks every volume and mass against its analytic value as it goes; here we
    // check the files it writes and that the saved parts reopen as what they were.
    let dir = scratch("demo");
    let r = tenon_cli::demo_m6(kernel(), &dir).unwrap();
    assert_eq!(r.json["ok"], true, "{}", r.text);

    // The nozzle: drafted flange + cone + tapered spout - bore, as STEP and in the project.
    let (t5, t3) = (5f64.to_radians().tan(), 3f64.to_radians().tan());
    let (b, r2) = (50.0 - 12.0 * t5, 8.0 - 20.0 * t3);
    let nozzle = 2.0 * (2500.0 + 50.0 * b + b * b) + 6240.0 * PI + PI * 20.0 / 3.0 * (64.0 + 8.0 * r2 + r2 * r2) - 896.0 * PI;
    let mut k = OcctKernel::new();
    let info = tenon_cli::info(&mut k, &dir.join("nozzle.step")).unwrap();
    let v = info.json["shapes"][0]["volume_mm3"].as_f64().unwrap();
    assert!(near(v, nozzle, 1e-6), "STEP volume {v} vs {nozzle}");
    assert!(std::fs::metadata(dir.join("nozzle.stl")).unwrap().len() > 1000);
    for png in ["spring.png", "spring-stiff.png", "bolt.png", "handle.png", "nozzle.png", "nozzle-section.png", "fittings.png"] {
        assert!(std::fs::read(dir.join(png)).unwrap().starts_with(&[0x89, b'P', b'N', b'G']), "{png}");
    }

    // The parts list: a material and a mass for each part.
    let csv = std::fs::read_to_string(dir.join("fittings-bom.csv")).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "Item,Part,Name,Quantity,Volume (mm^3),Material,Mass (g)");
    assert_eq!(lines[1], "1,nozzle,Nozzle,1,34993.304,PLA,43.392", "{csv}");
    assert!(lines[2].starts_with("2,bolt,Bolt,1,21") && lines[2].contains(",\"Steel, mild\",16."), "{csv}");
    assert!(lines[3].starts_with("3,spring,Spring,1,947.48") && lines[3].ends_with(",Stainless steel,7.580"), "{csv}");
    assert_eq!(lines[4], "4,handle,Handle,1,1857.849,ABS,1.932", "{csv}");
    assert_eq!(lines.len(), 5);

    // The saved spring reopens with its material and its table; the other size is one command away.
    let mut e = Engine::new(kernel(), &dir);
    e.exec("file.open", &json!({ "path": "spring.tenon" })).unwrap();
    let t = e.exec("table.show", &json!({})).unwrap();
    assert_eq!(t["table"]["active"], "Soft");
    assert_eq!(t["table"]["rows"][1], json!({ "name": "Stiff", "values": [3.0, 10.0] }));
    let turn = PI * 16.0 * PI;
    let m = e.exec("model.mass", &json!({})).unwrap();
    assert!(near(m["bodies"][0]["volume"].as_f64().unwrap(), 6.0 * turn, 1e-4) && m["material"] == "Stainless steel", "{m}");
    e.exec("table.activate", &json!({ "row": "Stiff" })).unwrap();
    let m = e.exec("model.mass", &json!({})).unwrap();
    assert!(near(m["mass"].as_f64().unwrap(), 10.0 * turn * 8.0 / 1000.0, 1e-4), "{m}");

    // The bolt keeps its cut thread, sized from the shank.
    e.exec("file.open", &json!({ "path": "bolt.tenon" })).unwrap();
    let t = e.exec("model.threads", &json!({})).unwrap();
    assert_eq!((t["threads"][0]["designation"].as_str(), t["threads"][0]["modelled"].as_bool()), (Some("M8x1.25"), Some(true)), "{t}");
    // The handle is red, though ABS is not.
    e.exec("file.open", &json!({ "path": "handle.tenon" })).unwrap();
    let c = e.exec("document.materials", &json!({})).unwrap();
    assert_eq!(
        (c["material"]["name"].as_str(), c["material"]["color"].as_str(), c["color"].as_str()),
        (Some("ABS"), Some("#e8e4d8"), Some("#d04030")),
        "{c}"
    );

    // The assembly reopens and weighs the same.
    let opened = e.exec("asm.open", &json!({ "path": "fittings.tenonasm" })).unwrap();
    assert_eq!(opened["missing"].as_array().map(Vec::len), Some(0), "{opened}");
    let total = e.exec("asm.mass", &json!({})).unwrap();
    assert!(near(total["mass"].as_f64().unwrap(), 69.42, 5e-3), "{total}");
}

#[test]
fn the_checked_in_m6_example_is_what_the_script_writes() {
    // The copies in the repository are compared with a fresh run as `tenon-cli diff` compares
    // files: everything that was put in, to the stored precision. (Face fingerprints are worked
    // out by the kernel and may differ in the last digit from one machine to another; the STEP
    // file holds a timestamp; pictures depend on the renderer.)
    let dir = scratch("same");
    tenon_cli::demo_m6(kernel(), &dir).unwrap();
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m6-fittings");
    for f in ["spring.tenon", "bolt.tenon", "handle.tenon", "nozzle.tenon", "fittings.tenonasm"] {
        let d = tenon_io::diff::diff_files(&example.join(f), &dir.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert!(d.same(), "{f}: regenerate the example (see its README):\n{}", d.text());
    }
    // And the comparison is not blind: the other size of the spring is a different part.
    let mut e = Engine::new(kernel(), &dir);
    e.exec("file.open", &json!({ "path": "spring.tenon" })).unwrap();
    e.exec("table.activate", &json!({ "row": "Stiff" })).unwrap();
    e.exec("file.save", &json!({ "path": "spring-stiff.tenon" })).unwrap();
    let d = tenon_io::diff::diff_files(&example.join("spring.tenon"), &dir.join("spring-stiff.tenon")).unwrap();
    assert!(d.text().contains("active row  Soft -> Stiff"), "{}", d.text());
}

/// A part file in `dir`: a pin of diameter 8 whose length (the extrusion's distance, `d0`) comes
/// in three sizes from a design table.
fn sized_pin(dir: &std::path::Path) {
    let mut e = Engine::new(kernel(), dir);
    e.exec("file.new", &json!({ "name": "Pin" })).unwrap();
    let sk = e.exec("sketch.create", &json!({ "plane": "xy" })).unwrap()["feature"].clone();
    e.exec("sketch.circle", &json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 4 })).unwrap();
    e.exec("model.extrude", &json!({ "sketch": sk, "distance": 20 })).unwrap();
    e.exec("document.material", &json!({ "name": "Steel, mild" })).unwrap();
    e.exec("table.create", &json!({ "columns": ["d0"], "row": "8x20" })).unwrap();
    e.exec("table.add_row", &json!({ "name": "8x30", "values": { "d0": 30 } })).unwrap();
    e.exec("table.add_row", &json!({ "name": "8x50", "values": { "d0": 50 } })).unwrap();
    e.exec("file.save", &json!({ "path": "pin.tenon" })).unwrap();
}

#[test]
fn components_of_one_part_file_come_in_the_sizes_of_its_design_table() {
    let dir = scratch("sizes");
    sized_pin(&dir);
    let mut e = Engine::new(kernel(), &dir);
    e.exec("asm.new", &json!({ "name": "Pins" })).unwrap();
    // The part as its file has it (20 long), then two more of it in other sizes.
    let a = e.exec("asm.insert", &json!({ "path": "pin.tenon" })).unwrap();
    let b = e.exec("asm.insert", &json!({ "path": "pin.tenon", "at": [20, 0, 0], "row": "8x50" })).unwrap();
    let c = e.exec("asm.insert", &json!({ "path": "pin.tenon", "at": [40, 0, 0], "row": "8x50" })).unwrap();
    assert_eq!((a["row"].is_null(), b["row"].as_str(), c["row"].as_str()), (true, Some("8x50"), Some("8x50")));
    let per_mm = PI * 16.0;
    let mass = e.exec("asm.mass", &json!({})).unwrap();
    assert!(near(mass["volume"].as_f64().unwrap(), per_mm * (20.0 + 50.0 + 50.0), 1e-9), "{mass}");
    assert!(near(mass["mass"].as_f64().unwrap(), per_mm * 120.0 * 7.85 / 1000.0, 1e-9), "{mass}");
    // The parts list has an item for each size, named with it.
    let bom = e.exec("asm.bom", &json!({})).unwrap();
    let rows = bom["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{bom}");
    assert_eq!((rows[0]["part"].as_str(), rows[0]["name"].as_str(), rows[0]["quantity"].as_u64()), (Some("pin"), Some("Pin"), Some(1)));
    assert_eq!((rows[1]["part"].as_str(), rows[1]["name"].as_str(), rows[1]["quantity"].as_u64()), (Some("pin (8x50)"), Some("Pin, 8x50"), Some(2)));
    assert!(near(rows[1]["volume"].as_f64().unwrap(), per_mm * 50.0, 1e-9) && rows[1]["material"] == "Steel, mild", "{bom}");
    let tree = e.exec("asm.tree", &json!({})).unwrap();
    assert_eq!(tree["components"][1]["row"], "8x50");
    assert!(tree["components"][0]["row"].is_null() && tree["components"][1]["missing"].is_null(), "{tree}");

    // A component changes size: the third one becomes 30 long; the first takes a size too.
    let r = e.exec("asm.set_row", &json!({ "component": c["component"], "row": "8x30" })).unwrap();
    assert_eq!(r["rows"], json!(["8x20", "8x30", "8x50"]));
    let mass = e.exec("asm.mass", &json!({})).unwrap();
    assert!(near(mass["volume"].as_f64().unwrap(), per_mm * (20.0 + 50.0 + 30.0), 1e-9), "{mass}");
    assert_eq!(e.exec("asm.bom", &json!({})).unwrap()["parts"], 3);
    // Undo puts it back; a size the part does not have is refused, saying which it has.
    e.exec("asm.undo", &json!({})).unwrap();
    let mass = e.exec("asm.mass", &json!({})).unwrap();
    assert!(near(mass["volume"].as_f64().unwrap(), per_mm * 120.0, 1e-9), "{mass}");
    let err = e.exec("asm.set_row", &json!({ "component": c["component"], "row": "8x99" })).unwrap_err();
    assert!(err.contains("no row `8x99`") && err.contains("8x20, 8x30, 8x50"), "{err}");
    let err = e.exec("asm.insert", &json!({ "path": "pin.tenon", "row": "long" })).unwrap_err();
    assert!(err.contains("no row `long`"), "{err}");

    // The part edited in place: every size follows (the pin is made thicker; each keeps its length).
    let info = e.exec("asm.edit_part", &json!({ "component": a["component"], "run": "sketch.info", "with": { "sketch": 1 } })).unwrap();
    let ring = info["result"]["entities"].as_array().unwrap().iter().find(|x| x["type"] == "circle").unwrap()["id"].clone();
    e.exec("asm.edit_part", &json!({ "component": a["component"], "run": "sketch.constrain", "with": { "sketch": 1, "constraint": { "type": "radius", "curve": ring, "value": 5.0 } } }))
        .unwrap();
    let mass = e.exec("asm.mass", &json!({})).unwrap();
    assert!(near(mass["volume"].as_f64().unwrap(), PI * 25.0 * 120.0, 1e-9), "{mass}");

    // Saved and opened again: the sizes are in the assembly file, one field on each component,
    // and no file is written for a size.
    e.exec("asm.save", &json!({ "path": "pins.tenonasm" })).unwrap();
    let text = std::fs::read_to_string(dir.join("pins.tenonasm")).unwrap();
    assert_eq!(text.matches("row = \"8x50\"").count(), 2, "{text}");
    assert_eq!(text.matches("row = ").count(), 2, "the component as its file has it says nothing: {text}");
    let files: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|f| f.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(files.len(), 2, "{files:?}");
    let mut again = Engine::new(kernel(), &dir);
    let opened = again.exec("asm.open", &json!({ "path": "pins.tenonasm" })).unwrap();
    assert_eq!(opened["missing"].as_array().map(Vec::len), Some(0), "{opened}");
    let mass = again.exec("asm.mass", &json!({})).unwrap();
    assert!(near(mass["volume"].as_f64().unwrap(), PI * 25.0 * 120.0, 1e-9), "{mass}");

    // A drawing of the assembly: the front view shows each pin at its own length (the tallest is
    // 50), and the parts list has the assembly's items, a size each.
    again.exec("drw.new", &json!({ "name": "Pins" })).unwrap();
    let front = again.exec("drw.view.base", &json!({ "model": "pins.tenonasm", "orientation": "front", "scale": 1, "at": [100, 100] })).unwrap();
    again.exec("drw.parts_list", &json!({ "view": front["view"] })).unwrap();
    let info = again.exec("drw.info", &json!({})).unwrap();
    let view = &info["views"][0];
    assert!(view["error"].is_null(), "{view}");
    let list = info["annotations"].as_array().unwrap().iter().find(|a| a["rows"].is_array()).unwrap_or_else(|| panic!("no parts list: {info}"));
    let rows = list["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{list}");
    assert_eq!((rows[0]["quantity"].as_u64(), rows[0]["part_number"].as_str(), rows[0]["description"].as_str()), (Some(1), Some("pin"), Some("Pin")));
    assert_eq!(
        (rows[1]["quantity"].as_u64(), rows[1]["part_number"].as_str(), rows[1]["description"].as_str()),
        (Some(2), Some("pin (8x50)"), Some("Pin, 8x50"))
    );
    again.exec("drw.export.dxf", &json!({ "path": "pins.dxf" })).unwrap();
    let tags = tenon_dxf::parse(&std::fs::read(dir.join("pins.dxf")).unwrap()).unwrap();
    let entities = tenon_dxf::sections(&tags).into_iter().find(|s| s.name == "ENTITIES").unwrap();
    let ys: Vec<f64> = tenon_dxf::records(&entities.tags)
        .iter()
        .filter(|(k, r)| k == "LINE" && r.iter().any(|t| t.code == 8 && t.str() == "VISIBLE"))
        .flat_map(|(_, r)| r.iter().filter(|t| t.code == 20 || t.code == 21).map(tenon_dxf::Tag::f64).collect::<Vec<_>>())
        .collect();
    let (lo, hi) = ys.iter().fold((f64::MAX, f64::MIN), |(lo, hi), y| (lo.min(*y), hi.max(*y)));
    assert!(near(hi - lo, 50.0, 1e-6), "the view is as tall as the longest pin: {lo} to {hi}");
    std::fs::remove_file(dir.join("pins.dxf")).unwrap();

    // A row that the part no longer has: the component says so, the others are there.
    let pin = std::fs::read_to_string(dir.join("pin.tenon")).unwrap();
    std::fs::write(dir.join("pin.tenon"), pin.replace("name = \"8x50\"", "name = \"8x55\"")).unwrap();
    let mut gone = Engine::new(kernel(), &dir);
    let opened = gone.exec("asm.open", &json!({ "path": "pins.tenonasm" })).unwrap();
    let missing = opened["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 1, "{opened}");
    assert!(missing[0].as_str().unwrap().contains("cannot be had at row `8x50`"), "{opened}");
    let tree = gone.exec("asm.tree", &json!({})).unwrap();
    assert!(tree["components"][0]["missing"].is_null() && tree["components"][1]["missing"].is_string(), "{tree}");
}
