//! Contour Roll, Lofted Flange and Punch Tool through the UI, by real pointer and key input.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};

use egui::vec2;
use serde_json::json;
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;
use tenon_model::FeatureKind;
use tenon_model::sheet::{Forming, PunchShape, Sides};

use crate::Workbench;
use crate::commands::{SHEET_TAB, find};
use crate::panels::Panel;
use crate::tests::{Driver, browser_row, on_screen, pressable, type_into};

fn frames(d: &mut Driver, wb: &mut Workbench) {
    for _ in 0..5 {
        d.frame(wb, vec![]);
    }
}

fn volume(wb: &Workbench) -> f64 {
    wb.scene().bodies.iter().map(|b| b.volume).sum()
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn double_click(d: &mut Driver, wb: &mut Workbench, at: egui::Pos2) {
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    for _ in 0..45 {
        d.frame(wb, vec![]);
    }
    d.click(wb, at);
    d.click(wb, at);
}

/// Presses a ribbon button of the Sheet Metal tab by its rectangle.
fn press(d: &mut Driver, wb: &mut Workbench, id: &str) {
    wb.chrome.tab = SHEET_TAB;
    frames(d, wb);
    d.click(wb, pressable(d, &format!("ribbon {id}")));
    frames(d, wb);
}

fn new_part(d: &mut Driver, wb: &mut Workbench) {
    wb.run_unprompted("file.new_sheet_metal").unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": 2, "bend_radius": 3 })).unwrap();
    frames(d, wb);
}

/// A sketch on the plane `p` with lines through `points`: its feature id.
fn lines(wb: &mut Workbench, p: serde_json::Value, points: &[(f64, f64)]) -> u64 {
    let sk = wb.exec("sketch.create", p).unwrap()["feature"].as_u64().unwrap();
    let mut last: Option<serde_json::Value> = None;
    for w in points.windows(2) {
        let mut q = json!({ "sketch": sk, "x1": w[0].0, "y1": w[0].1, "x2": w[1].0, "y2": w[1].1 });
        if let Some(end) = &last {
            q["start"] = end.clone();
        }
        last = Some(wb.exec("sketch.line", q).unwrap()["end"].clone());
    }
    sk
}

#[test]
fn the_three_are_on_the_ribbon_and_live() {
    for id in ["sheet.contour_roll", "sheet.lofted_flange", "sheet.punch"] {
        assert!(find(id).is_some(), "{id}");
    }
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1000.0));
    new_part(&mut d, &mut wb);
    // With nothing to work on, each says what it wants.
    assert!(wb.run_ui("sheet.contour_roll").unwrap_err().contains("draw an open profile"));
    assert!(wb.run_ui("sheet.lofted_flange").unwrap_err().contains("draw two open profiles"));
    assert!(wb.run_ui("sheet.punch").unwrap_err().contains("there is no sheet yet"));
}

#[test]
fn a_contour_roll_is_made_from_its_panel_by_clicks_and_edits_again() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1200.0));
    new_part(&mut d, &mut wb);
    // A line 40 long on the XZ plane, 50 from the Z axis; and a line of the sketch to roll about.
    let sk = lines(&mut wb, json!({ "plane": "xz" }), &[(50.0, 0.0), (50.0, 40.0)]);
    frames(&mut d, &mut wb);
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    press(&mut d, &mut wb, "sheet.contour_roll");
    let Some(Panel::ContourRoll(p)) = &wb.panel else { panic!("the panel: {}", wb.status()) };
    assert_eq!((p.sketch.0 as u64, p.degrees, p.axis), (sk, 180.0, crate::panels::AxisChoice::Origin(tenon_model::OriginAxis::Z)));
    let ring = |a: f64, b: f64, turn: f64| (b * b - a * a) / 2.0 * turn * 40.0;
    let half = volume(&wb);
    assert!(near(half, ring(50.0, 52.0, PI), 1e-3) || near(half, ring(48.0, 50.0, PI), 1e-3), "the preview: {half}: {}", wb.status());
    let texts = d.texts();
    for shown in ["Contour Roll", "Profile", "Axis", "Rolled Angle", "Direction", "Position", "Bend Radius", "By rule", "3 mm"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    // It says that one straight line unrolls exactly.
    assert!(texts.iter().any(|t| t.contains("which unrolls exactly")), "{texts:?}");
    // 270 degrees typed; the other side of the profile, chosen from the list.
    type_into(&mut d, &mut wb, "tn_props_roll_angle", "270");
    frames(&mut d, &mut wb);
    let three = volume(&wb);
    assert!(near(three, half * 1.5, 1e-3), "{three} {half}");
    d.click(&mut wb, pressable(&d, "tn_roll_position"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_roll_position_flipped"));
    frames(&mut d, &mut wb);
    let other = volume(&wb);
    assert!(!near(other, three, 1.0), "the sheet on the other side has another volume: {other} {three}");
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let made = wb.document().features().last().unwrap().clone();
    let FeatureKind::ContourRoll(c) = &made.kind else { panic!("{made:?}") };
    assert_eq!((made.name.as_str(), c.position, c.radius), ("Contour Roll1", Sides::Flipped, None));
    assert!(near(c.angle, 1.5 * PI, 1e-9));
    assert!(near(volume(&wb), other, 1e-6), "{}", wb.status());
    // A double-click on its row brings the panel back.
    let row = browser_row(&d, "Contour Roll1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    let Some(Panel::ContourRoll(p)) = &wb.panel else { panic!("double-click edits it: {}", wb.status()) };
    assert_eq!((p.editing, p.position), (Some(made.id), Sides::Flipped));
    assert!(near(p.degrees, 270.0, 1e-9));
    wb.panel = None;
    // Undone, it is gone; redone, it is back.
    frames(&mut d, &mut wb);
    wb.exec("edit.undo", json!({})).unwrap();
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), 0.0, 1e-9), "{}", volume(&wb));
    wb.exec("edit.redo", json!({})).unwrap();
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), other, 1e-6), "{}", volume(&wb));
}

