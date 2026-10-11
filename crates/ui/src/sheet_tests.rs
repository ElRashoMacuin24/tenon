//! Sheet metal through the UI (M8), with real pointer and key input: the Sheet Metal tab, Sheet
//! Metal Defaults, the Face and Flange panels, and the flat pattern with its DXF.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};
use std::path::{Path, PathBuf};

use egui::vec2;
use serde_json::json;
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;
use tenon_model::FeatureKind;
use tenon_model::sheet::{BendPosition, CornerShape};

use crate::Workbench;
use crate::commands::{FLAT_TAB, MODEL_TAB, RIBBON, SHEET_TAB};
use crate::panels::Panel;
use crate::tests::{Driver, browser_row, on_screen, pressable, type_into};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tenon-ui-sheet-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn frames(d: &mut Driver, wb: &mut Workbench) {
    for _ in 0..5 {
        d.frame(wb, vec![]);
    }
}

fn volume(wb: &Workbench) -> f64 {
    wb.scene().bodies.iter().map(|b| b.volume).sum()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

/// Two clicks at one place, after a pause: clicks soon after earlier ones would count as a
/// triple click.
fn double_click(d: &mut Driver, wb: &mut Workbench, at: egui::Pos2) {
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    for _ in 0..45 {
        d.frame(wb, vec![]);
    }
    d.click(wb, at);
    d.click(wb, at);
}

/// The names of the ribbon tabs that show, in order.
fn tabs(wb: &Workbench) -> Vec<&'static str> {
    wb.part_tabs().into_iter().map(|i| RIBBON[i].name).collect()
}

/// Sends the save dialog's answer to `path`.
fn save_to(wb: &mut Workbench, path: &Path) {
    let to = path.to_path_buf();
    wb.services.pick_save = Some(Box::new(move |_: &str, _: &str| Some(to.clone())));
}

/// A `w` x `h` sketch on the XY plane, made a face: a plate of the part's thickness.
fn plate(wb: &mut Workbench, w: f64, h: f64) -> (u64, Vec<u64>) {
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].as_u64().unwrap();
    let r = wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": w, "y2": h })).unwrap();
    let face = wb.exec("sheet.face", json!({ "sketch": sk })).unwrap()["feature"].as_u64().unwrap();
    (face, r["lines"].as_array().unwrap().iter().map(|l| l.as_u64().unwrap()).collect())
}

