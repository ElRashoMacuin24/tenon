//! Baseline and ordinate sets through the UI, with real pointer and key input: made by clicking
//! in a view, changed from the panel, moved as a set, opened again from the sheet and the
//! browser, deleted; values that follow the model. The figures are the M4 demo plate's: 120 x 80,
//! holes at (15, 15), (105, 15), (105, 65), (15, 65) of radius 4, and at (60, 40) of radius 5 in a
//! counterbore of radius 9.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use egui::vec2;
use serde_json::{Value, json};
use tenon_drawing::{AnnotId, Orientation, Owner, ViewId};
use tenon_geom::Vec2;
use tenon_kernel_occt::OcctKernel;

use crate::Workbench;
use crate::drawing::DrwTool;
use crate::tests::{Driver, browser_row, ctrl, pressable, type_into};

fn plate(name: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m4-plate");
    let dir = std::env::temp_dir().join(format!("tenon-ui-sets-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(src.join("plate.tenon"), dir.join("plate.tenon")).unwrap();
    dir
}

/// A new drawing with one view of the plate, at `scale`, and the Annotate tab showing.
fn drawing_of_plate(name: &str, orientation: Orientation, scale: f64) -> (Workbench, Driver) {
    let dir = plate(name);
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_ui("file.new_drawing").unwrap();
    d.frame(&mut wb, vec![]);
    wb.place_base_view(&dir.join("plate.tenon"), orientation, Some(scale), true).unwrap();
    d.tap(&mut wb, egui::Key::Escape);
    wb.drw.as_mut().unwrap().selected = None;
    assert_eq!(wb.ribbon_def()[1].name, "Annotate");
    wb.chrome.tab = 1;
    d.frame(&mut wb, vec![]);
    (wb, d)
}

fn info(wb: &mut Workbench) -> Value {
    wb.drw_exec("drw.info", json!({})).unwrap()
}

/// Where a point of view 1 (model mm) is on the sheet.
fn sheet(wb: &mut Workbench, x: f64, y: f64) -> Vec2 {
    let r = wb.drw_exec("drw.to_sheet", json!({ "view": 1, "at": [x, y] })).unwrap();
    Vec2::new(r["x"].as_f64().unwrap(), r["y"].as_f64().unwrap())
}

/// Where a point of view 1 (model mm) is on the screen.
fn in_view(wb: &mut Workbench, x: f64, y: f64) -> egui::Pos2 {
    let s = sheet(wb, x, y);
    wb.sheet_on_screen(s)
}

fn press(d: &mut Driver, wb: &mut Workbench, key: &str) {
    let at = pressable(d, key);
    d.click(wb, at);
    settle(d, wb, 2);
}

fn settle(d: &mut Driver, wb: &mut Workbench, n: usize) {
    for _ in 0..n {
        d.frame(wb, vec![]);
    }
}

fn double_click(d: &mut Driver, wb: &mut Workbench, at: egui::Pos2) {
    // Pause first: clicks less than 0.6 s after earlier ones would count as a triple click.
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    settle(d, wb, 45);
    d.click(wb, at);
    d.click(wb, at);
    settle(d, wb, 2);
}

fn right_click(d: &mut Driver, wb: &mut Workbench, at: egui::Pos2) {
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    d.frame(wb, vec![Driver::button(at, egui::PointerButton::Secondary, true)]);
    d.frame(wb, vec![Driver::button(at, egui::PointerButton::Secondary, false)]);
    settle(d, wb, 2);
}

fn panel(wb: &Workbench) -> Option<crate::dim_sets::SetPanel> {
    match wb.drw.as_ref().and_then(|d| d.tool.clone()) {
        Some(DrwTool::Set(p)) => Some(*p),
        _ => None,
    }
}

/// `shown` and `level` of each row of an annotation.
fn rows(a: &Value) -> Vec<(String, f64)> {
    a["rows"].as_array().unwrap_or_else(|| panic!("{a}")).iter().map(|r| (r["shown"].as_str().unwrap().to_owned(), r["level"].as_f64().unwrap_or(f64::NAN))).collect()
}

fn shown(a: &Value) -> Vec<String> {
    rows(a).into_iter().map(|r| r.0).collect()
}

/// The rows of a baseline read these, the first at `level` and each so much past it. (A level
/// clicked is what the pointer's pixel comes to on the sheet.)
fn rows_are(a: &Value, level: f64, want: &[(&str, f64)]) {
    let got = rows(a);
    assert_eq!(got.len(), want.len(), "{a}");
    for (g, (text, past)) in got.iter().zip(want) {
        assert!(g.0 == *text && (g.1 - level - past).abs() < 1e-3, "{g:?} is not {text} at {}: {a}", level + past);
    }
}

/// Where a row's text is on the screen.
fn row_on_screen(wb: &Workbench, a: &Value, i: usize) -> egui::Pos2 {
    let at = &a["rows"][i]["at"];
    wb.sheet_on_screen(Vec2::new(at[0].as_f64().unwrap(), at[1].as_f64().unwrap()))
}

#[test]
fn a_baseline_set_is_made_changed_moved_and_deleted_by_clicking() {
    let _quiet = crate::tests::timing_lock();
    let (mut wb, mut d) = drawing_of_plate("baseline", Orientation::Top, 1.0);

    // Annotate > Baseline: not greyed; the panel opens and says what to do.
    assert!(crate::commands::find("drw.baseline").unwrap().available());
    press(&mut d, &mut wb, "ribbon drw.baseline");
    let p = panel(&wb).expect("the panel is open");
    assert!(!p.ordinate && p.from.is_none());
    assert!(d.texts().iter().any(|t| t == "Baseline Dimensions"), "{:?}", d.texts());
    assert!(wb.status().contains("click the datum"), "{}", wb.status());
    // Clear of every edge, with nothing picked, it says so.
    let clear = in_view(&mut wb, 60.0, 130.0);
    d.click(&mut wb, clear);
    assert!(wb.status().contains("click an edge"), "{}", wb.status());
    assert!(panel(&wb).is_some() && info(&mut wb)["annotations"].as_array().unwrap().is_empty());

    // The left side is the datum; then the hole at x = 15, the counterbore at 60, the hole at 105
    // and the right side, each by a click on its edge. Each is marked.
    for (x, y) in [(0.0, 40.0), (19.0, 15.0), (69.0, 40.0), (109.0, 65.0), (120.0, 40.0)] {
        let at = in_view(&mut wb, x, y);
        d.click(&mut wb, at);
    }
    let p = panel(&wb).unwrap();
    assert!(p.from.is_some() && p.targets.len() == 4 && p.view == Some(ViewId(1)), "{p:?}, {}", wb.status());
    assert!(wb.status().contains("where the set goes"), "{}", wb.status());
    settle(&mut d, &mut wb, 2);
    let hole = in_view(&mut wb, 15.0, 15.0);
    assert!(d.marks_at(hole).1 >= 1, "the picked hole is marked: {:?}", d.marks_at(hole));

    // A second click on one takes it away; a third brings it back.
    let side = in_view(&mut wb, 120.0, 40.0);
    d.click(&mut wb, side);
    assert_eq!(panel(&wb).unwrap().targets.len(), 3);
    d.click(&mut wb, side);
    assert_eq!(panel(&wb).unwrap().targets.len(), 4);
    assert!(info(&mut wb)["annotations"].as_array().unwrap().is_empty(), "nothing is made yet");

    // Above the view it shows where it would go; a click puts it there, at 110 in the view.
    let place = in_view(&mut wb, 60.0, 110.0);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(place)]);
    settle(&mut d, &mut wb, 2);
    d.click(&mut wb, place);
    assert!(panel(&wb).is_none(), "{}", wb.status());
    let i = info(&mut wb);
    let set = i["annotations"][0].clone();
    assert_eq!((set["type"].as_str(), set["axis"].as_str()), (Some("baseline"), Some("horizontal")), "{i}");
    // Hand: the shortest at the level clicked, the rest 8 further each.
    let level = sheet(&mut wb, 0.0, 110.0).y;
    rows_are(&set, level, &[("15", 0.0), ("60", 8.0), ("105", 16.0), ("120", 24.0)]);
    assert_eq!(wb.drw.as_ref().unwrap().selected, Some(Owner::Annotation(AnnotId(1))));
    // In the browser, as one record.
    settle(&mut d, &mut wb, 2);
    browser_row(&d, "Baseline 1: 4 dimension(s)");

    // One undo takes it all away, one redo brings it back.
    ctrl(&mut d, &mut wb, egui::Key::Z);
    assert!(info(&mut wb)["annotations"].as_array().unwrap().is_empty());
    wb.drw_undo(false).unwrap();
    let again = info(&mut wb);
    assert_eq!(shown(&again["annotations"][0]), ["15", "60", "105", "120"]);
    settle(&mut d, &mut wb, 2);

    // Double-clicking its text opens the panel on it; the set shows as it would be, the original
    // not drawn twice. Remove the first in the list (15); click a hole with the same x as one
    // there: refused in words, nothing changes.
    let on_text = row_on_screen(&wb, &set, 3);
    double_click(&mut d, &mut wb, on_text);
    let p = panel(&wb).expect("the panel opens on it");
    assert_eq!((p.annotation, p.targets.len()), (Some(AnnotId(1)), 4));
    press(&mut d, &mut wb, "tn_set_remove_0");
    assert_eq!(panel(&wb).unwrap().targets.len(), 3);
    let same_x = in_view(&mut wb, 109.0, 15.0);
    d.click(&mut wb, same_x);
    assert!(wb.status().contains("in one place"), "{}", wb.status());
    assert_eq!(panel(&wb).unwrap().targets.len(), 3);
    // Vertical is said to be so in words: the counterbore is level with the middle of the datum.
    press(&mut d, &mut wb, "tn_set_axis_v");
    press(&mut d, &mut wb, "tn_set_ok");
    assert!(wb.status().contains("where the datum is"), "{}", wb.status());
    assert!(panel(&wb).is_some(), "still open, to put right");
    press(&mut d, &mut wb, "tn_set_axis_h");
    // Wider apart and one decimal, then OK: one change, one undo.
    type_into(&mut d, &mut wb, "tn_set_spacing", "12");
    press(&mut d, &mut wb, "tn_set_precision");
    press(&mut d, &mut wb, "tn_set_precision_1");
    press(&mut d, &mut wb, "tn_set_ok");
    assert!(panel(&wb).is_none(), "{}", wb.status());
    let changed = info(&mut wb)["annotations"][0].clone();
    rows_are(&changed, level, &[("60", 0.0), ("105", 12.0), ("120", 24.0)]);
    assert_eq!((changed["spacing"].as_f64(), changed["precision"].as_u64()), (Some(12.0), Some(1)));
    settle(&mut d, &mut wb, 2);
    browser_row(&d, "Baseline 1: 3 dimension(s)");
    ctrl(&mut d, &mut wb, egui::Key::Z);
    assert_eq!(shown(&info(&mut wb)["annotations"][0]), ["15", "60", "105", "120"], "one undo");
    wb.drw_undo(false).unwrap();
    settle(&mut d, &mut wb, 2);

    // Dragged as a set: every line comes along, across what it measures and not along it.
    let px = wb.drw.as_ref().unwrap().cam.px;
    let grab = row_on_screen(&wb, &changed, 0);
    d.drag(&mut wb, grab, grab + vec2(70.0, -40.0), egui::PointerButton::Primary);
    settle(&mut d, &mut wb, 2);
    let moved = info(&mut wb)["annotations"][0].clone();
    for (r, was) in rows(&moved).iter().zip(rows(&changed)) {
        assert!((r.1 - was.1 - 40.0 / px).abs() < 0.05, "{moved}");
    }
    for k in 0..3 {
        assert!((moved["rows"][k]["at"][0].as_f64().unwrap() - changed["rows"][k]["at"][0].as_f64().unwrap()).abs() < 1e-9, "{moved}");
    }
    assert_eq!(moved["offset"]["x"], changed["offset"]["x"], "not along it");

    // Moving the view takes the set with it.
    let before = info(&mut wb)["annotations"][0]["rows"][0]["at"].clone();
    let edge = in_view(&mut wb, 0.0, 30.0);
    d.drag(&mut wb, edge, edge + vec2(0.0, 30.0), egui::PointerButton::Primary);
    settle(&mut d, &mut wb, 2);
    let after = info(&mut wb)["annotations"][0]["rows"][0]["at"].clone();
    assert!((after[1].as_f64().unwrap() - before[1].as_f64().unwrap() + 30.0 / px).abs() < 0.05, "{before} {after}");

    // Its browser row: a double-click opens the panel again, its menu deletes it.
    let row = browser_row(&d, "Baseline 1: 3 dimension(s)").center();
    double_click(&mut d, &mut wb, row);
    assert_eq!(panel(&wb).map(|p| p.annotation), Some(Some(AnnotId(1))));
    press(&mut d, &mut wb, "tn_set_cancel");
    assert!(panel(&wb).is_none());
    let row = browser_row(&d, "Baseline 1: 3 dimension(s)").center();
    right_click(&mut d, &mut wb, row);
    press(&mut d, &mut wb, "tn_set_row_delete");
    assert!(info(&mut wb)["annotations"].as_array().unwrap().is_empty(), "{}", wb.status());
}

