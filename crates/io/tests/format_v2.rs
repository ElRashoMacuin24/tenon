//! Format version 2 (plain text, DEC-031): every version-1 file upgrades to the text checked in
//! beside it and regenerates the same, saving keeps the version-1 original once, damaged text is
//! refused with its line, edits on two branches merge, and `diff` reports what changed.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use tenon_io::project::{self, ProjectError};
use tenon_io::{asm, diff, drw};
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::{Document, Session};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-io-v2-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every version-1 file kept as a fixture, with the version-2 file it became in the repository.
fn fixtures() -> Vec<(PathBuf, PathBuf)> {
    let v1 = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1");
    let mut out = Vec::new();
    for dir in std::fs::read_dir(&v1).unwrap() {
        let dir = dir.unwrap().path();
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        for f in std::fs::read_dir(&dir).unwrap() {
            let f = f.unwrap().path();
            let file = f.file_name().unwrap().to_owned();
            let now = if name == "templates" { root().join("assets/templates").join(file) } else { root().join("examples").join(&name).join(file) };
            out.push((f, now));
        }
    }
    out.sort();
    assert_eq!(out.len(), 14, "every version-1 example and template is kept: {out:?}");
    out
}

fn ext(p: &Path) -> String {
    p.extension().unwrap().to_string_lossy().into_owned()
}

/// A file's bytes as this build writes them, from the file at `p` (read in its own folder).
fn resaved(p: &Path) -> Vec<u8> {
    let data = std::fs::read(p).unwrap();
    let dir = p.parent().unwrap();
    match ext(p).as_str() {
        "tenonasm" => asm::to_bytes(&asm::from_bytes(&data, dir).unwrap(), dir).unwrap(),
        "tenondrw" => drw::to_bytes(&drw::from_bytes(&data, dir).unwrap(), dir).unwrap(),
        _ => {
            let (doc, extra) = project::from_bytes(&data).unwrap();
            project::to_bytes(&doc, &extra).unwrap()
        }
    }
}

/// Text as checked out (Git may have turned line ends into CRLF).
fn checked_out(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap().replace("\r\n", "\n")
}

#[test]
fn every_version_1_file_upgrades_to_the_text_beside_it_and_saves_the_same_again() {
    for (old, now) in fixtures() {
        assert!(project::is_version_1(&std::fs::read(&old).unwrap()), "{}", old.display());
        let upgraded = String::from_utf8(resaved(&old)).unwrap();
        assert_eq!(upgraded, checked_out(&now), "{} upgrades to {}", old.display(), now.display());
        // Read and written again: the same bytes (stable output).
        let tmp = scratch("again").join(now.file_name().unwrap());
        std::fs::write(&tmp, &upgraded).unwrap();
        assert_eq!(String::from_utf8(resaved(&tmp)).unwrap(), upgraded, "{}", now.display());
        // And nothing changed in what the file means.
        let d = diff::diff_files(&old, &now).unwrap();
        assert!(d.same(), "{}: {}", now.display(), d.text());
    }
}

#[test]
fn upgraded_parts_regenerate_the_same_solids() {
    let mut k = OcctKernel::new();
    let mut solids = |doc: Document| -> (Vec<f64>, Option<String>) {
        let mut s = Session::default();
        s.replace_document(doc, None);
        let r = s.regen(&mut k);
        let error = r.first_error().map(|(_, m)| m.to_owned());
        let shapes: Vec<_> = r.bodies.iter().map(|b| b.shape).collect();
        (shapes.into_iter().map(|h| k.mass_properties(h, 1.0).unwrap().volume).collect(), error)
    };
    let mut parts = 0;
    for (old, now) in fixtures().into_iter().filter(|(o, _)| ext(o) == "tenon") {
        let (a, ea) = solids(project::open(&old).unwrap().0);
        let (b, eb) = solids(project::open(&now).unwrap().0);
        assert_eq!((ea, eb), (None, None), "{}", now.display());
        assert!(!a.is_empty() && a.len() == b.len(), "{}: {a:?} {b:?}", now.display());
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() <= tenon_geom::tol::MEASURE_REL * x.abs(), "{}: volume {x} became {y}", now.display());
        }
        parts += 1;
    }
    assert_eq!(parts, 9);
}

