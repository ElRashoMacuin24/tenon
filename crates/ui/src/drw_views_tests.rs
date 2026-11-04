//! Auxiliary, crop, break, break out and overlay views through the UI, by real pointer and key
//! input: the ribbon's command, the clicks on the sheet, the panel, its row in the browser.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use egui::{Pos2, vec2};
use serde_json::{Value, json};
use tenon_drawing::graphics::Pen;
use tenon_drawing::{Orientation, Owner, ViewId};
use tenon_geom::Vec2;
use tenon_kernel_occt::OcctKernel;

use crate::Workbench;
use crate::drawing::DrwTool;
use crate::tests::{Driver, browser_row, ctrl, pressable};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-ui-drw-views-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn info(wb: &mut Workbench) -> Value {
    wb.drw_exec("drw.info", json!({})).unwrap()
}

fn view(wb: &mut Workbench, n: usize) -> Value {
    info(wb)["views"][n].clone()
}

/// Where a point of a view (model mm) is on the screen.
fn in_view(wb: &mut Workbench, view: u32, x: f64, y: f64) -> Pos2 {
    let r = wb.drw_exec("drw.to_sheet", json!({ "view": view, "at": [x, y] })).unwrap();
    wb.sheet_on_screen(Vec2::new(r["x"].as_f64().unwrap(), r["y"].as_f64().unwrap()))
}

fn tool(wb: &Workbench) -> Option<DrwTool> {
    wb.drw.as_ref().and_then(|d| d.tool.clone())
}

fn settle(d: &mut Driver, wb: &mut Workbench, n: usize) {
    for _ in 0..n {
        d.frame(wb, vec![]);
    }
}

/// Clicks a text field, selects what is in it and types `text`.
fn type_into(d: &mut Driver, wb: &mut Workbench, field: &str, text: &str) {
    let r = d.ctx.read_response(egui::Id::new(field)).unwrap_or_else(|| panic!("no field {field}")).rect;
    d.click(wb, r.center());
    d.frame(wb, vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }]);
    d.frame(wb, vec![egui::Event::Text(text.into())]);
    d.frame(wb, vec![]);
}

fn double_click(d: &mut Driver, wb: &mut Workbench, at: Pos2) {
    // Pause first: clicks less than 0.6 s after earlier ones would count as a triple click.
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    settle(d, wb, 45);
    d.click(wb, at);
    d.click(wb, at);
}

fn right_click(d: &mut Driver, wb: &mut Workbench, at: Pos2) {
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    d.frame(wb, vec![Driver::button(at, egui::PointerButton::Secondary, true)]);
    d.frame(wb, vec![Driver::button(at, egui::PointerButton::Secondary, false)]);
}

fn press(d: &mut Driver, wb: &mut Workbench, key: &str) {
    settle(d, wb, 2);
    let at = pressable(d, key);
    d.click(wb, at);
    d.frame(wb, vec![]);
}

/// The sheet as primitives.
fn sheet_of(wb: &Workbench) -> tenon_drawing::Graphics {
    let s = wb.drawing().unwrap();
    tenon_drawing::annotate::build(s.drawing(), tenon_drawing::SheetId(1), s.evaluation().unwrap()).0
}

fn drawn(wb: &Workbench, view: u32, pen: Pen) -> Vec<Vec<Vec2>> {
    sheet_of(wb).strokes.into_iter().filter(|s| s.0 == Owner::View(ViewId(view)) && s.1 == pen).map(|s| s.2).collect()
}

fn extent(runs: &[Vec<Vec2>]) -> (Vec2, Vec2) {
    let mut pts = runs.iter().flatten().copied();
    let first = pts.next().expect("something is drawn");
    pts.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)))
}

fn length(runs: &[Vec<Vec2>]) -> f64 {
    runs.iter().map(|r| r.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>()).sum()
}

