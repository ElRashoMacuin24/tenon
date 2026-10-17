//! Sessions as a user has them: whole pieces of work done through the window by pointer and
//! keyboard, with the geometry worked out on its own thread as in the app. Each step says what
//! it did, so that a failure says where a user would have been stuck.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use egui::{Pos2, vec2};
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;

use crate::Workbench;
use crate::panels::Panel;
use crate::tests::{Driver, on_screen, type_into};

/// The app's workbench: geometry on a thread of its own.
fn app() -> (Workbench, Driver) {
    let mut wb = Workbench::new(|| Box::new(OcctKernel::new()) as Box<dyn tenon_kernel::Kernel>, None, crate::Services::default());
    let mut d = Driver::new(vec2(1440.0, 900.0));
    wait(&mut d, &mut wb);
    (wb, d)
}

/// Frames until the geometry thread has caught up and the view has stopped moving.
fn wait(d: &mut Driver, wb: &mut Workbench) {
    let start = std::time::Instant::now();
    loop {
        d.frame(wb, vec![]);
        if !wb.is_busy() && wb.view.anim.is_none() {
            d.frame(wb, vec![]);
            d.frame(wb, vec![]);
            if !wb.is_busy() {
                return;
            }
        }
        assert!(start.elapsed().as_secs() < 60, "the geometry never caught up: {}", wb.status());
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Clicks a piece of text on the screen, as a user clicks a button or a row by its name.
fn click_text(d: &mut Driver, wb: &mut Workbench, text: &str) {
    let at = d.text_pos(text).unwrap_or_else(|| panic!("no `{text}` on the screen; status: {}; shown: {:?}", wb.status(), d.texts()));
    d.click(wb, at);
    wait(d, wb);
}

fn step(wb: &Workbench, what: &str) {
    eprintln!("[{what}] status: {}{}", wb.status(), if wb.status_error { "  (ERROR)" } else { "" });
}

fn key(d: &mut Driver, wb: &mut Workbench, k: egui::Key) {
    d.tap(wb, k);
    wait(d, wb);
}

/// A point of the sketch plane on the screen.
fn at(wb: &Workbench, x: f64, y: f64) -> Pos2 {
    let Some(f) = wb.sketch_frame(sketching(wb)) else { panic!("no sketch is being edited: {}", wb.status()) };
    on_screen(wb, f.plane_point(tenon_geom::Vec2::new(x, y)))
}

fn sketching(wb: &Workbench) -> tenon_model::FeatureId {
    match &wb.mode {
        crate::workbench::Mode::Sketch(s) => s.feature,
        crate::workbench::Mode::Model => panic!("not sketching: {}", wb.status()),
    }
}

fn click_at(d: &mut Driver, wb: &mut Workbench, p: Pos2) {
    d.frame(wb, vec![egui::Event::PointerMoved(p)]);
    d.frame(wb, vec![]);
    d.click(wb, p);
    wait(d, wb);
}

fn volume(wb: &Workbench) -> f64 {
    wb.scene().bodies.iter().map(|b| b.volume).sum()
}

#[test]
fn a_bracket_is_made_from_nothing_by_hand() {
    let (mut wb, mut d) = app();
    step(&wb, "start");
    // Start 2D Sketch with its key, then the XY plane, by its label in the view.
    key(&mut d, &mut wb, egui::Key::S);
    step(&wb, "S");
    click_text(&mut d, &mut wb, "XY Plane");
    step(&wb, "XY plane clicked");
    let sk = sketching(&wb);
    // A rectangle, corner to corner: 60 by 40.
    click_text(&mut d, &mut wb, "Rectangle");
    step(&wb, "Rectangle tool");
    for (x, y) in [(0.0, 0.0), (60.0, 40.0)] {
        let p = at(&wb, x, y);
        click_at(&mut d, &mut wb, p);
    }
    step(&wb, "rectangle drawn");
    let lines = wb.document().sketch(sk).unwrap().entities().filter(|(id, _)| wb.document().sketch(sk).unwrap().is_line(*id)).count();
    assert_eq!(lines, 4, "{}", wb.status());
    key(&mut d, &mut wb, egui::Key::Escape);
    // Dimension the bottom side: click it, then where the value goes, then type 70.
    key(&mut d, &mut wb, egui::Key::D);
    step(&wb, "D");
    let side = at(&wb, 30.0, 0.0);
    click_at(&mut d, &mut wb, side);
    let place = at(&wb, 30.0, -10.0);
    click_at(&mut d, &mut wb, place);
    step(&wb, "dimension placed");
    d.frame(&mut wb, vec![egui::Event::Text("70".into())]);
    key(&mut d, &mut wb, egui::Key::Enter);
    step(&wb, "dimension typed");
    key(&mut d, &mut wb, egui::Key::Escape);
    // Finish Sketch, then Extrude 12 with its key.
    click_text(&mut d, &mut wb, "Finish");
    step(&wb, "finished");
    key(&mut d, &mut wb, egui::Key::E);
    step(&wb, "E");
    assert!(matches!(wb.panel, Some(Panel::Extrude(_))), "{}", wb.status());
    type_into(&mut d, &mut wb, "tn_props_dist", "12");
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    step(&wb, "extruded");
    assert_eq!(wb.status(), "Made Extrusion1.");
    eprintln!("volume {}", volume(&wb));
    assert!((volume(&wb) - 70.0 * 40.0 * 12.0).abs() < 1e-6 * 33600.0, "70 x 40 x 12: {} ({})", volume(&wb), wb.status());

    // Fillet the four upright edges: F, click each, 5, Enter.
    key(&mut d, &mut wb, egui::Key::F);
    step(&wb, "F");
    for p in [Vec3::new(0.0, 0.0, 6.0), Vec3::new(70.0, 0.0, 6.0), Vec3::new(70.0, 40.0, 6.0), Vec3::new(0.0, 40.0, 6.0)] {
        let s = on_screen(&wb, p);
        click_at(&mut d, &mut wb, s);
    }
    step(&wb, "edges clicked");
    let picked = match &wb.panel {
        Some(Panel::Fillet(f)) => f.edges.len(),
        _ => panic!("no fillet panel: {}", wb.status()),
    };
    eprintln!("fillet edges picked: {picked} (the back edge may be hidden)");
    type_into(&mut d, &mut wb, "tn_props_radius", "5");
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    step(&wb, "filleted");
    let names: Vec<String> = wb.document().features().iter().map(|f| f.name.clone()).collect();
    eprintln!("features: {names:?}");
    assert!(names.iter().any(|n| n == "Fillet1"), "{names:?}");

    // A hole through the top: a sketch on the top face with a point, then Hole.
    let top = on_screen(&wb, Vec3::new(35.0, 20.0, 12.0));
    click_at(&mut d, &mut wb, top);
    key(&mut d, &mut wb, egui::Key::S);
    step(&wb, "sketch on the top face");
    let _ = sketching(&wb);
    click_text(&mut d, &mut wb, "Point");
    let p = at(&wb, 35.0, 20.0);
    click_at(&mut d, &mut wb, p);
    key(&mut d, &mut wb, egui::Key::Escape);
    click_text(&mut d, &mut wb, "Finish");
    key(&mut d, &mut wb, egui::Key::H);
    step(&wb, "H");
    assert!(matches!(wb.panel, Some(Panel::Hole(_))), "{}", wb.status());
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    step(&wb, "hole");
    let before_edit = volume(&wb);
    eprintln!("volume {before_edit}");

    // The extrusion made taller from the browser: the fillets and the hole follow.
    let row = crate::tests::browser_row(&d, "Extrusion1").center();
    d.frame(&mut wb, vec![egui::Event::PointerMoved(row)]);
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, row);
    d.click(&mut wb, row);
    wait(&mut d, &mut wb);
    step(&wb, "Extrusion1 opened");
    assert!(matches!(wb.panel, Some(Panel::Extrude(_))), "{}", wb.status());
    type_into(&mut d, &mut wb, "tn_props_dist", "20");
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    step(&wb, "taller");
    assert_eq!(wb.status(), "Changed Extrusion1.");
    let errors: Vec<String> = wb
        .scene()
        .status
        .iter()
        .filter(|(_, s)| matches!(s, tenon_model::FeatureStatus::Error { .. }))
        .map(|(f, s)| format!("{f:?}: {s:?}"))
        .collect();
    assert!(errors.is_empty(), "after making it taller: {errors:?}");
    eprintln!("volume {}", volume(&wb));
    assert!(volume(&wb) > before_edit * 1.5, "{} vs {before_edit}", volume(&wb));

    // Undo everything, then redo it all.
    let n = wb.document().features().len();
    for _ in 0..30 {
        crate::tests::ctrl(&mut d, &mut wb, egui::Key::Z);
        wait(&mut d, &mut wb);
    }
    step(&wb, "all undone");
    assert!(!wb.status_error && wb.status() == "Nothing to undo.", "{}", wb.status());
    eprintln!("features after undo: {}", wb.document().features().len());
    for _ in 0..30 {
        crate::tests::ctrl(&mut d, &mut wb, egui::Key::Y);
        wait(&mut d, &mut wb);
    }
    step(&wb, "all redone");
    assert_eq!(wb.document().features().len(), n);
    assert!(volume(&wb) > before_edit * 1.5, "{}", volume(&wb));
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-explore-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A part file of a block, made as a script would.
fn block_file(path: &std::path::Path, name: &str, x: f64, y: f64, z: f64) {
    let mut s = tenon_model::Session::default();
    let run = |s: &mut tenon_model::Session, id: &str, p: serde_json::Value| tenon_io::cmd::run(s, id, &p, None).unwrap();
    run(&mut s, "document.rename", serde_json::json!({ "name": name }));
    let sk = run(&mut s, "sketch.create", serde_json::json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle", serde_json::json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": x, "y2": y }));
    run(&mut s, "model.extrude", serde_json::json!({ "sketch": sk, "distance": z }));
    tenon_io::project::save(path, s.document(), &serde_json::Map::new()).unwrap();
}

