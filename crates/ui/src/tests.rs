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
    assert!(Workbench::without_kernel().run_ui("sketch.stretch").unwrap_err().contains("not in the current plan"));
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

/// Held by the frame-time tests and by tests with heavy geometry work, so the timings measure the
/// workbench and not other tests competing for the processor.
pub(crate) static TIMING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Takes [`TIMING`] (a test that panicked while holding it does not poison it for the rest).
pub(crate) fn timing_lock() -> std::sync::MutexGuard<'static, ()> {
    TIMING.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Feeds raw pointer events through egui, frame by frame, as the windowing layer would.
pub(crate) struct Driver {
    pub(crate) ctx: egui::Context,
    time: f64,
    size: egui::Vec2,
    /// What the last frame drew.
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Driver {
    pub(crate) fn new(size: egui::Vec2) -> Driver {
        Driver { ctx: egui::Context::default(), time: 0.0, size, shapes: Vec::new() }
    }
    pub(crate) fn frame(&mut self, wb: &mut Workbench, events: Vec<egui::Event>) {
        self.time += 1.0 / 60.0;
        let input =
            egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)), time: Some(self.time), events, ..Default::default() };
        let mut out = self.ctx.run_ui(input, |ui| wb.ui(ui, None));
        self.shapes = std::mem::take(&mut out.shapes);
        out.drop_without_applying_deltas();
    }
    /// The texts the last frame drew (labels, banners, tooltips), one per piece.
    pub(crate) fn texts(&self) -> Vec<String> {
        fn walk(s: &egui::Shape, out: &mut Vec<String>) {
            match s {
                egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
                egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for c in &self.shapes {
            walk(&c.shape, &mut out);
        }
        out
    }
    /// The middle of the piece of text `text` the last frame drew, to click it as a user would.
    pub(crate) fn text_pos(&self, text: &str) -> Option<Pos2> {
        fn walk(s: &egui::Shape, text: &str) -> Option<Pos2> {
            match s {
                egui::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() / 2.0),
                egui::Shape::Vec(v) => v.iter().find_map(|x| walk(x, text)),
                _ => None,
            }
        }
        self.shapes.iter().find_map(|c| walk(&c.shape, text))
    }
    /// Holds (or releases) modifier keys from the next frame on.
    pub(crate) fn modifiers(&mut self, wb: &mut Workbench, m: egui::Modifiers) {
        self.frame(wb, vec![egui::Event::ModifiersChanged(m)]);
    }
    /// Runs frames until view transitions have finished.
    pub(crate) fn settle(&mut self, wb: &mut Workbench) {
        for _ in 0..120 {
            self.frame(wb, vec![]);
            if wb.view.anim.is_none() {
                return;
            }
        }
        panic!("the view never settled");
    }
    /// Holds a key down (`true`) or lets it go.
    pub(crate) fn key(&mut self, wb: &mut Workbench, key: egui::Key, pressed: bool) {
        self.frame(wb, vec![egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::default() }]);
    }
    /// Presses and releases a key (a second press without a release counts as a repeat).
    pub(crate) fn tap(&mut self, wb: &mut Workbench, key: egui::Key) {
        let ev = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::default() };
        self.frame(wb, vec![ev(true), ev(false)]);
    }
    pub(crate) fn button(pos: Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::default() }
    }
    pub(crate) fn click(&mut self, wb: &mut Workbench, pos: Pos2) {
        self.frame(wb, vec![egui::Event::PointerMoved(pos)]);
        self.frame(wb, vec![Self::button(pos, egui::PointerButton::Primary, true)]);
        self.frame(wb, vec![Self::button(pos, egui::PointerButton::Primary, false)]);
    }
    pub(crate) fn drag(&mut self, wb: &mut Workbench, from: Pos2, to: Pos2, button: egui::PointerButton) {
        self.frame(wb, vec![egui::Event::PointerMoved(from)]);
        self.frame(wb, vec![Self::button(from, button, true)]);
        for k in 1..=10 {
            self.frame(wb, vec![egui::Event::PointerMoved(from + (to - from) * (k as f32 / 10.0))]);
        }
        self.frame(wb, vec![Self::button(to, button, false)]);
    }
}

/// Where a dialog or menu button was drawn in the last frame (see `drawing::remember`).
pub(crate) fn pressable(d: &Driver, key: &str) -> Pos2 {
    d.ctx
        .data(|x| x.get_temp::<std::collections::BTreeMap<String, Rect>>(egui::Id::new("tn_buttons")))
        .unwrap_or_default()
        .get(key)
        .unwrap_or_else(|| panic!("no button {key}"))
        .center()
}

/// Presses `key` with Ctrl (Cmd on macOS) held.
pub(crate) fn ctrl(d: &mut Driver, wb: &mut Workbench, key: egui::Key) {
    d.modifiers(wb, egui::Modifiers::COMMAND);
    d.frame(wb, vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }]);
    d.frame(wb, vec![egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::COMMAND }]);
    d.modifiers(wb, egui::Modifiers::default());
}

/// Screen position of a model point in the last viewport.
pub(crate) fn on_screen(wb: &Workbench, p: tenon_geom::Vec3) -> Pos2 {
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
    // The arrow follows the pointer along its axis, landing on round numbers.
    assert!(((p.distance / 0.1).round() * 0.1 - p.distance).abs() < 1e-9, "snapped: {}", p.distance);
    wb.panel_request = Some(crate::panels::PanelRequest::Ok);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(volume(&wb) > 40.0 * 20.0 * 26.0, "{}", volume(&wb));

    // Regression: dragged almost to nothing, the arrow comes back out (it used to stick, since a
    // drag was scaled by the length shown).
    wb.edit_feature(ext).unwrap();
    d.settle(&mut wb);
    let Some(Panel::Extrude(p)) = wb.panel.clone() else { panic!() };
    let tip = on_screen(&wb, base + n * p.distance);
    let near = on_screen(&wb, base + n * 0.2);
    d.drag(&mut wb, tip, near, egui::PointerButton::Primary);
    let Some(Panel::Extrude(p)) = wb.panel.clone() else { panic!() };
    assert!(p.distance < 1.0, "down to almost nothing: {}", p.distance);
    let tip = on_screen(&wb, base + n * p.distance);
    let to = on_screen(&wb, base + n * 15.0);
    d.drag(&mut wb, tip, to, egui::PointerButton::Primary);
    let Some(Panel::Extrude(p)) = wb.panel.clone() else { panic!() };
    assert!((p.distance - 15.0).abs() < 0.5, "and back out: {}", p.distance);
    assert_eq!(p.direction, crate::panels::Direction::Default);
    // Past the sketch plane, it turns round.
    let tip = on_screen(&wb, base + n * p.distance);
    let to = on_screen(&wb, base - n * 8.0);
    d.drag(&mut wb, tip, to, egui::PointerButton::Primary);
    let Some(Panel::Extrude(p)) = wb.panel.clone() else { panic!() };
    assert_eq!(p.direction, crate::panels::Direction::Flipped);
    assert!((p.distance - 8.0).abs() < 0.5, "{}", p.distance);
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

#[test]
fn measure_faces_and_edges_by_clicking_them() {
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
    let result = |wb: &Workbench, label: &str| match &wb.panel {
        Some(Panel::Measure(m)) => m.result.as_ref().and_then(|r| r.as_ref().ok()).and_then(|r| r.get(label)),
        _ => panic!("no measure panel"),
    };

    wb.run_ui("inspect.measure").unwrap();
    // The top face: its area.
    let top = on_screen(&wb, Vec3::new(20.0, 10.0, 10.0));
    d.click(&mut wb, top);
    assert!((result(&wb, "Area").unwrap() - 800.0).abs() < 1e-6);
    // Then the front face: they meet at 90 degrees.
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    let front = on_screen(&wb, Vec3::new(20.0, 0.0, 5.0));
    d.click(&mut wb, front);
    assert!(result(&wb, "Distance").unwrap().abs() < 1e-9);
    assert!((result(&wb, "Angle").unwrap() - 90.0).abs() < 1e-9);
    // A third click starts over: the front-right vertical edge is 10 long.
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    let edge = on_screen(&wb, Vec3::new(40.0, 0.0, 5.0));
    d.click(&mut wb, edge);
    assert!((result(&wb, "Length").unwrap() - 10.0).abs() < 1e-9, "{:?}", wb.panel.as_ref().map(|_| ()));
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none());
}

