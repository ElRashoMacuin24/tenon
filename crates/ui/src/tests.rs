#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use egui::{Pos2, Rect, pos2, vec2};
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
    wb.sketch_click(f, &sketch, Click { at: Vec2::new(x, y), point: None, infer: None }, None, false);
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
    assert!(Workbench::without_kernel().run_ui("model.rib").unwrap_err().contains("M2"));
    assert!(Workbench::without_kernel().run_ui("model.fillet").unwrap_err().contains("no solid"));
    assert!(Workbench::without_kernel().run_ui("nonsense.cmd").unwrap_err().contains("unknown"));
}

#[test]
fn sketch_extrude_and_sketch_on_the_top_face_through_the_ui() {
    let ctx = egui::Context::default();
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let size = vec2(1400.0, 860.0);
    frame(&mut wb, &ctx, size);

    // Start 2D Sketch with nothing selected waits for a plane; pick XY.
    wb.run_ui("sketch.new").unwrap();
    assert!(wb.pick_plane, "waiting for a plane");
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let sk1 = sketching(&wb);

    // Rectangle by two clicks (plus the projected origin point).
    wb.run_ui("sketch.rectangle").unwrap();
    click(&mut wb, 0.0, 0.0);
    click(&mut wb, 40.0, 20.0);
    assert_eq!(wb.document().sketch(sk1).unwrap().entity_count(), 9);
    frame(&mut wb, &ctx, size);
    assert_eq!(wb.sketch_dof_text().unwrap(), "4 dimensions needed");

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
        p.direction = crate::panels::Direction::Flipped;
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

/// Feeds raw pointer events through egui, frame by frame, as the windowing layer would.
struct Driver {
    ctx: egui::Context,
    time: f64,
    size: egui::Vec2,
}

impl Driver {
    fn new(size: egui::Vec2) -> Driver {
        Driver { ctx: egui::Context::default(), time: 0.0, size }
    }
    fn frame(&mut self, wb: &mut Workbench, events: Vec<egui::Event>) {
        self.time += 1.0 / 60.0;
        let input =
            egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)), time: Some(self.time), events, ..Default::default() };
        self.ctx.run_ui(input, |ui| wb.ui(ui, None)).drop_without_applying_deltas();
    }
    /// Holds (or releases) modifier keys from the next frame on.
    fn modifiers(&mut self, wb: &mut Workbench, m: egui::Modifiers) {
        self.frame(wb, vec![egui::Event::ModifiersChanged(m)]);
    }
    /// Runs frames until view transitions have finished.
    fn settle(&mut self, wb: &mut Workbench) {
        for _ in 0..120 {
            self.frame(wb, vec![]);
            if wb.view.anim.is_none() {
                return;
            }
        }
        panic!("the view never settled");
    }
    /// Holds a key down (`true`) or lets it go.
    fn key(&mut self, wb: &mut Workbench, key: egui::Key, pressed: bool) {
        self.frame(wb, vec![egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::default() }]);
    }
    /// Presses and releases a key (a second press without a release counts as a repeat).
    fn tap(&mut self, wb: &mut Workbench, key: egui::Key) {
        let ev = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::default() };
        self.frame(wb, vec![ev(true), ev(false)]);
    }
    fn button(pos: Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::default() }
    }
    fn click(&mut self, wb: &mut Workbench, pos: Pos2) {
        self.frame(wb, vec![egui::Event::PointerMoved(pos)]);
        self.frame(wb, vec![Self::button(pos, egui::PointerButton::Primary, true)]);
        self.frame(wb, vec![Self::button(pos, egui::PointerButton::Primary, false)]);
    }
    fn drag(&mut self, wb: &mut Workbench, from: Pos2, to: Pos2, button: egui::PointerButton) {
        self.frame(wb, vec![egui::Event::PointerMoved(from)]);
        self.frame(wb, vec![Self::button(from, button, true)]);
        for k in 1..=10 {
            self.frame(wb, vec![egui::Event::PointerMoved(from + (to - from) * (k as f32 / 10.0))]);
        }
        self.frame(wb, vec![Self::button(to, button, false)]);
    }
}

/// Screen position of a model point in the last viewport.
fn on_screen(wb: &Workbench, p: tenon_geom::Vec3) -> Pos2 {
    let r = wb.view.rect;
    let (x, y, _) = wb.view.camera.project(p, f64::from(r.width()), f64::from(r.height())).unwrap();
    r.min + vec2(x as f32, y as f32)
}

