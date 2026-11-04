//! Emboss, Derive and UCS through the window, by real pointer and keyboard (DEC-133): the ribbon's
//! button, the panel, what is picked in the view, OK, and a double-click on the browser's row.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};

use egui::{Pos2, vec2};
use serde_json::json;
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;
use tenon_model::{FeatureKind, WorkGeom};

use crate::Workbench;
use crate::panels::Panel;
use crate::tests::{Driver, browser_row, on_screen, pressable, type_into};
use crate::work::WorkMethod;

fn setup() -> (Workbench, Driver) {
    let wb = Workbench::headless(Box::new(OcctKernel::new()));
    let d = Driver::new(vec2(1400.0, 860.0));
    (wb, d)
}

fn frames(d: &mut Driver, wb: &mut Workbench) {
    for _ in 0..6 {
        d.frame(wb, vec![]);
    }
}

fn ribbon(d: &mut Driver, wb: &mut Workbench, id: &str) {
    frames(d, wb);
    let at = pressable(d, &format!("ribbon {id}"));
    d.click(wb, at);
    frames(d, wb);
}

fn click_at(d: &mut Driver, wb: &mut Workbench, p: Pos2) {
    d.frame(wb, vec![egui::Event::PointerMoved(p)]);
    d.frame(wb, vec![]);
    d.click(wb, p);
    frames(d, wb);
}

fn enter(d: &mut Driver, wb: &mut Workbench) {
    d.tap(wb, egui::Key::Enter);
    frames(d, wb);
}

fn volume(wb: &Workbench) -> f64 {
    wb.scene().bodies.iter().map(|b| b.volume).sum()
}

#[track_caller]
fn near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-6 * b.abs().max(1.0), "{a} against {b}");
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
    frames(d, wb);
}

/// A block 40 x 40 x 10 from the origin.
fn block(wb: &mut Workbench) {
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].as_u64().unwrap();
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 40 })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": sk, "distance": 10 })).unwrap();
}

/// A sketch `distance` above XY with a rectangle.
fn rect_above(wb: &mut Workbench, distance: f64, r: [f64; 4]) -> u64 {
    let wp = wb.exec("work.plane", json!({ "base": "xy", "distance": distance })).unwrap()["feature"].as_u64().unwrap();
    let sk = wb.exec("sketch.create", json!({ "work_plane": wp })).unwrap()["feature"].as_u64().unwrap();
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": r[0], "y1": r[1], "x2": r[2], "y2": r[3] })).unwrap();
    sk
}

#[test]
fn every_command_of_this_lane_is_available_with_a_true_tip() {
    for id in ["model.emboss", "model.derive", "work.ucs"] {
        let c = crate::commands::find(id).unwrap();
        assert!(c.available(), "{id}");
        assert!(tenon_io::cmd::find(id).is_some(), "{id} is a command");
    }
    assert!(crate::commands::find("model.emboss").unwrap().tip.contains("recessed"));
    assert!(crate::commands::find("model.derive").unwrap().tip.contains("follows"));
    assert!(crate::commands::find("work.ucs").unwrap().tip.contains("planes, axes and origin"));
}