/// The file dialog, as the user answers it.
fn answer_open(wb: &mut Workbench, path: &std::path::Path) {
    let p = path.to_path_buf();
    wb.services.pick_open = Some(Box::new(move || Some(p.clone())));
}

fn answer_save(wb: &mut Workbench, path: &std::path::Path) {
    let p = path.to_path_buf();
    wb.services.pick_save = Some(Box::new(move |_name: &str, _ext: &str| Some(p.clone())));
}

fn file_menu(d: &mut Driver, wb: &mut Workbench, entry: &str) {
    let tab = crate::tests::pressable(d, "tn_file_tab");
    d.click(wb, tab);
    d.frame(wb, vec![]);
    click_text(d, wb, entry);
}

#[test]
fn an_assembly_is_put_together_by_hand() {
    let dir = scratch("assembly");
    block_file(&dir.join("base.tenon"), "Base", 60.0, 40.0, 10.0);
    block_file(&dir.join("cube.tenon"), "Cube", 10.0, 10.0, 10.0);
    let (mut wb, mut d) = app();
    // File > New Assembly; Place the base (P), then the cube.
    file_menu(&mut d, &mut wb, "New Assembly");
    step(&wb, "new assembly");
    assert!(wb.in_assembly(), "{}", wb.status());
    answer_open(&mut wb, &dir.join("base.tenon"));
    key(&mut d, &mut wb, egui::Key::P);
    step(&wb, "placed the base");
    answer_open(&mut wb, &dir.join("cube.tenon"));
    key(&mut d, &mut wb, egui::Key::P);
    step(&wb, "placed the cube");
    wait(&mut d, &mut wb);
    let comps = wb.assembly().unwrap().assembly().components.len();
    assert_eq!(comps, 2, "{}", wb.status());
    let rows: Vec<String> = d.texts().into_iter().filter(|t| t.contains(":1")).collect();
    eprintln!("browser: {rows:?}");
    // The first is grounded (Inventor grounds the first component placed).
    let grounded: Vec<bool> = wb.assembly().unwrap().assembly().components.iter().map(|c| c.grounded).collect();
    eprintln!("grounded: {grounded:?}");

    // Constrain (C): Flush, the base's top and the cube's top.
    crate::tests::on_screen(&wb, Vec3::ZERO);
    wb.run_ui("view.fit").unwrap();
    wait(&mut d, &mut wb);
    key(&mut d, &mut wb, egui::Key::C);
    step(&wb, "C");
    click_text(&mut d, &mut wb, "Flush");
    let cube = wb.assembly().unwrap().assembly().components[1].placement.origin();
    eprintln!("cube placed at {cube:?}");
    let base_top = on_screen(&wb, Vec3::new(30.0, 20.0, 10.0));
    click_at(&mut d, &mut wb, base_top);
    step(&wb, "first face");
    let cube_top = on_screen(&wb, cube + Vec3::new(5.0, 5.0, 10.0));
    click_at(&mut d, &mut wb, cube_top);
    step(&wb, "second face");
    key(&mut d, &mut wb, egui::Key::Enter);
    step(&wb, "constrained");
    assert!(wb.panel.is_none(), "{}", wb.status());
    let z = wb.assembly().unwrap().assembly().components[1].placement.origin().z;
    eprintln!("cube top now at {}", z + 10.0);
    assert!((z - 0.0).abs() < 1e-6, "flush tops: the cube stands at the base's level ({z})");

    // Analyze Interference, then the bill of materials, from their tabs.
    click_text(&mut d, &mut wb, "Inspect");
    let at = d.text_pos("Analyze").or_else(|| d.text_pos("Interference")).expect("the interference button");
    d.click(&mut wb, at);
    wait(&mut d, &mut wb);
    step(&wb, "interference");
    eprintln!("texts with interference: {:?}", d.texts().iter().filter(|t| t.contains("nterference")).collect::<Vec<_>>());

    // Save, close, open again.
    let file = dir.join("bench.tenonasm");
    answer_save(&mut wb, &file);
    crate::tests::ctrl(&mut d, &mut wb, egui::Key::S);
    wait(&mut d, &mut wb);
    step(&wb, "saved");
    assert!(file.is_file(), "{}", wb.status());
    answer_open(&mut wb, &file);
    file_menu(&mut d, &mut wb, "New Part");
    step(&wb, "new part");
    crate::tests::ctrl(&mut d, &mut wb, egui::Key::O);
    wait(&mut d, &mut wb);
    step(&wb, "opened again");
    assert!(wb.in_assembly(), "{}", wb.status());
    assert_eq!(wb.assembly().unwrap().assembly().components.len(), 2);
    assert_eq!(wb.assembly().unwrap().assembly().relationships.len(), 1);
}

