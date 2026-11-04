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
        "~ material Brass (8.5 g/cm^3, #c9a64a, 110 GPa, 0.34, 125 MPa) -> PLA (1.24 g/cm^3, #4f9fd8, 3.5 GPa, 0.36, 50 MPa)",
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
fn a_part_says_its_number_and_what_it_is_for_parts_lists() {
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }));
    run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": 10 }));
    let extra = Map::new();
    // None at first: the file says nothing of them.
    let plain = String::from_utf8(project::to_bytes(s.document(), &extra).unwrap()).unwrap();
    assert!(!plain.contains("part_number") && !plain.contains("description"), "{plain}");
    let before = serde_json::to_value(s.document()).unwrap();

    // Said; one undo step; said again in part.
    let said = run(&mut s, "document.properties", json!({ "part_number": " BR-0042 ", "description": "Mounting bracket, 3 mm steel" }));
    assert_eq!(said, json!({ "part_number": "BR-0042", "description": "Mounting bracket, 3 mm steel" }));
    s.undo();
    assert_eq!((s.document().part_number.as_deref(), s.document().description.as_deref()), (None, None));
    s.redo();
    run(&mut s, "document.properties", json!({ "description": "Mounting bracket" }));
    assert_eq!((s.document().part_number.as_deref(), s.document().description.as_deref()), (Some("BR-0042"), Some("Mounting bracket")));
    // Refused in words: nothing to say, more than a line, more than 200 characters, a number.
    for (p, told) in [
        (json!({}), "part_number"),
        (json!({ "part_number": "BR\n42" }), "one line"),
        (json!({ "description": "x".repeat(201) }), "200 characters"),
        (json!({ "part_number": 42 }), "must be text"),
    ] {
        let e = s.exec("document.properties", &p, None).unwrap_err().to_string();
        assert!(e.contains(told), "{p}: {e}");
    }

    // In the file after the name, read back the same; the diff says what changed.
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(
        text.contains(
            "
part_number = \"BR-0042\"
description = \"Mounting bracket\"
"
        ),
        "{text}"
    );
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(&back, s.document());
    let d = diff::diff_documents("part", &before, &serde_json::to_value(s.document()).unwrap()).text();
    assert!(d.contains("part number none -> BR-0042") && d.contains("description none -> Mounting bracket"), "{d}");
    // Null takes one away; a file with one on two lines is damaged.
    run(&mut s, "document.properties", json!({ "part_number": null }));
    assert_eq!(s.document().part_number, None);
    let m = damaged(&text.replace("part_number = \"BR-0042\"", "part_number = \"BR\\n0042\""));
    assert!(m.contains("part number"), "{m}");

    // A parts list shows the part's own number and description, and the file's name and the
    // part's name where it has none.
    let row = |number: Option<&str>, description: Option<&str>, row: Option<&str>| tenon_assembly::session::BomRow {
        item: 1,
        part: "bracket".into(),
        key: "bracket.tenon".into(),
        name: "Bracket".into(),
        row: row.map(str::to_owned),
        quantity: 2,
        material: None,
        mass: None,
        volume: None,
        part_number: number.map(str::to_owned),
        description: description.map(str::to_owned),
        components: Vec::new(),
    };
    assert_eq!((row(None, None, None).number(), row(None, None, None).what()), ("bracket".to_owned(), "Bracket".to_owned()));
    let own = row(Some("BR-0042"), Some("Mounting bracket"), None);
    assert_eq!((own.number(), own.what()), ("BR-0042".to_owned(), "Mounting bracket".to_owned()));
    assert_eq!(row(Some("SC-5"), None, Some("M5x16")).number(), "SC-5 (M5x16)");
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
fn a_text_and_the_letters_cut_from_it_are_written_and_read_back() {
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let words = "Tenon \"1\"\nline 2";
    let t = run(&mut s, "sketch.text", json!({ "sketch": sk, "x": 1, "y": 2, "text": words, "height": 4, "angle": 0.5 }))["text"].as_u64().unwrap();
    run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": 1, "regions": [{ "text": t }] }));
    let extra = Map::new();
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains(&format!("\nregions = {{ keys = [{{ text = {t} }}] }}\n")), "{text}");
    assert!(text.contains(r#"type = "text", at = 1, text = "Tenon \"1\"\nline 2", height = 4.0, angle = 0.5,"#), "{text}");
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(&back, s.document());
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes);
}

#[test]
fn what_an_older_build_would_misbuild_is_written_as_a_type_it_refuses() {
    // A revolution turned the other way.
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle", json!({ "sketch": sk, "x1": 10, "y1": 0, "x2": 20, "y2": 10 }));
    run(&mut s, "model.revolve", json!({ "sketch": sk, "axis": "y", "angle": 1.5, "reverse": true }));
    let extra = Map::new();
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("type = \"flipped_revolve\"\n"), "{text}");
    assert!(text.contains("reverse = true\n"), "{text}");
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(&back, s.document());
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes);

    // One the default way is a plain revolve, as ever.
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle", json!({ "sketch": sk, "x1": 10, "y1": 0, "x2": 20, "y2": 10 }));
    run(&mut s, "model.revolve", json!({ "sketch": sk, "axis": "y", "angle": 1.5 }));
    let text = String::from_utf8(project::to_bytes(s.document(), &extra).unwrap()).unwrap();
    assert!(text.contains("type = \"revolve\"\n"), "{text}");

    // A variable fillet and a face fillet, from a constant one, in the file and back; and such a
    // fillet held as `fillet` still reads, as it did before this.
    let constant = enclosure_text();
    assert!(constant.contains("type = \"fillet\"\n"));
    let radii = constant.replacen("type = \"fillet\"\n", "type = \"variable_fillet\"\nradii = [{ at = 1.0, radius = 3.0 }]\n", 1);
    let (doc, _) = project::from_bytes(radii.as_bytes()).unwrap();
    assert!(doc.features().iter().any(|f| matches!(&f.kind, tenon_model::FeatureKind::Fillet(x) if x.radii.len() == 1)));
    let written = String::from_utf8(project::to_bytes(&doc, &extra).unwrap()).unwrap();
    assert!(written.contains("type = \"variable_fillet\"\n") && !written.contains("type = \"fillet\"\n"), "{written}");
    let plain = radii.replacen("type = \"variable_fillet\"\n", "type = \"fillet\"\n", 1);
    assert_eq!(project::from_bytes(plain.as_bytes()).unwrap().0, doc);

    let faces = constant.replacen("type = \"fillet\"\n", "type = \"face_fillet\"\nfaces = [[], []]\n", 1);
    let (doc, _) = project::from_bytes(faces.as_bytes()).unwrap();
    assert!(doc.features().iter().any(|f| matches!(&f.kind, tenon_model::FeatureKind::Fillet(x) if x.faces.is_some())));
    let written = String::from_utf8(project::to_bytes(&doc, &extra).unwrap()).unwrap();
    assert!(written.contains("type = \"face_fillet\"\n"), "{written}");
}