#[test]
fn viewport_responds_to_real_pointer_input() {
    use tenon_geom::Vec3;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    let mut d = Driver::new(vec2(1400.0, 860.0));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.scene().bodies.len(), 1);
    let rect = wb.view.rect;
    let is_top = |wb: &Workbench| match wb.view.selection.as_slice() {
        [Pick::Face { body, face }] => matches!(wb.scene().bodies[*body].faces[*face as usize].0, Some(FaceOrigin::Cap { end: CapEnd::End, .. })),
        _ => false,
    };

    d.settle(&mut wb);
    wb.run_ui("view.home").unwrap();
    d.settle(&mut wb);

    // A click on the top face selects it and names it in the status bar.
    let top_centre = on_screen(&wb, Vec3::new(20.0, 10.0, 10.0));
    d.click(&mut wb, top_centre);
    assert!(is_top(&wb), "{:?}", wb.view.selection);
    assert!(wb.status().contains("end face of Extrusion1"), "{}", wb.status());
    // A click on empty background clears it.
    let empty = rect.left_top() + vec2(40.0, rect.height() / 2.0);
    d.click(&mut wb, empty);
    assert!(wb.view.selection.is_empty());

    // Selection boxes: left to right takes what is fully inside, right to left what it touches.
    let (lo, hi) = (on_screen(&wb, Vec3::new(0.0, 0.0, 0.0)), on_screen(&wb, Vec3::new(40.0, 20.0, 10.0)));
    let (all_a, all_b) = (pos2(rect.left() + 20.0, rect.top() + 20.0), pos2(rect.right() - 200.0, rect.bottom() - 20.0));
    d.drag(&mut wb, all_a, all_b, egui::PointerButton::Primary);
    let faces = |wb: &Workbench| wb.view.selection.iter().filter(|p| matches!(p, Pick::Face { .. })).count();
    assert_eq!(faces(&wb), 6, "a window around the block takes all six faces");
    let mid = lo + (hi - lo) * 0.5;
    d.drag(&mut wb, mid + vec2(4.0, -4.0), mid - vec2(4.0, -4.0), egui::PointerButton::Primary);
    assert!(faces(&wb) >= 1, "a small crossing box takes the face it touches: {:?}", wb.view.selection);
    d.drag(&mut wb, mid - vec2(4.0, -4.0), mid + vec2(4.0, -4.0), egui::PointerButton::Primary);
    assert_eq!(faces(&wb), 0, "the same box as a window holds no whole face");
    d.click(&mut wb, empty);

    // No left-drag orbit in select mode (that was a box); F4 + left drag and Shift + middle drag
    // orbit, middle drag pans, the wheel zooms.
    let c = rect.center();
    let yaw = wb.view.camera.yaw;
    d.key(&mut wb, egui::Key::F4, true);
    d.drag(&mut wb, c, c + vec2(120.0, 0.0), egui::PointerButton::Primary);
    d.key(&mut wb, egui::Key::F4, false);
    assert!((wb.view.camera.yaw - yaw).abs() > 0.1, "F4 + left drag orbits");
    let pitch = wb.view.camera.pitch;
    d.modifiers(&mut wb, egui::Modifiers::SHIFT);
    d.drag(&mut wb, c, c + vec2(0.0, -80.0), egui::PointerButton::Middle);
    d.modifiers(&mut wb, egui::Modifiers::default());
    assert!(wb.view.camera.pitch < pitch - 0.3, "Shift + middle drag orbits: pitch {pitch} -> {}", wb.view.camera.pitch);
    let target = wb.view.camera.target;
    d.drag(&mut wb, c, c + vec2(100.0, 40.0), egui::PointerButton::Middle);
    assert!(wb.view.camera.target.dist(target) > 1.0, "middle drag pans");
    let distance = wb.view.camera.distance;
    d.frame(&mut wb, vec![egui::Event::PointerMoved(c)]);
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 120.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::default(),
    };
    d.frame(&mut wb, vec![wheel]);
    for _ in 0..40 {
        d.frame(&mut wb, vec![]);
    }
    assert!(wb.view.camera.distance < distance * 0.95, "wheel up zooms in: {distance} -> {}", wb.view.camera.distance);

    // F6 goes home, F5 goes back to where we were.
    let before_home = wb.view.camera;
    d.tap(&mut wb, egui::Key::F6);
    d.settle(&mut wb);
    assert!((wb.view.camera.yaw - wb.view.home.0).abs() < 1e-9);
    d.tap(&mut wb, egui::Key::F5);
    d.settle(&mut wb);
    assert!(wb.view.camera.target.near(before_home.target, 1e-9) && (wb.view.camera.yaw - before_home.yaw).abs() < 1e-9, "previous view");

    // Right click opens the radial menu; its Home slot works like F6.
    wb.run_ui("view.fit").unwrap();
    d.settle(&mut wb);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(c)]);
    d.frame(&mut wb, vec![Driver::button(c, egui::PointerButton::Secondary, true)]);
    d.frame(&mut wb, vec![Driver::button(c, egui::PointerButton::Secondary, false)]);
    let menu = wb.chrome.radial.clone().expect("radial menu open");
    assert_eq!(menu.slots[2].as_ref().map(|e| e.id), Some("model.extrude"), "Extrude to the east");
    // A flick south-west picks Home without opening the menu.
    wb.chrome.radial = None;
    d.drag(&mut wb, c, c + vec2(-70.0, 70.0), egui::PointerButton::Secondary);
    assert!(wb.chrome.radial.is_none(), "a flick closes the menu");
    d.settle(&mut wb);
    assert!((wb.view.camera.yaw - wb.view.home.0).abs() < 1e-9 && (wb.view.camera.pitch - wb.view.home.1).abs() < 1e-9, "flicked to Home");

    // The orientation cube: faces, edges and corners. Clicks glide the view there and do not
    // reach the model behind (the selection survives).
    let top_centre = on_screen(&wb, Vec3::new(20.0, 10.0, 10.0));
    d.click(&mut wb, top_centre);
    assert!(is_top(&wb));
    let cube_centre = pos2(rect.right() - 92.0, rect.top() + 82.0);
    for dir in [[0, 0, 1], [1, -1, 0], [1, -1, 1], [0, -1, 1]] {
        wb.run_ui("view.home").unwrap();
        d.settle(&mut wb);
        let cv = crate::cube::CubeView::new(&wb.view.camera, cube_centre, crate::cube::CUBE_SCALE);
        let at = cv.point_of(dir).unwrap();
        d.click(&mut wb, at);
        assert!(wb.view.anim.is_some(), "{dir:?}: the view glides");
        d.settle(&mut wb);
        let want = crate::cube::vec(dir).normalized();
        assert!(wb.view.camera.eye_dir().near(want, 1e-9), "{dir:?}: looking from {:?}", wb.view.camera.eye_dir());
        assert!(is_top(&wb), "the cube click left the selection alone");
    }
    // Square to the top face, the arrows turn by quarter turns: the bottom arrow brings the front.
    let cv = crate::cube::CubeView::new(&wb.view.camera, cube_centre, crate::cube::CUBE_SCALE);
    assert!(cv.face_on().is_none(), "the last view was a top-front edge");
    wb.look_from(Vec3::Z);
    d.settle(&mut wb);
    let reach = crate::cube::CUBE_SCALE + 16.0;
    d.click(&mut wb, cube_centre + vec2(0.0, reach));
    d.settle(&mut wb);
    assert!(wb.view.camera.eye_dir().near(-Vec3::Y, 1e-9), "front view: {:?}", wb.view.camera.eye_dir());
}