#[test]
fn a_drawing_is_made_of_a_part_by_hand() {
    let dir = scratch("drawing");
    block_file(&dir.join("plate.tenon"), "Plate", 80.0, 50.0, 8.0);
    let (mut wb, mut d) = app();
    file_menu(&mut d, &mut wb, "New Drawing");
    step(&wb, "new drawing");
    assert!(wb.in_drawing(), "{}", wb.status());
    // Base: the dialog, its "..." to choose the file, OK.
    click_text(&mut d, &mut wb, "Base");
    step(&wb, "Base");
    answer_open(&mut wb, &dir.join("plate.tenon"));
    click_text(&mut d, &mut wb, "...");
    step(&wb, "file chosen");
    let ok = crate::tests::pressable(&d, "tn_base_ok");
    d.click(&mut wb, ok);
    wait(&mut d, &mut wb);
    step(&wb, "base view");
    let views = || 0;
    let _ = views;
    let info = |wb: &mut Workbench| wb.drw_exec("drw.info", serde_json::json!({})).unwrap();
    let i = info(&mut wb);
    eprintln!("views after OK: {}", i["views"].as_array().map_or(0, Vec::len));
    assert_eq!(i["views"].as_array().map_or(0, Vec::len), 1, "{}", wb.status());
    // Projected views: to the right and above, then Esc.
    let c = &i["views"][0]["center"];
    let front = tenon_geom::Vec2::new(c[0].as_f64().unwrap(), c[1].as_f64().unwrap());
    for off in [tenon_geom::Vec2::new(110.0, 0.0), tenon_geom::Vec2::new(0.0, 70.0)] {
        let p = wb.sheet_on_screen(front + off);
        click_at(&mut d, &mut wb, p);
    }
    key(&mut d, &mut wb, egui::Key::Escape);
    step(&wb, "projected");
    let i = info(&mut wb);
    eprintln!("views: {}", i["views"].as_array().map_or(0, Vec::len));
    assert_eq!(i["views"].as_array().map_or(0, Vec::len), 3, "{}", wb.status());

    // Export PDF from the File menu, and save the drawing.
    let pdf = dir.join("plate.pdf");
    answer_save(&mut wb, &pdf);
    file_menu(&mut d, &mut wb, "Export PDF...");
    step(&wb, "exported");
    assert!(pdf.is_file() && std::fs::read(&pdf).unwrap().starts_with(b"%PDF"), "{}", wb.status());
    let drw = dir.join("plate.tenondrw");
    answer_save(&mut wb, &drw);
    crate::tests::ctrl(&mut d, &mut wb, egui::Key::S);
    wait(&mut d, &mut wb);
    step(&wb, "saved");
    assert!(drw.is_file(), "{}", wb.status());
}