#[test]
fn a_text_in_a_file_is_held_to_what_can_be_outlined() {
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.text", json!({ "sketch": sk, "x": 1, "y": 2, "text": "HHHH", "height": 4 }));
    let text = String::from_utf8(project::to_bytes(s.document(), &Map::new()).unwrap()).unwrap();
    assert!(project::from_bytes(text.as_bytes()).is_ok());
    // A million letters would hang the rebuild; here they are refused at once, in words.
    let long = text.replace("text = \"HHHH\"", &format!("text = \"{}\"", "H".repeat(1_000_000)));
    assert!(damaged(&long).contains("a text has at most 2000 characters"), "{}", damaged(&long));
    let flat = text.replace("height = 4.0", "height = 0.0");
    assert!(damaged(&flat).contains("a text's height must be above 0 mm"), "{}", damaged(&flat));
    // The range of heights is for making one: a height past it does no harm, and opens.
    for height in ["0.005", "20000.0"] {
        project::from_bytes(text.replace("height = 4.0", &format!("height = {height}")).as_bytes()).unwrap();
    }
}

#[test]
fn a_text_scaled_as_far_as_it_goes_is_saved_and_opened() {
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let t = run(&mut s, "sketch.text", json!({ "sketch": sk, "x": 0, "y": 0, "text": "AB", "height": 0.5 }))["text"].clone();
    // Past the limit, it is refused when it is asked for, not when the file is next opened.
    let e = s.exec("sketch.scale", &json!({ "sketch": sk, "entities": [t], "cx": 0, "cy": 0, "factor": 0.01 }), None).unwrap_err().to_string();
    assert!(e.contains("from 0.01 to 10000 mm, not 0.005"), "{e}");
    run(&mut s, "sketch.scale", json!({ "sketch": sk, "entities": [t], "cx": 0, "cy": 0, "factor": 0.02 }));
    let bytes = project::to_bytes(s.document(), &Map::new()).unwrap();
    project::from_bytes(&bytes).unwrap();
}

#[test]
fn a_name_in_a_file_is_held_to_a_length() {
    let text = enclosure_text();
    let name = |n: usize| format!("name = \"{}\"", "N".repeat(n));
    let first = text.lines().find(|l| l.starts_with("name = ")).unwrap().to_owned();
    // What a command can have given it opens, as it did: 256 bytes.
    assert!(project::from_bytes(text.replacen(&first, &name(256), 1).as_bytes()).is_ok());
    // A name of a million bytes is no name, and is said so rather than drawn.
    let said = damaged(&text.replacen(&first, &name(1_000_000), 1));
    assert!(said.contains("the name of the part") && said.contains("1000000 bytes"), "{said}");
    assert!(said.len() < 200, "{said}");
    // So too a feature's.
    let feature = text.lines().filter(|l| l.starts_with("name = ")).nth(1).unwrap().to_owned();
    let said = damaged(&text.replacen(&feature, &name(2000), 1));
    assert!(said.contains("the name of a feature"), "{said}");
}

#[test]
fn a_parameter_in_a_file_is_held_to_a_length() {
    let mut s = Session::default();
    run(&mut s, "param.add", json!({ "name": "width", "equation": "10 mm", "comment": "as wide" }));
    let text = String::from_utf8(project::to_bytes(s.document(), &Map::new()).unwrap()).unwrap();
    let with = |what: &str, by: String| text.replace(what, &by);
    // What the Parameters window would have to draw, and has not the graphics memory for.
    let cases = [
        ("\"width\"", format!("\"{}\"", "w".repeat(1_000_000)), "the name of a parameter"),
        ("\"10 mm\"", format!("\"{}\"", "1".repeat(1_000_000)), "the equation of `width`"),
        ("\"as wide\"", format!("\"{}\"", "c".repeat(1_000_000)), "the comment of `width`"),
    ];
    for (what, by, said) in &cases {
        let m = damaged(&with(what, by.clone()));
        assert!(m.contains(said) && m.contains("1000000 bytes") && m.len() < 200, "{m}");
    }
    // What commands could have left, and more, opens: an equation made long by a rename, and a
    // long comment given by a script before there was a limit.
    let long = with("\"10 mm\"", format!("\"{}\"", "1".repeat(32_000)));
    project::from_bytes(long.as_bytes()).unwrap();
    project::from_bytes(with("\"as wide\"", format!("\"{}\"", "c".repeat(5_000))).as_bytes()).unwrap();
    // A command takes no more of a comment than it means to.
    let e = s.exec("param.add", &json!({ "name": "w2", "equation": "1", "comment": "c".repeat(1001) }), None).unwrap_err().to_string();
    assert!(e.contains("the comment is too long"), "{e}");
}

#[test]
fn a_sketch_shown_by_the_user_says_so_on_its_record() {
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 5 }));
    run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": 5 }));
    let extra = Map::new();
    let before = String::from_utf8(project::to_bytes(s.document(), &extra).unwrap()).unwrap();
    assert!(!before.contains("visible"), "{before}");
    run(&mut s, "feature.visibility", json!({ "feature": sk, "visible": true }));
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("name = \"Sketch1\"\nsuppressed = false\nvisible = true\ntype = \"sketch\""), "{text}");
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(&back, s.document());
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

