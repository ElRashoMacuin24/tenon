//! The sketch environment, round two (DEC-055 onward), by real pointer and key input: edges of
//! the part shown in a sketch, centre lines and diameters to them, and the sketch's Modify and
//! Pattern tools.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use egui::{Pos2, vec2};
use serde_json::json;
use tenon_geom::{Vec2, Vec3};
use tenon_kernel_occt::OcctKernel;
use tenon_model::FeatureId;
use tenon_sketch::{Constraint, EntityId, Geometry};

use crate::Workbench;
use crate::panels::Panel;
use crate::tests::{Driver, on_screen, pressable};
use crate::workbench::Mode;

fn sketching(wb: &Workbench) -> FeatureId {
    match &wb.mode {
        Mode::Sketch(s) => s.feature,
        Mode::Model => panic!("not in sketch mode"),
    }
}

fn volume(wb: &Workbench) -> f64 {
    wb.scene().bodies.iter().map(|b| b.volume).sum()
}

/// A block 40 x 20 x 10 with a corner on the origin; its width is a dimension. Returns the
/// base sketch and that dimension.
fn block(wb: &mut Workbench) -> (FeatureId, u64) {
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(wb);
    let r = wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    let origin = wb.document().sketch(f).unwrap().entities().next().unwrap().0;
    wb.exec("sketch.constrain", json!({ "sketch": f.0, "constraint": { "type": "coincident", "a": r["corners"][0], "b": origin.0 } })).unwrap();
    let width =
        wb.exec("sketch.constrain", json!({ "sketch": f.0, "constraint": { "type": "length", "line": r["lines"][0], "value": 40.0 } })).unwrap();
    wb.exec("sketch.constrain", json!({ "sketch": f.0, "constraint": { "type": "length", "line": r["lines"][1], "value": 20.0 } })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    (f, width["constraint"].as_u64().unwrap())
}

/// Start 2D Sketch, then a click on the middle of the block's top.
fn sketch_on_top(d: &mut Driver, wb: &mut Workbench) -> FeatureId {
    wb.run_ui("sketch.new").unwrap();
    d.frame(wb, vec![]);
    let top = on_screen(wb, Vec3::new(20.0, 10.0, 10.0));
    d.frame(wb, vec![egui::Event::PointerMoved(top)]);
    d.click(wb, top);
    d.settle(wb);
    sketching(wb)
}

/// Where sketch point `id` is on the part.
fn on_part(wb: &Workbench, sketch: FeatureId, id: EntityId) -> Vec3 {
    let p = wb.document().sketch(sketch).unwrap().point(id).unwrap();
    wb.sketch_frame(sketch).unwrap().plane_point(p)
}

fn move_to(d: &mut Driver, wb: &mut Workbench, p: Pos2) {
    d.frame(wb, vec![egui::Event::PointerMoved(p)]);
    d.frame(wb, vec![]);
}

#[test]
fn a_sketch_on_a_face_shows_its_edges_and_a_hole_is_dimensioned_from_one() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let (base, width) = block(&mut wb);
    d.settle(&mut wb);
    let sk = sketch_on_top(&mut d, &mut wb);
    // The top's four edges are in the sketch as reference geometry, and the sketch is held.
    assert_eq!(wb.document().projections(sk).len(), 4);
    let sketch = wb.document().sketch(sk).unwrap().clone();
    assert_eq!(sketch.entities().filter(|(id, e)| e.reference && sketch.is_line(*id)).count(), 4);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.sketch_dof_text().unwrap(), "Fully Constrained");
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 10.0));

    // A circle near the right-hand end, then Dimension: its centre, the right-hand edge, a
    // place for the value, and 8 typed in.
    d.tap(&mut wb, egui::Key::C);
    for (x, y) in [(30.0, 8.0), (33.0, 8.0)] {
        let p = at(&wb, x, y);
        d.click(&mut wb, p);
    }
    d.tap(&mut wb, egui::Key::Escape);
    let sketch = wb.document().sketch(sk).unwrap().clone();
    let (ring, centre) = sketch
        .entities()
        .find_map(|(id, e)| match e.geometry {
            Geometry::Circle { center, .. } => Some((id, center)),
            _ => None,
        })
        .expect("the circle");
    // (Clicks land on pixels: the size is given exactly.)
    wb.exec("sketch.constrain", json!({ "sketch": sk.0, "constraint": { "type": "diameter", "curve": ring.0, "value": 6.0 } })).unwrap();
    d.tap(&mut wb, egui::Key::D);
    for (x, y) in [(30.0, 8.0), (40.0, 15.0)] {
        let p = at(&wb, x, y);
        d.click(&mut wb, p);
    }
    let place = at(&wb, 35.0, 3.0);
    move_to(&mut d, &mut wb, place);
    assert!(d.texts().iter().any(|t| t == "10"), "10 from the edge as drawn: {:?}", d.texts());
    d.click(&mut wb, place);
    assert!(matches!(wb.panel, Some(Panel::Value(_))), "the value box is open: {}", wb.status());
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![egui::Event::Text("8".into())]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let c = on_part(&wb, sk, centre);
    assert!((c.x - 32.0).abs() < 1e-6, "8 from the right-hand edge: {c:?}");
    let made: Vec<Constraint> = wb.document().sketch(sk).unwrap().constraints().filter(|(_, c)| c.is_dimensional()).map(|(_, c)| c.clone()).collect();
    assert!(made.iter().any(|c| matches!(c, Constraint::Distance { a, b, .. } if *a == centre && sketch.is_reference(*b))), "{made:?}");

    // Finished and cut through: the circle is what Extrude takes, not the top with a hole in it.
    d.tap(&mut wb, egui::Key::Escape);
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": sk.0, "through_all": true, "reverse": true, "operation": "cut" })).unwrap();
    d.settle(&mut wb);
    assert!((volume(&wb) - (8000.0 - 9.0 * PI * 10.0)).abs() < 1e-6, "{}", volume(&wb));
    // The block made 60 wide: the edge is found again further on, the sketch follows it in the
    // document too, and the hole is still 8 from the end.
    wb.exec("sketch.set_dimension", json!({ "sketch": base.0, "constraint": width, "value": 60.0 })).unwrap();
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    assert!((volume(&wb) - (12000.0 - 9.0 * PI * 10.0)).abs() < 1e-6, "{} {} {:?}", volume(&wb), wb.status(), wb.scene().status);
    let c = on_part(&wb, sk, centre);
    assert!((c.x - 52.0).abs() < 1e-6, "{c:?}");
    assert!(wb.scene().lost.is_empty());
    let _ = ring;

    // With the option off (Application Options), a new sketch on a face shows nothing of it.
    wb.run_ui("tools.options").unwrap();
    for _ in 0..5 {
        d.frame(&mut wb, vec![]);
    }
    assert!(d.texts().iter().any(|t| t == "Autoproject edges for sketch creation and edit"), "{:?}", d.texts());
    d.click(&mut wb, pressable(&d, "tn_options_autoproject"));
    d.frame(&mut wb, vec![]);
    assert!(!wb.chrome.autoproject);
    wb.chrome.options = false;
    d.frame(&mut wb, vec![]);
    let bare = sketch_on_top(&mut d, &mut wb);
    assert_ne!(bare, sk);
    assert!(wb.document().projections(bare).is_empty());
}