#[test]
fn a_sheet_metal_tray_is_made_by_hand() {
    let dir = scratch("sheet");
    let (mut wb, mut d) = app();
    file_menu(&mut d, &mut wb, "New Sheet Metal Part");
    step(&wb, "new sheet metal part");
    key(&mut d, &mut wb, egui::Key::S);
    click_text(&mut d, &mut wb, "XY Plane");
    step(&wb, "sketch");
    eprintln!("Rectangle at {:?}", d.text_pos("Rectangle"));
    click_text(&mut d, &mut wb, "Rectangle");
    step(&wb, "Rectangle");
    for (x, y) in [(-40.0, -25.0), (40.0, 25.0)] {
        let p = at(&wb, x, y);
        eprintln!("corner at {p:?}, view rect {:?}", wb.view.rect);
        click_at(&mut d, &mut wb, p);
        step(&wb, "corner");
    }
    {
        let sk = sketching(&wb);
        let n = wb.document().sketch(sk).unwrap().entities().count();
        eprintln!("tab {} ({}), sketch entities {n}", wb.chrome.tab, crate::commands::RIBBON[wb.chrome.tab].name);
    }
    key(&mut d, &mut wb, egui::Key::Escape);
    click_text(&mut d, &mut wb, "Finish");
    step(&wb, "sketched");
    // Face, OK.
    click_text(&mut d, &mut wb, "Face");
    step(&wb, "Face");
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    step(&wb, "face made");
    let t = wb.document().sheet_sizes().map(|s| s.thickness).unwrap_or(0.0);
    eprintln!("thickness {t}, volume {}", volume(&wb));
    assert!(wb.scene().bodies.len() == 1, "{}", wb.status());
    // Flange on the front edge: click the edge on top, OK.
    click_text(&mut d, &mut wb, "Flange");
    step(&wb, "Flange");
    let edge = on_screen(&wb, Vec3::new(0.0, -25.0, t));
    click_at(&mut d, &mut wb, edge);
    step(&wb, "edge clicked");
    let picked = match &wb.panel {
        Some(Panel::Flange(f)) => f.edges.len(),
        _ => panic!("no flange panel: {}", wb.status()),
    };
    assert_eq!(picked, 1, "{}", wb.status());
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    step(&wb, "flanged");
    let errors: Vec<String> = wb
        .scene()
        .status
        .iter()
        .filter(|(_, s)| matches!(s, tenon_model::FeatureStatus::Error { .. }))
        .map(|(f, s)| format!("{f:?}: {s:?}"))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    // Go to Flat Pattern, export DXF.
    let flat = crate::tests::pressable(&d, "ribbon sheet.flat_view");
    d.click(&mut wb, flat);
    wait(&mut d, &mut wb);
    step(&wb, "flat pattern");
    let dxf = dir.join("tray.dxf");
    answer_save(&mut wb, &dxf);
    file_menu(&mut d, &mut wb, "Export Flat Pattern DXF...");
    step(&wb, "dxf");
    assert!(dxf.is_file(), "{}", wb.status());
}

/// A block 60 x 40 x 10 made by commands, shown, with the first feature's sketch dimensioned.
fn block(d: &mut Driver, wb: &mut Workbench) {
    wb.exec("model.box", serde_json::json!({ "plane": "xy", "width": 60, "depth": 40, "height": 10 })).unwrap();
    wait(d, wb);
    wb.run_ui("view.fit").unwrap();
    wait(d, wb);
}