#[test]
fn rib_from_the_sketch_being_drawn() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    // An L-bracket, 40 deep.
    wb.create_sketch(json!({ "plane": "xz" })).unwrap();
    let f = sketching(&wb);
    let pts = [(0.0, 0.0), (60.0, 0.0), (60.0, 8.0), (8.0, 8.0), (8.0, 38.0), (0.0, 38.0)];
    for i in 0..6 {
        let (a, b) = (pts[i], pts[(i + 1) % 6]);
        wb.exec("sketch.line", json!({ "sketch": f.0, "x1": a.0, "y1": a.1, "x2": b.0, "y2": b.1 })).unwrap();
    }
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 40 })).unwrap();
    let wp = wb.exec("work.plane", json!({ "base": "xz", "distance": 20 })).unwrap()["feature"].as_u64().unwrap();
    // The rib line, in a sketch halfway along.
    wb.create_sketch(json!({ "work_plane": wp })).unwrap();
    let sk = sketching(&wb);
    wb.exec("sketch.line", json!({ "sketch": sk.0, "x1": 30, "y1": 8, "x2": 8, "y2": 30 })).unwrap();
    d.settle(&mut wb);
    let bracket = (60.0 * 8.0 + 8.0 * 30.0) * 40.0;

    // Rib while sketching: the sketch finishes and the rib previews, 2 thick.
    wb.run_ui("model.rib").unwrap();
    assert!(!wb.is_sketching());
    d.settle(&mut wb);
    let Some(Panel::Rib(p)) = wb.panel.clone() else { panic!("no rib panel: {}", wb.status()) };
    assert_eq!((p.sketch, p.lines.len()), (sk, 1));
    let tri = 22.0 * 22.0 / 2.0;
    assert!((volume(&wb) - (bracket + tri * 2.0)).abs() < 1e-6, "preview: {}", volume(&wb));
    type_into(&mut d, &mut wb, "tn_props_rib_thickness", "6");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!((volume(&wb) - (bracket + tri * 6.0)).abs() < 1e-6, "{}", volume(&wb));
    assert_eq!(wb.document().features().last().unwrap().name, "Rib1");
}

#[test]
fn editing_a_used_sketch_waits_for_finish_to_update_the_part() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    let r = wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    d.settle(&mut wb);
    assert!((volume(&wb) - 8000.0).abs() < 1e-6);
    // Edit the sketch and drag its far corner out: while sketching, the part is not rebuilt.
    wb.edit_feature(f).unwrap();
    d.settle(&mut wb);
    let corner = r["corners"].as_array().map(|c| c[2].as_u64().unwrap()).unwrap_or_else(|| {
        let sk = wb.document().sketch(f).unwrap();
        sk.entities().find(|(id, _)| sk.point(*id).is_some_and(|p| p.dist(Vec2::new(40.0, 20.0)) < 1e-9)).unwrap().0.0 as u64
    });
    for i in 1..=5 {
        wb.exec("sketch.drag", json!({ "sketch": f.0, "point": corner, "x": 40.0 + 2.0 * f64::from(i), "y": 20.0 })).unwrap();
        d.frame(&mut wb, vec![]);
    }
    assert!((volume(&wb) - 8000.0).abs() < 1e-6, "still the part as the sketch was opened: {}", volume(&wb));
    // Finish Sketch: now the part follows.
    wb.finish_sketch();
    d.frame(&mut wb, vec![]);
    assert!((volume(&wb) - 50.0 * 20.0 * 10.0).abs() < 1e-6, "{}", volume(&wb));
}

