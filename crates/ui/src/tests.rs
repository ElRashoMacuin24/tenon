#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use egui::{Pos2, Rect, vec2};
use serde_json::json;
use tenon_geom::Vec2;
use tenon_kernel_occt::OcctKernel;
use tenon_model::{CapEnd, FaceOrigin, FeatureId};

use crate::Workbench;
use crate::chrome::available_ids;
use crate::panels::Panel;
use crate::sketcher::Click;
use crate::viewport::Pick;
use crate::workbench::Mode;

fn frame(wb: &mut Workbench, ctx: &egui::Context, size: egui::Vec2) {
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), ..Default::default() };
    ctx.run_ui(input, |ui| wb.ui(ui, None)).drop_without_applying_deltas();
}

fn sketching(wb: &Workbench) -> FeatureId {
    match &wb.mode {
        Mode::Sketch(s) => s.feature,
        Mode::Model => panic!("not in sketch mode"),
    }
}

fn click(wb: &mut Workbench, x: f64, y: f64) {
    let f = sketching(wb);
    let sketch = wb.document().sketch(f).unwrap().clone();
    wb.sketch_click(f, &sketch, Click { at: Vec2::new(x, y), point: None }, None, false);
}

fn volume(wb: &Workbench) -> f64 {
    wb.scene().bodies.iter().map(|b| b.volume).sum()
}

#[test]
fn renders_every_tab_and_size_without_a_kernel() {
    let ctx = egui::Context::default();
    let mut wb = Workbench::without_kernel();
    for i in 0..crate::commands::RIBBON.len() {
        wb.chrome.tab = i;
        frame(&mut wb, &ctx, vec2(1400.0, 860.0));
    }
    wb.run_ui("view.browser").unwrap();
    wb.run_ui("app.about").unwrap();
    for size in [vec2(1.0, 1.0), vec2(0.0, 0.0), vec2(5000.0, 3000.0)] {
        frame(&mut wb, &ctx, size);
    }
}

#[test]
fn every_available_command_has_a_handler() {
    for id in available_ids() {
        let mut wb = Workbench::without_kernel();
        if id.starts_with("sketch.") && id != "sketch.new" {
            wb.create_sketch(json!({ "plane": "xy" })).unwrap();
            let f = sketching(&wb);
            wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 10, "y2": 5 })).unwrap();
            if let Mode::Sketch(s) = &mut wb.mode {
                s.selection = vec![tenon_sketch::EntityId(5)];
            }
        }
        if let Err(e) = wb.run_ui(id) {
            assert!(!e.contains("unknown command"), "{id}: {e}");
        }
    }
    assert!(Workbench::without_kernel().run_ui("model.fillet").unwrap_err().contains("M2"));
    assert!(Workbench::without_kernel().run_ui("nonsense.cmd").unwrap_err().contains("unknown"));
}

