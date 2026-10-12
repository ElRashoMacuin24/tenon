//! Sheet metal through the UI (M8), with real pointer and key input: the Sheet Metal tab, Sheet
//! Metal Defaults, the panels of its features, Project Flat Pattern in a sketch, and the flat
//! pattern with its DXF.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};
use std::path::{Path, PathBuf};

use egui::vec2;
use serde_json::json;
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;
use tenon_model::FeatureKind;
use tenon_model::sheet::{BendPosition, CornerShape, DoubleBend, FoldLocation, HemShape, Seam, Sides};

use crate::Workbench;
use crate::commands::{FLAT_TAB, MODEL_TAB, RIBBON, SHEET_TAB};
use crate::panels::Panel;
use crate::sheet::HemChoice;
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
    let Some(Panel::SheetCut(p)) = &wb.panel else { panic!("the Cut panel: {}", wb.status()) };
    assert_eq!((p.across, p.distance), (false, 2.0));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    let cut = own - 10.0 * 2.0 * 12.0 - 16.0 * PI * 2.0;
    assert!(wb.panel.is_none() && near(volume(&wb), cut), "{} {cut}: {}", volume(&wb), wb.status());
    // It is an extrusion that cuts, a Thickness each way.
    let Some(FeatureKind::Extrude(e)) = wb.document().features().last().map(|f| f.kind.clone()) else { panic!("an extrusion") };
    assert_eq!((e.operation, e.extent), (tenon_model::Operation::Cut, tenon_model::ExtrudeExtent::TwoSided { forward: 2.0, backward: 2.0 }));
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
    assert!(near(flat.length, 70.0 + 2.0 * ba) && near(flat.width, 50.0 + 2.0 * ba) && flat.bends.len() == 4 && flat.inner.is_empty(), "{flat:?}");
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
    assert!(texts.iter().any(|t| t.starts_with("4 bends, 0 cut-outs, ")), "{texts:?}");
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
    // The outline with its mitred walls and round reliefs (9 lines and 4 arcs to a corner, less
    // the four tops counted twice); four centre lines; eight extent lines.
    assert_eq!((on("OUTER"), on("INTERIOR"), on("BEND_UP"), on("BEND_DOWN"), on("BEND_EXTENT")), (52, 0, 4, 0, 8), "{text}");
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

/// The cross-section of a bend through `angle` with inside radius `r`, in 2 mm sheet.
fn ring(angle: f64, r: f64) -> f64 {
    angle / 2.0 * ((r + 2.0) * (r + 2.0) - r * r)
}

#[test]
fn a_contour_flange_and_a_hem_are_made_from_their_panels_by_clicks() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1300.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3 })).unwrap();
    frames(&mut d, &mut wb);
    // The two are on the Create panel, live: Contour Flange with nothing to follow says what
    // it wants.
    for id in ["sheet.contour_flange", "sheet.hem"] {
        assert!(crate::commands::find(id).is_some_and(|c| c.available()), "{id}");
    }
    let e = wb.run_ui("sheet.contour_flange").unwrap_err();
    assert!(e.contains("draw an open profile in a sketch first"), "{e}");

    // An L drawn on the XZ plane, 30 along and 20 up; Contour Flange while the sketch is still
    // open finishes it and shows the sheet: 50 wide, on the inside of the corner.
    wb.create_sketch(json!({ "plane": "xz" })).unwrap();
    let Some(sk) = wb.document().features().last().map(|f| f.id) else { panic!() };
    let a = wb.exec("sketch.line", json!({ "sketch": sk.0, "x1": 0, "y1": 0, "x2": 30, "y2": 0 })).unwrap();
    wb.exec("sketch.line", json!({ "sketch": sk.0, "start": a["end"], "x2": 30, "y2": 20 })).unwrap();
    wb.run_ui("sheet.contour_flange").unwrap();
    frames(&mut d, &mut wb);
    assert!(!wb.is_sketching() && wb.has_properties());
    assert!(matches!(&wb.panel, Some(Panel::ContourFlange(p)) if p.sketch == sk && p.distance == 50.0 && !p.own_radius));
    let inside = 2.0 * (25.0 + 15.0) + ring(FRAC_PI_2, 3.0);
    assert!(near(volume(&wb), inside * 50.0), "the preview: {} {}", volume(&wb), inside * 50.0);
    let texts = d.texts();
    for shown in ["Contour Flange", "Profile", "Position", "Direction", "Distance", "Bend Radius", "By rule", "3 mm"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    // 40 typed for the distance; Flipped chosen for the position: the sheet is outside the
    // profile, which is the inside of the bend.
    type_into(&mut d, &mut wb, "tn_props_contour_distance", "40");
    d.click(&mut wb, pressable(&d, "tn_contour_position"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_contour_position_flipped"));
    frames(&mut d, &mut wb);
    let outside = 2.0 * (27.0 + 17.0) + ring(FRAC_PI_2, 3.0);
    assert!(near(volume(&wb), outside * 40.0), "{} {}", volume(&wb), outside * 40.0);
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let made = wb.document().features().last().unwrap().clone();
    let FeatureKind::ContourFlange(c) = &made.kind else { panic!("{made:?}") };
    assert_eq!((made.name.as_str(), c.distance, c.position, c.radius), ("Contour Flange1", 40.0, Sides::Flipped, None));
    assert!(browser_row(&d, "Contour Flange1").height() > 0.0);
    // It edits again from the browser.
    wb.edit_feature(made.id).unwrap();
    assert!(matches!(&wb.panel, Some(Panel::ContourFlange(p)) if p.editing == Some(made.id) && p.position == Sides::Flipped));
    wb.panel = None;

    // Hem, on a plate: a click on the right-hand edge of its top. It folds at once, by the
    // rule: half a thickness of gap, four thicknesses long.
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3 })).unwrap();
    plate(&mut wb, 40.0, 30.0);
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    wb.run_ui("sheet.hem").unwrap();
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Hem(p)) if p.edges.is_empty() && p.gap == 1.0 && p.length == 8.0));
    let edge = on_screen(&wb, Vec3::new(40.0, 15.0, 2.0));
    d.click(&mut wb, edge);
    frames(&mut d, &mut wb);
    let Some(Panel::Hem(p)) = &wb.panel else { panic!("the hem panel") };
    assert_eq!(p.edges.len(), 1);
    assert!(near(volume(&wb), 2400.0 + (ring(PI, 0.5) + 5.5 * 2.0) * 30.0), "{}", volume(&wb));
    let texts = d.texts();
    for shown in ["Hem", "Edges", "1 selected", "Type", "Single", "Gap", "Length", "Direction", "Width Extents"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    // Double, and 12 long: folded over twice.
    d.click(&mut wb, pressable(&d, "tn_hem_type"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_hem_type_double"));
    frames(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_hem_length", "12");
    frames(&mut d, &mut wb);
    let double = ring(PI, 2.0) + ring(PI, 0.5) + 2.0 * 5.5 * 2.0;
    assert!(near(volume(&wb), 2400.0 + double * 30.0), "{} {}", volume(&wb), 2400.0 + double * 30.0);
    // Rolled: the fields are a radius (the part's bend radius) and an angle.
    d.click(&mut wb, pressable(&d, "tn_hem_type"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_hem_type_rolled"));
    frames(&mut d, &mut wb);
    let texts = d.texts();
    assert!(texts.iter().any(|t| t == "Radius") && texts.iter().any(|t| t == "Angle") && !texts.iter().any(|t| t == "Gap"), "{texts:?}");
    type_into(&mut d, &mut wb, "tn_props_hem_angle", "270");
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), 2400.0 + ring(1.5 * PI, 3.0) * 30.0), "{}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let made = wb.document().features().last().unwrap().clone();
    let FeatureKind::Hem(h) = &made.kind else { panic!("{made:?}") };
    assert!(
        made.name == "Hem1" && matches!(h.shape, HemShape::Rolled { radius, angle } if radius == 3.0 && (angle - 1.5 * PI).abs() < 1e-12),
        "{h:?}"
    );
    // Its radius is the rule's by name: a change of the rule is a change of the hem.
    wb.exec("sheet.defaults", json!({ "bend_radius": 4 })).unwrap();
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), 2400.0 + ring(1.5 * PI, 4.0) * 30.0), "{}", volume(&wb));
    wb.edit_feature(made.id).unwrap();
    assert!(matches!(&wb.panel, Some(Panel::Hem(p)) if p.editing == Some(made.id) && p.shape == HemChoice::Rolled && p.degrees == 270.0));
}