#[test]
fn projected_edges_centre_lines_and_diameters_to_them_are_written_and_read_back() {
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let r = run(&mut s, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }));
    let x = run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": 10 }))["feature"].as_u64().unwrap();
    let top = json!({ "origin": { "type": "cap", "feature": x, "end": "end" }, "fingerprint": { "surface": "plane", "direction": { "x": 0.0, "y": 0.0, "z": 1.0 }, "centroid": { "x": 20.0, "y": 10.0, "z": 10.0 }, "area": 800.0 } });
    let on = run(&mut s, "sketch.create", json!({ "face": top, "project_origin": false }))["feature"].as_u64().unwrap();
    let extra = Map::new();
    let plain = String::from_utf8(project::to_bytes(s.document(), &extra).unwrap()).unwrap();
    assert!(
        !plain.contains("projections") && !plain.contains("reference") && !plain.contains("centerline"),
        "nothing of them where there is none: {plain}"
    );

    // The top's front edge shown in the sketch (said by the caller: there is no kernel here), a
    // centre line of the sketch's own, and a diameter from the line to a point.
    let edge = json!({
        "faces": [{ "type": "cap", "feature": x, "end": "end" }, { "type": "side", "feature": x, "curve": r["lines"][0] }],
        "fingerprint": { "mid": { "x": 20.0, "y": 0.0, "z": 10.0 }, "length": 40.0 },
    });
    let said = json!([{ "type": "line", "start": { "x": 0.0, "y": 0.0 }, "end": { "x": 40.0, "y": 0.0 } }]);
    let shown = run(&mut s, "sketch.project", json!({ "sketch": on, "edges": [edge], "geometry": said }));
    let front = shown["entities"][0].as_u64().unwrap();
    let axis = run(&mut s, "sketch.line", json!({ "sketch": on, "x1": 0, "y1": 10, "x2": 40, "y2": 10 }));
    run(&mut s, "sketch.centerline", json!({ "sketch": on, "entity": axis["line"] }));
    let pt = run(&mut s, "sketch.point", json!({ "sketch": on, "x": 12, "y": 14 }))["point"].as_u64().unwrap();
    // A sketch point beside it (DEC-117).
    run(&mut s, "sketch.point", json!({ "sketch": on, "x": 30, "y": 14, "center": false }));
    // An ellipse: its three points (DEC-120).
    let ellipse =
        run(&mut s, "sketch.ellipse", json!({ "sketch": on, "cx": 20, "cy": 30, "x": 30, "y": 30, "minor": 4 }))["ellipse"].as_u64().unwrap();
    let across = run(
        &mut s,
        "sketch.constrain",
        json!({ "sketch": on, "constraint": { "type": "linear_diameter", "point": pt, "axis": axis["line"], "value": 6.0 } }),
    );
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    // The line and its two ends say they are reference geometry; the centre line says so; the
    // diameter is a dimension of its own kind; and the sketch lists what it shows of the part.
    assert_eq!(text.matches("reference = true").count(), 3, "{text}");
    assert_eq!(text.matches("centerline = true").count(), 1, "{text}");
    assert_eq!(text.matches("sketch_point = true").count(), 1, "{text}");
    assert!(text.contains(&format!("{{ id = {ellipse}, type = \"ellipse\", center = ")), "{text}");
    assert!(text.contains(&format!("{{ id = {front}, type = \"line\", start = ")), "{text}");
    let record = format!("{{ id = {}, type = \"linear_diameter\", point = {pt}, axis = {}, value = 6.0 }}", across["constraint"], axis["line"]);
    assert!(text.contains(&record), "{text}");
    assert!(text.contains("projections = [") && text.contains(&format!("entity = {front}")), "{text}");
    // (Solved coordinates are written to nine decimals, so the sketch read back is compared by
    // what was put in, and by saving the same again.)
    let (back, _) = project::from_bytes(&bytes).unwrap();
    let on_id = tenon_model::FeatureId(on as u32);
    assert_eq!(back.projections(on_id), s.document().projections(on_id));
    let (was, now) = (s.document().sketch(on_id).unwrap(), back.sketch(on_id).unwrap());
    for (id, e) in was.entities() {
        let read = now.entity(id).unwrap();
        assert_eq!(
            (read.reference, read.centerline, read.construction, read.sketch_point),
            (e.reference, e.centerline, e.construction, e.sketch_point),
            "{id}"
        );
    }
    assert_eq!(now.constraints().collect::<Vec<_>>(), was.constraints().collect::<Vec<_>>());
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes);
    back.validate().unwrap();

    // The diff says an edge was projected, and when it no longer is.
    let before = serde_json::to_value(s.document()).unwrap();
    run(&mut s, "sketch.break_link", json!({ "sketch": on, "entities": [front] }));
    let after = serde_json::to_value(s.document()).unwrap();
    assert!(
        diff::diff_documents("part", &before, &after).text().contains("1 edge no longer projected"),
        "{}",
        diff::diff_documents("part", &before, &after).text()
    );
    assert!(diff::diff_documents("part", &after, &before).text().contains("1 edge projected"));
    // A file that says an edge is shown by an entity that is not reference geometry is refused.
    let broken = text.replacen("reference = true", "reference = false", 3);
    let e = project::from_bytes(broken.as_bytes()).unwrap_err().to_string();
    assert!(e.contains("is said to be a projected edge but is not reference geometry"), "{e}");
}