#[test]
fn sketching_with_real_clicks_and_drags() {
    use tenon_geom::Vec3;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    d.settle(&mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));

    // Line tool: three clicks, then a click on the first point closes the chain.
    wb.run_ui("sketch.line").unwrap();
    for (x, y) in [(0.0, 0.0), (30.0, 0.0), (30.0, 20.0), (0.0, 0.0)] {
        let p = at(&wb, x, y);
        d.click(&mut wb, p);
    }
    let sk = wb.document().sketch(f).unwrap().clone();
    assert_eq!(sk.entity_count(), 6, "three lines and three shared points");
    let info = tenon_model::cmd::sketch_info_value(&sk);
    let regions = info["regions"].as_array().unwrap();
    assert_eq!(regions.len(), 1, "the chain closed: {info}");
    assert!((regions[0]["area"].as_f64().unwrap() - 300.0).abs() < 0.5, "{}", regions[0]);

    // Escape twice: end the chain, then back to the select tool.
    let esc = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::default() };
    d.frame(&mut wb, vec![esc.clone()]);
    d.frame(&mut wb, vec![esc]);
    assert_eq!(wb.active_tool_id(), None, "select tool");

    // Drag the corner at (30, 20) quickly: a single 15 px pointer step already leaves the 9 px
    // snap radius, so the point must be picked where the button went down.
    let from = at(&wb, 30.0, 20.0);
    let to = at(&wb, 36.0, 26.0);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(from)]);
    d.frame(&mut wb, vec![Driver::button(from, egui::PointerButton::Primary, true)]);
    let steps = ((to - from).length() / 15.0).ceil().max(2.0) as usize;
    for k in 1..=steps {
        d.frame(&mut wb, vec![egui::Event::PointerMoved(from + (to - from) * (k as f32 / steps as f32))]);
    }
    d.frame(&mut wb, vec![Driver::button(to, egui::PointerButton::Primary, false)]);
    let sk = wb.document().sketch(f).unwrap();
    let moved = sk.entities().any(|(_, e)| match e.geometry {
        tenon_sketch::Geometry::Point { pos } => pos.dist(Vec2::new(36.0, 26.0)) < 0.2,
        _ => false,
    });
    assert!(moved, "the dragged corner follows the pointer: {}", tenon_model::cmd::sketch_info_value(sk));
}

#[test]
fn properties_panel_and_drag_arrow_drive_the_extrusion() {
    use tenon_geom::Vec3;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    d.settle(&mut wb);
    // E starts Extrude: the properties panel docks above the browser, the preview shows 10 mm.
    d.tap(&mut wb, egui::Key::E);
    d.settle(&mut wb);
    assert!(wb.has_properties(), "properties panel open");
    assert!((volume(&wb) - 8000.0).abs() < 1e-6);

    // Type 25 into Distance and press Enter: OK.
    let field = d.ctx.read_response(egui::Id::new("tn_props_dist")).expect("distance field").rect;
    d.click(&mut wb, field.center());
    d.frame(
        &mut wb,
        vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }],
    );
    d.frame(&mut wb, vec![egui::Event::Text("25".into())]);
    d.frame(&mut wb, vec![]);
    assert!((volume(&wb) - 20000.0).abs() < 1e-6, "the preview follows the typing: {}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(!wb.has_properties(), "Enter is OK");
    assert_eq!(wb.document().features().len(), 2);
    assert!((volume(&wb) - 20000.0).abs() < 1e-6);

    // Edit it again and drag the arrow's tip outwards.
    let ext = wb.document().features()[1].id;
    wb.edit_feature(ext).unwrap();
    d.settle(&mut wb);
    let (base, n) = wb.profile_anchor(f).unwrap();
    let a = on_screen(&wb, base);
    let tip = on_screen(&wb, base + n * 25.0);
    let out = tip + (tip - a).normalized() * 40.0;
    d.drag(&mut wb, tip, out, egui::PointerButton::Primary);
    let Some(Panel::Extrude(p)) = wb.panel.clone() else { panic!("still editing") };
    assert!(p.distance > 26.0, "dragging the arrow lengthens the extrusion: {}", p.distance);
    wb.panel_request = Some(crate::panels::PanelRequest::Ok);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(volume(&wb) > 40.0 * 20.0 * 26.0, "{}", volume(&wb));
    let _ = Vec3::ZERO;
}