/// Where a browser row was drawn in the last frame.
pub(crate) fn browser_row(d: &Driver, label: &str) -> Rect {
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

/// The UI's own work per frame (layout, picking, the browser; not GPU drawing) stays well inside
/// a 60 Hz frame on a 40-feature part. Numbers: `cargo test --release -p tenon-ui frame_time --
/// --nocapture`.
#[test]
fn frame_time_stays_within_budget() {
    use std::time::Instant;
    let _quiet = timing_lock();
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1440.0, 900.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 200, "y2": 100 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    let ext = wb.document().features().last().unwrap().id.0;
    for i in 0..19 {
        let top = wb.exec("model.face_ref", json!({ "origin": { "type": "cap", "feature": ext, "end": "end" } })).unwrap();
        let sk = wb.exec("sketch.create", json!({ "face": top })).unwrap()["feature"].as_u64().unwrap();
        for y in [20.0, 80.0] {
            wb.exec("sketch.point", json!({ "sketch": sk, "x": 10.0 + 10.0 * f64::from(i), "y": y })).unwrap();
        }
        wb.exec("model.hole", json!({ "sketch": sk, "diameter": 4, "through_all": true })).unwrap();
    }
    wb.mode = Mode::Model;
    d.settle(&mut wb);
    let time = |d: &mut Driver, wb: &mut Workbench, events: &dyn Fn(usize) -> Vec<egui::Event>| {
        let t = Instant::now();
        for i in 0..60 {
            d.frame(wb, events(i));
        }
        t.elapsed().as_secs_f64() * 1000.0 / 60.0
    };
    let idle = time(&mut d, &mut wb, &|_| vec![]);
    let c = wb.view.rect.center();
    let hover = time(&mut d, &mut wb, &|i| vec![egui::Event::PointerMoved(c + vec2(i as f32 * 3.0, 0.0))]);
    println!("{} features: idle frame {idle:.2} ms, pointer moving over the part {hover:.2} ms", wb.document().features().len());
    assert!(idle < 16.0 && hover < 16.0, "over the 16 ms frame budget: idle {idle:.2} ms, moving {hover:.2} ms");
}

/// Timing, not a check: frames while sketching on a busy sketch.
/// `cargo test --release -p tenon-ui sketch_frame_time -- --ignored --nocapture`
#[test]
#[ignore]
fn sketch_frame_time() {
    use std::time::Instant;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1440.0, 900.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    // 25 rectangles (100 lines), each dimensioned.
    for i in 0..5 {
        for j in 0..5 {
            let (x, y) = (f64::from(i) * 12.0, f64::from(j) * 12.0);
            let r = wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": x, "y1": y, "x2": x + 8.0, "y2": y + 8.0 })).unwrap();
            wb.exec("sketch.constrain", json!({ "sketch": f.0, "constraint": { "type": "length", "line": r["lines"][0], "value": 8 } })).unwrap();
        }
    }
    d.settle(&mut wb);
    let c = wb.view.rect.center();
    let mut time = |wb: &mut Workbench, tool: &str| {
        if !tool.is_empty() {
            wb.run_ui(tool).unwrap();
        }
        let t = Instant::now();
        for i in 0..60 {
            d.frame(wb, vec![egui::Event::PointerMoved(c + vec2(i as f32 * 2.0, (i % 7) as f32))]);
        }
        t.elapsed().as_secs_f64() * 1000.0 / 60.0
    };
    let select = time(&mut wb, "");
    let line = time(&mut wb, "sketch.line");
    // One drag step: solve, undo snapshot, parameters.
    let sk = wb.document().sketch(f).unwrap().clone();
    let corner = sk.entities().find(|(id, e)| !e.construction && sk.point(*id).is_some_and(|p| p.dist(Vec2::new(24.0, 24.0)) < 1e-9)).unwrap().0;
    let t = Instant::now();
    for i in 0..30 {
        wb.exec("sketch.drag", json!({ "sketch": f.0, "point": corner.0, "x": 0.1 * f64::from(i), "y": 0.0 })).unwrap();
    }
    let drag = t.elapsed().as_secs_f64() * 1000.0 / 30.0;
    println!("100-line sketch: pointer moving {select:.2} ms (select), {line:.2} ms (line tool); a drag step {drag:.2} ms");
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
    let (cid, c) = sk.constraints().find(|(_, c)| matches!(c, tenon_sketch::Constraint::Length { .. })).map(|(id, c)| (id, c.clone())).unwrap();
    // It stays where it was put down: 6 below the line.
    let placed = sk.place(cid).expect("the dimension has a place");
    assert!((placed.x - 15.0).abs() < 0.5 && (placed.y + 6.0).abs() < 0.5, "{placed:?}");
    let label = wb.dimension_anchor(f, &c, Some(placed)).unwrap();
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
    wb.dimension_click(f, &sketch, Some(bottom), Vec2::new(15.0, 0.0));
    wb.dimension_click(f, &sketch, Some(bottom), Vec2::new(15.0, 0.0));
    let Some(Panel::Value(v)) = wb.panel.clone() else { panic!("no value panel") };
    assert!((v.value - 30.0).abs() < 1e-9);
    let mut v2 = v;
    v2.value = 45.0;
    wb.panel = Some(Panel::Value(v2));
    if let Some(Panel::Value(v)) = wb.panel.take()
        && let crate::panels::ValueFor::Dimension { sketch, constraint, .. } = v.what
    {
        let mut c = constraint;
        c.set_value(v.value);
        wb.exec("sketch.constrain", json!({ "sketch": sketch.0, "constraint": serde_json::to_value(&c).unwrap() })).unwrap();
    }
    let (a, b) = wb.document().sketch(f).unwrap().line(bottom).unwrap();
    assert!((a.dist(b) - 45.0).abs() < 1e-7);
    frame(&mut wb, &ctx, vec2(1200.0, 800.0));
}

#[test]
fn a_failing_feature_says_why_in_the_viewport_and_its_browser_row() {
    let _quiet = timing_lock();
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let sk = sketching(&wb);
    let rect = wb.exec("sketch.rectangle", json!({ "sketch": sk.0, "x1": 0, "y1": 0, "x2": 20, "y2": 20 })).unwrap();
    wb.finish_sketch();
    let ex = wb.exec("model.extrude", json!({ "sketch": sk.0, "distance": 10 })).unwrap()["feature"].clone();
    let side = |i: usize| json!({ "type": "side", "feature": ex, "curve": rect["lines"][i] });
    let edge = wb.exec("model.edge_ref", json!({ "faces": [side(0), side(1)] })).unwrap();
    // A fillet far too large for the 20 mm block.
    wb.exec("model.fillet", json!({ "edges": [edge], "radius": 25 })).unwrap();
    d.settle(&mut wb);
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    let plain = "The 25 mm fillet could not be made on this edge. The radius is probably too large";
    // The viewport says which feature failed and why, in plain words, without being asked.
    let shown = d.texts();
    assert!(shown.iter().any(|t| t.starts_with(&format!("Fillet1: {plain}"))), "{shown:?}");
    // Its browser row says the same when the pointer rests on it.
    let row = browser_row(&d, "Fillet1");
    d.frame(&mut wb, vec![egui::Event::PointerMoved(row.center())]);
    for _ in 0..90 {
        d.frame(&mut wb, vec![]);
    }
    let shown = d.texts();
    assert!(shown.iter().any(|t| t.starts_with(plain) && t.contains("try a smaller radius")), "{shown:?}");

    // Ctrl+Z: the fillet is gone, the part rebuilds whole and nothing reports a failure.
    let banner = |d: &Driver| d.texts().iter().any(|t| t.starts_with("Fillet1: "));
    let block = 20.0 * 20.0 * 10.0;
    d.frame(&mut wb, vec![egui::Event::PointerMoved(pos2(700.0, 400.0))]);
    ctrl(&mut d, &mut wb, egui::Key::Z);
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    assert!(!banner(&d) && (volume(&wb) - block).abs() < 1e-6, "{:?} {}", d.texts(), volume(&wb));
    // Ctrl+Y: the same failure, said the same way, and the part as it was just before it.
    ctrl(&mut d, &mut wb, egui::Key::Y);
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    assert!(banner(&d) && (volume(&wb) - block).abs() < 1e-6, "{:?} {}", d.texts(), volume(&wb));
}

#[test]
fn a_lost_edge_is_repaired_from_the_banner_with_one_click() {
    let _quiet = timing_lock();
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    // A 40 x 30 x 10 block, the edge between its top and its right side rounded (R2).
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let sk = sketching(&wb);
    let rect = wb.exec("sketch.rectangle", json!({ "sketch": sk.0, "x1": 0, "y1": 0, "x2": 40, "y2": 30 })).unwrap();
    wb.finish_sketch();
    let ex = wb.exec("model.extrude", json!({ "sketch": sk.0, "distance": 10 })).unwrap()["feature"].clone();
    let right = rect["lines"][1].clone();
    let edge = wb
        .exec(
            "model.edge_ref",
            json!({ "faces": [{ "type": "cap", "feature": ex, "end": "end" }, { "type": "side", "feature": ex, "curve": right }] }),
        )
        .unwrap();
    wb.exec("model.fillet", json!({ "edges": [edge], "radius": 2 })).unwrap();
    let rounded = 40.0 * 30.0 * 10.0 - (4.0 - std::f64::consts::PI) * 30.0;
    d.settle(&mut wb);
    assert!((volume(&wb) - rounded).abs() < 1e-6);
    // The right line deleted and drawn again: a new side face, so the fillet's edge is gone.
    let info = wb.exec("sketch.info", json!({ "sketch": sk.0 })).unwrap();
    let line = info["entities"].as_array().unwrap().iter().find(|e| e["id"] == right).unwrap().clone();
    wb.exec("sketch.delete", json!({ "sketch": sk.0, "entities": [right] })).unwrap();
    wb.exec("sketch.line", json!({ "sketch": sk.0, "start": line["start"], "end": line["end"] })).unwrap();
    d.settle(&mut wb);
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    assert!(d.texts().iter().any(|t| t.starts_with("Fillet1: the referenced edge no longer exists")), "{:?}", d.texts());

    // Repair: the nearest edges are highlighted and the status says what to do.
    let at = pressable(&d, "tn_repair");
    d.click(&mut wb, at);
    assert!(wb.status().starts_with("Fillet1 uses an edge that no longer exists. Click the edge to use instead"), "{}", wb.status());
    assert!(!wb.repair_candidates().is_empty());
    // One click on the edge where the old one was: the fillet is back, as one undoable edit.
    let corner = on_screen(&wb, tenon_geom::Vec3::new(40.0, 15.0, 10.0));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(corner)]);
    d.click(&mut wb, corner);
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    assert!(wb.repair.is_none() && wb.status() == "Fillet1 now uses the one you picked.", "{}", wb.status());
    assert!(wb.scene.status.iter().all(|(_, s)| !matches!(s, tenon_model::FeatureStatus::Error { .. })));
    assert!((volume(&wb) - rounded).abs() < 1e-6, "{}", volume(&wb));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(egui::pos2(700.0, 120.0))]);
    ctrl(&mut d, &mut wb, egui::Key::Z);
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    assert!(d.texts().iter().any(|t| t.starts_with("Fillet1: the referenced edge no longer exists")), "undone: {:?}", d.texts());
    // Esc leaves repairing without changing anything.
    let at = pressable(&d, "tn_repair");
    d.click(&mut wb, at);
    assert!(wb.repair.is_some());
    d.tap(&mut wb, egui::Key::Escape);
    assert!(wb.repair.is_none() && wb.status() == "Repair stopped.", "{}", wb.status());
}

