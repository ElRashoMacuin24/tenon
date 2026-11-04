//! A 3D sketch as a user has it (DEC-136): made and changed through the window by pointer and keyboard.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};

use egui::{Pos2, vec2};
use serde_json::json;
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;
use tenon_model::FeatureId;
use tenon_sketch::{Constraint3, EntityId, Geometry3, Sketch3d};

use crate::Workbench;
use crate::panels::Panel;
use crate::tests::{Driver, ctrl, on_screen, pressable, type_into};
use crate::workbench::Mode;

fn app() -> (Workbench, Driver) {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    (wb, d)
}

fn sketch_open(wb: &Workbench) -> FeatureId {
    match &wb.mode {
        Mode::Sketch3d(s) => s.feature,
        _ => panic!("no 3D sketch is open: {}", wb.status()),
    }
}

fn sk(wb: &Workbench) -> Sketch3d {
    wb.document().sketch3d(sketch_open(wb)).unwrap().clone()
}

fn settle(d: &mut Driver, wb: &mut Workbench) {
    for _ in 0..4 {
        d.frame(wb, vec![]);
    }
    d.settle(wb);
}

/// A double click. (After a pause: clicks less than 0.6 s after the earlier ones would count as a
/// triple click.)
fn double_click(d: &mut Driver, wb: &mut Workbench, at: Pos2) {
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    for _ in 0..45 {
        d.frame(wb, vec![]);
    }
    for pressed in [true, false, true, false] {
        d.frame(wb, vec![Driver::button(at, egui::PointerButton::Primary, pressed)]);
    }
}

/// Types X, Y and Z into the box and presses its Add.
fn add_xyz(d: &mut Driver, wb: &mut Workbench, x: f64, y: f64, z: f64) {
    for (field, v) in [("tn_s3_x", x), ("tn_s3_y", y), ("tn_s3_z", z)] {
        type_into(d, wb, field, &v.to_string());
        d.tap(wb, egui::Key::Tab);
    }
    d.frame(wb, vec![]);
    let at = pressable(d, "tn_s3_add");
    d.click(wb, at);
    d.frame(wb, vec![]);
}

/// Where a point of the part is on the screen.
fn at(wb: &Workbench, x: f64, y: f64, z: f64) -> Pos2 {
    on_screen(wb, Vec3::new(x, y, z))
}

/// The value box of a bend or a dimension, typed in, OK by Enter.
fn value_box(d: &mut Driver, wb: &mut Workbench, v: f64) {
    assert!(matches!(wb.panel, Some(Panel::Value(_))), "no value box: {}", wb.status());
    type_into(d, wb, "tn_value_box", &v.to_string());
    d.tap(wb, egui::Key::Enter);
    d.frame(wb, vec![]);
}

/// A block 40 by 20 by 10 at the origin, seen from the home view.
fn block(wb: &mut Workbench, d: &mut Driver) -> FeatureId {
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let Mode::Sketch(s) = &wb.mode else { panic!() };
    let f = s.feature;
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.finish_sketch();
    let e = wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    d.frame(wb, vec![]);
    settle(d, wb);
    wb.run_ui("view.home").unwrap();
    settle(d, wb);
    FeatureId(e["feature"].as_u64().unwrap() as u32)
}

#[test]
fn start_3d_sketch_is_a_button_that_works_and_says_what_it_does() {
    let c = crate::commands::find("sketch.new_3d").unwrap();
    assert!(c.tip.contains("no plane"), "{}", c.tip);
    let (mut wb, mut d) = app();
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    assert_eq!(sketch_open(&wb), wb.document().features()[0].id);
    assert_eq!(wb.document().features()[0].name, "3D Sketch1");
    assert_eq!(wb.chrome.tab, crate::commands::SKETCH3D_TAB, "its own tab");
    assert!(d.text_pos("Finish\n3D Sketch").is_some() || d.texts().iter().any(|t| t.contains("3D Sketch")), "{:?}", d.texts());
    assert!(wb.part_tabs().contains(&crate::commands::SKETCH3D_TAB));
    assert!(!wb.part_tabs().contains(&crate::commands::SKETCH_TAB));
    wb.run_ui("sketch3d.finish").unwrap();
    assert!(!wb.is_sketching3d());
    assert!(!wb.part_tabs().contains(&crate::commands::SKETCH3D_TAB), "only in a 3D sketch");
    assert_eq!(wb.chrome.tab, crate::commands::MODEL_TAB);
}

#[test]
fn a_path_typed_and_bent_makes_a_tube_of_the_volume_of_its_length() {
    let (mut wb, mut d) = app();
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    // The Line tool by its key, its points typed.
    d.tap(&mut wb, egui::Key::L);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.active_tool_id(), Some("sketch3d.line"), "{}", wb.status());
    for p in [[0.0, 0.0, 0.0], [0.0, 0.0, 50.0], [50.0, 0.0, 50.0], [50.0, 50.0, 50.0]] {
        add_xyz(&mut d, &mut wb, p[0], p[1], p[2]);
    }
    let s = sk(&wb);
    let lines: Vec<EntityId> = s.entities().filter(|(id, _)| s.is_line(*id)).map(|(id, _)| id).collect();
    assert_eq!(lines.len(), 3, "typed: {} ; {:?}", wb.status(), s);
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    // See it all.
    wb.run_ui("view.home").unwrap();
    wb.view.camera.fit(&tenon_geom::Aabb3::from_points(s.extent()).unwrap());
    settle(&mut d, &mut wb);
    assert!(d.texts().iter().any(|t| t.contains("12 degrees of freedom")), "the status bar says: {:?}", d.texts());
    // Bend both corners: click each, say the radius.
    wb.run_ui("sketch3d.bend").unwrap();
    d.frame(&mut wb, vec![]);
    for corner in [(0.0, 0.0, 50.0), (50.0, 0.0, 50.0)] {
        let q = at(&wb, corner.0, corner.1, corner.2);
        d.click(&mut wb, q);
        d.frame(&mut wb, vec![]);
        value_box(&mut d, &mut wb, 10.0);
        assert!(!wb.status_error, "{}", wb.status());
    }
    let s = sk(&wb);
    let arcs: Vec<EntityId> = s.entities().filter(|(id, _)| s.is_arc(*id)).map(|(id, _)| id).collect();
    assert_eq!(arcs.len(), 2, "{}", wb.status());
    assert!(d.texts().iter().any(|t| t == "R10"), "the radius shows: {:?}", d.texts());
    // Select the first bend: the status bar says how long it is, a quarter of a circle of 10.
    wb.run_ui("sketch3d.bend").unwrap();
    d.frame(&mut wb, vec![]);
    let mid = s.polyline(arcs[0])[s.polyline(arcs[0]).len() / 3];
    let q = on_screen(&wb, mid);
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    assert!(wb.status().contains("Length 15.708 mm"), "{}", wb.status());
    d.tap(&mut wb, egui::Key::Escape);
    // Finish, and sweep a circle along it from the ribbon.
    wb.run_ui("sketch3d.finish").unwrap();
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let Mode::Sketch(c) = &wb.mode else { panic!() };
    let circle = c.feature;
    wb.exec("sketch.circle", json!({ "sketch": circle.0, "cx": 0, "cy": 0, "r": 5 })).unwrap();
    wb.finish_sketch();
    wb.run_ui("model.sweep").unwrap();
    let Some(Panel::Sweep(p)) = wb.panel.clone() else { panic!("no sweep panel: {}", wb.status()) };
    assert_eq!(p.sketch, circle);
    assert_eq!(p.path.as_ref().unwrap().curves.len(), 5, "the 3D sketch is the path: {:?}", p.path);
    wb.panel_request = Some(crate::panels::PanelRequest::Ok);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.document().features().last().unwrap().name, "Sweep1");
    let length = 40.0 + 30.0 + 40.0 + 2.0 * (FRAC_PI_2 * 10.0);
    let v: f64 = wb.scene().bodies.iter().map(|b| b.volume).sum();
    assert!((v - PI * 25.0 * length).abs() < 1e-3 * PI * 25.0 * length, "{v}: {}", wb.status());
}

