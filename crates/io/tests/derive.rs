//! Derive and the file (DEC-133): a part's solid brought into another, by a path kept relative to
//! the file, following the other file, failing in words when it is gone, and never deriving itself.
//! And what Emboss, Derive and UCS are in a file: read back to the same bytes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tenon_io::cmd::run as exec;
use tenon_io::project;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    exec(s, id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn feature(v: &Value) -> u64 {
    v["feature"].as_u64().unwrap()
}

/// A folder of its own for one test.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-derive-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn path(p: &Path) -> Value {
    json!(p.to_string_lossy())
}

/// A block w x d x h from (x0, 0, 0), saved at `file`.
fn block_file(file: &Path, x0: f64, w: f64, d: f64, h: f64) {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": x0, "y1": 0, "x2": x0 + w, "y2": d }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": h }));
    run(&mut s, &mut k, "file.save", json!({ "path": path(file) }));
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

fn fails(s: &mut Session, k: &mut OcctKernel) -> String {
    let r = run(s, k, "model.regenerate", json!({}));
    r["error"]["message"].as_str().unwrap_or_else(|| panic!("it should fail: {r}")).to_owned()
}

fn bodies(s: &mut Session, k: &mut OcctKernel) -> Vec<Value> {
    run(s, k, "model.topology", json!({}))["bodies"].as_array().unwrap().clone()
}

fn bbox(b: &Value) -> [f64; 6] {
    let v = |key: &str, i: usize| b["bbox"][key][i].as_f64().unwrap();
    [v("min", 0), v("min", 1), v("min", 2), v("max", 0), v("max", 1), v("max", 2)]
}

#[track_caller]
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6 * (1.0 + b.abs()), "{a} against {b}");
}

