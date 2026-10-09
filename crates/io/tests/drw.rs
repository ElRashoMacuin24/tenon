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

    // Inside the file (text): the model path relative to the drawing, with forward slashes.
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains(
            "
format = \"tenon-drawing\"
version = 2
"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "
standard = \"iso\"
"
        ),
        "{text}"
    );
    let models: Vec<&str> = text.lines().filter(|l| l.starts_with("model = ")).collect();
    assert_eq!(models, ["model = \"parts/plate.tenon\""; 2], "{text}");

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

/// Changes `plate.tenon`'s thickness parameter `t` on disk, as another program would.
fn set_thickness_on_disk(file: &Path, t: f64) {
    let (doc, extra) = tenon_io::project::open(file).unwrap();
    let mut s = tenon_model::Session::default();
    s.replace_document(doc, None);
    tenon_io::cmd::run(&mut s, "param.set", &json!({ "name": "t", "equation": t.to_string() }), None).unwrap();
    tenon_io::project::save(file, s.document(), &extra).unwrap();
}

fn thickness(s: &DrwSession, key: &str) -> f64 {
    match s.models.get(key) {
        Some(DrwModel::Part(p)) => p.document().parameter_values()["t"],
        _ => panic!("no part {key}"),
    }
}

#[test]
fn model_files_changed_on_disk_are_read_again_unless_changed_in_the_drawing() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m4-plate");
    let dir = scratch("changed");
    for f in ["plate.tenon", "pin.tenon", "plate-pins.tenonasm", "plate.tenondrw"] {
        std::fs::copy(src.join(f), dir.join(f)).unwrap();
    }
    let mut s = DrwSession::default();
    drw::run(&mut s, "drw.open", &json!({ "path": dir.join("plate.tenondrw").to_str().unwrap() }), None).unwrap();
    let plate = tenon_io::asm::part_key(&dir.join("plate.tenon"));
    let generation = s.generation;
    // Nothing changed: nothing is read.
    assert_eq!(drw::reload_changed(&mut s), drw::Changed::default());
    assert_eq!(s.generation, generation);

    // Another program saves the plate thicker: read again, and the views will be computed anew.
    std::thread::sleep(std::time::Duration::from_millis(20));
    set_thickness_on_disk(&dir.join("plate.tenon"), 25.0);
    let changed = drw::reload_changed(&mut s);
    let mut reloaded = changed.reloaded.clone();
    reloaded.sort();
    assert_eq!(reloaded, ["plate-pins.tenonasm".to_string(), "plate.tenon".to_string()], "the assembly uses the plate too: {changed:?}");
    assert!(changed.kept.is_empty());
    assert!(s.generation > generation);
    assert_eq!(thickness(&s, &plate), 25.0);
    assert_eq!(drw::reload_changed(&mut s), drw::Changed::default(), "once only");

    // Changed in the drawing and not saved: the file's change does not overwrite it.
    if let Some(DrwModel::Part(p)) = s.models.get_mut(&plate) {
        tenon_io::cmd::run(p, "param.set", &json!({ "name": "t", "equation": "30" }), None).unwrap();
    }
    std::thread::sleep(std::time::Duration::from_millis(20));
    set_thickness_on_disk(&dir.join("plate.tenon"), 40.0);
    let changed = drw::reload_changed(&mut s);
    assert_eq!(changed.kept, ["plate.tenon".to_string()], "{changed:?}");
    assert_eq!(thickness(&s, &plate), 30.0);
    // Saving the drawing writes the drawing's plate; that is not read back as a change.
    drw::run(&mut s, "drw.save", &json!({ "path": dir.join("plate.tenondrw").to_str().unwrap() }), None).unwrap();
    assert!(drw::reload_changed(&mut s).reloaded.iter().all(|f| f != "plate.tenon"));
    assert_eq!(thickness(&s, &plate), 30.0);
}

#[test]
fn drawings_start_from_templates_and_save_as_templates() {
    let dir = scratch("drawing-templates");
    // A drawing with two sheets, properties, a note and views (of a model that need not exist).
    let mut d = sample(&dir);
    let second = d.add_sheet(tenon_drawing::sheets::size_named("A3").unwrap());
    d.props.company = "ACME".into();
    d.props.drawn_by = "JD".into();
    let id = d.take_annotation_id();
    d.annotations
        .push(Annotation { id, kind: AnnotKind::Note { sheet: second, at: Vec2::new(30.0, 30.0), text: "GENERAL NOTES".into(), height: 3.5 } });
    let mut s = DrwSession::default();
    s.replace(d.clone(), Default::default());
    let file = dir.join("acme.tenondrw");
    let r = drw::run(&mut s, "drw.save_template", &json!({ "path": file.to_str().unwrap(), "name": "ACME A3" }), None).unwrap();
    assert_eq!((r["sheets"].as_u64(), r["notes"].as_u64()), (Some(2), Some(2)));

    // A new drawing from it: the same standard, properties, sheets and notes; no views, no
    // dimensions; no model read.
    let mut t = DrwSession::default();
    let r = drw::run(&mut t, "drw.new", &json!({ "name": "Bracket", "template": file.to_str().unwrap() }), None).unwrap();
    assert_eq!(r["sheets"], json!(["A3", "A3"]), "{r}");
    let n = t.drawing();
    assert_eq!((n.name.as_str(), n.standard, n.props.company.as_str(), n.props.drawn_by.as_str()), ("Bracket", Standard::Iso, "ACME", "JD"));
    assert!(n.views.is_empty() && t.models.is_empty());
    assert_eq!(n.sheets.iter().map(|x| &x.title_block).collect::<Vec<_>>(), d.sheets.iter().map(|x| &x.title_block).collect::<Vec<_>>());
    assert_eq!(n.annotations.len(), 2);
    assert!(n.annotations.iter().all(|a| matches!(a.kind, AnnotKind::Note { .. })));
    assert!(n.validate().is_ok());
    // A template brings its own standard and sheets.
    assert!(drw::run(&mut t, "drw.new", &json!({ "template": file.to_str().unwrap(), "size": "B" }), None).is_err());
    assert!(drw::run(&mut t, "drw.new", &json!({ "template": dir.join("none.tenondrw").to_str().unwrap() }), None).is_err());
    assert_eq!(t.drawing().name, "Bracket", "a refused template changes nothing");

    // The templates Tenon ships: ANSI B, third-angle; ISO A3, first-angle.
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/templates");
    for (file, standard, size) in [("ansi-b.tenondrw", Standard::Ansi, "B"), ("iso-a3.tenondrw", Standard::Iso, "A3")] {
        let d = drw::read_drawing_template(&shipped.join(file), "New").unwrap();
        assert_eq!((d.standard, d.sheets.len(), d.sheets[0].size.name.as_str()), (standard, 1, size), "{file}");
        assert!(d.views.is_empty() && d.annotations.is_empty() && d.validate().is_ok(), "{file}");
    }
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
    // Newer than this build, as a zip or as text.
    let too_new = json!({ "format": "tenon-drawing", "version": 3, "drawing": serde_json::to_value(&d).unwrap() });
    assert!(matches!(drw::from_bytes(&zip_with(&serde_json::to_vec(&too_new).unwrap()), &dir), Err(ProjectError::TooNew(3))));
    let text = String::from_utf8(good.clone()).unwrap().replace(
        "
version = 2
",
        "
version = 3
",
    );
    assert!(matches!(drw::from_bytes(text.as_bytes(), &dir), Err(ProjectError::TooNew(3))));
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
