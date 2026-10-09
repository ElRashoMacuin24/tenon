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

    // PDF: read back as a reader does (cross-references, stream lengths, page tree): two pages,
    // with the title block's words in them (drawn in the drafting font, kept as invisible text).
    let pdf = std::fs::read(dir.join("plate.pdf")).unwrap();
    assert_eq!(tenon_drawing::export::check_pdf(&pdf), Ok(2));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("(MOUNTING PLATE)") && text.contains("BT 3 Tr"));

    // SVG of sheet 1 (written while the plate was 12 thick), read by an XML parser: the holes
    // are circles.
    let svg = std::fs::read_to_string(dir.join("plate.svg")).unwrap();
    let mut circles = 0;
    let mut reader = quick_xml::Reader::from_str(&svg);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Empty(e)) if e.name().as_ref() == b"circle" => circles += 1,
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => panic!("plate.svg is not well-formed XML: {e}"),
            _ => {}
        }
    }
    assert!(circles >= 8, "{circles} circles");
    for t in ["SECTION A-A", "DETAIL B", "SCALE 2:1", "Ø18", "CBORE"] {
        assert!(svg.contains(t), "{t} not in the SVG");
    }

    // DXF read back: valid structure, a layer per pen, the holes as circles, curves joined, so
    // a few hundred entities where there were over a thousand segments; the title as text.
    let dxf = std::fs::read(dir.join("plate.dxf")).unwrap();
    let stats = tenon_drawing::export::check_dxf(&String::from_utf8_lossy(&dxf)).unwrap();
    assert!(stats.circles >= 8 && stats.lines + stats.polylines + stats.arcs + stats.circles < 600, "{stats:?}");
    let tags = tenon_dxf::parse(&dxf).unwrap();
    let entities = tenon_dxf::sections(&tags).into_iter().find(|s| s.name == "ENTITIES").unwrap();
    let records = tenon_dxf::records(&entities.tags);
    let layer = |r: &[tenon_dxf::Tag]| r.iter().find(|t| t.code == 8).map(tenon_dxf::Tag::str).unwrap_or_default();
    let coord = |r: &[tenon_dxf::Tag], code: i32| r.iter().find(|t| t.code == code).map_or(f64::NAN, tenon_dxf::Tag::f64);
    for pen in ["VISIBLE", "HIDDEN", "CENTER", "CUTTING", "HATCH", "THIN", "BORDER"] {
        assert!(records.iter().any(|(k, r)| k != "TEXT" && layer(r) == pen), "nothing on layer {pen}");
    }
    let lines: Vec<&(String, Vec<tenon_dxf::Tag>)> = records.iter().filter(|(k, _)| k == "LINE").collect();
    // Centre marks: each hole of the top view (centred at (100, 136), 1:1) has a horizontal and a
    // vertical centre line crossing at its centre.
    let centre_lines: Vec<(f64, f64, f64, f64)> =
        lines.iter().filter(|(_, r)| layer(r) == "CENTER").map(|(_, r)| (coord(r, 10), coord(r, 20), coord(r, 11), coord(r, 21))).collect();
    for (x, y) in [(15.0, 15.0), (105.0, 15.0), (105.0, 65.0), (15.0, 65.0), (60.0, 40.0)] {
        let c = (100.0 + x - 60.0, 136.0 + y - 40.0);
        let through = |horizontal: bool| {
            centre_lines.iter().any(|(x0, y0, x1, y1)| {
                let mid = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                (mid.0 - c.0).abs() < 1e-6 && (mid.1 - c.1).abs() < 1e-6 && if horizontal { (y0 - y1).abs() < 1e-9 } else { (x0 - x1).abs() < 1e-9 }
            })
        };
        assert!(through(true) && through(false), "no centre mark at {c:?}");
    }
    // The counterbore seen from above: a circle 18 across on the top view, at (100, 136).
    assert!(records.iter().any(|(k, r)| k == "CIRCLE"
        && layer(r) == "VISIBLE"
        && (coord(r, 40) - 9.0).abs() < 1e-6
        && (coord(r, 10) - 100.0).abs() < 1e-6
        && (coord(r, 20) - 136.0).abs() < 1e-6));
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

