//! Autosave and crash recovery (DEC-033) with real pointer and key input: unsaved work kept while
//! Tenon runs comes back after a crash (Recover, Discard or Not Now), a drawing comes back with
//! the model edited from it, a running Tenon's work is never offered to another, and closing
//! properly leaves nothing behind.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use egui::vec2;
use serde_json::json;
use tenon_kernel_occt::OcctKernel;

use crate::Workbench;
use crate::tests::{Driver, ctrl, pressable};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-ui-recover-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An example's files, copied so the test may change them.
fn example(example: &str, files: &[&str], name: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(example);
    let dir = scratch(name);
    for f in files {
        std::fs::copy(src.join(f), dir.join(f)).unwrap();
    }
    dir
}

/// A Tenon keeping unsaved work under `base`, autosaving every tenth of a second.
fn tenon(base: &Path) -> (Workbench, Driver) {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    wb.autosave_seconds = 0.1;
    wb.enable_recovery(base).unwrap();
    (wb, Driver::new(vec2(1400.0, 860.0)))
}

/// Long enough for an autosave.
fn work(d: &mut Driver, wb: &mut Workbench) {
    for _ in 0..12 {
        d.frame(wb, vec![]);
    }
}

/// The folders Tenons keep work in.
fn folders(base: &Path) -> usize {
    std::fs::read_dir(base).map_or(0, |r| r.flatten().count())
}

fn height(wb: &Workbench) -> f64 {
    wb.document().parameter_values()["H"]
}

#[test]
fn unsaved_part_work_survives_a_crash_and_comes_back_on_request() {
    let _quiet = crate::tests::timing_lock();
    let (base, dir) = (scratch("part-base"), example("m2-enclosure", &["enclosure.tenon"], "part"));
    let file = dir.join("enclosure.tenon");
    let (mut wb, mut d) = tenon(&base);
    assert!(wb.recover_offer.is_none(), "nothing to offer the first time");
    wb.open(&file).unwrap();
    wb.exec("param.set", json!({ "name": "H", "equation": "40 mm" })).unwrap();
    work(&mut d, &mut wb);
    // The file itself is untouched: only Tenon's own copy has the change.
    assert!(std::fs::read_to_string(&file).unwrap().contains("equation = \"30 mm\""));
    // A crash: no proper close, so the copy stays.
    drop(wb);

    // The next start offers it back; Enter recovers it.
    let (mut wb, mut d) = tenon(&base);
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    let shown = d.texts();
    assert!(shown.iter().any(|t| t == "Recover unsaved work?"), "{shown:?}");
    assert!(shown.iter().any(|t| t.starts_with("Tenon closed without saving changes to enclosure.tenon.")), "{shown:?}");
    d.tap(&mut wb, egui::Key::Enter);
    assert!(wb.recover_offer.is_none());
    assert_eq!(height(&wb), 40.0, "{}", wb.status());
    assert!(wb.status().starts_with("Recovered enclosure.tenon as it was "), "{}", wb.status());
    assert_eq!(wb.path.as_deref(), Some(std::path::absolute(&file).unwrap().as_path()));
    assert!(wb.session.is_dirty(), "recovered work is unsaved until saved");
    // Undo goes back to the file as it was saved; redo to the recovered work.
    d.frame(&mut wb, vec![egui::Event::PointerMoved(egui::pos2(700.0, 400.0))]);
    ctrl(&mut d, &mut wb, egui::Key::Z);
    assert_eq!(height(&wb), 30.0);
    ctrl(&mut d, &mut wb, egui::Key::Y);
    assert_eq!(height(&wb), 40.0);
    // Saved: the file has it, and closing properly leaves nothing for next time.
    ctrl(&mut d, &mut wb, egui::Key::S);
    assert!(std::fs::read_to_string(&file).unwrap().contains("equation = \"40 mm\""));
    work(&mut d, &mut wb);
    wb.shutdown();
    assert_eq!(folders(&base), 0, "a proper close leaves no copies");
    let (wb, _) = tenon(&base);
    assert!(wb.recover_offer.is_none());
}

#[test]
fn not_now_keeps_the_work_for_next_time_and_discard_drops_it() {
    let _quiet = crate::tests::timing_lock();
    let base = scratch("later-base");
    let (mut wb, mut d) = tenon(&base);
    // A new part, never saved.
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    work(&mut d, &mut wb);
    drop(wb);

    // Esc: not now. The copy stays; the empty part is left as it was.
    let (mut wb, mut d) = tenon(&base);
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    assert!(d.texts().iter().any(|t| t.starts_with("Tenon closed without saving changes to a new part.")), "{:?}", d.texts());
    d.tap(&mut wb, egui::Key::Escape);
    assert!(wb.recover_offer.is_none() && wb.document().features().is_empty());
    assert!(wb.status().contains("offers it again next time"), "{}", wb.status());
    wb.shutdown();

    // Offered again; Discard drops it for good.
    let (mut wb, mut d) = tenon(&base);
    assert!(wb.recover_offer.is_some(), "offered again");
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    let at = pressable(&d, "tn_recover_discard");
    d.click(&mut wb, at);
    assert!(wb.recover_offer.is_none() && wb.status() == "The unsaved work was discarded.");
    wb.shutdown();
    let (wb, _) = tenon(&base);
    assert!(wb.recover_offer.is_none(), "discarded for good");
}