#[test]
fn flanges_that_meet_are_mitred_unless_the_panel_says_not_and_the_gap_is_the_rules() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1300.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3, "corner": "trim" })).unwrap();
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
    let flange = wb.exec("sheet.flange", json!({ "edges": edges, "height": 15 })).unwrap()["feature"].as_u64().unwrap();
    frames(&mut d, &mut wb);
    // The walls of a tray, brought together until a thickness is left between them: each ends
    // 2 + root 2 from its corner.
    let tray = |walls: f64| 1500.0 * 2.0 + ring(FRAC_PI_2, 3.0) * 160.0 + 10.0 * 2.0 * walls;
    let mitred = |gap: f64| 200.0 - 8.0 * (2.0 + gap / 2f64.sqrt());
    assert!(near(volume(&wb), tray(mitred(2.0))), "{} {}", volume(&wb), tray(mitred(2.0)));
    // The gap is the rule's: 3 typed into Miter Gap in Sheet Metal Defaults.
    wb.run_ui("sheet.defaults").unwrap();
    frames(&mut d, &mut wb);
    assert!(d.texts().iter().any(|t| t == "Miter Gap"), "{:?}", d.texts());
    assert_eq!(wb.chrome.sheet.defaults.as_ref().unwrap().sizes[7], "Thickness");
    type_into(&mut d, &mut wb, "tn_sheet_gap", "3");
    d.click(&mut wb, pressable(&d, "tn_sheet_ok"));
    frames(&mut d, &mut wb);
    assert_eq!(wb.document().sheet_sizes().unwrap().gap, 3.0, "{}", wb.status());
    assert!(near(volume(&wb), tray(mitred(3.0))), "{} {}", volume(&wb), tray(mitred(3.0)));
    // Auto-Mitering unticked in the flange's panel: each wall ends where its bend does.
    wb.edit_feature(tenon_model::FeatureId(flange as u32)).unwrap();
    frames(&mut d, &mut wb);
    let texts = d.texts();
    assert!(texts.iter().any(|t| t == "Auto-Mitering") && texts.iter().any(|t| t == "3 mm"), "{texts:?}");
    d.click(&mut wb, pressable(&d, "tn_flange_miter"));
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), tray(160.0)), "the preview: {} {}", volume(&wb), tray(160.0));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let FeatureKind::Flange(f) = &wb.document().feature(tenon_model::FeatureId(flange as u32)).unwrap().kind else { panic!() };
    assert!(!f.miter);
    assert!(near(volume(&wb), tray(160.0)));
}

#[test]
fn a_long_panel_is_shown_whole_above_the_browser() {
    // A laptop-sized window. The Flange panel is the longest there is: its last section, Corner,
    // is on screen, the buttons under it, and the browser still below them.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    plate(&mut wb, 40.0, 30.0);
    frames(&mut d, &mut wb);
    wb.run_ui("sheet.flange").unwrap();
    frames(&mut d, &mut wb);
    let texts = d.texts();
    for shown in ["Input Geometry", "Width Extents", "Bend Radius", "Corner", "Auto-Mitering"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    let (miter, model) = (pressable(&d, "tn_flange_miter"), browser_row(&d, "Origin"));
    assert!(miter.y > 400.0 && miter.y < model.top() && model.bottom() < 860.0, "{miter:?} {model:?}");
    // A short panel takes no more of the column than it did: the browser has the rest.
    wb.panel = None;
    wb.run_ui("sheet.hem").unwrap();
    frames(&mut d, &mut wb);
    let short = browser_row(&d, "Origin").top();
    assert!(short < model.top() - 40.0, "{short} {}", model.top());
}

#[test]
fn a_corner_seam_is_made_by_clicking_the_ends_of_two_flanges() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1300.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3, "corner": "trim" })).unwrap();
    // Two flanges made one after the other, on the front edge and then the right one: the
    // corner between them is open, with the first one's bend running through it.
    let (face, lines) = plate(&mut wb, 60.0, 40.0);
    for line in &lines[..2] {
        let edge = wb
            .exec(
                "model.edge_ref",
                json!({ "faces": [{ "type": "cap", "feature": face, "end": "end" }, { "type": "side", "feature": face, "curve": line }] }),
            )
            .unwrap();
        wb.exec("sheet.flange", json!({ "edges": [edge], "height": 15 })).unwrap();
    }
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    let open = 55.0 * 35.0 * 2.0 + ring(FRAC_PI_2, 3.0) * 95.0 + 10.0 * 2.0 * 95.0;
    assert!(near(volume(&wb), open), "{} {open}", volume(&wb));

    // Corner Seam is on the Modify panel, live. Nothing changes until both ends are clicked.
    assert!(crate::commands::find("sheet.corner_seam").is_some_and(|c| c.available()));
    wb.run_ui("sheet.corner_seam").unwrap();
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::CornerSeam(p)) if p.edges.is_empty() && p.corner.seam == Seam::Symmetric && !p.corner.own_gap));
    let texts = d.texts();
    for shown in ["Corner Seam", "Edges", "click two edges", "Seam", "Symmetric Gap", "Gap", "By rule", "2 mm", "Relief", "By rule (Trim to Bend)"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    // The end of the front wall, then the end of the side wall.
    let first = on_screen(&wb, Vec3::new(60.0, 0.0, 10.0));
    d.click(&mut wb, first);
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), open), "one edge is no corner yet: {}", volume(&wb));
    let second = on_screen(&wb, Vec3::new(60.0, 5.0, 10.0));
    d.click(&mut wb, second);
    frames(&mut d, &mut wb);
    let Some(Panel::CornerSeam(p)) = &wb.panel else { panic!("the corner seam panel") };
    assert_eq!(p.edges.len(), 2);
    // The corner closes in the preview: the bends end where they cross and each wall stops
    // 2 + root 2 from the corner.
    let closed = |walls: f64| 55.0 * 35.0 * 2.0 + ring(FRAC_PI_2, 3.0) * 90.0 + 10.0 * 2.0 * walls;
    let mitred = 100.0 - 2.0 * (2.0 + 2f64.sqrt());
    assert!(near(volume(&wb), closed(mitred)), "{} {}", volume(&wb), closed(mitred));
    // Overlap chosen from the list: the first wall runs across the end of the second.
    d.click(&mut wb, pressable(&d, "tn_seam_seam"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_seam_seam_overlap"));
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), closed(60.0 + 36.0)), "{} {}", volume(&wb), closed(96.0));
    assert!(d.texts().iter().any(|t| t == "Overlap"), "the overlap's own field shows: {:?}", d.texts());
    // A gap of its own, 1: the second wall stops 1 short of the first.
    d.click(&mut wb, pressable(&d, "tn_seam_gap_by_rule"));
    frames(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_seam_gap", "1");
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), closed(60.0 + 37.0)), "{} {}", volume(&wb), closed(97.0));
    // OK. The feature is what was set up, shows in the browser, and edits again from there.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let made = wb.document().features().last().unwrap().clone();
    let FeatureKind::CornerSeam(c) = &made.kind else { panic!("{made:?}") };
    assert_eq!((made.name.as_str(), c.corner.seam, c.corner.gap, c.corner.relief), ("Corner Seam1", Seam::Overlap, Some(1.0), None));
    assert!(browser_row(&d, "Corner Seam1").height() > 0.0);
    assert!(near(volume(&wb), closed(97.0)));
    wb.edit_feature(made.id).unwrap();
    frames(&mut d, &mut wb);
    assert!(
        matches!(&wb.panel, Some(Panel::CornerSeam(p)) if p.editing == Some(made.id) && p.edges.len() == 2 && p.corner.own_gap && p.corner.gap == 1.0)
    );
    // A third edge clicked takes the place of the second; with one edge OK asks for the other.
    wb.panel = None;
    wb.run_ui("sheet.corner_seam").unwrap();
    frames(&mut d, &mut wb);
    d.click(&mut wb, first);
    frames(&mut d, &mut wb);
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.status_error && wb.status().contains("the edge at the end of each of the two faces"), "{}", wb.status());
    assert!(wb.panel.is_some());
}