#[test]
fn sweep_coil_and_loft_from_the_ribbon() {
    // Sweep: a Ø4 circle along a path drawn last, in a sketch of its own.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let profile = sketching(&wb);
    wb.exec("sketch.circle", json!({ "sketch": profile.0, "cx": 0, "cy": 0, "r": 2 })).unwrap();
    wb.finish_sketch();
    // Another closed profile, drawn later, in a plane parallel to the path's: it cannot be swept
    // along the path, so it is not the one offered.
    wb.create_sketch(json!({ "plane": "xz" })).unwrap();
    let other = sketching(&wb);
    wb.exec("sketch.circle", json!({ "sketch": other.0, "cx": 40, "cy": 0, "r": 1 })).unwrap();
    wb.finish_sketch();
    wb.create_sketch(json!({ "plane": "xz" })).unwrap();
    let path = sketching(&wb);
    wb.exec("sketch.line", json!({ "sketch": path.0, "x1": 0, "y1": 0, "x2": 0, "y2": 20 })).unwrap();
    // Sweep while drawing the path: the path is not taken for the profile.
    wb.run_ui("model.sweep").unwrap();
    d.settle(&mut wb);
    let Some(Panel::Sweep(p)) = wb.panel.clone() else { panic!("no sweep panel: {}", wb.status()) };
    assert_eq!((p.sketch, p.path.map(|c| c.sketch)), (profile, Some(path)));
    assert!((volume(&wb) - PI * 4.0 * 20.0).abs() < 1e-6, "preview: {}", volume(&wb));
    // The sketches show over the part: the profile and the path brighter.
    assert_eq!(wb.shown_sketches(), [(profile, true), (other, false), (path, true)]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(wb.document().features().last().unwrap().name, "Sweep1");
    assert_eq!(wb.shown_sketches(), [(other, false)], "the sweep's own sketches are in it now");

    // Coil: a Ø2 circle 10 from the Z axis, the axis found in the sketch's plane; 3 turns typed.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    wb.create_sketch(json!({ "plane": "xz" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.circle", json!({ "sketch": f.0, "cx": 10, "cy": 0, "r": 1 })).unwrap();
    wb.run_ui("model.coil").unwrap();
    d.settle(&mut wb);
    let Some(Panel::Coil(p)) = wb.panel.clone() else { panic!("no coil panel: {}", wb.status()) };
    assert_eq!(p.axis, crate::panels::AxisChoice::Origin(tenon_model::OriginAxis::Z));
    let per_turn = PI * 2.0 * PI * 10.0;
    assert!((volume(&wb) - 5.0 * per_turn).abs() < 1e-3 * per_turn, "preview: {}", volume(&wb));
    type_into(&mut d, &mut wb, "tn_props_turns", "3");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!((volume(&wb) - 3.0 * per_turn).abs() < 1e-3 * per_turn, "{}", volume(&wb));
    // Edited from the browser: the panel starts with its values.
    let coil = wb.document().features().last().unwrap().id;
    wb.edit_feature(coil).unwrap();
    let Some(Panel::Coil(p)) = wb.panel.clone() else { panic!() };
    assert_eq!((p.pitch, p.turns), (10.0, 3.0));
    wb.panel = None;

    // Loft: two squares 10 apart; both sections are picked.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let bottom = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": bottom.0, "x1": -10, "y1": -10, "x2": 10, "y2": 10 })).unwrap();
    wb.finish_sketch();
    let up = wb.exec("work.plane", json!({ "base": "xy", "distance": 10 })).unwrap()["feature"].as_u64().unwrap();
    wb.create_sketch(json!({ "work_plane": up })).unwrap();
    let top = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": top.0, "x1": -5, "y1": -5, "x2": 5, "y2": 5 })).unwrap();
    wb.run_ui("model.loft").unwrap();
    d.settle(&mut wb);
    let Some(Panel::Loft(p)) = wb.panel.clone() else { panic!("no loft panel: {}", wb.status()) };
    assert_eq!(p.sections, vec![bottom, top]);
    let frustum = 10.0 / 3.0 * (400.0 + 100.0 + 200.0);
    assert!((volume(&wb) - frustum).abs() < 1e-6, "preview: {}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(wb.document().features().last().unwrap().name, "Loft1");
    // One sketch alone cannot be lofted: said in the status bar.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.circle", json!({ "sketch": f.0, "cx": 0, "cy": 0, "r": 2 })).unwrap();
    let e = wb.run_ui("model.loft").unwrap_err();
    assert!(e.contains("two or more sketches"), "{e}");
}

#[test]
fn draft_split_and_combine_pick_faces_planes_and_bodies() {
    use tenon_geom::Vec3;
    let block = |wb: &mut Workbench, d: &mut Driver| {
        wb.create_sketch(json!({ "plane": "xy" })).unwrap();
        let f = sketching(wb);
        wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
        wb.finish_sketch();
        wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
        d.frame(wb, vec![]);
        wb.look_from(Vec3::new(1.0, -1.0, 1.0));
        d.settle(wb);
    };
    // Draft: the front and right faces, clicked, lean in from the XY plane by the angle typed.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    block(&mut wb, &mut d);
    wb.run_ui("model.draft").unwrap();
    assert!(wb.has_properties());
    let (front, right) = (on_screen(&wb, Vec3::new(20.0, 0.0, 5.0)), on_screen(&wb, Vec3::new(40.0, 10.0, 5.0)));
    d.click(&mut wb, front);
    d.click(&mut wb, right);
    let Some(Panel::Draft(p)) = wb.panel.clone() else { panic!("no draft panel") };
    assert_eq!((p.faces.len(), p.degrees), (2, 3.0));
    assert_eq!(wb.panel_picks().len(), 2, "both faces are highlighted");
    // Each level is a rectangle shorter by z tan(a) both ways.
    let drafted = |deg: f64| {
        let k = deg.to_radians().tan();
        8000.0 - 3000.0 * k + 1000.0 / 3.0 * k * k
    };
    d.settle(&mut wb);
    assert!((volume(&wb) - drafted(3.0)).abs() < 1e-6, "preview: {}", volume(&wb));
    type_into(&mut d, &mut wb, "tn_props_draft_angle", "5");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!((volume(&wb) - drafted(5.0)).abs() < 1e-6, "{}", volume(&wb));
    assert_eq!(wb.document().features().last().unwrap().name, "Draft1");

    // Split: the work plane made last is offered; both sides stay, as two solids.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    block(&mut wb, &mut d);
    let wp = wb.exec("work.plane", json!({ "base": "xy", "distance": 4 })).unwrap()["feature"].as_u64().unwrap();
    wb.run_ui("model.split").unwrap();
    d.settle(&mut wb);
    let Some(Panel::Split(p)) = wb.panel.clone() else { panic!("no split panel: {}", wb.status()) };
    assert_eq!(p.plane, Some(tenon_model::PlaneRef::Work(FeatureId(u32::try_from(wp).unwrap()))));
    assert_eq!(wb.scene().bodies.len(), 2, "preview");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let volumes: Vec<f64> = wb.scene().bodies.iter().map(|b| b.volume).collect();
    assert!((volumes[0] - 3200.0).abs() < 1e-6 && (volumes[1] - 4800.0).abs() < 1e-6, "{volumes:?}");

    // Combine: the first solid clicked stays, the next is joined to it; a click on the first
    // again is refused, saying why.
    wb.run_ui("model.combine").unwrap();
    d.settle(&mut wb);
    let (low, high) = (on_screen(&wb, Vec3::new(20.0, 0.0, 2.0)), on_screen(&wb, Vec3::new(20.0, 0.0, 7.0)));
    d.click(&mut wb, low);
    let Some(Panel::Combine(p)) = wb.panel.clone() else { panic!("no combine panel") };
    assert!(p.base.is_some() && p.slot == crate::bodies::CombineSlot::Tools, "the other solids are picked next");
    assert_eq!(wb.panel_picks().len(), 6, "the whole solid is highlighted");
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, low);
    assert!(wb.status().contains("the body that stays"), "{}", wb.status());
    d.click(&mut wb, high);
    let Some(Panel::Combine(p)) = wb.panel.clone() else { panic!("no combine panel") };
    assert_eq!(p.tools.len(), 1);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(wb.scene().bodies.len(), 1);
    assert!((volume(&wb) - 8000.0).abs() < 1e-6, "{}", volume(&wb));
    assert_eq!(wb.document().features().last().unwrap().name, "Combine1");
    // With one solid there is nothing to combine.
    let e = wb.run_ui("model.combine").unwrap_err();
    assert!(e.contains("two or more solid bodies"), "{e}");
}

#[test]
fn a_thread_is_put_on_a_shaft_by_clicking_it_and_cut_when_modelled() {
    use tenon_geom::Vec3;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.circle", json!({ "sketch": f.0, "cx": 0, "cy": 0, "r": 4 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 20 })).unwrap();
    d.frame(&mut wb, vec![]);
    wb.look_from(Vec3::new(1.0, -1.0, 1.0));
    d.settle(&mut wb);
    let plain = PI * 16.0 * 20.0;

    // Thread, then a click on the shaft: sized from the face, shown on it, the part unchanged.
    wb.run_ui("model.thread").unwrap();
    let Some(Panel::Thread(p)) = wb.panel.clone() else { panic!("no thread panel") };
    assert!(p.face.is_none() && !p.modelled);
    let wall = on_screen(&wb, Vec3::new(8f64.sqrt(), -(8f64.sqrt()), 10.0));
    d.click(&mut wb, wall);
    d.settle(&mut wb);
    let Some(Panel::Thread(p)) = wb.panel.clone() else { panic!("no thread panel") };
    assert!(p.face.is_some(), "the round face is taken");
    assert_eq!(wb.scene().threads.len(), 1, "preview");
    assert_eq!(wb.scene().threads[0].mark.designation, "M8x1.25");
    assert_eq!(wb.thread_faces().len(), 1, "the threaded face is drawn in its own colour");
    assert!((volume(&wb) - plain).abs() < 1e-6);
    // The flat end is not a face a thread goes on.
    let top = on_screen(&wb, Vec3::new(0.0, 0.0, 20.0));
    d.click(&mut wb, top);
    let Some(Panel::Thread(q)) = wb.panel.clone() else { panic!("no thread panel") };
    assert_eq!(q.face, p.face);
    // A name of its own for drawings, typed; Enter is OK.
    type_into(&mut d, &mut wb, "tn_props_thread_name", "M8 special");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.settle(&mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(wb.document().features().last().unwrap().name, "Thread1");
    assert_eq!(wb.scene().threads[0].mark.designation, "M8 special");
    assert!((volume(&wb) - plain).abs() < 1e-6, "cosmetic: {}", volume(&wb));

    // Edited and ticked Modelled: the groove is cut, sixteen turns of it.
    let thread = wb.document().features().last().unwrap().id;
    wb.edit_feature(thread).unwrap();
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    let tick = pressable(&d, "tn_props_thread_modelled");
    d.click(&mut wb, tick);
    d.settle(&mut wb);
    let Some(Panel::Thread(p)) = wb.panel.clone() else { panic!("no thread panel") };
    assert!(p.modelled);
    let (pitch, depth) = (1.25, 0.625 * 0.866_025_403_784_438_6 * 1.25);
    let per_turn = (0.875 + 0.25) / 2.0 * pitch * depth * 2.0 * PI * (4.0 - depth * (0.875 + 0.5) / (3.0 * 1.125));
    let turns = (plain - volume(&wb)) / per_turn;
    assert!((turns - 16.0).abs() < 0.5, "preview: {turns} turns' worth cut");
    assert!(wb.thread_faces().is_empty(), "a modelled thread shows as what it is");
    wb.panel_request = Some(crate::panels::PanelRequest::Ok);
    d.frame(&mut wb, vec![]);
    d.settle(&mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!(((plain - volume(&wb)) / per_turn - 16.0).abs() < 0.5);
    assert!(!wb.status_error, "{}", wb.status());
}

#[test]
fn an_extrusion_is_tapered_from_its_advanced_properties() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": -10, "y1": -10, "x2": 10, "y2": 10 })).unwrap();
    d.settle(&mut wb);
    d.tap(&mut wb, egui::Key::E);
    d.settle(&mut wb);
    assert!((volume(&wb) - 4000.0).abs() < 1e-6);
    // Advanced Properties is folded away until it is opened; Taper is in it.
    assert!(d.ctx.read_response(egui::Id::new("tn_props_taper")).is_none());
    let header = d.text_pos("Advanced Properties").expect("the Advanced Properties header");
    d.click(&mut wb, header);
    d.frame(&mut wb, vec![]);
    type_into(&mut d, &mut wb, "tn_props_taper", "5");
    d.frame(&mut wb, vec![]);
    d.settle(&mut wb);
    let frustum = |deg: f64| {
        let top = 20.0 - 2.0 * 10.0 * f64::to_radians(deg).tan();
        10.0 / 3.0 * (400.0 + top * top + 20.0 * top)
    };
    assert!((volume(&wb) - frustum(5.0)).abs() < 1e-6, "the preview narrows: {}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.settle(&mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!((volume(&wb) - frustum(5.0)).abs() < 1e-6);
    // Edited again, the section is open on its value.
    let ext = wb.document().features().last().unwrap().id;
    wb.edit_feature(ext).unwrap();
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Extrude(p)) = wb.panel.clone() else { panic!("no extrude panel") };
    assert!((p.taper - 5.0).abs() < 1e-9);
    assert!(d.ctx.read_response(egui::Id::new("tn_props_taper")).is_some());
    type_into(&mut d, &mut wb, "tn_props_taper", "-5");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.settle(&mut wb);
    assert!((volume(&wb) - frustum(-5.0)).abs() < 1e-6, "leaning out: {}", volume(&wb));
}

#[test]
fn revolve_turns_a_circle_into_a_sphere_and_takes_a_clicked_line_for_its_axis() {
    use crate::panels::AxisChoice;
    use tenon_geom::Vec3;
    use tenon_model::OriginAxis;
    let ball = 4.0 / 3.0 * PI * 1000.0;
    // A circle on any origin plane, and Revolve: an origin axis in the sketch's plane is offered
    // (never one square to it), and the preview is a sphere.
    for (plane, axis) in [("xy", OriginAxis::Y), ("xz", OriginAxis::Z), ("yz", OriginAxis::Z)] {
        let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
        let mut d = Driver::new(vec2(1400.0, 860.0));
        wb.create_sketch(json!({ "plane": plane })).unwrap();
        let f = sketching(&wb);
        wb.exec("sketch.circle", json!({ "sketch": f.0, "cx": 0, "cy": 0, "r": 10 })).unwrap();
        d.settle(&mut wb);
        d.tap(&mut wb, egui::Key::R);
        d.settle(&mut wb);
        let Some(Panel::Revolve(p)) = wb.panel.clone() else { panic!("no revolve panel: {}", wb.status()) };
        assert_eq!(p.axis, AxisChoice::Origin(axis), "{plane}");
        assert!((volume(&wb) - ball).abs() < 1e-6, "{plane}: {} ({})", volume(&wb), wb.status());
        d.tap(&mut wb, egui::Key::Enter);
        d.frame(&mut wb, vec![]);
        d.settle(&mut wb);
        assert!(wb.panel.is_none() && !wb.status_error, "{}", wb.status());
        assert!((volume(&wb) - ball).abs() < 1e-6);
    }

    // A line drawn through the circle is what it turns about, without being asked.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.circle", json!({ "sketch": f.0, "cx": 30, "cy": 5, "r": 10 })).unwrap();
    let through = wb.exec("sketch.line", json!({ "sketch": f.0, "x1": 15, "y1": 5, "x2": 45, "y2": 5 })).unwrap()["line"].as_u64().unwrap();
    wb.run_ui("model.revolve").unwrap();
    d.settle(&mut wb);
    let Some(Panel::Revolve(p)) = wb.panel.clone() else { panic!("no revolve panel") };
    assert_eq!(p.axis, AxisChoice::Line(tenon_sketch::EntityId(u32::try_from(through).unwrap())));
    assert!((volume(&wb) - ball).abs() < 1e-6, "{} ({})", volume(&wb), wb.status());
    wb.panel = None;

    // Two lines to choose from: the profile is turned about an origin axis until one is clicked
    // in the viewport.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 10, "y1": 0, "x2": 20, "y2": 10 })).unwrap();
    wb.exec("sketch.line", json!({ "sketch": f.0, "x1": 5, "y1": -5, "x2": 5, "y2": 15 })).unwrap();
    let below = wb.exec("sketch.line", json!({ "sketch": f.0, "x1": 0, "y1": -10, "x2": 30, "y2": -10 })).unwrap()["line"].as_u64().unwrap();
    wb.run_ui("model.revolve").unwrap();
    d.settle(&mut wb);
    let Some(Panel::Revolve(p)) = wb.panel.clone() else { panic!("no revolve panel") };
    assert_eq!(p.axis, AxisChoice::Origin(OriginAxis::Y), "the origin axis the profile is beside");
    // (Pappus: the square's area times the way its middle goes round.)
    assert!((volume(&wb) - 100.0 * 2.0 * PI * 15.0).abs() < 1e-6, "{}", volume(&wb));
    assert!(wb.shown_sketches().contains(&(f, true)), "the sketch's lines show, to be clicked");
    let on_line = on_screen(&wb, Vec3::new(15.0, -10.0, 0.0));
    d.click(&mut wb, on_line);
    d.settle(&mut wb);
    let Some(Panel::Revolve(p)) = wb.panel.clone() else { panic!("no revolve panel") };
    assert_eq!(p.axis, AxisChoice::Line(tenon_sketch::EntityId(u32::try_from(below).unwrap())));
    assert!((volume(&wb) - 100.0 * 2.0 * PI * 15.0).abs() < 1e-6, "its middle is 15 from this line too: {}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.settle(&mut wb);
    assert!(wb.panel.is_none() && !wb.status_error, "{}", wb.status());
    assert_eq!(wb.document().features().last().unwrap().name, "Revolution1");
}

#[test]
fn a_dimension_follows_the_pointer_is_put_down_by_a_click_and_can_be_dragged() {
    use tenon_geom::Vec3;
    use tenon_sketch::Constraint;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    // A sloping line from (0, 0) to (40, 30), and a circle.
    let line = wb.exec("sketch.line", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 30 })).unwrap()["line"].as_u64().unwrap();
    let ring = wb.exec("sketch.circle", json!({ "sketch": f.0, "cx": 80, "cy": 10, "r": 10 })).unwrap()["circle"].as_u64().unwrap();
    d.settle(&mut wb);
    let at = |wb: &Workbench, x: f64, y: f64| on_screen(wb, Vec3::new(x, y, 0.0));
    let dims = |wb: &Workbench| -> Vec<(tenon_sketch::ConstraintId, Constraint)> {
        let sk = wb.document().sketch(f).unwrap();
        sk.constraints().filter(|(_, c)| c.is_dimensional()).map(|(id, c)| (id, c.clone())).collect()
    };
    let move_to = |d: &mut Driver, wb: &mut Workbench, p: Pos2| {
        d.frame(wb, vec![egui::Event::PointerMoved(p)]);
        d.frame(wb, vec![]);
    };

    // D, a click on the line: nothing is made yet. The dimension follows the pointer, and what
    // it measures depends on where the pointer is: under the line its width, beside it its
    // height, off its end its length.
    d.tap(&mut wb, egui::Key::D);
    let on_line = at(&wb, 20.0, 15.0);
    d.click(&mut wb, on_line);
    assert!(wb.panel.is_none() && dims(&wb).is_empty());
    let under = at(&wb, 20.0, -10.0);
    move_to(&mut d, &mut wb, under);
    assert!(d.texts().iter().any(|t| t == "40"), "its width, under the line: {:?}", d.texts());
    let beside = at(&wb, 55.0, 15.0);
    move_to(&mut d, &mut wb, beside);
    assert!(d.texts().iter().any(|t| t == "30"), "its height, beside it: {:?}", d.texts());
    let off_end = at(&wb, -10.0, 40.0);
    move_to(&mut d, &mut wb, off_end);
    assert!(d.texts().iter().any(|t| t == "50"), "its length, off its end: {:?}", d.texts());
    // A click under the line puts the width down there; its value box opens on it; Enter.
    d.click(&mut wb, under);
    assert!(matches!(wb.panel, Some(Panel::Value(_))), "the value box is open");
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let made = dims(&wb);
    assert!(matches!(made.as_slice(), [(_, Constraint::HorizontalDistance { value, .. })] if (value.abs() - 40.0).abs() < 1e-9), "{made:?}");
    let place = wb.document().sketch(f).unwrap().place(made[0].0).expect("it stays where it was put");
    assert!((place.x - 20.0).abs() < 0.5 && (place.y + 10.0).abs() < 0.5, "{place:?}");

    // The circle: a click on it, a click beside it, and its diameter is there.
    let on_ring = at(&wb, 90.0, 10.0);
    d.click(&mut wb, on_ring);
    let outside = at(&wb, 100.0, 30.0);
    move_to(&mut d, &mut wb, outside);
    assert!(d.texts().iter().any(|t| t == "Ø20"), "{:?}", d.texts());
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, outside);
    d.frame(&mut wb, vec![]);
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    let made = dims(&wb);
    assert!(made.iter().any(|(_, c)| matches!(c, Constraint::Diameter { curve, .. } if u64::from(curve.0) == ring)), "{made:?}");
    let _ = line;

    // With no tool running, the width's value is dragged somewhere else: the dimension moves
    // with it, in one step that Ctrl+Z takes back.
    d.tap(&mut wb, egui::Key::Escape);
    d.frame(&mut wb, vec![]);
    let width = made.iter().find(|(_, c)| matches!(c, Constraint::HorizontalDistance { .. })).unwrap().0;
    let from = at(&wb, place.x, place.y);
    let to = at(&wb, 25.0, -22.0);
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.drag(&mut wb, from, to, egui::PointerButton::Primary);
    let moved = wb.document().sketch(f).unwrap().place(width).unwrap();
    assert!((moved.x - 25.0).abs() < 1.0 && (moved.y + 22.0).abs() < 1.0, "{moved:?}");
    assert_eq!(dims(&wb).len(), 2, "moving a dimension changes nothing else");
    ctrl(&mut d, &mut wb, egui::Key::Z);
    d.frame(&mut wb, vec![]);
    let back = wb.document().sketch(f).unwrap().place(width).unwrap();
    assert!((back.x - place.x).abs() < 1e-9 && (back.y - place.y).abs() < 1e-9, "{back:?}");
}

