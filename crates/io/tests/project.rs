//! Project files and file commands.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;
use std::io::Write;
use std::path::PathBuf;

use serde_json::{Map, Value, json};
use tenon_io::project::{self, ProjectError};
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::{Document, Session};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-io-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(s: &mut Session, k: Option<&mut OcctKernel>, id: &str, p: Value) -> Value {
    tenon_io::cmd::run(s, id, &p, k.map(|k| k as &mut dyn Kernel)).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// A plate with a hole, built from document commands only.
fn plate(s: &mut Session) {
    let sk = run(s, None, "sketch.create", json!({}))["feature"].as_u64().unwrap();
    run(s, None, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 30, "y2": 20 }));
    run(s, None, "sketch.circle", json!({ "sketch": sk, "cx": 15, "cy": 10, "r": 3 }));
    run(s, None, "model.extrude", json!({ "sketch": sk, "distance": 5 }));
}

fn zip_with(name: &str, body: &[u8]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zw = zip::ZipWriter::new(&mut buf);
    zw.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
    zw.write_all(body).unwrap();
    zw.finish().unwrap();
    buf.into_inner()
}

#[test]
fn round_trip_keeps_the_document_and_unknown_fields() {
    let mut s = Session::default();
    plate(&mut s);
    let bytes = project::to_bytes(s.document(), &Map::new()).unwrap();
    let (doc, extra) = project::from_bytes(&bytes).unwrap();
    assert_eq!(&doc, s.document());
    assert!(extra.is_empty());

    // A field from a newer minor revision survives open + save.
    let file = json!({ "format": "tenon", "version": 1, "document": doc, "future_field": { "x": 1 } });
    let with_extra = zip_with("project.json", &serde_json::to_vec(&file).unwrap());
    let (doc2, extra2) = project::from_bytes(&with_extra).unwrap();
    assert_eq!(extra2.get("future_field"), Some(&json!({ "x": 1 })));
    let resaved = project::to_bytes(&doc2, &extra2).unwrap();
    let (_, extra3) = project::from_bytes(&resaved).unwrap();
    assert_eq!(extra3, extra2);
}

#[test]
fn save_open_and_export_commands() {
    let dir = scratch("cmds");
    let mut k = OcctKernel::new();
    let mut s = Session::default();
    plate(&mut s);
    assert!(s.is_dirty());
    let file = dir.join("plate.tenon");
    run(&mut s, None, "file.save", json!({ "path": file.to_str().unwrap() }));
    assert!(!s.is_dirty());

    let mut t = Session::default();
    let opened = run(&mut t, None, "file.open", json!({ "path": file.to_str().unwrap() }));
    assert_eq!(opened["features"], 2);
    assert_eq!(t.document(), s.document());

    let step = dir.join("plate.step");
    let stl = dir.join("plate.stl");
    run(&mut t, Some(&mut k), "export.step", json!({ "path": step.to_str().unwrap() }));
    run(&mut t, Some(&mut k), "export.stl", json!({ "path": stl.to_str().unwrap() }));
    let mut k2 = OcctKernel::new();
    let shapes = k2.import_step(&std::fs::read(&step).unwrap()).unwrap();
    let v = k2.mass_properties(shapes[0], 1.0).unwrap().volume;
    assert!((v - (600.0 - 9.0 * PI) * 5.0).abs() < 1e-6, "{v}");
    assert!(tenon_io::stl::binary_triangle_count(&std::fs::read(&stl).unwrap()).unwrap() > 20);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_files_are_rejected_with_reasons() {
    assert!(matches!(project::from_bytes(b"not a zip at all"), Err(ProjectError::NotAProject(_))));
    assert!(matches!(project::from_bytes(&zip_with("other.json", b"{}")), Err(ProjectError::NotAProject(_))));
    assert!(matches!(project::from_bytes(&zip_with("project.json", br#"{"format":"other","version":1}"#)), Err(ProjectError::NotAProject(_))));
    assert!(matches!(project::from_bytes(&zip_with("project.json", b"{ broken json")), Err(ProjectError::Damaged(_))));
    let too_new = json!({ "format": "tenon", "version": 99, "document": Document::default() });
    assert!(matches!(project::from_bytes(&zip_with("project.json", &serde_json::to_vec(&too_new).unwrap())), Err(ProjectError::TooNew(99))));
    // A sketch line pointing at a missing point: valid JSON, invalid model.
    let mut s = Session::default();
    plate(&mut s);
    let good = serde_json::to_string(&json!({ "format": "tenon", "version": 1, "document": s.document() })).unwrap();
    // (Point 1 is the projected origin; the plate's first line starts at point 2.)
    let broken = good.replacen("\"start\":2", "\"start\":9999", 1);
    assert_ne!(broken, good);
    assert!(matches!(project::from_bytes(&zip_with("project.json", broken.as_bytes())), Err(ProjectError::Damaged(_))));
    // Truncated file.
    let bytes = project::to_bytes(s.document(), &Map::new()).unwrap();
    assert!(project::from_bytes(&bytes[..bytes.len() / 2]).is_err());
    // Missing file through the command.
    let mut t = Session::default();
    assert!(tenon_io::cmd::run(&mut t, "file.open", &json!({ "path": "C:/definitely/missing.tenon" }), None).is_err());
    assert_eq!(t.document(), &Document::default());
}

#[test]
fn what_is_not_a_plain_file_is_no_document() {
    let dir = std::env::temp_dir().join(format!("tenon-not-a-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let e = project::read_file(&dir).unwrap_err().to_string();
    assert!(e.contains("is not a file") || e.contains("cannot read"), "{e}");
    // (Its length is nothing to go by, and it never ends.)
    #[cfg(unix)]
    {
        let e = project::open(std::path::Path::new("/dev/zero")).unwrap_err().to_string();
        assert!(e.contains("is not a file"), "{e}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_is_refused_over_what_changed_since_it_was_read_until_it_is_allowed() {
    let dir = std::env::temp_dir().join(format!("tenon-seen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("seen.tenon");
    let doc = tenon_model::Document::default();
    let extra = serde_json::Map::new();
    project::save(&file, &doc, &extra).unwrap();
    // Saved again with nothing between: no question.
    project::save(&file, &doc, &extra).unwrap();
    // Another program writes it.
    let theirs = format!("{}\n# theirs\n", std::fs::read_to_string(&file).unwrap());
    std::fs::write(&file, &theirs).unwrap();
    let e = project::save(&file, &doc, &extra).unwrap_err();
    assert!(matches!(e, project::ProjectError::ChangedOnDisk(_)), "{e}");
    assert!(e.to_string().contains("seen.tenon"), "{e}");
    assert_eq!(project::take_refused_save().as_deref(), Some(file.as_path()));
    assert_eq!(project::take_refused_save(), None, "once");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), theirs);
    // Reading it again makes it what is known; or the user says.
    project::open(&file).unwrap();
    project::save(&file, &doc, &extra).unwrap();
    std::fs::write(&file, &theirs).unwrap();
    project::may_overwrite(&file);
    project::save(&file, &doc, &extra).unwrap();
    assert_ne!(std::fs::read_to_string(&file).unwrap(), theirs);
    let _ = std::fs::remove_dir_all(&dir);
}