#[test]
fn project_geometry_shows_a_clicked_edge_or_face_in_the_sketch() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    block(&mut wb);
    d.settle(&mut wb);
    // A sketch on a work plane 15 above the block: nothing of the part is in it to begin with.
    let plane = wb.exec("work.plane", json!({ "base": "xy", "distance": 25 })).unwrap()["feature"].as_u64().unwrap();
    d.settle(&mut wb);
    wb.create_sketch(json!({ "work_plane": plane })).unwrap();
    let sk = sketching(&wb);
    d.settle(&mut wb);
    assert!(wb.document().projections(sk).is_empty());

    // Project Geometry, then a click on the top's front edge: it is in the sketch, 40 long.
    wb.run_ui("sketch.project").unwrap();
    assert_eq!(wb.active_tool_id(), Some("sketch.project"));
    let edge = on_screen(&wb, Vec3::new(14.0, 0.0, 10.0));
    move_to(&mut d, &mut wb, edge);
    assert!(matches!(wb.view.hover, Some(crate::viewport::Pick::Edge { .. })), "the edge lights up: {:?}", wb.view.hover);
    d.click(&mut wb, edge);
    assert!(!wb.status_error && wb.status().starts_with("Projected: the edge is reference geometry"), "{}", wb.status());
    let shown = wb.document().projections(sk).to_vec();
    assert_eq!(shown.len(), 1);
    let sketch = wb.document().sketch(sk).unwrap().clone();
    let (a, b) = sketch.line(shown[0].entity).expect("a line");
    assert!((a.dist(b) - 40.0).abs() < 1e-9 && sketch.is_reference(shown[0].entity));
    // The same edge again adds nothing; a click on the top face adds its other three edges.
    d.click(&mut wb, edge);
    assert_eq!(wb.document().projections(sk).len(), 1);
    let face = on_screen(&wb, Vec3::new(22.0, 11.0, 10.0));
    move_to(&mut d, &mut wb, face);
    assert!(matches!(wb.view.hover, Some(crate::viewport::Pick::Face { .. })));
    d.click(&mut wb, face);
    assert_eq!(wb.document().projections(sk).len(), 4, "{}", wb.status());
    assert!(wb.status().starts_with("Projected 4 edges"), "{}", wb.status());
    // A click on nothing says what to click. Esc ends the tool and nothing stays lit.
    let nowhere = on_screen(&wb, Vec3::new(-30.0, -30.0, 10.0));
    move_to(&mut d, &mut wb, nowhere);
    d.click(&mut wb, nowhere);
    assert!(wb.status_error && wb.status().contains("click an edge of the part"), "{}", wb.status());
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.active_tool_id(), None);
    assert!(wb.view.hover.is_none());

    // Selected and deleted, a projected edge is gone from the sketch and forgotten.
    let on_line = on_screen(&wb, wb.sketch_frame(sk).unwrap().plane_point(a.mid(b)));
    d.click(&mut wb, on_line);
    d.tap(&mut wb, egui::Key::Delete);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.document().projections(sk).len(), 3);
    // One undo brings it back with its link.
    wb.run_ui("edit.undo").unwrap();
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.document().projections(sk).len(), 4);
    // Break Link (the right-click menu) makes the selected projected edge the sketch's own; it
    // says what it is for when what is selected is not projected.
    d.click(&mut wb, on_line);
    wb.run_ui("sketch.break_link").unwrap();
    assert_eq!(wb.document().projections(sk).len(), 3);
    assert!(!wb.document().sketch(sk).unwrap().is_reference(shown[0].entity));
    assert!(wb.run_ui("sketch.break_link").unwrap_err().contains("select projected geometry"));
}

#[test]
fn projected_edges_follow_the_part_on_the_geometry_thread() {
    let _quiet = crate::tests::timing_lock();
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
    let (base, width) = block(&mut wb);
    settle(&mut d, &mut wb);
    // A sketch on the top by a click, as the app does it: its edges come from what is on
    // screen (the kernel is on the other thread), and the view turns to it once its plane is
    // known.
    let sk = sketch_on_top(&mut d, &mut wb);
    settle(&mut d, &mut wb);
    assert_eq!(wb.document().projections(sk).len(), 4);
    assert!(wb.view.aim_at.is_none(), "the view has turned to the sketch");
    let top_right = |wb: &Workbench| -> f64 {
        let s = wb.document().sketch(sk).unwrap();
        let frame = wb.sketch_frame(sk).unwrap();
        s.entities().filter(|(_, e)| e.reference).filter_map(|(id, _)| s.point(id)).map(|p| frame.plane_point(p).x).fold(f64::MIN, f64::max)
    };
    assert!((top_right(&wb) - 40.0).abs() < 1e-9);
    wb.finish_sketch();
    // The block made 55 wide: once it is rebuilt, the document's sketch has the edges there.
    wb.exec("sketch.set_dimension", json!({ "sketch": base.0, "constraint": width, "value": 55.0 })).unwrap();
    settle(&mut d, &mut wb);
    assert!((top_right(&wb) - 55.0).abs() < 1e-9, "{}", top_right(&wb));
    assert!(wb.scene().lost.is_empty() && !wb.status_error, "{}", wb.status());
}

#[test]
fn a_centre_line_gives_diameters_and_is_the_axis_revolve_takes() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    // Half the section of a pin above its axis: the axis from the origin along X, the outline
    // 10 above it.
    let pts = [(0.0, 0.0), (30.0, 0.0), (30.0, 10.0), (0.0, 10.0)];
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    d.settle(&mut wb);
    wb.run_ui("sketch.line").unwrap();
    for (x, y) in pts.iter().chain([&pts[0]]) {
        let p = at(&wb, *x, *y);
        d.click(&mut wb, p);
    }
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    let sketch = wb.document().sketch(f).unwrap().clone();
    let axis = sketch
        .entities()
        .find(|(id, _)| sketch.line(*id).is_some_and(|(a, b)| a.y.abs() < 1e-9 && b.y.abs() < 1e-9))
        .map(|(id, _)| id)
        .expect("the line along X");
    // Nothing selected: Centerline says what it needs. With the axis selected it is one.
    assert!(wb.run_ui("sketch.centerline").unwrap_err().contains("select the line"));
    let on_axis = at(&wb, 15.0, 0.0);
    d.click(&mut wb, on_axis);
    wb.run_ui("sketch.centerline").unwrap();
    assert!(wb.document().sketch(f).unwrap().is_centerline(axis), "{}", wb.status());
    assert!(wb.status().starts_with("Centre line"), "{}", wb.status());

    // Dimension: the top line, then the centre line. It reads as a diameter across the axis,
    // twice the 10 the line is above it; 24 typed in makes the pin 24 across.
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::D);
    for (x, y) in [(15.0, 10.0), (20.0, 0.0)] {
        let p = at(&wb, x, y);
        d.click(&mut wb, p);
    }
    let place = at(&wb, 40.0, 4.0);
    move_to(&mut d, &mut wb, place);
    assert!(d.texts().iter().any(|t| t == "Ø20"), "{:?}", d.texts());
    d.click(&mut wb, place);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![egui::Event::Text("24".into())]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    let sketch = wb.document().sketch(f).unwrap().clone();
    let made: Vec<Constraint> = sketch.constraints().filter(|(_, c)| c.is_dimensional()).map(|(_, c)| c.clone()).collect();
    assert!(matches!(made.as_slice(), [Constraint::LinearDiameter { axis: a, value, .. }] if *a == axis && (*value - 24.0).abs() < 1e-9), "{made:?}");
    let top = sketch.entities().filter_map(|(id, _)| sketch.line(id)).map(|(a, b)| a.y.max(b.y)).fold(0.0, f64::max);
    assert!((top - 12.0).abs() < 1e-6, "{top}");
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "Ø24"), "the diameter is drawn as one: {:?}", d.texts());

    // Revolve starts with the centre line as its axis: a pin 24 across and 30 long.
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::R);
    d.settle(&mut wb);
    let Some(Panel::Revolve(p)) = &wb.panel else { panic!("no Revolve panel: {}", wb.status()) };
    assert_eq!(p.axis, crate::panels::AxisChoice::Line(axis));
    assert!((volume(&wb) - PI * 144.0 * 30.0).abs() < 1e-6 * PI * 144.0 * 30.0, "{}", volume(&wb));
    let _ = Vec2::ZERO;
}