#[test]
fn a_line_by_clicks_on_vertices_follows_the_part_and_stays_along_an_axis() {
    let (mut wb, mut d) = app();
    let extrude = block(&mut wb, &mut d);
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    wb.run_ui("sketch3d.line").unwrap();
    d.frame(&mut wb, vec![]);
    // A click on the top corner of the block, then a little above it on the screen, along Z.
    let corner = Vec3::new(40.0, 20.0, 10.0);
    let q = on_screen(&wb, corner);
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let Mode::Sketch3d(sm) = &wb.mode else { panic!() };
    assert_eq!(sm.clicks.len(), 1, "{}", wb.status());
    assert!(matches!(sm.clicks[0].target, crate::sketch3d::Target::Vertex { .. }), "a vertex: {:?}", sm.clicks[0]);
    let up = on_screen(&wb, corner + Vec3::new(0.0, 0.0, 15.0));
    d.click(&mut wb, up);
    d.frame(&mut wb, vec![]);
    let s = sk(&wb);
    let line = s.entities().find(|(id, _)| s.is_line(*id)).map(|(id, _)| id).unwrap_or_else(|| panic!("no line: {}", wb.status()));
    let (a, b) = s.line(line).unwrap();
    assert!(a.dist(corner) < 1e-9, "{a:?}");
    assert!((b.x - a.x).abs() < 1e-9 && (b.y - a.y).abs() < 1e-9, "it goes straight up: {a:?} {b:?}");
    assert!((b.z - 25.0).abs() < 1.0, "near enough where it was clicked: {b:?}");
    assert!(s.constraints().any(|(_, c)| matches!(c, Constraint3::Along { axis: tenon_sketch::Axis3::Z, .. })), "held so: {:?}", s);
    assert_eq!(wb.document().links3(sketch_open(&wb)).len(), 1, "the vertex is a link");
    wb.run_ui("sketch3d.finish").unwrap();
    d.frame(&mut wb, vec![]);
    // The block made taller: the line's start goes with its corner, and the whole with it.
    let mut kind = serde_json::to_value(&wb.document().feature(extrude).unwrap().kind).unwrap();
    kind["extent"]["distance"] = json!(30.0);
    wb.exec("feature.update", json!({ "feature": extrude.0, "kind": kind })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let f = wb.document().features().iter().find(|f| f.name == "3D Sketch1").unwrap().id;
    let now = wb.document().sketch3d(f).unwrap().line(line).unwrap();
    assert!(now.0.dist(Vec3::new(40.0, 20.0, 30.0)) < 1e-7, "{:?}", now.0);
    // Undone, back with the block.
    ctrl(&mut d, &mut wb, egui::Key::Z);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let now = wb.document().sketch3d(f).unwrap().line(line).unwrap();
    assert!(now.0.dist(corner) < 1e-7, "{:?}", now.0);
}

#[test]
fn include_geometry_by_a_click_and_a_line_constrained_to_it_by_clicks() {
    let (mut wb, mut d) = app();
    block(&mut wb, &mut d);
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    wb.run_ui("sketch3d.include").unwrap();
    d.frame(&mut wb, vec![]);
    // The edge that stands at the near corner: along Z.
    let q = on_screen(&wb, Vec3::new(40.0, 0.0, 5.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let s = sk(&wb);
    let edge = s.entities().find(|(id, _)| s.is_line(*id)).map(|(id, _)| id).unwrap_or_else(|| panic!("nothing came in: {}", wb.status()));
    assert!(s.is_reference(edge), "reference geometry");
    let (a, b) = s.line(edge).unwrap();
    assert!((a.dist(b) - 10.0).abs() < 1e-9, "{a:?} {b:?}");
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    // A line of its own, by typed points, made parallel by clicks.
    wb.run_ui("sketch3d.line").unwrap();
    d.frame(&mut wb, vec![]);
    add_xyz(&mut d, &mut wb, 60.0, 0.0, 0.0);
    add_xyz(&mut d, &mut wb, 70.0, 5.0, 20.0);
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    let s = sk(&wb);
    let own = s.entities().find(|(id, e)| s.is_line(*id) && !e.reference).map(|(id, _)| id).unwrap();
    wb.run_ui("sketch3d.parallel").unwrap();
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, Vec3::new(40.0, 0.0, 5.0));
    d.click(&mut wb, q);
    let mid = s.line(own).map(|(a, b)| a.lerp(b, 0.5)).unwrap();
    let q = on_screen(&wb, mid);
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let (a, b) = sk(&wb).line(own).unwrap();
    assert!((b - a).normalized().cross(Vec3::Z).len() < 1e-8, "{a:?} {b:?}: {}", wb.status());
}

#[test]
fn over_constraining_by_clicks_is_refused_in_words() {
    let (mut wb, mut d) = app();
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    let f = sketch_open(&wb);
    let l = wb.exec("sketch3d.line", json!({ "sketch": f.0, "start": [0, 0, 0], "end": [30, 0, 0] })).unwrap();
    let line = EntityId(l["line"].as_u64().unwrap() as u32);
    wb.exec("sketch3d.constrain", json!({ "sketch": f.0, "constraint": { "type": "along", "line": line.0, "axis": "x" } })).unwrap();
    wb.view.camera.fit(&tenon_geom::Aabb3::from_points(sk(&wb).extent()).unwrap());
    settle(&mut d, &mut wb);
    // Along X already: along Y as well would leave nothing of it.
    wb.run_ui("sketch3d.along_y").unwrap();
    d.frame(&mut wb, vec![]);
    let before = sk(&wb);
    let q = on_screen(&wb, Vec3::new(15.0, 0.0, 0.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error && wb.status().contains("would over-constrain the sketch"), "{}", wb.status());
    assert_eq!(sk(&wb), before);
    // Dimensioned by the tool: its length, through the value box; then double-clicked to change.
    wb.run_ui("sketch3d.dimension").unwrap();
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    value_box(&mut d, &mut wb, 40.0);
    assert!(!wb.status_error, "{}", wb.status());
    let (a, b) = sk(&wb).line(line).unwrap();
    assert!((a.dist(b) - 40.0).abs() < 1e-9);
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    let label = d.text_pos("40").unwrap_or_else(|| panic!("no value: {:?}", d.texts()));
    double_click(&mut d, &mut wb, label);
    d.frame(&mut wb, vec![]);
    assert!(matches!(&wb.panel, Some(Panel::Value(v)) if v.title == "Edit Dimension"), "{}", wb.status());
    value_box(&mut d, &mut wb, 25.0);
    let (a, b) = sk(&wb).line(line).unwrap();
    assert!((a.dist(b) - 25.0).abs() < 1e-9, "{}", wb.status());
}

#[test]
fn a_spline_by_clicks_and_keys_and_undo_as_a_sketch_has_it() {
    let (mut wb, mut d) = app();
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    wb.run_ui("sketch3d.spline").unwrap();
    d.frame(&mut wb, vec![]);
    for p in [[0.0, 0.0, 0.0], [10.0, 5.0, 0.0], [20.0, 0.0, 5.0], [30.0, 5.0, 5.0]] {
        add_xyz(&mut d, &mut wb, p[0], p[1], p[2]);
    }
    let Mode::Sketch3d(sm) = &wb.mode else { panic!() };
    assert_eq!(sm.clicks.len(), 4, "{}", wb.status());
    assert!(sk(&wb).is_empty(), "not yet");
    let make = pressable(&d, "tn_s3_make");
    d.click(&mut wb, make);
    d.frame(&mut wb, vec![]);
    let s = sk(&wb);
    let spline =
        s.entities().find(|(_, e)| matches!(e.geometry, Geometry3::Spline { .. })).map(|(id, _)| id).unwrap_or_else(|| panic!("{}", wb.status()));
    assert!(s.length(spline).unwrap() > 30.0);
    // Ctrl+Z takes it back in the sketch, which stays open; Ctrl+Y puts it back.
    ctrl(&mut d, &mut wb, egui::Key::Z);
    d.frame(&mut wb, vec![]);
    assert!(sk(&wb).is_empty(), "{:?}", sk(&wb));
    assert!(wb.is_sketching3d());
    ctrl(&mut d, &mut wb, egui::Key::Y);
    d.frame(&mut wb, vec![]);
    assert!(sk(&wb).entity(spline).is_some());
    // Undo that takes the sketch away leaves it.
    let f = sketch_open(&wb);
    for _ in 0..3 {
        ctrl(&mut d, &mut wb, egui::Key::Z);
        d.frame(&mut wb, vec![]);
    }
    assert!(wb.document().feature(f).is_none());
    assert!(!wb.is_sketching3d(), "{}", wb.status());
}

#[test]
fn the_browser_opens_it_again_by_a_double_click_and_it_shows_in_its_colour_until_used() {
    let (mut wb, mut d) = app();
    wb.run_ui("sketch.new_3d").unwrap();
    let f = sketch_open(&wb);
    wb.exec("sketch3d.line", json!({ "sketch": f.0, "start": [0, 0, 0], "end": [0, 0, 30] })).unwrap();
    wb.run_ui("sketch3d.finish").unwrap();
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.shown_sketches3d(), [(f, false)]);
    let row = d.text_pos("3D Sketch1").unwrap_or_else(|| panic!("no row: {:?}", d.texts()));
    d.click(&mut wb, row);
    d.click(&mut wb, row);
    d.frame(&mut wb, vec![]);
    assert_eq!(sketch_open(&wb), f, "{}", wb.status());
    wb.run_ui("sketch3d.finish").unwrap();
    d.frame(&mut wb, vec![]);
    // Hidden, and shown, by its Visibility.
    wb.exec("feature.visibility", json!({ "feature": f.0, "visible": false })).unwrap();
    assert!(wb.shown_sketches3d().is_empty());
    wb.exec("feature.visibility", json!({ "feature": f.0, "visible": true })).unwrap();
    // Suppressed: not shown, and no error to speak of.
    wb.exec("feature.suppress", json!({ "feature": f.0 })).unwrap();
    d.frame(&mut wb, vec![]);
    assert!(wb.shown_sketches3d().is_empty());
    wb.exec("feature.suppress", json!({ "feature": f.0, "suppressed": false })).unwrap();
    // Measure: a click on the curve says how long it is.
    wb.view.camera.fit(&tenon_geom::Aabb3::from_points(wb.document().sketch3d(f).unwrap().extent()).unwrap());
    settle(&mut d, &mut wb);
    wb.run_ui("inspect.measure").unwrap();
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, Vec3::new(0.0, 0.0, 15.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Measure(m)) = &wb.panel else { panic!() };
    let got =
        m.result.as_ref().and_then(|r| r.as_ref().ok()).and_then(|r| r.get("Length")).unwrap_or_else(|| panic!("nothing measured: {:?}", m.result));
    assert!((got - 30.0).abs() < 1e-9, "{got}");
}

#[test]
fn a_work_plane_square_to_a_curve_and_a_work_point_at_its_end_by_clicks() {
    let (mut wb, mut d) = app();
    wb.run_ui("sketch.new_3d").unwrap();
    let f = sketch_open(&wb);
    let l = wb.exec("sketch3d.line", json!({ "sketch": f.0, "start": [10, 20, 30], "end": [10, 60, 30] })).unwrap();
    let end = EntityId(l["end"].as_u64().unwrap() as u32);
    wb.run_ui("sketch3d.finish").unwrap();
    d.frame(&mut wb, vec![]);
    wb.view.camera.fit(&tenon_geom::Aabb3::from_points(wb.document().sketch3d(f).unwrap().extent()).unwrap());
    settle(&mut d, &mut wb);
    // A plane square to the line, through the end nearer the click.
    wb.run_ui("work.plane").unwrap();
    let Some(Panel::Work(w)) = &mut wb.panel else { panic!() };
    **w = crate::work::WorkPanel { slot: crate::work::WorkSlot::Curve, method: crate::work::WorkMethod::SquareToCurve, ..(**w).clone() };
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, Vec3::new(10.0, 55.0, 30.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Work(w)) = &wb.panel else { panic!() };
    assert_eq!(w.curve.map(|c| c.2), Some(end), "{}", wb.status());
    wb.panel_request = Some(crate::panels::PanelRequest::Ok);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let plane = wb.document().features().last().unwrap();
    assert_eq!(plane.name, "Work Plane1", "{}", wb.status());
    let g = wb.scene().work.iter().find(|w| w.0 == plane.id).map(|w| w.1).unwrap();
    let tenon_model::WorkGeom::Plane(frame) = g else { panic!("{g:?}") };
    assert!(frame.z().cross(Vec3::Y).len() < 1e-9 && frame.origin().dist(Vec3::new(10.0, 60.0, 30.0)) < 1e-9, "{frame:?}");
    // Its double-click reopens the panel as it is.
    wb.edit_feature(plane.id).unwrap();
    assert!(matches!(&wb.panel, Some(Panel::Work(w)) if w.method == crate::work::WorkMethod::SquareToCurve && w.curve.is_some()));
    wb.panel = None;
    // A work point at the line's other end, picked in the view.
    wb.run_ui("work.point").unwrap();
    let Some(Panel::Work(w)) = &mut wb.panel else { panic!() };
    **w = crate::work::WorkPanel { slot: crate::work::WorkSlot::P1, method: crate::work::WorkMethod::PointAt, ..(**w).clone() };
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, Vec3::new(10.0, 20.0, 30.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    wb.panel_request = Some(crate::panels::PanelRequest::Ok);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let point = wb.document().features().last().unwrap();
    assert_eq!(point.name, "Work Point1", "{}", wb.status());
    let g = wb.scene().work.iter().find(|w| w.0 == point.id).map(|w| w.1).unwrap();
    assert!(matches!(g, tenon_model::WorkGeom::Point(q) if q.dist(Vec3::new(10.0, 20.0, 30.0)) < 1e-9), "{g:?}");
}

#[test]
fn dragging_a_point_moves_it_in_the_view_and_a_delete_takes_it_away() {
    let (mut wb, mut d) = app();
    wb.run_ui("sketch.new_3d").unwrap();
    let f = sketch_open(&wb);
    let a = wb.exec("sketch3d.line", json!({ "sketch": f.0, "start": [0, 0, 0], "end": [30, 0, 0] })).unwrap();
    let b = wb.exec("sketch3d.line", json!({ "sketch": f.0, "start": a["end"], "end": [30, 30, 0] })).unwrap();
    let end = EntityId(b["end"].as_u64().unwrap() as u32);
    wb.exec("sketch3d.constrain", json!({ "sketch": f.0, "constraint": { "type": "along", "line": b["line"], "axis": "y" } })).unwrap();
    wb.view.camera.fit(&tenon_geom::Aabb3::from_points(sk(&wb).extent()).unwrap());
    settle(&mut d, &mut wb);
    let from = on_screen(&wb, Vec3::new(30.0, 30.0, 0.0));
    let to = from + vec2(60.0, 40.0);
    d.drag(&mut wb, from, to, egui::PointerButton::Primary);
    d.frame(&mut wb, vec![]);
    let s = sk(&wb);
    let moved = s.point(end).unwrap();
    assert!(moved.dist(Vec3::new(30.0, 30.0, 0.0)) > 1.0, "it moved: {moved:?}");
    // The line stays along Y.
    let (p, q) = s.line(EntityId(b["line"].as_u64().unwrap() as u32)).unwrap();
    assert!((p.x - q.x).abs() < 1e-9 && (p.z - q.z).abs() < 1e-9, "{p:?} {q:?}");
    // Click a line and Delete: gone, and its end point; undo brings it back.
    let line = EntityId(b["line"].as_u64().unwrap() as u32);
    let mid = on_screen(&wb, p.lerp(q, 0.5));
    d.click(&mut wb, mid);
    d.frame(&mut wb, vec![]);
    let Mode::Sketch3d(sm) = &wb.mode else { panic!() };
    assert_eq!(sm.selection, [line], "{}", wb.status());
    d.tap(&mut wb, egui::Key::Delete);
    d.frame(&mut wb, vec![]);
    assert!(sk(&wb).entity(line).is_none());
    wb.run_ui("edit.undo").unwrap();
    assert!(sk(&wb).entity(line).is_some());
}

#[test]
fn start_3d_sketch_is_under_the_arrow_of_start_2d_sketch() {
    let (mut wb, mut d) = app();
    wb.chrome.dropdown = Some(("sketch.new", vec2(10.0, 150.0).to_pos2()));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let row = d.text_pos("Start 3D Sketch").unwrap_or_else(|| panic!("no entry: {:?}", d.texts()));
    d.click(&mut wb, row);
    d.frame(&mut wb, vec![]);
    assert!(wb.is_sketching3d(), "{}", wb.status());
}

#[test]
fn a_line_from_the_origin_to_a_work_point_by_clicks_follows_the_work_point() {
    let (mut wb, mut d) = app();
    let plane = wb.exec("work.plane", json!({ "by": "offset", "base": "xz", "distance": 40 })).unwrap();
    wb.exec("work.point", json!({ "axis": "y", "plane": { "work": plane["feature"] } })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    wb.run_ui("sketch.new_3d").unwrap();
    let f = sketch_open(&wb);
    d.frame(&mut wb, vec![]);
    wb.run_ui("view.home").unwrap();
    wb.view.camera.fit(&tenon_geom::Aabb3::from_points([Vec3::new(-30.0, -60.0, -30.0), Vec3::new(60.0, 60.0, 60.0)]).unwrap());
    settle(&mut d, &mut wb);
    let WorkAt(work_at) = work_at(&wb);
    wb.run_ui("sketch3d.line").unwrap();
    d.frame(&mut wb, vec![]);
    let o = at(&wb, 0.0, 0.0, 0.0);
    d.click(&mut wb, o);
    d.frame(&mut wb, vec![]);
    let Mode::Sketch3d(sm) = &wb.mode else { panic!() };
    assert_eq!(sm.clicks.first().map(|c| c.target.clone()), Some(crate::sketch3d::Target::Origin), "{}", wb.status());
    let w = on_screen(&wb, work_at);
    d.click(&mut wb, w);
    d.frame(&mut wb, vec![]);
    let s = sk(&wb);
    let line = s.entities().find(|(id, _)| s.is_line(*id)).map(|(id, _)| id).unwrap_or_else(|| panic!("no line: {}", wb.status()));
    let (a, b) = s.line(line).unwrap();
    assert!(a.dist(Vec3::ZERO) < 1e-9 && b.dist(work_at) < 1e-9, "{a:?} {b:?}");
    assert_eq!(wb.document().links3(f).len(), 2, "the origin and the work point");
    // On a point of the sketch: the next line shares it.
    add_xyz(&mut d, &mut wb, 30.0, 40.0, 0.0);
    let s = sk(&wb);
    let lines: Vec<EntityId> = s.entities().filter(|(id, _)| s.is_line(*id)).map(|(id, _)| id).collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(s.geometry(lines[0]).unwrap().ends().unwrap().1, s.geometry(lines[1]).unwrap().ends().unwrap().0, "a chain");
    wb.run_ui("sketch3d.finish").unwrap();
    d.frame(&mut wb, vec![]);
    // The plane moved: the work point goes with it, and the line with that.
    let id = FeatureId(plane["feature"].as_u64().unwrap() as u32);
    let mut kind = serde_json::to_value(&wb.document().feature(id).unwrap().kind).unwrap();
    kind["distance"] = json!(55.0);
    wb.exec("feature.update", json!({ "feature": id.0, "kind": kind })).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let now = wb.document().sketch3d(f).unwrap().line(line).unwrap();
    assert!(now.1.dist(work_at) > 10.0 && (now.1.y.abs() - 55.0).abs() < 1e-7 && now.1.x.abs() < 1e-7, "{:?}", now.1);
}

/// Where the part's one work point is.
struct WorkAt(Vec3);
fn work_at(wb: &Workbench) -> WorkAt {
    let q = wb.scene().work.iter().find_map(|w| match w.1 {
        tenon_model::WorkGeom::Point(q) => Some(q),
        _ => None,
    });
    WorkAt(q.unwrap_or_else(|| panic!("no work point: {}", wb.status())))
}

#[test]
fn every_constraint_tool_works_by_clicks_and_a_symbol_clicked_can_be_deleted() {
    let (mut wb, mut d) = app();
    wb.run_ui("sketch.new_3d").unwrap();
    let f = sketch_open(&wb);
    let a = wb.exec("sketch3d.line", json!({ "sketch": f.0, "start": [0, 0, 0], "end": [40, 3, 0] })).unwrap();
    let b = wb.exec("sketch3d.line", json!({ "sketch": f.0, "start": [50, 10, 0], "end": [60, 30, 25] })).unwrap();
    let id = |v: &serde_json::Value| EntityId(v.as_u64().unwrap() as u32);
    let (la, lb) = (id(&a["line"]), id(&b["line"]));
    let (a0, a1, b0, b1) = (id(&a["start"]), id(&a["end"]), id(&b["start"]), id(&b["end"]));
    wb.view.camera.fit(&tenon_geom::Aabb3::from_points(sk(&wb).extent()).unwrap());
    settle(&mut d, &mut wb);
    let on_line = |wb: &Workbench, l: EntityId| {
        let (p, q) = sk(wb).line(l).unwrap();
        on_screen(wb, p.lerp(q, 0.5))
    };
    let on_point = |wb: &Workbench, e: EntityId| on_screen(wb, sk(wb).point(e).unwrap());
    let press = |d: &mut Driver, wb: &mut Workbench, tool: &str, clicks: Vec<Pos2>| {
        wb.run_ui(tool).unwrap();
        d.frame(wb, vec![]);
        for q in clicks {
            d.click(wb, q);
            d.frame(wb, vec![]);
        }
        d.tap(wb, egui::Key::Escape);
        d.tap(wb, egui::Key::Escape);
        d.frame(wb, vec![]);
    };
    // Fix a point.
    let q = on_point(&wb, a0);
    press(&mut d, &mut wb, "sketch3d.fix", vec![q]);
    assert!(sk(&wb).constraints().any(|(_, c)| matches!(c, Constraint3::Fix { point } if *point == a0)), "{}", wb.status());
    // Coincident: the second line's start comes to the first's end.
    let qs = vec![on_point(&wb, a1), on_point(&wb, b0)];
    press(&mut d, &mut wb, "sketch3d.coincident", qs);
    let s = sk(&wb);
    assert!(s.point(a1).unwrap().dist(s.point(b0).unwrap()) < 1e-9, "{}", wb.status());
    // Along X, by a click.
    let q = on_line(&wb, la);
    press(&mut d, &mut wb, "sketch3d.along_x", vec![q]);
    let (p, r) = sk(&wb).line(la).unwrap();
    assert!((p.y - r.y).abs() < 1e-9 && (p.z - r.z).abs() < 1e-9, "{p:?} {r:?}: {}", wb.status());
    // Perpendicular.
    let qs = vec![on_line(&wb, la), on_line(&wb, lb)];
    press(&mut d, &mut wb, "sketch3d.perpendicular", qs);
    let s = sk(&wb);
    let (u, v) = (s.line(la).unwrap(), s.line(lb).unwrap());
    assert!((u.1 - u.0).normalized().dot((v.1 - v.0).normalized()).abs() < 1e-9, "{}", wb.status());
    // Equal.
    let qs = vec![on_line(&wb, la), on_line(&wb, lb)];
    press(&mut d, &mut wb, "sketch3d.equal", qs);
    let s = sk(&wb);
    assert!((s.length(la).unwrap() - s.length(lb).unwrap()).abs() < 1e-9, "{}", wb.status());
    // A coordinate, through its box.
    wb.run_ui("sketch3d.coord_z").unwrap();
    d.frame(&mut wb, vec![]);
    let q = on_point(&wb, b1);
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    value_box(&mut d, &mut wb, 12.5);
    assert!((sk(&wb).point(b1).unwrap().z - 12.5).abs() < 1e-9, "{}", wb.status());
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    // A distance by two points, through its box.
    wb.run_ui("sketch3d.dimension").unwrap();
    d.frame(&mut wb, vec![]);
    for e in [a0, a1] {
        let q = on_point(&wb, e);
        d.click(&mut wb, q);
        d.frame(&mut wb, vec![]);
    }
    value_box(&mut d, &mut wb, 33.0);
    let s = sk(&wb);
    assert!((s.point(a0).unwrap().dist(s.point(a1).unwrap()) - 33.0).abs() < 1e-8, "{}", wb.status());
    assert!((s.length(lb).unwrap() - 33.0).abs() < 1e-8, "equal follows: {}", s.length(lb).unwrap());
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    // The symbol of Along X is clicked and deleted.
    assert!(d.texts().iter().any(|t| t == "X"), "{:?}", d.texts());
    let Mode::Sketch3d(sm) = &wb.mode else { panic!() };
    let along = sk(&wb).constraints().find(|(_, c)| matches!(c, Constraint3::Along { .. })).map(|(c, _)| c).unwrap();
    let badge = sm.badges_of(along).unwrap();
    d.click(&mut wb, badge);
    d.frame(&mut wb, vec![]);
    assert!(wb.status().contains("parallel to X"), "{}", wb.status());
    d.tap(&mut wb, egui::Key::Delete);
    d.frame(&mut wb, vec![]);
    assert!(sk(&wb).constraint(along).is_none());
}

#[test]
fn a_tangent_between_a_line_and_a_spline_by_clicks() {
    let (mut wb, mut d) = app();
    wb.run_ui("sketch.new_3d").unwrap();
    let f = sketch_open(&wb);
    let a = wb.exec("sketch3d.line", json!({ "sketch": f.0, "start": [0, 0, 0], "end": [0, 0, 30] })).unwrap();
    let sp = wb.exec("sketch3d.spline", json!({ "sketch": f.0, "points": [a["end"], [10, 0, 40], [25, 10, 45]] })).unwrap();
    for e in ["start", "end"] {
        wb.exec("sketch3d.constrain", json!({ "sketch": f.0, "constraint": { "type": "fix", "point": a[e] } })).unwrap();
    }
    wb.view.camera.fit(&tenon_geom::Aabb3::from_points(sk(&wb).extent()).unwrap());
    settle(&mut d, &mut wb);
    wb.run_ui("sketch3d.tangent").unwrap();
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, Vec3::new(0.0, 0.0, 15.0));
    d.click(&mut wb, q);
    let spline = EntityId(sp["spline"].as_u64().unwrap() as u32);
    let s = sk(&wb);
    let line = s.polyline(spline);
    let q = on_screen(&wb, line[line.len() * 3 / 4]);
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let t = sk(&wb).spline(spline).unwrap().start_tangent();
    assert!(t.cross(Vec3::Z).len() < 1e-8, "{t:?}: {}", wb.status());
    // Two lines are parallel, not tangent: said so.
    let b = wb.exec("sketch3d.line", json!({ "sketch": f.0, "start": a["end"], "end": [30, 0, 30] })).unwrap();
    let e = wb.exec("sketch3d.constrain", json!({ "sketch": f.0, "constraint": { "type": "tangent", "a": a["line"], "b": b["line"] } })).unwrap_err();
    assert!(e.contains("two lines are Parallel"), "{e}");
}

#[test]
fn a_point_clicked_on_nothing_is_where_the_pointer_is_and_on_a_face_is_on_the_face() {
    let (mut wb, mut d) = app();
    block(&mut wb, &mut d);
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    wb.run_ui("sketch3d.point").unwrap();
    d.frame(&mut wb, vec![]);
    // On the top of the block, away from its corners.
    let q = at(&wb, 20.0, 10.0, 10.0);
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let s = sk(&wb);
    let p = s.entities().find_map(|(id, _)| s.point(id)).unwrap_or_else(|| panic!("no point: {}", wb.status()));
    assert!((p.z - 10.0).abs() < 1e-6 && p.dist(Vec3::new(20.0, 10.0, 10.0)) < 0.5, "{p:?}");
    // On nothing at all: where the pointer is on the screen.
    let r = wb.view.rect;
    let q = r.left_top() + vec2(120.0, 120.0);
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let s = sk(&wb);
    let points: Vec<Vec3> = s.entities().filter_map(|(id, _)| s.point(id)).collect();
    assert_eq!(points.len(), 2, "{}", wb.status());
    assert!(on_screen(&wb, points[1]).distance(q) < 0.5, "{:?} and {q:?}", on_screen(&wb, points[1]));
}

#[test]
fn a_sweeps_path_is_picked_by_a_click_on_a_3d_sketch_and_a_measure_adds_up_curves() {
    let (mut wb, mut d) = app();
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let Mode::Sketch(c) = &wb.mode else { panic!() };
    let circle = c.feature;
    wb.exec("sketch.circle", json!({ "sketch": circle.0, "cx": 0, "cy": 0, "r": 2 })).unwrap();
    wb.finish_sketch();
    let make = |wb: &mut Workbench, end: serde_json::Value| {
        let r = wb.exec("sketch.new_3d", json!({})).unwrap();
        let id = r["feature"].as_u64().unwrap();
        wb.exec("sketch3d.line", json!({ "sketch": id, "start": [0, 0, 0], "end": end })).unwrap();
        FeatureId(id as u32)
    };
    let tall = make(&mut wb, json!([0, 0, 60]));
    let short = make(&mut wb, json!([0, 0, 20]));
    d.frame(&mut wb, vec![]);
    wb.view.camera.fit(&tenon_geom::Aabb3::from_points([Vec3::new(-30.0, -30.0, 0.0), Vec3::new(30.0, 30.0, 60.0)]).unwrap());
    settle(&mut d, &mut wb);
    // Sweep takes the last path drawn; a click on the taller, above the other, takes that.
    wb.run_ui("model.sweep").unwrap();
    d.frame(&mut wb, vec![]);
    let Some(Panel::Sweep(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert_eq!(p.path.as_ref().unwrap().sketch, short);
    let q = on_screen(&wb, Vec3::new(0.0, 0.0, 45.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Sweep(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert_eq!(p.path.as_ref().unwrap().sketch, tall, "{}", wb.status());
    // The preview is of it.
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let v: f64 = wb.scene().bodies.iter().map(|b| b.volume).sum();
    assert!((v - PI * 4.0 * 60.0).abs() < 1e-3, "{v}");
    wb.panel = None;
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    // Measure: a curve; another, on its own; both, with Shift.
    let side = wb.exec("sketch3d.line", json!({ "sketch": tall.0, "start": [0, 0, 60], "end": [30, 0, 60] })).unwrap();
    assert!(side["line"].is_u64());
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    wb.run_ui("inspect.measure").unwrap();
    d.frame(&mut wb, vec![]);
    let got = |wb: &Workbench| {
        let Some(Panel::Measure(m)) = &wb.panel else { panic!() };
        m.result.as_ref().and_then(|r| r.as_ref().ok()).and_then(|r| r.get("Length")).unwrap_or(-1.0)
    };
    let (up, across) = (on_screen(&wb, Vec3::new(0.0, 0.0, 45.0)), on_screen(&wb, Vec3::new(15.0, 0.0, 60.0)));
    d.click(&mut wb, up);
    d.frame(&mut wb, vec![]);
    assert!((got(&wb) - 60.0).abs() < 1e-9, "{}", got(&wb));
    d.click(&mut wb, across);
    d.frame(&mut wb, vec![]);
    assert!((got(&wb) - 30.0).abs() < 1e-9, "{}", got(&wb));
    d.modifiers(&mut wb, egui::Modifiers::SHIFT);
    d.click(&mut wb, up);
    d.frame(&mut wb, vec![]);
    d.modifiers(&mut wb, egui::Modifiers::default());
    assert!((got(&wb) - 90.0).abs() < 1e-9, "{}", got(&wb));
}

#[test]
fn undo_in_the_middle_of_a_chain_of_lines_starts_the_chain_again() {
    let (mut wb, mut d) = app();
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    wb.run_ui("sketch3d.line").unwrap();
    d.frame(&mut wb, vec![]);
    add_xyz(&mut d, &mut wb, 0.0, 0.0, 0.0);
    add_xyz(&mut d, &mut wb, 30.0, 0.0, 0.0);
    assert_eq!(sk(&wb).entities().filter(|(id, _)| sk(&wb).is_line(*id)).count(), 1, "{}", wb.status());
    wb.run_ui("edit.undo").unwrap();
    d.frame(&mut wb, vec![]);
    // The point the chain would go on from is gone: the next two points make a line of their own.
    add_xyz(&mut d, &mut wb, 0.0, 10.0, 0.0);
    assert!(!wb.status_error, "{}", wb.status());
    add_xyz(&mut d, &mut wb, 0.0, 10.0, 25.0);
    assert!(!wb.status_error, "{}", wb.status());
    let s = sk(&wb);
    let line = s.entities().find(|(id, _)| s.is_line(*id)).map(|(id, _)| id).unwrap();
    let (a, b) = s.line(line).unwrap();
    assert!(a.dist(Vec3::new(0.0, 10.0, 0.0)) < 1e-9 && b.dist(Vec3::new(0.0, 10.0, 25.0)) < 1e-9, "{a:?} {b:?}");
}

/// Three lines 50 long along Z, X and Y, from a fixed start, each held to its length.
fn stair(wb: &mut Workbench, d: &mut Driver) -> (FeatureId, [EntityId; 3], [EntityId; 4]) {
    wb.run_ui("sketch.new_3d").unwrap();
    let f = sketch_open(wb);
    let id = |v: &serde_json::Value| EntityId(v.as_u64().unwrap() as u32);
    let a = wb.exec("sketch3d.line", json!({ "sketch": f.0, "start": [0, 0, 0], "end": [0, 0, 50], "along": "z" })).unwrap();
    let b = wb.exec("sketch3d.line", json!({ "sketch": f.0, "start": a["end"], "end": [50, 0, 50], "along": "x" })).unwrap();
    let c = wb.exec("sketch3d.line", json!({ "sketch": f.0, "start": b["end"], "end": [50, 50, 50], "along": "y" })).unwrap();
    wb.exec("sketch3d.constrain", json!({ "sketch": f.0, "constraint": { "type": "fix", "point": a["start"] } })).unwrap();
    for l in [&a, &b, &c] {
        wb.exec("sketch3d.constrain", json!({ "sketch": f.0, "constraint": { "type": "length", "line": l["line"], "value": 50 } })).unwrap();
    }
    wb.view.camera.fit(&tenon_geom::Aabb3::from_points(sk(wb).extent()).unwrap());
    settle(d, wb);
    (f, [id(&a["line"]), id(&b["line"]), id(&c["line"])], [id(&a["start"]), id(&a["end"]), id(&b["end"]), id(&c["end"])])
}

/// A circle of radius `r` at (`cx`, `cy`) on a plane, in a sketch of its own: a profile.
fn circle_on(wb: &mut Workbench, d: &mut Driver, plane: &str, cx: f64, cy: f64, r: f64) -> FeatureId {
    wb.create_sketch(json!({ "plane": plane })).unwrap();
    let Mode::Sketch(c) = &wb.mode else { panic!() };
    let f = c.feature;
    wb.exec("sketch.circle", json!({ "sketch": f.0, "cx": cx, "cy": cy, "r": r })).unwrap();
    wb.finish_sketch();
    d.frame(wb, vec![]);
    f
}

fn volume_of(wb: &Workbench) -> f64 {
    wb.scene().bodies.iter().map(|b| b.volume).sum()
}

/// OK in the open panel, and the frames it takes to show.
fn ok_panel(d: &mut Driver, wb: &mut Workbench) {
    wb.panel_request = Some(crate::panels::PanelRequest::Ok);
    d.frame(wb, vec![]);
    d.frame(wb, vec![]);
    d.frame(wb, vec![]);
}

#[test]
fn a_radius_too_large_for_its_lines_is_refused_in_the_box_and_the_red_goes_when_one_fits() {
    let (mut wb, mut d) = app();
    let (_, l, _) = stair(&mut wb, &mut d);
    // By the Bend tool: 80 will not go, and the box stays; 10 does, and nothing red is left.
    wb.run_ui("sketch3d.bend").unwrap();
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, Vec3::new(0.0, 0.0, 50.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    value_box(&mut d, &mut wb, 80.0);
    assert!(wb.status_error && wb.status().contains("the largest radius that fits is just under 50.000 mm"), "{}", wb.status());
    assert!(matches!(wb.panel, Some(Panel::Value(_))), "the box stays");
    value_box(&mut d, &mut wb, 10.0);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!(!wb.status_error, "what was said of 80 is over: {}", wb.status());
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    // Double-clicked to change: 70 is refused the same way, and the line stays as it was.
    let before = sk(&wb);
    let label = d.text_pos("R10").unwrap_or_else(|| panic!("no radius: {:?}", d.texts()));
    double_click(&mut d, &mut wb, label);
    d.frame(&mut wb, vec![]);
    value_box(&mut d, &mut wb, 70.0);
    assert!(wb.status_error && wb.status().contains("the largest radius that fits is just under 50.000 mm"), "{}", wb.status());
    assert_eq!(sk(&wb), before, "no line running back through its end");
    value_box(&mut d, &mut wb, 20.0);
    assert!(wb.panel.is_none() && !wb.status_error, "{}", wb.status());
    let (a, b) = sk(&wb).line(l[0]).unwrap();
    assert!(a.dist(Vec3::ZERO) < 1e-7 && b.dist(Vec3::new(0.0, 0.0, 30.0)) < 1e-7, "{a:?} {b:?}");
}

#[test]
fn a_bend_by_the_window_under_a_sweep_keeps_the_sweep_and_the_view_shows_it_again_at_finish() {
    let (mut wb, mut d) = app();
    let (f, _, _) = stair(&mut wb, &mut d);
    wb.run_ui("sketch3d.finish").unwrap();
    circle_on(&mut wb, &mut d, "xy", 0.0, 0.0, 5.0);
    wb.run_ui("model.sweep").unwrap();
    ok_panel(&mut d, &mut wb);
    let sharp = PI * 25.0 * 150.0;
    assert!((volume_of(&wb) - sharp).abs() < 1e-6 * sharp, "{}: {}", volume_of(&wb), wb.status());
    // Opened again by its row, and rounded.
    let row = d.text_pos("3D Sketch1").unwrap_or_else(|| panic!("no row: {:?}", d.texts()));
    d.click(&mut wb, row);
    d.click(&mut wb, row);
    d.frame(&mut wb, vec![]);
    assert_eq!(sketch_open(&wb), f, "{}", wb.status());
    settle(&mut d, &mut wb);
    wb.run_ui("sketch3d.bend").unwrap();
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, Vec3::new(0.0, 0.0, 50.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    value_box(&mut d, &mut wb, 10.0);
    assert!(!wb.status_error, "{}", wb.status());
    settle(&mut d, &mut wb);
    // While it is open the part is as it was when opened, nothing red about it.
    assert!((volume_of(&wb) - sharp).abs() < 1e-6 * sharp, "{}", volume_of(&wb));
    assert!(!wb.scene().bodies.is_empty());
    wb.run_ui("sketch3d.finish").unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let bent = PI * 25.0 * (150.0 - 20.0 + FRAC_PI_2 * 10.0);
    assert!((volume_of(&wb) - bent).abs() < 1e-6 * bent, "{} and {bent}: {}", volume_of(&wb), wb.status());
}

#[test]
fn a_spline_typed_is_made_by_enter_after_its_points_went_in_by_enter_or_by_add() {
    let (mut wb, mut d) = app();
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    wb.run_ui("sketch3d.spline").unwrap();
    d.frame(&mut wb, vec![]);
    // Enter in the Z box puts a point there, and does no more.
    for (n, p) in [[0.0, 0.0, 0.0], [10.0, 5.0, 0.0], [20.0, 0.0, 5.0], [30.0, 5.0, 5.0]].into_iter().enumerate() {
        for (field, v) in [("tn_s3_x", p[0]), ("tn_s3_y", p[1])] {
            type_into(&mut d, &mut wb, field, &v.to_string());
            d.tap(&mut wb, egui::Key::Tab);
        }
        type_into(&mut d, &mut wb, "tn_s3_z", &p[2].to_string());
        d.tap(&mut wb, egui::Key::Enter);
        d.frame(&mut wb, vec![]);
        let Mode::Sketch3d(sm) = &wb.mode else { panic!() };
        assert_eq!(sm.clicks.len(), n + 1, "{}", wb.status());
        assert!(!wb.status_error && sk(&wb).is_empty(), "{}", wb.status());
    }
    // Enter again, the box not having the keys: the spline, through the four.
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    let s = sk(&wb);
    let id =
        s.entities().find(|(_, e)| matches!(e.geometry, Geometry3::Spline { .. })).map(|(id, _)| id).unwrap_or_else(|| panic!("{}", wb.status()));
    assert_eq!(s.geometry(id).unwrap().points().len(), 4);
    // Three more by Add, then Enter: no fourth by Add again, and a spline through three.
    let Mode::Sketch3d(sm) = &wb.mode else { panic!() };
    assert!(sm.clicks.is_empty());
    for p in [[0.0, 20.0, 0.0], [10.0, 25.0, 0.0], [20.0, 20.0, 5.0]] {
        add_xyz(&mut d, &mut wb, p[0], p[1], p[2]);
    }
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    let s = sk(&wb);
    let splines: Vec<EntityId> = s.entities().filter(|(_, e)| matches!(e.geometry, Geometry3::Spline { .. })).map(|(id, _)| id).collect();
    assert_eq!(splines.len(), 2, "{}", wb.status());
    assert_eq!(s.geometry(splines[1]).unwrap().points().len(), 3, "{}", wb.status());
    assert!(!wb.status_error, "{}", wb.status());
}

#[test]
fn after_add_the_keys_are_the_sketchs_undo_and_the_tools() {
    let (mut wb, mut d) = app();
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    wb.run_ui("sketch3d.line").unwrap();
    d.frame(&mut wb, vec![]);
    let lines = |wb: &Workbench| {
        let s = sk(wb);
        s.entities().filter(|(id, _)| s.is_line(*id)).count()
    };
    for p in [[0.0, 0.0, 0.0], [30.0, 0.0, 0.0], [30.0, 30.0, 0.0]] {
        add_xyz(&mut d, &mut wb, p[0], p[1], p[2]);
    }
    assert_eq!(lines(&wb), 2, "{}", wb.status());
    assert!(!d.ctx.egui_wants_keyboard_input(), "the button gave the keys back");
    ctrl(&mut d, &mut wb, egui::Key::Z);
    d.frame(&mut wb, vec![]);
    assert_eq!(lines(&wb), 1, "Ctrl+Z, straight after Add");
    ctrl(&mut d, &mut wb, egui::Key::Z);
    d.frame(&mut wb, vec![]);
    assert_eq!(lines(&wb), 0);
    // A tool by its key.
    add_xyz(&mut d, &mut wb, 0.0, 10.0, 0.0);
    d.tap(&mut wb, egui::Key::D);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.active_tool_id(), Some("sketch3d.dimension"), "{}", wb.status());
}

#[test]
fn a_dimensions_value_is_selected_by_a_click_and_deleted_as_a_symbol_is() {
    let (mut wb, mut d) = app();
    let (_, l, _) = stair(&mut wb, &mut d);
    let held = |wb: &Workbench, line: EntityId| {
        sk(wb).constraints().find(|(_, c)| matches!(c, Constraint3::Length { line: x, .. } if *x == line)).map(|(c, _)| c)
    };
    let cid = held(&wb, l[0]).unwrap();
    d.frame(&mut wb, vec![]);
    let Mode::Sketch3d(sm) = &wb.mode else { panic!() };
    let on = sm.dim_of(cid).unwrap();
    d.click(&mut wb, on);
    d.frame(&mut wb, vec![]);
    let Mode::Sketch3d(sm) = &wb.mode else { panic!() };
    assert_eq!(sm.constraints, [cid], "{}", wb.status());
    assert!(wb.status().contains("this line 50 mm long") && wb.status().contains("Delete removes it"), "{}", wb.status());
    // A second length: said what is in the way, by what it can be told by.
    wb.run_ui("sketch3d.dimension").unwrap();
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, Vec3::new(0.0, 0.0, 25.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    value_box(&mut d, &mut wb, 30.0);
    assert!(wb.status_error, "{}", wb.status());
    assert!(wb.status().contains("this line 30 mm long cannot be had") && wb.status().contains("this line 50 mm long"), "{}", wb.status());
    assert!(!wb.status().contains(&format!("{}", l[0])), "no ids: {}", wb.status());
    wb.panel = None;
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    // Selected, Delete takes it: the line is free to be 30.
    let Mode::Sketch3d(sm) = &wb.mode else { panic!() };
    let on = sm.dim_of(cid).unwrap();
    d.click(&mut wb, on);
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Delete);
    d.frame(&mut wb, vec![]);
    assert!(held(&wb, l[0]).is_none(), "{}", wb.status());
    wb.run_ui("edit.undo").unwrap();
    assert!(held(&wb, l[0]).is_some());
}

#[test]
fn an_edge_brought_in_is_a_path_from_the_ribbon_and_is_not_lost_while_it_comes_in() {
    let (mut wb, mut d) = app();
    // A disc: its rim alone, by a click, with nothing else in the sketch.
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let Mode::Sketch(c) = &wb.mode else { panic!() };
    let f = c.feature;
    wb.exec("sketch.circle", json!({ "sketch": f.0, "cx": 0, "cy": 0, "r": 30 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    d.frame(&mut wb, vec![]);
    settle(&mut d, &mut wb);
    wb.run_ui("view.home").unwrap();
    settle(&mut d, &mut wb);
    let disc = volume_of(&wb);
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    let path = sketch_open(&wb);
    wb.run_ui("sketch3d.include").unwrap();
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, Vec3::new(0.0, -30.0, 10.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let rim = sk(&wb)
        .entities()
        .find(|(_, e)| matches!(e.geometry, Geometry3::Edge { .. }))
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("nothing came in: {}", wb.status()));
    // Not lost: it is in the colour of what follows the part from the first.
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(!wb.scene().lost.contains_key(&path), "{:?}", wb.scene().lost);
    wb.run_ui("sketch3d.finish").unwrap();
    d.frame(&mut wb, vec![]);
    // A circle across the rim where it starts: on the XZ plane, at (30, 10).
    circle_on(&mut wb, &mut d, "xz", 30.0, 10.0, 2.0);
    wb.run_ui("model.sweep").unwrap();
    let Some(Panel::Sweep(p)) = wb.panel.clone() else { panic!("no sweep panel: {}", wb.status()) };
    assert_eq!(p.path.as_ref().unwrap().curves, [rim], "{:?}", p.path);
    let Some(Panel::Sweep(p)) = &mut wb.panel else { panic!() };
    p.operation = tenon_model::Operation::NewBody;
    ok_panel(&mut d, &mut wb);
    let want = PI * 4.0 * 2.0 * PI * 30.0;
    assert!((volume_of(&wb) - disc - want).abs() < 1e-3 * want, "{} and {want}: {}", volume_of(&wb) - disc, wb.status());
}

#[test]
fn a_line_on_from_an_edge_brought_in_joins_it_for_a_path_and_a_click_chooses_a_run() {
    let (mut wb, mut d) = app();
    block(&mut wb, &mut d);
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    let path = sketch_open(&wb);
    // The top edge along X at y = 0, brought in by a click.
    wb.run_ui("sketch3d.include").unwrap();
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, Vec3::new(20.0, 0.0, 10.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let s = sk(&wb);
    let edge = s.entities().find(|(id, _)| s.is_line(*id)).map(|(id, _)| id).unwrap_or_else(|| panic!("nothing came in: {}", wb.status()));
    let (a, b) = s.line(edge).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(!wb.scene().lost.contains_key(&path), "{:?}", wb.scene().lost);
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    // A line on from its end at the far corner: a click on that point, and one above it.
    let corner = if a.x > b.x { a } else { b };
    wb.run_ui("sketch3d.line").unwrap();
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, corner);
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let up = on_screen(&wb, corner + Vec3::new(0.0, 0.0, 15.0));
    d.click(&mut wb, up);
    d.frame(&mut wb, vec![]);
    // And one apart, of its own.
    d.tap(&mut wb, egui::Key::Escape);
    add_xyz(&mut d, &mut wb, 80.0, 80.0, 0.0);
    add_xyz(&mut d, &mut wb, 80.0, 80.0, 30.0);
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    let s = sk(&wb);
    let lines: Vec<EntityId> = s.entities().filter(|(id, e)| s.is_line(*id) && !e.reference).map(|(id, _)| id).collect();
    assert_eq!(lines.len(), 2, "{}", wb.status());
    wb.run_ui("sketch3d.finish").unwrap();
    d.frame(&mut wb, vec![]);
    circle_on(&mut wb, &mut d, "xy", 80.0, 80.0, 1.0);
    wb.view.camera.fit(&tenon_geom::Aabb3::from_points([Vec3::new(-10.0, -10.0, 0.0), Vec3::new(90.0, 90.0, 40.0)]).unwrap());
    settle(&mut d, &mut wb);
    // The two of its own do not join: the default is both, a click on one takes its run.
    wb.run_ui("model.sweep").unwrap();
    d.frame(&mut wb, vec![]);
    let Some(Panel::Sweep(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert_eq!(p.path.as_ref().unwrap().sketch, path);
    assert_eq!(p.path.as_ref().unwrap().curves.len(), 2, "{:?}", p.path);
    let q = on_screen(&wb, corner + Vec3::new(0.0, 0.0, 7.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Sweep(p)) = &wb.panel else { panic!("{}", wb.status()) };
    let curves = &p.path.as_ref().unwrap().curves;
    assert_eq!(curves.len(), 2, "the line, and the edge it goes on from: {curves:?}");
    assert!(curves.contains(&edge) && curves.contains(&lines[0]), "{curves:?} {edge:?} {lines:?}");
    // On the other: that alone.
    let q = on_screen(&wb, Vec3::new(80.0, 80.0, 15.0));
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Sweep(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert_eq!(p.path.as_ref().unwrap().curves, [lines[1]]);
}

#[test]
fn what_a_bend_tool_click_cannot_round_it_says_before_asking_a_radius() {
    let (mut wb, mut d) = app();
    stair(&mut wb, &mut d);
    wb.run_ui("sketch3d.bend").unwrap();
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, Vec3::ZERO);
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "no box");
    assert!(wb.status_error && wb.status().contains("Only one curve ends at that point"), "{}", wb.status());
    // A bend, and a click on the corner it kept.
    let corner = on_screen(&wb, Vec3::new(0.0, 0.0, 50.0));
    d.click(&mut wb, corner);
    d.frame(&mut wb, vec![]);
    value_box(&mut d, &mut wb, 10.0);
    d.click(&mut wb, corner);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "no box");
    assert!(wb.status_error && wb.status().contains("is the corner of a bend already"), "{}", wb.status());
}

#[test]
fn a_bends_centre_clicked_and_deleted_gives_its_corner_back() {
    let (mut wb, mut d) = app();
    let (f, l, p) = stair(&mut wb, &mut d);
    let b = wb.exec("sketch3d.bend", json!({ "sketch": f.0, "point": p[1].0, "radius": 10 })).unwrap();
    let centre = sk(&wb).point(EntityId(b["center"].as_u64().unwrap() as u32)).unwrap();
    d.frame(&mut wb, vec![]);
    let q = on_screen(&wb, centre);
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Delete);
    d.frame(&mut wb, vec![]);
    let s = sk(&wb);
    assert_eq!(s.entities().filter(|(id, _)| s.is_arc(*id)).count(), 0, "{}", wb.status());
    assert_eq!(s.entities().filter(|(_, e)| e.geometry.is_point()).count(), 4, "{s:?}");
    let length: f64 = l.iter().map(|c| s.length(*c).unwrap()).sum();
    assert!((length - 150.0).abs() < 1e-7, "{length}");
    assert_eq!(s.chain(&l).unwrap().len(), 3);
}

#[test]
fn the_status_bar_says_nothing_of_constraints_until_there_is_something_and_why_a_point_stays() {
    let (mut wb, mut d) = app();
    block(&mut wb, &mut d);
    wb.run_ui("sketch.new_3d").unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    let f = sketch_open(&wb);
    assert!(d.texts().iter().any(|t| t.contains("Nothing drawn yet")), "{:?}", d.texts());
    assert!(!d.texts().iter().any(|t| t.contains("Fully Constrained")));
    // A line from a corner of the part, by clicks; the point on the corner, dragged, stays and says.
    wb.run_ui("sketch3d.line").unwrap();
    d.frame(&mut wb, vec![]);
    let corner = Vec3::new(40.0, 20.0, 10.0);
    let q = on_screen(&wb, corner);
    d.click(&mut wb, q);
    d.frame(&mut wb, vec![]);
    let up = on_screen(&wb, corner + Vec3::new(0.0, 0.0, 15.0));
    d.click(&mut wb, up);
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    assert!(!d.texts().iter().any(|t| t.contains("Nothing drawn yet")), "{:?}", d.texts());
    // It is that corner whichever way the kernel comes to name its edge: it said which way it lay.
    let sel = wb.document().links3(f).iter().find_map(|l| match &l.to {
        tenon_model::sketch3d::LinkTo::Point(tenon_model::PointSel::Vertex { towards, .. }) => Some(*towards),
        _ => None,
    });
    assert!(matches!(sel, Some(Some(t)) if (t.len() - 1.0).abs() < 1e-9), "{sel:?}");
    let before = sk(&wb);
    let q = on_screen(&wb, corner);
    d.drag(&mut wb, q, q + vec2(50.0, 30.0), egui::PointerButton::Primary);
    d.frame(&mut wb, vec![]);
    assert_eq!(sk(&wb), before);
    assert!(wb.status().contains("follows the part"), "{}", wb.status());
}