#[test]
fn a_line_of_the_sketch_clicked_in_the_view_is_what_a_contour_roll_is_about() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1000.0));
    new_part(&mut d, &mut wb);
    let sk = lines(&mut wb, json!({ "plane": "xz" }), &[(60.0, 40.0), (30.0, 0.0)]);
    // A line beside the profile: at x = 0, not yet construction.
    let line = wb.exec("sketch.line", json!({ "sketch": sk, "x1": 0, "y1": -10, "x2": 0, "y2": 50 })).unwrap();
    let id = line["line"].as_u64().unwrap_or_else(|| panic!("{line}"));
    wb.exec("sketch.construction", json!({ "sketch": sk, "entity": id })).unwrap();
    frames(&mut d, &mut wb);
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    wb.run_ui("sheet.contour_roll").unwrap();
    frames(&mut d, &mut wb);
    let Some(Panel::ContourRoll(p)) = &wb.panel else { panic!() };
    assert_eq!(p.axis, crate::panels::AxisChoice::Line(tenon_sketch::EntityId(id as u32)), "a construction line is the axis it starts with");
    // Change it from the list, and back by a click on the line in the view.
    d.click(&mut wb, pressable(&d, "tn_roll_axis"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_roll_axis_Z"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::ContourRoll(p)) if p.axis == crate::panels::AxisChoice::Origin(tenon_model::OriginAxis::Z)));
    let frame = wb.sketch_frame(tenon_model::FeatureId(sk as u32)).unwrap();
    let on_line = on_screen(&wb, frame.plane_point(tenon_geom::Vec2::new(0.0, 45.0)));
    d.click(&mut wb, on_line);
    frames(&mut d, &mut wb);
    assert!(
        matches!(&wb.panel, Some(Panel::ContourRoll(p)) if p.axis == crate::panels::AxisChoice::Line(tenon_sketch::EntityId(id as u32))),
        "{}",
        wb.status()
    );
}

#[test]
fn a_lofted_flange_is_made_from_its_panel_by_clicks_and_says_how_close_it_is() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1200.0));
    new_part(&mut d, &mut wb);
    // Two lines one above the other, as the two ends of a plate; one drawn so far.
    let low = lines(&mut wb, json!({ "plane": "xy" }), &[(0.0, 0.0), (40.0, 0.0)]);
    frames(&mut d, &mut wb);
    press(&mut d, &mut wb, "sheet.lofted_flange");
    let Some(Panel::LoftedFlange(p)) = &wb.panel else { panic!("the panel: {}", wb.status()) };
    assert_eq!((p.sketches[0].map(|s| s.0 as u64), p.sketches[1]), (Some(low), None));
    assert!(wb.status().contains("click the second profile"), "{}", wb.status());
    wb.panel = None;
    // Enter with nothing to go between says so, and keeps the panel.
    let up = wb.exec("work.plane", json!({ "by": "offset", "base": "xy", "distance": 30 })).unwrap()["feature"].as_u64().unwrap();
    let high = lines(&mut wb, json!({ "work_plane": up }), &[(0.0, 0.0), (20.0, 0.0)]);
    frames(&mut d, &mut wb);
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    wb.run_ui("sheet.lofted_flange").unwrap();
    frames(&mut d, &mut wb);
    let Some(Panel::LoftedFlange(p)) = &wb.panel else { panic!() };
    assert_eq!(p.sketches.map(|s| s.map(|x| x.0 as u64)), [Some(low), Some(high)]);
    // A click on a profile takes it back, and the kept one stays; another click puts it back.
    let at = on_screen(&wb, Vec3::new(10.0, 0.0, 30.0));
    d.click(&mut wb, at);
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::LoftedFlange(p)) if p.sketches[1].is_none() && p.sketches[0].is_some()), "{}", wb.status());
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_some() && wb.status().contains("click the two profiles"), "{}", wb.status());
    d.click(&mut wb, at);
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::LoftedFlange(p)) if p.sketches[1].is_some()), "{}", wb.status());
    // Die formed by default: the preview, and how close it is, in words on the panel.
    let area = (40.0 + 20.0) / 2.0 * (30.0f64 * 30.0 + 10.0 * 10.0 * 0.0).sqrt();
    assert!(volume(&wb) > 0.0, "the preview: {}", wb.status());
    assert!(near(volume(&wb), area * 2.0, area * 2.0 * 0.05), "{} {}", volume(&wb), area * 2.0);
    frames(&mut d, &mut wb);
    let texts = d.texts();
    assert!(texts.iter().any(|t| t.contains("Die formed") && t.contains("how close")), "{texts:?}");
    // As press-brake facets, chosen from the list, a chord tolerance to type.
    d.click(&mut wb, pressable(&d, "tn_lofted_forming"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_lofted_forming_brake"));
    frames(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_lofted_chord", "0.25");
    frames(&mut d, &mut wb);
    let texts = d.texts();
    assert!(texts.iter().any(|t| t.contains("Press brake") && t.contains("which is exact")), "{texts:?}");
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let made = wb.document().features().last().unwrap().clone();
    let FeatureKind::LoftedFlange(l) = &made.kind else { panic!("{made:?}") };
    assert_eq!(l.forming, Forming::PressBrake { chord: 0.25 });
    let row = browser_row(&d, "Lofted Flange1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::LoftedFlange(p)) if p.editing == Some(made.id) && p.press_brake && p.chord == 0.25), "{}", wb.status());
}

/// A plate 100 x 60 with its top in view.
fn plate_in_view(d: &mut Driver, wb: &mut Workbench) {
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].as_u64().unwrap();
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 100, "y2": 60 })).unwrap();
    wb.exec("sheet.face", json!({ "sketch": sk })).unwrap();
    frames(d, wb);
    wb.look_from(Vec3::new(0.2, -0.2, 1.0));
    d.settle(wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(wb);
}