#[test]
fn a_flange_s_corners_take_a_seam_and_a_relief_of_their_own_in_its_panel() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1300.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3, "corner": "trim" })).unwrap();
    let (face, lines) = plate(&mut wb, 60.0, 40.0);
    let edges: Vec<serde_json::Value> = lines[..2]
        .iter()
        .map(|l| {
            wb.exec(
                "model.edge_ref",
                json!({ "faces": [{ "type": "cap", "feature": face, "end": "end" }, { "type": "side", "feature": face, "curve": l }] }),
            )
            .unwrap()
        })
        .collect();
    let flange = wb.exec("sheet.flange", json!({ "edges": edges, "height": 15 })).unwrap()["feature"].as_u64().unwrap();
    frames(&mut d, &mut wb);
    let closed = |walls: f64| 55.0 * 35.0 * 2.0 + ring(FRAC_PI_2, 3.0) * 90.0 + 10.0 * 2.0 * walls;
    wb.edit_feature(tenon_model::FeatureId(flange as u32)).unwrap();
    frames(&mut d, &mut wb);
    let texts = d.texts();
    for shown in ["Auto-Mitering", "Seam", "Symmetric Gap", "Gap", "Relief", "By rule (Trim to Bend)"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    // Reverse Overlap: the second edge's wall runs across the end of the first.
    d.click(&mut wb, pressable(&d, "tn_flange_seam"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_flange_seam_reverse"));
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), closed(56.0 + 40.0)), "{} {}", volume(&wb), closed(96.0));
    // A square relief of its own, 8 across by the rule's size: less of the floor, the bends
    // and the walls. Then a size of its own.
    let before = volume(&wb);
    d.click(&mut wb, pressable(&d, "tn_flange_relief"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_flange_relief_square"));
    frames(&mut d, &mut wb);
    let relieved = volume(&wb);
    assert!(relieved < before - 1.0, "{relieved} {before}");
    assert!(d.texts().iter().any(|t| t == "Relief Size") && d.texts().iter().any(|t| t == "8 mm"), "{:?}", d.texts());
    d.click(&mut wb, pressable(&d, "tn_flange_size_by_rule"));
    frames(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_flange_relief_size", "12");
    frames(&mut d, &mut wb);
    assert!(volume(&wb) < relieved - 1.0, "{} {relieved}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let FeatureKind::Flange(f) = &wb.document().feature(tenon_model::FeatureId(flange as u32)).unwrap().kind else { panic!() };
    assert_eq!(
        (f.corner.seam, f.corner.relief, f.corner.relief_size, f.corner.gap),
        (Seam::ReverseOverlap, Some(CornerShape::Square), Some(12.0), None)
    );
    // Auto-Mitering off: the seam and the gap have nothing to say, and are not shown.
    wb.edit_feature(tenon_model::FeatureId(flange as u32)).unwrap();
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_flange_miter"));
    frames(&mut d, &mut wb);
    let texts = d.texts();
    assert!(!texts.iter().any(|t| t == "Seam") && texts.iter().any(|t| t == "Relief"), "{texts:?}");
}

#[test]
fn a_cut_goes_round_a_bend_when_its_panel_is_told_to() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1000.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3 })).unwrap();
    // A plate with a wall bent up from its right edge: the bend line is at x = 55.
    let (face, lines) = plate(&mut wb, 60.0, 40.0);
    let edge = wb
        .exec(
            "model.edge_ref",
            json!({ "faces": [{ "type": "cap", "feature": face, "end": "end" }, { "type": "side", "feature": face, "curve": lines[1] }] }),
        )
        .unwrap();
    wb.exec("sheet.flange", json!({ "edges": [edge], "height": 15 })).unwrap();
    frames(&mut d, &mut wb);
    let whole = volume(&wb);
    // A circle of radius 4 drawn on the top of the plate, centred on the bend line.
    let top = wb.exec("model.face_ref", json!({ "origin": { "type": "cap", "feature": face, "end": "end" } })).unwrap();
    wb.create_sketch(json!({ "face": top })).unwrap();
    let Some(sketch) = wb.document().features().last().map(|f| f.id) else { panic!() };
    frames(&mut d, &mut wb);
    let at = wb.sketch_frame(sketch).unwrap().to_local(Vec3::new(55.0, 20.0, 2.0));
    wb.exec("sketch.circle", json!({ "sketch": sketch.0, "cx": at.x, "cy": at.y, "r": 4 })).unwrap();

    // Cut, from the Modify panel of the Sheet Metal tab: straight through to begin with, one
    // Thickness each way from the sketch, which takes a slice of the bend that is no circle on
    // the flat sheet.
    wb.run_ui("sheet.cut").unwrap();
    frames(&mut d, &mut wb);
    let Some(Panel::SheetCut(p)) = &wb.panel else { panic!("the Cut panel: {}", wb.status()) };
    assert_eq!((p.editing, p.across, p.distance), (None, false, 2.0));
    let texts = d.texts();
    for shown in ["Cut", "Profiles", "1 selected", "Cut Across Bend", "Distance", "fx: Thickness"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    let wrapped = whole - (8.0 * PI * 2.0 + 8.0 * PI * (25.0 - 9.0) / (2.0 * 3.88));
    assert!(volume(&wb) < whole - 8.0 * PI * 2.0 && !near(volume(&wb), wrapped), "{} {wrapped}", volume(&wb));
    // Cut Across Bend ticked: the preview is the hole wrapped round the bend, half of it on the
    // plate and half on the bend.
    d.click(&mut wb, pressable(&d, "tn_cut_across"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::SheetCut(p)) if p.across));
    assert!(near(volume(&wb), wrapped), "{} {wrapped}", volume(&wb));
    // OK: a feature of its own, named Cut, its depth the Thickness by equation.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let (id, name, kind) = wb.document().features().last().map(|f| (f.id, f.name.clone(), f.kind.clone())).unwrap();
    let FeatureKind::CutAcrossBend(c) = kind else { panic!("{kind:?}") };
    assert_eq!((name.as_str(), c.sketch, c.distance), ("Cut1", sketch, Some(2.0)));
    assert!(near(volume(&wb), wrapped), "{} {wrapped}", volume(&wb));
    // In the flat pattern it is the circle that was drawn.
    let flat = wb.exec("sheet.flat", json!({ "curves": true })).unwrap();
    assert!(
        flat["holes"] == 1 && flat["inner"][0][0]["type"] == "circle" && flat["inner"][0][0]["radius"].as_f64().is_some_and(|r| near(r, 4.0)),
        "{flat}"
    );
    // A thicker sheet is still cut through: the depth follows the Thickness.
    wb.exec("sheet.defaults", json!({ "thickness": 3 })).unwrap();
    frames(&mut d, &mut wb);
    assert_eq!(wb.exec("sheet.flat", json!({})).unwrap()["holes"], 1);
    wb.exec("sheet.defaults", json!({ "thickness": 2 })).unwrap();
    frames(&mut d, &mut wb);

    // Edited from the browser it is still a cut across the bend (that cannot be unticked), and
    // a depth typed into it makes a pocket: 1 deep from the top, the inside of the bend.
    let row = browser_row(&d, "Cut1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    let Some(Panel::SheetCut(p)) = &wb.panel else { panic!("double-click edits the cut: {}", wb.status()) };
    assert_eq!((p.editing, p.across, p.distance), (Some(id), true, 2.0));
    d.click(&mut wb, pressable(&d, "tn_cut_across"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::SheetCut(p)) if p.across), "the box is not for unticking here");
    type_into(&mut d, &mut wb, "tn_props_cut_distance", "1");
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    let pocket = whole - (8.0 * PI * 1.0 + 8.0 * PI * (16.0 - 9.0) / (2.0 * 3.88));
    assert!(wb.panel.is_none() && near(volume(&wb), pocket), "{} {pocket}: {}", volume(&wb), wb.status());
}

