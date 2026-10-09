//! Drawings through the UI, with real pointer and key input: placing views by clicking,
//! dimensioning by clicking edges, moving and deleting, sections, the browser, editing a view's
//! model from the drawing and coming back, saving and exporting.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use egui::vec2;
use serde_json::{Value, json};
use tenon_drawing::{Orientation, Owner, ViewId};
use tenon_geom::Vec2;
use tenon_kernel_occt::OcctKernel;

use crate::Workbench;
use crate::drawing::DrwTool;
use crate::tests::{Driver, browser_row};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-ui-drw-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The M4 demo's files (examples/m4-plate), copied so tests may change them.
fn m4(name: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/m4-plate");
    let dir = scratch(name);
    for f in ["plate.tenon", "pin.tenon", "plate-pins.tenonasm", "plate.tenondrw"] {
        std::fs::copy(src.join(f), dir.join(f)).unwrap();
    }
    dir
}

fn info(wb: &mut Workbench) -> Value {
    wb.drw_exec("drw.info", json!({})).unwrap()
}

fn center(i: &Value, v: usize) -> Vec2 {
    Vec2::new(i["views"][v]["center"][0].as_f64().unwrap(), i["views"][v]["center"][1].as_f64().unwrap())
}

/// Where a point of a view (model mm) is on the screen.
fn in_view(wb: &mut Workbench, view: u32, x: f64, y: f64) -> egui::Pos2 {
    let r = wb.drw_exec("drw.to_sheet", json!({ "view": view, "at": [x, y] })).unwrap();
    wb.sheet_on_screen(Vec2::new(r["x"].as_f64().unwrap(), r["y"].as_f64().unwrap()))
}

fn click_sheet(d: &mut Driver, wb: &mut Workbench, p: Vec2) {
    let s = wb.sheet_on_screen(p);
    d.click(wb, s);
}

fn ctrl(d: &mut Driver, wb: &mut Workbench, key: egui::Key) {
    d.modifiers(wb, egui::Modifiers::COMMAND);
    d.frame(wb, vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }]);
    d.frame(wb, vec![egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::COMMAND }]);
    d.modifiers(wb, egui::Modifiers::default());
}

fn tool(wb: &Workbench) -> Option<DrwTool> {
    wb.drw.as_ref().and_then(|d| d.tool.clone())
}