/// A 120 x 80 x 20 plate with a blind hole 10 across, 10 deep up from below at (60, 40), saved at
/// `path`, in a new drawing: seen from above at 1:1, nothing selected. The view is number 1.
fn plate_view(wb: &mut Workbench, d: &mut Driver, path: &Path) {
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 120, "y2": 80 })).unwrap();
    let body = wb.exec("model.extrude", json!({ "sketch": sk, "distance": 20 })).unwrap()["feature"].clone();
    let under = wb.exec("model.face_ref", json!({ "origin": { "type": "cap", "feature": body, "end": "start" } })).unwrap();
    let holes = wb.exec("sketch.create", json!({ "face": under })).unwrap()["feature"].clone();
    wb.exec("sketch.point", json!({ "sketch": holes, "x": 60, "y": -40 })).unwrap();
    wb.exec("model.hole", json!({ "sketch": holes, "diameter": 10, "depth": 10, "flat_bottom": true })).unwrap();
    wb.save(path).unwrap();
    wb.run_ui("file.new_drawing").unwrap();
    d.frame(wb, vec![]);
    wb.place_base_view(path, Orientation::Top, Some(1.0), true).unwrap();
    d.tap(wb, egui::Key::Escape);
    wb.drw.as_mut().unwrap().selected = None;
    settle(d, wb, 2);
}

#[test]
fn an_auxiliary_view_is_placed_by_clicking_and_changed_from_its_row() {
    let _quiet = crate::tests::timing_lock();
    let dir = scratch("aux");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    // A 100 x 60 plate 25 thick with its corner cut from (100, 30) to (70, 60).
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    let mut last = Value::Null;
    let first = wb.exec("sketch.line", json!({ "sketch": sk, "x1": 0, "y1": 60, "x2": 0, "y2": 0 })).unwrap();
    last.clone_from(&first["end"]);
    for (x, y) in [(100, 0), (100, 30), (70, 60)] {
        let l = wb.exec("sketch.line", json!({ "sketch": sk, "start": last, "x2": x, "y2": y })).unwrap();
        last.clone_from(&l["end"]);
    }
    wb.exec("sketch.line", json!({ "sketch": sk, "start": last, "end": first["start"] })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": sk, "distance": 25 })).unwrap();
    let path = dir.join("wedge.tenon");
    wb.save(&path).unwrap();
    wb.run_ui("file.new_drawing").unwrap();
    d.frame(&mut wb, vec![]);
    wb.place_base_view(&path, Orientation::Top, Some(1.0), true).unwrap();
    d.tap(&mut wb, egui::Key::Escape);
    wb.drw.as_mut().unwrap().selected = None;
    settle(&mut d, &mut wb, 2);

    // Place Views > Auxiliary, the view; a click off any edge says what to do; then the inclined
    // edge, then up and to the right of it.
    wb.run_ui("drw.auxiliary").unwrap();
    assert!(matches!(tool(&wb), Some(DrwTool::Auxiliary { parent: None, .. })));
    let inside = in_view(&mut wb, 1, 30.0, 30.0);
    d.click(&mut wb, inside);
    assert!(matches!(tool(&wb), Some(DrwTool::Auxiliary { parent: Some(ViewId(1)), edge: None, .. })));
    d.click(&mut wb, inside);
    assert!(wb.status().contains("straight edge"), "{}", wb.status());
    let edge = in_view(&mut wb, 1, 85.0, 45.0);
    d.click(&mut wb, edge);
    assert!(matches!(tool(&wb), Some(DrwTool::Auxiliary { edge: Some(_), .. })), "{}", wb.status());
    // The pointer out where it goes: the preview follows, nothing is placed yet.
    let out = edge + vec2(150.0, -150.0);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(out)]);
    settle(&mut d, &mut wb, 2);
    assert_eq!(info(&mut wb)["views"].as_array().unwrap().len(), 1);
    d.click(&mut wb, out);
    assert_eq!(tool(&wb), None);
    let aux = view(&mut wb, 1);
    assert_eq!(aux["kind"]["type"], "auxiliary", "{aux}");
    assert_eq!(aux["name"], "A");
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let z = |a: &Value, i: usize| a["direction"][i].as_f64().unwrap();
    assert!((z(&aux, 0) - s).abs() < 1e-9 && (z(&aux, 1) - s).abs() < 1e-9 && z(&aux, 2).abs() < 1e-9, "{aux}");
    // Lined up with its parent, square to the edge.
    let c = |a: &Value| Vec2::new(a["center"][0].as_f64().unwrap(), a["center"][1].as_f64().unwrap());
    let base = view(&mut wb, 0);
    assert!((c(&aux) - c(&base)).dot(Vec2::new(-s, s)).abs() < 1e-9, "{aux} {base}");

    // Double-click its row: Edit View, with the side it is seen from. Ticked, OK: from the other
    // side, and on the other side of its parent. One undo brings it back.
    settle(&mut d, &mut wb, 2);
    let row = browser_row(&d, "Auxiliary A");
    double_click(&mut d, &mut wb, row.center());
    settle(&mut d, &mut wb, 3);
    assert!(wb.drw.as_ref().unwrap().view_dialog.as_ref().is_some_and(|x| x.flip == Some(false)), "{}", wb.status());
    press(&mut d, &mut wb, "tn_view_flip");
    press(&mut d, &mut wb, "tn_view_ok");
    let flipped = view(&mut wb, 1);
    assert!((z(&flipped, 0) + s).abs() < 1e-9, "{flipped}");
    assert!((c(&flipped) - c(&base)).dot(Vec2::new(s, s)) < 0.0, "{flipped}");
    ctrl(&mut d, &mut wb, egui::Key::Z);
    assert!((z(&view(&mut wb, 1), 0) - s).abs() < 1e-9);

    // Pick Another Edge from there: the bottom edge. It now looks along Y.
    settle(&mut d, &mut wb, 2);
    let row = browser_row(&d, "Auxiliary A");
    double_click(&mut d, &mut wb, row.center());
    press(&mut d, &mut wb, "tn_view_edge");
    assert!(matches!(tool(&wb), Some(DrwTool::Auxiliary { changing: Some(_), .. })), "{}", wb.status());
    let bottom = in_view(&mut wb, 1, 50.0, 0.0);
    d.click(&mut wb, bottom);
    let turned = view(&mut wb, 1);
    assert_eq!(tool(&wb), None, "{}", wb.status());
    assert!(z(&turned, 0).abs() < 1e-9 && (z(&turned, 1).abs() - 1.0).abs() < 1e-9, "{turned}");
    ctrl(&mut d, &mut wb, egui::Key::Z);

    // Dragging it only slides it square to its edge.
    let before = view(&mut wb, 1);
    let lines = drawn(&wb, 2, Pen::Visible);
    let on = lines[0][0] + (lines[0][1] - lines[0][0]) * 0.5;
    let grab = wb.sheet_on_screen(on);
    d.drag(&mut wb, grab, grab + vec2(40.0, 0.0), egui::PointerButton::Primary);
    let after = view(&mut wb, 1);
    let by = c(&after) - c(&before);
    assert!(by.len() > 1.0, "{after}");
    assert!(by.dot(Vec2::new(-s, s)).abs() < 1e-6, "{by:?}");
}