#[test]
fn a_sheet_metal_part_is_written_as_text_and_read_back() {
    let mut s = Session::default();
    let extra = Map::new();
    let plain = String::from_utf8(project::to_bytes(s.document(), &extra).unwrap()).unwrap();
    assert!(!plain.contains("sheet_metal") && !plain.contains("Thickness"), "nothing of it in a part that is not sheet metal: {plain}");

    // The rule, a face, and a flange on one edge of it (named by its two faces: there is no
    // kernel here).
    run(&mut s, "sheet.defaults", json!({ "thickness": 1.5, "bend_radius": "Thickness * 2", "corner": "square" }));
    let before = serde_json::to_value(s.document()).unwrap();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let r = run(&mut s, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 30 }));
    let face = run(&mut s, "sheet.face", json!({ "sketch": sk }))["feature"].as_u64().unwrap();
    let edge = json!({
        "faces": [{ "type": "cap", "feature": face, "end": "end" }, { "type": "side", "feature": face, "curve": r["lines"][1] }],
        "fingerprint": { "mid": { "x": 40.0, "y": 15.0, "z": 1.5 }, "length": 30.0 },
    });
    run(&mut s, "sheet.flange", json!({ "edges": [edge.clone()], "height": 12, "angle": 1.25, "position": "adjacent", "width": 20, "radius": 2 }));
    // A hem, a contour flange along an open profile, and a flange that is not mitred.
    run(&mut s, "sheet.hem", json!({ "edges": [edge.clone()], "gap": 0.75, "length": 6, "offsets": [2, 3] }));
    let profile = run(&mut s, "sketch.create", json!({ "plane": "xz", "project_origin": false }))["feature"].as_u64().unwrap();
    let first = run(&mut s, "sketch.line", json!({ "sketch": profile, "x1": 0, "y1": 0, "x2": 30, "y2": 0 }));
    run(&mut s, "sketch.line", json!({ "sketch": profile, "start": first["end"], "x2": 30, "y2": 20 }));
    run(&mut s, "sheet.contour_flange", json!({ "sketch": profile, "distance": 50, "position": "flipped", "direction": "symmetric", "radius": 2.5 }));
    run(&mut s, "sheet.flange", json!({ "edges": [edge.clone()], "height": 12, "miter": false }));
    // A flange whose corners have a seam, a gap and a relief of their own, and a corner seam
    // between two edges.
    run(
        &mut s,
        "sheet.flange",
        json!({ "edges": [edge.clone()], "height": 9, "seam": "reverse_overlap", "overlap": 0.5, "gap": 0.75, "relief": "square", "relief_size": 5 }),
    );
    let other = json!({
        "faces": [{ "type": "cap", "feature": face, "end": "end" }, { "type": "side", "feature": face, "curve": r["lines"][2] }],
        "fingerprint": { "mid": { "x": 20.0, "y": 30.0, "z": 1.5 }, "length": 40.0 },
    });
    run(&mut s, "sheet.corner_seam", json!({ "edges": [edge.clone(), other.clone()], "seam": "overlap", "gap": 1.25 }));
    // A cut across bends, through and to a depth, from a sketch that shows an edge of the part
    // laid flat (Project Flat Pattern).
    let across = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.circle", json!({ "sketch": across, "cx": 38, "cy": 15, "r": 4 }));
    let laid = json!([{ "type": "line", "start": { "x": 52.0, "y": 0.0 }, "end": { "x": 52.0, "y": 30.0 } }]);
    run(&mut s, "sketch.project", json!({ "sketch": across, "edges": [other.clone()], "geometry": laid, "flat": true }));
    run(&mut s, "sheet.cut_across_bend", json!({ "sketch": across }));
    run(&mut s, "sheet.cut_across_bend", json!({ "sketch": across, "distance": 0.75 }));
    // A bend between two faces: by the rule and as it comes, and one with a radius, a kind of
    // double bend and a fixed edge of its own.
    run(&mut s, "sheet.bend", json!({ "edges": [edge.clone(), other.clone()] }));
    run(&mut s, "sheet.bend", json!({ "edges": [edge.clone(), other.clone()], "radius": 1.5, "double": "90_degree", "flip_fixed": true }));
    // A fold along the one line of its sketch as it comes, and one with a line named and
    // everything of its own; a contour flange along an edge; a cut normal, through and to a
    // depth; a corner rip with a seam of its own, and one of two corners as the rule has them.
    let folding = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let along = run(&mut s, "sketch.line", json!({ "sketch": folding, "x1": 10, "y1": 0, "x2": 10, "y2": 30 }));
    run(&mut s, "sheet.fold", json!({ "sketch": folding }));
    run(
        &mut s,
        "sheet.fold",
        json!({ "sketch": folding, "line": along["line"], "angle": 1.0, "radius": 2.5, "location": "start", "flip_side": true, "flip_direction": true }),
    );
    run(&mut s, "sheet.contour_flange", json!({ "sketch": profile, "edges": [edge.clone()], "width": 20 }));
    run(&mut s, "sheet.cut_normal", json!({ "sketch": across }));
    run(&mut s, "sheet.cut_normal", json!({ "sketch": across, "distance": 0.5 }));
    run(&mut s, "sheet.corner_rip", json!({ "edges": [edge.clone()], "seam": "overlap", "overlap": 0.25, "gap": 0.6 }));
    run(&mut s, "sheet.corner_rip", json!({ "edges": [edge.clone(), other.clone()] }));
    // A bend on sharp edges, by the rule and with a radius of its own.
    run(&mut s, "sheet.sharp_bend", json!({ "edges": [edge.clone(), other.clone()] }));
    run(&mut s, "sheet.sharp_bend", json!({ "edges": [edge.clone()], "radius": 1.25 }));
    run(&mut s, "sheet.flat_pattern", json!({}));
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    // The rule is a table of its own; its sizes are parameters, each with its equation; the
    // features are records like any others, with only what is not the usual written out.
    assert!(text.contains("[sheet_metal]\nrelief = \"straight\"\ncorner = \"square\"\nflat = true\n"), "{text}");
    for (name, equation) in [
        ("Thickness", "1.5 mm"),
        ("BendRadius", "Thickness * 2"),
        ("BendReliefDepth", "Thickness * 0.5"),
        ("KFactor", "0.44"),
        ("GapSize", "Thickness"),
    ] {
        assert!(text.contains(&format!("name = \"{name}\", equation = \"{equation}\"")), "{name}: {text}");
    }
    assert!(text.contains("type = \"sheet_face\"") && text.contains("type = \"flange\""), "{text}");
    let flange = text.split("type = \"flange\"").nth(1).unwrap();
    for field in ["angle = 1.25", "height = 12.0", "position = \"adjacent\"", "radius = 2.0", "width = { centered = { width = 20.0 } }"] {
        assert!(flange.contains(field), "{field}: {flange}");
    }
    // A corner's own seam, gap and relief are written where it has them, and only there.
    assert_eq!((text.matches("seam = ").count(), text.matches("overlap = ").count(), text.matches("relief_size = ").count()), (3, 2, 1), "{text}");
    let seamed = text.split("type = \"corner_seam\"").nth(1).unwrap();
    for field in ["seam = \"overlap\"", "gap = 1.25"] {
        assert!(seamed.contains(field), "{field}: {seamed}");
    }
    let own = text.split("height = 9.0").nth(1).unwrap();
    for field in ["seam = \"reverse_overlap\"", "overlap = 0.5", "gap = 0.75", "relief = \"square\"", "relief_size = 5.0"] {
        assert!(own.contains(field), "{field}: {own}");
    }
    // A flange is mitred unless it says not; the hem and the contour flange say what they are.
    assert_eq!(text.matches("miter = false").count(), 1, "{text}");
    let hem = text.split("type = \"hem\"").nth(1).unwrap();
    for field in ["shape = { single = { gap = 0.75, length = 6.0 } }", "width = { offset = { start = 2.0, end = 3.0 } }"] {
        assert!(hem.contains(field), "{field}: {hem}");
    }
    let contour = text.split("type = \"contour_flange\"").nth(1).unwrap();
    for field in ["distance = 50.0", "direction = \"symmetric\"", "position = \"flipped\"", "radius = 2.5"] {
        assert!(contour.contains(field), "{field}: {contour}");
    }
    // A bend says its two edges, and what it has of its own where it has it.
    let bends: Vec<&str> = text.split("type = \"bend\"").skip(1).map(|c| c.split("[[").next().unwrap()).collect();
    assert!(bends.len() == 2 && !bends[0].contains("radius") && !bends[0].contains("double") && !bends[0].contains("flip_fixed"), "{bends:?}");
    for field in ["radius = 1.5", "double = \"90_degree\"", "flip_fixed = true"] {
        assert!(bends[1].contains(field), "{field}: {}", bends[1]);
    }
    // A fold says its sketch and its angle, and the rest where it is not as it comes.
    let folds: Vec<&str> = text.split("type = \"fold\"").skip(1).map(|c| c.split("[[").next().unwrap()).collect();
    assert!(folds.len() == 2 && folds[0].contains("angle = 1.5707963267948966"), "{folds:?}");
    for field in ["line = ", "radius", "location", "flip_side", "flip_direction"] {
        assert!(!folds[0].contains(field), "{field}: {}", folds[0]);
    }
    for field in ["line = ", "angle = 1.0", "radius = 2.5", "location = \"start\"", "flip_side = true", "flip_direction = true"] {
        assert!(folds[1].contains(field), "{field}: {}", folds[1]);
    }
    // A contour flange along edges says them and how much of them, and no distance.
    let along_edges = text.split("type = \"contour_flange\"").nth(2).unwrap().split("[[").next().unwrap();
    assert!(
        along_edges.contains("edges = ") && along_edges.contains("width = { centered = { width = 20.0 } }") && !along_edges.contains("distance"),
        "{along_edges}"
    );
    assert!(!contour.split("[[").next().unwrap().contains("edges"), "{contour}");
    // A cut normal is its sketch, and its depth where it has one.
    let normals: Vec<&str> = text.split("type = \"cut_normal\"").skip(1).map(|c| c.split("[[").next().unwrap()).collect();
    assert!(normals.len() == 2 && !normals[0].contains("distance") && normals[1].contains("distance = 0.5"), "{normals:?}");
    // A corner rip is its edges, and a seam, an overlap and a gap where it has them.
    let rips: Vec<&str> = text.split("type = \"corner_rip\"").skip(1).map(|c| c.split("[[").next().unwrap()).collect();
    assert!(rips.len() == 2 && rips.iter().all(|r| r.contains("edges = ") && !r.contains("relief")), "{rips:?}");
    for field in ["seam = \"overlap\"", "overlap = 0.25", "gap = 0.6"] {
        assert!(rips[0].contains(field) && !rips[1].contains(field.split(' ').next().unwrap()), "{field}: {rips:?}");
    }
    // A bend on sharp edges is its edges, and its radius where it has one.
    let sharp: Vec<&str> = text.split("type = \"sharp_bend\"").skip(1).map(|c| c.split("[[").next().unwrap()).collect();
    assert!(
        sharp.len() == 2 && sharp.iter().all(|b| b.contains("edges = ")) && !sharp[0].contains("radius") && sharp[1].contains("radius = 1.25"),
        "{sharp:?}"
    );
    // A cut across bends is its sketch, and its depth where it has one; an edge shown laid flat
    // says so (the rule's own `flat = true` is the flat pattern).
    let cuts: Vec<&str> = text.split("type = \"cut_across_bend\"").skip(1).map(|c| c.split("[[").next().unwrap()).collect();
    assert!(cuts.len() == 2 && !cuts[0].contains("distance") && cuts[1].contains("distance = 0.75"), "{cuts:?}");
    assert_eq!(text.matches("flat = true").count(), 2, "{text}");
    // Read back, it is the same part, and saves as the same bytes.
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(back.sheet_metal(), s.document().sheet_metal());
    assert_eq!(back.sheet_sizes().unwrap(), s.document().sheet_sizes().unwrap());
    assert_eq!(back.sheet_sizes().unwrap().bend_radius, 3.0);
    assert_eq!(back.features().len(), 24);
    assert_eq!(back.projections(tenon_model::FeatureId(across as u32)), s.document().projections(tenon_model::FeatureId(across as u32)));
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes);
    back.validate().unwrap();

    // The diff says the part became sheet metal and what the rule's changes are.
    let empty = serde_json::to_value(Document::default()).unwrap();
    let made = diff::diff_documents("part", &empty, &before).text();
    assert!(made.contains("now a sheet metal part (bend relief straight, corner relief square)") && made.contains("Thickness"), "{made}");
    let after = serde_json::to_value(s.document()).unwrap();
    run(&mut s, "sheet.defaults", json!({ "corner": "round", "relief": "round", "thickness": 2 }));
    run(&mut s, "sheet.flat_pattern", json!({ "on": false }));
    let changed = diff::diff_documents("part", &after, &serde_json::to_value(s.document()).unwrap()).text();
    for line in ["sheet metal bend relief straight -> round", "sheet metal corner relief square -> round", "flat pattern deleted", "Thickness"] {
        assert!(changed.contains(line), "{line}: {changed}");
    }
    // A part saved before there was a gap between mitred flanges has none written: it opens,
    // and the gap is a thickness.
    let older: String = text
        .lines()
        .filter(|l| !l.contains("GapSize"))
        .map(|l| {
            format!(
                "{l}
"
            )
        })
        .collect();
    assert!(older.len() < text.len());
    let (old, _) = project::from_bytes(older.as_bytes()).unwrap();
    assert_eq!(old.sheet_sizes().unwrap().gap, 1.5);
    // A file edited into something that cannot be is refused as damaged, saying what is wrong.
    let e = project::from_bytes(text.replace("angle = 1.25", "angle = 4.0").as_bytes()).unwrap_err().to_string();
    assert!(e.contains("the flange angle must be from 1 to 179 degrees"), "{e}");
    let e = project::from_bytes(text.replace("corner = \"square\"", "corner = \"wavy\"").as_bytes()).unwrap_err().to_string();
    assert!(e.contains("wavy"), "{e}");
    let e = project::from_bytes(text.replace("gap = 0.75", "gap = 0.0").as_bytes()).unwrap_err().to_string();
    assert!(e.contains("the hem's gap must be from"), "{e}");
    let e = project::from_bytes(text.replace("distance = 50.0", "distance = -1.0").as_bytes()).unwrap_err().to_string();
    assert!(e.contains("the contour flange's distance must be more than 0"), "{e}");
    let e = project::from_bytes(text.replace("overlap = 0.5", "overlap = 2.0").as_bytes()).unwrap_err().to_string();
    assert!(e.contains("from 0 to 1"), "{e}");
}