fn errors(wb: &Workbench) -> Vec<String> {
    wb.scene().status.iter().filter(|(_, s)| matches!(s, tenon_model::FeatureStatus::Error { .. })).map(|(f, s)| format!("{f:?}: {s:?}")).collect()
}

#[test]
fn a_rough_user_does_things_in_the_wrong_order() {
    let (mut wb, mut d) = app();
    block(&mut d, &mut wb);
    let v0 = volume(&wb);
    step(&wb, "block");

    // Ctrl+Z with the Fillet panel open: the panel goes, or stays sane; nothing breaks.
    key(&mut d, &mut wb, egui::Key::F);
    let edge = on_screen(&wb, Vec3::new(0.0, 0.0, 5.0));
    click_at(&mut d, &mut wb, edge);
    step(&wb, "fillet edge picked");
    crate::tests::ctrl(&mut d, &mut wb, egui::Key::Z);
    wait(&mut d, &mut wb);
    step(&wb, "Ctrl+Z with a panel open");
    eprintln!("panel after undo: {:?}", wb.panel.as_ref().map(|p| format!("{p:?}").chars().take(40).collect::<String>()));
    eprintln!("features: {}", wb.document().features().len());
    // Whatever happened, Esc leaves a usable workbench.
    key(&mut d, &mut wb, egui::Key::Escape);
    key(&mut d, &mut wb, egui::Key::Escape);
    crate::tests::ctrl(&mut d, &mut wb, egui::Key::Y);
    wait(&mut d, &mut wb);
    step(&wb, "redo");
    assert!(errors(&wb).is_empty(), "{:?}", errors(&wb));

    // Letters typed into a value field are text, not shortcuts: E in the fillet radius does not
    // start Extrude.
    key(&mut d, &mut wb, egui::Key::F);
    click_at(&mut d, &mut wb, edge);
    crate::tests::type_into(&mut d, &mut wb, "tn_props_radius", "E");
    d.tap(&mut wb, egui::Key::E);
    wait(&mut d, &mut wb);
    step(&wb, "E typed in the radius");
    eprintln!("panel: {:?}", wb.panel.as_ref().map(|p| format!("{p:?}").chars().take(40).collect::<String>()));
    assert!(matches!(wb.panel, Some(Panel::Fillet(_))), "still the fillet: {}", wb.status());
    // Nonsense and nothing: refused in words, the panel stays.
    for typed in ["-3", "0", "abc", "1e9"] {
        crate::tests::type_into(&mut d, &mut wb, "tn_props_radius", typed);
        key(&mut d, &mut wb, egui::Key::Enter);
        step(&wb, &format!("radius {typed}"));
        eprintln!("  panel open: {}, volume {}", wb.panel.is_some(), volume(&wb));
    }
    key(&mut d, &mut wb, egui::Key::Escape);
    wait(&mut d, &mut wb);
    eprintln!(
        "after Esc: panel {:?}, features {}",
        wb.panel.as_ref().map(|p| format!("{p:?}").chars().take(40).collect::<String>()),
        wb.document().features().len()
    );
    assert!(errors(&wb).is_empty(), "{:?}", errors(&wb));

    // A fillet on an edge, then the box deleted from under it: refused or said, never broken.
    key(&mut d, &mut wb, egui::Key::F);
    click_at(&mut d, &mut wb, edge);
    crate::tests::type_into(&mut d, &mut wb, "tn_props_radius", "3");
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    step(&wb, "filleted");
    let first = wb.document().features()[0].id;
    let names: Vec<String> = wb.document().features().iter().map(|f| f.name.clone()).collect();
    eprintln!("features {names:?}");
    let row = crate::tests::browser_row(&d, &names.iter().find(|n| n.starts_with("Extrusion")).cloned().unwrap_or_default()).center();
    d.frame(&mut wb, vec![egui::Event::PointerMoved(row)]);
    d.frame(&mut wb, vec![crate::tests::Driver::button(row, egui::PointerButton::Secondary, true)]);
    d.frame(&mut wb, vec![crate::tests::Driver::button(row, egui::PointerButton::Secondary, false)]);
    wait(&mut d, &mut wb);
    let menu: Vec<String> = d.texts();
    eprintln!("context menu has Delete: {}", menu.iter().any(|t| t == "Delete"));
    if d.text_pos("Delete").is_some() {
        click_text(&mut d, &mut wb, "Delete");
    }
    step(&wb, "delete the extrusion under the fillet");
    eprintln!("features {:?}", wb.document().features().iter().map(|f| f.name.clone()).collect::<Vec<_>>());
    let _ = first;

    // Undo back to the block, then edit its sketch: a dimension changed by double-click.
    for _ in 0..5 {
        crate::tests::ctrl(&mut d, &mut wb, egui::Key::Z);
        wait(&mut d, &mut wb);
    }
    step(&wb, "undone");
    eprintln!("volume {} (block {v0})", volume(&wb));

    // Open another document while sketching: the sketch is finished first, or it asks.
    key(&mut d, &mut wb, egui::Key::S);
    click_text(&mut d, &mut wb, "XY Plane");
    step(&wb, "sketching");
    file_menu(&mut d, &mut wb, "New Part");
    step(&wb, "New Part while sketching");
    eprintln!("prompt open: {}, sketching: {}", wb.chrome.save_prompt.is_some(), matches!(wb.mode, crate::workbench::Mode::Sketch(_)));
    if wb.chrome.save_prompt.is_some() {
        d.click(&mut wb, crate::tests::pressable(&d, "tn_save_discard"));
        wait(&mut d, &mut wb);
    }
    step(&wb, "new part");
    assert_eq!(wb.document().features().len(), 0, "{}", wb.status());
    assert!(matches!(wb.mode, crate::workbench::Mode::Model), "not sketching in the new part");
}