/// Selects all of a value field and types a new value (Enter is left to the caller).
fn type_into(d: &mut Driver, wb: &mut Workbench, field: &str, text: &str) {
    // Grids size rows from the previous frame: let the layout settle before reading where the
    // field is.
    d.frame(wb, vec![]);
    d.frame(wb, vec![]);
    let r = d.ctx.read_response(egui::Id::new(field)).unwrap_or_else(|| panic!("no field {field}")).rect;
    d.click(wb, r.center());
    d.frame(wb, vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }]);
    d.frame(wb, vec![egui::Event::Text(text.into())]);
}

#[test]
fn fillet_chamfer_and_shell_pick_edges_and_faces_in_the_viewport() {
    use tenon_geom::Vec3;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    d.frame(&mut wb, vec![]);
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    let edges = |wb: &Workbench| match &wb.panel {
        Some(Panel::Fillet(p)) => p.edges.len(),
        Some(Panel::Chamfer(p)) => p.edges.len(),
        _ => panic!("no fillet or chamfer panel"),
    };

    // F starts Fillet with nothing selected; clicks on edges add them, a second click removes.
    d.tap(&mut wb, egui::Key::F);
    assert!(wb.has_properties() && edges(&wb) == 0, "fillet panel open");
    let (front, back) = (on_screen(&wb, Vec3::new(20.0, 0.0, 10.0)), on_screen(&wb, Vec3::new(20.0, 20.0, 10.0)));
    d.click(&mut wb, front);
    d.click(&mut wb, back);
    assert_eq!(edges(&wb), 2);
    assert_eq!(wb.panel_picks().len(), 2, "both edges are highlighted");
    d.click(&mut wb, front);
    assert_eq!(edges(&wb), 1, "a second click removes the edge");
    // Avoid a double click (which would read as a triple click).
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, front);
    assert_eq!(edges(&wb), 2);
    // Faces are not taken by Fillet.
    let top = on_screen(&wb, Vec3::new(20.0, 10.0, 10.0));
    d.click(&mut wb, top);
    assert_eq!(edges(&wb), 2);

    // Radius 2 in the properties panel, Enter is OK.
    type_into(&mut d, &mut wb, "tn_props_radius", "2");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(!wb.has_properties(), "{}", wb.status());
    let rounded = 8000.0 - 2.0 * (1.0 - PI / 4.0) * 4.0 * 40.0;
    assert!((volume(&wb) - rounded).abs() < 1e-6, "{}", volume(&wb));
    let fillet = wb.document().features().last().unwrap().id;

    // Editing shows the part rolled back to before the fillet, with its edges highlighted; Esc
    // leaves it as it was.
    wb.edit_feature(fillet).unwrap();
    d.frame(&mut wb, vec![]);
    assert!((volume(&wb) - 8000.0).abs() < 1e-6, "rolled back: {}", volume(&wb));
    assert_eq!(wb.panel_picks().len(), 2);
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none() && (volume(&wb) - rounded).abs() < 1e-6);

    // Chamfer takes the edge selected beforehand.
    let bottom_right = on_screen(&wb, Vec3::new(40.0, 10.0, 0.0));
    d.click(&mut wb, bottom_right);
    assert!(matches!(wb.view.selection.as_slice(), [Pick::Edge { .. }]), "{:?}", wb.view.selection);
    wb.run_ui("model.chamfer").unwrap();
    assert_eq!(edges(&wb), 1);
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    let chamfered = rounded - 0.5 * 1.0 * 1.0 * 20.0;
    assert!((volume(&wb) - chamfered).abs() < 1e-6, "{}", volume(&wb));

    // Shell, opening the right-hand face, 1 mm thick.
    wb.run_ui("model.shell").unwrap();
    let right = on_screen(&wb, Vec3::new(40.0, 10.0, 5.0));
    d.click(&mut wb, right);
    let Some(Panel::Shell(p)) = wb.panel.clone() else { panic!("no shell panel") };
    assert_eq!(p.faces.len(), 1);
    d.frame(&mut wb, vec![]);
    type_into(&mut d, &mut wb, "tn_props_thickness", "1");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!(volume(&wb) > 0.0 && volume(&wb) < chamfered * 0.4, "hollow: {}", volume(&wb));
    assert_eq!(wb.document().features().len(), 5);
    assert!(!wb.status_error, "{}", wb.status());
}