/// What an annotation of view 1 draws, from the graphics of the sheet.
fn drawn(wb: &Workbench, a: u32) -> (Vec<Vec<Vec2>>, usize, Vec<tenon_drawing::Text>) {
    let d = wb.drawing().unwrap();
    let (g, _) = tenon_drawing::annotate::build(d.drawing(), d.drawing().sheets[0].id, d.evaluation().unwrap());
    let o = Owner::Annotation(AnnotId(a));
    let lines = g.strokes.iter().filter(|s| s.0 == o).map(|s| s.2.clone()).collect();
    let arrows = g.fills.iter().filter(|f| f.0 == o).count();
    (lines, arrows, g.texts.iter().filter(|t| t.0 == o).map(|t| t.1.clone()).collect())
}

#[test]
fn an_ordinate_set_goes_where_it_is_clicked_with_its_values_kept_apart() {
    let _quiet = crate::tests::timing_lock();
    let (mut wb, mut d) = drawing_of_plate("ordinate", Orientation::Top, 0.5);
    assert!(crate::commands::find("drw.ordinate").unwrap().available());
    press(&mut d, &mut wb, "ribbon drw.ordinate");
    assert!(d.texts().iter().any(|t| t == "Ordinate Dimensions"), "{:?}", d.texts());
    assert!(wb.status().contains("click the origin"), "{}", wb.status());

    // The left side is the origin; the holes at 15 and 105, the right side at 120.
    for (x, y) in [(0.0, 40.0), (19.0, 15.0), (109.0, 15.0), (120.0, 40.0)] {
        let at = in_view(&mut wb, x, y);
        d.click(&mut wb, at);
    }
    assert_eq!(panel(&wb).unwrap().targets.len(), 3, "{}", wb.status());
    // Below the view, then: horizontal, and the values below their lines.
    let place = in_view(&mut wb, 60.0, -24.0);
    d.click(&mut wb, place);
    assert!(panel(&wb).is_none(), "{}", wb.status());
    let i = info(&mut wb);
    let set = i["annotations"][0].clone();
    assert_eq!((set["type"].as_str(), set["axis"].as_str()), (Some("ordinate"), Some("horizontal")), "{i}");
    assert_eq!(shown(&set), ["0", "15", "105", "120"], "{set}");
    // No arrows; one extension line each.
    let (lines, arrows, texts) = drawn(&wb, 1);
    assert_eq!((lines.len(), arrows, texts.len()), (4, 0, 4), "{lines:?}");
    // At 1:2 the 105 and 120 are 7.5 apart and wider than that (8.87 and 2 clear): they move apart
    // about their middle, by the same each, and their lines jog. The 0 and 15 are clear.
    let width = tenon_drawing::stroke::text_width("105", tenon_drawing::graphics::TEXT);
    let x = |x: f64, wb: &mut Workbench| sheet(wb, x, 0.0).x;
    let (at_0, at_15, at_105, at_120) = (x(0.0, &mut wb), x(15.0, &mut wb), x(105.0, &mut wb), x(120.0, &mut wb));
    let at = |i: usize| set["rows"][i]["at"][0].as_f64().unwrap();
    assert!((at(0) - at_0).abs() < 1e-9 && (at(1) - at_15).abs() < 1e-9, "{set}");
    let between = (at_105 + at_120) / 2.0;
    assert!((at(2) - (between - (width + 2.0) / 2.0)).abs() < 1e-9 && (at(3) - (between + (width + 2.0) / 2.0)).abs() < 1e-9, "{set}");
    let jogged: Vec<&Vec<Vec2>> = lines.iter().filter(|l| l.len() == 3).collect();
    assert_eq!(jogged.len(), 2, "{lines:?}");
    // The texts do not touch.
    for w in texts.windows(2) {
        assert!(w[0].bounds().1.x < w[1].bounds().0.x, "{texts:?}");
    }
    settle(&mut d, &mut wb, 2);
    browser_row(&d, "Ordinate 1: 3 place(s)");

    // Opened again from the sheet: click the left side's value. Add the hole at (15, 65), which
    // has the x of one there: refused. Made vertical by the panel, the holes at y = 15 are in one
    // place: refused in words, until one is taken away. Measured from the left side's middle
    // (y = 40), the right side's middle is level with the origin: that is refused too.
    let on_text = row_on_screen(&wb, &set, 1);
    double_click(&mut d, &mut wb, on_text);
    assert_eq!(panel(&wb).map(|p| p.annotation), Some(Some(AnnotId(1))));
    let again = in_view(&mut wb, 19.0, 65.0);
    d.click(&mut wb, again);
    assert!(wb.status().contains("in one place"), "{}", wb.status());
    press(&mut d, &mut wb, "tn_set_axis_v");
    press(&mut d, &mut wb, "tn_set_ok");
    assert!(wb.status().contains("in one place") && panel(&wb).is_some(), "{}", wb.status());
    let hole = in_view(&mut wb, 109.0, 15.0);
    d.click(&mut wb, hole);
    press(&mut d, &mut wb, "tn_set_ok");
    assert!(wb.status().contains("where the origin is") && panel(&wb).is_some(), "{}", wb.status());
    let side = in_view(&mut wb, 120.0, 40.0);
    d.click(&mut wb, side);
    d.click(&mut wb, side);
    assert!(wb.status().contains("where the origin is"), "{}", wb.status());
    assert_eq!(panel(&wb).unwrap().targets.len(), 1, "{}", wb.status());
    let top_hole = in_view(&mut wb, 109.0, 65.0);
    d.click(&mut wb, top_hole);
    assert_eq!(panel(&wb).unwrap().targets.len(), 2, "{}", wb.status());
    // A click clear of everything moves it, to the right of the view; OK.
    let beside = in_view(&mut wb, 150.0, 40.0);
    d.click(&mut wb, beside);
    assert!(panel(&wb).is_some());
    press(&mut d, &mut wb, "tn_set_ok");
    assert!(panel(&wb).is_none(), "{}", wb.status());
    let set = info(&mut wb)["annotations"][0].clone();
    assert_eq!(set["axis"], "vertical", "{set}");
    assert_eq!(shown(&set), ["-25", "0", "25"], "{set}");
    let (_, arrows, texts) = drawn(&wb, 1);
    assert_eq!(arrows, 0);
    let (left, _) = (sheet(&mut wb, 150.0, 0.0).x, 0);
    assert!(texts.iter().all(|t| (t.at.x - (left + 1.0)).abs() < 0.2 && t.align == tenon_drawing::Align::Left), "{texts:?}");

    // Esc in the middle of making one leaves nothing, and neither does Cancel.
    press(&mut d, &mut wb, "ribbon drw.ordinate");
    let at = in_view(&mut wb, 0.0, 40.0);
    d.click(&mut wb, at);
    d.tap(&mut wb, egui::Key::Escape);
    assert!(panel(&wb).is_none());
    assert_eq!(info(&mut wb)["annotations"].as_array().unwrap().len(), 1);
}