#[test]
fn a_stress_study_and_a_materials_strength_are_written_as_text_and_read_back() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let mut go =
        |s: &mut Session, id: &str, p: Value| tenon_io::cmd::run(s, id, &p, Some(&mut k as &mut dyn Kernel)).unwrap_or_else(|e| panic!("{id}: {e}"));
    go(&mut s, "model.box", json!({ "width": 100, "depth": 20, "height": 10 }));
    let extra = Map::new();
    // A part with no study says nothing of studies: files written before are as they were.
    let plain = String::from_utf8(project::to_bytes(s.document(), &extra).unwrap()).unwrap();
    assert!(!plain.contains("study") && !plain.contains("strength"), "{plain}");

    let face = |s: &mut Session, go: &mut dyn FnMut(&mut Session, &str, Value) -> Value, way: &str| {
        go(s, "model.find", json!({ "what": "faces", "facing": way }))["found"][0]["reference"].clone()
    };
    let (left, right, top) = (face(&mut s, &mut go, "left"), face(&mut s, &mut go, "right"), face(&mut s, &mut go, "top"));
    go(&mut s, "document.material", json!({ "name": "Aluminium 6061" }));
    go(&mut s, "study.new", json!({ "name": "End load" }));
    go(&mut s, "study.fix", json!({ "faces": [left] }));
    go(&mut s, "study.force", json!({ "faces": [right], "newtons": 150, "direction": "down" }));
    go(&mut s, "study.pressure", json!({ "faces": [top, right], "megapascals": 0.25, "name": "Snow" }));
    go(&mut s, "study.mesh_size", json!({ "size": 2.5 }));
    go(&mut s, "study.new", json!({}));
    // A hole through it, for a pin; the bar free to slide on its bottom; its own weight.
    go(&mut s, "model.cylinder", json!({ "x": 50, "y": 10, "diameter": 8, "height": 10, "operation": "cut" }));
    let bore = go(&mut s, "model.find", json!({ "what": "faces", "kind": "cylinder" }))["found"][0]["reference"].clone();
    let bottom = face(&mut s, &mut go, "bottom");
    go(&mut s, "study.pin", json!({ "study": 2, "faces": [bore], "tangential": true }));
    go(&mut s, "study.frictionless", json!({ "study": 2, "faces": [bottom] }));
    go(&mut s, "study.gravity", json!({ "study": 2 }));
    go(&mut s, "study.set", json!({ "study": 2, "item": 2, "suppressed": true }));
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    // The material's strength with the material; each study a record of its own after the
    // features, its supports and loads within it.
    assert!(text.contains("\n[material]\nname = \"Aluminium 6061\"\ndensity = 2.7\ncolor = \"#c3c8ce\"\n"), "{text}");
    assert!(text.contains("youngs_modulus = 69.0") && text.contains("poisson = 0.33") && text.contains("yield_strength = 275.0"), "{text}");
    assert!(text.contains("\n[[study]]\nid = 1\nname = \"End load\"\n"), "{text}");
    assert!(text.contains("mesh_size = 2.5"), "{text}");
    for said in [
        "kind = \"fixed\"",
        "name = \"Fixed1\"",
        "type = \"force\"",
        "newtons = 150.0",
        "name = \"Snow\"",
        "type = \"pressure\"",
        "megapascals = 0.25",
    ] {
        assert!(text.contains(said), "{said} is not in:\n{text}");
    }
    assert!(text.contains("\n[[study]]\nid = 2\nname = \"Study1\"\n"), "{text}");
    for said in [
        "kind = \"pin\"",
        "radial = true",
        "axial = true",
        "tangential = true",
        "kind = \"frictionless\"",
        "suppressed = true",
        "type = \"gravity\"",
        "acceleration = 9810.0",
    ] {
        assert!(text.contains(said), "{said} is not in:\n{text}");
    }
    let studies = &text[text.find("[[study]]").unwrap()..];
    assert_eq!(studies.matches("suppressed").count(), 1, "a support or load that is not suppressed does not say so:\n{studies}");
    assert!(text.find("[[feature]]").unwrap() < text.find("[[study]]").unwrap(), "{text}");
    // Read back it is the same part (but for the rounding of what the kernel measured of
    // each face, as in every reference), and saves the same again.
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes);
    assert_eq!(back.studies().len(), 2);
    let (was, now) = (&s.document().studies()[0], &back.studies()[0]);
    assert_eq!((now.id, &now.name, now.mesh_size), (was.id, &was.name, was.mesh_size));
    assert_eq!(now.loads.iter().map(|l| l.kind).collect::<Vec<_>>(), was.loads.iter().map(|l| l.kind).collect::<Vec<_>>());
    let origins = |st: &tenon_model::study::Study| {
        st.supports.iter().map(|x| &x.faces).chain(st.loads.iter().map(|l| &l.faces)).flatten().map(|f| f.origin).collect::<Vec<_>>()
    };
    assert_eq!(origins(now), origins(was));
    assert_eq!(origins(now).len(), 4);
    let (was, now) = (&s.document().studies()[1], &back.studies()[1]);
    assert_eq!(
        now.supports.iter().map(|x| (x.kind, x.suppressed)).collect::<Vec<_>>(),
        was.supports.iter().map(|x| (x.kind, x.suppressed)).collect::<Vec<_>>()
    );
    assert_eq!(
        now.loads.iter().map(|l| (l.kind, l.faces.len())).collect::<Vec<_>>(),
        was.loads.iter().map(|l| (l.kind, l.faces.len())).collect::<Vec<_>>()
    );
    assert_eq!(back.strength().map(|m| m.yield_strength), Some(275.0));

    // A study damaged by hand is refused with what is wrong with it.
    let broken = text.replace("newtons = 150.0", "newtons = 0.0");
    let err = project::from_bytes(broken.as_bytes()).unwrap_err().to_string();
    assert!(err.contains("other than 0"), "{err}");
    // What changed between two saves of the part is said by diff.
    go(&mut s, "study.force", json!({ "study": 1, "faces": [top], "newtons": 40 }));
    let later = project::to_bytes(s.document(), &extra).unwrap();
    let dir = scratch("study-diff");
    std::fs::write(dir.join("a.tenon"), &bytes).unwrap();
    std::fs::write(dir.join("b.tenon"), &later).unwrap();
    go(&mut s, "study.rename", json!({ "study": 2, "name": "Twisted" }));
    go(&mut s, "study.set", json!({ "study": 2, "item": 2, "suppressed": false }));
    go(&mut s, "study.set", json!({ "study": 2, "item": 1, "axial": false }));
    go(&mut s, "study.set", json!({ "study": 2, "item": 3, "direction": "-y" }));
    go(&mut s, "study.remove", json!({ "study": 1, "item": 3 }));
    go(&mut s, "study.mesh_size", json!({ "study": 1, "size": null }));
    go(&mut s, "study.new", json!({ "name": "Third" }));
    go(&mut s, "document.material", json!({ "name": "Aluminium 6061", "yield_strength": 240 }));
    let later = project::to_bytes(s.document(), &extra).unwrap();
    std::fs::write(dir.join("b.tenon"), &later).unwrap();
    let d = diff::diff_files(&dir.join("a.tenon"), &dir.join("b.tenon")).unwrap();
    let said: Vec<String> = d.changes.iter().map(|c| format!("{} {} {} {}", c.section, c.op, c.item, c.detail)).collect();
    let has = |what: &str| said.iter().any(|l| l.contains(what));
    assert!(has("Studies ~ End load [1]") && has("element size 2.5 mm -> chosen from the part"), "{said:#?}");
    assert!(has("- Snow") && has("+ Force2 (force 40 N square to them on 1 face)"), "{said:#?}");
    assert!(has("Studies ~ Twisted [2] name Study1 -> Twisted"), "{said:#?}");
    assert!(has("Pin1: pin held radial, axial, tangential on 1 face -> pin held radial, tangential on 1 face"), "{said:#?}");
    assert!(has("Frictionless1: frictionless on 1 face, suppressed -> frictionless on 1 face"), "{said:#?}");
    assert!(has("Gravity1: gravity 9810 mm/s^2 along (0, 0, -1) -> gravity 9810 mm/s^2 along (0, -1, 0)"), "{said:#?}");
    assert!(has("Studies + Third [3] 0 supports, 0 loads"), "{said:#?}");
    assert!(has("69 GPa, 0.33, 275 MPa") && has("69 GPa, 0.33, 240 MPa"), "{said:#?}");
    // The same file against itself: nothing.
    assert!(diff::diff_files(&dir.join("b.tenon"), &dir.join("b.tenon")).unwrap().same());
}