#[test]
fn hole_takes_sketch_points_and_toggles_them_in_the_viewport() {
    use tenon_geom::{Vec2, Vec3};
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    d.frame(&mut wb, vec![]);
    // Three centre points on the top face.
    let top = wb.scene().bodies[0].faces.iter().position(|(n, _)| matches!(n, Some(FaceOrigin::Cap { end: CapEnd::End, .. }))).unwrap() as u32;
    wb.view.selection = vec![Pick::Face { body: 0, face: top }];
    wb.run_ui("sketch.new").unwrap();
    let sk = sketching(&wb);
    for (x, y) in [(10.0, 10.0), (30.0, 10.0), (20.0, 5.0)] {
        wb.exec("sketch.point", json!({ "sketch": sk.0, "x": x, "y": y })).unwrap();
    }
    wb.finish_sketch();
    d.settle(&mut wb);
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);

    // H starts Hole on every centre point; the preview drills them (12 deep goes through 10).
    d.tap(&mut wb, egui::Key::H);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Hole(p)) = wb.panel.clone() else { panic!("no hole panel: {}", wb.status()) };
    assert_eq!((p.sketch, p.points.len(), p.reverse), (sk, 3, false), "into the part, from the top face");
    let through = PI * 9.0 * 10.0;
    assert!((volume(&wb) - (8000.0 - 3.0 * through)).abs() < 1e-6, "preview: {}", volume(&wb));

    // A click on a centre's marker takes it out.
    let frame = wb.sketch_frame(sk).unwrap();
    let third = on_screen(&wb, frame.plane_point(Vec2::new(20.0, 5.0)));
    d.click(&mut wb, third);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Hole(p)) = wb.panel.clone() else { panic!() };
    assert_eq!(p.points.len(), 2);
    assert!((volume(&wb) - (8000.0 - 2.0 * through)).abs() < 1e-6, "{}", volume(&wb));

    // Counterbored, through all, 4 mm: typed diameter, Enter is OK.
    if let Some(Panel::Hole(p)) = &mut wb.panel {
        p.seat = crate::panels::Seat::Counterbore;
        p.through = true;
    }
    d.frame(&mut wb, vec![]);
    type_into(&mut d, &mut wb, "tn_props_hole_dia", "4");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let bore = PI * 5.4 * 5.4 * 3.0 + PI * 4.0 * 7.0;
    assert!((volume(&wb) - (8000.0 - 2.0 * bore)).abs() < 1e-6, "{} vs {}", volume(&wb), 8000.0 - 2.0 * bore);
    let hole = wb.document().features().last().unwrap();
    assert_eq!(hole.name, "Hole1");

    // Editing brings the same settings back.
    let id = hole.id;
    wb.edit_feature(id).unwrap();
    let Some(Panel::Hole(p)) = wb.panel.clone() else { panic!() };
    assert_eq!((p.points.len(), p.seat, p.through, p.diameter), (2, crate::panels::Seat::Counterbore, true, 4.0));
    d.tap(&mut wb, egui::Key::Escape);
    assert!(!wb.status_error, "{}", wb.status());
}