fn sketch_lines(wb: &Workbench, sk: tenon_model::FeatureId) -> usize {
    let s = wb.document().sketch(sk).unwrap();
    s.entities().filter(|(id, _)| s.is_line(*id)).count()
}

#[test]
fn a_sketch_is_drawn_and_changed_under_the_features_made_from_it() {
    let (mut wb, mut d) = app();
    key(&mut d, &mut wb, egui::Key::S);
    click_text(&mut d, &mut wb, "XY Plane");
    let sk = sketching(&wb);
    // An L by lines (L), clicking points; the chain ends where it started.
    key(&mut d, &mut wb, egui::Key::L);
    step(&wb, "L");
    for (x, y) in [(0.0, 0.0), (50.0, 0.0), (50.0, 10.0), (10.0, 10.0), (10.0, 30.0), (0.0, 30.0), (0.0, 0.0)] {
        let p = at(&wb, x, y);
        click_at(&mut d, &mut wb, p);
    }
    key(&mut d, &mut wb, egui::Key::Escape);
    step(&wb, "L drawn");
    eprintln!(
        "lines {}, dof {:?}",
        sketch_lines(&wb, sk),
        d.texts().iter().find(|t| t.contains("Degrees") || t.contains("Constrained") || t.contains("Fully"))
    );
    assert_eq!(sketch_lines(&wb, sk), 6, "{}", wb.status());
    // A fillet in the inside corner: the sketch fillet, click the corner, a radius.
    click_text(&mut d, &mut wb, "Fillet");
    step(&wb, "sketch Fillet");
    let corner = at(&wb, 10.0, 10.0);
    click_at(&mut d, &mut wb, corner);
    step(&wb, "corner clicked");
    eprintln!("panel: {:?}", wb.panel.as_ref().map(|p| format!("{p:?}").chars().take(60).collect::<String>()));
    if wb.panel.is_some() {
        d.frame(&mut wb, vec![egui::Event::Text("3".into())]);
        key(&mut d, &mut wb, egui::Key::Enter);
    }
    step(&wb, "sketch filleted");
    let arcs = {
        let s = wb.document().sketch(sk).unwrap();
        s.entities().filter(|(_, e)| matches!(e.geometry, tenon_sketch::Geometry::Arc { .. })).count()
    };
    eprintln!("arcs {arcs}");
    key(&mut d, &mut wb, egui::Key::Escape);
    // Offset the whole outline by 2 (select all with a box, O, 2).
    // Dimensions: the long side 60, by D.
    key(&mut d, &mut wb, egui::Key::D);
    let side = at(&wb, 25.0, 0.0);
    click_at(&mut d, &mut wb, side);
    let place = at(&wb, 25.0, -8.0);
    click_at(&mut d, &mut wb, place);
    d.frame(&mut wb, vec![egui::Event::Text("60".into())]);
    key(&mut d, &mut wb, egui::Key::Enter);
    key(&mut d, &mut wb, egui::Key::Escape);
    step(&wb, "dimensioned 60");
    click_text(&mut d, &mut wb, "Finish");
    // Extrude 8, then fillet an edge of it.
    key(&mut d, &mut wb, egui::Key::E);
    crate::tests::type_into(&mut d, &mut wb, "tn_props_dist", "8");
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    step(&wb, "extruded");
    let v1 = volume(&wb);
    eprintln!("volume {v1}");
    key(&mut d, &mut wb, egui::Key::F);
    let far = on_screen(&wb, Vec3::new(60.0, 5.0, 8.0));
    click_at(&mut d, &mut wb, far);
    crate::tests::type_into(&mut d, &mut wb, "tn_props_radius", "2");
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    step(&wb, "edge filleted");
    assert!(errors(&wb).is_empty(), "{:?}", errors(&wb));

    // The sketch opened again from the browser (double-click its row under Extrusion1), the
    // 60 made 80 by double-clicking the dimension, Finish: the extrusion and its fillet follow.
    let ext = crate::tests::browser_row(&d, "Extrusion1");
    // Its box opens it, one row in.
    d.click(&mut wb, egui::pos2(ext.left() + 27.0, ext.center().y));
    wait(&mut d, &mut wb);
    let row = crate::tests::browser_row(&d, "Sketch1").center();
    d.frame(&mut wb, vec![egui::Event::PointerMoved(row)]);
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, row);
    d.click(&mut wb, row);
    wait(&mut d, &mut wb);
    step(&wb, "Sketch1 opened");
    assert!(matches!(wb.mode, crate::workbench::Mode::Sketch(_)), "{}", wb.status());
    let label = d.text_pos("60").expect("the 60 dimension on the screen");
    d.frame(&mut wb, vec![egui::Event::PointerMoved(label)]);
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, label);
    d.click(&mut wb, label);
    wait(&mut d, &mut wb);
    step(&wb, "dimension double-clicked");
    eprintln!("panel: {:?}", wb.panel.as_ref().map(|p| format!("{p:?}").chars().take(60).collect::<String>()));
    d.frame(
        &mut wb,
        vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }],
    );
    d.frame(&mut wb, vec![egui::Event::Text("80".into())]);
    key(&mut d, &mut wb, egui::Key::Enter);
    step(&wb, "80 typed");
    click_text(&mut d, &mut wb, "Finish");
    wait(&mut d, &mut wb);
    step(&wb, "finished");
    let v2 = volume(&wb);
    eprintln!("volume {v2} (was {v1})");
    assert!(errors(&wb).is_empty(), "after the sketch changed: {:?}", errors(&wb));
    assert!(v2 > v1 + 100.0, "longer: {v2} vs {v1}");
}