#[test]
fn a_constraint_symbol_is_pointed_at_selected_and_deleted() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    let marks = |wb: &Workbench| match &wb.mode {
        Mode::Sketch(sm) => sm.badges.clone(),
        Mode::Model => Vec::new(),
    };
    let picked = |wb: &Workbench| match &wb.mode {
        Mode::Sketch(sm) => (sm.constraints.clone(), sm.constraint_hover),
        Mode::Model => (Vec::new(), None),
    };
    // A symbol on each side of the rectangle. The part origin shown in the sketch is held too,
    // but that is not a constraint anyone put there: it has no symbol.
    let all = marks(&wb);
    assert_eq!(all.len(), 4, "{all:?}");
    let sketch = wb.document().sketch(f).unwrap().clone();
    assert!(sketch.constraints().any(|(_, c)| matches!(c, Constraint::Fix { .. })));
    let level: Vec<_> = all.iter().filter(|(id, _)| matches!(sketch.constraint(*id), Some(Constraint::Horizontal { .. }))).cloned().collect();
    assert_eq!(level.len(), 2);
    assert_eq!(wb.sketch_dof_text().unwrap(), "4 dimensions needed");

    // Pointed at, it and its line light up; clicked, it is selected and the status bar says
    // what it is.
    let (first, at) = (level[0].0, level[0].1.center());
    move_to(&mut d, &mut wb, at);
    assert_eq!(picked(&wb).1, Some(first));
    d.click(&mut wb, at);
    assert_eq!(picked(&wb).0, vec![first]);
    assert!(wb.status().starts_with("Selected the horizontal constraint"), "{}", wb.status());
    // Shift and a click on the other adds it; a click on empty space lets both go.
    d.modifiers(&mut wb, egui::Modifiers::SHIFT);
    d.click(&mut wb, level[1].1.center());
    d.modifiers(&mut wb, egui::Modifiers::default());
    assert_eq!(picked(&wb).0.len(), 2);
    let away = on_screen(&wb, Vec3::new(70.0, 50.0, 0.0));
    d.click(&mut wb, away);
    assert!(picked(&wb).0.is_empty());

    // Selected and Delete: the constraint is gone and the side is free to tilt; undo has it back.
    d.click(&mut wb, at);
    d.tap(&mut wb, egui::Key::Delete);
    d.frame(&mut wb, vec![]);
    assert!(wb.document().sketch(f).unwrap().constraint(first).is_none(), "{}", wb.status());
    assert_eq!(marks(&wb).len(), 3);
    assert_eq!(wb.sketch_dof_text().unwrap(), "5 dimensions needed");
    assert!(picked(&wb).0.is_empty());
    wb.run_ui("edit.undo").unwrap();
    d.frame(&mut wb, vec![]);
    assert!(wb.document().sketch(f).unwrap().constraint(first).is_some());
    assert_eq!(marks(&wb).len(), 4);

    // Two lines made parallel carry the symbol at each of them, and it is one constraint.
    let a = wb.exec("sketch.line", json!({ "sketch": f.0, "x1": 50, "y1": 0, "x2": 60, "y2": 20 })).unwrap()["line"].as_u64().unwrap();
    let b = wb.exec("sketch.line", json!({ "sketch": f.0, "x1": 70, "y1": 0, "x2": 82, "y2": 20 })).unwrap()["line"].as_u64().unwrap();
    let made = wb.exec("sketch.constrain", json!({ "sketch": f.0, "constraint": { "type": "parallel", "a": a, "b": b } })).unwrap();
    d.frame(&mut wb, vec![]);
    let parallel: Vec<_> = marks(&wb).into_iter().filter(|(id, _)| u64::from(id.0) == made["constraint"].as_u64().unwrap()).collect();
    assert_eq!(parallel.len(), 2, "{:?}", marks(&wb));
    assert!(parallel[0].1.center().distance(parallel[1].1.center()) > 40.0);
}

/// The sketch being edited.
fn open(wb: &Workbench) -> tenon_sketch::Sketch {
    wb.document().sketch(sketching(wb)).unwrap().clone()
}

fn circles(wb: &Workbench) -> Vec<(Vec2, f64)> {
    let s = open(wb);
    s.entities().filter_map(|(id, _)| s.circle(id).filter(|_| s.arc(id).is_none())).collect()
}

fn text(d: &mut Driver, wb: &mut Workbench, typed: &str) {
    d.frame(wb, vec![egui::Event::Text(typed.into())]);
}

#[test]
fn geometry_is_moved_copied_turned_and_scaled_with_the_modify_tools() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.circle", json!({ "sketch": f.0, "cx": 30, "cy": 10, "r": 3 })).unwrap();
    let line = wb.exec("sketch.line", json!({ "sketch": f.0, "x1": 50, "y1": 0, "x2": 60, "y2": 0 })).unwrap();
    d.settle(&mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));

    // With nothing selected the tools say what they need.
    for id in ["sketch.move", "sketch.copy", "sketch.rotate", "sketch.scale", "sketch.pattern.rect"] {
        assert!(wb.run_ui(id).unwrap_err().starts_with("select the geometry to "), "{id}");
    }
    // The circle selected, Move, a click on its centre and a click 10 right and 10 up.
    let rim = at(&wb, 33.0, 10.0);
    d.click(&mut wb, rim);
    wb.run_ui("sketch.move").unwrap();
    let (from, to) = (at(&wb, 30.0, 10.0), at(&wb, 40.0, 20.0));
    d.click(&mut wb, from);
    assert!(wb.status().starts_with("Click where it goes"), "{}", wb.status());
    move_to(&mut d, &mut wb, to);
    d.click(&mut wb, to);
    let c = circles(&wb)[0];
    assert!(c.0.dist(Vec2::new(40.0, 20.0)) < 0.2 && (c.1 - 3.0).abs() < 1e-9, "{c:?}");
    assert!(wb.active_tool_id().is_none() && wb.status() == "Moved.", "{}", wb.status());
    // Again by typing: 5 across, Tab, 7 up, Enter. Exactly that far.
    wb.run_ui("sketch.move").unwrap();
    let from = at(&wb, c.0.x, c.0.y);
    d.click(&mut wb, from);
    text(&mut d, &mut wb, "5");
    d.tap(&mut wb, egui::Key::Tab);
    text(&mut d, &mut wb, "7");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    let moved = circles(&wb)[0];
    assert!(moved.0.near(c.0 + Vec2::new(5.0, 7.0), 1e-9), "{moved:?} from {c:?}");

    // Copy: two more of it, one click each; Esc ends the tool.
    wb.run_ui("sketch.copy").unwrap();
    let from = at(&wb, moved.0.x, moved.0.y);
    d.click(&mut wb, from);
    for (x, y) in [(0.0, 40.0), (20.0, 40.0)] {
        let p = at(&wb, x, y);
        move_to(&mut d, &mut wb, p);
        d.click(&mut wb, p);
    }
    assert_eq!(circles(&wb).len(), 3);
    assert_eq!(wb.active_tool_id(), Some("sketch.copy"));
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::Escape);
    assert!(wb.active_tool_id().is_none());

    // Scale: the circle still selected, the point that stays is its centre, 2 typed in.
    wb.run_ui("sketch.scale").unwrap();
    d.click(&mut wb, from);
    text(&mut d, &mut wb, "2");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(circles(&wb).iter().any(|c| (c.1 - 6.0).abs() < 1e-9 && c.0.near(moved.0, 1e-6)), "{:?}", circles(&wb));
    assert_eq!(wb.status(), "Scaled.");

    // Rotate: the line selected, turned 90 degrees about its start by typing; then back by
    // three clicks (about the start, from its end, to where the end was).
    d.tap(&mut wb, egui::Key::Escape);
    let on_line = at(&wb, 55.0, 0.0);
    d.click(&mut wb, on_line);
    wb.run_ui("sketch.rotate").unwrap();
    let start = at(&wb, 50.0, 0.0);
    d.click(&mut wb, start);
    text(&mut d, &mut wb, "90");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    let end = open(&wb).point(EntityId(line["end"].as_u64().unwrap() as u32)).unwrap();
    assert!(end.near(Vec2::new(50.0, 10.0), 1e-6), "{end:?}");
    wb.run_ui("sketch.rotate").unwrap();
    d.click(&mut wb, start);
    let (up, right) = (at(&wb, 50.0, 10.0), at(&wb, 60.0, 0.0));
    d.click(&mut wb, up);
    move_to(&mut d, &mut wb, right);
    d.click(&mut wb, right);
    let end = open(&wb).point(EntityId(line["end"].as_u64().unwrap() as u32)).unwrap();
    assert!(end.dist(Vec2::new(60.0, 0.0)) < 0.2, "{end:?}");
    // Each was one step: the last undone, the line is upright again.
    wb.run_ui("edit.undo").unwrap();
    d.frame(&mut wb, vec![]);
    let end = open(&wb).point(EntityId(line["end"].as_u64().unwrap() as u32)).unwrap();
    assert!(end.near(Vec2::new(50.0, 10.0), 1e-6));
}