#[test]
fn pattern_and_mirror_pick_features_in_the_viewport() {
    use tenon_geom::Vec3;
    use tenon_model::{DirectionRef, OriginAxis};
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    // A 60 x 40 x 10 block centred on the origin with a 3 mm boss, 5 tall, at (10, 10).
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": -30, "y1": -20, "x2": 30, "y2": 20 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let b = sketching(&wb);
    wb.exec("sketch.circle", json!({ "sketch": b.0, "cx": 10, "cy": 10, "r": 3 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": b.0, "distance": 15 })).unwrap();
    let boss_vol = PI * 9.0 * 5.0;
    d.frame(&mut wb, vec![]);
    assert!((volume(&wb) - (24000.0 + boss_vol)).abs() < 1e-6, "{}", volume(&wb));
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);

    // Rectangular Pattern: a click on the boss picks its feature; the preview shows 2 along X.
    wb.run_ui("model.pattern.rect").unwrap();
    let boss_top = on_screen(&wb, Vec3::new(10.0, 10.0, 15.0));
    d.click(&mut wb, boss_top);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Pattern(p)) = wb.panel.clone() else { panic!("no pattern panel") };
    assert_eq!(p.features.len(), 1, "{}", wb.status());
    assert!((volume(&wb) - (24000.0 + 2.0 * boss_vol)).abs() < 1e-6, "preview: {}", volume(&wb));
    // Spacing 15, and a second direction 20 towards -Y: four bosses.
    type_into(&mut d, &mut wb, "tn_props_spacing1", "15");
    if let Some(Panel::Pattern(p)) = &mut wb.panel {
        p.dir2 = Some(DirectionRef::Origin(OriginAxis::Y));
        p.reverse2 = true;
        p.count2 = 2.0;
        p.spacing2 = 20.0;
    }
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!((volume(&wb) - (24000.0 + 4.0 * boss_vol)).abs() < 1e-6, "{}", volume(&wb));
    let pattern = wb.document().features().last().unwrap().clone();
    assert_eq!(pattern.name, "Rectangular Pattern1");

    // Mirror the pattern across YZ: clicking one of its copies picks the pattern.
    wb.run_ui("model.mirror").unwrap();
    let copy_top = on_screen(&wb, Vec3::new(25.0, -10.0, 15.0));
    d.click(&mut wb, copy_top);
    let Some(Panel::Pattern(p)) = wb.panel.clone() else { panic!("no mirror panel") };
    assert_eq!(p.features, vec![pattern.id], "the copy belongs to the pattern");
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!((volume(&wb) - (24000.0 + 8.0 * boss_vol)).abs() < 1e-6, "every occurrence is mirrored, the first too: {}", volume(&wb));
    assert!(!wb.status_error, "{}", wb.status());
}

#[test]
fn work_plane_from_a_face_carries_a_sketch_and_origin_axes_come_from_the_browser() {
    use tenon_geom::Vec3;
    use tenon_model::{PlaneRef, WorkGeom};
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    d.frame(&mut wb, vec![]);
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);

    // Select the top face, Plane: an offset plane 10 above it shows at once.
    let top = on_screen(&wb, Vec3::new(20.0, 10.0, 10.0));
    d.click(&mut wb, top);
    wb.run_ui("work.plane").unwrap();
    d.frame(&mut wb, vec![]);
    let Some(Panel::Work(w)) = wb.panel.clone() else { panic!("no work panel") };
    assert!(matches!(w.a, Some(PlaneRef::Face(_))), "the selected face is the base");
    let plane_z = |wb: &Workbench| {
        wb.scene().work.iter().find_map(|(_, g)| match g {
            WorkGeom::Plane(f) => Some(f.origin().z),
            _ => None,
        })
    };
    assert_eq!(plane_z(&wb), Some(20.0), "preview");
    type_into(&mut d, &mut wb, "tn_props_work_offset", "5");
    d.frame(&mut wb, vec![]);
    assert_eq!(plane_z(&wb), Some(15.0));
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let plane = wb.document().features().last().unwrap().clone();
    assert_eq!(plane.name, "Work Plane1");

    // Start 2D Sketch, then a click on the work plane (drawn over the part) sketches on it.
    wb.run_ui("sketch.new").unwrap();
    assert!(wb.pick_plane);
    d.frame(&mut wb, vec![]);
    let on_plane = on_screen(&wb, Vec3::new(20.0, 10.0, 15.0));
    d.click(&mut wb, on_plane);
    let sk = sketching(&wb);
    assert!(
        matches!(&wb.document().feature(sk).unwrap().kind, tenon_model::FeatureKind::Sketch { plane: PlaneRef::Work(id), .. } if *id == plane.id)
    );
    d.settle(&mut wb);
    assert!((wb.sketch_frame(sk).unwrap().origin().z - 15.0).abs() < 1e-9);
    wb.finish_sketch();
    d.settle(&mut wb);

    // Axis: the Z axis picked from the origin folder of the browser.
    wb.run_ui("work.axis").unwrap();
    wb.browser_action(crate::browser::BrowserAction::Reference(crate::work::Reference::Axis(tenon_model::OriginAxis::Z)));
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!(wb.scene().work.iter().any(|(_, g)| matches!(g, WorkGeom::Axis(a) if a.dir().near(Vec3::Z, 1e-12))), "{:?}", wb.scene().work);
    assert!(!wb.status_error, "{}", wb.status());
}

#[test]
fn equations_typed_into_fields_and_the_parameters_dialog() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    let lines = wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap()["lines"].clone();
    let width = wb.exec("sketch.constrain", json!({ "sketch": f.0, "constraint": { "type": "length", "line": lines[0], "value": 40 } })).unwrap();
    assert_eq!(width["name"], "d0");
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    d.settle(&mut wb);
    let ext = wb.document().features().last().unwrap().id;

    // Edit the extrusion and type an equation into Distance: the preview follows at once.
    wb.edit_feature(ext).unwrap();
    d.settle(&mut wb);
    type_into(&mut d, &mut wb, "tn_props_dist", "d0 / 2");
    d.frame(&mut wb, vec![]);
    assert!((volume(&wb) - 40.0 * 20.0 * 20.0).abs() < 1e-6, "preview: {}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let list = wb.exec("param.list", json!({})).unwrap();
    let d1 = list["model"].as_array().unwrap().iter().find(|p| p["name"] == "d1").unwrap().clone();
    assert_eq!(d1["equation"], "d0 / 2", "{list}");

    // The width changes: the depth follows its equation.
    wb.exec("param.set", json!({ "name": "d0", "equation": "60" })).unwrap();
    d.frame(&mut wb, vec![]);
    assert!((volume(&wb) - 60.0 * 20.0 * 30.0).abs() < 1e-6, "{}", volume(&wb));

    // Editing again shows the equation in the field.
    wb.edit_feature(ext).unwrap();
    assert_eq!(wb.panel_eqs.get("distance").map(String::as_str), Some("d0 / 2"));
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);

    // The Parameters dialog: type a new equation for d1 into its cell.
    wb.run_ui("tools.parameters").unwrap();
    // A new window sizes its grid columns over its first frames.
    for _ in 0..5 {
        d.frame(&mut wb, vec![]);
    }
    let cell = d.ctx.read_response(egui::Id::new(("tn_param_eq", "d1"))).expect("d1's equation cell").rect;
    d.click(&mut wb, cell.center());
    assert_eq!(d.ctx.memory(|m| m.focused()), Some(egui::Id::new(("tn_param_eq", "d1"))));
    d.frame(
        &mut wb,
        vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }],
    );
    d.frame(&mut wb, vec![egui::Event::Text("d0 / 3".into())]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!((volume(&wb) - 60.0 * 20.0 * 20.0).abs() < 1e-6, "{} {}", volume(&wb), wb.status());
    assert!(wb.chrome.params, "the dialog stays open");
    assert!(!wb.status_error, "{}", wb.status());
}