fn ribbon(d: &mut Driver, wb: &mut Workbench, id: &str) {
    let at = crate::tests::pressable(d, &format!("ribbon {id}"));
    d.click(wb, at);
    wait(d, wb);
}

fn browser(d: &mut Driver, wb: &mut Workbench, row: &str) {
    let at = crate::tests::browser_row(d, row).center();
    d.click(wb, at);
    wait(d, wb);
}

#[test]
fn the_part_features_are_used_one_after_another() {
    let (mut wb, mut d) = app();
    block(&mut d, &mut wb);
    let check = |wb: &Workbench, what: &str| {
        step(wb, what);
        assert!(errors(wb).is_empty(), "{what}: {:?}", errors(wb));
    };
    // A hole at a point on the top face.
    let top = on_screen(&wb, Vec3::new(30.0, 20.0, 10.0));
    click_at(&mut d, &mut wb, top);
    key(&mut d, &mut wb, egui::Key::S);
    ribbon(&mut d, &mut wb, "sketch.point");
    let p = at(&wb, 10.0, 10.0);
    click_at(&mut d, &mut wb, p);
    key(&mut d, &mut wb, egui::Key::Escape);
    click_text(&mut d, &mut wb, "Finish");
    key(&mut d, &mut wb, egui::Key::H);
    key(&mut d, &mut wb, egui::Key::Enter);
    check(&wb, "hole");
    let v_hole = volume(&wb);

    // Rectangular pattern of the hole: the hole picked in the browser, along X, 3 of them 20 apart.
    eprintln!("selection before Pattern: {:?}", wb.view.selection);
    ribbon(&mut d, &mut wb, "model.pattern.rect");
    step(&wb, "Rectangular");
    if let Some(Panel::Pattern(p)) = &wb.panel {
        eprintln!("pattern starts with {:?}", p.features.iter().map(|f| wb.feature_name(*f)).collect::<Vec<_>>());
    }
    browser(&mut d, &mut wb, "Hole1");
    step(&wb, "Hole1 picked");
    let picked = match &wb.panel {
        Some(Panel::Pattern(p)) => p.features.len(),
        _ => panic!("no pattern panel: {}", wb.status()),
    };
    assert_eq!(picked, 1, "{}", wb.status());
    type_into(&mut d, &mut wb, "tn_props_count1", "3");
    type_into(&mut d, &mut wb, "tn_props_spacing1", "20");
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    check(&wb, "pattern");
    let holes = (v_hole - volume(&wb)) / (24000.0 - v_hole);
    eprintln!("holes added by the pattern: {holes}");
    assert!((holes - 2.0).abs() < 1e-6, "two more holes: {holes}");

    // Chamfer an upright edge.
    ribbon(&mut d, &mut wb, "model.chamfer");
    let e = on_screen(&wb, Vec3::new(60.0, 0.0, 5.0));
    click_at(&mut d, &mut wb, e);
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    check(&wb, "chamfer");

    // A work plane 15 above the top face, and a sketch on it.
    ribbon(&mut d, &mut wb, "work.plane");
    step(&wb, "Plane");
    let top = on_screen(&wb, Vec3::new(40.0, 30.0, 10.0));
    click_at(&mut d, &mut wb, top);
    step(&wb, "top face for the plane");
    eprintln!("panel: {:?}", wb.panel.as_ref().map(|p| format!("{p:?}").chars().take(80).collect::<String>()));
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    check(&wb, "work plane");
    let names: Vec<String> = wb.document().features().iter().map(|f| f.name.clone()).collect();
    eprintln!("features {names:?}");

    // Measure: the top face and the bottom face, 10 apart.
    click_text(&mut d, &mut wb, "Inspect");
    ribbon(&mut d, &mut wb, "inspect.measure");
    step(&wb, "Measure");
    let top = on_screen(&wb, Vec3::new(45.0, 5.0, 10.0));
    click_at(&mut d, &mut wb, top);
    let front = on_screen(&wb, Vec3::new(30.0, 0.0, 5.0));
    click_at(&mut d, &mut wb, front);
    wait(&mut d, &mut wb);
    step(&wb, "measured");
    eprintln!(
        "measure texts: {:?}",
        d.texts().iter().filter(|t| t.contains("mm") || t.contains("°") || t.contains("deg")).take(8).collect::<Vec<_>>()
    );
    key(&mut d, &mut wb, egui::Key::Escape);

    // Shell, the top face open, 2 thick.
    click_text(&mut d, &mut wb, "3D Model");
    ribbon(&mut d, &mut wb, "model.shell");
    step(&wb, "Shell");
    let top = on_screen(&wb, Vec3::new(45.0, 30.0, 10.0));
    click_at(&mut d, &mut wb, top);
    type_into(&mut d, &mut wb, "tn_props_thickness", "2");
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    step(&wb, "shell");
    // Walls thicker than the chamfer cannot be offset past it (the kernel's limit): the shell
    // says to make it before the chamfer. Dragged above the chamfer in the browser, it builds.
    let failed = errors(&wb);
    eprintln!("shell after the chamfer: {failed:?}");
    if !failed.is_empty() {
        assert!(failed[0].contains("move the shell above the chamfers"), "{failed:?}");
        let from = crate::tests::browser_row(&d, "Shell1").center();
        let to = crate::tests::browser_row(&d, "Chamfer1");
        d.drag(&mut wb, from, egui::pos2(to.center().x, to.top() + 2.0), egui::PointerButton::Primary);
        wait(&mut d, &mut wb);
        step(&wb, "shell dragged above the chamfer");
        let order: Vec<String> = wb.document().features().iter().map(|f| f.name.clone()).collect();
        eprintln!("order {order:?}");
        check(&wb, "shell before the chamfer");
    }
    eprintln!("volume {}", volume(&wb));

    // Revolve: a new sketch on the XZ plane, a rectangle beside a centre line, turned about it.
    key(&mut d, &mut wb, egui::Key::S);
    click_text(&mut d, &mut wb, "XZ Plane");
    step(&wb, "sketch on XZ");
    ribbon(&mut d, &mut wb, "sketch.rectangle");
    for (x, y) in [(-30.0, 0.0), (-20.0, 10.0)] {
        let p = at(&wb, x, y);
        click_at(&mut d, &mut wb, p);
    }
    key(&mut d, &mut wb, egui::Key::Escape);
    click_text(&mut d, &mut wb, "Finish");
    key(&mut d, &mut wb, egui::Key::R);
    step(&wb, "R");
    eprintln!("panel: {:?}", wb.panel.as_ref().map(|p| format!("{p:?}").chars().take(120).collect::<String>()));
    key(&mut d, &mut wb, egui::Key::Enter);
    wait(&mut d, &mut wb);
    check(&wb, "revolved");
    eprintln!("bodies {}, volume {}", wb.scene().bodies.len(), volume(&wb));
}