#[test]
fn profiles_are_chosen_by_clicking_the_regions_of_the_sketch() {
    use tenon_geom::Vec3;
    use tenon_model::RegionSel;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    // Two rectangles side by side, 20 x 10 and 10 x 10, far enough apart to click each.
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 20, "y2": 10 })).unwrap();
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 60, "y1": 0, "x2": 70, "y2": 10 })).unwrap();
    d.settle(&mut wb);
    d.tap(&mut wb, egui::Key::E);
    d.settle(&mut wb);
    // Both are extruded until one is clicked.
    let Some(Panel::Extrude(p)) = wb.panel.clone() else { panic!("no extrude panel") };
    assert_eq!(p.regions, RegionSel::Default);
    assert!((volume(&wb) - 3000.0).abs() < 1e-6, "{}", volume(&wb));
    assert!(wb.shown_sketches().contains(&(f, true)), "the profile's sketch shows over the preview");
    // A click inside the small one takes it out; another puts it back.
    let small = on_screen(&wb, Vec3::new(66.0, 3.0, 0.0));
    d.click(&mut wb, small);
    d.settle(&mut wb);
    let Some(Panel::Extrude(p)) = wb.panel.clone() else { panic!("no extrude panel") };
    assert!(matches!(&p.regions, RegionSel::Keys(k) if k.len() == 1), "{:?}", p.regions);
    assert!((volume(&wb) - 2000.0).abs() < 1e-6, "{}", volume(&wb));
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, small);
    d.settle(&mut wb);
    assert!((volume(&wb) - 3000.0).abs() < 1e-6, "{}", volume(&wb));
    // The large one out: only the small one is left, and it cannot be taken out as well.
    let large = on_screen(&wb, Vec3::new(4.0, 3.0, 0.0));
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, large);
    d.settle(&mut wb);
    assert!((volume(&wb) - 1000.0).abs() < 1e-6, "{}", volume(&wb));
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, small);
    d.settle(&mut wb);
    assert!((volume(&wb) - 1000.0).abs() < 1e-6, "the last profile stays: {}", volume(&wb));
    assert!(wb.status().contains("at least one profile"), "{}", wb.status());
    // OK: the feature keeps the choice.
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.settle(&mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!((volume(&wb) - 1000.0).abs() < 1e-6);
    let tenon_model::FeatureKind::Extrude(e) = &wb.document().features().last().unwrap().kind else { panic!("not an extrusion") };
    assert!(matches!(&e.regions, RegionSel::Keys(k) if k.len() == 1));
}