#[test]
fn optimize_shape_settings_are_kept_with_the_study() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let mut go =
        |s: &mut Session, id: &str, p: Value| tenon_io::cmd::run(s, id, &p, Some(&mut k as &mut dyn Kernel)).unwrap_or_else(|e| panic!("{id}: {e}"));
    go(&mut s, "model.box", json!({ "width": 100, "depth": 20, "height": 10 }));
    let face = |s: &mut Session, go: &mut dyn FnMut(&mut Session, &str, Value) -> Value, way: &str| {
        go(s, "model.find", json!({ "what": "faces", "facing": way }))["found"][0]["reference"].clone()
    };
    let (left, right) = (face(&mut s, &mut go, "left"), face(&mut s, &mut go, "right"));
    go(&mut s, "study.new", json!({ "name": "Bracket" }));
    go(&mut s, "study.fix", json!({ "faces": [left] }));
    go(&mut s, "study.force", json!({ "faces": [right], "newtons": 150, "direction": "down" }));
    // Kept faces share the study's run of ids (DEC-123), and are named Keep1... unless named.
    let r = go(&mut s, "study.keep", json!({ "faces": [right], "depth": 3, "name": "Bolt" }));
    assert_eq!(r["kept"], 3, "{r}");
    let r = go(&mut s, "study.keep", json!({ "faces": [left] }));
    assert_eq!((r["kept"].as_u64(), r["optimize"]["kept"][1]["name"].as_str()), (Some(4), Some("Keep1")), "{r}");
    let r = go(
        &mut s,
        "study.shape",
        json!({ "remove": 60, "min_member": 4, "resolution": "medium", "symmetry": ["xz"], "sharpen": false, "iterations": 80 }),
    );
    assert_eq!(
        (
            r["optimize"]["remove"].as_f64(),
            r["optimize"]["resolution"].as_str(),
            r["optimize"]["symmetry"].as_u64(),
            r["optimize"]["sharpen"].as_bool()
        ),
        (Some(60.0), Some("medium"), Some(1), Some(false)),
        "{r}"
    );
    let extra = Map::new();
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    // A table of its own under the study's record.
    assert!(text.contains("\n[study.optimize]\nremove = 60.0\nmin_member = 4.0\nresolution = \"medium\"\n"), "{text}");
    for said in ["name = \"Bolt\"", "depth = 3.0", "name = \"Keep1\"", "sharpen = false", "iterations = 80", "symmetry = ["] {
        assert!(text.contains(said), "{said} is not in:\n{text}");
    }
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes);
    let (was, now) = (s.document().studies()[0].optimize.clone().unwrap(), back.studies()[0].optimize.clone().unwrap());
    assert_eq!(
        (now.target, now.min_member, now.resolution, now.sharpen, now.iterations),
        (was.target, was.min_member, was.resolution, was.sharpen, was.iterations)
    );
    assert_eq!(now.symmetry, was.symmetry);
    assert_eq!(
        now.kept.iter().map(|k| (k.id, k.name.clone(), k.depth, k.faces[0].origin)).collect::<Vec<_>>(),
        was.kept.iter().map(|k| (k.id, k.name.clone(), k.depth, k.faces[0].origin)).collect::<Vec<_>>()
    );

    // Changed and taken away as supports and loads are; a support's id is no kept faces.
    let r = go(&mut s, "study.set", json!({ "item": 3, "depth": null, "name": "Bore" }));
    assert_eq!((r["optimize"]["kept"][0]["name"].as_str(), r["optimize"]["kept"][0]["depth"].is_null()), (Some("Bore"), true), "{r}");
    let r = go(&mut s, "study.remove", json!({ "item": 4 }));
    assert_eq!(r["optimize"]["kept"].as_array().unwrap().len(), 1, "{r}");
    // Refused in words, the study as it was.
    let before = s.document().clone();
    for (id, p, said) in [
        ("study.shape", json!({ "remove": 99 }), "from 5 to 95 %"),
        ("study.shape", json!({ "remove": 40, "grams": 10 }), "not both"),
        ("study.shape", json!({ "iterations": 0 }), "from 1 to 300"),
        ("study.shape", json!({ "symmetry": ["xy", "yz", "xz", "xy"] }), "at most 3 planes"),
        ("study.shape", json!({ "resolution": "huge" }), "coarse"),
        ("study.set", json!({ "item": 3, "suppressed": true }), "removed"),
        ("study.set", json!({ "item": 1, "depth": 2 }), "depth"),
        ("study.keep", json!({ "faces": [right], "depth": -1 }), "depth"),
    ] {
        let e = tenon_io::cmd::run(&mut s, id, &p, Some(&mut k as &mut dyn Kernel)).unwrap_err();
        assert!(e.to_string().contains(said), "{id} {p}: {e}");
    }
    assert_eq!(s.document(), &before);
}