#[test]
fn curves_are_extended_and_split_by_a_click() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    let short = wb.exec("sketch.line", json!({ "sketch": f.0, "x1": 0, "y1": 5, "x2": 20, "y2": 5 })).unwrap();
    let wall = wb.exec("sketch.line", json!({ "sketch": f.0, "x1": 40, "y1": -10, "x2": 40, "y2": 20 })).unwrap();
    d.settle(&mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    // Extend, a click near the short line's right-hand end: it reaches the wall.
    wb.run_ui("sketch.extend").unwrap();
    let near_end = at(&wb, 18.0, 5.0);
    move_to(&mut d, &mut wb, near_end);
    d.click(&mut wb, near_end);
    let end = open(&wb).point(EntityId(short["end"].as_u64().unwrap() as u32)).unwrap();
    assert!(end.near(Vec2::new(40.0, 5.0), 1e-6), "{end:?} {}", wb.status());
    // A click on empty space says what to click.
    let nowhere = at(&wb, 10.0, 30.0);
    move_to(&mut d, &mut wb, nowhere);
    d.click(&mut wb, nowhere);
    assert!(wb.status_error && wb.status().contains("click a line or an arc"), "{}", wb.status());
    // Split, a click on the wall near where the line now meets it: the wall is two lines.
    wb.run_ui("sketch.split").unwrap();
    let lines = |wb: &Workbench| {
        let s = open(wb);
        s.entities().filter(|(id, _)| s.is_line(*id)).count()
    };
    assert_eq!(lines(&wb), 2);
    let on_wall = at(&wb, 40.0, 8.0);
    move_to(&mut d, &mut wb, on_wall);
    d.click(&mut wb, on_wall);
    assert_eq!(lines(&wb), 3, "{}", wb.status());
    let s = open(&wb);
    let (a, b) = s.line(EntityId(wall["line"].as_u64().unwrap() as u32)).unwrap();
    assert!(a.near(Vec2::new(40.0, -10.0), 1e-6) && b.near(Vec2::new(40.0, 5.0), 1e-6), "{a:?} {b:?}");
}

#[test]
fn sketch_patterns_are_set_up_in_a_window_after_a_direction_or_a_centre_is_clicked() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 60, "y2": 40 })).unwrap();
    wb.exec("sketch.circle", json!({ "sketch": f.0, "cx": 10, "cy": 10, "r": 3 })).unwrap();
    d.settle(&mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    // The hole selected, Rectangular, a click on the plate's bottom edge for the direction.
    let rim = at(&wb, 13.0, 10.0);
    d.click(&mut wb, rim);
    wb.run_ui("sketch.pattern.rect").unwrap();
    let bottom = at(&wb, 40.0, 0.0);
    move_to(&mut d, &mut wb, bottom);
    d.click(&mut wb, bottom);
    assert!(matches!(wb.panel, Some(Panel::SketchPattern(_))), "{}", wb.status());
    for _ in 0..5 {
        d.frame(&mut wb, vec![]);
    }
    // Three across, 20 apart, in two rows 20 apart: each number is typed into its box.
    for (key, value) in [("tn_skpattern_count", "3"), ("tn_skpattern_spacing", "20"), ("tn_skpattern_count2", "2"), ("tn_skpattern_spacing2", "20")] {
        d.click(&mut wb, pressable(&d, key));
        d.frame(
            &mut wb,
            vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }],
        );
        text(&mut d, &mut wb, value);
        d.tap(&mut wb, egui::Key::Enter);
        d.frame(&mut wb, vec![]);
    }
    let Some(Panel::SketchPattern(p)) = &wb.panel else { panic!("the window is still open while it is set up: {}", wb.status()) };
    assert_eq!((p.count, p.spacing, p.count2, p.spacing2), (3, 20.0, 2, 20.0));
    d.click(&mut wb, pressable(&d, "tn_skpattern_ok"));
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let mut places: Vec<(i64, i64)> = circles(&wb).iter().map(|c| (c.0.x.round() as i64, c.0.y.round() as i64)).collect();
    places.sort_unstable();
    assert_eq!(places, [(10, 10), (10, 30), (30, 10), (30, 30), (50, 10), (50, 30)]);

    // Circular: the first hole again, round the plate's middle (a point put there), six by
    // default; Cancel makes nothing.
    let mid = wb.exec("sketch.point", json!({ "sketch": f.0, "x": 30, "y": 20 })).unwrap();
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Escape);
    d.click(&mut wb, rim);
    wb.run_ui("sketch.pattern.circular").unwrap();
    let centre = at(&wb, 30.0, 20.0);
    move_to(&mut d, &mut wb, centre);
    d.click(&mut wb, centre);
    for _ in 0..5 {
        d.frame(&mut wb, vec![]);
    }
    let Some(Panel::SketchPattern(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert!(p.along.is_none() && p.count == 6 && p.center.dist(Vec2::new(30.0, 20.0)) < 0.2);
    d.click(&mut wb, pressable(&d, "tn_skpattern_cancel"));
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none() && circles(&wb).len() == 6);
    d.click(&mut wb, rim);
    wb.run_ui("sketch.pattern.circular").unwrap();
    move_to(&mut d, &mut wb, centre);
    d.click(&mut wb, centre);
    for _ in 0..5 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, pressable(&d, "tn_skpattern_ok"));
    d.frame(&mut wb, vec![]);
    assert_eq!(circles(&wb).len(), 11, "{}", wb.status());
    let _ = mid;
}