#[test]
fn project_flat_pattern_shows_the_wall_beyond_a_bend_in_a_sketch_on_the_plate() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1000.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3 })).unwrap();
    let (face, lines) = plate(&mut wb, 60.0, 40.0);
    let edge = wb
        .exec(
            "model.edge_ref",
            json!({ "faces": [{ "type": "cap", "feature": face, "end": "end" }, { "type": "side", "feature": face, "curve": lines[1] }] }),
        )
        .unwrap();
    wb.exec("sheet.flange", json!({ "edges": [edge], "height": 15 })).unwrap();
    let top = wb.exec("model.face_ref", json!({ "origin": { "type": "cap", "feature": face, "end": "end" } })).unwrap();
    wb.create_sketch(json!({ "face": top })).unwrap();
    let Some(sk) = wb.document().features().last().map(|f| f.id) else { panic!() };
    d.settle(&mut wb);
    // Seen from the left, so that the inside of the wall can be clicked.
    wb.look_from(Vec3::new(-1.0, -1.0, 1.0));
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    let flat = |wb: &Workbench| wb.document().projections(sk).iter().filter(|p| p.flat).count();
    assert_eq!(flat(&wb), 0);

    // Project Flat Pattern is under Project Geometry on the Sketch tab. A click on the inside
    // of the wall: its four edges are in the sketch, where the wall lies with the sheet laid
    // flat from the plate.
    assert!(crate::commands::find("sketch.project_flat").is_some_and(|c| c.available()));
    wb.run_ui("sketch.project_flat").unwrap();
    assert_eq!(wb.active_tool_id(), Some("sketch.project_flat"));
    let wall = on_screen(&wb, Vec3::new(58.0, 20.0, 10.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(wall)]);
    d.frame(&mut wb, vec![]);
    assert!(matches!(wb.view.hover, Some(crate::viewport::Pick::Face { .. })), "the face lights up: {:?}", wb.view.hover);
    d.click(&mut wb, wall);
    frames(&mut d, &mut wb);
    assert!(!wb.status_error && wb.status().starts_with("Laid flat: 4 edges"), "{}", wb.status());
    assert_eq!(flat(&wb), 4);
    // The far edge of the wall is the bend's allowance and the wall's 10 beyond the bend line.
    let ba = FRAC_PI_2 * 3.88;
    let frame = wb.sketch_frame(sk).unwrap();
    let sketch = wb.document().sketch(sk).unwrap().clone();
    let far = frame.to_local(Vec3::new(65.0 + ba, 20.0, 2.0));
    let on_far = |entity| sketch.line(entity).is_some_and(|(a, b)| (a.mid(b).x - far.x).abs() < 1e-9 && (a.mid(b).y - far.y).abs() < 1e-9);
    assert!(
        wb.document().projections(sk).iter().any(|p| p.flat && sketch.is_reference(p.entity) && on_far(p.entity)),
        "the top of the wall, laid flat"
    );
    // A click on nothing says what to click; Esc ends the tool.
    let nowhere = on_screen(&wb, Vec3::new(30.0, 20.0, 30.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(nowhere)]);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, nowhere);
    assert!(wb.status_error && wb.status().contains("click a face of the sheet"), "{}", wb.status());
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.active_tool_id(), None);
}

