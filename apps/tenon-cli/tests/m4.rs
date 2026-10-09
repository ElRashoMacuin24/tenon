//! M4 demo: the plate drawing script (views, sections, details, dimensions picked from the views,
//! a hole table, an assembly sheet with a parts list and balloons, PDF/SVG/DXF, associativity)
//! and the files it writes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tenon_cli::Engine;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;

fn kernel() -> Box<dyn Kernel> {
    Box::new(OcctKernel::new())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-cli-m4-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn demo(name: &str) -> PathBuf {
    let dir = scratch(name);
    let r = tenon_cli::demo_m4(kernel(), &dir).unwrap();
    assert_eq!(r.json["ok"], true, "{}", r.text);
    dir
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn copy(from: &Path, to: &Path, files: &[&str]) {
    for f in files {
        std::fs::copy(from.join(f), to.join(f)).unwrap();
    }
}

#[test]
fn the_m4_demo_script_draws_a_plate_and_its_assembly() {
    // The script checks every view's direction and size, the dimension values, the hole table,
    // the parts list and balloons, and that the drawing follows the plate when it is made
    // thicker; here we check the files it writes and that the drawing reopens elsewhere.
    let dir = demo("plate");

    // PDF: two pages, the title block's text in it.
    let pdf = std::fs::read(dir.join("plate.pdf")).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    let text = String::from_utf8_lossy(&pdf);
    assert_eq!(text.matches("/Type /Page").count() - text.matches("/Type /Pages").count(), 2);
    assert!(text.contains("(MOUNTING PLATE)"));

    // SVG of sheet 1 (written while the plate was 12 thick).
    let svg = std::fs::read_to_string(dir.join("plate.svg")).unwrap();
    assert!(svg.contains("<svg") && svg.trim_end().ends_with("</svg>"));
    for t in ["SECTION A-A", "DETAIL B", "SCALE 2:1", "Ø18", "CBORE"] {
        assert!(svg.contains(t), "{t} not in the SVG");
    }

    // DXF read back: a layer per pen, the views' lines on them, the title as text.
    let dxf = std::fs::read(dir.join("plate.dxf")).unwrap();
    let tags = tenon_dxf::parse(&dxf).unwrap();
    let entities = tenon_dxf::sections(&tags).into_iter().find(|s| s.name == "ENTITIES").unwrap();
    let records = tenon_dxf::records(&entities.tags);
    let layer = |r: &[tenon_dxf::Tag]| r.iter().find(|t| t.code == 8).map(tenon_dxf::Tag::str).unwrap_or_default();
    let coord = |r: &[tenon_dxf::Tag], code: i32| r.iter().find(|t| t.code == code).map_or(f64::NAN, tenon_dxf::Tag::f64);
    let lines: Vec<&(String, Vec<tenon_dxf::Tag>)> = records.iter().filter(|(k, _)| k == "LINE").collect();
    assert!(lines.len() > 500, "{} lines", lines.len());
    for pen in ["VISIBLE", "HIDDEN", "CENTER", "CUTTING", "HATCH", "THIN", "BORDER"] {
        assert!(lines.iter().any(|(_, r)| layer(r) == pen), "nothing on layer {pen}");
    }
    // The top view's back edge: a visible line 120 long at y = 176, from x = 40.
    let back: Vec<[f64; 2]> = lines
        .iter()
        .filter(|(_, r)| layer(r) == "VISIBLE" && (coord(r, 20) - 176.0).abs() < 1e-6 && (coord(r, 21) - 176.0).abs() < 1e-6)
        .map(|(_, r)| [coord(r, 10).min(coord(r, 11)), coord(r, 10).max(coord(r, 11))])
        .collect();
    let lo = back.iter().map(|s| s[0]).fold(f64::MAX, f64::min);
    let hi = back.iter().map(|s| s[1]).fold(f64::MIN, f64::max);
    assert!((lo - 40.0).abs() < 1e-6 && (hi - 160.0).abs() < 1e-6, "{back:?}");
    assert!(records.iter().any(|(k, r)| k == "TEXT" && r.iter().any(|t| t.code == 1 && t.str() == "MOUNTING PLATE")));

    for png in ["plate-sheet1.png", "plate-sheet2.png"] {
        assert!(std::fs::read(dir.join(png)).unwrap().starts_with(&[0x89, b'P', b'N', b'G']), "{png}");
    }

    // Model paths are stored relative to the drawing: the folder can move.
    let moved = scratch("moved");
    copy(&dir, &moved, &["plate.tenondrw", "plate.tenon", "pin.tenon", "plate-pins.tenonasm"]);
    let mut e = Engine::new(kernel(), &moved);
    let opened = e.exec("drw.open", &json!({ "path": "plate.tenondrw" })).unwrap();
    assert_eq!(opened["missing"], json!([]), "{opened}");
    let info = e.exec("drw.info", &json!({})).unwrap();
    assert!(info["views"].as_array().unwrap().iter().all(|v| v["error"].is_null()), "{info}");
    assert!(info["views"][0]["model"].as_str().unwrap().starts_with(moved.to_str().unwrap()));
    assert!((num(&info["annotations"][3]["value"]) - 16.0).abs() < 1e-9);
    assert_eq!(info["annotations"][4]["rows"].as_array().unwrap().len(), 5);

    // Without its models the drawing still opens and exports, the views reporting why they
    // are empty.
    let lonely = scratch("lonely");
    copy(&dir, &lonely, &["plate.tenondrw"]);
    let mut e = Engine::new(kernel(), &lonely);
    let opened = e.exec("drw.open", &json!({ "path": "plate.tenondrw" })).unwrap();
    assert_eq!(opened["missing"].as_array().unwrap().len(), 2, "{opened}");
    let info = e.exec("drw.info", &json!({})).unwrap();
    for (i, file) in [(0, "plate.tenon"), (6, "plate-pins.tenonasm")] {
        let err = info["views"][i]["error"].as_str().unwrap_or_default();
        assert!(err.starts_with(&format!("{file} could not be read")), "{err}");
    }
    let problem = info["annotations"][3]["problem"].as_str().unwrap_or_default();
    assert!(problem.starts_with("plate.tenon could not be read"), "{problem}");
    let r = e.exec("drw.export.pdf", &json!({ "path": "plate.pdf" })).unwrap();
    assert_eq!(r["pages"], 2);
}

#[test]
fn drawing_edits_undo_and_bad_input_is_refused() {
    let dir = demo("edits");
    let mut e = Engine::new(kernel(), &dir);
    e.exec("drw.open", &json!({ "path": "plate.tenondrw" })).unwrap();
    let info = |e: &mut Engine| e.exec("drw.info", &json!({})).unwrap();
    let center = |i: &Value, v: usize| [num(&i["views"][v]["center"][0]), num(&i["views"][v]["center"][1])];
    let before = info(&mut e);

    // Moving the front view takes the views projected from it along (they stay lined up); the
    // section and detail stay where they are. Undo and redo.
    e.exec("drw.view.edit", &json!({ "view": 1, "by": [10, 0] })).unwrap();
    let moved = info(&mut e);
    for v in [0, 1, 2, 3] {
        assert_eq!(center(&moved, v), [center(&before, v)[0] + 10.0, center(&before, v)[1]], "view {v}");
    }
    for v in [4, 5, 6] {
        assert_eq!(center(&moved, v), center(&before, v), "view {v}");
    }
    assert!((num(&moved["annotations"][0]["value"]) - 120.0).abs() < 1e-9);
    e.exec("drw.undo", &json!({})).unwrap();
    assert_eq!(center(&info(&mut e), 1), center(&before, 1));
    e.exec("drw.redo", &json!({})).unwrap();
    assert_eq!(center(&info(&mut e), 1), center(&moved, 1));
    e.exec("drw.undo", &json!({})).unwrap();
    // The top view, projected above the front view, only slides up and down in line with it;
    // the right view only sideways.
    e.exec("drw.view.edit", &json!({ "view": 2, "by": [10, 5] })).unwrap();
    e.exec("drw.view.edit", &json!({ "view": 3, "by": [10, 5] })).unwrap();
    let slid = info(&mut e);
    assert_eq!(center(&slid, 1), [center(&before, 1)[0], center(&before, 1)[1] + 5.0]);
    assert_eq!(center(&slid, 2), [center(&before, 2)[0] + 10.0, center(&before, 2)[1]]);
    e.exec("drw.undo", &json!({})).unwrap();
    e.exec("drw.undo", &json!({})).unwrap();

    // Deleting the top view takes its section and the annotations on it.
    e.exec("drw.view.delete", &json!({ "view": 2 })).unwrap();
    let after = info(&mut e);
    assert_eq!(after["views"].as_array().unwrap().len(), 5);
    assert!(after["views"].as_array().unwrap().iter().all(|v| v["name"] != "A" && v["id"] != 2));
    assert_eq!(after["annotations"].as_array().unwrap().len(), 5, "{after}");
    e.exec("drw.undo", &json!({})).unwrap();
    assert_eq!(info(&mut e)["annotations"], before["annotations"]);

    // Refused, changing nothing.
    let n = |e: &mut Engine| e.exec("drw.info", &json!({})).unwrap()["annotations"].as_array().unwrap().len();
    let count = n(&mut e);
    let line = e.exec("drw.pick", &json!({ "view": 2, "view_at": [60, 80] })).unwrap();
    assert_eq!(line["kind"], "line");
    let err = e.exec("drw.dimension", &json!({ "view": 2, "type": "diameter", "a": line, "by": [0, 10] })).unwrap_err();
    assert!(err.contains("circular"), "{err}");
    assert!(e.exec("drw.dimension", &json!({ "view": 2, "type": "horizontal", "a": line })).unwrap_err().contains("by"));
    assert!(e.exec("drw.pick", &json!({ "view": 2, "at": [400, 20] })).unwrap_err().contains("no edge"));
    assert!(e.exec("drw.view.section", &json!({ "parent": 2, "a": [5, 5], "b": [5, 5] })).is_err());
    assert!(e.exec("drw.view.detail", &json!({ "parent": 1, "center": [0, 0], "radius": -1 })).is_err());
    assert!(e.exec("drw.view.projected", &json!({ "parent": 1, "side": "sideways" })).is_err());
    assert!(e.exec("drw.view.projected", &json!({ "parent": 99, "side": "right" })).is_err());
    assert!(e.exec("drw.parts_list", &json!({ "view": 1 })).unwrap_err().contains("assembly"));
    assert!(e.exec("drw.hole_table", &json!({ "view": 1 })).unwrap_err().contains("no holes"));
    assert!(e.exec("drw.hole_table", &json!({ "view": 7 })).unwrap_err().contains("part views"));
    assert!(e.exec("drw.view.edit", &json!({ "view": 1, "scale": "1:0" })).is_err());
    assert_eq!(n(&mut e), count);
    assert_eq!(info(&mut e)["views"], before["views"]);
    let err = e.exec("drw.open", &json!({ "path": "plate.tenon" })).unwrap_err();
    assert!(err.contains("part"), "{err}");
    assert!(e.exec("drw.export.pdf", &json!({ "path": "no/such/folder/plate.pdf" })).is_err());

    // New views go clear of their parent: the plate is 16 thick now, so the top view (80 deep)
    // goes 8 + 40 + 20 above the front view's centre, the right view (80 wide) 60 + 40 + 20 right.
    e.exec("drw.new", &json!({ "name": "Layout" })).unwrap();
    let front = e.exec("drw.view.base", &json!({ "model": "plate.tenon", "scale": 1, "at": [100, 70] })).unwrap();
    assert_eq!(front["scale"], 1.0);
    e.exec("drw.view.projected", &json!({ "parent": 1, "side": "above" })).unwrap();
    e.exec("drw.view.projected", &json!({ "parent": 1, "side": "right" })).unwrap();
    let i = info(&mut e);
    assert_eq!(center(&i, 1), [100.0, 138.0]);
    assert_eq!(center(&i, 2), [220.0, 70.0]);
    // With no scale given, the largest standard scale that fits the sheet.
    let fit = e.exec("drw.view.base", &json!({ "model": "plate.tenon", "orientation": "top" })).unwrap();
    assert_eq!(fit["scale"], 1.0);
    e.exec("drw.sheet.size", &json!({ "sheet": 1, "size": "A" })).unwrap();
    let fit = e.exec("drw.view.base", &json!({ "model": "plate.tenon", "orientation": "top" })).unwrap();
    assert_eq!(fit["scale"], 0.5, "{fit}");
}