#[test]
fn saving_over_a_version_1_file_keeps_it_once() {
    let dir = scratch("keep");
    let v1 = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1/m4-plate");
    for f in ["plate.tenon", "pin.tenon", "plate-pins.tenonasm"] {
        std::fs::copy(v1.join(f), dir.join(f)).unwrap();
    }
    let part = dir.join("plate.tenon");
    let original = std::fs::read(&part).unwrap();
    let (doc, extra) = project::open(&part).unwrap();
    let kept = project::save(&part, &doc, &extra).unwrap();
    assert_eq!(kept.as_deref(), Some(dir.join("plate.v1.tenon").as_path()));
    assert_eq!(std::fs::read(dir.join("plate.v1.tenon")).unwrap(), original, "the copy is the original, byte for byte");
    assert!(std::fs::read_to_string(&part).unwrap().contains("\nversion = 2\n"));
    // Saved again: a version-2 file is replaced without a copy.
    assert_eq!(project::save(&part, &doc, &extra).unwrap(), None);
    // An assembly the same way; the copy opens in this build too.
    let a = dir.join("plate-pins.tenonasm");
    let (assembly, _) = asm::open(&a).unwrap();
    assert_eq!(asm::save(&a, &assembly).unwrap().as_deref(), Some(dir.join("plate-pins.v1.tenonasm").as_path()));
    assert_eq!(asm::open(&dir.join("plate-pins.v1.tenonasm")).unwrap().0.components.len(), assembly.components.len());
    // A copy already there is never overwritten.
    std::fs::copy(v1.join("pin.tenon"), dir.join("pin.tenon")).unwrap();
    std::fs::write(dir.join("pin.v1.tenon"), b"older copy").unwrap();
    let (pin, extra) = project::open(&dir.join("pin.tenon")).unwrap();
    assert_eq!(project::save(&dir.join("pin.tenon"), &pin, &extra).unwrap(), None);
    assert_eq!(std::fs::read(dir.join("pin.v1.tenon")).unwrap(), b"older copy");
    // The save command says so.
    let mut s = Session::default();
    std::fs::copy(v1.join("pin.tenon"), dir.join("pin2.tenon")).unwrap();
    tenon_io::cmd::run(&mut s, "file.open", &json!({ "path": dir.join("pin2.tenon") }), None).unwrap();
    let r = tenon_io::cmd::run(&mut s, "file.save", &json!({ "path": dir.join("pin2.tenon") }), None).unwrap();
    assert_eq!(r["kept_version_1"], json!([dir.join("pin2.v1.tenon").display().to_string()]));
}

fn enclosure_text() -> String {
    checked_out(&root().join("examples/m2-enclosure/enclosure.tenon"))
}

fn damaged(text: &str) -> String {
    match project::from_bytes(text.as_bytes()) {
        Err(ProjectError::Damaged(m)) => m,
        other => panic!("expected damaged, got {other:?}"),
    }
}