#[test]
fn views_and_dimensions_are_placed_by_clicking_on_the_sheet() {
    let dir = m4("place");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_ui("file.new_drawing").unwrap();
    d.frame(&mut wb, vec![]);
    assert!(wb.in_drawing());
    assert_eq!(wb.ribbon_def()[0].name, "Place Views");
    // Base opens the Drawing View dialog; its OK places the view, then views project from it.
    wb.run_ui("drw.base").unwrap();
    assert!(wb.drw.as_ref().unwrap().base_dialog.is_some());
    wb.drw.as_mut().unwrap().base_dialog = None;
    wb.place_base_view(&dir.join("plate.tenon"), Orientation::Front, Some(1.0), true).unwrap();
    d.frame(&mut wb, vec![]);
    assert_eq!(tool(&wb), Some(DrwTool::Projected { parent: Some(ViewId(1)) }));
    let front = center(&info(&mut wb), 0);

    // Click above, then to the right: a top and a right view, lined up with the front view.
    let above = wb.sheet_on_screen(front + Vec2::new(3.0, 75.0));
    d.click(&mut wb, above);
    let right = wb.sheet_on_screen(front + Vec2::new(115.0, -2.0));
    d.click(&mut wb, right);
    d.tap(&mut wb, egui::Key::Escape);
    assert_eq!(tool(&wb), None);
    let i = info(&mut wb);
    assert_eq!(i["views"].as_array().unwrap().len(), 3, "{i}");
    assert_eq!(i["views"][1]["kind"]["side"], "above");
    assert_eq!(i["views"][2]["kind"]["side"], "right");
    assert!((center(&i, 1).x - front.x).abs() < 1e-9 && (center(&i, 2).y - front.y).abs() < 1e-9, "lined up");
    assert_eq!(i["views"][1]["direction"], json!([0.0, 0.0, 1.0]));

    // D, the top view's back edge, then above it: its length. Then the counterbore: its diameter.
    d.tap(&mut wb, egui::Key::D);
    assert!(matches!(tool(&wb), Some(DrwTool::Dimension { .. })));
    let edge = in_view(&mut wb, 2, 60.0, 80.0);
    d.click(&mut wb, edge);
    d.click(&mut wb, edge + vec2(0.0, -30.0));
    let cbore = in_view(&mut wb, 2, 69.0, 40.0);
    d.click(&mut wb, cbore);
    d.click(&mut wb, cbore + vec2(40.0, -40.0));
    // Two edges of the front view: its left and right sides, the plate's width between them.
    let (l, r) = (in_view(&mut wb, 1, 0.0, 10.0), in_view(&mut wb, 1, 120.0, 10.0));
    d.click(&mut wb, l);
    d.click(&mut wb, r);
    d.click(&mut wb, l + vec2(30.0, 40.0));
    d.tap(&mut wb, egui::Key::Escape);
    let i = info(&mut wb);
    let dims: Vec<(String, f64)> =
        i["annotations"].as_array().unwrap().iter().map(|a| (a["dim"].as_str().unwrap().to_owned(), a["value"].as_f64().unwrap())).collect();
    assert_eq!(dims.len(), 3, "{i}");
    assert_eq!(dims[0].0, "horizontal");
    assert!((dims[0].1 - 120.0).abs() < 1e-9);
    assert_eq!(dims[1].0, "diameter");
    assert!((dims[1].1 - 18.0).abs() < 1e-9);
    assert_eq!(dims[2].0, "horizontal");
    assert!((dims[2].1 - 120.0).abs() < 1e-9);

    // Dragging the front view moves the views projected from it and their dimensions; one drag
    // is one undo step (Ctrl+Z).
    let px = wb.drw.as_ref().unwrap().cam.px;
    let grab = in_view(&mut wb, 1, 30.0, 0.0);
    d.drag(&mut wb, grab, grab + vec2(40.0, 0.0), egui::PointerButton::Primary);
    d.frame(&mut wb, vec![]);
    let moved = info(&mut wb);
    for v in 0..3 {
        assert!((center(&moved, v).x - center(&i, v).x - 40.0 / px).abs() < 1e-6, "view {v}");
    }
    assert_eq!(moved["annotations"][0]["offset"], i["annotations"][0]["offset"], "dimensions sit relative to their view");
    ctrl(&mut d, &mut wb, egui::Key::Z);
    assert_eq!(center(&info(&mut wb), 0), center(&i, 0));

    // A section: the top view, the two ends of the line across its middle, then where it goes:
    // placed beyond the line's back side, it is seen from behind (third-angle).
    wb.drw.as_mut().unwrap().selected = None;
    wb.run_ui("drw.section").unwrap();
    let mid = in_view(&mut wb, 2, 60.0, 50.0);
    d.click(&mut wb, mid);
    let (a, b) = (in_view(&mut wb, 2, -8.0, 40.0), in_view(&mut wb, 2, 128.0, 40.0));
    d.click(&mut wb, a);
    d.click(&mut wb, b);
    let place = in_view(&mut wb, 2, 200.0, 70.0);
    d.click(&mut wb, place);
    let i = info(&mut wb);
    let sec = &i["views"][3];
    assert_eq!(sec["name"], "A", "{i}");
    assert!(sec["hatch"].as_u64().unwrap() > 0, "{sec}");
    assert_eq!(sec["direction"], json!([0.0, 1.0, 0.0]), "seen from behind, looking to the front: {sec}");
    assert_eq!(tool(&wb), None);

    // Clicking the diameter's text selects it; Delete removes it.
    let dia = &i["annotations"][1];
    assert_eq!(dia["dim"], "diameter");
    let text_at =
        wb.sheet_on_screen(center(&i, 1) + Vec2::new(dia["offset"]["x"].as_f64().unwrap() + 3.0, dia["offset"]["y"].as_f64().unwrap() + 1.5));
    d.click(&mut wb, text_at);
    assert_eq!(wb.drw.as_ref().unwrap().selected, Some(Owner::Annotation(tenon_drawing::AnnotId(2))));
    d.tap(&mut wb, egui::Key::Delete);
    let after = info(&mut wb);
    assert_eq!(after["annotations"].as_array().unwrap().len(), 2);
    assert!(after["annotations"].as_array().unwrap().iter().all(|a| a["dim"] != "diameter"));
}