#[test]
fn a_drawing_comes_back_with_the_model_edited_from_it() {
    let _quiet = crate::tests::timing_lock();
    let base = scratch("drw-base");
    let dir = example("m4-plate", &["plate.tenon", "pin.tenon", "plate-pins.tenonasm", "plate.tenondrw"], "drw");
    let (drawing, plate) = (dir.join("plate.tenondrw"), dir.join("plate.tenon"));
    let (mut wb, mut d) = tenon(&base);
    wb.open(&drawing).unwrap();
    d.frame(&mut wb, vec![]);
    let front = crate::tests::browser_row(&d, "VIEW1: plate.tenon");
    d.click(&mut wb, front.center());
    wb.drw_exec("drw.props", json!({ "company": "ACME" })).unwrap();
    // Open Model, make the plate thicker, and crash while still in the model.
    wb.run_ui("drw.edit_model").unwrap();
    wb.exec("param.set", json!({ "name": "t", "equation": "20" })).unwrap();
    work(&mut d, &mut wb);
    assert!(wb.editing_from_drawing());
    drop(wb);

    let (mut wb, mut d) = tenon(&base);
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    assert!(
        d.texts().iter().any(|t| t.starts_with("Tenon closed without saving changes to plate.tenondrw and 1 file(s) it uses.")),
        "{:?}",
        d.texts()
    );
    let at = pressable(&d, "tn_recover_recover");
    d.click(&mut wb, at);
    assert!(wb.status().starts_with("Recovered plate.tenondrw and 1 file(s) it uses"), "{}", wb.status());
    let doc = wb.drw.as_ref().unwrap();
    assert_eq!(doc.session.drawing().props.company, "ACME");
    assert!(doc.session.is_dirty());
    let key = tenon_io::asm::part_key(&plate);
    match doc.session.models.get(&key) {
        Some(tenon_drawing::DrwModel::Part(s)) => {
            assert_eq!(s.document().parameter_values()["t"], 20.0);
            assert!(s.is_dirty());
        }
        other => panic!("the plate is not in the drawing: {:?}", other.map(|_| ())),
    }
    // The files themselves were never touched.
    assert!(!std::fs::read_to_string(&drawing).unwrap().contains("ACME"));
    wb.shutdown();
}

#[test]
fn a_running_tenons_work_is_never_offered_to_another() {
    let _quiet = crate::tests::timing_lock();
    let base = scratch("two-base");
    let (mut first, mut d) = tenon(&base);
    first.create_sketch(json!({ "plane": "xy" })).unwrap();
    work(&mut d, &mut first);
    // A second Tenon starts while the first still runs: nothing is offered.
    let (second, _) = tenon(&base);
    assert!(second.recover_offer.is_none());
    assert_eq!(folders(&base), 2);
    // Saved work leaves no copy.
    first.save(&base.join("..").join(format!("tenon-ui-recover-{}-saved.tenon", std::process::id()))).unwrap();
    work(&mut d, &mut first);
    drop(first);
    let (third, _) = tenon(&base);
    assert!(third.recover_offer.is_none(), "nothing was unsaved when it stopped");
}

/// A part that derives its solid, 20 x 10 x 5, from another file beside it. Both are written.
fn deriving_part(dir: &Path) -> PathBuf {
    let other = dir.join("other.tenon");
    let mine = dir.join("mine.tenon");
    let mut k = OcctKernel::new();
    let mut s = tenon_model::Session::default();
    let sk = tenon_io::cmd::run(&mut s, "sketch.create", &json!({ "plane": "xy" }), Some(&mut k)).unwrap()["feature"].as_u64().unwrap();
    tenon_io::cmd::run(&mut s, "sketch.rectangle", &json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 20, "y2": 10 }), Some(&mut k)).unwrap();
    tenon_io::cmd::run(&mut s, "model.extrude", &json!({ "sketch": sk, "distance": 5 }), Some(&mut k)).unwrap();
    tenon_io::cmd::run(&mut s, "file.save", &json!({ "path": other.to_string_lossy() }), Some(&mut k)).unwrap();
    let mut s = tenon_model::Session::default();
    tenon_io::cmd::run(&mut s, "model.derive", &json!({ "path": other.to_string_lossy() }), Some(&mut k)).unwrap();
    tenon_io::cmd::run(&mut s, "file.save", &json!({ "path": mine.to_string_lossy() }), Some(&mut k)).unwrap();
    mine
}