#[test]
fn sketch_extrude_and_sketch_on_the_top_face_through_the_ui() {
    let ctx = egui::Context::default();
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let size = vec2(1400.0, 860.0);
    frame(&mut wb, &ctx, size);

    // New Sketch with nothing selected asks for a plane; pick XY.
    wb.run_ui("sketch.new").unwrap();
    assert!(matches!(wb.panel, Some(Panel::NewSketch)));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let sk1 = sketching(&wb);

    // Rectangle by two clicks.
    wb.run_ui("sketch.rectangle").unwrap();
    click(&mut wb, 0.0, 0.0);
    click(&mut wb, 40.0, 20.0);
    assert_eq!(wb.document().sketch(sk1).unwrap().entity_count(), 8);
    frame(&mut wb, &ctx, size);
    assert_eq!(wb.sketch_dof_text().unwrap(), "4 degrees of freedom");

    // Extrude: the panel previews, OK commits.
    wb.run_ui("model.extrude").unwrap();
    assert!(!wb.is_sketching(), "extrude finishes the sketch");
    frame(&mut wb, &ctx, size);
    assert!((volume(&wb) - 8000.0).abs() < 1e-6, "preview shows the 10 mm default: {}", volume(&wb));
    let Some(Panel::Extrude(p)) = wb.panel.clone() else { panic!("no extrude panel") };
    assert!(wb.commit_feature(p.editing, p.kind()));
    wb.panel = None;
    frame(&mut wb, &ctx, size);
    assert_eq!(wb.document().features().len(), 2);
    assert!((volume(&wb) - 8000.0).abs() < 1e-6);

    // Select the top face, New Sketch goes straight onto it.
    let top = wb.scene().bodies[0].faces.iter().position(|(n, _)| matches!(n, Some(FaceOrigin::Cap { end: CapEnd::End, .. }))).unwrap() as u32;
    wb.view.selection = vec![Pick::Face { body: 0, face: top }];
    wb.run_ui("sketch.new").unwrap();
    let sk2 = sketching(&wb);
    assert_ne!(sk1, sk2);
    frame(&mut wb, &ctx, size);
    let frame2 = wb.sketch_frame(sk2).unwrap();
    assert!((frame2.origin().z - 10.0).abs() < 1e-9, "the sketch sits on the top face");

    // A hole: circle, then cut through all.
    wb.run_ui("sketch.circle").unwrap();
    click(&mut wb, 20.0, 10.0);
    click(&mut wb, 23.0, 10.0);
    wb.finish_sketch();
    wb.open_extrude(None).unwrap();
    if let Some(Panel::Extrude(p)) = &mut wb.panel {
        p.sketch = sk2;
        p.extent = crate::panels::ExtentChoice::ThroughAll;
        p.reverse = true;
        p.operation = tenon_model::Operation::Cut;
    }
    let Some(Panel::Extrude(p)) = wb.panel.clone() else { panic!() };
    assert!(wb.commit_feature(p.editing, p.kind()));
    wb.panel = None;
    frame(&mut wb, &ctx, size);
    assert!((volume(&wb) - (8000.0 - PI * 9.0 * 10.0)).abs() < 1e-6, "{}", volume(&wb));
    assert!(wb.status().is_empty() || !wb.status_error, "{}", wb.status());

    // Undo removes the cut; the view keeps working.
    wb.run_ui("edit.undo").unwrap();
    frame(&mut wb, &ctx, size);
    assert!((volume(&wb) - 8000.0).abs() < 1e-6);
    for id in ["view.home", "view.fit", "view.orbit", "view.orbit", "inspect.mass", "view.cube"] {
        wb.run_ui(id).unwrap();
        frame(&mut wb, &ctx, size);
    }
}

#[test]
fn dimension_tool_creates_a_driving_dimension() {
    let ctx = egui::Context::default();
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    let lines = wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 30, "y2": 10 })).unwrap();
    let bottom = tenon_sketch::EntityId(lines["lines"][0].as_u64().unwrap() as u32);
    wb.run_ui("sketch.dimension").unwrap();
    let sketch = wb.document().sketch(f).unwrap().clone();
    // Click the line twice: its length.
    wb.dimension_click(f, &sketch, Some(bottom));
    wb.dimension_click(f, &sketch, Some(bottom));
    let Some(Panel::Value(v)) = wb.panel.clone() else { panic!("no value panel") };
    assert!((v.value - 30.0).abs() < 1e-9);
    let mut v2 = v;
    v2.value = 45.0;
    wb.panel = Some(Panel::Value(v2));
    if let Some(Panel::Value(v)) = wb.panel.take()
        && let crate::panels::ValueFor::Dimension { sketch, constraint } = v.what
    {
        let mut c = constraint;
        c.set_value(v.value);
        wb.exec("sketch.constrain", json!({ "sketch": sketch.0, "constraint": serde_json::to_value(&c).unwrap() })).unwrap();
    }
    let (a, b) = wb.document().sketch(f).unwrap().line(bottom).unwrap();
    assert!((a.dist(b) - 45.0).abs() < 1e-7);
    frame(&mut wb, &ctx, vec2(1200.0, 800.0));
}