#[test]
fn a_crop_is_clicked_on_a_view_and_changed_from_its_row() {
    let _quiet = crate::tests::timing_lock();
    let dir = scratch("crop");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    plate_view(&mut wb, &mut d, &dir.join("plate.tenon"));
    let i = info(&mut wb);
    let c = Vec2::new(i["views"][0]["center"][0].as_f64().unwrap(), i["views"][0]["center"][1].as_f64().unwrap());

    // Modify > Crop: its panel, the view asked for; then two corners; OK.
    wb.run_ui("drw.crop").unwrap();
    assert!(wb.drw.as_ref().unwrap().mod_panel.is_some());
    assert!(wb.status().contains("view to crop"), "{}", wb.status());
    let inside = in_view(&mut wb, 1, 60.0, 40.0);
    d.click(&mut wb, inside);
    assert_eq!(wb.drw.as_ref().unwrap().mod_panel.as_ref().unwrap().view, Some(ViewId(1)));
    // OK too soon says what is wanted.
    settle(&mut d, &mut wb, 2);
    press(&mut d, &mut wb, "tn_mod_ok");
    assert!(wb.status().contains("corners"), "{}", wb.status());
    assert!(wb.drw.as_ref().unwrap().mod_panel.is_some(), "the panel waits");
    let (p, q) = (in_view(&mut wb, 1, -5.0, -5.0), in_view(&mut wb, 1, 70.0, 50.0));
    d.click(&mut wb, p);
    // The preview follows the pointer; nothing is cut yet.
    d.frame(&mut wb, vec![egui::Event::PointerMoved(q)]);
    settle(&mut d, &mut wb, 2);
    d.click(&mut wb, q);
    assert!(wb.status().contains("OK"), "{}", wb.status());
    assert!(view(&mut wb, 0)["modifiers"].as_array().unwrap().is_empty(), "OK puts it on");
    press(&mut d, &mut wb, "tn_mod_ok");
    assert!(wb.drw.as_ref().unwrap().mod_panel.is_none() && tool(&wb).is_none(), "{}", wb.status());
    // Inside it: the plate's two edges to (70, 50) and the hole, hidden from above.
    let v = view(&mut wb, 0);
    assert_eq!(v["modifiers"][0]["type"], "crop", "{v}");
    let (lo, hi) = extent(&drawn(&wb, 1, Pen::Visible));
    let at = |x: f64, y: f64| c + Vec2::new(x - 60.0, y - 40.0);
    let px = wb.drw.as_ref().unwrap().cam.px;
    assert!(lo.near(at(0.0, 0.0), 2.0 / px) && hi.near(at(70.0, 50.0), 2.0 / px), "{lo:?} {hi:?} {:?}", at(70.0, 50.0));

    // Its row in the browser: double-click, the panel with what it has; a circle typed; OK.
    settle(&mut d, &mut wb, 2);
    let row = browser_row(&d, "Crop");
    double_click(&mut d, &mut wb, row.center());
    settle(&mut d, &mut wb, 3);
    let panel = wb.drw.as_ref().unwrap().mod_panel.clone().expect("its panel");
    assert_eq!(panel.id, Some(1));
    assert!(!panel.circle);
    press(&mut d, &mut wb, "tn_mod_circle");
    type_into(&mut d, &mut wb, "tn_mod_cx", "60");
    type_into(&mut d, &mut wb, "tn_mod_cy", "40");
    type_into(&mut d, &mut wb, "tn_mod_radius", "20");
    press(&mut d, &mut wb, "tn_mod_ok");
    let v = view(&mut wb, 0);
    assert_eq!(v["modifiers"][0]["boundary"]["shape"], "circle", "{v} {}", wb.status());
    assert_eq!(v["modifiers"].as_array().unwrap().len(), 1, "changed, not added: {v}");
    // Clicking its outline selects it; the view's context menu edits it; Delete takes it away.
    let edge = in_view(&mut wb, 1, 80.0, 40.0);
    d.click(&mut wb, edge);
    assert_eq!(wb.drw.as_ref().unwrap().selected, Some(Owner::Mod(ViewId(1), 1)));
    d.tap(&mut wb, egui::Key::Delete);
    assert!(view(&mut wb, 0)["modifiers"].as_array().unwrap().is_empty());
    ctrl(&mut d, &mut wb, egui::Key::Z);
    assert_eq!(view(&mut wb, 0)["modifiers"].as_array().unwrap().len(), 1);
    // Esc closes a panel and changes nothing.
    wb.drw.as_mut().unwrap().selected = None;
    wb.run_ui("drw.crop").unwrap();
    d.tap(&mut wb, egui::Key::Escape);
    assert!(wb.drw.as_ref().unwrap().mod_panel.is_none() && tool(&wb).is_none());
}