#[test]
fn damaged_text_is_refused_with_its_line_and_record() {
    let text = enclosure_text();
    let fillet_line = text.lines().position(|l| l == "name = \"Fillet1\"").unwrap();
    let header_line = fillet_line; // `[[feature]]` and `id = 3` come just before the name.
    // A value of the wrong type: the line of its feature, and which one.
    let m = damaged(&text.replace("radius = 6.0", "radius = \"six\""));
    assert!(m.starts_with(&format!("line {}: feature 3 (Fillet1): ", header_line - 1)), "{m}");
    // A feature type this build does not know.
    let m = damaged(&text.replace("type = \"shell\"", "type = \"twist\""));
    assert!(m.contains("feature 4 (Shell1)") && m.contains("twist"), "{m}");
    // Cut short in the middle of a line: where.
    let cut = &text[..text.find("radius = 6.0").unwrap() + 6];
    let m = damaged(cut);
    assert!(m.starts_with("line "), "{m}");
    // Two features with one id (two branches each added one): refused, both named.
    let dup = text.replacen("id = 13\n", "id = 12\n", 1);
    let m = damaged(&dup);
    assert_eq!(m, "two features have id 12: Sketch4 and Hole2");
    // Text written as version 1, or a file that is not Tenon text at all.
    assert!(damaged(&text.replace("\nversion = 2\n", "\nversion = 1\n")).contains("zip"));
    assert!(matches!(project::from_bytes(b"hello, world"), Err(ProjectError::NotAProject(_))));
    assert!(matches!(project::from_bytes(&[0xff, 0xfe, 0x00]), Err(ProjectError::NotAProject(_))));
    // Line ends as Git on Windows may write them read the same.
    let (a, _) = project::from_bytes(text.as_bytes()).unwrap();
    let (b, _) = project::from_bytes(text.replace('\n', "\r\n").as_bytes()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn names_with_quotes_backslashes_newlines_and_other_scripts_round_trip() {
    let (mut doc, extra) = project::from_bytes(enclosure_text().as_bytes()).unwrap();
    doc.name = "Gehäuse \"A\" \\ 外壳\nline 2\ttab".into();
    let bytes = project::to_bytes(&doc, &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("name = \"Gehäuse \\\"A\\\" \\\\ 外壳\\nline 2\\ttab\"\n"), "{text}");
    assert_eq!(project::from_bytes(&bytes).unwrap().0, doc);
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    tenon_io::cmd::run(s, id, &p, None).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn session_from(text: &str) -> Session {
    let mut s = Session::default();
    s.replace_document(project::from_bytes(text.as_bytes()).unwrap().0, None);
    s
}

#[test]
fn a_parameter_change_and_a_new_feature_merge_cleanly() {
    let dir = scratch("merge");
    let base = enclosure_text();
    // Ours: the box made taller. Theirs: a drain hole cut through the floor.
    let mut ours = session_from(&base);
    run(&mut ours, "param.set", json!({ "name": "H", "equation": "35 mm" }));
    let mut theirs = session_from(&base);
    let sk = run(&mut theirs, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
    run(&mut theirs, "sketch.circle", json!({ "sketch": sk, "cx": 40, "cy": 30, "r": 3 }));
    run(&mut theirs, "model.extrude", json!({ "sketch": sk, "distance": 2, "operation": "cut" }));
    let write = |name: &str, doc: &Document| {
        let p = dir.join(name);
        std::fs::write(&p, project::to_bytes(doc, &Map::new()).unwrap()).unwrap();
        p
    };
    let (b, o, t) = (dir.join("base.tenon"), write("ours.tenon", ours.document()), write("theirs.tenon", theirs.document()));
    std::fs::write(&b, &base).unwrap();
    let Ok(out) = std::process::Command::new("git").arg("merge-file").arg("-p").args([&o, &b, &t]).output() else {
        eprintln!("git is not installed: the merge was not tried");
        return;
    };
    assert_eq!(out.status.code(), Some(0), "conflicts:\n{}", String::from_utf8_lossy(&out.stdout));
    let (merged, _) = project::from_bytes(&out.stdout).unwrap();
    assert_eq!(merged.parameter_values()["H"], 35.0);
    assert_eq!(merged.features().len(), ours.document().features().len() + 2);
    // It regenerates: taller, with the hole.
    let mut s = Session::default();
    s.replace_document(merged, None);
    let mut k = OcctKernel::new();
    assert!(s.regen(&mut k).first_error().is_none());
    // `diff` from ours to the merge: only theirs.
    std::fs::write(dir.join("merged.tenon"), &out.stdout).unwrap();
    let d = diff::diff_files(&o, &dir.join("merged.tenon")).unwrap();
    assert!(d.changes.iter().all(|c| c.op == '+' && c.section == "Features"), "{}", d.text());
    assert_eq!(d.changes.len(), 2, "{}", d.text());
}

#[test]
fn diff_names_parameters_and_features_and_what_follows_from_what() {
    let base = enclosure_text();
    let mut s = session_from(&base);
    run(&mut s, "param.set", json!({ "name": "H", "equation": "35 mm" }));
    run(&mut s, "param.set", json!({ "name": "t", "comment": "floor and walls" }));
    run(&mut s, "feature.rename", json!({ "feature": 3, "name": "Round edges" }));
    run(&mut s, "feature.move", json!({ "feature": 3, "before": 5 }));
    run(&mut s, "feature.suppress", json!({ "feature": 5 }));
    run(&mut s, "feature.delete", json!({ "feature": 13 }));
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    let a = serde_json::to_value(project::from_bytes(base.as_bytes()).unwrap().0).unwrap();
    let b = serde_json::to_value(s.document()).unwrap();
    let d = diff::diff_documents("part", &a, &b);
    let text = d.text();
    // Lines compared with their spacing (items are lined up) collapsed.
    let lines: Vec<String> = text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    let has = |line: &str| assert!(lines.iter().any(|l| l == line), "no line {line:?} in\n{text}");
    has("Parameters");
    has("~ H 30 mm -> 35 mm (outside height)");
    has("~ t comment wall thickness -> floor and walls");
    has("Features");
    has("~ Extrusion1 [2] extent distance 30 -> 35 (from H)");
    has("~ Extrusion2 [7] extent distance 23 -> 28 (from H)");
    has("~ Work Plane2 [8] distance 25 -> 30 (from H)");
    has("~ Sketch4 [12] 1 point moved; dimension d15 15 -> 17.5 (from H)");
    has("> Round edges [3] moved after Shell1 [4]");
    has("~ Round edges [3] renamed Fillet1 -> Round edges");
    has("~ Work Plane1 [5] suppressed");
    has("- Hole2 [13]");
    assert!(lines.iter().any(|l| l.starts_with(&format!("+ Sketch5 [{sk}] sketch, "))), "{text}");
    // The same, read by tools; and nothing for a document compared with itself.
    let j = d.json();
    assert_eq!(j["same"], false);
    assert!(j["changes"].as_array().unwrap().iter().any(|c| c["op"] == "moved" && c["item"] == "Round edges [3]"));
    assert!(diff::diff_documents("part", &a, &a).same());
    assert_eq!(diff::diff_documents("part", &a, &a).text(), "No changes.");
}

#[test]
fn diff_reports_assemblies_and_drawings_and_refuses_mixed_kinds() {
    let pivot = root().join("examples/m3-pivot/pivot.tenonasm");
    let a = serde_json::to_value(asm::from_bytes(&std::fs::read(&pivot).unwrap(), Path::new(".")).unwrap()).unwrap();
    let mut b = a.clone();
    b["components"][1]["placement"]["origin"]["x"] = json!(99.0);
    b["components"][1]["visible"] = json!(false);
    b["relationships"][0]["name"] = json!("Hinge");
    b["explode"].as_array_mut().unwrap().pop();
    let text = diff::diff_documents("assembly", &a, &b).text();
    assert!(text.contains("Components\n") && text.contains("visible yes -> no; moved"), "{text}");
    assert!(text.contains("Relationships\n") && text.contains("name ") && text.contains(" -> Hinge"), "{text}");
    assert!(text.contains("Explode\n  - 4"), "{text}");

    let plate = root().join("examples/m4-plate/plate.tenondrw");
    let a = serde_json::to_value(drw::from_bytes(&std::fs::read(&plate).unwrap(), Path::new(".")).unwrap()).unwrap();
    let mut b = a.clone();
    b["props"]["title"] = json!("BASE PLATE");
    b["views"][1]["scale"] = json!(2.0);
    b["views"][2]["center"]["x"] = json!(1.0);
    b["annotations"].as_array_mut().unwrap().remove(0);
    let text = diff::diff_documents("drawing", &a, &b).text();
    assert!(text.contains("Document\n  ~ props title MOUNTING PLATE -> BASE PLATE"), "{text}");
    assert!(text.contains("~ VIEW2 [2]  scale 1 -> 2"), "{text}");
    assert!(text.contains("~ VIEW3 [3]  moved"), "{text}");
    assert!(text.contains("Annotations\n  - [1]"), "{text}");

    let e = diff::diff_files(&pivot, &plate).unwrap_err().to_string();
    assert!(e.contains("assembly") && e.contains("drawing"), "{e}");
    // From scripts and MCP: the same, as a command.
    let mut s = Session::default();
    let r = tenon_io::cmd::run(&mut s, "file.diff", &json!({ "a": plate, "b": plate }), None).unwrap();
    assert_eq!((r["same"].clone(), r["text"].clone()), (json!(true), json!("No changes.")));
}

#[test]
fn upgraded_assemblies_solve_with_every_relationship_holding() {
    let mut k = OcctKernel::new();
    let mut n = 0;
    for (old, now) in fixtures().into_iter().filter(|(o, _)| ext(o) == "tenonasm") {
        let mut s = tenon_assembly::AsmSession::default();
        asm::run(&mut s, "asm.open", &json!({ "path": now }), Some(&mut k as &mut dyn Kernel)).unwrap();
        let before: Vec<_> = s.assembly().components.iter().map(|c| c.placement).collect();
        let solved = s.update().unwrap();
        assert!(solved.converged && solved.failing.is_empty(), "{}: {:?}", now.display(), solved.failing);
        // Solving again moves nothing: the rounded placements are where the parts belong.
        for (c, b) in s.assembly().components.iter().zip(&before) {
            assert!(c.placement.origin().dist(b.origin()) < 1e-6, "{}: {} moved", now.display(), c.name);
        }
        // And the version-1 file's components are where the upgrade put them.
        let (v1, _) = asm::open(&old).unwrap();
        for (c, b) in v1.components.iter().zip(&before) {
            assert!(c.placement.origin().dist(b.origin()) < 1e-6, "{}: {}", now.display(), c.name);
        }
        n += 1;
    }
    assert_eq!(n, 2);
}

#[test]
fn a_placed_dimension_keeps_its_place_on_its_own_record() {
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let lines = run(&mut s, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }))["lines"].clone();
    // One dimension placed by hand, one left beside its geometry.
    let placed = run(
        &mut s,
        "sketch.constrain",
        json!({ "sketch": sk, "constraint": { "type": "length", "line": lines[0], "value": 40.0 }, "at_x": 20, "at_y": -8.123456789012345 }),
    )["constraint"]
        .as_u64()
        .unwrap();
    run(&mut s, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": lines[1], "value": 20.0 } }));
    let extra = Map::new();
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    // Its place is on the dimension's line, rounded like every stored number; the other
    // dimension's line is as it always was.
    assert!(
        text.contains(&format!(
            "{{ id = {placed}, type = \"length\", line = {}, value = 40.0, at = {{ x = 20.0, y = -8.123456789 }} }},\n",
            lines[0]
        )),
        "{text}"
    );
    assert!(text.contains(&format!("type = \"length\", line = {}, value = 20.0 }},\n", lines[1])), "{text}");
    assert!(!text.contains("places"), "no list of places beside the dimensions: {text}");
    // Read back, it is the same document to the stored precision, and saves the same again.
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes);
    let places = serde_json::to_value(&back).unwrap()["features"][0]["kind"]["sketch"]["places"].clone();
    assert_eq!(places, json!([[placed, { "x": 20.0, "y": -8.123456789 }]]));
    // Moved: the diff says a dimension moved, and nothing else changed.
    let before = serde_json::to_value(s.document()).unwrap();
    run(&mut s, "sketch.place_dimension", json!({ "sketch": sk, "constraint": placed, "x": 25, "y": -14 }));
    let d = diff::diff_documents("part", &before, &serde_json::to_value(s.document()).unwrap());
    assert!(d.text().contains("1 dimension moved"), "{}", d.text());
    assert!(!d.text().contains("constraint"), "{}", d.text());
    // A place on something that is not a dimension is refused, with the record's line.
    let bad = text.replacen("type = \"horizontal\", line", "type = \"horizontal\", at = { x = 1.0, y = 2.0 }, line", 1);
    assert_ne!(bad, text, "the rectangle has a horizontal constraint to damage");
    let m = damaged(&bad);
    assert!(m.contains("not a dimension"), "{m}");
}

#[test]
fn material_appearance_and_design_table_are_written_as_text_and_read_back() {
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let lines = run(&mut s, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }))["lines"].clone();
    run(&mut s, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": lines[0], "value": 40.0 } }));
    run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": 10 }));
    let extra = Map::new();
    // A part with none of the three says nothing of them: files written before are as they were.
    let plain = String::from_utf8(project::to_bytes(s.document(), &extra).unwrap()).unwrap();
    assert!(!plain.contains("material") && !plain.contains("appearance") && !plain.contains("[table]"), "{plain}");

    run(&mut s, "document.material", json!({ "name": "Brass" }));
    run(&mut s, "document.appearance", json!({ "color": "#D04030" }));
    run(&mut s, "table.create", json!({ "columns": ["d0", "d1"], "row": "Small" }));
    run(&mut s, "table.add_row", json!({ "name": "Large", "values": { "d0": 60, "d1": 25.5 } }));
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    // Each on lines of its own, before the parameters.
    assert!(text.contains("\nappearance = \"#d04030\"\n"), "{text}");
    assert!(text.contains("\n[material]\nname = \"Brass\"\ndensity = 8.5\ncolor = \"#c9a64a\"\n"), "{text}");
    let table = "\n[table]\nactive = \"Small\"\ncolumns = [\"d0\", \"d1\"]\nrows = [\n  { name = \"Small\", values = [40.0, 10.0] },\n  { name = \"Large\", values = [60.0, 25.5] },\n]\n";
    assert!(text.contains(table), "{text}");
    assert!(text.find("[table]").unwrap() < text.find("[parameters]").unwrap(), "{text}");
    // Read back it is the same part, and saves the same again.
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(&back, s.document());
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes);

    // The diff says what changed about the part as a whole, and which size moved.
    let before = serde_json::to_value(s.document()).unwrap();
    run(&mut s, "document.material", json!({ "name": "PLA" }));
    run(&mut s, "document.appearance", json!({ "color": null }));
    run(&mut s, "table.set", json!({ "row": "Large", "column": "d0", "value": 70 }));
    run(&mut s, "table.add_row", json!({ "name": "Tall", "values": { "d1": 80 } }));
    run(&mut s, "table.activate", json!({ "row": "Large" }));
    let d = diff::diff_documents("part", &before, &serde_json::to_value(s.document()).unwrap()).text();
    for line in [
        "~ material Brass (8.5 g/cm^3, #c9a64a) -> PLA (1.24 g/cm^3, #4f9fd8)",
        "~ appearance #d04030 -> the material's",
        "~ active row  Small -> Large",
        "~ Large       d0 60 -> 70",
        "+ Tall        d0 = 40, d1 = 80",
    ] {
        assert!(d.contains(line), "no `{line}` in:\n{d}");
    }
    // Deleted, or made where there was none.
    run(&mut s, "table.delete", json!({}));
    let after = serde_json::to_value(s.document()).unwrap();
    assert!(diff::diff_documents("part", &before, &after).text().contains("Design Table\n  - table"));
    let made = diff::diff_documents("part", &after, &before).text();
    assert!(made.contains("+ table  d0, d1 at row Small") && made.contains("+ Large  d0 = 60, d1 = 25.5"), "{made}");

    // Damaged by hand or by a merge: refused, saying what is wrong.
    let m = damaged(&text.replace("density = 8.5", "density = -1.0"));
    assert!(m.contains("density"), "{m}");
    let m = damaged(&text.replace("appearance = \"#d04030\"", "appearance = \"red\""));
    assert!(m.contains("#rrggbb"), "{m}");
    let m = damaged(&text.replace("columns = [\"d0\", \"d1\"]", "columns = [\"d0\", \"d9\"]"));
    assert!(m.contains("`d9`, which is not a parameter"), "{m}");
    let m = damaged(&text.replace("active = \"Small\"", "active = \"Medium\""));
    assert!(m.contains("active row `Medium`"), "{m}");
    let m = damaged(&text.replace("values = [60.0, 25.5]", "values = [60.0]"));
    assert!(m.contains("row `Large`"), "{m}");
    let m = damaged(&text.replace("name = \"Large\", values", "name = \"Small\", values"));
    assert!(m.contains("two rows named `Small`"), "{m}");
    // The active row is the part as it stands. A file edited so that the two disagree is read
    // with the part's own values, and written back whole.
    let edited = text.replace("values = [40.0, 10.0]", "values = [41.0, 10.0]");
    assert_ne!(edited, text);
    let (read, _) = project::from_bytes(edited.as_bytes()).unwrap();
    assert_eq!(read.table().unwrap().rows[0].values, [40.0, 10.0]);
    assert_eq!(project::to_bytes(&read, &extra).unwrap(), bytes);
}

