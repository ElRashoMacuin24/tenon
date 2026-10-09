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