/// A 60 x 40 x 20 block with its back right vertical edge filleted (r) and its top front edge
/// chamfered (c), both user parameters, saved as `block.tenon`.
fn filleted_chamfered_block(e: &mut Engine) {
    let run = |e: &mut Engine, id: &str, p: Value| e.exec(id, &p).unwrap_or_else(|err| panic!("{id}: {err}"));
    run(e, "file.new", json!({ "name": "Block" }));
    run(e, "param.add", json!({ "name": "r", "equation": "10 mm" }));
    run(e, "param.add", json!({ "name": "c", "equation": "5 mm" }));
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    let rect = run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 40 }));
    let body = run(e, "model.extrude", json!({ "sketch": sk, "distance": 20 }))["feature"].clone();
    let side = |i: usize| json!({ "type": "side", "feature": body, "curve": rect["lines"][i] });
    let corner = run(e, "model.edge_ref", json!({ "faces": [side(1), side(2)] }));
    run(e, "model.fillet", json!({ "edges": [corner], "radius": 10, "equations": { "/radius": "r" } }));
    let top_front = run(e, "model.edge_ref", json!({ "faces": [{ "type": "cap", "feature": body, "end": "end" }, side(0)] }));
    run(e, "model.chamfer", json!({ "edges": [top_front], "distance": 5, "equations": { "/size/equal": "c" } }));
    run(e, "file.save", json!({ "path": "block.tenon" }));
}

#[test]
fn aligned_radius_and_angle_dimensions_measure_the_model_and_follow_it() {
    let dir = scratch("dims");
    let mut e = Engine::new(kernel(), &dir);
    filleted_chamfered_block(&mut e);
    let run = |e: &mut Engine, id: &str, p: Value| e.exec(id, &p).unwrap_or_else(|err| panic!("{id}: {err}"));
    run(&mut e, "drw.new", json!({ "name": "Dims" }));
    let top = run(&mut e, "drw.view.base", json!({ "model": "block.tenon", "orientation": "top", "scale": 1, "at": [100, 150] }))["view"].clone();
    let right = run(&mut e, "drw.view.base", json!({ "model": "block.tenon", "orientation": "right", "scale": 2, "at": [280, 150] }))["view"].clone();

    // The fillet seen from above is a quarter circle: its radius.
    let arc = run(&mut e, "drw.pick", json!({ "view": top, "view_at": [50.0 + 10.0 * 0.5f64.sqrt(), 30.0 + 10.0 * 0.5f64.sqrt()] }));
    assert_eq!((arc["kind"].as_str(), arc["diameter"].as_f64()), (Some("circle"), Some(20.0)), "{arc}");
    let radius = run(&mut e, "drw.dimension", json!({ "view": top, "type": "radius", "a": arc, "by": [12, 12] }));
    assert_eq!((radius["value"].as_f64(), radius["shown"].as_str()), (Some(10.0), Some("R10")));

    // From the right (Y across, Z up) the chamfer is a 45-degree line from (0, 15) to (5, 20):
    // its true length 5 √2, and its angle to the top edge.
    let chamfer = run(&mut e, "drw.pick", json!({ "view": right, "view_at": [2.5, 17.5] }));
    assert!((chamfer["length"].as_f64().unwrap() - 5.0 * 2f64.sqrt()).abs() < 1e-9, "{chamfer}");
    let aligned = run(&mut e, "drw.dimension", json!({ "view": right, "type": "aligned", "a": chamfer, "by": [-8, 8] }));
    assert!((aligned["value"].as_f64().unwrap() - 5.0 * 2f64.sqrt()).abs() < 1e-9, "{aligned}");
    assert_eq!(aligned["shown"], "7.07");
    let top_edge = run(&mut e, "drw.pick", json!({ "view": right, "view_at": [15.0, 20.0] }));
    assert_eq!(top_edge["kind"], "line");
    // The lines meet at (5, 20) and make four sectors. Placed between the top edge (rightwards)
    // and the chamfer's extension (up and right): 45 degrees. Straight above the meeting point,
    // between that extension and the top edge's extension (leftwards): 135. Below and to the
    // right, inside the part's corner: 135.
    let angle = run(&mut e, "drw.dimension", json!({ "view": right, "type": "angle", "a": chamfer, "b": top_edge, "by": [12, 5] }));
    assert!((angle["value"].as_f64().unwrap() - 45.0).abs() < 1e-9, "{angle}");
    assert_eq!(angle["shown"], "45°");
    let above = run(&mut e, "drw.dimension", json!({ "view": right, "type": "angle", "a": chamfer, "b": top_edge, "by": [0, 12] }));
    assert!((above["value"].as_f64().unwrap() - 135.0).abs() < 1e-9, "{above}");
    run(&mut e, "drw.undo", json!({}));
    let obtuse = run(&mut e, "drw.dimension", json!({ "view": right, "type": "angle", "a": chamfer, "b": top_edge, "by": [8, -6] }));
    assert!((obtuse["value"].as_f64().unwrap() - 135.0).abs() < 1e-9, "{obtuse}");

    // They follow the model: r = 14, c = 3.
    run(&mut e, "file.open", json!({ "path": "block.tenon" }));
    run(&mut e, "param.set", json!({ "name": "r", "equation": "14" }));
    run(&mut e, "param.set", json!({ "name": "c", "equation": "3" }));
    run(&mut e, "file.save", json!({ "path": "block.tenon" }));
    run(&mut e, "drw.update", json!({}));
    let info = run(&mut e, "drw.info", json!({}));
    let value = |i: usize| info["annotations"][i]["value"].as_f64().unwrap_or(f64::NAN);
    assert!((value(0) - 14.0).abs() < 1e-9, "{}", info["annotations"][0]);
    assert!((value(1) - 3.0 * 2f64.sqrt()).abs() < 1e-9, "{}", info["annotations"][1]);
    assert!((value(2) - 45.0).abs() < 1e-9 && (value(3) - 135.0).abs() < 1e-9);
}