#[test]
fn a_punch_is_stamped_by_clicks_on_a_face_with_a_shape_from_its_library_window() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1200.0));
    new_part(&mut d, &mut wb);
    plate_in_view(&mut d, &mut wb);
    let plate = volume(&wb);
    assert!(near(plate, 12000.0, 1e-6));
    press(&mut d, &mut wb, "sheet.punch");
    let Some(Panel::Punch(p)) = &wb.panel else { panic!("the panel: {}", wb.status()) };
    assert!(p.on_face() && matches!(p.shape, PunchShape::Round { diameter } if diameter == 10.0), "{p:?}");
    // The library comes up with it: what there is, by kind.
    let texts = d.texts();
    for shown in ["Punch Library", "Cuts", "Formed", "Round", "Square", "Rectangle", "Obround Slot", "D-Shape", "Dimple", "Louver"] {
        assert!(texts.iter().any(|t| t == shown), "{shown}: {texts:?}");
    }
    // Enter with nowhere to stamp says so.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_some() && wb.status().contains("click a flat face"), "{}", wb.status());
    // An obround slot from the window.
    d.click(&mut wb, pressable(&d, "tn_punch_library_obround"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Punch(p)) if matches!(p.shape, PunchShape::Obround { .. })));
    d.click(&mut wb, pressable(&d, "tn_punch_library_close"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Punch(p)) if !p.library));
    // The panel brings it back.
    d.click(&mut wb, pressable(&d, "tn_punch_library_open"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Punch(p)) if p.library));
    d.click(&mut wb, pressable(&d, "tn_punch_library_close"));
    frames(&mut d, &mut wb);
    // Two clicks on the top; each a stamp, with a preview. One clicked again goes away.
    let (a, b) = (on_screen(&wb, Vec3::new(30.0, 30.0, 2.0)), on_screen(&wb, Vec3::new(70.0, 30.0, 2.0)));
    d.click(&mut wb, a);
    frames(&mut d, &mut wb);
    let slot = 16.0 * 8.0 + PI * 16.0;
    assert!(near(volume(&wb), plate - slot * 2.0, 1e-3), "{}: {}", volume(&wb), wb.status());
    d.click(&mut wb, b);
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), plate - 2.0 * slot * 2.0, 1e-3), "{}: {}", volume(&wb), wb.status());
    d.click(&mut wb, b);
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), plate - slot * 2.0, 1e-3), "{}: {}", volume(&wb), wb.status());
    d.click(&mut wb, b);
    frames(&mut d, &mut wb);
    // Its sizes typed; turned by a quarter.
    type_into(&mut d, &mut wb, "tn_props_punch_length", "30");
    frames(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_punch_rotation", "90");
    frames(&mut d, &mut wb);
    let long = 22.0 * 8.0 + PI * 16.0;
    assert!(near(volume(&wb), plate - 2.0 * long * 2.0, 1e-3), "{} {}", volume(&wb), plate - 2.0 * long * 2.0);
    // OK: a sketch on the face, and the punch on its points, as one undo step.
    let before = wb.document().features().len();
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(wb.document().features().len(), before + 2);
    let made = wb.document().features().last().unwrap().clone();
    let FeatureKind::Punch(p) = &made.kind else { panic!("{made:?}") };
    assert!(matches!(p.shape, PunchShape::Obround { length, .. } if length == 30.0) && near(p.rotation, FRAC_PI_2, 1e-9) && p.points.len() == 2);
    assert!(near(volume(&wb), plate - 2.0 * long * 2.0, 1e-3));
    // It edits again from its row.
    let row = browser_row(&d, "Punch1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    assert!(
        matches!(&wb.panel, Some(Panel::Punch(p)) if p.editing == Some(made.id) && matches!(p.shape, PunchShape::Obround { .. })),
        "{}",
        wb.status()
    );
    wb.panel = None;
    frames(&mut d, &mut wb);
    wb.exec("edit.undo", json!({})).unwrap();
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), plate, 1e-6), "taken back together: {}", volume(&wb));
    assert_eq!(wb.document().features().len(), before);
}

