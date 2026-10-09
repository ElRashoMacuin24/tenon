//! "Save changes?" before New, Open and Exit, with real pointer and key input: Save, Don't Save
//! and Cancel for a part, an assembly with a part edited in place, a drawing with a model edited
//! from it, and leaving the app from the menu or the window's close button.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use egui::vec2;
use serde_json::json;
use tenon_assembly::ComponentId;
use tenon_drawing::{Owner, ViewId};
use tenon_kernel_occt::OcctKernel;

use crate::Workbench;
use crate::tests::{Driver, browser_row, ctrl, pressable};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-ui-save-{}-{name}", std::process::id()));
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

/// The files "Save changes?" lists, or None when it is not open.
fn asking(wb: &Workbench) -> Option<Vec<String>> {
    wb.chrome.save_prompt.as_ref().map(|p| p.files.clone())
}

fn names(files: &[&str]) -> Option<Vec<String>> {
    Some(files.iter().map(|f| (*f).to_owned()).collect())
}

/// Lets the prompt lay out, then clicks one of its buttons.
fn press(d: &mut Driver, wb: &mut Workbench, button: &str) {
    for _ in 0..3 {
        d.frame(wb, vec![]);
    }
    let at = pressable(d, button);
    d.click(wb, at);
}

/// Clicks the prompt beside its buttons, so no button has the keyboard focus.
fn click_beside_buttons(d: &mut Driver, wb: &mut Workbench) {
    for _ in 0..3 {
        d.frame(wb, vec![]);
    }
    let at = pressable(d, "tn_save_save") - vec2(60.0, 0.0);
    d.click(wb, at);
    assert!(!d.ctx.egui_wants_keyboard_input(), "no widget has the focus");
    assert!(asking(wb).is_some(), "a click in the prompt does not close it");
}

/// Clicks File, then the menu entry that runs `command`.
fn file_menu(d: &mut Driver, wb: &mut Workbench, command: &str) {
    let tab = pressable(d, "tn_file_tab");
    d.click(wb, tab);
    for _ in 0..2 {
        d.frame(wb, vec![]);
    }
    let at = pressable(d, command);
    d.click(wb, at);
}

/// Answers save dialogs with the suggested name in `dir` (None: the dialog is cancelled), and
/// keeps the names they were asked with.
fn save_dialog(wb: &mut Workbench, dir: Option<PathBuf>) -> Rc<RefCell<Vec<String>>> {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let log = asked.clone();
    wb.services.pick_save = Some(Box::new(move |name: &str, _ext: &str| {
        log.borrow_mut().push(name.to_owned());
        dir.as_ref().map(|d| d.join(name))
    }));
    asked
}

/// File > Open picks `path`.
fn open_dialog(wb: &mut Workbench, path: PathBuf) {
    wb.services.pick_open = Some(Box::new(move || Some(path.clone())));
}

fn features(path: &Path) -> usize {
    tenon_io::project::open(path).unwrap().0.features().len()
}

fn parameter(path: &Path, name: &str) -> f64 {
    tenon_io::project::open(path).unwrap().0.parameter_values()[name]
}

/// Adds a sketch to the part on the workbench.
fn change_part(wb: &mut Workbench) {
    wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap();
}