#[test]
fn a_set_follows_the_model_edited_from_the_drawing() {
    let _quiet = crate::tests::timing_lock();
    // The front view: the plate's thickness is t, 16 in the example's file. From the bottom edge
    // to the top: 16.
    let (mut wb, mut d) = drawing_of_plate("follows", Orientation::Front, 1.0);
    press(&mut d, &mut wb, "ribbon drw.baseline");
    let (bottom, top) = (in_view(&mut wb, 60.0, 0.0), in_view(&mut wb, 60.0, 16.0));
    d.click(&mut wb, bottom);
    d.click(&mut wb, top);
    assert_eq!(panel(&wb).unwrap().targets.len(), 1, "{}", wb.status());
    let place = in_view(&mut wb, 150.0, 8.0);
    d.click(&mut wb, place);
    let i = info(&mut wb);
    assert_eq!(i["annotations"][0]["axis"], "vertical", "to the right of the view: {i}");
    assert_eq!(shown(&i["annotations"][0]), ["16"]);

    // Open Model, t = 20, Return: the same record reads 20.
    wb.drw.as_mut().unwrap().selected = Some(Owner::View(ViewId(1)));
    wb.run_ui("drw.edit_model").unwrap();
    d.frame(&mut wb, vec![]);
    wb.exec("param.set", json!({ "name": "t", "equation": "20" })).unwrap();
    d.frame(&mut wb, vec![]);
    wb.run_ui("drw.return").unwrap();
    d.frame(&mut wb, vec![]);
    let i = info(&mut wb);
    assert_eq!(shown(&i["annotations"][0]), ["20"], "{i}");
}