#[test]
fn the_status_bar_changes_how_dimensions_read_and_hides_constraint_symbols() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    let lines = wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 30 })).unwrap();
    let (bottom, side) = (lines["lines"][0].clone(), lines["lines"][1].clone());
    // The width, and the height driven by it.
    wb.exec("sketch.constrain", json!({ "sketch": f.0, "constraint": { "type": "length", "line": bottom, "value": 40.0 }, "at_x": 20, "at_y": -10 }))
        .unwrap();
    wb.exec(
        "sketch.constrain",
        json!({ "sketch": f.0, "constraint": { "type": "length", "line": side, "value": 20.0 }, "equation": "d0 / 2", "at_x": 52, "at_y": 10 }),
    )
    .unwrap();
    d.settle(&mut wb);
    d.frame(&mut wb, vec![]);
    let drawn = |d: &Driver, text: &str| d.texts().iter().any(|t| t == text);
    // Values, an equation's marked "fx:".
    assert!(drawn(&d, "40") && drawn(&d, "fx: 20"), "{:?}", d.texts());
    // One click on the status bar's Dimensions button: their names. Another: name and value, or
    // the equation that drives it. A third: values again.
    let button = pressable(&d, "tn_status_dimensions");
    d.click(&mut wb, button);
    d.frame(&mut wb, vec![]);
    assert!(drawn(&d, "d0") && drawn(&d, "d1") && !drawn(&d, "40"), "{:?}", d.texts());
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, button);
    d.frame(&mut wb, vec![]);
    assert!(drawn(&d, "d0 = 40") && drawn(&d, "d1 = d0 / 2"), "{:?}", d.texts());
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, button);
    d.frame(&mut wb, vec![]);
    assert!(drawn(&d, "40") && drawn(&d, "fx: 20"), "{:?}", d.texts());

    // The rectangle's sides carry their constraint symbols until F9; F8 brings them back, and
    // so does the status bar's Constraints button.
    assert!(drawn(&d, "H") && drawn(&d, "V"), "{:?}", d.texts());
    d.tap(&mut wb, egui::Key::F9);
    d.frame(&mut wb, vec![]);
    assert!(!drawn(&d, "H") && !drawn(&d, "V") && drawn(&d, "40"), "symbols hidden, dimensions not: {:?}", d.texts());
    d.tap(&mut wb, egui::Key::F8);
    d.frame(&mut wb, vec![]);
    assert!(drawn(&d, "H"));
    let button = pressable(&d, "tn_status_constraints");
    for _ in 0..45 {
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, button);
    d.frame(&mut wb, vec![]);
    assert!(!drawn(&d, "H"), "{:?}", d.texts());
    assert!(d.texts().iter().any(|t| t == "Constraints: hidden"));
}