#[test]
fn a_changed_part_asks_before_new_and_open_and_each_answer_does_what_it_says() {
    let dir = scratch("part");
    let file = dir.join("Part1.tenon");
    let mut wb = Workbench::without_kernel();
    let mut d = Driver::new(vec2(1400.0, 860.0));
    d.frame(&mut wb, vec![]);

    // Nothing changed: New does not ask.
    ctrl(&mut d, &mut wb, egui::Key::N);
    assert_eq!(asking(&wb), None);
    change_part(&mut wb);
    d.frame(&mut wb, vec![]);

    // Ctrl+N asks about the part. Cancel, or Esc, leaves it as it was.
    ctrl(&mut d, &mut wb, egui::Key::N);
    assert_eq!(asking(&wb), names(&["Part1.tenon"]));
    press(&mut d, &mut wb, "tn_save_cancel");
    assert_eq!(asking(&wb), None);
    assert_eq!(wb.document().features().len(), 1);
    assert!(wb.session.is_dirty());
    ctrl(&mut d, &mut wb, egui::Key::N);
    assert!(asking(&wb).is_some());
    d.tap(&mut wb, egui::Key::Escape);
    assert_eq!(asking(&wb), None);
    assert_eq!(wb.document().features().len(), 1);

    // While it asks, keys go to it alone: Ctrl+Z does not undo behind it.
    ctrl(&mut d, &mut wb, egui::Key::N);
    click_beside_buttons(&mut d, &mut wb);
    ctrl(&mut d, &mut wb, egui::Key::Z);
    assert_eq!(wb.document().features().len(), 1, "nothing undone under the prompt");
    assert!(asking(&wb).is_some());

    // Save on a part never saved asks where; a save dialog closed without a file stops there.
    let asked = save_dialog(&mut wb, None);
    press(&mut d, &mut wb, "tn_save_save");
    assert_eq!(*asked.borrow(), ["Part1.tenon"]);
    assert_eq!(asking(&wb), None);
    assert_eq!(wb.document().features().len(), 1, "not saved, so not replaced");
    assert!(wb.status().contains("Not saved"), "{}", wb.status());

    // Enter answers Save (the default button): written where chosen, then the new part.
    let asked = save_dialog(&mut wb, Some(dir.clone()));
    ctrl(&mut d, &mut wb, egui::Key::N);
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    d.tap(&mut wb, egui::Key::Enter);
    assert_eq!(*asked.borrow(), ["Part1.tenon"]);
    assert_eq!(asking(&wb), None);
    assert_eq!(features(&file), 1);
    assert_eq!(wb.document().features().len(), 0, "a new part");
    assert!(!wb.session.is_dirty());

    // Don't Save drops the changes: File > Open of the saved part replaces two new sketches.
    change_part(&mut wb);
    change_part(&mut wb);
    open_dialog(&mut wb, file.clone());
    ctrl(&mut d, &mut wb, egui::Key::O);
    assert_eq!(asking(&wb), names(&["Part1.tenon"]));
    press(&mut d, &mut wb, "tn_save_discard");
    assert_eq!(asking(&wb), None);
    assert_eq!(wb.document().features().len(), 1, "the file as it was saved");
    assert_eq!(features(&file), 1);
    assert!(wb.status().contains("Opened"), "{}", wb.status());

    // A part saved before is saved where it is, without a dialog, before the next file opens.
    change_part(&mut wb);
    let asked = save_dialog(&mut wb, None);
    let other = dir.join("Other.tenon");
    std::fs::copy(&file, &other).unwrap();
    open_dialog(&mut wb, other);
    ctrl(&mut d, &mut wb, egui::Key::O);
    press(&mut d, &mut wb, "tn_save_save");
    assert!(asked.borrow().is_empty(), "{:?}", asked.borrow());
    assert_eq!(features(&file), 2);
    assert_eq!(wb.path.as_deref().and_then(Path::file_name), Some("Other.tenon".as_ref()));
}

#[test]
fn an_assembly_and_the_part_edited_in_place_are_asked_about_together() {
    let dir = example("m3-pivot", &["pivot.tenonasm", "base.tenon", "arm.tenon", "pin.tenon", "block.tenon"], "asm");
    let (pivot, arm) = (dir.join("pivot.tenonasm"), dir.join("arm.tenon"));
    let t = parameter(&arm, "t");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let edit_the_arm = |wb: &mut Workbench, d: &mut Driver| {
        wb.open(&pivot).unwrap();
        d.settle(wb);
        let id = wb.assembly().unwrap().assembly().components.iter().find(|c| c.part.ends_with("arm.tenon")).unwrap().id;
        wb.edit_in_place(id).unwrap();
        wb.exec("param.set", json!({ "name": "t", "equation": "5" })).unwrap();
        d.frame(wb, vec![]);
        assert!(wb.editing_in_place());
    };
    edit_the_arm(&mut wb, &mut d);

    // File > New Assembly asks about the assembly and the arm changed in it.
    file_menu(&mut d, &mut wb, "file.new_assembly");
    assert_eq!(asking(&wb), names(&["pivot.tenonasm", "arm.tenon"]));
    press(&mut d, &mut wb, "tn_save_cancel");
    assert!(wb.editing_in_place() && wb.document().name == "Arm");
    assert_eq!(wb.document().parameter_values()["t"], 5.0, "the change is kept");
    assert_eq!(parameter(&arm, "t"), t, "nothing written");

    // Don't Save: the new assembly, the arm's file as it was.
    file_menu(&mut d, &mut wb, "file.new_assembly");
    press(&mut d, &mut wb, "tn_save_discard");
    assert!(wb.in_assembly());
    assert!(wb.assembly().unwrap().assembly().components.is_empty(), "a new assembly");
    assert_eq!(parameter(&arm, "t"), t);

    // Save: the arm is saved with the assembly, then File > Open opens the base.
    edit_the_arm(&mut wb, &mut d);
    open_dialog(&mut wb, dir.join("base.tenon"));
    ctrl(&mut d, &mut wb, egui::Key::O);
    assert_eq!(asking(&wb), names(&["pivot.tenonasm", "arm.tenon"]));
    press(&mut d, &mut wb, "tn_save_save");
    assert_eq!(parameter(&arm, "t"), 5.0);
    assert!(wb.asm.is_none() && wb.document().name == "Base", "{}", wb.status());
    assert!(tenon_io::asm::open(&pivot).is_ok());
}