#[test]
fn optimize_shape_with_both_targets_or_neither_is_refused_in_words() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let mut go =
        |s: &mut Session, id: &str, p: Value| tenon_io::cmd::run(s, id, &p, Some(&mut k as &mut dyn Kernel)).unwrap_or_else(|e| panic!("{id}: {e}"));
    go(&mut s, "model.box", json!({ "width": 100, "depth": 20, "height": 10 }));
    go(&mut s, "study.new", json!({ "name": "Bracket" }));
    go(&mut s, "study.shape", json!({ "grams": 12.5, "min_member": 4 }));
    let extra = Map::new();
    let bytes = project::to_bytes(s.document(), &extra).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    // Written with its one key, in its place; read back and written the same.
    assert!(text.contains("\n[study.optimize]\ngrams = 12.5\nmin_member = 4.0\n"), "{text}");
    assert!(!text.contains("remove"), "{text}");
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(back.studies()[0].optimize, s.document().studies()[0].optimize);
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes);

    // Both keys, whichever comes first: neither wins, and the file is not read.
    let dir = scratch("shape-targets");
    let one = "grams = 12.5\n";
    for both in ["remove = 40.0\ngrams = 12.5\n", "grams = 12.5\nremove = 40.0\n"] {
        let hand = text.replace(one, both);
        let e = project::from_bytes(hand.as_bytes()).unwrap_err().to_string();
        assert!(e.contains("give `remove` or `grams`, not both"), "{e}");
        // Which study, and where it is, as a feature that does not read is told.
        let line = text[..text.find("[[study]]").unwrap()].lines().count() + 1;
        assert!(e.contains(&format!("line {line}: study 1 (Bracket): ")), "{e}");
        std::fs::write(dir.join("both.tenon"), &hand).unwrap();
        // Comparing files says the same, and of which file.
        let e = diff::diff_files(&dir.join("both.tenon"), &dir.join("both.tenon")).unwrap_err().to_string();
        assert!(e.contains(&format!("both.tenon: line {line}: study 1 (Bracket): give `remove` or `grams`, not both")), "{e}");
    }
    // Neither.
    let e = project::from_bytes(text.replace(one, "").as_bytes()).unwrap_err().to_string();
    assert!(e.contains("[study.optimize] needs `remove` or `grams`") && e.contains("study 1 (Bracket)"), "{e}");
    std::fs::write(dir.join("neither.tenon"), text.replace(one, "")).unwrap();
    let e = diff::diff_files(&dir.join("neither.tenon"), &dir.join("neither.tenon")).unwrap_err().to_string();
    assert!(e.contains("[study.optimize] needs `remove` or `grams`") && e.contains("study 1 (Bracket)"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_face_held_as_a_plane_of_symmetry_in_a_file_opens_and_the_check_says_so() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let mut go =
        |s: &mut Session, id: &str, p: Value| tenon_io::cmd::run(s, id, &p, Some(&mut k as &mut dyn Kernel)).unwrap_or_else(|e| panic!("{id}: {e}"));
    go(&mut s, "model.box", json!({ "width": 100, "depth": 20, "height": 10 }));
    let face = |s: &mut Session, go: &mut dyn FnMut(&mut Session, &str, Value) -> Value, way: &str| {
        go(s, "model.find", json!({ "what": "faces", "facing": way }))["found"][0]["reference"].clone()
    };
    let (left, right) = (face(&mut s, &mut go, "left"), face(&mut s, &mut go, "right"));
    go(&mut s, "document.material", json!({ "name": "Steel, mild" }));
    go(&mut s, "study.new", json!({}));
    go(&mut s, "study.fix", json!({ "faces": [left] }));
    go(&mut s, "study.force", json!({ "faces": [right], "newtons": 150, "direction": "down" }));
    go(&mut s, "study.shape", json!({ "remove": 60 }));
    // As a file edited by hand, or written before faces were refused, has it.
    let mut v = serde_json::to_value(s.document()).unwrap();
    v["studies"][0]["optimize"]["symmetry"] = json!([{ "face": left }]);
    let doc: Document = serde_json::from_value(v).unwrap();
    let extra = Map::new();
    let bytes = project::to_bytes(&doc, &extra).unwrap();
    let (back, _) = project::from_bytes(&bytes).unwrap();
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes, "kept as it was");
    let held = back.studies()[0].optimize.as_ref().unwrap();
    assert!(matches!(held.symmetry.as_slice(), [tenon_model::PlaneRef::Face(_)]), "{held:?}");

    // A stress check is not held up by it; what Optimize Shape needs says it.
    let mut opened = Session::new(back);
    let c = tenon_io::cmd::run(&mut opened, "study.check", &json!({}), Some(&mut k as &mut dyn Kernel)).unwrap();
    assert_eq!((c["ready"].clone(), c["needs"].clone()), (json!(true), json!([])), "{c}");
    assert_eq!(c["shape_needs"], json!([tenon_model::study::FACE_PLANE_REFUSED]), "{c}");
}