#[test]
fn a_polygon_has_as_many_sides_as_its_box_says() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    d.settle(&mut wb);
    let lines = |wb: &Workbench| wb.document().sketch(f).unwrap().entities().filter(|(_, e)| matches!(e.geometry, Geometry::Line { .. })).count();
    // Polygon: six sides unless told; the box says five, then two clicks.
    wb.run_ui("sketch.polygon").unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.status().contains("how many sides"), "{}", wb.status());
    crate::tests::type_into(&mut d, &mut wb, "tn_polygon_sides", "5");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    let (centre, corner) = (on_screen(&wb, Vec3::new(0.0, 0.0, 0.0)), on_screen(&wb, Vec3::new(20.0, 0.0, 0.0)));
    for p in [centre, corner] {
        move_to(&mut d, &mut wb, p);
        d.click(&mut wb, p);
    }
    assert_eq!(lines(&wb), 5, "{}", wb.status());
    // Two sides are too few: the box keeps five. Eight, then.
    crate::tests::type_into(&mut d, &mut wb, "tn_polygon_sides", "2");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    if let Mode::Sketch(sm) = &wb.mode {
        assert_eq!(sm.polygon_sides, 5);
    }
    crate::tests::type_into(&mut d, &mut wb, "tn_polygon_sides", "8");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    let (centre, corner) = (on_screen(&wb, Vec3::new(60.0, 0.0, 0.0)), on_screen(&wb, Vec3::new(70.0, 0.0, 0.0)));
    for p in [centre, corner] {
        move_to(&mut d, &mut wb, p);
        d.click(&mut wb, p);
    }
    assert_eq!(lines(&wb), 13, "{}", wb.status());
}

#[test]
fn slots_rectangles_and_arcs_are_drawn_by_their_own_clicks() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    d.settle(&mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    let areas = |wb: &Workbench| -> Vec<f64> {
        let info = tenon_model::cmd::sketch_info_value(&open(wb));
        info["regions"].as_array().unwrap().iter().map(|r| r["area"].as_f64().unwrap()).collect()
    };
    let draw = |d: &mut Driver, wb: &mut Workbench, tool: &str, pts: &[(f64, f64)]| {
        wb.run_ui(tool).unwrap();
        for (x, y) in pts {
            let p = at(wb, *x, *y);
            move_to(d, wb, p);
            d.click(wb, p);
        }
        d.tap(wb, egui::Key::Escape);
        d.tap(wb, egui::Key::Escape);
    };
    // A slot by the centres of its ends, 30 apart, and a third click 4 to its side: 8 wide.
    draw(&mut d, &mut wb, "sketch.slot", &[(0.0, 0.0), (30.0, 0.0), (15.0, 4.0)]);
    let a = areas(&wb);
    assert!(a.len() == 1 && (a[0] - (240.0 + 16.0 * PI)).abs() < 3.0, "{a:?} {}", wb.status());
    wb.run_ui("edit.undo").unwrap();
    // By its ends, 38 apart, the same slot; by its middle and one centre, the same again.
    draw(&mut d, &mut wb, "sketch.slot_overall", &[(-4.0, 0.0), (34.0, 0.0), (15.0, 4.0)]);
    let a = areas(&wb);
    assert!(a.len() == 1 && (a[0] - (240.0 + 16.0 * PI)).abs() < 3.0, "{a:?}");
    wb.run_ui("edit.undo").unwrap();
    draw(&mut d, &mut wb, "sketch.slot_center", &[(15.0, 0.0), (30.0, 0.0), (15.0, 4.0)]);
    let a = areas(&wb);
    assert!(a.len() == 1 && (a[0] - (240.0 + 16.0 * PI)).abs() < 3.0, "{a:?}");
    wb.run_ui("edit.undo").unwrap();

    // A rectangle round the origin: its centre is the origin point itself, so sized later it
    // grows both ways.
    draw(&mut d, &mut wb, "sketch.rectangle_center", &[(0.0, 0.0), (12.0, 5.0)]);
    let a = areas(&wb);
    assert!(a.len() == 1 && (a[0] - 240.0).abs() < 3.0, "{a:?}");
    let s = open(&wb);
    assert!(
        s.constraints().any(|(_, c)| matches!(c, Constraint::Coincident { .. }))
            && s.constraints().any(|(_, c)| matches!(c, Constraint::Midpoint { .. }))
    );
    wb.run_ui("edit.undo").unwrap();
    wb.run_ui("edit.undo").unwrap();
    // One at an angle: a side from (0, 0) to (30, 40) and 10 deep.
    draw(&mut d, &mut wb, "sketch.rectangle3", &[(40.0, 0.0), (70.0, 40.0), (32.0, 6.0)]);
    let a = areas(&wb);
    assert!(a.len() == 1 && (a[0] - 500.0).abs() < 8.0, "{a:?}");
    assert!(open(&wb).constraints().any(|(_, c)| matches!(c, Constraint::Perpendicular { .. })));
    wb.run_ui("edit.undo").unwrap();

    // An arc by its centre, start and end: the short way round, a quarter turn.
    draw(&mut d, &mut wb, "sketch.arc_center", &[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]);
    let s = open(&wb);
    let arc = s.entities().find_map(|(id, _)| s.arc(id)).expect("an arc");
    assert!((arc.radius - 10.0).abs() < 0.2 && (arc.sweep() - PI / 2.0).abs() < 0.05, "{arc:?}");
    wb.run_ui("edit.undo").unwrap();
    // A line, then an arc on from its end, tangent to it.
    let line = wb.exec("sketch.line", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 20, "y2": 0 })).unwrap();
    d.frame(&mut wb, vec![]);
    draw(&mut d, &mut wb, "sketch.arc_tangent", &[(20.0, 0.0), (30.0, 10.0)]);
    let s = open(&wb);
    let arc = s.entities().find_map(|(id, _)| s.arc(id)).expect("an arc");
    assert!((arc.radius - 10.0).abs() < 0.3 && arc.center.dist(Vec2::new(20.0, 10.0)) < 0.3, "{arc:?}");
    assert!(s.constraints().any(|(_, c)| matches!(c, Constraint::Tangent { .. })));
    assert_eq!(s.curves_at(EntityId(line["end"].as_u64().unwrap() as u32)).len(), 2, "the arc starts on the line's own end point");
    // Started off any curve's end, it says where to start.
    wb.run_ui("sketch.arc_tangent").unwrap();
    let nowhere = at(&wb, 50.0, 50.0);
    move_to(&mut d, &mut wb, nowhere);
    d.click(&mut wb, nowhere);
    assert!(wb.status_error && wb.status().contains("end point of a line or an arc"), "{}", wb.status());
}