#[test]
fn a_bend_is_made_by_clicking_an_edge_of_each_of_two_faces() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1000.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3 })).unwrap();
    // A plate, and a second one made apart from it: 12 higher, its edge 20 from the first
    // one's edge and facing it. Two pieces of sheet in one part.
    plate(&mut wb, 40.0, 30.0);
    let plane = wb.exec("work.plane", json!({ "base": "xy", "distance": 12 })).unwrap()["feature"].as_u64().unwrap();
    let sk = wb.exec("sketch.create", json!({ "work_plane": plane })).unwrap()["feature"].as_u64().unwrap();
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 60, "y1": 0, "x2": 100, "y2": 30 })).unwrap();
    wb.exec("sheet.face", json!({ "sketch": sk })).unwrap();
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    let plates = 2.0 * 40.0 * 30.0 * 2.0;
    assert!(near(volume(&wb), plates), "{}", volume(&wb));
    let pieces = |wb: &mut Workbench| wb.exec("model.topology", json!({})).unwrap()["bodies"][0]["solids"].as_u64().unwrap();
    assert_eq!(pieces(&mut wb), 2);

    // Bend is on the Create panel, live. Nothing changes until both edges are clicked.
    assert!(crate::commands::find("sheet.bend").is_some_and(|c| c.available()));
    wb.run_ui("sheet.bend").unwrap();
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Bend(p)) if p.edges.is_empty() && !p.own_radius && p.double == DoubleBend::FixEdges && !p.flip_fixed));
    let texts = d.texts();
    for shown in
        ["Bend", "Edges", "click a sharp edge, or two edges", "Bend Radius", "By rule", "3 mm", "Double Bend", "Type", "Fix Edges", "Flip Fixed Edge"]
    {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    let first = on_screen(&wb, Vec3::new(40.0, 15.0, 2.0));
    d.click(&mut wb, first);
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), plates), "one edge is no bend yet: {}", volume(&wb));
    let second = on_screen(&wb, Vec3::new(60.0, 15.0, 14.0));
    d.click(&mut wb, second);
    frames(&mut d, &mut wb);
    let Some(Panel::Bend(p)) = &wb.panel else { panic!("the bend panel") };
    assert_eq!(p.edges.len(), 2, "{}", wb.status());
    // Fix Edges, in the preview: two equal bends and a face between them reach from edge to
    // edge, and neither plate changes.
    let ring = |angle: f64, r: f64| angle / 2.0 * ((r + 2.0) * (r + 2.0) - r * r) * 30.0;
    let angle = (8.0 / 20f64.hypot(-4.0)).asin() - (-4f64).atan2(20.0);
    let web = (20.0 - 8.0 * angle.sin()) / angle.cos();
    let fixed = plates + 2.0 * ring(angle, 3.0) + web * 30.0 * 2.0;
    assert!(near(volume(&wb), fixed), "{} {fixed}: {}", volume(&wb), wb.status());
    // 90 Degree chosen from the list: two quarter turns with 4 of face between them, and the
    // second plate brought the 12 that is left to there.
    d.click(&mut wb, pressable(&d, "tn_bend_double"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_bend_double_90"));
    frames(&mut d, &mut wb);
    let square = plates + 2.0 * ring(FRAC_PI_2, 3.0) + 4.0 * 30.0 * 2.0 + 12.0 * 30.0 * 2.0;
    assert!(near(volume(&wb), square), "{} {square}", volume(&wb));
    // Flip Fixed Edge: the first plate is the one made longer. The volume is the same.
    d.click(&mut wb, pressable(&d, "tn_bend_flip"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Bend(p)) if p.flip_fixed && p.double == DoubleBend::Ninety));
    assert!(near(volume(&wb), square), "{} {square}", volume(&wb));
    // A radius of its own, 2: the bends take 6 of the 12 up and of the 20 along.
    d.click(&mut wb, pressable(&d, "tn_bend_by_rule"));
    frames(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_bend_r", "2");
    frames(&mut d, &mut wb);
    let tighter = plates + 2.0 * ring(FRAC_PI_2, 2.0) + 6.0 * 30.0 * 2.0 + 14.0 * 30.0 * 2.0;
    assert!(near(volume(&wb), tighter), "{} {tighter}", volume(&wb));
    // OK: the part is one piece of sheet, with a Bend in the browser that edits again.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let (id, name, kind) = wb.document().features().last().map(|f| (f.id, f.name.clone(), f.kind.clone())).unwrap();
    let FeatureKind::Bend(b) = kind else { panic!("{kind:?}") };
    assert_eq!((name.as_str(), b.radius, b.double, b.flip_fixed), ("Bend1", Some(2.0), DoubleBend::Ninety, true));
    assert!(near(volume(&wb), tighter) && pieces(&mut wb) == 1, "{}", volume(&wb));
    let flat = wb.exec("sheet.flat", json!({})).unwrap();
    assert_eq!(flat["bends"].as_array().unwrap().len(), 2, "{flat}");
    let row = browser_row(&d, "Bend1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    let Some(Panel::Bend(p)) = &wb.panel else { panic!("double-click edits the bend: {}", wb.status()) };
    assert_eq!((p.editing, p.edges.len(), p.own_radius, p.radius, p.double, p.flip_fixed), (Some(id), 2, true, 2.0, DoubleBend::Ninety, true));
}

#[test]
fn a_face_is_folded_along_a_line_from_its_panel() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1000.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3 })).unwrap();
    // A plate with a line drawn across its top, 40 from its left end.
    let (face, _) = plate(&mut wb, 60.0, 40.0);
    let top = wb.exec("model.face_ref", json!({ "origin": { "type": "cap", "feature": face, "end": "end" } })).unwrap();
    wb.create_sketch(json!({ "face": top })).unwrap();
    let Some(sketch) = wb.document().features().last().map(|f| f.id) else { panic!() };
    frames(&mut d, &mut wb);
    let frame = wb.sketch_frame(sketch).unwrap();
    let (a, b) = (frame.to_local(Vec3::new(40.0, 0.0, 2.0)), frame.to_local(Vec3::new(40.0, 40.0, 2.0)));
    wb.exec("sketch.line", json!({ "sketch": sketch.0, "x1": a.x, "y1": a.y, "x2": b.x, "y2": b.y })).unwrap();

    // Fold is on the Create panel, live: it takes the sketch being drawn, and the preview is
    // a quarter turn up about the line, which takes the bend's allowance of the plate.
    assert!(crate::commands::find("sheet.fold").is_some_and(|c| c.available()));
    wb.run_ui("sheet.fold").unwrap();
    frames(&mut d, &mut wb);
    let Some(Panel::Fold(p)) = &wb.panel else { panic!("the Fold panel: {}", wb.status()) };
    assert_eq!(
        (p.editing, p.sketch, p.degrees, p.own_radius, p.location, p.flip_side, p.flip_direction),
        (None, sketch, 90.0, false, FoldLocation::Centerline, false, false)
    );
    let texts = d.texts();
    for shown in
        ["Fold", "Bend Line", "Flip Side", "Flip Direction", "Fold Location", "Centerline of Bend", "Fold Angle", "Bend Radius", "By rule", "3 mm"]
    {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    let ring = |angle: f64| angle / 2.0 * (25.0 - 9.0) * 40.0;
    let folded = |angle: f64| (60.0 - angle * 3.88) * 40.0 * 2.0 + ring(angle);
    assert!(near(volume(&wb), folded(FRAC_PI_2)), "{} {}", volume(&wb), folded(FRAC_PI_2));
    // What stayed flat: the largest face that looks up.
    let flat_top = |wb: &Workbench| {
        let up = |s: &tenon_kernel::SurfaceKind| matches!(s, tenon_kernel::SurfaceKind::Plane { normal, .. } if normal.z > 0.999);
        wb.scene().bodies.iter().flat_map(|b| &b.faces).filter(|f| up(&f.1.surface)).map(|f| f.1.area).fold(0.0, f64::max)
    };
    let half = FRAC_PI_2 * 3.88 / 2.0;
    assert!(near(flat_top(&wb), (20.0 - half) * 40.0), "{}", flat_top(&wb));
    // Start of Bend from the list: the bend starts at the line, and the 20 beyond it stays whole.
    d.click(&mut wb, pressable(&d, "tn_fold_location"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_fold_location_start"));
    frames(&mut d, &mut wb);
    assert!(near(flat_top(&wb), 20.0 * 40.0), "{}", flat_top(&wb));
    // Flip Side: the 20 is now what folds, and the 40 less the bend stays.
    d.click(&mut wb, pressable(&d, "tn_fold_flip_side"));
    frames(&mut d, &mut wb);
    assert!(near(flat_top(&wb), 40.0 * 40.0), "{}", flat_top(&wb));
    // Flip Direction: it folds down, so nothing of it is above the plate.
    d.click(&mut wb, pressable(&d, "tn_fold_flip_direction"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Fold(p)) if p.flip_side && p.flip_direction && p.location == FoldLocation::Start));
    let top_of = wb.scene().bodies.iter().filter_map(|b| b.bbox.as_ref()).map(|b| b.max.z).fold(f64::NEG_INFINITY, f64::max);
    assert!(top_of < 2.0 + 1e-6, "{top_of}");
    // 45 degrees typed: less of a turn, less of the plate taken by the bend.
    type_into(&mut d, &mut wb, "tn_props_fold_angle", "45");
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), folded(FRAC_PI_2 / 2.0)), "{} {}", volume(&wb), folded(FRAC_PI_2 / 2.0));
    // OK: a Fold in the browser, and the part lays flat as the plate it was.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let (id, name, kind) = wb.document().features().last().map(|f| (f.id, f.name.clone(), f.kind.clone())).unwrap();
    let FeatureKind::Fold(f) = kind else { panic!("{kind:?}") };
    assert_eq!(
        (name.as_str(), f.sketch, f.location, f.flip_side, f.flip_direction, f.radius),
        ("Fold1", sketch, FoldLocation::Start, true, true, None)
    );
    assert!((f.angle - FRAC_PI_2 / 2.0).abs() < 1e-12);
    let flat = wb.exec("sheet.flat", json!({})).unwrap();
    let (long, short) = (
        flat["length"].as_f64().unwrap().max(flat["width"].as_f64().unwrap()),
        flat["length"].as_f64().unwrap().min(flat["width"].as_f64().unwrap()),
    );
    assert!(near(long, 60.0) && near(short, 40.0) && near(flat["area"].as_f64().unwrap(), 2400.0), "{flat}");
    let row = browser_row(&d, "Fold1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    let Some(Panel::Fold(p)) = &wb.panel else { panic!("double-click edits the fold: {}", wb.status()) };
    assert_eq!((p.editing, p.degrees.round(), p.location, p.flip_side, p.flip_direction), (Some(id), 45.0, FoldLocation::Start, true, true));
}