#[test]
fn a_break_is_clicked_and_its_lines_select_and_delete() {
    let _quiet = crate::tests::timing_lock();
    let dir = scratch("break");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    plate_view(&mut wb, &mut d, &dir.join("plate.tenon"));

    // Modify > Break with the view selected: straight to the lines. 40 and 80, the gap typed: 8.
    wb.drw.as_mut().unwrap().selected = Some(Owner::View(ViewId(1)));
    wb.run_ui("drw.break").unwrap();
    assert!(wb.status().contains("first break line"), "{}", wb.status());
    let (a, b) = (in_view(&mut wb, 1, 40.0, 20.0), in_view(&mut wb, 1, 80.0, 30.0));
    d.click(&mut wb, a);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(b)]);
    settle(&mut d, &mut wb, 2);
    d.click(&mut wb, b);
    let panel = wb.drw.as_ref().unwrap().mod_panel.clone().unwrap();
    assert!((panel.from.parse::<f64>().unwrap() - 40.0).abs() < 1.0 && (panel.to.parse::<f64>().unwrap() - 80.0).abs() < 1.0, "{panel:?}");
    settle(&mut d, &mut wb, 2);
    type_into(&mut d, &mut wb, "tn_mod_from", "40");
    type_into(&mut d, &mut wb, "tn_mod_to", "80");
    type_into(&mut d, &mut wb, "tn_mod_gap", "8");
    press(&mut d, &mut wb, "tn_mod_ok");
    let v = view(&mut wb, 0);
    assert_eq!(v["modifiers"][0]["type"], "break", "{v} {}", wb.status());
    assert_eq!(v["modifiers"][0]["lines"], "vertical");
    // 120 - 40 + 8 = 88 across.
    assert!((v["size"][0].as_f64().unwrap() - 88.0).abs() < 1e-9, "{v}");
    assert_eq!(v["size"][1], 80.0);

    // A dimension clicked across it: the edge beyond the band, then a place: the true 120.
    d.tap(&mut wb, egui::Key::D);
    let e = in_view(&mut wb, 1, 100.0, 0.0);
    d.click(&mut wb, e);
    d.click(&mut wb, e + vec2(0.0, 50.0));
    d.tap(&mut wb, egui::Key::Escape);
    let i = info(&mut wb);
    assert_eq!(i["annotations"][0]["shown"], "120", "{i}");

    // Its lines are its to click: selected, with their own row; double-click opens the panel.
    let on_line = in_view(&mut wb, 1, 40.0, 10.0);
    d.click(&mut wb, on_line);
    assert_eq!(wb.drw.as_ref().unwrap().selected, Some(Owner::Mod(ViewId(1), 1)), "{}", wb.status());
    double_click(&mut d, &mut wb, on_line);
    settle(&mut d, &mut wb, 3);
    assert_eq!(wb.drw.as_ref().unwrap().mod_panel.as_ref().map(|p| p.id), Some(Some(1)));
    press(&mut d, &mut wb, "tn_mod_cancel");
    assert!(wb.drw.as_ref().unwrap().mod_panel.is_none());
    // Too close: said in the status bar, the panel stays.
    wb.drw.as_mut().unwrap().selected = Some(Owner::View(ViewId(1)));
    wb.run_ui("drw.break").unwrap();
    settle(&mut d, &mut wb, 2);
    press(&mut d, &mut wb, "tn_mod_horizontal");
    type_into(&mut d, &mut wb, "tn_mod_from", "20");
    type_into(&mut d, &mut wb, "tn_mod_to", "22");
    press(&mut d, &mut wb, "tn_mod_ok");
    assert!(wb.status().contains("closer together"), "{}", wb.status());
    assert!(wb.drw.as_ref().unwrap().mod_panel.is_some());
    d.tap(&mut wb, egui::Key::Escape);
    // Suppress from its row's menu, then use it again.
    settle(&mut d, &mut wb, 2);
    let row = browser_row(&d, "Break 1");
    right_click(&mut d, &mut wb, row.center());
    settle(&mut d, &mut wb, 2);
    let s = pressable(&d, "tn_mod_menu_suppress");
    d.click(&mut wb, s);
    settle(&mut d, &mut wb, 2);
    assert_eq!(view(&mut wb, 0)["size"][0], 120.0);
    browser_row(&d, "Break 1 (suppressed)");
    // Selected and Delete: it goes; Ctrl+Z brings it back.
    let row = browser_row(&d, "Break 1 (suppressed)");
    d.click(&mut wb, row.center());
    assert_eq!(wb.drw.as_ref().unwrap().selected, Some(Owner::Mod(ViewId(1), 1)));
    wb.run_ui("drw.delete").unwrap();
    assert!(view(&mut wb, 0)["modifiers"].as_array().unwrap().is_empty());
    ctrl(&mut d, &mut wb, egui::Key::Z);
    assert_eq!(view(&mut wb, 0)["modifiers"].as_array().unwrap().len(), 1);
}