#[test]
fn models_edited_from_the_drawing_update_it() {
    let dir = m4("edit");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.open(&dir.join("plate.tenondrw")).unwrap();
    d.frame(&mut wb, vec![]);
    assert!(wb.in_drawing(), "{}", wb.status());
    for row in ["Plate", "Sheet:1 (active)", "Sheet:2", "VIEW1: plate.tenon", "VIEW2", "Section A-A", "Detail B", "VIEW5: plate-pins.tenonasm"] {
        browser_row(&d, row);
    }
    assert!((info(&mut wb)["annotations"][3]["value"].as_f64().unwrap() - 16.0).abs() < 1e-9);

    // Select the front view in the browser and open its model: the part environment, with a
    // way back.
    let r = browser_row(&d, "VIEW1: plate.tenon");
    d.click(&mut wb, r.center());
    assert_eq!(wb.drw.as_ref().unwrap().selected, Some(Owner::View(ViewId(1))));
    wb.run_ui("drw.edit_model").unwrap();
    d.frame(&mut wb, vec![]);
    assert!(!wb.in_drawing() && wb.editing_from_drawing());
    assert_eq!(wb.document().name, "Plate");
    assert_eq!(wb.ribbon_def()[0].name, "3D Model");
    // Drawing commands wait for the return; part commands work.
    assert!(wb.run_ui("drw.dimension").unwrap_err().contains("drawing"));
    wb.exec("param.set", json!({ "name": "t", "equation": "20" })).unwrap();
    d.frame(&mut wb, vec![]);
    wb.run_ui("drw.return").unwrap();
    d.frame(&mut wb, vec![]);
    assert!(wb.in_drawing());
    let i = info(&mut wb);
    assert!((i["annotations"][3]["value"].as_f64().unwrap() - 20.0).abs() < 1e-9, "the thickness follows: {}", i["annotations"][3]);
    assert!((i["views"][0]["size"][1].as_f64().unwrap() - 20.0).abs() < 1e-9);

    // Saving the drawing saves the part changed from it.
    wb.run_ui("file.save").unwrap();
    assert!(wb.status().contains("1 model file"), "{}", wb.status());
    let (doc, _) = tenon_io::project::open(&dir.join("plate.tenon")).unwrap();
    assert_eq!(doc.parameter_values().get("t").copied(), Some(20.0));

    // The assembly on sheet 2: opened from its view, edited, brought back.
    let r = browser_row(&d, "VIEW5: plate-pins.tenonasm");
    d.click(&mut wb, r.center());
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.drw.as_ref().unwrap().sheet, tenon_drawing::SheetId(2), "its sheet is shown");
    wb.run_ui("drw.edit_model").unwrap();
    d.frame(&mut wb, vec![]);
    assert!(wb.in_assembly());
    assert_eq!(wb.assembly().unwrap().assembly().components.len(), 3);
    wb.asm_exec("asm.delete", json!({ "component": 3 })).unwrap();
    wb.run_ui("drw.return").unwrap();
    d.frame(&mut wb, vec![]);
    let i = info(&mut wb);
    assert_eq!(i["annotations"][6]["rows"][1]["quantity"], 1, "one pin left: {}", i["annotations"][6]);

    // PDF, SVG and DXF of the drawing.
    let out = dir.clone();
    wb.services.pick_save = Some(Box::new(move |name: &str, _ext: &str| Some(out.join(name))));
    for (cmd, file) in [("export.pdf", "Plate.pdf"), ("export.svg", "Plate.svg"), ("export.dxf", "Plate.dxf")] {
        wb.run_ui(cmd).unwrap();
        assert!(std::fs::metadata(dir.join(file)).unwrap().len() > 1000, "{file}");
    }
    let pdf = String::from_utf8_lossy(&std::fs::read(dir.join("Plate.pdf")).unwrap()).into_owned();
    assert_eq!(pdf.matches("/Type /Page").count() - pdf.matches("/Type /Pages").count(), 2);

    // Manage > Save Template, then the template applied from Manage > Apply Template.
    wb.run_ui("drw.template.save").unwrap();
    let template = dir.join("ANSI.json");
    let mut t: Value = serde_json::from_slice(&std::fs::read(&template).unwrap()).unwrap();
    t["title_block"]["name"] = json!("SHOP");
    std::fs::write(&template, serde_json::to_vec(&t).unwrap()).unwrap();
    wb.services.pick_open_ext = Some(Box::new(move |ext: &str| (ext == "json").then(|| template.clone())));
    wb.run_ui("drw.template.apply").unwrap();
    assert!(wb.drawing().unwrap().drawing().sheets.iter().all(|s| s.title_block.name == "SHOP"), "{}", wb.status());
    assert!(wb.run_ui("export.step").unwrap_err().contains("Open Model"));

    // A part opened over the drawing closes it.
    wb.open(&dir.join("pin.tenon")).unwrap();
    assert!(wb.drw.is_none() && !wb.in_drawing());
}