#[test]
fn diff_says_what_changed_of_what_optimize_shape_is_asked() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let mut go =
        |s: &mut Session, id: &str, p: Value| tenon_io::cmd::run(s, id, &p, Some(&mut k as &mut dyn Kernel)).unwrap_or_else(|e| panic!("{id}: {e}"));
    go(&mut s, "model.box", json!({ "width": 100, "depth": 20, "height": 10 }));
    let face = |s: &mut Session, go: &mut dyn FnMut(&mut Session, &str, Value) -> Value, way: &str| {
        go(s, "model.find", json!({ "what": "faces", "facing": way }))["found"][0]["reference"].clone()
    };
    let (left, right) = (face(&mut s, &mut go, "left"), face(&mut s, &mut go, "right"));
    let plane = go(&mut s, "work.plane", json!({ "base": "yz", "distance": 50 }))["feature"].as_u64().unwrap();
    go(&mut s, "study.new", json!({}));
    let extra = Map::new();
    let dir = scratch("shape-diff");
    let (a, b) = (dir.join("a.tenon"), dir.join("b.tenon"));
    let write = |s: &Session, to: &Path| std::fs::write(to, project::to_bytes(s.document(), &extra).unwrap()).unwrap();

    // The settings given and taken away as a whole.
    write(&s, &a);
    go(&mut s, "study.shape", json!({ "grams": 12.5 }));
    let kept = go(&mut s, "study.keep", json!({ "faces": [left] }))["kept"].as_u64().unwrap();
    write(&s, &b);
    let whole = "+ Optimize Shape (target come to 12.5 g, thinnest member three elements across, resolution coarse, element size from the resolution, symmetry none, sharpen yes, most rounds as the method needs, 1 set of kept faces)";
    let d = diff::diff_files(&a, &b).unwrap();
    assert!(d.text().contains(whole), "{}", d.text());
    let d = diff::diff_files(&b, &a).unwrap();
    assert!(d.text().contains("- Optimize Shape"), "{}", d.text());

    // Each setting, on its own, is a change: the file against itself is none.
    write(&s, &a);
    assert!(diff::diff_files(&a, &a).unwrap().same());
    let planes = format!("Optimize Shape symmetry none -> XZ, work plane {plane}");
    for (id, p, said) in [
        ("study.shape", json!({ "remove": 60 }), "Optimize Shape target come to 12.5 g -> remove 60 %"),
        ("study.shape", json!({ "min_member": 4 }), "Optimize Shape thinnest member three elements across -> 4 mm"),
        ("study.shape", json!({ "resolution": "fine" }), "Optimize Shape resolution coarse -> fine"),
        ("study.shape", json!({ "element_size": 3 }), "Optimize Shape element size from the resolution -> 3 mm"),
        ("study.shape", json!({ "symmetry": ["xz", { "work": plane }] }), planes.as_str()),
        ("study.shape", json!({ "sharpen": false }), "Optimize Shape sharpen yes -> no"),
        ("study.shape", json!({ "iterations": 80 }), "Optimize Shape most rounds as the method needs -> 80"),
        ("study.set", json!({ "item": kept, "depth": 9 }), "Keep1: 1 face kept, one layer deep -> 1 face kept, 9 mm deep"),
        ("study.set", json!({ "item": kept, "faces": [right] }), "Keep1: 1 face kept, one layer deep, on other faces"),
        ("study.set", json!({ "item": kept, "faces": [left, right] }), "Keep1: 1 face kept, one layer deep -> 2 faces kept, one layer deep"),
        ("study.set", json!({ "item": kept, "name": "Bolt" }), "Bolt: name Keep1 -> Bolt"),
        ("study.keep", json!({ "faces": [right], "depth": 2 }), "+ Keep2 (1 face kept, 2 mm deep)"),
        ("study.remove", json!({ "item": kept }), "- Keep1"),
    ] {
        go(&mut s, id, p.clone());
        write(&s, &b);
        let d = diff::diff_files(&a, &b).unwrap();
        assert!(!d.same() && d.text().starts_with("Studies\n") && d.text().contains(said), "{id} {p}: {}", d.text());
        // And the command says the same as the function.
        let told = go(&mut s, "file.diff", json!({ "a": a, "b": b }));
        assert!(told["same"] == false && told["text"].as_str().unwrap().contains(said), "{id} {p}: {told}");
        go(&mut s, "edit.undo", json!({}));
    }
    // A target as it was, though the file says it another way (a number is a number).
    let text = std::fs::read_to_string(&a).unwrap();
    std::fs::write(&b, text.replace("grams = 12.5", "grams = 12.50000000001")).unwrap();
    assert!(diff::diff_files(&a, &b).unwrap().same());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_of_many_records_is_refused_in_good_time_with_its_line() {
    // Each record's place used to be counted from the top of the file: this took minutes.
    let many = "[[x]]\n".repeat(200_000);
    let text = format!("format = \"tenon\"\nversion = 2\n{many}");
    let started = std::time::Instant::now();
    assert!(project::from_bytes(text.as_bytes()).is_err());
    assert!(started.elapsed().as_secs_f64() < 30.0, "{:?}", started.elapsed());
    // Where each record starts is still right.
    let bad = enclosure_text().replacen("type = \"fillet\"", "type = \"no such thing\"", 1);
    let line = bad.lines().position(|l| l == "[[feature]]").unwrap();
    assert!(line > 0);
    assert!(damaged(&bad).starts_with("line "), "{}", damaged(&bad));
}

#[test]
fn a_file_listing_more_values_than_a_document_can_use_is_refused_before_it_is_read() {
    // Under the limit on a file's size, and far more in memory than it is on disk.
    let list = "1,".repeat(5_000_000);
    let text = format!("format = \"tenon\"\nversion = 2\nx = [{list}]\n");
    assert!(text.len() < 64 * 1024 * 1024);
    assert!(damaged(&text).contains("more than a document can use"), "{}", damaged(&text));
    // A version-1 zip, which is small on disk.
    let json = format!("{{\"format\":\"tenon\",\"version\":1,\"x\":[{}1]}}", "1,".repeat(5_000_000));
    let e = project::from_bytes(&project::zip_json(json.as_bytes()).unwrap()).unwrap_err().to_string();
    assert!(e.contains("more than a document can use"), "{e}");
}

#[test]
fn diff_sees_a_sketch_shown_and_does_not_call_a_marked_point_moved() {
    let scratch = scratch("diff-shown");
    let diff_of = |a: &[u8], b: &[u8]| {
        let (x, y) = (scratch.join("a.tenon"), scratch.join("b.tenon"));
        std::fs::write(&x, a).unwrap();
        std::fs::write(&y, b).unwrap();
        tenon_io::diff::diff_files(&x, &y)
    };
    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let text = |s: &Session| project::to_bytes(s.document(), &Map::new()).unwrap();
    let before = text(&s);
    run(&mut s, "feature.visibility", json!({ "feature": sk, "visible": true }));
    let shown = text(&s);
    let d = diff_of(&before, &shown).unwrap();
    assert!(d.text().contains("shown"), "{}", d.text());
    run(&mut s, "feature.visibility", json!({ "feature": sk, "visible": false }));
    let d = diff_of(&shown, &text(&s)).unwrap();
    assert!(d.text().contains("hidden"), "{}", d.text());

    let mut s = Session::default();
    let sk = run(&mut s, "sketch.create", json!({ "plane": "xy", "project_origin": false }))["feature"].as_u64().unwrap();
    let point = run(&mut s, "sketch.point", json!({ "sketch": sk, "x": 1, "y": 2 }))["point"].as_u64().unwrap();
    let before = text(&s);
    run(&mut s, "sketch.center_point", json!({ "sketch": sk, "entity": point, "on": false }));
    let d = diff_of(&before, &text(&s)).unwrap();
    assert!(!d.text().contains("moved") && d.text().contains("changed"), "{}", d.text());
}