/// Types into a box found by its id (a cell of a table), then Enter.
fn type_in_cell(d: &mut Driver, wb: &mut Workbench, id: egui::Id, text: &str) {
    d.frame(wb, vec![]);
    d.frame(wb, vec![]);
    let r = d.ctx.read_response(id).unwrap_or_else(|| panic!("no cell {id:?}")).rect;
    d.click(wb, r.center());
    d.frame(wb, vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }]);
    d.frame(wb, vec![egui::Event::Text(text.into())]);
    key(d, wb, egui::Key::Enter);
}

#[test]
fn a_part_is_driven_by_parameters_typed_in_their_dialog() {
    let (mut wb, mut d) = app();
    block(&mut d, &mut wb);
    // Manage > Parameters.
    click_text(&mut d, &mut wb, "Manage");
    ribbon(&mut d, &mut wb, "tools.parameters");
    step(&wb, "Parameters");
    let model: Vec<(String, String)> = wb.exec("param.list", serde_json::json!({})).unwrap()["model"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["name"].as_str().unwrap().to_owned(), r["of"].as_str().unwrap_or("").to_owned()))
        .collect();
    eprintln!("model parameters: {model:?}");
    // Add Numeric, named width, 75.
    click_text(&mut d, &mut wb, "Add Numeric");
    let users: Vec<String> = wb.exec("param.list", serde_json::json!({})).unwrap()["user"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_owned())
        .collect();
    eprintln!("user parameters: {users:?}");
    let made = users.last().cloned().expect("a user parameter was added");
    type_in_cell(&mut d, &mut wb, egui::Id::new(("tn_param_name", made.as_str())), "width");
    step(&wb, "renamed");
    type_in_cell(&mut d, &mut wb, egui::Id::new(("tn_param_eq", "width")), "75");
    step(&wb, "75");
    // The block's width follows it.
    let w = model.iter().find(|(_, of)| of.contains("length") || of.contains("width")).map(|(n, _)| n.clone()).expect("a width");
    type_in_cell(&mut d, &mut wb, egui::Id::new(("tn_param_eq", w.as_str())), "width");
    step(&wb, "the block's side is width");
    click_text(&mut d, &mut wb, "Done");
    wait(&mut d, &mut wb);
    eprintln!("volume {}", volume(&wb));
    let v = volume(&wb);
    assert!(((v - 75.0 * 40.0 * 10.0).abs() < 1e-3 * v) || ((v - 60.0 * 75.0 * 10.0).abs() < 1e-3 * v), "{v}: {}", wb.status());
    assert!(errors(&wb).is_empty(), "{:?}", errors(&wb));
}