#[test]
fn a_formed_punch_on_a_sketchs_points_stands_up_and_is_named_in_the_flat_pattern_on_screen() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1000.0));
    new_part(&mut d, &mut wb);
    plate_in_view(&mut d, &mut wb);
    let up = wb.exec("work.plane", json!({ "by": "offset", "base": "xy", "distance": 2 })).unwrap()["feature"].as_u64().unwrap();
    let sk = wb.exec("sketch.create", json!({ "work_plane": up, "project_origin": false })).unwrap()["feature"].as_u64().unwrap();
    wb.exec("sketch.point", json!({ "sketch": sk, "x": 50, "y": 30 })).unwrap();
    frames(&mut d, &mut wb);
    wb.run_ui("sheet.punch").unwrap();
    frames(&mut d, &mut wb);
    let Some(Panel::Punch(p)) = &wb.panel else { panic!() };
    assert_eq!(p.sketch.map(|s| s.0 as u64), Some(sk), "the sketch of centre points");
    // A dimple from the panel's list.
    d.click(&mut wb, pressable(&d, "tn_punch_family"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_punch_family_dimple"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Punch(p)) if matches!(p.shape, PunchShape::Dimple { .. })));
    // 20 across and 3 deep, as it starts: pi 9 x 2 more than the plate.
    assert!(near(volume(&wb), 12000.0 + PI * 9.0 * 2.0, 1e-3), "{}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    // In the flat pattern: its name is shown.
    wb.run_ui("sheet.flat_view").unwrap();
    frames(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    let texts = d.texts();
    assert!(texts.iter().any(|t| t == "Dimple 20 x 3"), "{texts:?}");
}

#[test]
fn a_shape_that_does_not_unroll_exactly_says_so_on_its_panel_its_row_and_its_flat_pattern() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1000.0));
    new_part(&mut d, &mut wb);
    // Down a cylinder and out along a flat ring: two lines, so a warning.
    lines(&mut wb, json!({ "plane": "xz" }), &[(50.0, 30.0), (50.0, 0.0), (80.0, 0.0)]);
    frames(&mut d, &mut wb);
    wb.run_ui("sheet.contour_roll").unwrap();
    frames(&mut d, &mut wb);
    let warned = |d: &Driver| d.texts().iter().any(|t| t.contains("does not unroll exactly"));
    assert!(warned(&d), "on the panel: {:?}", d.texts());
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!(!warned(&d), "no panel, no words");
    // Pointed at, its row says so.
    let row = browser_row(&d, "Contour Roll1").center();
    d.frame(&mut wb, vec![egui::Event::PointerMoved(row)]);
    for _ in 0..60 {
        d.frame(&mut wb, vec![]);
    }
    assert!(warned(&d), "on its row: {:?}", d.texts());
    d.frame(&mut wb, vec![egui::Event::PointerGone]);
    // And the flat pattern says so, with how far out it is.
    wb.run_ui("sheet.flat_view").unwrap();
    frames(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    assert!(
        d.texts().iter().any(|t| t.contains("does not unroll exactly") && t.contains("row by row") && t.contains(" mm from where")),
        "{:?}",
        d.texts()
    );
}

/// A flat ring is a flat side like any other: a click on it stamps it, from either face.
#[test]
fn a_flat_ring_is_stamped_by_a_click_on_it() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1200.0));
    new_part(&mut d, &mut wb);
    lines(&mut wb, json!({ "plane": "xz" }), &[(30.0, 10.0), (60.0, 10.0)]);
    frames(&mut d, &mut wb);
    wb.exec("sheet.contour_roll", json!({ "sketch": 1, "axis": "z", "angle": 2.0, "direction": "symmetric" })).unwrap();
    frames(&mut d, &mut wb);
    // By hand: (60^2 - 30^2) / 2 x 2 x 2.
    let whole = volume(&wb);
    assert!(near(whole, 5400.0, 1e-6), "{whole}");
    wb.look_from(Vec3::new(0.3, -0.3, 1.0));
    d.settle(&mut wb);
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    press(&mut d, &mut wb, "sheet.punch");
    d.click(&mut wb, pressable(&d, "tn_punch_library_close"));
    frames(&mut d, &mut wb);
    // The top of the ring, in the middle of it: the panel takes the click, and says nothing against it.
    let at = on_screen(&wb, Vec3::new(45.0, 0.0, 12.0));
    d.click(&mut wb, at);
    frames(&mut d, &mut wb);
    let Some(Panel::Punch(p)) = &wb.panel else { panic!("the panel: {}", wb.status()) };
    assert_eq!(p.face_points.len(), 1, "{}", wb.status());
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.exec("model.regenerate", json!({})).unwrap()["error"].is_null(), "{}", wb.status());
    // Round 10: its area x 2.
    assert!(near(volume(&wb), whole - 25.0 * PI * 2.0, 1e-5), "{}: {}", volume(&wb), wb.status());
}