/// Drops `wb` as a crash would, starts another Tenon and takes the Recover button.
fn crash_and_recover(wb: Workbench, base: &Path) -> (Workbench, Driver) {
    drop(wb);
    let (mut wb, mut d) = tenon(base);
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    let at = pressable(&d, "tn_recover_recover");
    d.click(&mut wb, at);
    work(&mut d, &mut wb);
    assert!(wb.status().starts_with("Recovered"), "{}", wb.status());
    (wb, d)
}

/// What a part's own file and table say once it is back.
#[track_caller]
fn it_can_read_what_it_derives(s: &tenon_model::Session, file: &Path) {
    let links = s.document().links();
    assert_eq!(links.home.as_deref(), Some(tenon_io::asm::part_key(file).as_str()));
    assert_eq!(links.sources.len(), 1, "what it derives from is read");
    assert!(links.sources.values().all(|v| v.document.is_ok()));
}

#[test]
fn a_part_that_derives_comes_back_able_to_build_itself() {
    let _quiet = crate::tests::timing_lock();
    // Open by itself.
    let (base, dir) = (scratch("derive-part-base"), scratch("derive-part"));
    let mine = deriving_part(&dir);
    let (mut wb, mut d) = tenon(&base);
    wb.open(&mine).unwrap();
    wb.exec("work.ucs", json!({ "x": 60, "z": 20 })).unwrap();
    work(&mut d, &mut wb);
    let (mut wb, mut d) = crash_and_recover(wb, &base);
    it_can_read_what_it_derives(&wb.session, &mine);
    work(&mut d, &mut wb);
    assert!((wb.scene().bodies.iter().map(|b| b.volume).sum::<f64>() - 1000.0).abs() < 1e-6, "{}", wb.status());
    // Saved, its path is relative to the file again.
    wb.save(&mine).unwrap();
    assert!(std::fs::read_to_string(&mine).unwrap().contains("path = \"other.tenon\""));
    wb.shutdown();

    // Shown in a drawing, edited from it: its view is whole, and Update keeps it so.
    let (base, dir) = (scratch("derive-drw-base"), scratch("derive-drw"));
    let mine = deriving_part(&dir);
    let draw = dir.join("mine.tenondrw");
    let (mut wb, mut d) = tenon(&base);
    wb.run_ui("file.new_drawing").unwrap();
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    wb.drw_exec("drw.view.base", json!({ "model": mine.to_string_lossy(), "orientation": "front" })).unwrap();
    wb.save_drawing(&draw).unwrap();
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    let front = crate::tests::browser_row(&d, "VIEW1: mine.tenon");
    d.click(&mut wb, front.center());
    wb.run_ui("drw.edit_model").unwrap();
    wb.exec("work.ucs", json!({ "x": 60, "z": 20 })).unwrap();
    work(&mut d, &mut wb);
    assert!(wb.editing_from_drawing());
    let (mut wb, mut d) = crash_and_recover(wb, &base);
    let key = tenon_io::asm::part_key(&mine);
    let Some(tenon_drawing::DrwModel::Part(s)) = wb.drw.as_ref().unwrap().session.models.get(&key) else { panic!("the part is not in the drawing") };
    it_can_read_what_it_derives(s, &mine);
    wb.drw_exec("drw.update", json!({})).unwrap();
    work(&mut d, &mut wb);
    let info = wb.drw_exec("drw.info", json!({})).unwrap();
    assert!(info["views"][0]["visible_curves"].as_u64().unwrap() > 0, "{info}");
    assert!(info["views"][0]["error"].is_null(), "{info}");
    wb.shutdown();

    // A part of an assembly, changed there.
    let (base, dir) = (scratch("derive-asm-base"), scratch("derive-asm"));
    let mine = deriving_part(&dir);
    let asm = dir.join("mine.tenonasm");
    let (mut wb, mut d) = tenon(&base);
    wb.run_ui("file.new_assembly").unwrap();
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    wb.asm_exec("asm.insert", json!({ "path": mine.to_string_lossy() })).unwrap();
    wb.save_assembly(&asm).unwrap();
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    let key = tenon_io::asm::part_key(&mine);
    let part = wb.asm.as_mut().unwrap().session.parts.get_mut(&key).unwrap();
    part.session.exec("work.ucs", &json!({ "x": 60, "z": 20 }), None).unwrap();
    work(&mut d, &mut wb);
    let (mut wb, _d) = crash_and_recover(wb, &base);
    let part = wb.asm.as_ref().unwrap().session.parts.get(&key).unwrap();
    it_can_read_what_it_derives(&part.session, &mine);
    wb.shutdown();
}