#[test]
fn a_contour_flange_runs_along_an_edge_clicked_in_its_panel() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1000.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3 })).unwrap();
    // A plate, and on the XZ plane (square to its right edge, at the edge's front end) a
    // profile that starts at the top of that edge: 15 up, then 15 out.
    plate(&mut wb, 60.0, 40.0);
    let sk = wb.exec("sketch.create", json!({ "plane": "xz" })).unwrap()["feature"].as_u64().unwrap();
    frames(&mut d, &mut wb);
    let frame = wb.sketch_frame(tenon_model::FeatureId(sk as u32)).unwrap();
    let at = |x: f64, z: f64| frame.to_local(Vec3::new(x, 0.0, z));
    let (a, b, c) = (at(60.0, 2.0), at(60.0, 17.0), at(75.0, 17.0));
    let first = wb.exec("sketch.line", json!({ "sketch": sk, "x1": a.x, "y1": a.y, "x2": b.x, "y2": b.y })).unwrap();
    wb.exec("sketch.line", json!({ "sketch": sk, "start": first["end"], "x2": c.x, "y2": c.y })).unwrap();
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);

    // Contour Flange takes the profile; with no edge it would be pushed out of its sketch.
    wb.run_ui("sheet.contour_flange").unwrap();
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::ContourFlange(p)) if p.sketch.0 as u64 == sk && p.edges.is_empty()));
    let texts = d.texts();
    for shown in ["Contour Flange", "Profile", "Edges", "none: pushed out of its sketch", "Distance"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    // A click on the plate's upper right edge: the profile runs along it, the whole 40. The
    // plate is cut back by 3 for the bend at the edge; then a bend, 7 of wall, a bend and 10 of lip.
    let edge = on_screen(&wb, Vec3::new(60.0, 20.0, 2.0));
    d.click(&mut wb, edge);
    frames(&mut d, &mut wb);
    let Some(Panel::ContourFlange(p)) = &wb.panel else { panic!("the contour flange panel") };
    assert_eq!(p.edges.len(), 1, "{}", wb.status());
    let strip = |width: f64| (-3.0 * 2.0 + 2.0 * ring(FRAC_PI_2, 3.0) + 17.0 * 2.0) * width;
    assert!(near(volume(&wb), 4800.0 + strip(40.0)), "{} {}: {}", volume(&wb), 4800.0 + strip(40.0), wb.status());
    let texts = d.texts();
    for (shown, there) in [("1 selected", true), ("Width Extents", true), ("Distance", false), ("Direction", false)] {
        assert_eq!(texts.iter().any(|t| t == shown), there, "{shown}: {texts:?}");
    }
    // 20 wide in the middle of the edge, from the list.
    d.click(&mut wb, pressable(&d, "tn_contour_width"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_contour_width_centered"));
    frames(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_contour_w", "20");
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), 4800.0 + strip(20.0)), "{} {}", volume(&wb), 4800.0 + strip(20.0));
    // OK: the feature says its edge and its width, and edits again with them.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let (id, kind) = wb.document().features().last().map(|f| (f.id, f.kind.clone())).unwrap();
    let FeatureKind::ContourFlange(c) = kind else { panic!("{kind:?}") };
    assert!(
        c.edges.len() == 1 && c.distance == 0.0 && matches!(c.width, tenon_model::sheet::FlangeWidth::Centered { width } if width == 20.0),
        "{c:?}"
    );
    assert!(near(volume(&wb), 4800.0 + strip(20.0)));
    let row = browser_row(&d, "Contour Flange1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    let Some(Panel::ContourFlange(p)) = &wb.panel else { panic!("double-click edits it: {}", wb.status()) };
    assert_eq!((p.editing, p.edges.len(), p.width, p.width_value), (Some(id), 1, crate::sheet::WidthChoice::Centered, 20.0));
}

#[test]
fn a_cut_is_square_to_the_sheet_where_its_profile_is_seen_when_its_panel_is_told_to() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1000.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3 })).unwrap();
    // A plate, and over it a work plane through the X axis turned 30 degrees up from it, with
    // a circle of radius 4 that is seen on the top of the plate at (30, 20).
    plate(&mut wb, 60.0, 40.0);
    let tilt = 30f64.to_radians();
    let wp = wb.exec("work.plane", json!({ "by": "angle", "base": "xy", "axis": "x", "angle": tilt })).unwrap()["feature"].as_u64().unwrap();
    wb.create_sketch(json!({ "work_plane": wp })).unwrap();
    let Some(sketch) = wb.document().features().last().map(|f| f.id) else { panic!() };
    frames(&mut d, &mut wb);
    let at = wb.sketch_frame(sketch).unwrap().to_local(Vec3::new(30.0, 20.0, 2.0));
    wb.exec("sketch.circle", json!({ "sketch": sketch.0, "cx": at.x, "cy": at.y, "r": 4 })).unwrap();

    // Cut: straight through to begin with, a Thickness each way from the sketch, which is
    // nowhere near the plate from a plane that is 11 above it there.
    wb.run_ui("sheet.cut").unwrap();
    frames(&mut d, &mut wb);
    let Some(Panel::SheetCut(p)) = &wb.panel else { panic!("the Cut panel: {}", wb.status()) };
    assert_eq!((p.editing, p.across, p.normal, p.distance), (None, false, false, 2.0));
    let texts = d.texts();
    for shown in ["Cut", "Profiles", "1 selected", "Cut Across Bend", "Cut Normal", "Distance", "fx: Thickness"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    assert!(near(volume(&wb), 4800.0), "{}", volume(&wb));
    // Cut Normal ticked: the circle is cut where it is seen on the plate, an oval 4 by
    // 4 / cos 30, through the sheet square to it.
    d.click(&mut wb, pressable(&d, "tn_cut_normal"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::SheetCut(p)) if p.normal && !p.across));
    let oval = 16.0 * PI / tilt.cos();
    let about = |a: f64, b: f64| (a - b).abs() <= 1e-3 * b.abs();
    assert!(about(4800.0 - volume(&wb), oval * 2.0), "{} {}: {}", 4800.0 - volume(&wb), oval * 2.0, wb.status());
    // OK: a feature of its own, named Cut, its depth the Thickness by equation.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let (id, name, kind) = wb.document().features().last().map(|f| (f.id, f.name.clone(), f.kind.clone())).unwrap();
    let FeatureKind::CutNormal(c) = kind else { panic!("{kind:?}") };
    assert_eq!((name.as_str(), c.sketch, c.distance), ("Cut1", sketch, Some(2.0)));
    assert!(about(4800.0 - volume(&wb), oval * 2.0), "{}", volume(&wb));
    let flat = wb.exec("sheet.flat", json!({})).unwrap();
    assert!(flat["holes"] == 1 && flat["area"].as_f64().is_some_and(|a| about(2400.0 - a, oval)), "{flat}");

    // Edited from the browser it is still a cut normal (neither box is for changing here), and
    // a depth typed into it makes a pocket half a millimetre deep in the top of the plate.
    let row = browser_row(&d, "Cut1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    let Some(Panel::SheetCut(p)) = &wb.panel else { panic!("double-click edits the cut: {}", wb.status()) };
    assert_eq!((p.editing, p.across, p.normal, p.distance), (Some(id), false, true, 2.0));
    for boxed in ["tn_cut_normal", "tn_cut_across"] {
        d.click(&mut wb, pressable(&d, boxed));
        frames(&mut d, &mut wb);
        assert!(matches!(&wb.panel, Some(Panel::SheetCut(p)) if p.normal && !p.across), "{boxed}");
    }
    type_into(&mut d, &mut wb, "tn_props_cut_distance", "0.5");
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none() && about(4800.0 - volume(&wb), oval * 0.5), "{}: {}", 4800.0 - volume(&wb), wb.status());
    let Some(FeatureKind::CutNormal(c)) = wb.document().feature(id).map(|f| f.kind.clone()) else { panic!() };
    assert_eq!(c.distance, Some(0.5));

    // A cut across bends is square to the sheet as it is: with that ticked in a new Cut, the
    // Cut Normal box is not for changing, and what is made is the cut across bends.
    let top = wb.exec("model.face_ref", json!({ "at": [5, 5, 2] })).unwrap();
    wb.create_sketch(json!({ "face": top })).unwrap();
    let Some(second) = wb.document().features().last().map(|f| f.id) else { panic!() };
    frames(&mut d, &mut wb);
    let at = wb.sketch_frame(second).unwrap().to_local(Vec3::new(10.0, 10.0, 2.0));
    wb.exec("sketch.circle", json!({ "sketch": second.0, "cx": at.x, "cy": at.y, "r": 3 })).unwrap();
    wb.run_ui("sheet.cut").unwrap();
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_cut_across"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_cut_normal"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::SheetCut(p)) if p.across && !p.normal), "{}", wb.status());
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none() && matches!(wb.document().features().last().map(|f| &f.kind), Some(FeatureKind::CutAcrossBend(_))), "{}", wb.status());
}