#[test]
fn a_break_out_is_clicked_with_its_depth_typed_and_changed_from_its_row() {
    let _quiet = crate::tests::timing_lock();
    let dir = scratch("break-out");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    plate_view(&mut wb, &mut d, &dir.join("plate.tenon"));
    assert_eq!(view(&mut wb, 0)["hatch"], 0);
    let hidden_before = view(&mut wb, 0)["hidden_curves"].as_u64().unwrap();
    assert!(hidden_before > 0);

    // Modify > Break Out: the view, a rectangle over the hole, 15 down: the floor at 5, across the
    // hole. 60 x 40 less the hole's 25 pi, over the 3 between hatch lines.
    wb.run_ui("drw.break_out").unwrap();
    let inside = in_view(&mut wb, 1, 10.0, 10.0);
    d.click(&mut wb, inside);
    let (p, q) = (in_view(&mut wb, 1, 30.0, 20.0), in_view(&mut wb, 1, 90.0, 60.0));
    d.click(&mut wb, p);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(q)]);
    settle(&mut d, &mut wb, 2);
    d.click(&mut wb, q);
    settle(&mut d, &mut wb, 2);
    type_into(&mut d, &mut wb, "tn_mod_depth", "15");
    press(&mut d, &mut wb, "tn_mod_ok");
    let v = view(&mut wb, 0);
    assert_eq!(v["modifiers"][0]["type"], "break_out", "{v} {}", wb.status());
    assert_eq!(v["modifiers"][0]["depth"], 15.0);
    assert!(v["error"].is_null(), "{v}");
    let px = wb.drw.as_ref().unwrap().cam.px;
    let area = 2400.0 - 25.0 * std::f64::consts::PI;
    let h = length(&drawn(&wb, 1, Pen::Hatch));
    // (The corners were clicked to within a pixel or so.)
    assert!((h - area / 3.0).abs() < area / 3.0 * 0.03 + 200.0 / px / 3.0, "{h} against {}", area / 3.0);

    // Its row: double-click; the depth is there; Through ticked; OK: nothing to hatch.
    settle(&mut d, &mut wb, 2);
    let row = browser_row(&d, "Break Out 1");
    double_click(&mut d, &mut wb, row.center());
    settle(&mut d, &mut wb, 3);
    assert_eq!(wb.drw.as_ref().unwrap().mod_panel.as_ref().unwrap().depth, "15");
    press(&mut d, &mut wb, "tn_mod_through");
    press(&mut d, &mut wb, "tn_mod_ok");
    let v = view(&mut wb, 0);
    assert!(v["modifiers"][0]["depth"].is_null(), "{v} {}", wb.status());
    assert_eq!(v["hatch"], 0, "{v}");
    ctrl(&mut d, &mut wb, egui::Key::Z);
    assert_eq!(view(&mut wb, 0)["modifiers"][0]["depth"], 15.0);
    // A section's view cannot have one: its panel does not open.
    wb.drw_exec("drw.view.section", json!({ "parent": 1, "a": [-5, 40], "b": [125, 40], "at": [100, 60] })).unwrap();
    wb.drw.as_mut().unwrap().selected = Some(Owner::View(ViewId(2)));
    assert!(wb.run_ui("drw.break_out").unwrap_err().contains("base, projected or auxiliary"));
}