#[test]
fn centre_marks_and_centrelines_placed_by_hand_follow_the_model() {
    let dir = scratch("centres");
    let mut e = Engine::new(kernel(), &dir);
    filleted_chamfered_block(&mut e);
    let run = |e: &mut Engine, id: &str, p: Value| e.exec(id, &p).unwrap_or_else(|err| panic!("{id}: {err}"));
    run(&mut e, "drw.new", json!({ "name": "Centres" }));
    // The top view at 1:1, its middle (30, 20) at (100, 150) on the sheet.
    let top = run(&mut e, "drw.view.base", json!({ "model": "block.tenon", "orientation": "top", "scale": 1, "at": [100, 150] }))["view"].clone();
    let right = run(&mut e, "drw.view.base", json!({ "model": "block.tenon", "orientation": "right", "scale": 2, "at": [280, 150] }))["view"].clone();
    let sheet = |x: f64, y: f64| (100.0 + x - 30.0, 150.0 + y - 20.0);
    let seg = |v: &Value, i: usize| -> [(f64, f64); 2] {
        let p = |k: usize| (v[i][k][0].as_f64().unwrap(), v[i][k][1].as_f64().unwrap());
        [p(0), p(1)]
    };
    let close = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;

    // A centre mark on the rounded corner: a cross at the arc's centre (50, 30), arms 10 + 2.
    let arc = run(&mut e, "drw.pick", json!({ "view": top, "view_at": [50.0 + 10.0 * 0.5f64.sqrt(), 30.0 + 10.0 * 0.5f64.sqrt()] }));
    let mark = run(&mut e, "drw.center_mark", json!({ "view": top, "a": arc.clone() }));
    let c = sheet(50.0, 30.0);
    let s = seg(&mark["segments"], 0);
    assert!(close(s[0], (c.0 - 12.0, c.1)) && close(s[1], (c.0 + 12.0, c.1)), "{mark}");
    // The block's centre line between its parallel left and right sides: x = 30, along both (from
    // y = 5, where the chamfer starts, to 40).
    let (left, right_side) =
        (run(&mut e, "drw.pick", json!({ "view": top, "view_at": [0, 20] })), run(&mut e, "drw.pick", json!({ "view": top, "view_at": [60, 15] })));
    let mid = run(&mut e, "drw.centerline.bisector", json!({ "view": top, "a": left.clone(), "b": right_side }));
    let s = seg(&mid["segments"], 0);
    let (lo, hi) = (sheet(30.0, 5.0), sheet(30.0, 40.0));
    let ends = |s: [(f64, f64); 2]| {
        (close(s[0], (lo.0, lo.1 - 2.0)) && close(s[1], (hi.0, hi.1 + 2.0))) || (close(s[1], (lo.0, lo.1 - 2.0)) && close(s[0], (hi.0, hi.1 + 2.0)))
    };
    assert!(ends(s), "{mid}");
    // A centre line from the arc's centre to the middle of the left side (y 5 to 40), a little
    // past both.
    let mut centre = arc.clone();
    centre["point"] = json!("center");
    let line = run(&mut e, "drw.centerline", json!({ "view": top, "a": centre, "b": left }));
    let s = seg(&line["segments"], 0);
    let (p, q) = (sheet(50.0, 30.0), sheet(0.0, 22.5));
    let len = ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt();
    let s_len = ((s[0].0 - s[1].0).powi(2) + (s[0].1 - s[1].1).powi(2)).sqrt();
    assert!((s_len - len - 4.0).abs() < 1e-6, "{line}");
    // Between the chamfer and the top edge, which meet: the bisector of the 135-degree corner.
    let (chamfer, top_edge) = (
        run(&mut e, "drw.pick", json!({ "view": right, "view_at": [2.5, 17.5] })),
        run(&mut e, "drw.pick", json!({ "view": right, "view_at": [15.0, 20.0] })),
    );
    let corner = run(&mut e, "drw.centerline.bisector", json!({ "view": right, "a": chamfer.clone(), "b": top_edge }));
    let s = seg(&corner["segments"], 0);
    let angle = (s[1].1 - s[0].1).atan2(s[1].0 - s[0].0).to_degrees();
    assert!((angle + 67.5).abs() < 1e-6, "{angle}");

    // Refused: a centre mark on a line, a centre line between two places that are one, moving one.
    assert!(e.exec("drw.center_mark", &json!({ "view": right, "a": chamfer.clone() })).unwrap_err().contains("circle"));
    assert!(e.exec("drw.centerline", &json!({ "view": right, "a": chamfer.clone(), "b": chamfer })).unwrap_err().contains("same"));
    let id = mark["annotation"].clone();
    assert!(e.exec("drw.annotation.edit", &json!({ "annotation": id, "by": [5, 0] })).unwrap_err().contains("geometry"));

    // Saved and reopened, then the round made 14: the mark follows the arc's centre to (46, 26);
    // the sides' centre line stays at x = 30.
    run(&mut e, "drw.save", json!({ "path": "centres.tenondrw" }));
    run(&mut e, "file.open", json!({ "path": "block.tenon" }));
    run(&mut e, "param.set", json!({ "name": "r", "equation": "14" }));
    run(&mut e, "file.save", json!({ "path": "block.tenon" }));
    run(&mut e, "drw.open", json!({ "path": "centres.tenondrw" }));
    let info = run(&mut e, "drw.info", json!({}));
    let kinds: Vec<&str> = info["annotations"].as_array().unwrap().iter().map(|a| a["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["center_mark", "centerline_bisector", "centerline", "centerline_bisector"]);
    let c = sheet(46.0, 26.0);
    let s = seg(&info["annotations"][0]["segments"], 0);
    assert!(close(s[0], (c.0 - 16.0, c.1)) && close(s[1], (c.0 + 16.0, c.1)), "{}", info["annotations"][0]);
    assert!(ends(seg(&info["annotations"][1]["segments"], 0)), "{}", info["annotations"][1]);
    // In the DXF, on the CENTER layer.
    run(&mut e, "drw.export.dxf", json!({ "path": "centres.dxf" }));
    let dxf = std::fs::read_to_string(dir.join("centres.dxf")).unwrap();
    assert!(tenon_drawing::export::check_dxf(&dxf).is_ok());
}

#[test]
fn suggested_dimensions_cover_the_part_once_and_follow_it() {
    let dir = scratch("suggest");
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m4-plate");
    std::fs::copy(src.join("plate.tenon"), dir.join("plate.tenon")).unwrap();
    let mut e = Engine::new(kernel(), &dir);
    let run = |e: &mut Engine, id: &str, p: Value| e.exec(id, &p).unwrap_or_else(|err| panic!("{id}: {err}"));
    run(&mut e, "drw.new", json!({ "name": "Plate" }));
    let front = run(&mut e, "drw.view.base", json!({ "model": "plate.tenon", "scale": 1, "at": [110, 70] }))["view"].clone();
    let top = run(&mut e, "drw.view.projected", json!({ "parent": front, "side": "above" }))["view"].clone();
    let iso = run(&mut e, "drw.view.projected", json!({ "parent": front, "side": "above_right", "scale": 0.5 }))["view"].clone();
    let shown = |r: &Value| -> Vec<String> {
        r["suggestions"].as_array().unwrap().iter().map(|s| format!("{} {}", s["type"].as_str().unwrap(), s["shown"].as_str().unwrap())).collect()
    };

    // The top view: 120 x 80 overall, four 8 mm holes as one, the counterbore and its hole.
    let r = run(&mut e, "drw.dimension.suggest", json!({ "view": top }));
    assert_eq!(shown(&r), ["horizontal 120", "vertical 80", "diameter Ø18", "diameter Ø10", "diameter 4X Ø8"], "{r}");
    // Each outside the view or beside its circle, not on top of the part.
    let at = |s: &Value| (s["at"][0].as_f64().unwrap(), s["at"][1].as_f64().unwrap());
    let sg = r["suggestions"].as_array().unwrap();
    assert!(at(&sg[0]).1 < 136.0 - 40.0 && at(&sg[1]).0 > 110.0 + 60.0, "{r}");
    // The front view: the plate's width and its thickness (t = 16).
    let r = run(&mut e, "drw.dimension.suggest", json!({ "view": front }));
    assert_eq!(shown(&r), ["horizontal 120", "vertical 16"], "{r}");
    // An isometric view is not dimensioned.
    assert!(e.exec("drw.dimension.suggest", &json!({ "view": iso })).unwrap_err().contains("isometric"));

    // Add all of the top view's but the 4X Ø8, all of the front view's; each is one undo step.
    let added = run(&mut e, "drw.dimension.auto", json!({ "view": top, "accept": [0, 1, 2, 3] }));
    assert_eq!(added["added"], 4);
    run(&mut e, "drw.dimension.auto", json!({ "view": front }));
    run(&mut e, "drw.undo", json!({}));
    run(&mut e, "drw.dimension.auto", json!({ "view": front }));
    let info = run(&mut e, "drw.info", json!({}));
    assert_eq!(info["annotations"].as_array().unwrap().len(), 6);
    assert_eq!(info["annotations"][3]["shown"], "Ø10");
    // What is there is not suggested again; the 4X Ø8 left out still is.
    assert_eq!(shown(&run(&mut e, "drw.dimension.suggest", json!({ "view": top }))), ["diameter 4X Ø8"]);
    assert!(shown(&run(&mut e, "drw.dimension.suggest", json!({ "view": front }))).is_empty());
    assert!(e.exec("drw.dimension.auto", &json!({ "view": top, "accept": [3] })).unwrap_err().contains("accept"));

    // They are ordinary dimensions: the plate made 20 thick, the thickness reads 20.
    run(&mut e, "file.open", json!({ "path": "plate.tenon" }));
    run(&mut e, "param.set", json!({ "name": "t", "equation": "20" }));
    run(&mut e, "file.save", json!({ "path": "plate.tenon" }));
    run(&mut e, "drw.update", json!({}));
    let info = run(&mut e, "drw.info", json!({}));
    assert_eq!(info["annotations"][5]["shown"], "20", "{}", info["annotations"][5]);
}

#[test]
fn assembly_views_hide_one_part_behind_another_and_sections_cut_every_part() {
    let dir = scratch("occlusion");
    let mut e = Engine::new(kernel(), &dir);
    let run = |e: &mut Engine, id: &str, p: Value| e.exec(id, &p).unwrap_or_else(|err| panic!("{id}: {err}"));
    // Two boxes: a 40 x 10 x 40 wall at the front, and a 20 x 10 x 20 block 30 behind it, in
    // the middle of the wall as seen from the front.
    for (name, x, y, z) in [("wall", 40, 10, 40), ("block", 20, 10, 20)] {
        run(&mut e, "file.new", json!({ "name": name }));
        let sk = run(&mut e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
        run(&mut e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": x, "y2": y }));
        run(&mut e, "model.extrude", json!({ "sketch": sk, "distance": z }));
        run(&mut e, "file.save", json!({ "path": format!("{name}.tenon") }));
    }
    run(&mut e, "asm.new", json!({ "name": "Pair" }));
    run(&mut e, "asm.insert", json!({ "path": "wall.tenon" }));
    let block = run(&mut e, "asm.insert", json!({ "path": "block.tenon" }))["component"].clone();
    run(&mut e, "asm.place", json!({ "component": block, "origin": [10, 30, 10] }));
    run(&mut e, "asm.save", json!({ "path": "pair.tenonasm" }));

    run(&mut e, "drw.new", json!({ "name": "Pair" }));
    let front =
        run(&mut e, "drw.view.base", json!({ "model": "pair.tenonasm", "orientation": "front", "scale": 1, "at": [100, 100] }))["view"].clone();
    run(&mut e, "drw.export.dxf", json!({ "path": "front.dxf" }));
    // In the DXF, with the view's centre at (100, 100) and the wall from x 0 to 40 and z 0 to 40:
    // the block's outline (x 10 to 30, z 10 to 30) is all hidden, nothing of it visible.
    let tags = tenon_dxf::parse(&std::fs::read(dir.join("front.dxf")).unwrap()).unwrap();
    let entities = tenon_dxf::sections(&tags).into_iter().find(|s| s.name == "ENTITIES").unwrap();
    let records = tenon_dxf::records(&entities.tags);
    let field = |r: &[tenon_dxf::Tag], code: i32| r.iter().find(|t| t.code == code).map(tenon_dxf::Tag::f64);
    let layer = |r: &[tenon_dxf::Tag]| r.iter().find(|t| t.code == 8).map(tenon_dxf::Tag::str).unwrap_or_default();
    let inside_block = |r: &[tenon_dxf::Tag]| {
        let pts = [(field(r, 10), field(r, 20)), (field(r, 11), field(r, 21))];
        pts.iter().all(|p| matches!(p, (Some(x), Some(y)) if (89.0..=111.0).contains(x) && (89.0..=111.0).contains(y)))
    };
    let lines: Vec<&(String, Vec<tenon_dxf::Tag>)> = records.iter().filter(|(k, _)| k == "LINE").collect();
    assert!(lines.iter().any(|(_, r)| layer(r) == "HIDDEN" && inside_block(r)), "the block is drawn hidden");
    assert!(!lines.iter().any(|(_, r)| layer(r) == "VISIBLE" && inside_block(r)), "nothing of the block is visible through the wall");

    // A section across both parts, seen from the right: both are cut and hatched; the block's
    // hatching runs the other way from the wall's so the two parts read apart.
    let section =
        run(&mut e, "drw.view.section", json!({ "parent": front, "a": [20, -10], "b": [20, 50], "flip": true, "at": [250, 100] }))["view"].clone();
    let info = run(&mut e, "drw.info", json!({}));
    let s = info["views"].as_array().unwrap().iter().find(|v| v["id"] == section).unwrap().clone();
    assert!(s["error"].is_null() && s["hatch"].as_u64().unwrap() > 0, "{s}");
    let hatch = |e: &mut Engine| {
        run(e, "drw.export.dxf", json!({ "path": "section.dxf" }));
        let tags = tenon_dxf::parse(&std::fs::read(dir.join("section.dxf")).unwrap()).unwrap();
        let entities = tenon_dxf::sections(&tags).into_iter().find(|s| s.name == "ENTITIES").unwrap();
        tenon_dxf::records(&entities.tags)
            .into_iter()
            .filter(|(k, r)| k == "LINE" && layer(r) == "HATCH")
            .map(|(_, r)| {
                let (x0, y0, x1, y1) = (field(&r, 10).unwrap(), field(&r, 20).unwrap(), field(&r, 11).unwrap(), field(&r, 21).unwrap());
                ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (y1 - y0).atan2(x1 - x0).to_degrees().rem_euclid(180.0))
            })
            .collect::<Vec<_>>()
    };
    let lines = hatch(&mut e);
    // Seen from the right: Y across (wall 0 to 10, block 30 to 40), Z up.
    let slope = |lo: f64, hi: f64| {
        lines.iter().filter(|(x, y, _)| (lo..=hi).contains(&(x - 250.0 + 20.0)) && (90.0..=110.0).contains(y)).map(|l| l.2).collect::<Vec<_>>()
    };
    let (wall, block) = (slope(0.5, 9.5), slope(30.5, 39.5));
    assert!(!wall.is_empty() && !block.is_empty(), "both parts are hatched: {lines:?}");
    assert!(wall.iter().all(|a| (a - wall[0]).abs() < 1e-6) && block.iter().all(|a| (a - block[0]).abs() < 1e-6));
    assert!((wall[0] - block[0]).abs() > 45.0, "the parts' hatching differs: {} and {}", wall[0], block[0]);
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
    // The parts list is the assembly's own bill of materials: same items, quantities and names.
    // After a component of a new part is added to the assembly, both lists number it the same.
    let check_lists = |e: &mut Engine| {
        let bom = e.exec("asm.bom", &json!({})).unwrap()["rows"].clone();
        let list = e.exec("drw.info", &json!({})).unwrap()["annotations"][6]["rows"].clone();
        let bom = bom.as_array().unwrap();
        assert_eq!(bom.len(), list.as_array().unwrap().len(), "{bom:?} {list}");
        for (b, l) in bom.iter().zip(list.as_array().unwrap()) {
            assert_eq!((&b["item"], &b["quantity"], &b["part"], &b["name"]), (&l["item"], &l["quantity"], &l["part_number"], &l["description"]));
        }
    };
    e.exec("asm.open", &json!({ "path": "plate-pins.tenonasm" })).unwrap();
    check_lists(&mut e);
    std::fs::copy(dir.join("pin.tenon"), dir.join("washer.tenon")).unwrap();
    e.exec("asm.insert", &json!({ "path": "washer.tenon" })).unwrap();
    e.exec("asm.insert", &json!({ "path": "pin.tenon" })).unwrap();
    e.exec("asm.save", &json!({ "path": "plate-pins.tenonasm" })).unwrap();
    e.exec("drw.update", &json!({})).unwrap();
    check_lists(&mut e);
    assert_eq!(e.exec("asm.bom", &json!({})).unwrap()["rows"][1]["quantity"], 3);

    // A title block template saved from the drawing, changed, and used on its sheets.
    e.exec("drw.template.save", &json!({ "path": "block.json" })).unwrap();
    let text = std::fs::read_to_string(dir.join("block.json")).unwrap().replace("\"COMPANY\"", "\"ORGANISATION\"");
    std::fs::write(dir.join("block.json"), text).unwrap();
    assert_eq!(e.exec("drw.template.apply", &json!({ "path": "block.json" })).unwrap()["sheets"], 2);
    e.exec("drw.export.svg", &json!({ "path": "block.svg" })).unwrap();
    assert!(std::fs::read_to_string(dir.join("block.svg")).unwrap().contains("ORGANISATION"));
    e.exec("drw.undo", &json!({})).unwrap();
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
