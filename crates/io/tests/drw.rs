//! Drawing files: round trip, model paths relative to the file, damaged and mistaken files.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tenon_drawing::{AnnotKind, Annotation, Drawing, DrwModel, DrwSession, Orientation, Standard, View, ViewKind};
use tenon_geom::Vec2;
use tenon_io::drw;
use tenon_io::project::ProjectError;
use tenon_model::Document;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-io-drw-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn zip_with(body: &[u8]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zw = zip::ZipWriter::new(&mut buf);
    zw.start_file("project.json", zip::write::SimpleFileOptions::default()).unwrap();
    zw.write_all(body).unwrap();
    zw.finish().unwrap();
    buf.into_inner()
}

fn file_with(d: &Value) -> Vec<u8> {
    zip_with(&serde_json::to_vec(&json!({ "format": "tenon-drawing", "version": 1, "drawing": d })).unwrap())
}

/// A first-angle drawing of a part in a sub-folder of `dir`: a base view, a section of it and a
/// note.
fn sample(dir: &Path) -> Drawing {
    let mut d = Drawing::new("Sample", Standard::Iso);
    let sheet = d.sheets[0].id;
    let model = tenon_io::asm::part_key(&dir.join("parts").join("plate.tenon"));
    let base = d.take_view_id();
    let view = |id, name: &str, kind| View {
        id,
        sheet,
        name: name.into(),
        model: model.clone(),
        kind,
        scale: 0.5,
        center: Vec2::new(100.0, 120.0),
        hidden: true,
        tangent: false,
        centerlines: true,
        label: false,
    };
    d.views.push(view(base, "VIEW1", ViewKind::Base { orientation: Orientation::Top }));
    let sec = d.take_view_id();
    d.views.push(view(sec, "A", ViewKind::Section { parent: base, a: Vec2::new(-5.0, 20.0), b: Vec2::new(90.0, 20.0), flip: true }));
    let id = d.take_annotation_id();
    d.annotations
        .push(Annotation { id, kind: AnnotKind::Note { sheet, at: Vec2::new(20.0, 20.0), text: "Ø8 THRU — 2 PLACES".into(), height: 3.5 } });
    d
}

#[test]
fn drawings_round_trip_with_model_paths_relative_to_the_file() {
    let dir = scratch("round");
    let d = sample(&dir);
    assert!(d.validate().is_ok());
    let file = dir.join("plate.tenondrw");
    drw::save(&file, &d).unwrap();

    // Inside the file: the model path relative to the drawing, with forward slashes.
    let mut za = zip::ZipArchive::new(std::fs::File::open(&file).unwrap()).unwrap();
    let json: Value = serde_json::from_reader(za.by_name("project.json").unwrap()).unwrap();
    assert_eq!(json["format"], "tenon-drawing");
    assert_eq!(json["version"], 1);
    assert_eq!(json["drawing"]["standard"], "iso");
    assert_eq!(json["drawing"]["views"][0]["model"], "parts/plate.tenon");
    assert_eq!(json["drawing"]["views"][1]["model"], "parts/plate.tenon");

    // Read back elsewhere: the paths follow the file; everything else is as it was.
    let bytes = std::fs::read(&file).unwrap();
    let other = scratch("other");
    let mut back = drw::from_bytes(&bytes, &other).unwrap();
    assert_eq!(back.views[0].model, tenon_io::asm::part_key(&other.join("parts").join("plate.tenon")));
    back.map_models(|p| p.replace(&*other.to_string_lossy(), &dir.to_string_lossy()));
    assert_eq!(back, d);

    // The model file is not there: opening marks it missing and keeps the drawing.
    let (opened, models) = drw::open(&file).unwrap();
    assert_eq!(opened.views.len(), 2);
    assert!(matches!(models.values().next(), Some(DrwModel::Missing(_))));

    // With the part saved where the drawing expects it, the drawing opens with it.
    std::fs::create_dir_all(dir.join("parts")).unwrap();
    let mut doc = Document::default();
    doc.name = "Plate".into();
    tenon_io::project::save(&dir.join("parts").join("plate.tenon"), &doc, &serde_json::Map::new()).unwrap();
    let (_, models) = drw::open(&file).unwrap();
    assert!(matches!(models.values().next(), Some(DrwModel::Part(_))));
}