/// The small value window (Sketch Fillet's radius, Offset's distance): what is typed takes the
/// place of the value it opens with, and Enter is OK.
#[test]
fn a_fillets_radius_and_an_offsets_distance_are_typed_and_entered() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    d.settle(&mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    let radii = |wb: &Workbench| -> Vec<f64> {
        let sk = wb.document().sketch(f).unwrap();
        let arcs = sk.entities().filter_map(|(_, e)| match &e.geometry {
            Geometry::Arc { center, start, .. } => Some((sk.point(*center)?, sk.point(*start)?)),
            _ => None,
        });
        arcs.map(|(c, s)| ((s.x - c.x).powi(2) + (s.y - c.y).powi(2)).sqrt()).collect()
    };

    // Fillet, a click on a corner: the window opens at 2 mm with the keyboard in its box.
    wb.run_ui("sketch.fillet").unwrap();
    let corner = at(&wb, 40.0, 20.0);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(corner)]);
    d.click(&mut wb, corner);
    assert!(matches!(&wb.panel, Some(Panel::Value(v)) if v.title == "Sketch Fillet" && v.value == 2.0), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![egui::Event::Text("3.5".into())]);
    assert!(matches!(&wb.panel, Some(Panel::Value(v)) if v.value == 3.5), "what is typed takes the place of the 2");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "Enter is OK: {}", wb.status());
    let made = radii(&wb);
    assert!(matches!(made.as_slice(), [r] if (r - 3.5).abs() < 1e-9), "{made:?}");

    // Offset of a selected line: a distance typed, Enter.
    d.tap(&mut wb, egui::Key::Escape);
    let lines = |wb: &Workbench| wb.document().sketch(f).unwrap().entities().filter(|(_, e)| matches!(e.geometry, Geometry::Line { .. })).count();
    let before = lines(&wb);
    let on_line = at(&wb, 15.0, 0.0);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(on_line)]);
    d.click(&mut wb, on_line);
    wb.run_ui("sketch.offset").unwrap();
    assert!(matches!(&wb.panel, Some(Panel::Value(v)) if v.title == "Offset"), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![egui::Event::Text("4".into())]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(lines(&wb), before + 1, "a line beside the one selected: {}", wb.status());
}

/// Two clicks at one place, after a pause: clicks soon after earlier ones would count as a
/// triple click.
fn double_click(d: &mut Driver, wb: &mut Workbench, at: Pos2) {
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    for _ in 0..45 {
        d.frame(wb, vec![]);
    }
    d.click(wb, at);
    d.click(wb, at);
}

#[test]
fn text_is_typed_in_its_box_changed_by_a_double_click_and_its_letters_are_a_profile() {
    use tenon_model::RegionSel;
    use tenon_sketch::RegionKey;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    d.settle(&mut wb);
    // Text, from the ribbon: a click where it starts opens the Format Text box. OK with
    // nothing typed is refused, and the box stays.
    d.click(&mut wb, pressable(&d, "ribbon sketch.text"));
    d.frame(&mut wb, vec![]);
    assert!(wb.status().contains("Format Text"), "{}", wb.status());
    let start = on_screen(&wb, Vec3::new(0.0, 25.0, 0.0));
    assert!(wb.view.rect.contains(start) && wb.view.rect.contains(on_screen(&wb, Vec3::new(30.0, 32.0, 0.0))), "the text's place is in view");
    move_to(&mut d, &mut wb, start);
    d.click(&mut wb, start);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_sketch_text_ok"));
    assert!(wb.status().contains("Type the text first"), "{}", wb.status());
    // Typed, 6 tall: placed where the click was.
    crate::tests::type_into(&mut d, &mut wb, "tn_sketch_text", "TENON");
    crate::tests::type_into(&mut d, &mut wb, "tn_text_height", "6");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_sketch_text_ok"));
    d.frame(&mut wb, vec![]);
    assert!(wb.status().contains("Placed the text"), "{}", wb.status());
    let s = open(&wb);
    let texts: Vec<EntityId> = s.texts().collect();
    let [t] = texts.as_slice() else { panic!("one text: {texts:?}") };
    let Some(Geometry::Text { at, text, height, .. }) = s.geometry(*t).cloned() else { panic!() };
    assert_eq!(text, "TENON");
    assert!((height - 6.0).abs() < 1e-9 && s.point(at).unwrap().near(Vec2::new(0.0, 25.0), 0.5), "{height} {:?}", s.point(at));
    // Esc ends the tool; a double-click in the text's box opens it again, to change.
    d.tap(&mut wb, egui::Key::Escape);
    let middle = on_screen(&wb, Vec3::new(12.0, 27.0, 0.0));
    double_click(&mut d, &mut wb, middle);
    d.frame(&mut wb, vec![]);
    crate::tests::type_into(&mut d, &mut wb, "tn_sketch_text", "TEN");
    d.click(&mut wb, pressable(&d, "tn_sketch_text_ok"));
    d.frame(&mut wb, vec![]);
    assert!(matches!(open(&wb).geometry(*t), Some(Geometry::Text { text, .. }) if text == "TEN"), "{}", wb.status());

    // Extrude: the rectangle by default. A click in the text's box adds its letters; one on the
    // rectangle takes it away, and the letters alone are left.
    d.tap(&mut wb, egui::Key::E);
    d.settle(&mut wb);
    let plate = volume(&wb);
    assert!(plate > 0.0);
    let letters = on_screen(&wb, Vec3::new(6.0, 27.0, 0.0));
    d.click(&mut wb, letters);
    d.settle(&mut wb);
    let Some(Panel::Extrude(p)) = wb.panel.clone() else { panic!("no extrude panel") };
    assert!(matches!(&p.regions, RegionSel::Keys(k) if k.contains(&RegionKey::Text { text: *t })), "{:?}", p.regions);
    let both = volume(&wb);
    assert!(both > plate, "{plate} then {both}");
    let rectangle = on_screen(&wb, Vec3::new(20.0, 10.0, 0.0));
    d.click(&mut wb, rectangle);
    d.settle(&mut wb);
    let Some(Panel::Extrude(p)) = wb.panel.clone() else { panic!("no extrude panel") };
    assert_eq!(p.regions, RegionSel::Keys(vec![RegionKey::Text { text: *t }]));
    assert!((volume(&wb) - (both - plate)).abs() < 1e-6 * both, "{} of {}", volume(&wb), both - plate);
}

#[test]
fn center_point_makes_sketch_points_and_a_hole_passes_them_over() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    block(&mut wb);
    d.settle(&mut wb);
    let sk = sketch_on_top(&mut d, &mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 10.0));
    // Point: a click makes a centre point. Format > Center Point with nothing selected, and the
    // tool makes sketch points (DEC-117).
    d.click(&mut wb, pressable(&d, "ribbon sketch.point"));
    let first = at(&wb, 10.0, 10.0);
    move_to(&mut d, &mut wb, first);
    d.click(&mut wb, first);
    d.click(&mut wb, pressable(&d, "ribbon sketch.center_point"));
    assert!(wb.status().contains("New points are sketch points"), "{}", wb.status());
    let second = at(&wb, 30.0, 10.0);
    move_to(&mut d, &mut wb, second);
    d.click(&mut wb, second);
    d.tap(&mut wb, egui::Key::Escape);
    let s = open(&wb);
    let free: Vec<EntityId> =
        s.entities().filter(|(id, e)| !e.reference && !e.construction && s.is_point(*id) && s.curves_at(*id).is_empty()).map(|(id, _)| id).collect();
    let [c, p] = free.as_slice() else { panic!("two points: {free:?}") };
    let (c, p) = (*c, *p);
    assert!(s.is_center_point(c) && !s.is_center_point(p), "{:?}", s.entity(p));
    // A centre point is drawn as a cross, a sketch point as a dot.
    let away = at(&wb, 20.0, 18.0);
    move_to(&mut d, &mut wb, away);
    let (cq, pq) = (on_screen(&wb, on_part(&wb, sk, c)), on_screen(&wb, on_part(&wb, sk, p)));
    assert_eq!((d.marks_at(cq), d.marks_at(pq)), ((2, 0), (0, 1)));
    // Selected, Center Point makes the sketch point a centre point, and again a sketch point.
    move_to(&mut d, &mut wb, pq);
    d.click(&mut wb, pq);
    d.click(&mut wb, pressable(&d, "ribbon sketch.center_point"));
    assert!(open(&wb).is_center_point(p) && wb.status().starts_with("Centre points"), "{}", wb.status());
    d.click(&mut wb, pressable(&d, "ribbon sketch.center_point"));
    assert!(!open(&wb).is_center_point(p), "{}", wb.status());
    // A hole goes at the centre point only.
    wb.finish_sketch();
    wb.run_ui("model.hole").unwrap();
    d.settle(&mut wb);
    let Some(Panel::Hole(h)) = wb.panel.clone() else { panic!("no hole panel: {}", wb.status()) };
    assert_eq!((h.sketch, h.points), (sk, vec![c]));
}