/// Editing a punch: another sketch means that sketch's centres, not the point ids of the old one;
/// and only a sketch before the punch is on the list, since it cannot go by one after it.
#[test]
fn a_punch_edited_to_another_sketch_goes_by_that_sketch_and_only_earlier_ones_are_offered() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1200.0));
    new_part(&mut d, &mut wb);
    plate_in_view(&mut d, &mut wb);
    let whole = volume(&wb);
    let up = wb.exec("work.plane", json!({ "by": "offset", "base": "xy", "distance": 2 })).unwrap()["feature"].as_u64().unwrap();
    let sketch =
        |wb: &mut Workbench| wb.exec("sketch.create", json!({ "work_plane": up, "project_origin": false })).unwrap()["feature"].as_u64().unwrap();
    let one = sketch(&mut wb);
    let p1 = wb.exec("sketch.point", json!({ "sketch": one, "x": 20, "y": 20 })).unwrap()["point"].as_u64().unwrap();
    let two = sketch(&mut wb);
    for (x, y) in [(50, 20), (50, 40), (70, 40)] {
        wb.exec("sketch.point", json!({ "sketch": two, "x": x, "y": y })).unwrap();
    }
    wb.exec("sheet.punch", json!({ "sketch": one, "points": [p1], "shape": "round", "diameter": 6 })).unwrap();
    let late = sketch(&mut wb);
    wb.exec("sketch.point", json!({ "sketch": late, "x": 30, "y": 40 })).unwrap();
    frames(&mut d, &mut wb);
    let stamp = 9.0 * PI * 2.0;
    assert!(near(volume(&wb), whole - stamp, 1e-5));

    let row = browser_row(&d, "Punch1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Punch(p)) if p.points.len() == 1), "{}", wb.status());
    // The list, opened by the pointer: the two before it, and not the one after.
    d.click(&mut wb, pressable(&d, "tn_punch_sketch"));
    frames(&mut d, &mut wb);
    let offered = |d: &Driver, id: u64| {
        d.ctx
            .data(|x| x.get_temp::<std::collections::BTreeMap<String, egui::Rect>>(egui::Id::new("tn_buttons")))
            .unwrap_or_default()
            .contains_key(&format!("tn_punch_sketch_{id}"))
    };
    assert!(offered(&d, one) && offered(&d, two) && !offered(&d, late));
    d.click(&mut wb, pressable(&d, &format!("tn_punch_sketch_{two}")));
    frames(&mut d, &mut wb);
    let Some(Panel::Punch(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert!(p.points.is_empty() && p.sketch == Some(tenon_model::FeatureId(two as u32)), "{p:?}");
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    // Three stamps now.
    assert!(wb.exec("model.regenerate", json!({})).unwrap()["error"].is_null(), "{}", wb.status());
    assert!(near(volume(&wb), whole - 3.0 * stamp, 1e-5), "{}: {}", volume(&wb), wb.status());
    let punch = wb.document().features().iter().find(|f| f.name == "Punch1").unwrap();
    let FeatureKind::Punch(p) = &punch.kind else { panic!("{punch:?}") };
    assert!(p.points.is_empty() && p.sketch.0 as u64 == two, "{p:?}");

    // Back to the first, with the first's own point, chosen again.
    let row = browser_row(&d, "Punch1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_punch_sketch"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, &format!("tn_punch_sketch_{one}")));
    frames(&mut d, &mut wb);
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), whole - stamp, 1e-5), "{}: {}", volume(&wb), wb.status());
}