#[test]
fn views_are_computed_on_the_geometry_thread() {
    let dir = m4("worker");
    let mut wb = Workbench::new(|| Box::new(OcctKernel::new()), None, crate::Services::default());
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let settle = |d: &mut Driver, wb: &mut Workbench| {
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
    settle(&mut d, &mut wb);
    let i = info(&mut wb);
    assert!((i["annotations"][3]["value"].as_f64().unwrap() - 16.0).abs() < 1e-9, "{i}");
    // A base view with no scale given: the model's size is learnt on the worker, then the view
    // goes on at the largest standard scale that fits (1:1 for the plate on a B sheet).
    wb.run_ui("drw.sheet.new").unwrap();
    wb.place_base_view(&dir.join("plate.tenon"), Orientation::Iso, None, false).unwrap();
    settle(&mut d, &mut wb);
    let i = info(&mut wb);
    let v = i["views"].as_array().unwrap().last().unwrap().clone();
    assert_eq!((v["sheet"].clone(), v["scale"].clone()), (json!(3), json!(1.0)), "{v}");
    assert!(v["visible_curves"].as_u64().unwrap() > 10 && v["error"].is_null(), "{v}");
    assert!(matches!(tool(&wb), Some(DrwTool::Projected { .. })));
}

#[test]
fn tables_balloons_and_text_through_the_tools() {
    let dir = m4("tables");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_ui("file.new_drawing").unwrap();
    d.frame(&mut wb, vec![]);
    wb.place_base_view(&dir.join("plate.tenon"), Orientation::Top, Some(1.0), true).unwrap();
    d.tap(&mut wb, egui::Key::Escape);
    let v = center(&info(&mut wb), 0);

    // Hole table: the view, then where the table goes.
    wb.run_ui("drw.hole_table").unwrap();
    click_sheet(&mut d, &mut wb, v + Vec2::new(10.0, 10.0));
    click_sheet(&mut d, &mut wb, Vec2::new(280.0, 260.0));
    let i = info(&mut wb);
    assert_eq!(i["annotations"][0]["type"], "hole_table");
    assert_eq!(i["annotations"][0]["rows"].as_array().unwrap().len(), 5);

    // A parts list needs an assembly view.
    wb.run_ui("drw.parts_list").unwrap();
    click_sheet(&mut d, &mut wb, v);
    click_sheet(&mut d, &mut wb, Vec2::new(280.0, 120.0));
    assert!(wb.status().contains("assembly"), "{}", wb.status());
    d.tap(&mut wb, egui::Key::Escape);

    // A new sheet with the assembly: B, an edge of a pin, then where the balloon goes.
    wb.run_ui("drw.sheet.new").unwrap();
    d.frame(&mut wb, vec![]);
    wb.place_base_view(&dir.join("plate-pins.tenonasm"), Orientation::Top, Some(1.0), false).unwrap();
    d.tap(&mut wb, egui::Key::Escape);
    let asm_view = 2;
    d.tap(&mut wb, egui::Key::B);
    // The first pin's head (radius 7 about 15, 15) seen from above.
    let head = in_view(&mut wb, asm_view, 22.0, 15.0);
    d.click(&mut wb, head);
    d.click(&mut wb, head + vec2(-60.0, -60.0));
    d.tap(&mut wb, egui::Key::Escape);
    let i = info(&mut wb);
    let balloon = &i["annotations"][1];
    assert_eq!(balloon["type"], "balloon", "{i}");
    assert_eq!(balloon["item"], 2, "the pin is item 2");
    // Attached where it was clicked: on the head's edge.
    let at = balloon["attach"].clone();
    let r = (at["x"].as_f64().unwrap().powi(2) + at["y"].as_f64().unwrap().powi(2)).sqrt();
    assert!((r - 7.0).abs() < 0.2, "{at}");
    // Auto balloon: the plate gets one; the pin already has its own.
    wb.drw.as_mut().unwrap().selected = Some(Owner::View(ViewId(asm_view)));
    wb.run_ui("drw.balloon.auto").unwrap();
    assert_eq!(info(&mut wb)["annotations"].as_array().unwrap().len(), 3);
    // Parts list from the tool.
    wb.drw.as_mut().unwrap().selected = None;
    wb.run_ui("drw.parts_list").unwrap();
    let p = in_view(&mut wb, asm_view, 60.0, 40.0);
    d.click(&mut wb, p);
    click_sheet(&mut d, &mut wb, Vec2::new(300.0, 120.0));
    let i = info(&mut wb);
    assert_eq!(i["annotations"][3]["rows"].as_array().unwrap().len(), 2, "{i}");

    // Text: T, a click, then the text typed into the dialog.
    d.tap(&mut wb, egui::Key::T);
    click_sheet(&mut d, &mut wb, Vec2::new(30.0, 30.0));
    d.frame(&mut wb, vec![]);
    assert!(wb.drw.as_ref().unwrap().text_dialog.is_some());
    d.frame(&mut wb, vec![egui::Event::Text("DEBURR".into())]);
    assert_eq!(wb.drw.as_ref().unwrap().text_dialog.as_ref().unwrap().text, "DEBURR");
}