#[test]
fn arc_slots_are_drawn_by_their_clicks_about_a_centre_or_through_three_points() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    d.settle(&mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    let areas = |wb: &Workbench| -> Vec<f64> {
        let info = tenon_model::cmd::sketch_info_value(&open(wb));
        info["regions"].as_array().unwrap().iter().map(|r| r["area"].as_f64().unwrap()).collect()
    };
    let draw = |d: &mut Driver, wb: &mut Workbench, tool: &str, pts: &[(f64, f64)]| {
        wb.run_ui(tool).unwrap();
        for (x, y) in pts {
            let p = at(wb, *x, *y);
            move_to(d, wb, p);
            d.click(wb, p);
        }
        d.tap(wb, egui::Key::Escape);
        d.tap(wb, egui::Key::Escape);
    };
    let near = |a: &[f64], want: f64| a.len() == 1 && (a[0] - want).abs() < 0.02 * want;
    // Center Point Arc Slot: about the origin, from 20 along x a quarter turn round, and a
    // fourth click 24 out: 8 wide. Its centre is the origin point itself.
    draw(&mut d, &mut wb, "sketch.slot_arc_center", &[(0.0, 0.0), (20.0, 0.0), (0.0, 35.0), (0.0, 24.0)]);
    let a = areas(&wb);
    assert!(near(&a, PI / 2.0 * 20.0 * 8.0 + 16.0 * PI), "{a:?} {}", wb.status());
    assert!(open(&wb).constraints().any(|(_, c)| matches!(c, Constraint::Coincident { .. })), "held at the origin");
    wb.run_ui("edit.undo").unwrap();
    wb.run_ui("edit.undo").unwrap();
    assert!(areas(&wb).is_empty());
    // Three Point Arc Slot: its start, its end, a point on the arc (clockwise from the start
    // here: it is the arc through the three that counts), then 4 inside it: 8 wide.
    draw(&mut d, &mut wb, "sketch.slot_arc_three", &[(-20.0, 0.0), (20.0, 0.0), (0.0, 20.0), (0.0, 16.0)]);
    let a = areas(&wb);
    assert!(near(&a, PI * 20.0 * 8.0 + 16.0 * PI), "{a:?} {}", wb.status());
    // Three points in a line make none, and say why.
    wb.run_ui("edit.undo").unwrap();
    wb.run_ui("sketch.slot_arc_three").unwrap();
    for (x, y) in [(-20.0, 5.0), (20.0, 5.0), (0.0, 5.0), (0.0, 9.0)] {
        let p = at(&wb, x, y);
        move_to(&mut d, &mut wb, p);
        d.click(&mut wb, p);
    }
    assert!(areas(&wb).is_empty() && wb.status().contains("in one line"), "{}", wb.status());
}

#[test]
fn stretch_pulls_a_side_and_the_width_across_it_follows() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    let r = wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    let width =
        wb.exec("sketch.constrain", json!({ "sketch": f.0, "constraint": { "type": "length", "line": r["lines"][0], "value": 40.0 } })).unwrap();
    let width = tenon_sketch::ConstraintId(width["constraint"].as_u64().unwrap() as u32);
    d.settle(&mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    let value = |wb: &Workbench| open(wb).constraint(width).unwrap().value().unwrap();
    // With nothing selected it says what it needs (DEC-119).
    assert_eq!(wb.run_ui("sketch.stretch").unwrap_err(), "select the geometry to stretch first");
    // The right side selected, Stretch from the ribbon, a click on the side and one 10 right.
    let side = at(&wb, 40.0, 10.0);
    move_to(&mut d, &mut wb, side);
    d.click(&mut wb, side);
    d.click(&mut wb, pressable(&d, "ribbon sketch.stretch"));
    assert_eq!(wb.active_tool_id(), Some("sketch.stretch"));
    d.click(&mut wb, side);
    assert!(wb.status().starts_with("Click where it goes"), "{}", wb.status());
    let to = at(&wb, 50.0, 10.0);
    move_to(&mut d, &mut wb, to);
    // What it would make is shown first: the side moved, the top and bottom stretched to it.
    let Mode::Sketch(sm) = &wb.mode else { panic!() };
    let ghost = wb.tool_ghost(&open(&wb), sm, Vec2::new(50.0, 10.0));
    assert_eq!(ghost.len(), 3, "{ghost:?}");
    assert!(ghost.iter().flatten().all(|q| q.x < 50.2) && ghost.iter().flatten().any(|q| (q.x - 50.0).abs() < 0.2), "{ghost:?}");
    d.click(&mut wb, to);
    assert!(wb.status() == "Stretched." && wb.active_tool_id().is_none(), "{}", wb.status());
    assert!((value(&wb) - 50.0).abs() < 0.2, "{}", value(&wb));
    // Again by typing 5 for X: exactly that much longer.
    let was = value(&wb);
    wb.run_ui("sketch.stretch").unwrap();
    let side = at(&wb, was, 10.0);
    d.click(&mut wb, side);
    text(&mut d, &mut wb, "5");
    d.tap(&mut wb, egui::Key::Tab);
    text(&mut d, &mut wb, "0");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!((value(&wb) - (was + 5.0)).abs() < 1e-9, "{} from {was}", value(&wb));
}

#[test]
fn an_ellipse_is_drawn_by_its_centre_an_axis_and_a_point_on_it() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    d.settle(&mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    // Ellipse (under Circle, DEC-120): the origin, 20 along x, then a point 10 up the other way.
    wb.run_ui("sketch.ellipse").unwrap();
    assert!(wb.status().starts_with("Click the centre, the end of one axis"), "{}", wb.status());
    for (x, y) in [(0.0, 0.0), (20.0, 0.0)] {
        let p = at(&wb, x, y);
        move_to(&mut d, &mut wb, p);
        d.click(&mut wb, p);
    }
    // What it would make follows the pointer.
    let p = at(&wb, 0.0, 10.0);
    move_to(&mut d, &mut wb, p);
    let Mode::Sketch(sm) = &wb.mode else { panic!() };
    let ghost = wb.tool_ghost(&open(&wb), sm, Vec2::new(0.0, 10.0));
    assert!(ghost.len() == 1 && ghost[0].iter().any(|q| (q.y - 10.0).abs() < 0.3), "{ghost:?}");
    d.click(&mut wb, p);
    let s = open(&wb);
    let info = tenon_model::cmd::sketch_info_value(&s);
    let areas: Vec<f64> = info["regions"].as_array().unwrap().iter().map(|r| r["area"].as_f64().unwrap()).collect();
    assert!(areas.len() == 1 && (areas[0] - PI * 200.0).abs() < 0.03 * PI * 200.0, "{areas:?} {}", wb.status());
    assert!(s.entities().any(|(_, e)| matches!(e.geometry, Geometry::Ellipse { .. })));
    assert!(s.constraints().any(|(_, c)| matches!(c, Constraint::Coincident { .. })), "held at the origin");
    // Dimension: a click on it and a place for the value sizes half its first axis; again, the
    // other.
    d.tap(&mut wb, egui::Key::Escape);
    let (id, _) = s.entities().find(|(_, e)| matches!(e.geometry, Geometry::Ellipse { .. })).unwrap();
    for (on, place, typed) in [((-20.0, 0.0), (-26.0, 14.0), "30"), ((0.0, -10.0), (12.0, -18.0), "6")] {
        // (The tool stays on after a dimension is placed.)
        if wb.active_tool_id() != Some("sketch.dimension") {
            d.tap(&mut wb, egui::Key::D);
        }
        for (x, y) in [on, place] {
            let p = at(&wb, x, y);
            move_to(&mut d, &mut wb, p);
            d.click(&mut wb, p);
        }
        assert!(matches!(wb.panel, Some(Panel::Value(_))), "the value box is open: {}", wb.status());
        d.frame(&mut wb, vec![]);
        d.frame(&mut wb, vec![egui::Event::Text(typed.into())]);
        d.tap(&mut wb, egui::Key::Enter);
        d.frame(&mut wb, vec![]);
    }
    let (_, major, minor) = open(&wb).ellipse(id).unwrap();
    assert!((major.len() - 30.0).abs() < 1e-7 && (minor - 6.0).abs() < 1e-7, "{major:?} {minor}");
    let s = open(&wb);
    // Extruded (10 deep, as Extrude starts), it is pi a b times that.
    d.tap(&mut wb, egui::Key::Escape);
    d.tap(&mut wb, egui::Key::E);
    d.settle(&mut wb);
    let (id, _) = s.entities().find(|(_, e)| matches!(e.geometry, Geometry::Ellipse { .. })).unwrap();
    let (_, major, minor) = s.ellipse(id).unwrap();
    let exact = PI * major.len() * minor * 10.0;
    assert!((volume(&wb) - exact).abs() < 1e-6 * exact, "{} vs {exact}", volume(&wb));
}