/// What a twist comes to is said where a person looks: on the panel, on the browser row and with
/// the flat pattern; and past what a sheet can be it is said on the panel, and nothing is made.
#[test]
fn a_twisted_sheet_says_how_thick_it_is_on_its_panel_its_row_and_its_flat_pattern_and_past_that_is_refused() {
    let twisted = |d: &mut Driver, wb: &mut Workbench, degrees: f64| {
        new_part(d, wb);
        let (sin, cos) = degrees.to_radians().sin_cos();
        lines(wb, json!({ "plane": "xy" }), &[(0.0, 0.0), (100.0, 0.0)]);
        let up = wb.exec("work.plane", json!({ "by": "offset", "base": "xy", "distance": 80 })).unwrap()["feature"].as_u64().unwrap();
        lines(wb, json!({ "work_plane": up }), &[(50.0 - 50.0 * cos, -50.0 * sin), (50.0 + 50.0 * cos, 50.0 * sin)]);
        frames(d, wb);
        wb.run_ui("sheet.lofted_flange").unwrap();
        frames(d, wb);
    };
    let says =
        |d: &Driver| d.texts().iter().any(|t| t.contains("twists by up to") && t.contains("thick on the whole") && t.contains("the rule says 2 mm"));
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1000.0));
    twisted(&mut d, &mut wb, 15.0);
    assert!(says(&d), "on the panel: {:?}", d.texts());
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none() && !says(&d), "{}", wb.status());
    let row = browser_row(&d, "Lofted Flange1").center();
    d.frame(&mut wb, vec![egui::Event::PointerMoved(row)]);
    for _ in 0..60 {
        d.frame(&mut wb, vec![]);
    }
    assert!(says(&d), "on its row: {:?}", d.texts());
    d.frame(&mut wb, vec![egui::Event::PointerGone]);
    wb.run_ui("sheet.flat_view").unwrap();
    frames(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    assert!(says(&d), "with the flat pattern: {:?}", d.texts());

    // Past it: the panel says so before anything is made, and Enter makes nothing.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 1000.0));
    twisted(&mut d, &mut wb, 45.0);
    let refused = |d: &Driver| d.texts().iter().any(|t| t.contains("These profiles twist") && t.contains("Press Brake"));
    assert!(refused(&d), "on the panel: {:?}", d.texts());
    assert_eq!(volume(&wb), 0.0, "no preview of what cannot be");
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    assert_eq!(volume(&wb), 0.0, "{}", wb.status());
    // Like any feature that fails: it says why in the view.
    assert!(d.texts().iter().any(|t| t.contains("These profiles twist")), "{:?}", d.texts());
}
