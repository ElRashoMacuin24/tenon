//! Baseline and ordinate sets by command (DEC-132): values worked out by hand on a plate whose
//! width and hole position are parameters, what they come to when the model changes, one undo
//! step, add and take away, saved and read back to the same bytes, PDF and DXF, what is refused.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tenon_cli::Engine;
use tenon_kernel_occt::OcctKernel;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-cli-sets-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(e: &mut Engine, id: &str, p: Value) -> Value {
    e.exec(id, &p).unwrap_or_else(|err| panic!("{id}: {err}"))
}

/// A plate w x 60 x t (w = 100, t = 10), its lower left corner at the origin, with 8 mm holes
/// through it at (hx, 20) (hx = 30) and (70, 40), saved as `plate.tenon`. Parameters w, hx, t.
fn plate(e: &mut Engine) {
    run(e, "file.new", json!({ "name": "Plate" }));
    for (name, value) in [("w", "100 mm"), ("hx", "30 mm"), ("t", "10 mm")] {
        run(e, "param.add", json!({ "name": name, "equation": value }));
    }
    let sk = run(e, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    let rect = run(e, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 100, "y2": 60 }));
    let corners = rect["corners"].clone();
    run(e, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "fix", "point": corners[0] } }));
    run(
        e,
        "sketch.constrain",
        json!({ "sketch": sk, "constraint": { "type": "horizontal_distance", "a": corners[0], "b": corners[1], "value": 100.0 }, "equation": "w" }),
    );
    run(
        e,
        "sketch.constrain",
        json!({ "sketch": sk, "constraint": { "type": "vertical_distance", "a": corners[0], "b": corners[3], "value": 60.0 } }),
    );
    let body = run(e, "model.extrude", json!({ "sketch": sk, "distance": 10, "equations": { "/extent/distance": "t" } }))["feature"].clone();
    let top = run(e, "model.face_ref", json!({ "origin": { "type": "cap", "feature": body, "end": "end" } }));
    let holes = run(e, "sketch.create", json!({ "face": top }))["feature"].clone();
    let origin = run(e, "sketch.point", json!({ "sketch": holes, "x": 0, "y": 0, "center": false }))["point"].clone();
    run(e, "sketch.constrain", json!({ "sketch": holes, "constraint": { "type": "fix", "point": origin } }));
    let one = run(e, "sketch.point", json!({ "sketch": holes, "x": 30, "y": 20 }))["point"].clone();
    run(
        e,
        "sketch.constrain",
        json!({ "sketch": holes, "constraint": { "type": "horizontal_distance", "a": origin, "b": one, "value": 30.0 }, "equation": "hx" }),
    );
    run(e, "sketch.constrain", json!({ "sketch": holes, "constraint": { "type": "vertical_distance", "a": origin, "b": one, "value": 20.0 } }));
    run(e, "sketch.point", json!({ "sketch": holes, "x": 70, "y": 40 }));
    run(e, "model.hole", json!({ "sketch": holes, "diameter": 8, "through_all": true }));
    run(e, "file.save", json!({ "path": "plate.tenon" }));
}

/// The plate on a new drawing, seen from above at `scale` with its middle at (150, 130).
fn top_view(e: &mut Engine, scale: f64) -> Value {
    run(e, "drw.new", json!({ "name": "Sets" }));
    run(e, "drw.view.base", json!({ "model": "plate.tenon", "orientation": "top", "scale": scale, "at": [150, 130] }))["view"].clone()
}

fn pick(e: &mut Engine, view: &Value, x: f64, y: f64) -> Value {
    run(e, "drw.pick", json!({ "view": view, "view_at": [x, y] }))
}