/// A 100 x 60 x 10 base, grounded, with a hole of 10 at (20, 20); an arm 60 x 10 x 5 turning on a
/// rotational joint in its hole, 5 from its end. Saved, in a new drawing, with a view from above at
/// 1:1, nothing selected. The joint is relationship 1.
fn pivot_view(wb: &mut Workbench, d: &mut Driver, dir: &Path) {
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 100, "y2": 60 })).unwrap();
    let hole = wb.exec("sketch.circle", json!({ "sketch": sk, "cx": 20, "cy": 20, "r": 5 })).unwrap()["circle"].clone();
    let base = wb.exec("model.extrude", json!({ "sketch": sk, "distance": 10 })).unwrap()["feature"].clone();
    wb.save(&dir.join("base.tenon")).unwrap();
    wb.exec("file.new", json!({ "name": "Arm" })).unwrap();
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 10 })).unwrap();
    let arm_hole = wb.exec("sketch.circle", json!({ "sketch": sk, "cx": 5, "cy": 5, "r": 3 })).unwrap()["circle"].clone();
    let arm = wb.exec("model.extrude", json!({ "sketch": sk, "distance": 5 })).unwrap()["feature"].clone();
    wb.save(&dir.join("arm.tenon")).unwrap();
    wb.run_ui("file.new_assembly").unwrap();
    d.frame(wb, vec![]);
    let insert = |wb: &mut Workbench, f: &str, at: Value| {
        wb.asm_exec("asm.insert", json!({ "path": dir.join(f).to_str().unwrap(), "at": at })).unwrap()["component"].clone()
    };
    let b = insert(wb, "base.tenon", json!([0, 0, 0]));
    let a = insert(wb, "arm.tenon", json!([15, 15, 10]));
    let edge = |wb: &mut Workbench, c: &Value, f: &Value, end: &str, curve: &Value| {
        wb.asm_exec(
            "asm.geom",
            json!({ "component": c, "edge": [{ "type": "cap", "feature": f, "end": end }, { "type": "side", "feature": f, "curve": curve }] }),
        )
        .unwrap()
    };
    let (ga, gb) = (edge(wb, &b, &base, "end", &hole), edge(wb, &a, &arm, "start", &arm_hole));
    wb.asm_exec("asm.joint", json!({ "type": "revolute", "a": ga, "b": gb })).unwrap();
    wb.asm_exec("asm.save", json!({ "path": dir.join("pivot.tenonasm").to_str().unwrap() })).unwrap();
    wb.run_ui("file.new_drawing").unwrap();
    settle(d, wb, 3);
    if wb.drw.is_none() {
        let at = pressable(d, "tn_save_discard");
        d.click(wb, at);
    }
    d.frame(wb, vec![]);
    wb.place_base_view(&dir.join("pivot.tenonasm"), Orientation::Top, Some(1.0), false).unwrap();
    d.tap(wb, egui::Key::Escape);
    wb.drw.as_mut().unwrap().selected = None;
    settle(d, wb, 2);
}