#[test]
fn a_drawing_and_the_model_edited_from_it_are_asked_about_together() {
    let _quiet = crate::tests::timing_lock();
    let dir = example("m4-plate", &["plate.tenon", "pin.tenon", "plate-pins.tenonasm", "plate.tenondrw"], "drw");
    let (drawing, plate) = (dir.join("plate.tenondrw"), dir.join("plate.tenon"));
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.open(&drawing).unwrap();
    d.frame(&mut wb, vec![]);
    let front = browser_row(&d, "VIEW1: plate.tenon");
    d.click(&mut wb, front.center());
    assert_eq!(wb.drw.as_ref().unwrap().selected, Some(Owner::View(ViewId(1))));
    wb.drw_exec("drw.props", json!({ "company": "ACME" })).unwrap();

    // The drawing asks before New; Delete pressed while it asks deletes nothing.
    ctrl(&mut d, &mut wb, egui::Key::N);
    assert_eq!(asking(&wb), names(&["plate.tenondrw"]));
    let views = wb.drawing().unwrap().drawing().views.len();
    click_beside_buttons(&mut d, &mut wb);
    d.tap(&mut wb, egui::Key::Delete);
    assert_eq!(wb.drawing().unwrap().drawing().views.len(), views, "the selected view stays");
    press(&mut d, &mut wb, "tn_save_cancel");
    assert!(wb.in_drawing());

    // Open Model, make the plate thicker: New Drawing asks about the drawing and the plate.
    wb.run_ui("drw.edit_model").unwrap();
    wb.exec("param.set", json!({ "name": "t", "equation": "20" })).unwrap();
    d.frame(&mut wb, vec![]);
    assert!(wb.editing_from_drawing());
    file_menu(&mut d, &mut wb, "file.new_drawing");
    assert_eq!(asking(&wb), names(&["plate.tenondrw", "plate.tenon"]));
    press(&mut d, &mut wb, "tn_save_cancel");
    assert!(wb.editing_from_drawing() && wb.document().parameter_values()["t"] == 20.0, "still editing the plate");
    assert_eq!(parameter(&plate, "t"), 16.0, "nothing written");

    // Don't Save: a new drawing; the drawing's and the plate's files as they were.
    file_menu(&mut d, &mut wb, "file.new_drawing");
    press(&mut d, &mut wb, "tn_save_discard");
    assert!(wb.in_drawing() && wb.drawing().unwrap().drawing().views.is_empty(), "a new drawing");
    assert_eq!(parameter(&plate, "t"), 16.0);
    assert_eq!(tenon_io::drw::open(&drawing).unwrap().0.props.company, "", "{}", wb.status());

    // Again, then Save: the drawing and the plate edited from it are written, then the pin opens.
    wb.open(&drawing).unwrap();
    d.frame(&mut wb, vec![]);
    wb.drw_exec("drw.props", json!({ "company": "ACME" })).unwrap();
    let front = browser_row(&d, "VIEW1: plate.tenon");
    d.click(&mut wb, front.center());
    wb.run_ui("drw.edit_model").unwrap();
    wb.exec("param.set", json!({ "name": "t", "equation": "20" })).unwrap();
    d.frame(&mut wb, vec![]);
    open_dialog(&mut wb, dir.join("pin.tenon"));
    ctrl(&mut d, &mut wb, egui::Key::O);
    assert_eq!(asking(&wb), names(&["plate.tenondrw", "plate.tenon"]));
    press(&mut d, &mut wb, "tn_save_save");
    assert_eq!(parameter(&plate, "t"), 20.0);
    assert_eq!(tenon_io::drw::open(&drawing).unwrap().0.props.company, "ACME");
    assert!(wb.drw.is_none() && wb.document().name == "Pin", "{}", wb.status());
}