#[test]
fn smooth_is_set_by_clicking_a_spline_and_the_line_it_ends_on() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    let line = wb.exec("sketch.line", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 20, "y2": 0 })).unwrap();
    let end = tenon_sketch::EntityId(line["end"].as_u64().unwrap() as u32);
    d.settle(&mut wb);
    let at = |wb: &Workbench, p: Vec2| on_screen(wb, Vec3::new(p.x, p.y, 0.0));
    // A spline drawn on from the line's end: its first control point is that end itself.
    wb.run_ui("sketch.spline").unwrap();
    for p in [Vec2::new(20.0, 0.0), Vec2::new(28.0, 6.0), Vec2::new(36.0, 12.0), Vec2::new(46.0, 12.0)] {
        let q = at(&wb, p);
        move_to(&mut d, &mut wb, q);
        d.click(&mut wb, q);
    }
    d.tap(&mut wb, egui::Key::Enter);
    d.tap(&mut wb, egui::Key::Escape);
    let s = open(&wb);
    let (spline, poles) = s
        .entities()
        .find_map(|(id, e)| match &e.geometry {
            Geometry::Spline { poles, .. } => Some((id, poles.clone())),
            _ => None,
        })
        .expect("the spline");
    assert_eq!(poles[0], end, "it starts on the line's end");
    // Smooth (G2) from the Constrain panel, then the spline and the line (DEC-121).
    d.click(&mut wb, pressable(&d, "ribbon sketch.smooth"));
    assert_eq!(wb.active_tool_id(), Some("sketch.smooth"));
    let sp = open(&wb).spline(spline).unwrap();
    let (lo, hi) = sp.domain();
    for p in [sp.eval((lo + hi) / 2.0), Vec2::new(8.0, 0.0)] {
        let q = at(&wb, p);
        move_to(&mut d, &mut wb, q);
        d.click(&mut wb, q);
    }
    assert_eq!(wb.status(), "Added smooth.");
    let s = open(&wb);
    assert!(s.constraints().any(|(_, c)| matches!(c, Constraint::Smooth { .. })));
    // It leaves the line straight on, unbent: its next two control points are in line with it.
    let (p, q) = s.line(tenon_sketch::EntityId(line["line"].as_u64().unwrap() as u32)).unwrap();
    let off = |i: usize| (q - p).normalized().cross(s.point(poles[i]).unwrap() - p);
    assert!(off(1).abs() < 1e-7 && off(2).abs() < 1e-7, "{} {}", off(1), off(2));
}

#[test]
fn a_fillet_is_made_variable_or_between_two_sets_of_faces_by_clicks() {
    use tenon_model::{FeatureKind, RadiusAt};
    let fillet = |wb: &Workbench| {
        wb.document()
            .features()
            .iter()
            .find_map(|f| match &f.kind {
                FeatureKind::Fillet(x) => Some(x.clone()),
                _ => None,
            })
            .expect("a fillet")
    };
    // Variable (DEC-122): F, the top front edge, Variable, 2 at the start and 4 at the end.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    block(&mut wb);
    d.settle(&mut wb);
    d.tap(&mut wb, egui::Key::F);
    let edge = on_screen(&wb, Vec3::new(20.0, 0.0, 10.0));
    move_to(&mut d, &mut wb, edge);
    d.click(&mut wb, edge);
    assert!(matches!(&wb.panel, Some(Panel::Fillet(p)) if p.edges.len() == 1), "{}", wb.status());
    d.frame(&mut wb, vec![]);
    d.click(&mut wb, pressable(&d, "tn_fillet_variable"));
    crate::tests::type_into(&mut d, &mut wb, "tn_props_radius", "2");
    crate::tests::type_into(&mut d, &mut wb, "tn_props_end_radius", "4");
    // Each end of the edge is marked in the view with its radius.
    d.frame(&mut wb, vec![]);
    let texts = d.texts();
    assert!(texts.iter().any(|t| t.starts_with("Start 2")) && texts.iter().any(|t| t.starts_with("End 4")), "{texts:?}");
    d.tap(&mut wb, egui::Key::Enter);
    d.settle(&mut wb);
    let f = fillet(&wb);
    assert_eq!((f.radius, f.radii.clone()), (2.0, vec![RadiusAt { at: 1.0, radius: 4.0 }]), "{}", wb.status());
    let even = (1.0 - PI / 4.0) * 40.0 * (4.0 + 8.0 + 16.0) / 3.0;
    assert!(((8000.0 - volume(&wb)) - even).abs() < 0.02 * even, "{} ({})", volume(&wb), wb.status());

    // Face Fillet: the top, which hands on to the second set, then the front; 1 round.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    block(&mut wb);
    d.settle(&mut wb);
    wb.run_ui("model.face_fillet").unwrap();
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t == "Face Set 1") && d.texts().iter().any(|t| t == "Face Set 2"), "{:?}", d.texts());
    for p in [Vec3::new(20.0, 10.0, 10.0), Vec3::new(20.0, 0.0, 4.0)] {
        let q = on_screen(&wb, p);
        move_to(&mut d, &mut wb, q);
        d.click(&mut wb, q);
    }
    assert!(matches!(&wb.panel, Some(Panel::Fillet(p)) if p.faces.as_ref().is_some_and(|s| s[0].len() == 1 && s[1].len() == 1)), "{}", wb.status());
    crate::tests::type_into(&mut d, &mut wb, "tn_props_radius", "1");
    d.tap(&mut wb, egui::Key::Enter);
    d.settle(&mut wb);
    let f = fillet(&wb);
    assert!(f.faces.is_some() && f.edges.is_empty(), "{f:?}");
    let round = (1.0 - PI / 4.0) * 40.0;
    assert!((8000.0 - volume(&wb) - round).abs() < 1e-6 * 8000.0, "{} ({})", volume(&wb), wb.status());
}