#[test]
fn an_overlay_is_typed_in_its_panel_with_what_it_comes_to_on_the_sheet() {
    let _quiet = crate::tests::timing_lock();
    let dir = scratch("overlay");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    pivot_view(&mut wb, &mut d, &dir);

    // Place Views > Overlay, then the view: its panel, the joint's angle as it is now.
    wb.run_ui("drw.overlay").unwrap();
    assert_eq!(tool(&wb), Some(DrwTool::Overlay));
    let inside = in_view(&mut wb, 1, 50.0, 40.0);
    d.click(&mut wb, inside);
    settle(&mut d, &mut wb, 3);
    let panel = wb.drw.as_ref().unwrap().overlay_panel.clone().expect("its panel");
    assert_eq!(panel.rows.len(), 1, "{panel:?}");
    assert_eq!(panel.rows[0].name, "Rotational:1");
    let now = panel.rows[0].angle_now.unwrap();
    // OK with nothing typed says what is wanted.
    press(&mut d, &mut wb, "tn_overlay_ok");
    assert!(wb.status().contains("at least one value"), "{}", wb.status());

    // A quarter turn typed: the preview shows where it goes (the arm's lines), nothing is placed.
    type_into(&mut d, &mut wb, "tn_overlay_angle_0", &(now + 90.0).to_string());
    settle(&mut d, &mut wb, 2);
    let panel = wb.drw.as_ref().unwrap().overlay_panel.clone().unwrap();
    assert!(panel.problem.is_none(), "{:?}", panel.problem);
    assert!(!panel.preview.is_empty(), "the arm, where it would go");
    assert_eq!(info(&mut wb)["views"].as_array().unwrap().len(), 1);
    // Something that is no number says so in the panel.
    type_into(&mut d, &mut wb, "tn_overlay_offset_0", "up");
    settle(&mut d, &mut wb, 2);
    let problem = wb.drw.as_ref().unwrap().overlay_panel.as_ref().unwrap().problem.clone();
    assert!(problem.is_some_and(|p| p.contains("not a number")));
    let r = d.ctx.read_response(egui::Id::new("tn_overlay_offset_0")).unwrap().rect;
    d.click(&mut wb, r.center());
    d.frame(
        &mut wb,
        vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }],
    );
    d.tap(&mut wb, egui::Key::Backspace);
    settle(&mut d, &mut wb, 2);
    press(&mut d, &mut wb, "tn_overlay_ok");
    assert!(wb.drw.as_ref().unwrap().overlay_panel.is_none(), "{}", wb.status());
    let v = view(&mut wb, 1);
    assert_eq!(v["kind"]["type"], "overlay", "{v}");
    // The arm a quarter turn round: 15 to 25 across and 15 to 75 up (the base's middle (50, 30) at the
    // view's centre), in phantom lines only.
    let center = Vec2::new(v["center"][0].as_f64().unwrap(), v["center"][1].as_f64().unwrap());
    let (lo, hi) = extent(&drawn(&wb, 2, Pen::Phantom));
    let (a, b) = (lo - center + Vec2::new(50.0, 30.0), hi - center + Vec2::new(50.0, 30.0));
    assert!((a.x - 15.0).abs() < 1e-6 && (b.x - 25.0).abs() < 1e-6 && (a.y - 15.0).abs() < 1e-6 && (b.y - 75.0).abs() < 1e-6, "{a:?} {b:?}");

    // Its row opens the panel with the value in it; another value, OK; one undo.
    settle(&mut d, &mut wb, 2);
    let row = browser_row(&d, "Overlay VIEW2");
    double_click(&mut d, &mut wb, row.center());
    settle(&mut d, &mut wb, 3);
    let panel = wb.drw.as_ref().unwrap().overlay_panel.clone().expect("its panel");
    assert_eq!(panel.view, Some(ViewId(2)));
    assert!((panel.rows[0].angle.parse::<f64>().unwrap() - (now + 90.0)).abs() < 1e-3);
    type_into(&mut d, &mut wb, "tn_overlay_angle_0", &(now + 180.0).to_string());
    settle(&mut d, &mut wb, 2);
    press(&mut d, &mut wb, "tn_overlay_ok");
    let (lo, hi) = extent(&drawn(&wb, 2, Pen::Phantom));
    let (a, b) = (lo - center + Vec2::new(50.0, 30.0), hi - center + Vec2::new(50.0, 30.0));
    // Half a turn: from 25 back to -35 across, 15 to 25 up.
    assert!((a.x + 35.0).abs() < 1e-3 && (b.x - 25.0).abs() < 1e-3, "{a:?} {b:?}");
    ctrl(&mut d, &mut wb, egui::Key::Z);
    let (lo, _) = extent(&drawn(&wb, 2, Pen::Phantom));
    assert!(((lo - center + Vec2::new(50.0, 30.0)).x - 15.0).abs() < 1e-6, "undone");
    // Dragging the view takes it along.
    let c0 = view(&mut wb, 1)["center"].clone();
    let lines = drawn(&wb, 1, Pen::Visible);
    let on = lines[0][0] + (lines[0][1] - lines[0][0]) * 0.5;
    let grab = wb.sheet_on_screen(on);
    d.drag(&mut wb, grab, grab + vec2(30.0, 10.0), egui::PointerButton::Primary);
    let c1 = view(&mut wb, 1)["center"].clone();
    assert_ne!(c0, c1, "{}", wb.status());
    assert_eq!(view(&mut wb, 0)["center"], c1, "it stays over its parent");
    // Deleted from the sheet like any view: its row goes, its parent stays.
    d.tap(&mut wb, egui::Key::Escape);
    wb.drw.as_mut().unwrap().selected = Some(Owner::View(ViewId(2)));
    d.tap(&mut wb, egui::Key::Delete);
    assert_eq!(info(&mut wb)["views"].as_array().unwrap().len(), 1, "{}", wb.status());
    ctrl(&mut d, &mut wb, egui::Key::Z);
    assert_eq!(info(&mut wb)["views"].as_array().unwrap().len(), 2);
}

