//! M7, standard parts through commands: the catalogue, a part started from a standard one, and
//! standard parts placed in an assembly in chosen sizes, held together and changed.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;
use std::path::PathBuf;

use serde_json::{Value, json};
use tenon_cli::Engine;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

fn kernel() -> Box<dyn Kernel> {
    Box::new(OcctKernel::new())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-cli-m7-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn near(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

fn hexagon(across: f64) -> f64 {
    3f64.sqrt() / 2.0 * across * across
}

fn disc(d: f64) -> f64 {
    PI * d * d / 4.0
}

#[test]
fn the_catalogue_lists_the_standard_parts_and_a_part_is_started_from_one() {
    let dir = scratch("part");
    let mut e = Engine::new(kernel(), &dir);
    let list = e.exec("library.list", &json!({})).unwrap();
    let parts = list["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 10, "{list}");
    let screw = parts.iter().find(|p| p["part"] == "hex-screw-iso-4017").unwrap();
    assert_eq!(
        (screw["name"].as_str(), screw["category"].as_str(), screw["standard"].as_str()),
        (Some("Hex Head Screw"), Some("Fasteners"), Some("ISO 4017"))
    );
    assert!(screw["sizes"].as_array().unwrap().contains(&json!("M8x30")) && screw["about"].as_str().unwrap().contains("cosmetic"), "{screw}");
    let sections = e.exec("library.list", &json!({ "category": "sections" })).unwrap();
    assert_eq!(sections["parts"].as_array().unwrap().len(), 5);
    assert!(sections["parts"].as_array().unwrap().iter().all(|p| p["category"] == "Sections"));

    // A length of 40 x 20 x 2 tube: the standard gives the section, the length is the part's own.
    let made = e.exec("library.new", &json!({ "part": "tube-rectangular", "size": "40x20x2" })).unwrap();
    assert_eq!((made["name"].as_str(), made["size"].as_str()), (Some("Square and Rectangular Tube"), Some("40x20x2")));
    e.exec("param.set", &json!({ "name": "L", "equation": "250 mm" })).unwrap();
    let m = e.exec("model.mass", &json!({})).unwrap();
    let section = 40.0 * 20.0 - 36.0 * 16.0;
    assert!(near(m["bodies"][0]["volume"].as_f64().unwrap(), section * 250.0, 1e-9), "{m}");
    assert!(m["material"] == "Aluminium 6061" && near(m["mass"].as_f64().unwrap(), section * 250.0 * 2.7 / 1000.0, 1e-9), "{m}");
    // It is a part file like any other: saved, opened, another size taken from its table.
    e.exec("file.save", &json!({ "path": "rail.tenon" })).unwrap();
    let mut again = Engine::new(kernel(), &dir);
    again.exec("file.open", &json!({ "path": "rail.tenon" })).unwrap();
    again.exec("table.activate", &json!({ "row": "50x30x3" })).unwrap();
    let m = again.exec("model.mass", &json!({})).unwrap();
    assert!(near(m["bodies"][0]["volume"].as_f64().unwrap(), (50.0 * 30.0 - 44.0 * 24.0) * 250.0, 1e-9), "{m}");

    // What there is not is refused, saying what there is.
    let err = e.exec("library.new", &json!({ "part": "widget" })).unwrap_err();
    assert!(err.contains("there is no standard part `widget`") && err.contains("hex-nut-iso-4032"), "{err}");
    let err = e.exec("library.new", &json!({ "part": "washer-iso-7089", "size": "M7" })).unwrap_err();
    assert!(err.contains("Plain Washer does not come in `M7`") && err.contains("M6, M8"), "{err}");
    assert!(e.exec("library.new", &json!({})).unwrap_err().contains("missing parameter `part`"));
}

#[test]
fn standard_parts_are_placed_in_sizes_held_together_and_changed() {
    let dir = scratch("assembly");
    let mut e = Engine::new(kernel(), &dir);
    let run = |e: &mut Engine, id: &str, p: Value| e.exec(id, &p).unwrap_or_else(|err| panic!("{id} {p}: {err}"));
    run(&mut e, "asm.new", json!({ "name": "Stack" }));
    // A washer, a screw through it and a second screw of another length: three components,
    // two part files.
    let washer = run(&mut e, "asm.insert_standard", json!({ "part": "washer-iso-7089", "size": "M8", "path": "standard" }));
    let screw = run(&mut e, "asm.insert_standard", json!({ "part": "hex-screw-iso-4017", "size": "M8x30", "path": "standard", "at": [30, 0, 0] }));
    let long = run(&mut e, "asm.insert_standard", json!({ "part": "hex-screw-iso-4017", "size": "M8x50", "path": "standard", "at": [60, 0, 0] }));
    assert_eq!((washer["written"].as_bool(), screw["written"].as_bool(), long["written"].as_bool()), (Some(true), Some(true), Some(false)));
    assert_eq!((screw["row"].as_str(), long["row"].as_str()), (Some("M8x30"), Some("M8x50")));
    let files: Vec<String> =
        std::fs::read_dir(dir.join("standard")).unwrap().map(|f| f.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(files.len(), 2, "{files:?}");
    assert!(files.contains(&"hex-screw-iso-4017.tenon".to_owned()) && files.contains(&"washer-iso-7089.tenon".to_owned()));

    // The parts list names each size; the mass is steel's.
    let bom = run(&mut e, "asm.bom", json!({}));
    let names: Vec<&str> = bom["rows"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Plain Washer, M8", "Hex Head Screw, M8x30", "Hex Head Screw, M8x50"]);
    let (w, s30, s50) = ((disc(16.0) - disc(8.4)) * 1.6, hexagon(13.0) * 5.3 + disc(8.0) * 30.0, hexagon(13.0) * 5.3 + disc(8.0) * 50.0);
    let mass = run(&mut e, "asm.mass", json!({}));
    assert!(near(mass["volume"].as_f64().unwrap(), w + s30 + s50, 1e-9), "{mass}");
    assert!(near(mass["mass"].as_f64().unwrap(), (w + s30 + s50) * 7.85 / 1000.0, 1e-9), "{mass}");

    // The screw is inserted in the washer: the edge under its head on the rim of the washer's
    // hole. Its head then sits on the washer, 1.6 up.
    let under_head = json!([{ "type": "cap", "feature": 2, "end": "start" }, { "type": "side", "feature": 4, "curve": 2 }]);
    let rim = json!([{ "type": "cap", "feature": 2, "end": "end" }, { "type": "side", "feature": 2, "curve": 2 }]);
    let a = run(&mut e, "asm.geom", json!({ "component": washer["component"], "edge": rim }));
    let b = run(&mut e, "asm.geom", json!({ "component": screw["component"], "edge": under_head }));
    assert_eq!((a["kind"].as_str(), b["kind"].as_str()), (Some("circle"), Some("circle")), "{a} {b}");
    let held = run(&mut e, "asm.constrain", json!({ "type": "insert", "a": a, "b": b }));
    assert!(held["name"].as_str().unwrap().starts_with("Insert"), "{held}");
    let origin = |e: &mut Engine, i: usize| -> Vec<f64> {
        e.exec("asm.tree", &json!({})).unwrap()["components"][i]["placement"]["origin"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect()
    };
    let at = origin(&mut e, 1);
    assert!(at[0].abs() < 1e-9 && at[1].abs() < 1e-9 && near(at[2], 1.6, 1e-9), "{at:?}");

    // The washer becomes an M12: thicker, so the screw rides up with it (the same faces are
    // found in every size). Then the screw becomes a longer one and stays where it is held.
    run(&mut e, "asm.set_row", json!({ "component": washer["component"], "row": "M12" }));
    let at = origin(&mut e, 1);
    assert!(near(at[2], 2.5, 1e-9), "the M12 washer is 2.5 thick: {at:?}");
    run(&mut e, "asm.set_row", json!({ "component": screw["component"], "row": "M8x80" }));
    let at = origin(&mut e, 1);
    assert!(near(at[2], 2.5, 1e-9) && at[0].abs() < 1e-9, "{at:?}");
    let tree = run(&mut e, "asm.tree", json!({}));
    assert!(tree["relationships"].as_array().unwrap().iter().all(|r| r["failing"].is_null()), "{tree}");
    let mass = run(&mut e, "asm.mass", json!({}));
    let (w12, s80) = ((disc(24.0) - disc(13.0)) * 2.5, hexagon(13.0) * 5.3 + disc(8.0) * 80.0);
    assert!(near(mass["volume"].as_f64().unwrap(), w12 + s80 + s50, 1e-9), "{mass}");

    // Saved and opened again from another place: the folder of standard parts goes with it.
    run(&mut e, "asm.save", json!({ "path": "stack.tenonasm" }));
    let text = std::fs::read_to_string(dir.join("stack.tenonasm")).unwrap();
    assert!(text.contains("part = \"standard/hex-screw-iso-4017.tenon\"") && text.contains("row = \"M8x80\""), "{text}");
    let moved = scratch("moved");
    std::fs::create_dir_all(moved.join("standard")).unwrap();
    std::fs::copy(dir.join("stack.tenonasm"), moved.join("stack.tenonasm")).unwrap();
    for f in ["hex-screw-iso-4017.tenon", "washer-iso-7089.tenon"] {
        std::fs::copy(dir.join("standard").join(f), moved.join("standard").join(f)).unwrap();
    }
    let mut again = Engine::new(kernel(), &moved);
    let opened = again.exec("asm.open", &json!({ "path": "stack.tenonasm" })).unwrap();
    assert_eq!(opened["missing"].as_array().map(Vec::len), Some(0), "{opened}");
    let mass = again.exec("asm.mass", &json!({})).unwrap();
    assert!(near(mass["volume"].as_f64().unwrap(), w12 + s80 + s50, 1e-9), "{mass}");

    // What there is not is refused, saying what there is; nothing is written for it.
    let err = e.exec("asm.insert_standard", &json!({ "part": "hex-nut-iso-4032", "size": "M7", "path": "standard" })).unwrap_err();
    assert!(err.contains("Hex Nut does not come in `M7`") && err.contains("M6, M8"), "{err}");
    let err = e.exec("asm.insert_standard", &json!({ "part": "hex-nut-iso-4032", "path": "standard" })).unwrap_err();
    assert!(err.contains("missing parameter `size`"), "{err}");
    let err = e.exec("asm.insert_standard", &json!({ "part": "sprocket", "size": "M8", "path": "standard" })).unwrap_err();
    assert!(err.contains("there is no standard part `sprocket`"), "{err}");
    assert_eq!(std::fs::read_dir(dir.join("standard")).unwrap().count(), 2);
}

/// A block `x` by `y` by `z` from the origin, in a new part.
fn block(e: &mut Engine, x: f64, y: f64, z: f64) {
    e.exec("file.new", &json!({ "name": "Block" })).unwrap();
    let sk = e.exec("sketch.create", &json!({ "plane": "xy" })).unwrap()["feature"].clone();
    e.exec("sketch.rectangle", &json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": x, "y2": y })).unwrap();
    e.exec("model.extrude", &json!({ "sketch": sk, "distance": z })).unwrap();
}

/// A table: a 10 x 10 leg 20 high under the middle of a 40 x 20 top 2 thick.
fn table(e: &mut Engine) {
    e.exec("file.new", &json!({ "name": "Table" })).unwrap();
    let leg = e.exec("sketch.create", &json!({ "plane": "xy" })).unwrap()["feature"].clone();
    e.exec("sketch.rectangle", &json!({ "sketch": leg, "x1": 15, "y1": 5, "x2": 25, "y2": 15 })).unwrap();
    e.exec("model.extrude", &json!({ "sketch": leg, "distance": 20 })).unwrap();
    let plane = e.exec("work.plane", &json!({ "base": "xy", "distance": 20 })).unwrap()["feature"].clone();
    let top = e.exec("sketch.create", &json!({ "work_plane": plane })).unwrap()["feature"].clone();
    e.exec("sketch.rectangle", &json!({ "sketch": top, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    e.exec("model.extrude", &json!({ "sketch": top, "distance": 2 })).unwrap();
}

#[test]
fn the_print_check_reads_a_part_as_a_printer_would() {
    let dir = scratch("print");
    let mut e = Engine::new(kernel(), &dir);
    // A plain block: closed, nothing overhanging, 10 through its thinnest way, 12 triangles.
    block(&mut e, 40.0, 20.0, 10.0);
    let c = e.exec("print.check", &json!({})).unwrap();
    assert_eq!(c["ok"], true, "{c}");
    assert_eq!(c["watertight"], json!({ "ok": true, "open_edges": 0, "crowded_edges": 0, "flipped_edges": 0 }));
    assert!(c["overhangs"]["ok"] == true && c["overhangs"]["faces"].as_array().unwrap().is_empty() && c["overhangs"]["limit"] == 45.0, "{c}");
    assert!(c["walls"]["ok"] == true && near(c["walls"]["thinnest"].as_f64().unwrap(), 10.0, 1e-6) && c["walls"]["min_wall"] == 0.8, "{c}");
    assert_eq!(
        (c["size"]["x"].as_f64(), c["size"]["y"].as_f64(), c["size"]["z"].as_f64(), c["size"]["height"].as_f64()),
        (Some(40.0), Some(20.0), Some(10.0), Some(10.0))
    );
    assert!(c["size"]["ok"].is_null() && c["size"]["bed"].is_null(), "no bed, no verdict: {c}");
    assert!(near(c["volume"].as_f64().unwrap(), 8000.0, 1e-9) && c["triangles"] == 12 && c["up"] == "z", "{c}");
    // The bed: 40 long does not go on a 30 x 30 bed, and does on one 50 long (either way round).
    let c = e.exec("print.check", &json!({ "bed": [30, 30, 30] })).unwrap();
    assert!(c["size"]["ok"] == false && c["ok"] == false, "{c}");
    for bed in [json!([50, 30, 30]), json!([30, 50, 30])] {
        assert_eq!(e.exec("print.check", &json!({ "bed": bed })).unwrap()["size"]["ok"], true);
    }
    // Stood on its end (X up) it is 40 high on a 20 x 10 footprint.
    let c = e.exec("print.check", &json!({ "up": "x", "bed": [25, 25, 50] })).unwrap();
    assert!(c["size"]["height"] == 40.0 && c["size"]["ok"] == true && c["up"] == "x", "{c}");
    assert_eq!(e.exec("print.check", &json!({ "up": "x", "bed": [25, 25, 30] })).unwrap()["size"]["ok"], false);

    // A table: the underside of its top hangs over air, all but where the leg is.
    table(&mut e);
    let c = e.exec("print.check", &json!({})).unwrap();
    assert!(c["ok"] == false && c["watertight"]["ok"] == true && c["overhangs"]["ok"] == false, "{c}");
    let faces = c["overhangs"]["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 1, "{c}");
    assert!(near(faces[0]["area"].as_f64().unwrap(), 700.0, 1e-9) && near(faces[0]["steepest"].as_f64().unwrap(), 90.0, 1e-9), "{c}");
    assert!(near(c["overhangs"]["area"].as_f64().unwrap(), 700.0, 1e-9));
    // The face it names is the one a later command would find there: its underside.
    let face = e.exec("model.faces", &json!({ "body": 0 })).unwrap()["faces"][faces[0]["face"].as_u64().unwrap() as usize].clone();
    assert!(near(face["centroid"][2].as_f64().unwrap(), 20.0, 1e-9) && face["surface"]["Plane"]["normal"]["z"].as_f64().unwrap() < -0.99, "{face}");
    // Turned over, the top lies on the bed and nothing overhangs.
    let c = e.exec("print.check", &json!({ "up": "-z" })).unwrap();
    assert!(c["overhangs"]["ok"] == true && c["ok"] == true, "{c}");
    // Its top is 2 thick: fine for a 0.8 wall, too thin when 3 is asked for.
    assert!(near(c["walls"]["thinnest"].as_f64().unwrap(), 2.0, 1e-6), "{c}");
    let c = e.exec("print.check", &json!({ "up": "-z", "min_wall": 3 })).unwrap();
    assert!(c["walls"]["ok"] == false && c["ok"] == false, "{c}");
    let thin = c["walls"]["faces"].as_array().unwrap();
    assert_eq!(thin.len(), 2, "the top's two faces: {c}");
    assert!(thin.iter().all(|t| near(t["thinnest"].as_f64().unwrap(), 2.0, 1e-6)), "{c}");
    let areas: f64 = thin.iter().map(|t| t["area"].as_f64().unwrap()).sum();
    assert!(near(areas, 800.0 + 700.0, 1e-9), "{c}");

    // A hole bored level through a block: the top of the bore overhangs, a quarter of the way
    // round (45 degrees each side of its crown).
    block(&mut e, 40.0, 20.0, 20.0);
    let side = e.exec("sketch.create", &json!({ "plane": "xz" })).unwrap()["feature"].clone();
    e.exec("sketch.circle", &json!({ "sketch": side, "cx": 20, "cy": 10, "r": 6 })).unwrap();
    e.exec("model.extrude", &json!({ "sketch": side, "symmetric": 100, "operation": "cut" })).unwrap();
    let c = e.exec("print.check", &json!({})).unwrap();
    let faces = c["overhangs"]["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 1, "{c}");
    let quarter = std::f64::consts::PI / 2.0 * 6.0 * 20.0;
    assert!(near(faces[0]["area"].as_f64().unwrap(), quarter, 0.3) && faces[0]["steepest"].as_f64().unwrap() > 80.0, "{c} vs {quarter}");
    // Allowed to lean right over, nothing does.
    assert_eq!(e.exec("print.check", &json!({ "overhang": 90 })).unwrap()["overhangs"]["ok"], true);

    // What is not a setting is refused; so is a part with nothing in it.
    for (bad, what) in [
        (json!({ "min_wall": 0 }), "`min_wall` must be more than 0"),
        (json!({ "overhang": 100 }), "`overhang` must be more than 0 and at most 90"),
        (json!({ "up": "w" }), "`up` is `w`"),
        (json!({ "bed": [200, 200] }), "`bed` must be three sizes"),
    ] {
        let err = e.exec("print.check", &bad).unwrap_err();
        assert!(err.contains(what), "{bad}: {err}");
    }
    e.exec("file.new", &json!({})).unwrap();
    assert!(e.exec("print.check", &json!({})).unwrap_err().contains("no solid"));
}

/// The corners and triangles of the first object in a 3MF file, and how many objects and build
/// items it has.
fn read_3mf(path: &std::path::Path) -> (Vec<[f64; 3]>, Vec<[usize; 3]>, usize, usize, String) {
    use std::io::Read as _;
    let mut za = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut text = String::new();
    za.by_name("3D/3dmodel.model").unwrap().read_to_string(&mut text).unwrap();
    let attrs = |line: &str| -> Vec<String> { line.split('"').skip(1).step_by(2).map(str::to_owned).collect() };
    let first = text.split("</object>").next().unwrap();
    let corners = first.lines().filter(|l| l.contains("<vertex ")).map(|l| {
        let a = attrs(l);
        [a[0].parse().unwrap(), a[1].parse().unwrap(), a[2].parse().unwrap()]
    });
    let triangles = first.lines().filter(|l| l.contains("<triangle ")).map(|l| {
        let a = attrs(l);
        [a[0].parse().unwrap(), a[1].parse().unwrap(), a[2].parse().unwrap()]
    });
    (corners.collect(), triangles.collect(), text.matches("<object ").count(), text.matches("<item ").count(), text)
}

/// The triangles of a binary STL file as corners and indices, equal corners made one.
fn read_stl(path: &std::path::Path) -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    let data = std::fs::read(path).unwrap();
    let count = u32::from_le_bytes(data[80..84].try_into().unwrap()) as usize;
    assert_eq!(data.len(), 84 + 50 * count, "a well-formed binary STL");
    let (mut corners, mut at, mut triangles) = (Vec::new(), std::collections::HashMap::new(), Vec::new());
    for t in 0..count {
        let mut tri = [0usize; 3];
        for (v, slot) in tri.iter_mut().enumerate() {
            let o = 84 + 50 * t + 12 + 12 * v;
            let p: Vec<f32> = (0..3).map(|k| f32::from_le_bytes(data[o + 4 * k..o + 4 * k + 4].try_into().unwrap())).collect();
            let key = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()];
            *slot = *at.entry(key).or_insert_with(|| {
                corners.push([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
                corners.len() - 1
            });
        }
        triangles.push(tri);
    }
    (corners, triangles)
}

/// A mesh read back from a file: every edge joins two triangles that face the same way, and the
/// volume it encloses.
fn closed_volume(corners: &[[f64; 3]], triangles: &[[usize; 3]]) -> (bool, f64) {
    let mut edges: std::collections::HashMap<(usize, usize), (u32, u32)> = std::collections::HashMap::new();
    let mut volume = 0.0;
    for t in triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let e = edges.entry((a.min(b), a.max(b))).or_default();
            if a < b {
                e.0 += 1;
            } else {
                e.1 += 1;
            }
        }
        let (a, b, c) = (corners[t[0]], corners[t[1]], corners[t[2]]);
        volume += (a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0;
    }
    (edges.values().all(|e| *e == (1, 1)), volume)
}

#[test]
fn stl_and_3mf_files_hold_closed_solids_of_the_right_volume() {
    let dir = scratch("export");
    let mut e = Engine::new(kernel(), &dir);
    // A block is exact in both, and comes out as its eight corners.
    block(&mut e, 40.0, 20.0, 10.0);
    let r = e.exec("export.3mf", &json!({ "path": "block.3mf" })).unwrap();
    assert_eq!((r["objects"].as_u64(), r["triangles"].as_u64(), r["watertight"].as_bool()), (Some(1), Some(12), Some(true)), "{r}");
    let (corners, triangles, objects, items, text) = read_3mf(&dir.join("block.3mf"));
    assert_eq!((corners.len(), triangles.len(), objects, items), (8, 12, 1, 1));
    let (closed, volume) = closed_volume(&corners, &triangles);
    assert!(closed && near(volume, 8000.0, 1e-12), "{closed} {volume}");
    assert!(text.contains("<base name=\"Generic\" displaycolor=\"#B8C0C8FF\"/>") && text.contains("name=\"Block\""), "{text}");
    let r = e.exec("export.stl", &json!({ "path": "block.stl" })).unwrap();
    assert_eq!((r["triangles"].as_u64(), r["watertight"].as_bool()), (Some(12), Some(true)), "{r}");
    let (corners, triangles) = read_stl(&dir.join("block.stl"));
    let (closed, volume) = closed_volume(&corners, &triangles);
    assert!(closed && near(volume, 8000.0, 1e-12), "{closed} {volume}");

    // The M6 example's parts: curved, bored, coiled and threaded. Each file holds a closed
    // surface that encloses the part's volume to within what flat triangles lose on curves.
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m6-fittings");
    let mut total = 0.0;
    for (part, material, colour) in [
        ("nozzle", "PLA", "#4F9FD8FF"),
        ("handle", "ABS", "#D04030FF"),
        ("spring", "Stainless steel", "#B4BAC1FF"),
        ("bolt", "Steel, mild", "#8A9199FF"),
    ] {
        e.exec("file.open", &json!({ "path": example.join(format!("{part}.tenon")).to_string_lossy() })).unwrap();
        let exact = e.exec("model.mass", &json!({})).unwrap()["bodies"][0]["volume"].as_f64().unwrap();
        total += exact;
        let r = e.exec("export.3mf", &json!({ "path": format!("{part}.3mf") })).unwrap();
        assert_eq!(r["watertight"], true, "{part}: {r}");
        let (corners, triangles, _, _, text) = read_3mf(&dir.join(format!("{part}.3mf")));
        let (closed, volume) = closed_volume(&corners, &triangles);
        assert!(closed, "{part}.3mf is not a closed surface");
        assert!(near(volume, exact, 0.01) && volume <= exact * 1.0005, "{part}.3mf: {volume} vs {exact}");
        assert!(text.contains(&format!("<base name=\"{material}\" displaycolor=\"{colour}\"/>")), "{part}: its material and colour");
        let r = e.exec("export.stl", &json!({ "path": format!("{part}.stl") })).unwrap();
        assert_eq!(r["watertight"], true, "{part}: {r}");
        let (corners, triangles) = read_stl(&dir.join(format!("{part}.stl")));
        let (closed, stl_volume) = closed_volume(&corners, &triangles);
        assert!(closed && near(stl_volume, volume, 1e-6), "{part}.stl: closed {closed}, {stl_volume} vs {volume}");
        // And the check that reads it as a printer would agrees that it is closed.
        assert_eq!(e.exec("print.check", &json!({})).unwrap()["watertight"]["ok"], true, "{part}");
    }
    // Finer tolerances come closer.
    e.exec("file.open", &json!({ "path": example.join("nozzle.tenon").to_string_lossy() })).unwrap();
    let exact = e.exec("model.mass", &json!({})).unwrap()["bodies"][0]["volume"].as_f64().unwrap();
    let volume_at = |e: &mut Engine, linear: f64| {
        e.exec("export.3mf", &json!({ "path": "fine.3mf", "linear": linear, "angular": 0.05 })).unwrap();
        let (corners, triangles, ..) = read_3mf(&dir.join("fine.3mf"));
        closed_volume(&corners, &triangles).1
    };
    let (coarse, fine) = (volume_at(&mut e, 0.1), volume_at(&mut e, 0.002));
    assert!((exact - fine).abs() < (exact - coarse).abs() && near(fine, exact, 1e-3), "{coarse} {fine} {exact}");

    // The assembly: an object for each part, an item for each component where it is.
    e.exec("asm.open", &json!({ "path": example.join("fittings.tenonasm").to_string_lossy() })).unwrap();
    let r = e.exec("asm.export_3mf", &json!({ "path": "fittings.3mf" })).unwrap();
    assert_eq!((r["objects"].as_u64(), r["items"].as_u64(), r["watertight"].as_bool()), (Some(4), Some(4), Some(true)), "{r}");
    let (_, _, objects, items, text) = read_3mf(&dir.join("fittings.3mf"));
    assert_eq!((objects, items), (4, 4));
    assert!(text.contains("transform=\"1 0 0 0 1 0 0 0 1 50 0 30\""), "the bolt is at (50, 0, 30): {text}");
    assert!(text.contains("<metadata name=\"Title\">Fittings</metadata>"));
    // And as one STL: every solid where it is, enclosing what the four enclose between them.
    let r = e.exec("asm.export_stl", &json!({ "path": "fittings.stl" })).unwrap();
    assert_eq!((r["solids"].as_u64(), r["watertight"].as_bool()), (Some(4), Some(true)), "{r}");
    let (corners, triangles) = read_stl(&dir.join("fittings.stl"));
    let (closed, volume) = closed_volume(&corners, &triangles);
    assert!(closed && near(volume, total, 0.01) && volume < total, "fittings.stl: closed {closed}, {volume} vs {total}");
    let reach = corners.iter().map(|c| c[0]).fold(f64::MIN, f64::max);
    assert!(reach > 50.0, "the bolt is out at x = 50, not at the origin: {reach}");
}

#[test]
fn an_assembly_of_one_part_in_two_sizes_is_two_objects_in_3mf() {
    let dir = scratch("sizes3mf");
    let mut e = Engine::new(kernel(), &dir);
    e.exec("asm.new", &json!({ "name": "Screws" })).unwrap();
    for (size, x) in [("M8x30", 0), ("M8x50", 30), ("M8x30", 60)] {
        e.exec("asm.insert_standard", &json!({ "part": "hex-screw-iso-4017", "size": size, "path": "standard", "at": [x, 0, 0] })).unwrap();
    }
    let r = e.exec("asm.export_3mf", &json!({ "path": "screws.3mf" })).unwrap();
    assert_eq!((r["objects"].as_u64(), r["items"].as_u64(), r["watertight"].as_bool()), (Some(2), Some(3), Some(true)), "{r}");
    let (corners, triangles, _, _, text) = read_3mf(&dir.join("screws.3mf"));
    assert!(text.contains("name=\"Hex Head Screw, M8x30\"") && text.contains("name=\"Hex Head Screw, M8x50\""), "{text}");
    assert!(text.contains("<base name=\"Steel, mild\""));
    // The first object is the M8x30: a head and a shank, joined into one closed surface.
    let (closed, volume) = closed_volume(&corners, &triangles);
    let exact = hexagon(13.0) * 5.3 + disc(8.0) * 30.0;
    assert!(closed && near(volume, exact, 0.01), "{volume} vs {exact}");
}

#[test]
fn the_m7_demo_script_checks_a_printed_cap_and_screws_it_to_a_bar_from_the_library() {
    // The script checks volumes, the print check's verdicts and the assembly as it goes; here
    // we check the files it writes and that they reopen as what they were.
    let dir = scratch("demo");
    let r = tenon_cli::demo_m7(kernel(), &dir).unwrap();
    assert_eq!(r.json["ok"], true, "{}", r.text);

    // The cap for the slicer: one closed surface in each file, enclosing the cap to within what
    // chords lose round its hole and counterbore, in PLA's blue.
    let cap = 24.0 * 24.0 * 10.0 + (24.0 * 24.0 - 20.4 * 20.4) * 5.0 - 25.0 * PI * 6.0 - 2.75 * 2.75 * PI * 4.0;
    let (corners, triangles, objects, items, text) = read_3mf(&dir.join("end-cap.3mf"));
    let (closed, volume) = closed_volume(&corners, &triangles);
    assert!(closed && near(volume, cap, 2e-3) && volume > cap, "{volume} vs {cap}: chords across a hole leave a little more");
    assert_eq!((objects, items), (1, 1));
    assert!(
        text.contains("unit=\"millimeter\"") && text.contains("<base name=\"PLA\" displaycolor=\"#4F9FD8FF\"/>") && text.contains("name=\"End Cap\""),
        "{text}"
    );
    let (corners, triangles) = read_stl(&dir.join("end-cap.stl"));
    let (closed, stl_volume) = closed_volume(&corners, &triangles);
    assert!(closed && near(stl_volume, volume, 1e-6), "{stl_volume} vs {volume}");

    // The assembly for the slicer: three objects, each where the relationships put it.
    let (_, _, objects, items, text) = read_3mf(&dir.join("capped-bar.3mf"));
    assert_eq!((objects, items), (3, 3));
    for shown in [
        "<metadata name=\"Title\">Capped Bar</metadata>",
        "<base name=\"Aluminium 6061\"",
        "<base name=\"PLA\"",
        "<base name=\"Steel, mild\"",
        "name=\"T-Slot Profile\"",
        "name=\"Socket Head Cap Screw, M5x16\"",
        "transform=\"1 0 0 0 1 0 0 0 1 0 0 150\"",
        "transform=\"1 0 0 0 1 0 0 0 1 0 0 154\"",
    ] {
        assert!(text.contains(shown), "{shown}: {text}");
    }
    let csv = std::fs::read_to_string(dir.join("capped-bar-bom.csv")).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[1], "1,bar,T-Slot Profile,1,37041.836,Aluminium 6061,100.013", "{csv}");
    assert_eq!(lines[2], "2,end-cap,End Cap,1,5992.928,PLA,7.431", "{csv}");
    assert_eq!(lines[3], "3,socket-screw-iso-4762 (M5x16),\"Socket Head Cap Screw, M5x16\",1,563.243,\"Steel, mild\",4.421", "{csv}");
    for png in ["bar.png", "end-cap.png", "capped-bar.png"] {
        assert!(std::fs::read(dir.join(png)).unwrap().starts_with(&[0x89, b'P', b'N', b'G']), "{png}");
    }

    // The saved assembly reopens with its screw from the folder beside it, in the size it had,
    // and held as it was.
    assert!(dir.join("standard").join("socket-screw-iso-4762.tenon").is_file());
    let mut e = Engine::new(kernel(), &dir);
    let opened = e.exec("asm.open", &json!({ "path": "capped-bar.tenonasm" })).unwrap();
    assert_eq!(opened["missing"].as_array().map(Vec::len), Some(0), "{opened}");
    let tree = e.exec("asm.tree", &json!({})).unwrap();
    assert_eq!((tree["dof"].as_u64(), tree["components"][2]["row"].as_str()), (Some(1), Some("M5x16")), "{tree}");
    assert!(tree["relationships"].as_array().unwrap().iter().all(|r| r["failing"].is_null()), "{tree}");
    let clash = e.exec("asm.interference", &json!({})).unwrap();
    assert!(clash["count"] == 1 && near(clash["volume"].as_f64().unwrap(), PI * (2.5 * 2.5 - 2.1 * 2.1) * 12.0, 1e-9), "{clash}");
    // The bar is a part of its own, started from the library: its length is its parameter, and
    // the other sizes of the section are still in its table.
    e.exec("file.open", &json!({ "path": "bar.tenon" })).unwrap();
    let t = e.exec("table.show", &json!({})).unwrap();
    assert_eq!((t["table"]["active"].as_str(), t["table"]["rows"].as_array().map(Vec::len)), (Some("20x20 slot 6"), Some(3)), "{t}");
    let section = 400.0 - 4.0 * (6.0 * 1.8 + 8.0 * 3.0) - 2.1 * 2.1 * PI;
    let m = e.exec("model.mass", &json!({})).unwrap();
    assert!(near(m["bodies"][0]["volume"].as_f64().unwrap(), section * 150.0, 1e-9), "{m}");
    // The cap reopens and prints as it did: best with its flat top on the bed.
    e.exec("file.open", &json!({ "path": "end-cap.tenon" })).unwrap();
    let (up, down) = (e.exec("print.check", &json!({})).unwrap(), e.exec("print.check", &json!({ "up": "-z" })).unwrap());
    assert!(up["overhangs"]["area"].as_f64().unwrap() > 7.0 * down["overhangs"]["area"].as_f64().unwrap(), "{up} {down}");
}

#[test]
fn the_checked_in_m7_example_is_what_the_script_writes() {
    // Compared as `tenon-cli diff` compares files (see the M6 example's test for why not byte
    // for byte).
    let dir = scratch("same");
    tenon_cli::demo_m7(kernel(), &dir).unwrap();
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m7-end-cap");
    for f in ["bar.tenon", "end-cap.tenon", "standard/socket-screw-iso-4762.tenon", "capped-bar.tenonasm"] {
        let d = tenon_io::diff::diff_files(&example.join(f), &dir.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert!(d.same(), "{f}: regenerate the example (see its README):\n{}", d.text());
    }
    // The screw's file is the library's part as the library makes it today.
    let fresh = dir.join("fresh.tenon");
    let mut e = Engine::new(kernel(), &dir);
    e.exec("library.new", &json!({ "part": "socket-screw-iso-4762", "size": "M8x30" })).unwrap();
    e.exec("file.save", &json!({ "path": "fresh.tenon" })).unwrap();
    let d = tenon_io::diff::diff_files(&example.join("standard/socket-screw-iso-4762.tenon"), &fresh).unwrap();
    assert!(d.same(), "{}", d.text());
}