#[test]
fn a_drawing_its_assembly_and_a_part_in_place_in_it_are_saved_whole() {
    let _quiet = crate::tests::timing_lock();
    let dir = example("m4-plate", &["plate.tenon", "pin.tenon", "plate-pins.tenonasm", "plate.tenondrw"], "nested");
    let (asm, plate) = (dir.join("plate-pins.tenonasm"), dir.join("plate.tenon"));
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.open(&dir.join("plate.tenondrw")).unwrap();
    d.frame(&mut wb, vec![]);

    // The assembly view's model, one pin taken out; the plate in it edited in place.
    let r = browser_row(&d, "VIEW5: plate-pins.tenonasm");
    d.click(&mut wb, r.center());
    d.frame(&mut wb, vec![]);
    wb.run_ui("drw.edit_model").unwrap();
    d.frame(&mut wb, vec![]);
    assert!(wb.in_assembly());
    wb.asm_exec("asm.delete", json!({ "component": 3 })).unwrap();
    let id = wb.assembly().unwrap().assembly().components.iter().find(|c| c.part.ends_with("plate.tenon")).unwrap().id;
    wb.edit_in_place(id).unwrap();
    wb.exec("param.set", json!({ "name": "t", "equation": "20" })).unwrap();
    d.frame(&mut wb, vec![]);

    // New lists all three; Save writes all three (the drawing's own path, no dialog), then New.
    let asked = save_dialog(&mut wb, None);
    ctrl(&mut d, &mut wb, egui::Key::N);
    assert_eq!(asking(&wb), names(&["plate.tenondrw", "plate-pins.tenonasm", "plate.tenon"]));
    press(&mut d, &mut wb, "tn_save_save");
    assert!(asked.borrow().is_empty());
    assert_eq!(parameter(&plate, "t"), 20.0);
    let (saved, _) = tenon_io::asm::open(&asm).unwrap();
    assert_eq!(saved.components.len(), 2);
    assert!(saved.component(ComponentId(3)).is_none());
    assert!(wb.drw.is_none() && wb.asm.is_none() && wb.document().features().is_empty(), "a new part: {}", wb.status());
}

#[test]
fn exit_and_the_window_close_button_ask_first() {
    let dir = scratch("exit");
    let mut wb = Workbench::without_kernel();
    let mut d = Driver::new(vec2(1400.0, 860.0));
    d.frame(&mut wb, vec![]);

    // Nothing changed: the window closes at once.
    assert!(wb.close_requested());
    assert_eq!(asking(&wb), None);
    change_part(&mut wb);

    // File > Exit asks; Cancel keeps the app open.
    file_menu(&mut d, &mut wb, "app.exit");
    assert_eq!(asking(&wb), names(&["Part1.tenon"]));
    press(&mut d, &mut wb, "tn_save_cancel");
    assert!(!wb.exit_requested());

    // The close button asks too (the app cancels the close meanwhile); a save dialog closed
    // without a file keeps the app open.
    save_dialog(&mut wb, None);
    assert!(!wb.close_requested());
    press(&mut d, &mut wb, "tn_save_save");
    assert!(!wb.exit_requested() && asking(&wb).is_none());

    // Save, then exit: once it is asked to close again the window may.
    save_dialog(&mut wb, Some(dir.clone()));
    assert!(!wb.close_requested());
    press(&mut d, &mut wb, "tn_save_save");
    assert!(wb.exit_requested());
    assert_eq!(features(&dir.join("Part1.tenon")), 1);
    assert!(wb.close_requested());

    // Don't Save exits without writing.
    let mut wb = Workbench::without_kernel();
    let mut d = Driver::new(vec2(1400.0, 860.0));
    d.frame(&mut wb, vec![]);
    change_part(&mut wb);
    change_part(&mut wb);
    assert!(!wb.close_requested());
    press(&mut d, &mut wb, "tn_save_discard");
    assert!(wb.exit_requested() && wb.close_requested());
    assert_eq!(features(&dir.join("Part1.tenon")), 1);
}