#[test]
fn title_block_templates_round_trip_and_bad_ones_are_refused() {
    let dir = scratch("template");
    let mut s = DrwSession::default();
    drw::run(&mut s, "drw.new", &json!({ "name": "T", "size": "A3", "standard": "iso" }), None).unwrap();
    drw::run(&mut s, "drw.sheet.add", &json!({}), None).unwrap();
    let file = dir.join("block.json");
    drw::run(&mut s, "drw.template.save", &json!({ "path": file.to_str().unwrap() }), None).unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(saved["format"], "tenon-title-block");
    assert_eq!(saved["title_block"]["name"], "ISO");

    // A hand-edited template: a fixed company line and a smaller block, on every sheet; undoable.
    let mut edited = saved.clone();
    edited["title_block"]["name"] = json!("ACME");
    edited["title_block"]["fields"] = json!([
        { "key": "text", "label": "ACME WIDGETS", "at": { "x": -120.0, "y": 30.0 }, "height": 5.0 },
        { "key": "title", "label": "TITLE", "at": { "x": -120.0, "y": 14.0 }, "height": 4.0 }
    ]);
    edited["title_block"]["lines"] = json!([{ "a": { "x": -130.0, "y": 10.0 }, "b": { "x": -10.0, "y": 10.0 } }]);
    let acme = dir.join("acme.json");
    std::fs::write(&acme, serde_json::to_vec(&edited).unwrap()).unwrap();
    let r = drw::run(&mut s, "drw.template.apply", &json!({ "path": acme.to_str().unwrap() }), None).unwrap();
    assert_eq!(r["sheets"], 2);
    assert!(s.drawing().sheets.iter().all(|sh| sh.title_block.name == "ACME" && sh.title_block.fields.len() == 2));
    let g = tenon_drawing::annotate::build(s.drawing(), s.drawing().sheets[0].id, &tenon_drawing::Evaluation::default()).0;
    assert!(g.texts.iter().any(|(_, t)| t.text == "ACME WIDGETS"));
    drw::run(&mut s, "drw.undo", &json!({}), None).unwrap();
    assert_eq!(s.drawing().sheets[1].title_block.name, "ISO");

    // Refused, changing nothing: an unknown field, a value out of reach, not a template, newer.
    let try_apply = |s: &mut DrwSession, v: &Value| {
        let f = dir.join("bad.json");
        std::fs::write(&f, serde_json::to_vec(v).unwrap()).unwrap();
        drw::run(s, "drw.template.apply", &json!({ "path": f.to_str().unwrap() }), None)
    };
    let before = s.drawing().clone();
    let mut v = saved.clone();
    v["title_block"]["fields"][0]["key"] = json!("price");
    assert!(try_apply(&mut s, &v).unwrap_err().to_string().contains("price"));
    let mut v = saved.clone();
    v["title_block"]["lines"][0]["a"]["x"] = json!(1e9);
    assert!(try_apply(&mut s, &v).is_err());
    let mut v = saved.clone();
    v["format"] = json!("tenon-drawing");
    assert!(try_apply(&mut s, &v).unwrap_err().to_string().contains("template"));
    let mut v = saved.clone();
    v["version"] = json!(9);
    assert!(try_apply(&mut s, &v).unwrap_err().to_string().contains("newer"));
    assert_eq!(*s.drawing(), before);
}

#[test]
fn damaged_and_mistaken_drawing_files_are_refused() {
    let dir = scratch("bad");
    let d = sample(&dir);
    let good = drw::to_bytes(&d, &dir).unwrap();
    assert!(drw::from_bytes(&good, &dir).is_ok());
    // A part or an assembly is not a drawing, and a drawing is neither.
    let part = tenon_io::project::to_bytes(&Document::default(), &serde_json::Map::new()).unwrap();
    assert!(matches!(drw::from_bytes(&part, &dir), Err(ProjectError::NotAProject(m)) if m.contains("part")));
    assert!(matches!(tenon_io::project::from_bytes(&good), Err(ProjectError::NotAProject(m)) if m.contains("drawing")));
    assert!(matches!(tenon_io::asm::from_bytes(&good, &dir), Err(ProjectError::NotAProject(m)) if m.contains("drawing")));
    // Newer than this build.
    let too_new = json!({ "format": "tenon-drawing", "version": 2, "drawing": serde_json::to_value(&d).unwrap() });
    assert!(matches!(drw::from_bytes(&zip_with(&serde_json::to_vec(&too_new).unwrap()), &dir), Err(ProjectError::TooNew(2))));
    let base = serde_json::to_value(&d).unwrap();
    let damaged = |edit: &dyn Fn(&mut Value)| {
        let mut v = base.clone();
        edit(&mut v);
        drw::from_bytes(&file_with(&v), &dir)
    };
    // A section of a view that is not there, or along a line of no length.
    assert!(matches!(damaged(&|v| v["views"][1]["kind"]["parent"] = json!(9)), Err(ProjectError::Damaged(_))));
    assert!(matches!(damaged(&|v| v["views"][1]["kind"]["b"] = json!({ "x": -5.0, "y": 20.0 })), Err(ProjectError::Damaged(_))));
    // A view on a sheet that is not there; a scale of nothing; repeated ids.
    assert!(matches!(damaged(&|v| v["views"][0]["sheet"] = json!(7)), Err(ProjectError::Damaged(_))));
    assert!(matches!(damaged(&|v| v["views"][0]["scale"] = json!(0.0)), Err(ProjectError::Damaged(_))));
    assert!(matches!(damaged(&|v| v["views"][1]["id"] = json!(1)), Err(ProjectError::Damaged(_))));
    // A note on a missing sheet, a sheet with no size, no sheets at all.
    assert!(matches!(damaged(&|v| v["annotations"][0]["kind"]["sheet"] = json!(3)), Err(ProjectError::Damaged(_))));
    assert!(matches!(damaged(&|v| v["sheets"][0]["size"]["width"] = json!(0.0)), Err(ProjectError::Damaged(_))));
    assert!(matches!(damaged(&|v| v["sheets"] = json!([])), Err(ProjectError::Damaged(_))));
    // Truncated.
    assert!(drw::from_bytes(&good[..good.len() / 2], &dir).is_err());
    // A missing file through the command; the session is unchanged.
    let mut s = DrwSession::default();
    let before = s.drawing().clone();
    assert!(drw::run(&mut s, "drw.open", &json!({ "path": dir.join("nope.tenondrw").to_str().unwrap() }), None).is_err());
    assert_eq!(*s.drawing(), before);
    // A base view of a file that is not there is refused.
    assert!(drw::run(&mut s, "drw.view.base", &json!({ "model": dir.join("nope.tenon").to_str().unwrap() }), None).is_err());
    assert!(s.drawing().views.is_empty());
}