/// A 40 x 20 x 10 block whose width is the dimension d0, depth d1 and height d2.
fn dimensioned_block(wb: &mut Workbench) {
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(wb);
    let lines = wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap()["lines"].clone();
    for (line, value) in [(0, 40), (1, 20)] {
        wb.exec("sketch.constrain", json!({ "sketch": f.0, "constraint": { "type": "length", "line": lines[line], "value": value } })).unwrap();
    }
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
}

/// Runs the frames a new window takes to size itself.
fn open_window(d: &mut Driver, wb: &mut Workbench) {
    for _ in 0..5 {
        d.frame(wb, vec![]);
    }
}

#[test]
fn a_part_is_given_a_material_a_density_and_a_colour_in_its_properties() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    dimensioned_block(&mut wb);
    d.settle(&mut wb);
    let shows = |d: &Driver, text: &str| d.texts().iter().any(|t| t == text);
    let standard = wb.body_palette(0).0;

    wb.run_ui("inspect.mass").unwrap();
    open_window(&mut d, &mut wb);
    // 8 cubic centimetres of nothing in particular weigh as water does.
    assert!(shows(&d, "8 g") && shows(&d, "8000 mm³"), "{:?}", d.texts());

    // The material is picked from the list: brass is 8.5 g/cm^3 and brass-coloured.
    let combo = d.text_pos("Generic").expect("the material box");
    d.click(&mut wb, combo);
    d.frame(&mut wb, vec![]);
    let brass = d.text_pos("Brass").expect("Brass in the list");
    d.click(&mut wb, brass);
    open_window(&mut d, &mut wb);
    assert_eq!(wb.document().material().map(|m| (m.name.as_str(), m.density)), Some(("Brass", 8.5)), "{}", wb.status());
    assert!(shows(&d, "68 g"), "{:?}", d.texts());
    assert_eq!(wb.body_palette(0).0, egui::Color32::from_rgb(0xc9, 0xa6, 0x4a));

    // A density of one's own keeps the name.
    type_into(&mut d, &mut wb, "tn_partprops_density", "8.4");
    d.tap(&mut wb, egui::Key::Enter);
    open_window(&mut d, &mut wb);
    assert_eq!(wb.document().material().map(|m| (m.name.as_str(), m.density)), Some(("Brass", 8.4)), "{}", wb.status());
    assert!(shows(&d, "67.2 g"), "{:?}", d.texts());
    // What is not a density is refused, and says so.
    type_into(&mut d, &mut wb, "tn_partprops_density", "heavy");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error && wb.status().contains("is not a density"), "{}", wb.status());
    type_into(&mut d, &mut wb, "tn_partprops_density", "0");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error && wb.status().contains("more than 0"), "{}", wb.status());
    assert_eq!(wb.document().density(), 8.4);

    // A swatch gives the part a colour of its own; As Material takes it back.
    d.click(&mut wb, pressable(&d, "tn_partprops_swatch_#d04030"));
    open_window(&mut d, &mut wb);
    assert_eq!(wb.document().appearance(), Some("#d04030"), "{}", wb.status());
    assert_eq!(wb.body_palette(0).0, egui::Color32::from_rgb(0xd0, 0x40, 0x30));
    let back = d.text_pos("As Material").expect("the As Material button");
    d.click(&mut wb, back);
    open_window(&mut d, &mut wb);
    assert_eq!(wb.document().appearance(), None);
    assert_eq!(wb.body_palette(0).0, egui::Color32::from_rgb(0xc9, 0xa6, 0x4a));
    // A colour is also typed.
    type_into(&mut d, &mut wb, "tn_partprops_color", "#3C3F44");
    d.tap(&mut wb, egui::Key::Enter);
    open_window(&mut d, &mut wb);
    assert_eq!(wb.document().appearance(), Some("#3c3f44"), "{}", wb.status());
    // (On a part this dark the edges are drawn light.)
    assert!(wb.body_palette(0).1.r() > 0x80);
    type_into(&mut d, &mut wb, "tn_partprops_color", "red");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error && wb.status().contains("#rrggbb"), "{}", wb.status());

    // Each change is one undo step: back to the standard part.
    for _ in 0..5 {
        wb.run_ui("edit.undo").unwrap();
    }
    d.frame(&mut wb, vec![]);
    assert!(wb.document().material().is_none() && wb.document().appearance().is_none());
    assert_eq!(wb.body_palette(0).0, standard);
    assert!(wb.chrome.mass, "the window stays open");
}