#[test]
fn a_part_is_brought_in_scaled_mirrored_joined_or_a_new_solid() {
    let dir = folder("bring");
    let other = dir.join("other.tenon");
    block_file(&other, 10.0, 20.0, 10.0, 5.0);
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let d = feature(&run(&mut s, &mut k, "model.derive", json!({ "path": path(&other) })));
    near(volume(&mut s, &mut k), 1000.0);
    assert_eq!(bodies(&mut s, &mut k).len(), 1);
    assert_eq!(s.document().feature(tenon_model::FeatureId(d as u32)).unwrap().name, "Derive1");

    // Twice as large about the origin: 8 times the volume, its box from x = 20.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.derive", json!({ "path": path(&other), "scale": 2.0 }));
    near(volume(&mut s, &mut k), 8000.0);
    let b = bbox(&bodies(&mut s, &mut k)[0]);
    near(b[0], 20.0);
    near(b[3], 60.0);

    // Mirrored in the YZ plane: from x = -30 to -10, and the same volume, not its negative.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.derive", json!({ "path": path(&other), "mirror": "yz" }));
    near(volume(&mut s, &mut k), 1000.0);
    let b = bbox(&bodies(&mut s, &mut k)[0]);
    near(b[0], -30.0);
    near(b[3], -10.0);

    // Scaled, then mirrored in a UCS's plane at x = 100.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 100 })));
    run(&mut s, &mut k, "model.derive", json!({ "path": path(&other), "scale": 0.5, "mirror": { "ucs": u, "plane": "yz" } }));
    near(volume(&mut s, &mut k), 125.0);
    let b = bbox(&bodies(&mut s, &mut k)[0]);
    near(b[0], 200.0 - 15.0);
    near(b[3], 200.0 - 5.0);

    // Joined to a part that has a solid: one body, overlap counted once. New solid: two bodies.
    let block = |s: &mut Session, k: &mut OcctKernel| {
        let sk = feature(&run(s, k, "sketch.create", json!({ "plane": "xy" })));
        run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 20, "y2": 10 }));
        run(s, k, "model.extrude", json!({ "sketch": sk, "distance": 5 }));
    };
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    run(&mut s, &mut k, "model.derive", json!({ "path": path(&other) }));
    near(volume(&mut s, &mut k), 1000.0 + 500.0);
    assert_eq!(bodies(&mut s, &mut k).len(), 1);
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    block(&mut s, &mut k);
    run(&mut s, &mut k, "model.derive", json!({ "path": path(&other), "operation": "new_body" }));
    near(volume(&mut s, &mut k), 1000.0 + 1000.0);
    assert_eq!(bodies(&mut s, &mut k).len(), 2);

    // What it brings in can be built on: faces have names, and a fillet takes its edges.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.derive", json!({ "path": path(&other) }));
    let top = run(&mut s, &mut k, "model.face_ref", json!({ "at": [20.0, 5.0, 5.0] }));
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "face": top })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 15, "y1": 2, "x2": 20, "y2": 8 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 3 }));
    near(volume(&mut s, &mut k), 1000.0 + 90.0);

    // Refused: an operation it cannot do, a scale out of range, a file that is not there.
    let e = exec(&mut s, "model.derive", &json!({ "path": path(&other), "scale": 0.0 }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("scale"), "{e}");
    let e = exec(&mut s, "model.derive", &json!({ "path": path(&dir.join("nothing.tenon")) }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("nothing.tenon") && e.contains("missing"), "{e}");
    let e = exec(&mut s, "model.derive", &json!({ "path": path(&other), "operation": "cut" }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("operation"), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn it_is_undone_and_redone_edited_and_copied_like_any_feature() {
    let dir = folder("feature");
    let other = dir.join("other.tenon");
    block_file(&other, 10.0, 20.0, 10.0, 5.0);
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let d = feature(&run(&mut s, &mut k, "model.derive", json!({ "path": path(&other) })));
    near(volume(&mut s, &mut k), 1000.0);
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(s.document().features().is_empty());
    run(&mut s, &mut k, "edit.redo", json!({}));
    near(volume(&mut s, &mut k), 1000.0);
    // Edited as a whole.
    let kind = json!({ "type": "derive", "path": other.to_string_lossy(), "scale": 2.0, "operation": "new_body" });
    run(&mut s, &mut k, "feature.update", json!({ "feature": d, "kind": kind }));
    near(volume(&mut s, &mut k), 8000.0);
    // Its scale is a parameter.
    run(&mut s, &mut k, "param.add", json!({ "name": "big", "equation": "3", "unit": "ul" }));
    run(&mut s, &mut k, "feature.update", json!({ "feature": d, "kind": kind, "equations": { "/scale": "big" } }));
    near(volume(&mut s, &mut k), 27000.0);
    // Copied by a pattern: three of it, 100 apart.
    run(&mut s, &mut k, "model.pattern.rect", json!({ "features": [d], "direction": "y", "count": 3, "spacing": 100 }));
    near(volume(&mut s, &mut k), 81000.0);
    // Pointed at another file by the feature's own definition: it is read.
    let second = dir.join("second.tenon");
    block_file(&second, 0.0, 1.0, 1.0, 1.0);
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let kind = json!({ "type": "derive", "path": second.to_string_lossy() });
    run(&mut s, &mut k, "feature.add", json!({ "kind": kind }));
    near(volume(&mut s, &mut k), 1.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn its_path_is_kept_relative_to_the_file_and_its_contents_are_kept_out() {
    let dir = folder("relative");
    std::fs::create_dir_all(dir.join("parts")).unwrap();
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let other = dir.join("parts").join("other.tenon");
    block_file(&other, 0.0, 10.0, 10.0, 10.0);
    let mine = dir.join("work").join("mine.tenon");
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.derive", json!({ "path": path(&other), "scale": 1.5 }));
    run(&mut s, &mut k, "file.save", json!({ "path": path(&mine) }));
    let text = std::fs::read_to_string(&mine).unwrap();
    assert!(text.contains("path = \"../parts/other.tenon\""), "{text}");
    assert!(!text.contains(&*dir.to_string_lossy().replace('\\', "\\\\")), "no full path in the file: {text}");
    assert!(!text.contains("extrude"), "nothing of the other file is in this one: {text}");
    assert!(!s.is_dirty());

    // Read back: the same bytes.
    let (doc, extra) = project::open(&mine).unwrap();
    let mut again = Session::default();
    again.replace_document(doc, None);
    near(volume(&mut again, &mut k), 1000.0 * 1.5f64.powi(3));
    project::save(&mine, again.document(), &extra).unwrap();
    assert_eq!(std::fs::read_to_string(&mine).unwrap(), text);

    // The whole folder moved: it still finds it.
    let moved = dir.with_file_name(format!("{}-moved", dir.file_name().unwrap().to_string_lossy()));
    let _ = std::fs::remove_dir_all(&moved);
    std::fs::rename(&dir, &moved).unwrap();
    let (doc, _) = project::open(&moved.join("work").join("mine.tenon")).unwrap();
    let mut there = Session::default();
    there.replace_document(doc, None);
    near(volume(&mut there, &mut k), 1000.0 * 1.5f64.powi(3));

    // Saved somewhere else: the path is rewritten from there.
    let elsewhere = moved.join("parts").join("near.tenon");
    run(&mut there, &mut k, "file.save", json!({ "path": path(&elsewhere) }));
    let text = std::fs::read_to_string(&elsewhere).unwrap();
    assert!(text.contains("path = \"other.tenon\""), "{text}");
    let _ = std::fs::remove_dir_all(moved);
}

#[test]
fn when_the_other_file_changes_the_part_follows_on_rebuild_and_not_before() {
    let dir = folder("follow");
    let other = dir.join("other.tenon");
    block_file(&other, 0.0, 10.0, 10.0, 10.0);
    let mine = dir.join("mine.tenon");
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.derive", json!({ "path": path(&other) }));
    run(&mut s, &mut k, "file.save", json!({ "path": path(&mine) }));
    near(volume(&mut s, &mut k), 1000.0);

    block_file(&other, 0.0, 10.0, 10.0, 20.0);
    // Until it is rebuilt, what was read stands: it is asked for by rebuilding.
    let m = run(&mut s, &mut k, "model.mass", json!({}));
    near(m["bodies"][0]["volume"].as_f64().unwrap(), 1000.0);
    near(volume(&mut s, &mut k), 2000.0);
    assert!(!s.is_dirty(), "another file changing is no change to this one");

    // Undo after that does not bring back what the file no longer holds.
    let mut with_edit = s;
    run(&mut with_edit, &mut k, "document.rename", json!({ "name": "Mine" }));
    run(&mut with_edit, &mut k, "edit.undo", json!({}));
    let m = run(&mut with_edit, &mut k, "model.mass", json!({}));
    near(m["bodies"][0]["volume"].as_f64().unwrap(), 2000.0);

    // Through another: a part that derives one that derives the changed file.
    let middle = dir.join("middle.tenon");
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.derive", json!({ "path": path(&other), "scale": 2.0 }));
    run(&mut s, &mut k, "file.save", json!({ "path": path(&middle) }));
    let (mut top, mut k2) = (Session::default(), OcctKernel::new());
    run(&mut top, &mut k2, "model.derive", json!({ "path": path(&middle) }));
    near(volume(&mut top, &mut k2), 8.0 * 2000.0);
    block_file(&other, 0.0, 10.0, 10.0, 5.0);
    near(volume(&mut top, &mut k2), 8.0 * 500.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_file_that_is_gone_fails_in_words_and_the_part_still_opens() {
    let dir = folder("missing");
    let other = dir.join("other.tenon");
    block_file(&other, 0.0, 10.0, 10.0, 10.0);
    let mine = dir.join("mine.tenon");
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 50, "y1": 0, "x2": 60, "y2": 10 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 2 }));
    let d = feature(&run(&mut s, &mut k, "model.derive", json!({ "path": path(&other), "operation": "new_body" })));
    run(&mut s, &mut k, "file.save", json!({ "path": path(&mine) }));
    std::fs::remove_file(&other).unwrap();

    let (doc, _) = project::open(&mine).unwrap();
    let mut s = Session::default();
    s.replace_document(doc, None);
    let said = fails(&mut s, &mut k);
    assert!(said.contains("other.tenon") && said.contains("missing") && said.contains("choose the file again"), "{said}");
    let tree = run(&mut s, &mut k, "model.regenerate", json!({}));
    assert_eq!(tree["error"]["feature"].as_u64(), Some(d), "{tree}");
    // What comes before it stands.
    assert_eq!(bodies(&mut s, &mut k).len(), 1);

    // A file that is no part says what it is.
    std::fs::write(&other, b"not a part").unwrap();
    let said = fails(&mut s, &mut k);
    assert!(said.contains("other.tenon") && said.contains("cannot be brought in"), "{said}");

    // Put back, it rebuilds; suppressed, it is left out; deleted, gone.
    block_file(&other, 0.0, 10.0, 10.0, 10.0);
    near(volume(&mut s, &mut k), 200.0 + 1000.0);
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": d }));
    near(volume(&mut s, &mut k), 200.0);
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": d, "suppressed": false }));
    run(&mut s, &mut k, "feature.delete", json!({ "feature": d }));
    near(volume(&mut s, &mut k), 200.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_part_cannot_derive_itself_directly_or_through_another() {
    let dir = folder("itself");
    let a = dir.join("a.tenon");
    let b = dir.join("b.tenon");
    block_file(&a, 0.0, 10.0, 10.0, 10.0);

    // Directly: refused as a command.
    let (doc, _) = project::open(&a).unwrap();
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    s.replace_document(doc, None);
    let e = exec(&mut s, "model.derive", &json!({ "path": path(&a) }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("cannot derive itself") && e.contains("a.tenon"), "{e}");

    // Through another: b derives a, and a is told to derive b.
    let (mut sb, mut kb) = (Session::default(), OcctKernel::new());
    run(&mut sb, &mut kb, "model.derive", json!({ "path": path(&a) }));
    run(&mut sb, &mut kb, "file.save", json!({ "path": path(&b) }));
    let e = exec(&mut s, "model.derive", &json!({ "path": path(&b) }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("cannot derive itself") && e.contains("a.tenon") && e.contains("b.tenon"), "{e}");

    // In files written by hand, or by two sessions: a derives b, b derives a. Each opens, and
    // the feature says what is wrong; nothing goes round for ever.
    let (mut s2, mut k2) = (Session::default(), OcctKernel::new());
    run(&mut s2, &mut k2, "model.derive", json!({ "path": path(&b) }));
    // (This one was not known to be a.tenon when it was told to derive b.)
    run(&mut s2, &mut k2, "file.save", json!({ "path": path(&a) }));
    let said = fails(&mut s2, &mut k2);
    assert!(said.contains("cannot derive itself"), "{said}");
    let (doc, _) = project::open(&a).unwrap();
    let mut s3 = Session::default();
    s3.replace_document(doc, None);
    let said = fails(&mut s3, &mut k2);
    assert!(said.contains("cannot derive itself"), "{said}");

    // A part saved over the file it derives from says so too.
    let c = dir.join("c.tenon");
    block_file(&c, 0.0, 5.0, 5.0, 5.0);
    let (mut s4, mut k4) = (Session::default(), OcctKernel::new());
    run(&mut s4, &mut k4, "model.derive", json!({ "path": path(&c) }));
    run(&mut s4, &mut k4, "file.save", json!({ "path": path(&c) }));
    let said = fails(&mut s4, &mut k4);
    assert!(said.contains("cannot derive itself"), "{said}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_assembly_of_a_part_that_derives_gets_its_solid() {
    let dir = folder("assembly");
    let other = dir.join("other.tenon");
    block_file(&other, 0.0, 10.0, 10.0, 10.0);
    let mine = dir.join("mine.tenon");
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.derive", json!({ "path": path(&other), "scale": 2.0 }));
    run(&mut s, &mut k, "file.save", json!({ "path": path(&mine) }));

    let mut asm = tenon_assembly::AsmSession::default();
    tenon_io::asm::run(&mut asm, "asm.insert", &json!({ "path": path(&mine) }), Some(&mut k)).unwrap();
    let key = tenon_io::asm::part_key(&mine);
    let part = asm.parts.get_mut(&key).unwrap();
    let r = part.session.regen(&mut k);
    assert!(r.first_error().is_none(), "{:?}", r.first_error());
    let v = k.mass_properties(r.bodies[0].shape, 1.0).unwrap().volume;
    near(v, 8000.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn emboss_derive_and_ucs_read_back_to_the_same_bytes() {
    let dir = folder("bytes");
    let other = dir.join("other.tenon");
    block_file(&other, 0.0, 40.0, 40.0, 10.0);
    let mine = dir.join("mine.tenon");
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let d = feature(&run(&mut s, &mut k, "model.derive", json!({ "path": path(&other), "mirror": "xz", "scale": 1.0 })));
    let _ = d;
    let u = feature(&run(&mut s, &mut k, "work.ucs", json!({ "x": 20, "y": -20, "z": 10, "rz": 0.5, "rx": 0.25 })));
    let v = feature(&run(&mut s, &mut k, "work.ucs", json!({ "base": u, "z": 2 })));
    let t = feature(&run(&mut s, &mut k, "work.ucs", json!({ "by": "three_points", "points": ["origin", { "ucs": u }, { "ucs": v }] })));
    let _ = t;
    let sk = feature(&run(&mut s, &mut k, "sketch.create", json!({ "ucs_plane": { "ucs": u, "plane": "xz" } })));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": -5, "y1": -5, "x2": 5, "y2": 5 }));
    run(&mut s, &mut k, "model.emboss", json!({ "sketch": sk, "depth": 1.5, "style": "recess", "taper": 0.1, "reverse": true }));
    run(&mut s, &mut k, "work.axis", json!({ "axis": { "ucs": u, "axis": "z" } }));
    run(&mut s, &mut k, "work.plane", json!({ "base": { "ucs": v, "plane": "yz" }, "distance": 3 }));
    run(&mut s, &mut k, "file.save", json!({ "path": path(&mine) }));
    let bytes = std::fs::read(&mine).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("type = \"ucs\"") && text.contains("type = \"emboss\"") && text.contains("type = \"derive\""), "{text}");
    assert!(text.contains("by = \"offset\"") && text.contains("by = \"three_points\""), "{text}");

    let (doc, extra) = project::open(&mine).unwrap();
    assert_eq!(project::to_bytes(&tenon_io::derive::written(&doc, &mine), &extra).unwrap(), bytes);
    // Unknown to a reader: it refuses the file naming the record, not a crash.
    let again = project::from_bytes(&bytes).unwrap();
    assert!(again.0.validate().is_ok());
    let old = text.replace("type = \"emboss\"", "type = \"not_yet_known\"");
    let err = project::from_bytes(old.as_bytes()).unwrap_err().to_string();
    assert!(err.contains("not_yet_known"), "{err}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_save_takes_no_change_of_the_other_file() {
    let dir = folder("save");
    let other = dir.join("other.tenon");
    block_file(&other, 0.0, 10.0, 10.0, 5.0);
    let mine = dir.join("mine.tenon");
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.derive", json!({ "path": path(&other) }));
    run(&mut s, &mut k, "file.save", json!({ "path": path(&mine) }));
    near(volume(&mut s, &mut k), 500.0);
    let stood = |s: &mut Session, k: &mut OcctKernel| run(s, k, "model.mass", json!({}))["bodies"][0]["volume"].as_f64().unwrap();

    block_file(&other, 0.0, 10.0, 10.0, 50.0);
    // An edit elsewhere takes no notice; nor does a save where it was.
    run(&mut s, &mut k, "document.rename", json!({ "name": "Mine" }));
    near(stood(&mut s, &mut k), 500.0);
    run(&mut s, &mut k, "file.save", json!({ "path": path(&mine) }));
    near(stood(&mut s, &mut k), 500.0);
    assert!(!s.is_dirty());
    // Nor a save in a new place, though the path in the file is from there.
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let elsewhere = sub.join("mine.tenon");
    run(&mut s, &mut k, "file.save", json!({ "path": path(&elsewhere) }));
    near(stood(&mut s, &mut k), 500.0);
    assert!(std::fs::read_to_string(&elsewhere).unwrap().contains("path = \"../other.tenon\""));
    assert_eq!(s.document().links().home.as_deref(), Some(tenon_io::asm::part_key(&elsewhere).as_str()));
    // A rebuild takes it.
    near(volume(&mut s, &mut k), 5000.0);

    // A file gone is found out on a rebuild as well, not on a save.
    std::fs::remove_file(&other).unwrap();
    run(&mut s, &mut k, "file.save", json!({ "path": path(&elsewhere) }));
    near(stood(&mut s, &mut k), 5000.0);
    let said = fails(&mut s, &mut k);
    assert!(said.contains("other.tenon") && said.contains("missing"), "{said}");

    // Saved over what it derives from, directly and through another, it says so at once.
    block_file(&other, 0.0, 10.0, 10.0, 5.0);
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "model.derive", json!({ "path": path(&other) }));
    near(volume(&mut s, &mut k), 500.0);
    run(&mut s, &mut k, "file.save", json!({ "path": path(&other) }));
    let said = fails(&mut s, &mut k);
    assert!(said.contains("cannot derive itself"), "{said}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_relative_path_in_a_definition_is_from_this_file_and_with_no_file_it_is_refused() {
    let dir = folder("definition");
    let other = dir.join("other.tenon");
    block_file(&other, 0.0, 10.0, 10.0, 5.0);
    let mine = dir.join("mine.tenon");
    // With no file for it to be relative to, in words, both ways of writing a definition.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let kind = json!({ "type": "derive", "path": "other.tenon" });
    let e = exec(&mut s, "feature.add", &json!({ "kind": kind }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("other.tenon") && e.contains("relative") && e.contains("save the part first"), "{e}");
    assert!(s.document().features().is_empty());
    let d = feature(&run(&mut s, &mut k, "model.derive", json!({ "path": path(&other) })));
    let e = exec(&mut s, "feature.update", &json!({ "feature": d, "kind": kind }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("relative") && e.contains("save the part first"), "{e}");
    near(volume(&mut s, &mut k), 500.0);
    // Saved, it is from the file, wherever the program is, and held in full from then on.
    run(&mut s, &mut k, "file.save", json!({ "path": path(&mine) }));
    let kind = json!({ "type": "derive", "path": "other.tenon", "scale": 2.0 });
    run(&mut s, &mut k, "feature.update", json!({ "feature": d, "kind": kind }));
    near(volume(&mut s, &mut k), 4000.0);
    assert_eq!(s.document().derived_paths(), vec![tenon_io::asm::part_key(&other)]);
    // Undone and redone, and saved again: the same.
    run(&mut s, &mut k, "edit.undo", json!({}));
    near(volume(&mut s, &mut k), 500.0);
    run(&mut s, &mut k, "edit.redo", json!({}));
    near(volume(&mut s, &mut k), 4000.0);
    run(&mut s, &mut k, "file.save", json!({ "path": path(&mine) }));
    assert!(std::fs::read_to_string(&mine).unwrap().contains("path = \"other.tenon\""));
    let _ = std::fs::remove_dir_all(dir);
}