#[test]
fn a_sheet_metal_part_has_its_own_tab_and_its_defaults_are_typed_into_a_window() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    frames(&mut d, &mut wb);
    // An ordinary part has no Sheet Metal tab; its 3D Model tab offers the way in.
    assert!(!tabs(&wb).contains(&"Sheet Metal") && !tabs(&wb).contains(&"Flat Pattern"), "{:?}", tabs(&wb));
    assert_eq!(wb.chrome.tab, MODEL_TAB);
    assert!(d.texts().iter().any(|t| t == "Convert to"), "the Convert panel: {:?}", d.texts());
    // The sheet metal commands say what is missing.
    let e = wb.run_ui("sheet.face").unwrap_err();
    assert!(e.contains("not a sheet metal part yet") && e.contains("Convert to Sheet Metal"), "{e}");
    assert!(wb.run_ui("sheet.flat_view").unwrap_err().contains("not a sheet metal part yet"));

    // Convert to Sheet Metal is Sheet Metal Defaults: 2 typed for the thickness, a square
    // corner relief clicked, OK.
    wb.run_ui("sheet.convert").unwrap();
    frames(&mut d, &mut wb);
    let texts = d.texts();
    for shown in ["Sheet Metal Defaults", "Thickness", "Bend Radius", "Relief Width", "Minimum Remnant", "K Factor", "Trim to Bend"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    assert!(texts.iter().any(|t| t.starts_with("OK makes this a sheet metal part")), "{texts:?}");
    assert!(wb.document().sheet_metal().is_none(), "nothing changes until OK");
    type_into(&mut d, &mut wb, "tn_sheet_thickness", "2");
    d.click(&mut wb, pressable(&d, "tn_sheet_corner_square"));
    d.click(&mut wb, pressable(&d, "tn_sheet_ok"));
    frames(&mut d, &mut wb);
    let sizes = wb.document().sheet_sizes().unwrap();
    assert_eq!((sizes.thickness, sizes.bend_radius, sizes.relief_depth, sizes.k_factor), (2.0, 2.0, 1.0, 0.44));
    assert_eq!(wb.document().sheet_metal().unwrap().corner, CornerShape::Square);
    assert!(wb.chrome.sheet.defaults.is_none(), "the window closed");
    // Now it is a sheet metal part: its tab comes first and is open.
    assert_eq!(tabs(&wb)[..3], ["Sheet Metal", "3D Model", "Sketch"]);
    assert_eq!(wb.chrome.tab, SHEET_TAB);
    let texts = d.texts();
    for shown in ["Face", "Flange", "Contour", "Corner Round", "Sheet Metal", "Defaults", "Flat Pattern", "Define A-Side"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    // One step: undone, it is an ordinary part again and the tab goes with it.
    wb.run_ui("edit.undo").unwrap();
    frames(&mut d, &mut wb);
    assert!(wb.document().sheet_metal().is_none() && !tabs(&wb).contains(&"Sheet Metal") && wb.chrome.tab == MODEL_TAB);
    wb.run_ui("edit.redo").unwrap();
    frames(&mut d, &mut wb);

    // Opened again it shows the rule as it is: the radius follows the thickness, and says what
    // that comes to. A value that cannot be is refused and the window stays.
    wb.run_ui("sheet.defaults").unwrap();
    frames(&mut d, &mut wb);
    let edit = wb.chrome.sheet.defaults.clone().unwrap();
    assert_eq!((edit.sizes[0].as_str(), edit.sizes[1].as_str(), edit.corner), ("2 mm", "Thickness", CornerShape::Square));
    assert!(d.texts().iter().any(|t| t == "= 2"), "what the bend radius comes to: {:?}", d.texts());
    type_into(&mut d, &mut wb, "tn_sheet_k_factor", "1.4");
    d.click(&mut wb, pressable(&d, "tn_sheet_ok"));
    frames(&mut d, &mut wb);
    assert!(wb.status_error && wb.status().contains("KFactor must be from 0 to 1"), "{}", wb.status());
    assert!(wb.chrome.sheet.defaults.is_some() && wb.document().sheet_sizes().unwrap().k_factor == 0.44);
    type_into(&mut d, &mut wb, "tn_sheet_k_factor", "0.33");
    type_into(&mut d, &mut wb, "tn_sheet_bend_radius", "Thickness * 1.5");
    d.click(&mut wb, pressable(&d, "tn_sheet_relief_round"));
    d.click(&mut wb, pressable(&d, "tn_sheet_ok"));
    frames(&mut d, &mut wb);
    let sizes = wb.document().sheet_sizes().unwrap();
    assert_eq!((sizes.k_factor, sizes.bend_radius), (0.33, 3.0), "{}", wb.status());
    assert_eq!(wb.document().sheet_metal().unwrap().relief, tenon_model::sheet::ReliefShape::Round);
    // Cancel changes nothing.
    wb.run_ui("sheet.defaults").unwrap();
    frames(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_sheet_thickness", "9");
    d.click(&mut wb, pressable(&d, "tn_sheet_cancel"));
    frames(&mut d, &mut wb);
    assert!(wb.chrome.sheet.defaults.is_none() && wb.document().sheet_sizes().unwrap().thickness == 2.0);

    // File > New Sheet Metal Part starts one outright, 1 mm thick.
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    frames(&mut d, &mut wb);
    assert_eq!((wb.document().sheet_sizes().unwrap().thickness, wb.chrome.tab, wb.document().features().len()), (1.0, SHEET_TAB, 0));
    // And a new ordinary part is ordinary.
    wb.run_unprompted("file.new").unwrap();
    frames(&mut d, &mut wb);
    assert!(wb.document().sheet_metal().is_none() && wb.chrome.tab == MODEL_TAB);
}

#[test]
fn a_face_and_a_flange_are_made_from_their_panels_by_clicks() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    // (A tall window: the flange's panel shows whole, without scrolling.)
    let mut d = Driver::new(vec2(1400.0, 1300.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3 })).unwrap();
    // A sketch, then Face while it is still open: the sketch is finished and its profile shown
    // one thickness thick.
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let Some(sk) = wb.document().features().last().map(|f| f.id) else { panic!() };
    wb.exec("sketch.rectangle", json!({ "sketch": sk.0, "x1": 0, "y1": 0, "x2": 40, "y2": 30 })).unwrap();
    wb.run_ui("sheet.face").unwrap();
    frames(&mut d, &mut wb);
    assert!(!wb.is_sketching() && matches!(wb.panel, Some(Panel::SheetFace(_))) && wb.has_properties());
    assert!(near(volume(&wb), 40.0 * 30.0 * 2.0), "the preview: {}", volume(&wb));
    let texts = d.texts();
    for shown in ["Face", "Profiles", "1 selected", "Direction", "Thickness", "2 mm"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    // Enter is OK.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none());
    let made = wb.document().features().last().unwrap();
    assert!(matches!(made.kind, FeatureKind::SheetFace(_)) && made.name == "Face1", "{made:?}");
    assert_eq!(wb.chrome.tab, SHEET_TAB, "back on the part's own tab");

    // Flange: a click on the right-hand edge of the top. The wall shows at once, 20 high (four
    // times the bend's radius and thickness), its outer face through the edge.
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    wb.run_ui("sheet.flange").unwrap();
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Flange(p)) if p.edges.is_empty() && p.height == 20.0 && p.degrees == 90.0));
    assert!(near(volume(&wb), 2400.0), "nothing yet: {}", volume(&wb));
    let edge = on_screen(&wb, Vec3::new(40.0, 15.0, 2.0));
    d.click(&mut wb, edge);
    frames(&mut d, &mut wb);
    let Some(Panel::Flange(p)) = &wb.panel else { panic!("the flange panel") };
    assert_eq!(p.edges.len(), 1);
    let bend = |width: f64| FRAC_PI_2 / 2.0 * (25.0 - 9.0) * width;
    let inside = 35.0 * 30.0 * 2.0 + bend(30.0) + 15.0 * 2.0 * 30.0;
    assert!(near(volume(&wb), inside), "{} {inside}", volume(&wb));
    let texts = d.texts();
    for shown in ["Flange", "Edges", "1 selected", "Height", "Outer Faces", "Bend Position", "Inside", "Width Extents", "By rule", "3 mm"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    // 30 typed for the height; From Edge clicked: the bend starts at the edge, nothing is cut.
    type_into(&mut d, &mut wb, "tn_props_flange_h", "30");
    d.click(&mut wb, pressable(&d, "tn_flange_position"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_flange_position_adjacent"));
    frames(&mut d, &mut wb);
    let adjacent = 40.0 * 30.0 * 2.0 + bend(30.0) + 25.0 * 2.0 * 30.0;
    assert!(near(volume(&wb), adjacent), "{} {adjacent}", volume(&wb));
    // Width: 10 in the middle of the edge; a radius of its own, 1.
    d.click(&mut wb, pressable(&d, "tn_flange_width"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_flange_width_centered"));
    frames(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_flange_w", "12");
    d.click(&mut wb, pressable(&d, "tn_flange_by_rule"));
    frames(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_flange_r", "1");
    frames(&mut d, &mut wb);
    let own = 2400.0 + FRAC_PI_2 / 2.0 * (9.0 - 1.0) * 12.0 + 27.0 * 2.0 * 12.0;
    assert!(near(volume(&wb), own), "{} {own}", volume(&wb));
    // OK from the panel. The feature is as set up, and edits again from the browser.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let (id, kind) = wb.document().features().last().map(|f| (f.id, f.kind.clone())).unwrap();
    let FeatureKind::Flange(f) = kind else { panic!("{kind:?}") };
    assert_eq!((f.height, f.position, f.radius, f.edges.len()), (30.0, BendPosition::Adjacent, Some(1.0), 1));
    assert!(matches!(f.width, tenon_model::sheet::FlangeWidth::Centered { width } if width == 12.0));
    assert!(near(volume(&wb), own));
    let row = browser_row(&d, "Flange1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    let Some(Panel::Flange(p)) = &wb.panel else { panic!("double-click edits the flange: {}", wb.status()) };
    assert_eq!((p.editing, p.height, p.own_radius, p.radius), (Some(id), 30.0, true, 1.0));
    // Its height changed in the panel it is edited in: 10 less of wall.
    type_into(&mut d, &mut wb, "tn_props_flange_h", "20");
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), own - 10.0 * 2.0 * 12.0), "{}", volume(&wb));

    // Cut: a hole drawn on the top is cut through the sheet, one thickness each way from its
    // sketch (so it does not matter which side the sketch is on).
    let top = wb.exec("model.face_ref", json!({ "origin": { "type": "cap", "feature": 2, "end": "end" } })).unwrap();
    wb.create_sketch(json!({ "face": top })).unwrap();
    let Some(hole) = wb.document().features().last().map(|f| f.id) else { panic!() };
    frames(&mut d, &mut wb);
    let at = wb.sketch_frame(hole).unwrap().to_local(Vec3::new(12.0, 15.0, 2.0));
    wb.exec("sketch.circle", json!({ "sketch": hole.0, "cx": at.x, "cy": at.y, "r": 4 })).unwrap();
    wb.run_ui("sheet.cut").unwrap();
    frames(&mut d, &mut wb);
    let Some(Panel::Extrude(p)) = &wb.panel else { panic!("Cut is an extrusion that cuts") };
    assert_eq!((p.operation, p.distance, p.distance_b), (tenon_model::Operation::Cut, 2.0, 2.0));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    let cut = own - 10.0 * 2.0 * 12.0 - 16.0 * PI * 2.0;
    assert!(wb.panel.is_none() && near(volume(&wb), cut), "{} {cut}: {}", volume(&wb), wb.status());
    // Its distances follow the thickness: a thicker sheet is still cut through.
    wb.exec("sheet.defaults", json!({ "thickness": 3 })).unwrap();
    frames(&mut d, &mut wb);
    let flat = wb.exec("sheet.flat", json!({})).unwrap();
    assert_eq!(flat["holes"], 1, "the hole is through: {flat}");
}

#[test]
fn the_flat_pattern_is_shown_in_place_of_the_part_and_written_as_dxf() {
    let dir = scratch("flat");
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3, "corner": "round" })).unwrap();
    let (face, lines) = plate(&mut wb, 60.0, 40.0);
    let edges: Vec<serde_json::Value> = lines
        .iter()
        .map(|l| {
            wb.exec(
                "model.edge_ref",
                json!({ "faces": [{ "type": "cap", "feature": face, "end": "end" }, { "type": "side", "feature": face, "curve": l }] }),
            )
            .unwrap()
        })
        .collect();
    wb.exec("sheet.flange", json!({ "edges": edges, "height": 15 })).unwrap();
    frames(&mut d, &mut wb);
    assert!(!d.texts().iter().any(|t| t == "Go to"), "the button reads Create Flat Pattern: {:?}", d.texts());
    assert!(!wb.document().sheet_metal().unwrap().flat);

    // Create Flat Pattern: the part has one from now on, and the window shows it, with a tab
    // of its own in place of the part's.
    wb.run_ui("sheet.flat_view").unwrap();
    frames(&mut d, &mut wb);
    assert!(wb.document().sheet_metal().unwrap().flat && wb.flat_shown());
    assert_eq!((tabs(&wb), wb.chrome.tab), (vec!["Flat Pattern", "Tools", "View"], FLAT_TAB));
    let ba = FRAC_PI_2 * (3.0 + 0.44 * 2.0);
    let flat = wb.flat_now().expect("worked out when shown").clone().unwrap();
    assert!(near(flat.length, 70.0 + 2.0 * ba) && near(flat.width, 50.0 + 2.0 * ba) && flat.bends.len() == 4 && flat.inner.len() == 4, "{flat:?}");
    let texts = d.texts();
    for shown in [
        "Flat pattern  82.189 x 62.189 mm",
        "Sheet 2 mm, K factor 0.44",
        "Up 90°  R3",
        "82.189",
        "62.189",
        "Go to",
        "Folded Part",
        "Export",
        "Bend Lines",
    ] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    assert!(texts.iter().any(|t| t.starts_with("4 bends, 4 cut-outs, ")), "{texts:?}");
    // The browser has the two states; the flat one is the one shown.
    let (folded, flat_row) = (browser_row(&d, "Folded Model"), browser_row(&d, "Flat Pattern"));
    assert!(folded.top() < flat_row.top());

    // Export DXF from its tab: the file a cutter reads, with the bend lines on their layers.
    let file = dir.join("tray.dxf");
    save_to(&mut wb, &file);
    wb.run_ui("sheet.export_dxf").unwrap();
    assert!(!wb.status_error && wb.status().starts_with("Exported ") && wb.status().contains("82.189 x 62.189 mm"), "{}", wb.status());
    let text = std::fs::read_to_string(&file).unwrap();
    let tags = tenon_dxf::parse(text.as_bytes()).unwrap();
    let sections = tenon_dxf::sections(&tags);
    let on = |layer: &str| tenon_dxf::records(&sections[2].tags).iter().filter(|(_, r)| r.iter().any(|t| t.code == 8 && t.str() == layer)).count();
    // A cross of 12 lines; four reliefs of two lines and an arc; four centre lines; eight
    // extent lines.
    assert_eq!((on("OUTER"), on("INTERIOR"), on("BEND_UP"), on("BEND_DOWN"), on("BEND_EXTENT")), (12, 12, 4, 0, 8), "{text}");
    // With the extent lines hidden they are left out of the file too.
    wb.run_ui("sheet.show_extents").unwrap();
    frames(&mut d, &mut wb);
    wb.run_ui("sheet.export_dxf").unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.contains("BEND_EXTENT\n  0\nLINE") && text.matches("BEND_UP").count() >= 5, "{text}");
    wb.run_ui("sheet.show_extents").unwrap();
    let svg = dir.join("tray.svg");
    save_to(&mut wb, &svg);
    wb.run_ui("sheet.export_svg").unwrap();
    assert!(std::fs::read_to_string(&svg).unwrap().starts_with("<svg "));

    // The part changed while its pattern is shown (the rule, from Tools or a script): the
    // pattern follows.
    wb.exec("sheet.defaults", json!({ "k_factor": 0.5 })).unwrap();
    frames(&mut d, &mut wb);
    let now = wb.flat_now().expect("worked out again").clone().unwrap();
    assert!(near(now.length, 70.0 + 2.0 * FRAC_PI_2 * 4.0), "{}", now.length);
    wb.run_ui("edit.undo").unwrap();
    frames(&mut d, &mut wb);

    // Go to Folded Part; a double-click on Flat Pattern in the browser comes back; the button
    // now reads Go to Flat Pattern.
    wb.run_ui("sheet.folded").unwrap();
    frames(&mut d, &mut wb);
    assert!(!wb.flat_shown() && wb.chrome.tab == SHEET_TAB && volume(&wb) > 1000.0);
    assert!(d.texts().iter().any(|t| t == "Go to"), "{:?}", d.texts());
    let row = browser_row(&d, "Flat Pattern").center();
    double_click(&mut d, &mut wb, row);
    frames(&mut d, &mut wb);
    assert!(wb.flat_shown() && wb.chrome.tab == FLAT_TAB, "{}", wb.status());
    wb.run_ui("sheet.folded").unwrap();
    frames(&mut d, &mut wb);

    // Define A-Side, then a click on the underside: the pattern is laid out from there, and its
    // bends go down.
    wb.look_from(Vec3::new(1.0, -1.0, -1.0));
    d.settle(&mut wb);
    wb.run_ui("sheet.a_side").unwrap();
    frames(&mut d, &mut wb);
    assert!(wb.chrome.sheet.pick_a_side && wb.status().starts_with("Define A-Side"), "{}", wb.status());
    let underside = on_screen(&wb, Vec3::new(30.0, 20.0, 0.0));
    d.click(&mut wb, underside);
    frames(&mut d, &mut wb);
    assert!(!wb.chrome.sheet.pick_a_side && wb.document().sheet_metal().unwrap().a_side.is_some(), "{}", wb.status());
    wb.run_ui("sheet.flat_view").unwrap();
    frames(&mut d, &mut wb);
    let under = wb.flat_now().unwrap().clone().unwrap();
    assert!(under.bends.iter().all(|b| !b.up) && near(under.length, 70.0 + 2.0 * ba), "{under:?}");
    assert!(d.texts().iter().any(|t| t == "Down 90°  R3"), "{:?}", d.texts());
    // Esc gives the pick up.
    wb.run_ui("sheet.folded").unwrap();
    wb.run_ui("sheet.a_side").unwrap();
    assert!(wb.chrome.sheet.pick_a_side);
    wb.run_ui("ui.cancel").unwrap();
    assert!(!wb.chrome.sheet.pick_a_side);

    // What cannot be laid flat says why, in the window, in plain words.
    wb.exec("param.set", json!({ "name": "Thickness", "equation": "2 mm" })).unwrap();
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].clone();
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 20, "y1": 10, "x2": 30, "y2": 20 })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": sk, "distance": 12 })).unwrap();
    wb.run_ui("sheet.flat_view").unwrap();
    frames(&mut d, &mut wb);
    assert!(matches!(wb.flat_now(), Some(Err(e)) if e.contains("not a sheet 2 mm thick all over")), "{:?}", wb.flat_now());
    assert!(d.texts().iter().any(|t| t.starts_with("No flat pattern") && t.contains("not a sheet 2 mm thick")), "{:?}", d.texts());
    // Deleted from the browser's menu (here by its command), the part is folded again.
    wb.run_ui("sheet.delete_flat").unwrap();
    frames(&mut d, &mut wb);
    assert!(!wb.document().sheet_metal().unwrap().flat && !wb.flat_shown() && wb.chrome.tab == SHEET_TAB);
}