/// Where a browser row was drawn in the last frame.
fn browser_row(d: &Driver, label: &str) -> Rect {
    d.ctx
        .data(|x| x.get_temp::<Vec<(String, Rect)>>(egui::Id::new("tn_browser_rows")))
        .unwrap_or_default()
        .into_iter()
        .find(|(l, _)| l == label)
        .unwrap_or_else(|| panic!("no browser row {label}"))
        .1
}

#[test]
fn end_of_part_and_features_are_dragged_in_the_browser() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    for (x0, h) in [(0.0, 10.0), (50.0, 5.0)] {
        wb.create_sketch(json!({ "plane": "xy" })).unwrap();
        let f = sketching(&wb);
        wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": x0, "y1": 0, "x2": x0 + 10.0, "y2": 10 })).unwrap();
        wb.finish_sketch();
        wb.exec("model.extrude", json!({ "sketch": f.0, "distance": h, "operation": "new_body" })).unwrap();
    }
    d.settle(&mut wb);
    assert!((volume(&wb) - 1500.0).abs() < 1e-6);
    let names = |wb: &Workbench| wb.document().features().iter().map(|f| f.name.clone()).collect::<Vec<_>>();

    // Drag End of Part up onto Extrusion2: it is rolled back.
    let (eop, ex2) = (browser_row(&d, "End of Part"), browser_row(&d, "Extrusion2"));
    d.drag(&mut wb, eop.center(), ex2.center() - vec2(0.0, 4.0), egui::PointerButton::Primary);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.document().end_of_part(), 2, "{:?}", names(&wb));
    assert!((volume(&wb) - 1000.0).abs() < 1e-6, "{}", volume(&wb));
    assert!(browser_row(&d, "End of Part").top() < browser_row(&d, "Extrusion2").top(), "the marker is drawn above it");

    // Back to the bottom, then drag Extrusion2 above Extrusion1: its sketch goes along.
    wb.exec("feature.end_of_part", json!({})).unwrap();
    d.frame(&mut wb, vec![]);
    let (ex1, ex2) = (browser_row(&d, "Extrusion1"), browser_row(&d, "Extrusion2"));
    d.drag(&mut wb, ex2.center(), ex1.center() - vec2(0.0, 4.0), egui::PointerButton::Primary);
    d.frame(&mut wb, vec![]);
    assert_eq!(names(&wb), ["Sketch2", "Extrusion2", "Sketch1", "Extrusion1"]);
    assert!((volume(&wb) - 1500.0).abs() < 1e-6);
    assert!(wb.chrome.browser_drag.is_none());
    assert!(!wb.status_error, "{}", wb.status());
}

#[test]
fn start_2d_sketch_picks_a_plane_or_face_in_the_viewport() {
    use tenon_geom::Vec3;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    d.frame(&mut wb, vec![]);
    // S starts a sketch: the origin planes appear; Esc gives up.
    d.tap(&mut wb, egui::Key::S);
    assert!(wb.pick_plane);
    d.tap(&mut wb, egui::Key::Escape);
    assert!(!wb.pick_plane && !wb.is_sketching());

    // Again, and click on the XY plane where the other two are not in front of it (from the
    // home view the ray to (20, -20, 0) crosses neither x = 0 nor y = 0).
    d.tap(&mut wb, egui::Key::S);
    let on_xy = on_screen(&wb, Vec3::new(20.0, -20.0, 0.0));
    d.click(&mut wb, on_xy);
    assert!(wb.is_sketching(), "{}", wb.status());
    let f = sketching(&wb);
    let sk = wb.document().sketch(f).unwrap();
    assert_eq!(sk.entity_count(), 1, "the projected origin point");
    assert!(matches!(
        wb.document().feature(f).map(|x| &x.kind),
        Some(tenon_model::FeatureKind::Sketch { plane: tenon_model::PlaneRef::Origin(tenon_model::OriginPlane::XY), .. })
    ));

    // Draw, extrude, then S and a click on the block's top face sketches on that face.
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 30, "y2": 30 })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    wb.finish_sketch();
    d.settle(&mut wb);
    wb.run_ui("view.home").unwrap();
    d.settle(&mut wb);
    d.tap(&mut wb, egui::Key::S);
    let on_top = on_screen(&wb, Vec3::new(20.0, 20.0, 10.0));
    d.click(&mut wb, on_top);
    assert!(wb.is_sketching(), "{}", wb.status());
    let f2 = sketching(&wb);
    assert_ne!(f, f2);
    d.settle(&mut wb);
    let frame = wb.sketch_frame(f2).unwrap();
    assert!((frame.origin().z - 10.0).abs() < 1e-9, "on the top face, nearer than the XY plane below it");
}