#[test]
fn a_square_corner_is_opened_by_the_corner_rip_of_corner_seam() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1300.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3, "gap": 1 })).unwrap();
    // A box 60 by 40 by 20, shelled to the sheet's thickness with its top open: four walls on
    // a floor, with no seam where they meet.
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].as_u64().unwrap();
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 40 })).unwrap();
    let block = wb.exec("model.extrude", json!({ "sketch": sk, "distance": 20 })).unwrap()["feature"].as_u64().unwrap();
    let top = wb.exec("model.face_ref", json!({ "origin": { "type": "cap", "feature": block, "end": "end" } })).unwrap();
    wb.exec("model.shell", json!({ "remove": [top], "thickness": 2 })).unwrap();
    wb.look_from(Vec3::new(-1.0, -1.0, 1.0));
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    let whole = 60.0 * 40.0 * 20.0 - 56.0 * 36.0 * 18.0;
    assert!(near(volume(&wb), whole), "{}", volume(&wb));

    // Corner Seam opens as a seam; Corner Rip is chosen from its Shape list, and then the
    // panel asks for the corner's edge and has no relief to offer.
    wb.run_ui("sheet.corner_seam").unwrap();
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::CornerSeam(p)) if !p.rip && p.edges.is_empty()));
    let texts = d.texts();
    for shown in ["Shape", "Corner Seam", "click two edges", "Relief"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    d.click(&mut wb, pressable(&d, "tn_seam_shape"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_seam_shape_rip"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::CornerSeam(p)) if p.rip && p.edges.is_empty()));
    let texts = d.texts();
    for (shown, there) in
        [("Corner Rip", true), ("click a corner's edge", true), ("Seam", true), ("Symmetric Gap", true), ("1 mm", true), ("Relief", false)]
    {
        assert_eq!(texts.iter().any(|t| t == shown), there, "{shown}: {texts:?}");
    }
    // A click on the outside edge of the near corner: the corner opens in the preview, each
    // wall stopping 2 + 1 / sqrt 2 from it, and the floor loses its corner with them.
    let near_corner = on_screen(&wb, Vec3::new(0.0, 0.0, 10.0));
    d.click(&mut wb, near_corner);
    frames(&mut d, &mut wb);
    let Some(Panel::CornerSeam(p)) = &wb.panel else { panic!("the corner seam panel") };
    assert_eq!(p.edges.len(), 1, "{}", wb.status());
    let reach = 2.0 + 1.0 / 2f64.sqrt();
    let symmetric = (reach * reach - (reach - 2.0) * (reach - 2.0)) * 18.0 + reach * reach * 2.0;
    assert!(near(volume(&wb), whole - symmetric), "{} {}: {}", volume(&wb), whole - symmetric, wb.status());
    // Overlap from the list: one wall keeps the corner, the other stops the gap short of it.
    d.click(&mut wb, pressable(&d, "tn_seam_seam"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_seam_seam_overlap"));
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), whole - 2.0 * 1.0 * 20.0), "{} {}", volume(&wb), whole - 40.0);
    // OK: a feature of its own, a Corner Rip with the one edge and the rule's gap.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let (id, name, kind) = wb.document().features().last().map(|f| (f.id, f.name.clone(), f.kind.clone())).unwrap();
    let FeatureKind::CornerRip(r) = kind else { panic!("{kind:?}") };
    assert_eq!((name.as_str(), r.edges.len(), r.corner.seam, r.corner.gap, r.corner.relief), ("Corner Rip1", 1, Seam::Overlap, None, None));
    assert!(near(volume(&wb), whole - 40.0), "{}", volume(&wb));

    // Edited from the browser it is a rip still (its Shape is not for changing), and a click
    // on another corner's edge opens that one too.
    let row = browser_row(&d, "Corner Rip1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    let Some(Panel::CornerSeam(p)) = &wb.panel else { panic!("double-click edits the rip: {}", wb.status()) };
    assert_eq!((p.editing, p.rip, p.edges.len(), p.corner.seam), (Some(id), true, 1, Seam::Overlap));
    assert!(d.texts().iter().any(|t| t == "Corner Rip"), "{:?}", d.texts());
    let other_corner = on_screen(&wb, Vec3::new(60.0, 0.0, 10.0));
    d.click(&mut wb, other_corner);
    frames(&mut d, &mut wb);
    let Some(Panel::CornerSeam(p)) = &wb.panel else { panic!("the corner seam panel") };
    assert_eq!(p.edges.len(), 2, "{}", wb.status());
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none() && near(volume(&wb), whole - 80.0), "{}: {}", volume(&wb), wb.status());
    let Some(FeatureKind::CornerRip(r)) = wb.document().feature(id).map(|f| f.kind.clone()) else { panic!() };
    assert_eq!(r.edges.len(), 2);
}

#[test]
fn sharp_edges_are_made_bends_by_clicking_them_in_the_bend_panel() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1300.0));
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3, "gap": 1 })).unwrap();
    // A box 60 by 40 by 20, shelled to the sheet's thickness with its top open and its four
    // corners ripped by their outside edges: four walls standing on a floor, all edges sharp.
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].as_u64().unwrap();
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 40 })).unwrap();
    let block = wb.exec("model.extrude", json!({ "sketch": sk, "distance": 20 })).unwrap()["feature"].as_u64().unwrap();
    let top = wb.exec("model.face_ref", json!({ "origin": { "type": "cap", "feature": block, "end": "end" } })).unwrap();
    wb.exec("model.shell", json!({ "remove": [top], "thickness": 2 })).unwrap();
    let corners: Vec<serde_json::Value> =
        [[0, 0], [60, 0], [60, 40], [0, 40]].iter().map(|c| wb.exec("model.edge_ref", json!({ "at": [c[0], c[1], 10] })).unwrap()).collect();
    wb.exec("sheet.corner_rip", json!({ "edges": corners })).unwrap();
    wb.look_from(Vec3::new(-1.0, -1.0, 1.5));
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    let reach = 2.0 + 1.0 / 2f64.sqrt();
    let ripped = 60.0 * 40.0 * 20.0 - 56.0 * 36.0 * 18.0 - 4.0 * ((reach * reach - (reach - 2.0) * (reach - 2.0)) * 18.0 + reach * reach * 2.0);
    assert!(near(volume(&wb), ripped), "{} {ripped}", volume(&wb));
    assert!(wb.exec("sheet.flat", json!({})).is_err(), "a box with sharp edges is not laid flat");

    // Bend, from the Create panel: nothing picked yet, so it is neither kind of bend.
    wb.run_ui("sheet.bend").unwrap();
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Bend(p)) if p.edges.is_empty() && !p.sharp));
    let texts = d.texts();
    for shown in ["Bend", "Edges", "click a sharp edge, or two edges", "Double Bend"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    // A click on the edge where the front wall stands on the floor, seen from outside: it is
    // a sharp edge between two faces, so the bend is one on sharp edges, and the panel has no
    // double bend to offer. With the one edge the bend would end against the floor beside
    // it, and the part stays as it is, saying why.
    let front = on_screen(&wb, Vec3::new(30.0, 0.0, 0.0));
    d.click(&mut wb, front);
    frames(&mut d, &mut wb);
    let Some(Panel::Bend(p)) = &wb.panel else { panic!("the bend panel") };
    assert!(p.sharp && p.edges.len() == 1, "{}", wb.status());
    let texts = d.texts();
    for (shown, there) in [("1 selected", true), ("On sharp edges", true), ("Bend Radius", true), ("Double Bend", false), ("Flip Fixed Edge", false)]
    {
        assert_eq!(texts.iter().any(|t| t == shown), there, "{shown}: {texts:?}");
    }
    assert!(near(volume(&wb), ripped), "{}", volume(&wb));
    // An edge of the rim is not for this bend: the click is refused, and says so.
    let rim = on_screen(&wb, Vec3::new(30.0, 0.0, 20.0));
    d.click(&mut wb, rim);
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Bend(p)) if p.edges.len() == 1));
    assert!(wb.status().contains("on sharp edges"), "{}", wb.status());
    // The other three: under the left wall from outside, and under the back and right walls
    // from inside the box. With all four the box is a tray: the floor 50 by 30, a bend along
    // each side of it, and the walls from the top of the bends up.
    for at in [Vec3::new(0.0, 20.0, 0.0), Vec3::new(30.0, 38.0, 2.0), Vec3::new(58.0, 20.0, 2.0)] {
        let edge = on_screen(&wb, at);
        d.click(&mut wb, edge);
        frames(&mut d, &mut wb);
    }
    let Some(Panel::Bend(p)) = &wb.panel else { panic!("the bend panel") };
    assert_eq!(p.edges.len(), 4, "{}", wb.status());
    let tray = |r: f64| {
        let ring = FRAC_PI_2 / 2.0 * ((r + 2.0) * (r + 2.0) - r * r);
        let (long, wide, high) = (60.0 - 2.0 * (r + 2.0), 40.0 - 2.0 * (r + 2.0), 20.0 - (r + 2.0));
        long * wide * 2.0 + ring * 2.0 * (long + wide) + high * 2.0 * 2.0 * ((60.0 - 2.0 * reach) + (40.0 - 2.0 * reach))
    };
    assert!(near(volume(&wb), tray(3.0)), "{} {}: {}", volume(&wb), tray(3.0), wb.status());
    // OK: one Bend feature with the four edges and the rule's radius, and the part lays flat.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let (id, name, kind) = wb.document().features().last().map(|f| (f.id, f.name.clone(), f.kind.clone())).unwrap();
    let FeatureKind::SharpBend(b) = kind else { panic!("{kind:?}") };
    assert_eq!((name.as_str(), b.edges.len(), b.radius), ("Bend1", 4, None));
    assert!(near(volume(&wb), tray(3.0)), "{}", volume(&wb));
    let flat = wb.exec("sheet.flat", json!({})).unwrap();
    assert!(flat["bends"].as_array().is_some_and(|b| b.len() == 4), "{flat}");

    // Edited from the browser it is a bend on sharp edges still, with its four; a radius of
    // its own, typed in, makes the bends tighter.
    let row = browser_row(&d, "Bend1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    let Some(Panel::Bend(p)) = &wb.panel else { panic!("double-click edits the bend: {}", wb.status()) };
    assert_eq!((p.editing, p.sharp, p.edges.len(), p.own_radius), (Some(id), true, 4, false));
    d.click(&mut wb, pressable(&d, "tn_bend_by_rule"));
    frames(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_bend_r", "1");
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none() && near(volume(&wb), tray(1.0)), "{} {}: {}", volume(&wb), tray(1.0), wb.status());
    let Some(FeatureKind::SharpBend(b)) = wb.document().feature(id).map(|f| f.kind.clone()) else { panic!() };
    assert_eq!(b.radius, Some(1.0));
}