#[test]
fn the_flat_pattern_is_worked_out_on_the_geometry_thread() {
    let _quiet = crate::tests::timing_lock();
    let dir = scratch("worker");
    let mut wb = Workbench::new(|| Box::new(OcctKernel::new()), None, crate::Services::default());
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let settle = |d: &mut Driver, wb: &mut Workbench| {
        for _ in 0..1200 {
            d.frame(wb, vec![]);
            if !wb.is_busy() {
                d.frame(wb, vec![]);
                if !wb.is_busy() {
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the geometry thread never finished");
    };
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3 })).unwrap();
    let (face, lines) = plate(&mut wb, 40.0, 30.0);
    // (Without a kernel on this thread an edge is named by its two faces, with where it is.)
    let edge = json!({
        "faces": [{ "type": "cap", "feature": face, "end": "end" }, { "type": "side", "feature": face, "curve": lines[1] }],
        "fingerprint": { "mid": { "x": 40.0, "y": 15.0, "z": 2.0 }, "length": 30.0 },
    });
    wb.exec("sheet.flange", json!({ "edges": [edge], "height": 20 })).unwrap();
    settle(&mut d, &mut wb);
    let folded = 35.0 * 30.0 * 2.0 + FRAC_PI_2 / 2.0 * 16.0 * 30.0 + 15.0 * 2.0 * 30.0;
    assert!(near(volume(&wb), folded), "{} {folded}", volume(&wb));

    // Exported from the File menu while the folded part is shown: the pattern is worked out on
    // the geometry thread first, and the file written when it arrives.
    let file = dir.join("bracket.dxf");
    save_to(&mut wb, &file);
    wb.run_ui("sheet.export_dxf").unwrap();
    assert!(wb.is_busy() && wb.status().starts_with("Laying the part flat"), "{}", wb.status());
    settle(&mut d, &mut wb);
    let ba = FRAC_PI_2 * (3.0 + 0.44 * 2.0);
    assert!(!wb.status_error && wb.status().starts_with("Exported ") && wb.status().contains("bracket.dxf"), "{}", wb.status());
    assert!(std::fs::read_to_string(&file).unwrap().contains("BEND_UP"));
    // Shown, it is there already; changed, it is worked out again.
    wb.run_ui("sheet.flat_view").unwrap();
    settle(&mut d, &mut wb);
    let flat = wb.flat_now().expect("the pattern").clone().unwrap();
    assert!(near(flat.length, 50.0 + ba) && near(flat.width, 30.0) && flat.bends.len() == 1, "{flat:?}");
    wb.exec("sheet.defaults", json!({ "thickness": 3 })).unwrap();
    settle(&mut d, &mut wb);
    let flat = wb.flat_now().expect("worked out again").clone().unwrap();
    assert!(near(flat.length, 34.0 + FRAC_PI_2 * (3.0 + 0.44 * 3.0) + 14.0), "{}", flat.length);
    assert!(d.texts().iter().any(|t| t == "Sheet 3 mm, K factor 0.44"), "{:?}", d.texts());
}