#[test]
fn a_region_of_a_divided_sketch_is_named_by_side_in_the_file() {
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let c = run(&mut s, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 10 }))["circle"].as_u64().unwrap();
    let l = run(&mut s, "sketch.line", json!({ "sketch": sk, "x1": -15, "y1": 0, "x2": 15, "y2": 0 }))["line"].as_u64().unwrap();
    // Above the line (its left) and below it; and the circle whole, as files always wrote it.
    run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": 5, "regions": [{ "left": [c, l] }] }));
    run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": 2, "regions": [{ "left": [c], "right": [l] }] }));
    run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": 1, "regions": [[c]] }));
    let extra = Map::new();
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains(&format!("\nregions = {{ keys = [{{ left = [{c}, {l}] }}] }}\n")), "{text}");
    assert!(text.contains(&format!("\nregions = {{ keys = [{{ left = [{c}], right = [{l}] }}] }}\n")), "{text}");
    assert!(text.contains(&format!("\nregions = {{ keys = [[{c}]] }}\n")), "{text}");
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(&back, s.document());
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes);
    // The diff says which feature changed what it is made from.
    let before = serde_json::to_value(s.document()).unwrap();
    let first = s.document().features()[1].id.0;
    let mut kind = serde_json::to_value(&s.document().features()[1].kind).unwrap();
    kind["regions"] = json!({ "keys": [{ "left": [c], "right": [l] }] });
    run(&mut s, "feature.update", json!({ "feature": first, "kind": kind }));
    let d = diff::diff_documents("part", &before, &serde_json::to_value(s.document()).unwrap()).text();
    assert!(d.contains("Extrusion1") && d.contains("regions"), "{d}");
    // A key that is neither a list of curves nor sides is refused, with the feature's line.
    let m = damaged(&text.replacen(&format!("{{ left = [{c}, {l}] }}"), &format!("{{ left = [{c}], inside = [{l}] }}"), 1));
    assert!(m.contains("feature 2 (Extrusion1)"), "{m}");
}