#[test]
fn a_design_table_is_made_from_ticked_parameters_and_its_rows_resize_the_part() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    dimensioned_block(&mut wb);
    d.settle(&mut wb);

    wb.run_ui("tools.table").unwrap();
    open_window(&mut d, &mut wb);
    // Nothing ticked: Create Table does nothing.
    d.click(&mut wb, pressable(&d, "tn_table_create"));
    d.frame(&mut wb, vec![]);
    assert!(wb.document().table().is_none());
    // Width and height go in the table; the part as it stands is its first row.
    for name in ["d0", "d2"] {
        let tick = pressable(&d, &format!("tn_table_pick_{name}"));
        d.click(&mut wb, tick);
        d.frame(&mut wb, vec![]);
    }
    d.click(&mut wb, pressable(&d, "tn_table_create"));
    open_window(&mut d, &mut wb);
    let table = wb.document().table().cloned().unwrap_or_else(|| panic!("no table: {}", wb.status()));
    assert_eq!(table.columns, ["d0", "d2"]);
    assert_eq!((table.active.as_str(), table.rows.len(), table.rows[0].values.as_slice()), ("Size 1", 1, [40.0, 10.0].as_slice()));

    // Another size: it starts as the active one, and its cells are typed into.
    d.click(&mut wb, pressable(&d, "tn_table_add_row"));
    open_window(&mut d, &mut wb);
    assert_eq!(wb.document().table().unwrap().rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Size 1", "Size 2"]);
    let set = |d: &mut Driver, wb: &mut Workbench, row: &str, column: &str, text: &str| {
        let at = d.ctx.read_response(egui::Id::new(("tn_table_cell", row, column))).expect("the cell").rect.center();
        d.click(wb, at);
        d.frame(
            wb,
            vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }],
        );
        d.frame(wb, vec![egui::Event::Text(text.into())]);
        d.tap(wb, egui::Key::Enter);
        for _ in 0..3 {
            d.frame(wb, vec![]);
        }
    };
    set(&mut d, &mut wb, "Size 2", "d0", "60");
    set(&mut d, &mut wb, "Size 2", "d2", "25");
    assert_eq!(wb.document().table().unwrap().rows[1].values, [60.0, 25.0], "{}", wb.status());
    // The part is still at Size 1.
    assert!((volume(&wb) - 40.0 * 20.0 * 10.0).abs() < 1e-6, "{}", volume(&wb));

    // Ticking the other row makes the part that size.
    d.click(&mut wb, pressable(&d, "tn_table_row_Size 2"));
    open_window(&mut d, &mut wb);
    assert_eq!(wb.document().table().unwrap().active, "Size 2", "{}", wb.status());
    assert!((volume(&wb) - 60.0 * 20.0 * 25.0).abs() < 1e-6, "{}", volume(&wb));
    // A cell of the active row changes the part at once.
    set(&mut d, &mut wb, "Size 2", "d2", "30");
    assert!((volume(&wb) - 60.0 * 20.0 * 30.0).abs() < 1e-6, "{} {}", volume(&wb), wb.status());
    // What is not a size is refused: the row keeps its value and the part its shape.
    set(&mut d, &mut wb, "Size 2", "d0", "wide");
    assert!(wb.status_error && wb.status().contains("is not a number"), "{}", wb.status());
    set(&mut d, &mut wb, "Size 2", "d0", "-5");
    assert!(wb.status_error, "{}", wb.status());
    assert_eq!(wb.document().table().unwrap().rows[1].values, [60.0, 30.0]);
    assert!((volume(&wb) - 60.0 * 20.0 * 30.0).abs() < 1e-6);

    // Editing the part outside the table: the active row follows it.
    wb.exec("param.set", json!({ "name": "d0", "equation": "50" })).unwrap();
    open_window(&mut d, &mut wb);
    assert_eq!(wb.document().table().unwrap().rows[1].values, [50.0, 30.0]);
    // Back to the first size, by its row; undo returns to the second.
    d.click(&mut wb, pressable(&d, "tn_table_row_Size 1"));
    open_window(&mut d, &mut wb);
    assert!((volume(&wb) - 40.0 * 20.0 * 10.0).abs() < 1e-6, "{} {}", volume(&wb), wb.status());
    wb.run_ui("edit.undo").unwrap();
    open_window(&mut d, &mut wb);
    assert_eq!(wb.document().table().unwrap().active, "Size 2");
    assert!((volume(&wb) - 50.0 * 20.0 * 30.0).abs() < 1e-6, "{}", volume(&wb));

    // Delete Table: the part keeps the size it is at.
    let delete = d.text_pos("Delete Table").expect("the Delete Table button");
    d.click(&mut wb, delete);
    open_window(&mut d, &mut wb);
    assert!(wb.document().table().is_none(), "{}", wb.status());
    assert!((volume(&wb) - 50.0 * 20.0 * 30.0).abs() < 1e-6);
    assert!(wb.chrome.table, "the dialog stays open, offering to make a table again");
    assert!(d.texts().iter().any(|t| t == "Create Table"));
}