#[test]
fn a_break_out_is_cut_on_the_geometry_thread_and_a_crop_needs_nothing_of_it() {
    let _quiet = crate::tests::timing_lock();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m4-plate");
    let dir = scratch("worker");
    for f in ["plate.tenon", "pin.tenon", "plate-pins.tenonasm", "plate.tenondrw"] {
        std::fs::copy(src.join(f), dir.join(f)).unwrap();
    }
    let mut wb = Workbench::new(|| Box::new(OcctKernel::new()), None, crate::Services::default());
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let idle = |d: &mut Driver, wb: &mut Workbench| {
        for _ in 0..600 {
            d.frame(wb, vec![]);
            if !wb.is_busy() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the geometry thread never finished");
    };
    wb.open(&dir.join("plate.tenondrw")).unwrap();
    idle(&mut d, &mut wb);
    let i = info(&mut wb);
    let top = i["views"].as_array().unwrap().iter().find(|v| v["direction"] == json!([0.0, 0.0, 1.0])).unwrap()["id"].as_u64().unwrap();
    // A crop: at once, with the views current all the while.
    wb.drw_exec("drw.crop", json!({ "view": top, "rect": [[-5, -5], [125, 85]], "outline": false })).unwrap();
    assert!(wb.drawing().unwrap().evaluation().is_some(), "a crop computes nothing");
    // A break out: the views go out of date until the thread is done, then it is hatched.
    let before = i["views"].as_array().unwrap().iter().find(|v| v["id"] == top).unwrap()["hatch"].as_u64().unwrap();
    wb.drw_exec("drw.break_out", json!({ "view": top, "center": [60, 40], "radius": 30, "depth": 4 })).unwrap();
    assert!(wb.drawing().unwrap().evaluation().is_none());
    idle(&mut d, &mut wb);
    let i = info(&mut wb);
    let v = i["views"].as_array().unwrap().iter().find(|v| v["id"] == top).unwrap().clone();
    assert!(v["error"].is_null() && v["hatch"].as_u64().unwrap() > before, "{v}");
}

#[test]
fn the_five_are_on_the_ribbon_and_available() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_ui("file.new_drawing").unwrap();
    d.frame(&mut wb, vec![]);
    for id in ["drw.auxiliary", "drw.crop", "drw.break", "drw.break_out", "drw.overlay"] {
        let c = crate::commands::find(id).unwrap();
        assert!(c.available(), "{id}");
        assert!(wb.run_ui(id).is_ok(), "{id}: {}", wb.status());
        d.tap(&mut wb, egui::Key::Escape);
    }
}