#[test]
fn a_driven_dimension_says_so_on_its_own_record() {
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let r = run(&mut s, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }));
    run(&mut s, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "fix", "point": r["corners"][0] } }));
    run(&mut s, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": r["lines"][0], "value": 40.0 } }));
    run(&mut s, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": r["lines"][1], "value": 20.0 } }));
    let extra = Map::new();
    let plain = String::from_utf8(project::to_bytes(s.document(), &extra).unwrap()).unwrap();
    assert!(!plain.contains("driven"), "a sketch without one is written as it always was: {plain}");
    // The top side, driven and placed by hand.
    let top = json!({ "type": "length", "line": r["lines"][2], "value": 1.0 });
    let made = run(&mut s, "sketch.constrain", json!({ "sketch": sk, "constraint": top, "driven": true, "at_x": 20, "at_y": 28 }));
    let id = made["constraint"].as_u64().unwrap();
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let record =
        format!("{{ id = {id}, type = \"length\", line = {}, value = 40.0, driven = true, at = {{ x = 20.0, y = 28.0 }} }},\n", r["lines"][2]);
    assert!(text.contains(&record), "{text}");
    assert_eq!(text.matches("driven").count(), 1, "nothing but the record says so: {text}");
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(&back, s.document());
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes);
    // Its parameter has a name, like any dimension's.
    assert!(text.contains(&format!("{{ name = \"d2\", target = {{ kind = \"dimension\", sketch = {sk}, constraint = {id} }} }}")), "{text}");

    // Made driving in the file by hand (`driven = false`, or the field taken out): one
    // dimension too many is not an error of the file; the sketch is read as it was written.
    for edited in [text.replace("driven = true, ", "driven = false, "), text.replace("driven = true, ", "")] {
        let (doc, _) = project::from_bytes(edited.as_bytes()).unwrap();
        assert!(doc.sketch(tenon_model::FeatureId(sk as u32)).unwrap().driven().next().is_none());
    }
    // Neither yes nor no, or on what is no dimension: refused, with the sketch's line.
    let m = damaged(&text.replace("driven = true", "driven = \"maybe\""));
    assert!(m.contains("feature 1 (Sketch1)"), "{m}");
    let m = damaged(&text.replacen("type = \"horizontal\", line", "type = \"horizontal\", driven = true, line", 1));
    assert!(m.contains("marked driven but is not a dimension"), "{m}");
    // The diff says the dimension changed over.
    let before = serde_json::to_value(s.document()).unwrap();
    run(&mut s, "sketch.set_driven", json!({ "sketch": sk, "constraint": 6, "driven": true }));
    let d = diff::diff_documents("part", &before, &serde_json::to_value(s.document()).unwrap()).text();
    assert!(d.contains("Sketch1") && d.contains("dimension d0 now driven"), "{d}");
}