/// `shown`, and the middle of its text, of each row.
fn rows(a: &Value) -> Vec<(String, f64, f64)> {
    a["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("{a}"))
        .iter()
        .map(|r| (r["shown"].as_str().unwrap().to_owned(), r["at"][0].as_f64().unwrap(), r["at"][1].as_f64().unwrap()))
        .collect()
}

fn rows_are(a: &Value, want: &[(&str, f64, f64)]) {
    let got = rows(a);
    assert_eq!(got.len(), want.len(), "{a}");
    for (g, w) in got.iter().zip(want) {
        assert!(g.0 == w.0 && (g.1 - w.1).abs() < 1e-6 && (g.2 - w.2).abs() < 1e-6, "{g:?} is not {w:?}: {a}");
    }
}

fn change_plate(e: &mut Engine, changes: &[(&str, &str)]) {
    run(e, "file.open", json!({ "path": "plate.tenon" }));
    for (name, value) in changes {
        run(e, "param.set", json!({ "name": name, "equation": value }));
    }
    run(e, "file.save", json!({ "path": "plate.tenon" }));
    run(e, "drw.update", json!({}));
}

#[test]
fn baseline_sets_measure_from_their_datum_and_follow_the_model() {
    let dir = scratch("baseline");
    let mut e = Engine::new(Box::new(OcctKernel::new()), &dir);
    plate(&mut e);
    let view = top_view(&mut e, 1.0);
    let datum = pick(&mut e, &view, 0.0, 30.0);
    let (hole1, hole2, right) = (pick(&mut e, &view, 34.0, 20.0), pick(&mut e, &view, 74.0, 40.0), pick(&mut e, &view, 100.0, 30.0));
    assert_eq!((datum["kind"].as_str(), hole1["kind"].as_str()), (Some("line"), Some("circle")));

    // The view's middle (50, 30) is at (150, 130): x on the sheet is 100 + x, y is 100 + y. Above
    // the view: horizontal, from the left side at 100. The shortest (30) at 180, then 8 further
    // each, each text in the middle of its span.
    let r = run(&mut e, "drw.baseline", json!({ "view": view, "datum": datum, "targets": [right, hole2, hole1], "at": [150, 180] }));
    assert_eq!(r["axis"], "horizontal", "{r}");
    assert_eq!(r["step"], 8.0);
    rows_are(&r, &[("30", 115.0, 180.0), ("70", 135.0, 188.0), ("100", 150.0, 196.0)]);
    assert_eq!(r["rows"][0]["place"], 2, "the place's number in the set");
    let set = r["annotation"].clone();

    // To the right of the view: vertical, from the bottom edge at y = 100, in columns at x = 220 on:
    // 20 and 40 and 60 to the hole centres and the top edge, each in the middle of its span.
    let bottom = pick(&mut e, &view, 50.0, 0.0);
    let top = pick(&mut e, &view, 50.0, 60.0);
    let (hole1, hole2) = (pick(&mut e, &view, 34.0, 20.0), pick(&mut e, &view, 74.0, 40.0));
    let up = run(&mut e, "drw.baseline", json!({ "view": view, "datum": bottom, "targets": [hole1, hole2, top], "at": [220, 130], "spacing": 10 }));
    assert_eq!(up["axis"], "vertical", "{up}");
    rows_are(&up, &[("20", 220.0, 110.0), ("40", 230.0, 120.0), ("60", 240.0, 130.0)]);

    // One undo takes the whole set away.
    let before = run(&mut e, "drw.info", json!({}));
    assert_eq!(before["annotations"].as_array().unwrap().len(), 2);
    run(&mut e, "drw.undo", json!({}));
    assert_eq!(run(&mut e, "drw.info", json!({}))["annotations"].as_array().unwrap().len(), 1);
    run(&mut e, "drw.redo", json!({}));

    // It follows the model: w = 120, hx = 36, t = 25. The view is 120 wide now, so its middle
    // (60, 30) is at (150, 130): x on the sheet is 90 + x.
    change_plate(&mut e, &[("w", "120"), ("hx", "36"), ("t", "25")]);
    let info = run(&mut e, "drw.info", json!({}));
    assert!(info["views"][0]["error"].is_null(), "{info}");
    rows_are(&info["annotations"][0], &[("36", 108.0, 180.0), ("70", 125.0, 188.0), ("120", 150.0, 196.0)]);
    assert_eq!(info["annotations"][0]["rows"][2]["value"], 120.0);

    // When the hole passes the other, the stack goes by the distances as they are now: hx = 80.
    change_plate(&mut e, &[("hx", "80")]);
    let info = run(&mut e, "drw.info", json!({}));
    rows_are(&info["annotations"][0], &[("70", 125.0, 180.0), ("80", 130.0, 188.0), ("120", 150.0, 196.0)]);
    assert_eq!(info["annotations"][0]["rows"][0]["place"], 1);
    change_plate(&mut e, &[("hx", "36")]);

    // Add to it, take from it, as one record: its middle place (the far hole) taken away and the
    // near hole left. Numbers are the set's, in the order given.
    let taken = run(&mut e, "drw.annotation.edit", json!({ "annotation": set, "remove": [1] }));
    rows_are(&taken, &[("36", 108.0, 180.0), ("120", 150.0, 188.0)]);
    let far_hole = pick(&mut e, &view, 74.0, 40.0);
    let added = run(&mut e, "drw.annotation.edit", json!({ "annotation": set, "add": [far_hole] }));
    rows_are(&added, &[("36", 108.0, 180.0), ("70", 125.0, 188.0), ("120", 150.0, 196.0)]);
    run(&mut e, "drw.undo", json!({}));
    assert_eq!(run(&mut e, "drw.info", json!({}))["annotations"][0]["targets"].as_array().unwrap().len(), 2, "one step each");
    run(&mut e, "drw.redo", json!({}));

    // A set moves whole, across what it measures: by [40, 10] moves its lines up by 10 and nothing
    // along. Spacing, decimals and direction change. From the left side's middle (y = 30), hole
    // at y = 20 is 10 away.
    let moved = run(&mut e, "drw.annotation.edit", json!({ "annotation": set, "by": [40, 10], "spacing": 12, "precision": 1 }));
    rows_are(&moved, &[("36", 108.0, 190.0), ("70", 125.0, 202.0), ("120", 150.0, 214.0)]);

    // Refused, changing nothing, in words that say what to do.
    let before = run(&mut e, "drw.info", json!({}));
    let bottom = pick(&mut e, &view, 60.0, 0.0);
    let far_hole = pick(&mut e, &view, 74.0, 40.0);
    let one_more = |e: &mut Engine, p: Value| e.exec("drw.annotation.edit", &p).unwrap_err();
    assert!(one_more(&mut e, json!({ "annotation": set, "add": [far_hole] })).contains("in one place"));
    assert!(one_more(&mut e, json!({ "annotation": set, "remove": [0, 1, 2] })).contains("at least one place"));
    assert!(one_more(&mut e, json!({ "annotation": set, "remove": [7] })).contains("places are 0 to 2"));
    assert!(one_more(&mut e, json!({ "annotation": set, "text": "<> TYP" })).contains("no text"));
    run(&mut e, "drw.annotation.edit", json!({ "annotation": set, "text": null }));
    assert!(
        one_more(&mut e, json!({ "annotation": set, "axis": "vertical" })).contains("where the datum is"),
        "the right side is level with the left"
    );
    assert!(one_more(&mut e, json!({ "annotation": set, "spacing": 0 })).contains("spacing"));
    let refuse = |e: &mut Engine, p: Value| e.exec("drw.baseline", &p).unwrap_err();
    assert!(refuse(&mut e, json!({ "view": view, "datum": bottom, "targets": [] })).contains("at least one"));
    assert!(refuse(&mut e, json!({ "view": view, "datum": bottom, "targets": [bottom], "axis": "vertical" })).contains("where the datum is"));
    assert!(
        refuse(&mut e, json!({ "view": view, "datum": bottom, "targets": [1], "axis": "vertical" })).contains("must be an object"),
        "by its schema"
    );
    assert_eq!(run(&mut e, "drw.info", json!({}))["annotations"], before["annotations"]);
    // Only a set takes what only a set takes.
    let note = run(&mut e, "drw.note", json!({ "text": "NOTE", "at": [30, 30] }))["annotation"].clone();
    assert!(one_more(&mut e, json!({ "annotation": note, "add": [bottom] })).contains("baseline and ordinate"));
}

#[test]
fn ordinate_sets_read_positions_from_their_origin_and_follow_the_model() {
    let dir = scratch("ordinate");
    let mut e = Engine::new(Box::new(OcctKernel::new()), &dir);
    plate(&mut e);
    let view = top_view(&mut e, 1.0);
    let origin = pick(&mut e, &view, 30.0, 16.0);
    assert_eq!(origin["kind"], "circle", "the hole at (30, 20)");
    let mut centre = origin.clone();
    centre["point"] = json!("center");
    let (left, hole2, right) = (pick(&mut e, &view, 0.0, 30.0), pick(&mut e, &view, 74.0, 40.0), pick(&mut e, &view, 100.0, 30.0));

    // Below the view (y on the sheet 100 to 160): horizontal, from the hole at x = 30. Left of it,
    // negative. Each value under the end of its line: the text's middle 1 and half a height below.
    let r = run(&mut e, "drw.ordinate", json!({ "view": view, "origin": centre, "targets": [left, hole2, right], "at": [150, 80] }));
    assert_eq!(r["axis"], "horizontal", "{r}");
    let y = 80.0 - 1.0 - 3.5 / 2.0;
    rows_are(&r, &[("-30", 100.0, y), ("0", 130.0, y), ("40", 170.0, y), ("70", 200.0, y)]);
    assert!(r["rows"][1]["place"].is_null(), "the origin is not a place");
    assert_eq!(r["rows"][0]["value"], -30.0);
    let set = r["annotation"].clone();

    // To the right: vertical, from the bottom edge. Values to the right of the ends of their lines.
    let bottom = pick(&mut e, &view, 50.0, 0.0);
    let (hole1, hole2, top) = (pick(&mut e, &view, 34.0, 20.0), pick(&mut e, &view, 74.0, 40.0), pick(&mut e, &view, 50.0, 60.0));
    let up = run(&mut e, "drw.ordinate", json!({ "view": view, "origin": bottom, "targets": [hole1, hole2, top], "at": [220, 130], "precision": 1 }));
    assert_eq!(up["axis"], "vertical", "{up}");
    let half = |s: &str| tenon_drawing::stroke::text_width(s, 3.5) / 2.0;
    rows_are(
        &up,
        &[("0", 221.0 + half("0"), 100.0), ("20", 221.0 + half("20"), 120.0), ("40", 221.0 + half("40"), 140.0), ("60", 221.0 + half("60"), 160.0)],
    );

    // It follows the model: w = 120, hx = 36: the origin moved, so every reading did. The view's
    // middle is now at x = 60, so x on the sheet is 90 + x.
    change_plate(&mut e, &[("w", "120"), ("hx", "36")]);
    let info = run(&mut e, "drw.info", json!({}));
    rows_are(&info["annotations"][0], &[("-36", 90.0, y), ("0", 126.0, y), ("34", 160.0, y), ("84", 210.0, y)]);

    // Moved across, added to and taken from.
    let moved = run(&mut e, "drw.annotation.edit", json!({ "annotation": set, "by": [99, -5], "remove": [1] }));
    let y = y - 5.0;
    rows_are(&moved, &[("-36", 90.0, y), ("0", 126.0, y), ("84", 210.0, y)]);
    assert!(e.exec("drw.annotation.edit", &json!({ "annotation": set, "spacing": 12 })).unwrap_err().contains("no spacing"));

    // Above the view the values sit above their lines.
    let high = run(&mut e, "drw.annotation.edit", json!({ "annotation": set, "by": [0, 100] }));
    let y = 175.0 + 1.0 + 3.5 / 2.0;
    rows_are(&high, &[("-36", 90.0, y), ("0", 126.0, y), ("84", 210.0, y)]);
}

/// Where each text of an annotation is, read from a DXF file.
fn dxf_texts(file: &Path) -> Vec<(String, f64, f64)> {
    let bytes = std::fs::read(file).unwrap();
    let tags = tenon_dxf::parse(&bytes).unwrap();
    let entities = tenon_dxf::sections(&tags).into_iter().find(|s| s.name == "ENTITIES").unwrap();
    tenon_dxf::records(&entities.tags)
        .iter()
        .filter(|(k, _)| k == "TEXT")
        .map(|(_, r)| {
            let f = |code: i32| r.iter().find(|t| t.code == code).map_or(f64::NAN, tenon_dxf::Tag::f64);
            (r.iter().find(|t| t.code == 1).map(tenon_dxf::Tag::str).unwrap_or_default(), f(10), f(20))
        })
        .collect()
}

#[test]
fn sets_are_saved_read_back_to_the_same_bytes_and_go_to_pdf_and_dxf() {
    let dir = scratch("files");
    let mut e = Engine::new(Box::new(OcctKernel::new()), &dir);
    plate(&mut e);
    let view = top_view(&mut e, 1.0);
    let (left, right, hole1, hole2) =
        (pick(&mut e, &view, 0.0, 30.0), pick(&mut e, &view, 100.0, 30.0), pick(&mut e, &view, 34.0, 20.0), pick(&mut e, &view, 74.0, 40.0));
    run(
        &mut e,
        "drw.baseline",
        json!({ "view": view, "datum": left, "targets": [hole1, hole2, right], "at": [150, 180], "spacing": 9.5, "precision": 3 }),
    );
    let bottom = pick(&mut e, &view, 50.0, 0.0);
    let top = pick(&mut e, &view, 50.0, 60.0);
    run(&mut e, "drw.ordinate", json!({ "view": view, "origin": bottom, "targets": [hole1, hole2, top], "at": [80, 130], "axis": "vertical" }));
    let info = run(&mut e, "drw.info", json!({}));

    // Saved: as text, its records saying what they are; read back, the same bytes again.
    run(&mut e, "drw.save", json!({ "path": "sets.tenondrw" }));
    let first = std::fs::read(dir.join("sets.tenondrw")).unwrap();
    let text = String::from_utf8_lossy(&first).into_owned();
    for line in ["type = \"baseline\"", "type = \"ordinate\"", "axis = \"horizontal\"", "axis = \"vertical\"", "spacing = 9.5", "precision = 3"] {
        assert!(text.lines().any(|l| l == line), "{line} not in:\n{text}");
    }
    assert!(text.lines().any(|l| l.starts_with("datum = { edge = ")) && text.lines().any(|l| l.starts_with("origin = { edge = ")), "{text}");
    let d = tenon_io::drw::from_bytes(&first, &dir).unwrap();
    assert_eq!(tenon_io::drw::to_bytes(&d, &dir).unwrap(), first, "the same bytes");
    run(&mut e, "drw.open", json!({ "path": "sets.tenondrw" }));
    let back = run(&mut e, "drw.info", json!({}));
    assert_eq!(rows(&back["annotations"][0]), rows(&info["annotations"][0]), "the same values in the same places");
    assert_eq!(rows(&back["annotations"][1]), rows(&info["annotations"][1]));
    run(&mut e, "drw.save", json!({ "path": "again.tenondrw" }));
    assert_eq!(std::fs::read(dir.join("again.tenondrw")).unwrap(), first);

    // A file without the optional fields reads with their defaults (8 mm, 2 decimals); one with a
    // set that holds nothing, or a spacing of nothing, is refused as damaged.
    let cut = |t: &str, gone: &[&str]| t.lines().filter(|l| !gone.iter().any(|g| l.starts_with(g))).collect::<Vec<_>>().join("\n") + "\n";
    let plain = tenon_io::drw::from_bytes(cut(&text, &["spacing =", "precision ="]).as_bytes(), &dir).unwrap();
    assert!(matches!(plain.annotations[0].kind, tenon_drawing::AnnotKind::Baseline { spacing: 8.0, precision: 2, .. }));
    let bad = text.replace("spacing = 9.5", "spacing = 0.0");
    assert!(tenon_io::drw::from_bytes(bad.as_bytes(), &dir).unwrap_err().to_string().contains("out of range"));
    let mut empty = d.clone();
    if let tenon_drawing::AnnotKind::Baseline { targets, .. } = &mut empty.annotations[0].kind {
        targets.clear();
    }
    let bytes = tenon_io::drw::to_bytes(&empty, &dir).unwrap();
    let err = tenon_io::drw::from_bytes(&bytes, &dir).unwrap_err().to_string();
    assert!(err.contains("out of range"), "{err}");
    // An old build did not know the type: how it takes a new file is what it does with any type it
    // does not know, seen here by a type this build does not know: a refusal that names it.
    let unknown = text.replace("type = \"baseline\"", "type = \"chain\"");
    let err = tenon_io::drw::from_bytes(unknown.as_bytes(), &dir).unwrap_err().to_string();
    assert!(err.contains("chain"), "{err}");

    // The DXF: every value at its place, as text, on the sheet; arrows only on the baseline set,
    // two a row; the file reads back as other programs read it.
    run(&mut e, "drw.export.dxf", json!({ "path": "sets.dxf" }));
    let texts = dxf_texts(&dir.join("sets.dxf"));
    let y = 180.0 - 3.5 / 2.0;
    for (t, x, y) in [("30", 115.0, y), ("70", 135.0, y + 9.5), ("100", 150.0, y + 19.0)] {
        assert!(texts.iter().any(|(s, a, b)| s == t && (a - x).abs() < 1e-6 && (b - y).abs() < 1e-6), "{t} at {x}, {y}: {texts:?}");
    }
    for t in ["0", "20", "40", "60"] {
        assert!(texts.iter().any(|(s, ..)| s == t), "{t}: {texts:?}");
    }
    let stats = tenon_drawing::export::check_dxf(&std::fs::read_to_string(dir.join("sets.dxf")).unwrap()).unwrap();
    assert_eq!(stats.solids, 6, "{stats:?}");
    // The extension lines and dimension lines on the THIN layer: the datum's, from 1.5 off the
    // left edge (its top, y = 160 on the sheet) to 2 past the last line (199 + 2).
    let dxf = std::fs::read(dir.join("sets.dxf")).unwrap();
    let tags = tenon_dxf::parse(&dxf).unwrap();
    let entities = tenon_dxf::sections(&tags).into_iter().find(|s| s.name == "ENTITIES").unwrap();
    let thin: Vec<[f64; 4]> = tenon_dxf::records(&entities.tags)
        .iter()
        .filter(|(k, r)| k == "LINE" && r.iter().any(|t| t.code == 8 && t.str() == "THIN"))
        .map(|(_, r)| [10, 20, 11, 21].map(|c| r.iter().find(|t| t.code == c).map_or(f64::NAN, tenon_dxf::Tag::f64)))
        .collect();
    assert!(
        thin.iter().any(|l| (l[0] - 100.0).abs() < 1e-6 && (l[1] - 161.5).abs() < 1e-6 && (l[2] - 100.0).abs() < 1e-6 && (l[3] - 201.0).abs() < 1e-6),
        "{thin:?}"
    );

    // The PDF: read back as a reader does, with the values among its words.
    run(&mut e, "drw.export.pdf", json!({ "path": "sets.pdf" }));
    let pdf = std::fs::read(dir.join("sets.pdf")).unwrap();
    assert_eq!(tenon_drawing::export::check_pdf(&pdf), Ok(1));
    let words = String::from_utf8_lossy(&pdf).into_owned();
    for t in ["(30)", "(70)", "(100)", "(20)", "(40)", "(60)"] {
        assert!(words.contains(t), "{t} not in the PDF");
    }
    // Deleting the set leaves the rest, and one undo brings it back.
    let id = info["annotations"][0]["id"].clone();
    run(&mut e, "drw.delete", json!({ "annotation": id }));
    assert_eq!(run(&mut e, "drw.info", json!({}))["annotations"].as_array().unwrap().len(), 1);
    run(&mut e, "drw.undo", json!({}));
    assert_eq!(run(&mut e, "drw.info", json!({}))["annotations"].as_array().unwrap().len(), 2);
    // `tenon-cli diff` tells of them as of other annotations.
    run(&mut e, "drw.annotation.edit", json!({ "annotation": id, "spacing": 20 }));
    run(&mut e, "drw.save", json!({ "path": "changed.tenondrw" }));
    let told = tenon_io::diff::diff_files(&dir.join("sets.tenondrw"), &dir.join("changed.tenondrw")).unwrap().text();
    assert!(told.contains("spacing"), "{told}");
    // A template keeps no set: they need a model.
    assert!(tenon_io::drw::as_template(&d, "T").annotations.is_empty());
}

#[test]
fn a_set_at_another_scale_reads_model_millimetres_and_what_it_has_lost_is_said() {
    let dir = scratch("scale");
    let mut e = Engine::new(Box::new(OcctKernel::new()), &dir);
    plate(&mut e);
    // At 1:2 the values are the model's and the distances on the sheet half that.
    let view = top_view(&mut e, 0.5);
    let (left, right, hole1, hole2) =
        (pick(&mut e, &view, 0.0, 30.0), pick(&mut e, &view, 100.0, 30.0), pick(&mut e, &view, 34.0, 20.0), pick(&mut e, &view, 74.0, 40.0));
    let r = run(&mut e, "drw.baseline", json!({ "view": view, "datum": left, "targets": [hole1, hole2, right], "at": [150, 180] }));
    // x on the sheet is 150 + (x - 50) / 2: the left side at 125.
    rows_are(&r, &[("30", 125.0 + 7.5, 180.0), ("70", 125.0 + 17.5, 188.0), ("100", 150.0, 196.0)]);

    // Nothing told: outside the view, above it for a horizontal set (the view's top is at 145).
    let r = run(&mut e, "drw.ordinate", json!({ "view": view, "origin": left, "targets": [hole1, right], "axis": "horizontal" }));
    // Past the set already above: its last extension lines end 2 past 196, then 10 clear; the text's
    // middle 1 and half a height above that.
    let level = 196.0 + 2.0 + 10.0;
    assert!((r["rows"][0]["at"][1].as_f64().unwrap() - (level + 1.0 + 3.5 / 2.0)).abs() < 1e-6, "{r}");
    // And to the right for a vertical one, its direction told by how the places are spread.
    let bottom = pick(&mut e, &view, 50.0, 0.0);
    let top = pick(&mut e, &view, 50.0, 60.0);
    let r = run(&mut e, "drw.baseline", json!({ "view": view, "datum": bottom, "targets": [top] }));
    assert_eq!(r["axis"], "vertical", "{r}");
    assert_eq!(r["rows"][0]["shown"], "60");
    // The view's right side is at 175, and the ordinate set's "100" stands half its width past that.
    let past = 175.0 + tenon_drawing::stroke::text_width("100", 3.5) / 2.0 + 10.0;
    assert!((r["rows"][0]["level"].as_f64().unwrap() - past).abs() < 1e-6, "{r}");

    // An isometric view is not dimensioned, as Suggest Dimensions says too.
    let iso = run(&mut e, "drw.view.base", json!({ "model": "plate.tenon", "orientation": "iso", "scale": 0.5, "at": [320, 200] }))["view"].clone();
    let edges = run(&mut e, "drw.pick", json!({ "view": iso, "edge": { "index": 0 } }));
    let more = run(&mut e, "drw.pick", json!({ "view": iso, "edge": { "index": 3 } }));
    let err = e.exec("drw.baseline", &json!({ "view": iso, "datum": edges, "targets": [more] })).unwrap_err();
    assert!(err.contains("isometric"), "{err}");

    // A hole gone from the model: what can still be found is drawn, and the rest is said. Delete
    // the hole feature: of the first set, the hole places are lost, the right side is not.
    run(&mut e, "file.open", json!({ "path": "plate.tenon" }));
    let tree = run(&mut e, "model.tree", json!({}));
    let hole = tree["features"].as_array().unwrap().iter().find(|f| f["type"] == "Hole").unwrap_or_else(|| panic!("{tree}"))["id"].clone();
    run(&mut e, "feature.delete", json!({ "feature": hole }));
    run(&mut e, "file.save", json!({ "path": "plate.tenon" }));
    run(&mut e, "drw.update", json!({}));
    let info = run(&mut e, "drw.info", json!({}));
    let set = &info["annotations"][0];
    assert_eq!(rows(set).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["100"], "{set}");
    assert!(set["problem"].as_str().unwrap_or_default().contains("2 places of the set cannot be found"), "{set}");
    run(&mut e, "drw.export.pdf", json!({ "path": "lost.pdf" }));
    // Taking them away puts it right.
    let id = set["id"].clone();
    run(&mut e, "drw.annotation.edit", json!({ "annotation": id, "remove": [0, 1] }));
    assert!(run(&mut e, "drw.info", json!({}))["annotations"][0]["problem"].is_null());
    // With its datum gone it cannot be drawn, and says so. (A pick of the origin's hole.)
    let info = run(&mut e, "drw.info", json!({}));
    assert!(info["views"][0]["error"].is_null(), "{info}");
}

#[test]
fn suggestions_leave_out_what_a_set_already_gives() {
    let dir = scratch("suggest");
    let mut e = Engine::new(Box::new(OcctKernel::new()), &dir);
    plate(&mut e);
    let view = top_view(&mut e, 1.0);
    let words = |r: &Value| -> Vec<String> {
        r["suggestions"].as_array().unwrap().iter().map(|s| format!("{} {}", s["type"].as_str().unwrap(), s["shown"].as_str().unwrap())).collect()
    };
    let before = words(&run(&mut e, "drw.dimension.suggest", json!({ "view": view })));
    assert!(before.contains(&"horizontal 100".to_owned()) && before.contains(&"vertical 60".to_owned()), "{before:?}");
    let (left, right) = (pick(&mut e, &view, 0.0, 30.0), pick(&mut e, &view, 100.0, 30.0));
    run(&mut e, "drw.baseline", json!({ "view": view, "datum": left, "targets": [right], "at": [150, 180] }));
    let (bottom, top) = (pick(&mut e, &view, 50.0, 0.0), pick(&mut e, &view, 50.0, 60.0));
    run(&mut e, "drw.ordinate", json!({ "view": view, "origin": bottom, "targets": [top], "at": [220, 130] }));
    let after = words(&run(&mut e, "drw.dimension.suggest", json!({ "view": view })));
    assert!(!after.iter().any(|w| w == "horizontal 100" || w == "vertical 60"), "{after:?}");
    assert_eq!(after.len(), before.len() - 2, "{after:?}");
}

#[test]
fn a_set_in_an_assembly_view_measures_across_its_parts() {
    let dir = scratch("assembly");
    let r = tenon_cli::demo_m4(Box::new(OcctKernel::new()), &dir).unwrap();
    assert_eq!(r.json["ok"], true, "{}", r.text);
    let mut e = Engine::new(Box::new(OcctKernel::new()), &dir);
    run(&mut e, "drw.new", json!({ "name": "Assembly" }));
    let view =
        run(&mut e, "drw.view.base", json!({ "model": "plate-pins.tenonasm", "orientation": "top", "scale": 1, "at": [150, 130] }))["view"].clone();
    // The plate's left side, and the heads of the pins (radius 7) at (15, 15) and (105, 65).
    let left = pick(&mut e, &view, 0.0, 40.0);
    assert!(left["kind"] == "line" && left["component"].is_number(), "{left}");
    let (near, far) = (pick(&mut e, &view, 22.0, 15.0), pick(&mut e, &view, 112.0, 65.0));
    assert_ne!(near["component"], far["component"], "{near} {far}");
    let r = run(&mut e, "drw.baseline", json!({ "view": view, "datum": left, "targets": [far, near], "axis": "horizontal", "at": [150, 190] }));
    assert_eq!(rows(&r).iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["15", "105"], "{r}");
    let r = run(&mut e, "drw.ordinate", json!({ "view": view, "origin": left, "targets": [far, near], "axis": "horizontal", "at": [150, 60] }));
    assert_eq!(rows(&r).iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["0", "15", "105"], "{r}");
}

fn annotation_count(e: &mut Engine) -> usize {
    run(e, "drw.info", json!({}))["annotations"].as_array().unwrap().len()
}

#[test]
fn a_view_made_from_an_isometric_one_is_not_dimensioned_either() {
    let dir = scratch("iso");
    let mut e = Engine::new(Box::new(OcctKernel::new()), &dir);
    plate(&mut e);
    run(&mut e, "drw.new", json!({ "name": "Sets", "size": "C" }));
    let top = run(&mut e, "drw.view.base", json!({ "model": "plate.tenon", "orientation": "top", "scale": 1, "at": [100, 300] }))["view"].clone();
    // The plate's two sides, 100 apart: picks of the model, good in any of its views.
    let (left, right) = (pick(&mut e, &top, 0.0, 30.0), pick(&mut e, &top, 100.0, 30.0));
    let view = |e: &mut Engine, id: &str, p: Value| run(e, id, p)["view"].clone();
    let iso = view(&mut e, "drw.view.base", json!({ "model": "plate.tenon", "orientation": "iso", "scale": 1, "at": [100, 150] }));
    let diagonal = view(&mut e, "drw.view.projected", json!({ "parent": top, "side": "above_right", "at": [300, 330] }));
    let slanting = [
        ("an isometric view", iso.clone()),
        ("a detail of one", view(&mut e, "drw.view.detail", json!({ "parent": iso, "center": [0, 0], "radius": 200, "scale": 1, "at": [320, 150] }))),
        ("a section of one", view(&mut e, "drw.view.section", json!({ "parent": iso, "a": [-100, 0], "b": [100, 0], "at": [520, 100] }))),
        ("a projection of one", view(&mut e, "drw.view.projected", json!({ "parent": iso, "side": "right", "at": [320, 330] }))),
        ("a diagonal projection", diagonal.clone()),
        ("a detail of that", view(&mut e, "drw.view.detail", json!({ "parent": diagonal, "center": [0, 0], "radius": 200, "scale": 1, "at": [520, 330] }))),
    ];
    let count = annotation_count(&mut e);
    for (what, v) in &slanting {
        for (id, from) in [("drw.baseline", "datum"), ("drw.ordinate", "origin")] {
            let err = e.exec(id, &json!({ "view": v, from: left, "targets": [right], "axis": "horizontal", "at": [100, 220] })).unwrap_err();
            assert!(err.contains("isometric"), "{id} on {what}: {err}");
        }
    }
    assert_eq!(annotation_count(&mut e), count, "nothing made");
    // Asked for no place at all, the refusal is still that one.
    let err = e.exec("drw.baseline", &json!({ "view": iso, "datum": left, "targets": [right], "axis": "horizontal" })).unwrap_err();
    assert!(err.contains("isometric"), "{err}");
    // Suggest Dimensions goes by the same test.
    for v in [&slanting[2].1, &slanting[3].1] {
        let err = e.exec("drw.dimension.suggest", &json!({ "view": v })).unwrap_err();
        assert!(err.contains("isometric"), "{err}");
    }

    // A view made from a square one is measured, and reads the true 100 (not 100 x cos 45 = 70.71).
    let detail = view(&mut e, "drw.view.detail", json!({ "parent": top, "center": [50, 30], "radius": 200, "scale": 1, "at": [320, 40] }));
    let section = view(&mut e, "drw.view.section", json!({ "parent": top, "a": [-10, 20], "b": [110, 20], "at": [100, 40] }));
    let front = view(&mut e, "drw.view.projected", json!({ "parent": top, "side": "below", "at": [520, 40] }));
    for v in [&top, &detail, &section, &front] {
        let r = run(&mut e, "drw.baseline", json!({ "view": v, "datum": left, "targets": [right], "axis": "horizontal", "at": [100, 220] }));
        assert_eq!(rows(&r)[0].0, "100", "{r}");
    }
}