#[test]
fn typed_values_and_inference_constrain_while_drawing() {
    use tenon_geom::Vec3;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    d.settle(&mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    let typed = |d: &mut Driver, wb: &mut Workbench, s: &str| d.frame(wb, vec![egui::Event::Text(s.into())]);

    // Rectangle from the projected origin: type 40, Tab, 25, Enter. The corner stays on the
    // origin and both sizes become dimensions, so the sketch is fully constrained.
    wb.run_ui("sketch.rectangle").unwrap();
    let origin = at(&wb, 0.0, 0.0);
    d.click(&mut wb, origin);
    let toward = at(&wb, 30.0, 20.0);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(toward)]);
    typed(&mut d, &mut wb, "40");
    d.tap(&mut wb, egui::Key::Tab);
    typed(&mut d, &mut wb, "25");
    d.tap(&mut wb, egui::Key::Enter);
    let sk = wb.document().sketch(f).unwrap().clone();
    let info = tenon_model::cmd::sketch_info_value(&sk);
    let regions = info["regions"].as_array().unwrap();
    assert_eq!(regions.len(), 1, "{info}");
    assert!((regions[0]["area"].as_f64().unwrap() - 1000.0).abs() < 1e-6, "40 x 25: {}", regions[0]);
    assert_eq!(info["dof"], 0, "fully constrained: {info}");
    assert_eq!(wb.sketch_dof_text().as_deref(), Some("Fully Constrained"));

    // A line drawn almost horizontally becomes horizontal.
    d.tap(&mut wb, egui::Key::Escape);
    wb.run_ui("sketch.line").unwrap();
    let a = at(&wb, 60.0, 0.0);
    d.click(&mut wb, a);
    let b = at(&wb, 90.0, 0.8);
    d.click(&mut wb, b);
    let sk = wb.document().sketch(f).unwrap().clone();
    let horizontal = sk.constraints().any(|(_, c)| matches!(c, tenon_sketch::Constraint::Horizontal { line } if sk.line(*line).is_some_and(|(p, q)| (p.x - 60.0).abs() < 0.5 && (q.x - 90.0).abs() < 0.5)));
    assert!(horizontal, "inferred horizontal: {}", tenon_model::cmd::sketch_info_value(&sk));
    // The chain continues; typed values place the next line exactly: 15 long at 90 degrees.
    typed(&mut d, &mut wb, "15");
    d.tap(&mut wb, egui::Key::Tab);
    typed(&mut d, &mut wb, "90");
    d.tap(&mut wb, egui::Key::Enter);
    let sk = wb.document().sketch(f).unwrap().clone();
    let vertical = sk.constraints().find_map(|(_, c)| match c {
        tenon_sketch::Constraint::Vertical { line } => sk.line(*line).filter(|(p, _)| (p.x - 90.0).abs() < 0.5),
        _ => None,
    });
    let (p, q) = vertical.expect("a vertical line");
    assert!((p.dist(q) - 15.0).abs() < 1e-6 && (q.y - p.y - 15.0).abs() < 1e-6, "{p:?} -> {q:?}");
    assert!(sk.constraints().any(|(_, c)| matches!(c, tenon_sketch::Constraint::Length { value, .. } if (*value - 15.0).abs() < 1e-9)));
}

#[test]
fn dimensions_are_typed_into_a_box_on_the_dimension() {
    use tenon_geom::Vec3;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    let lines = wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 30, "y2": 10 })).unwrap();
    let bottom = tenon_sketch::EntityId(lines["lines"][0].as_u64().unwrap() as u32);
    d.settle(&mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    let len = |wb: &Workbench| {
        let (a, b) = wb.document().sketch(f).unwrap().line(bottom).unwrap();
        a.dist(b)
    };

    // D, click the bottom line, click below it to place: the box opens on the dimension with
    // the measured value selected; typing replaces it, Enter applies.
    d.tap(&mut wb, egui::Key::D);
    let on_line = at(&wb, 15.0, 0.0);
    d.click(&mut wb, on_line);
    let below = at(&wb, 15.0, -6.0);
    d.click(&mut wb, below);
    assert!(matches!(wb.panel, Some(Panel::Value(_))), "the edit box is open");
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![egui::Event::Text("45".into())]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "Enter applied it");
    assert!((len(&wb) - 45.0).abs() < 1e-6, "{}", len(&wb));

    // Double-click the dimension's label: the same box, now editing it.
    d.tap(&mut wb, egui::Key::Escape);
    let sk = wb.document().sketch(f).unwrap().clone();
    let c = sk.constraints().find(|(_, c)| matches!(c, tenon_sketch::Constraint::Length { .. })).map(|(_, c)| c.clone()).unwrap();
    let label = wb.dimension_anchor(f, &c).unwrap();
    d.frame(&mut wb, vec![egui::Event::PointerMoved(label)]);
    // (Pause first: clicks less than 0.6 s after the earlier ones would count as a triple click.)
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    for pressed in [true, false, true, false] {
        d.frame(&mut wb, vec![Driver::button(label, egui::PointerButton::Primary, pressed)]);
    }
    assert!(matches!(wb.panel, Some(Panel::EditDimension { .. })), "double-click edits: {:?}", wb.panel.is_some());
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![egui::Event::Text("50".into())]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!((len(&wb) - 50.0).abs() < 1e-6, "{}", len(&wb));
}

#[test]
fn grid_steps_and_line_inference() {
    use crate::sketcher::grid_step;
    assert_eq!(grid_step(10.0, 18.0), 2.0, "10 px per mm: 2 mm lines are 20 px apart");
    assert_eq!(grid_step(1.0, 18.0), 20.0);
    assert_eq!(grid_step(0.3, 18.0), 100.0);
    assert_eq!(grid_step(100.0, 18.0), 0.2);
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