#[test]
fn emboss_is_raised_recessed_and_edited_by_pointer() {
    let (mut wb, mut d) = setup();
    frames(&mut d, &mut wb);
    // With nothing to emboss, it says so.
    ribbon(&mut d, &mut wb, "model.emboss");
    assert!(wb.status_error && wb.status().contains("nothing to emboss"), "{}", wb.status());
    block(&mut wb);
    rect_above(&mut wb, 10.0, [5.0, 5.0, 15.0, 11.0]);
    frames(&mut d, &mut wb);
    near(volume(&wb), 16000.0);

    ribbon(&mut d, &mut wb, "model.emboss");
    assert!(matches!(wb.panel, Some(Panel::Emboss(_))), "{}", wb.status());
    // The preview shows it at the depth it starts with, 1.
    frames(&mut d, &mut wb);
    near(volume(&wb), 16000.0 + 60.0);
    type_into(&mut d, &mut wb, "tn_props_emboss_depth", "2");
    enter(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(wb.status(), "Made Emboss1.");
    near(volume(&wb), 16000.0 + 120.0);
    assert!(wb.scene().status.iter().all(|(_, s)| matches!(s, tenon_model::FeatureStatus::Ok)), "{:?}", wb.scene().status);

    // A double-click on its row opens its panel, as the same emboss.
    let row = browser_row(&d, "Emboss1").center();
    double_click(&mut d, &mut wb, row);
    let Some(Panel::Emboss(p)) = wb.panel.clone() else { panic!("no emboss panel: {}", wb.status()) };
    assert_eq!(p.depth, 2.0);
    assert!(p.editing.is_some());
    // Recess by the list.
    let style = pressable(&d, "tn_emboss_style");
    d.click(&mut wb, style);
    frames(&mut d, &mut wb);
    let recess = pressable(&d, "tn_emboss_style_Recess");
    d.click(&mut wb, recess);
    frames(&mut d, &mut wb);
    near(volume(&wb), 16000.0 - 120.0);
    enter(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    assert_eq!(wb.status(), "Changed Emboss1.");
    near(volume(&wb), 16000.0 - 120.0);
    // Undo with the keyboard.
    crate::tests::ctrl(&mut d, &mut wb, egui::Key::Z);
    frames(&mut d, &mut wb);
    near(volume(&wb), 16000.0 + 120.0);
    assert_eq!(wb.document().features().last().unwrap().name, "Emboss1");
}

#[test]
fn wrap_to_face_is_ticked_and_its_cylinder_clicked() {
    let (mut wb, mut d) = setup();
    let (r, depth, w, len) = (10.0, 2.0, 16.0, 12.0);
    let sk = wb.exec("sketch.create", json!({ "plane": "yz" })).unwrap()["feature"].as_u64().unwrap();
    wb.exec("sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": r })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": sk, "distance": 50 })).unwrap();
    rect_above(&mut wb, 15.0, [10.0, -w / 2.0, 10.0 + len, w / 2.0]);
    frames(&mut d, &mut wb);
    let v0 = PI * r * r * 50.0;
    near(volume(&wb), v0);

    ribbon(&mut d, &mut wb, "model.emboss");
    type_into(&mut d, &mut wb, "tn_props_emboss_depth", "2");
    frames(&mut d, &mut wb);
    // Ticked: no preview until the face is clicked; OK says what it lacks.
    let tick = pressable(&d, "tn_emboss_wrap");
    d.click(&mut wb, tick);
    frames(&mut d, &mut wb);
    near(volume(&wb), v0);
    enter(&mut d, &mut wb);
    assert!(wb.status_error && wb.status().contains("click the cylinder or cone"), "{}", wb.status());
    // A flat end refused in words.
    d.settle(&mut wb);
    let end = on_screen(&wb, Vec3::new(50.0, 0.0, 0.0));
    click_at(&mut d, &mut wb, end);
    assert!(wb.status_error && wb.status().contains("cylinder or a cone"), "{}", wb.status());
    // The round face, clicked as a user would: where it faces the view.
    let round = on_screen(&wb, Vec3::new(30.0, 10.0 * 0.6, 10.0 * 0.8));
    click_at(&mut d, &mut wb, round);
    let Some(Panel::Emboss(p)) = wb.panel.clone() else { panic!("no emboss panel") };
    assert!(p.face.is_some(), "{}", wb.status());
    frames(&mut d, &mut wb);
    let raised = len * (w / r) * ((r + depth) * (r + depth) - r * r) / 2.0;
    assert!((volume(&wb) - (v0 + raised)).abs() < 1e-2, "{} against {}", volume(&wb), v0 + raised);
    enter(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    assert_eq!(wb.status(), "Made Emboss1.");
    assert!((volume(&wb) - (v0 + raised)).abs() < 1e-2);
    let FeatureKind::Emboss(e) = &wb.document().features().last().unwrap().kind else { panic!() };
    assert!(e.wrap.is_some());
}

#[test]
fn derive_takes_a_file_a_scale_and_a_mirror_and_follows_the_file_on_rebuild() {
    let dir = std::env::temp_dir().join(format!("tenon-ui-derive-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let other = dir.join("other.tenon");
    let write = |h: f64| {
        let mut s = tenon_model::Session::default();
        let run = |s: &mut tenon_model::Session, id: &str, p: serde_json::Value| tenon_io::cmd::run(s, id, &p, None).unwrap();
        let sk = run(&mut s, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
        run(&mut s, "sketch.rectangle", json!({ "sketch": sk, "x1": 10, "y1": 0, "x2": 30, "y2": 10 }));
        run(&mut s, "model.extrude", json!({ "sketch": sk, "distance": h }));
        tenon_io::project::save(&other, s.document(), &serde_json::Map::new()).unwrap();
    };
    write(5.0);
    let (mut wb, mut d) = setup();
    let chosen = other.clone();
    wb.services.pick_open_ext = Some(Box::new(move |ext: &str| {
        assert_eq!(ext, "tenon");
        Some(chosen.clone())
    }));
    ribbon(&mut d, &mut wb, "model.derive");
    assert!(matches!(wb.panel, Some(Panel::Derive(_))), "{}", wb.status());
    frames(&mut d, &mut wb);
    near(volume(&wb), 1000.0);
    assert!(d.texts().iter().any(|t| t == "other.tenon"), "the panel says which file");

    // Scale 2; mirrored: tick it, and take the YZ plane in the browser.
    type_into(&mut d, &mut wb, "tn_props_derive_scale", "2");
    frames(&mut d, &mut wb);
    near(volume(&wb), 8000.0);
    let tick = pressable(&d, "tn_derive_mirror");
    d.click(&mut wb, tick);
    frames(&mut d, &mut wb);
    enter(&mut d, &mut wb);
    assert!(wb.status_error && wb.status().contains("click the plane"), "{}", wb.status());
    wb.chrome.origin_open = true;
    frames(&mut d, &mut wb);
    let yz = browser_row(&d, "YZ Plane").center();
    d.click(&mut wb, yz);
    frames(&mut d, &mut wb);
    let b = wb.scene().bbox().unwrap();
    near(b.max.x, -20.0);
    near(b.min.x, -60.0);
    enter(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    assert_eq!(wb.status(), "Made Derive1.");
    near(volume(&wb), 8000.0);

    // Saved beside it, the path is relative.
    let mine = dir.join("mine.tenon");
    wb.save(&mine).unwrap();
    let text = std::fs::read_to_string(&mine).unwrap();
    assert!(text.contains("path = \"other.tenon\""), "{text}");
    frames(&mut d, &mut wb);

    // The other file changes: Rebuild All takes it.
    write(10.0);
    frames(&mut d, &mut wb);
    near(volume(&wb), 8000.0);
    wb.run_ui("model.rebuild").unwrap();
    frames(&mut d, &mut wb);
    near(volume(&wb), 16000.0);
    assert!(!wb.session.is_dirty());

    // A double-click on the row reopens the panel, with what it had.
    let row = browser_row(&d, "Derive1").center();
    double_click(&mut d, &mut wb, row);
    let Some(Panel::Derive(p)) = wb.panel.clone() else { panic!("no panel: {}", wb.status()) };
    assert_eq!((p.scale, p.mirrored, p.new_solid), (2.0, true, false));
    d.tap(&mut wb, egui::Key::Escape);
    frames(&mut d, &mut wb);

    // Opened with the file gone: it opens, and the status bar says what to do.
    wb.save(&mine).unwrap();
    std::fs::remove_file(&other).unwrap();
    wb.open(&mine).unwrap();
    frames(&mut d, &mut wb);
    assert!(wb.status().contains("other.tenon") && wb.status().contains("missing"), "{}", wb.status());
    assert!(wb.document().features().iter().any(|f| f.name == "Derive1"));

    // A part cannot be told to derive itself.
    let me = mine.clone();
    wb.services.pick_open_ext = Some(Box::new(move |_: &str| Some(me.clone())));
    ribbon(&mut d, &mut wb, "model.derive");
    assert!(wb.status_error && wb.status().contains("cannot derive itself"), "{}", wb.status());
    assert!(wb.panel.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn from_the_sketch_plane_it_is_chosen_and_wrap_goes_away() {
    let (mut wb, mut d) = setup();
    block(&mut wb);
    rect_above(&mut wb, 12.0, [5.0, 5.0, 15.0, 11.0]);
    frames(&mut d, &mut wb);
    ribbon(&mut d, &mut wb, "model.emboss");
    type_into(&mut d, &mut wb, "tn_props_emboss_depth", "5");
    frames(&mut d, &mut wb);
    let style = pressable(&d, "tn_emboss_style");
    d.click(&mut wb, style);
    frames(&mut d, &mut wb);
    let plane = pressable(&d, "tn_emboss_style_FromPlane");
    d.click(&mut wb, plane);
    frames(&mut d, &mut wb);
    // Stands up to the plane, 2 above the top.
    near(volume(&wb), 16000.0 + 120.0);
    // The Flip takes away where it added: looking the other way the plane has 5 of material to
    // add above it (floating), and it says so.
    if let Some(Panel::Emboss(p)) = &mut wb.panel {
        p.reverse = true;
    }
    frames(&mut d, &mut wb);
    let said: Vec<_> = wb
        .scene()
        .status
        .iter()
        .filter_map(|(_, s)| if let tenon_model::FeatureStatus::Error { message } = s { Some(message.clone()) } else { None })
        .collect();
    assert!(said.iter().any(|m| m.contains("does not reach the part")), "{said:?}");
    assert!(wb.status_error, "{}", wb.status());
    if let Some(Panel::Emboss(p)) = &mut wb.panel {
        p.reverse = false;
    }
    frames(&mut d, &mut wb);
    enter(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    assert_eq!(wb.status(), "Made Emboss1.");
    near(volume(&wb), 16000.0 + 120.0);
    let FeatureKind::Emboss(e) = &wb.document().features().last().unwrap().kind else { panic!() };
    assert_eq!(e.style, tenon_model::EmbossStyle::FromPlane);
}

fn ucs_frame(wb: &Workbench, name: &str) -> tenon_geom::Frame {
    let id = wb.document().features().iter().find(|f| f.name == name).unwrap().id;
    match wb.scene().work.iter().find(|(w, _)| *w == id).map(|x| x.1) {
        Some(WorkGeom::Ucs(f)) => f,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_ucs_is_typed_shown_and_its_planes_and_axes_picked_in_the_view() {
    // (Tall enough that the browser keeps every row in view beside an open panel.)
    let (mut wb, _) = setup();
    let mut d = Driver::new(vec2(1400.0, 1500.0));
    block(&mut wb);
    frames(&mut d, &mut wb);
    ribbon(&mut d, &mut wb, "work.ucs");
    assert!(matches!(&wb.panel, Some(Panel::Work(w)) if w.method == WorkMethod::UcsOffset), "{}", wb.status());
    type_into(&mut d, &mut wb, "tn_props_ucs_x", "60");
    type_into(&mut d, &mut wb, "tn_props_ucs_z", "20");
    type_into(&mut d, &mut wb, "tn_props_ucs_rz", "90");
    frames(&mut d, &mut wb);
    enter(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    assert_eq!(wb.status(), "Made UCS1.");
    let f = ucs_frame(&wb, "UCS1");
    assert!(f.origin().near(Vec3::new(60.0, 0.0, 20.0), 1e-9) && f.x().near(Vec3::Y, 1e-9), "{f:?}");
    // Its triad is drawn, with its letters and name.
    let texts = d.texts();
    assert!(["UCS1", "X", "Y", "Z"].iter().all(|l| texts.iter().any(|t| t == l)), "{texts:?}");

    // Start 2D Sketch, and the UCS's XY square clicked in the view: the sketch stands on it.
    d.settle(&mut wb);
    frames(&mut d, &mut wb);
    wb.run_ui("sketch.new").unwrap();
    frames(&mut d, &mut wb);
    let size = wb.scene().bbox().map_or(30.0, |b| (b.diagonal() * 0.35).max(15.0)) * 0.45 * 0.4;
    let corner = on_screen(&wb, f.origin() + f.x() * (size * 0.5) + f.y() * (size * 0.5));
    click_at(&mut d, &mut wb, corner);
    assert!(wb.is_sketching(), "{}", wb.status());
    let sk = wb.document().features().last().unwrap().id;
    assert!(
        matches!(
            &wb.document().feature(sk).unwrap().kind,
            FeatureKind::Sketch { plane: tenon_model::PlaneRef::Ucs { plane: tenon_model::OriginPlane::XY, .. }, .. }
        ),
        "{:?}",
        wb.document().feature(sk).unwrap().kind
    );
    wb.exec("sketch.rectangle", json!({ "sketch": sk.0, "x1": 1, "y1": 1, "x2": 11, "y2": 11 })).unwrap();
    wb.finish_sketch();
    frames(&mut d, &mut wb);

    // Extrude To: the plane clicked in the browser's row of the UCS: its YZ plane at x = 60, seen
    // along X of the sketch's own... the sketch's normal is world Z, so its XY plane cannot do; a
    // second UCS higher up.
    let u2 = wb.exec("work.ucs", json!({ "z": 35 })).unwrap()["feature"].as_u64().unwrap();
    let _ = u2;
    frames(&mut d, &mut wb);
    wb.open_extrude(None).unwrap();
    if let Some(Panel::Extrude(p)) = &mut wb.panel {
        p.sketch = sk;
        p.extent = crate::panels::ExtentChoice::To;
    }
    frames(&mut d, &mut wb);
    let row = browser_row(&d, "UCS2").center();
    // (Opened by its box, as the Origin folder is.)
    let box_at = Pos2::new(browser_row(&d, "UCS2").left() + 6.0 + 16.0 + 5.0, row.y);
    d.click(&mut wb, box_at);
    frames(&mut d, &mut wb);
    let xy = browser_row(&d, "UCS2: XY Plane").center();
    d.click(&mut wb, xy);
    frames(&mut d, &mut wb);
    enter(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    assert!(wb.status().contains("Made Extrusion2"), "{}", wb.status());
    // From z = 20 to z = 35: 10 x 10 x 15 more.
    near(volume(&wb), 16000.0 + 1500.0);

    // A mirror across its plane, its axis for a pattern, by the panel's selectors in the browser.
    let ex2 = wb.document().features().last().unwrap().id;
    let u1 = wb.document().features().iter().find(|f| f.name == "UCS1").unwrap().id;
    wb.exec("feature.visibility", json!({ "feature": u2, "visible": false })).unwrap();
    frames(&mut d, &mut wb);
    wb.open_pattern(crate::panels::CopyKind::Rect, None).unwrap();
    frames(&mut d, &mut wb);
    wb.toggle_pattern_feature(ex2);
    if let Some(Panel::Pattern(p)) = &mut wb.panel {
        p.slot = crate::panels::Slot::Dir1;
        p.count1 = 2.0;
        p.spacing1 = 30.0;
    }
    frames(&mut d, &mut wb);
    wb.chrome.expanded.insert(u1);
    frames(&mut d, &mut wb);
    let y = browser_row(&d, "UCS1: X Axis").center();
    d.click(&mut wb, y);
    frames(&mut d, &mut wb);
    let Some(Panel::Pattern(p)) = wb.panel.clone() else { panic!("no pattern panel") };
    assert_eq!(p.dir1, tenon_model::DirectionRef::Ucs { feature: u1, axis: tenon_model::OriginAxis::X });
    enter(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    near(volume(&wb), 16000.0 + 3000.0);

    // A double-click on its row opens its panel with what it was given.
    let row = browser_row(&d, "UCS1").center();
    double_click(&mut d, &mut wb, row);
    let Some(Panel::Work(w)) = wb.panel.clone() else { panic!("no panel: {}", wb.status()) };
    assert_eq!(w.method, WorkMethod::UcsOffset);
    assert!((w.offsets[0] - 60.0).abs() < 1e-9 && (w.turns[2] - 90.0).abs() < 1e-9, "{:?} {:?}", w.offsets, w.turns);
    type_into(&mut d, &mut wb, "tn_props_ucs_z", "25");
    enter(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    assert_eq!(wb.status(), "Changed UCS1.");
    near(ucs_frame(&wb, "UCS1").origin().z, 25.0);
    // (What stands on it stands higher, as far as the plane above: one 10 shorter each.)
    near(volume(&wb), 16000.0 + 2.0 * 1000.0);
    let _ = FRAC_PI_2;
}

#[test]
fn a_ucs_goes_through_three_corners_clicked_and_is_measured_to() {
    let (mut wb, _) = setup();
    let mut d = Driver::new(vec2(1400.0, 1500.0));
    block(&mut wb);
    frames(&mut d, &mut wb);
    ribbon(&mut d, &mut wb, "work.ucs");
    // Three Points, by the method list.
    if let Some(Panel::Work(w)) = &mut wb.panel {
        w.method = WorkMethod::UcsPoints;
        w.slot = crate::work::WorkSlot::P1;
    }
    frames(&mut d, &mut wb);
    d.settle(&mut wb);
    // A corner is clicked near it on an edge: the end of the edge nearer the pointer.
    for corner in [Vec3::new(0.0, 0.0, 10.0), Vec3::new(40.0, 0.0, 10.0), Vec3::new(0.0, 40.0, 10.0)] {
        let inward = (Vec3::new(20.0, 20.0, 10.0) - corner).normalized();
        // On the edge from there along the block: a little along X or Y.
        let along =
            if inward.x.abs() > 0.0 && corner.y == 0.0 { Vec3::new(inward.x.signum(), 0.0, 0.0) } else { Vec3::new(0.0, inward.y.signum(), 0.0) };
        let at = on_screen(&wb, corner + along * 3.0);
        click_at(&mut d, &mut wb, at);
    }
    let Some(Panel::Work(w)) = wb.panel.clone() else { panic!("no panel") };
    assert!(w.points.iter().all(Option::is_some), "{:?}: {}", w.points, wb.status());
    enter(&mut d, &mut wb);
    frames(&mut d, &mut wb);
    assert_eq!(wb.status(), "Made UCS1.");
    let f = ucs_frame(&wb, "UCS1");
    assert!(f.origin().near(Vec3::new(0.0, 0.0, 10.0), 1e-9) && f.x().near(Vec3::X, 1e-9) && f.z().near(Vec3::Z, 1e-9), "{f:?}");

    // Measure from its origin, clicked in the view, to the bottom face: 10.
    wb.run_ui("inspect.measure").unwrap();
    frames(&mut d, &mut wb);
    let origin = on_screen(&wb, f.origin());
    click_at(&mut d, &mut wb, origin);
    let Some(Panel::Measure(m)) = wb.panel.clone() else { panic!("no measure panel") };
    assert!(m.work_a.is_some(), "{}", wb.status());
    assert_eq!(m.result.as_ref().unwrap().as_ref().unwrap().get("Z"), Some(10.0));
    // Its origin shows where it is, then the far side of the block: from the origin to the
    // block's bottom is 10, by way of a click on the front face and the Distance it gives.
    let front = on_screen(&wb, Vec3::new(20.0, 0.0, 5.0));
    click_at(&mut d, &mut wb, front);
    let Some(Panel::Measure(m)) = wb.panel.clone() else { panic!("no measure panel") };
    let r = m.result.as_ref().unwrap().as_ref().unwrap();
    near(r.get("Distance").unwrap(), 0.0);
    // Then a plane of it against the bottom, by the browser and the click.
    d.tap(&mut wb, egui::Key::Escape);
    frames(&mut d, &mut wb);
    wb.run_ui("inspect.measure").unwrap();
    frames(&mut d, &mut wb);
    let id = wb.document().features().last().unwrap().id;
    wb.chrome.expanded.insert(id);
    frames(&mut d, &mut wb);
    let row = browser_row(&d, "UCS1: XY Plane").center();
    d.click(&mut wb, row);
    frames(&mut d, &mut wb);
    wb.view.camera = {
        let mut c = wb.view.camera;
        c.pitch = -c.pitch;
        c
    };
    frames(&mut d, &mut wb);
    let under = on_screen(&wb, Vec3::new(20.0, 20.0, 0.0));
    click_at(&mut d, &mut wb, under);
    let Some(Panel::Measure(m)) = wb.panel.clone() else { panic!("no measure panel") };
    let r = m.result.as_ref().unwrap().as_ref().unwrap();
    near(r.get("Distance").unwrap(), 10.0);
    near(r.get("Angle").unwrap(), 0.0);
}
