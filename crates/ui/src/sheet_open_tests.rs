//! Rip, Unfold and Refold through the UI, with real pointer and key input: the ribbon's buttons,
//! their panels, what is clicked in the view, the browser's rows.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use egui::vec2;
use serde_json::{Value, json};
use tenon_geom::Vec3;
use tenon_kernel_occt::OcctKernel;
use tenon_model::FeatureKind;
use tenon_model::sheet::RipWay;

use crate::Workbench;
use crate::commands::{SHEET_TAB, find};
use crate::panels::Panel;
use crate::sheet::RipChoice;
use crate::tests::{Driver, browser_row, on_screen, pressable, type_into};

const T: f64 = 2.0;
const R: f64 = 3.0;
const RHO: f64 = R + 0.44 * T;
const ALLOWANCE: f64 = FRAC_PI_2 * RHO;

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

fn num(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or_else(|| panic!("no number `{key}` in {v}"))
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

/// The pointer moved to a point of the part, and a click there.
fn click_on(d: &mut Driver, wb: &mut Workbench, at: Vec3) {
    let p = on_screen(wb, at);
    d.frame(wb, vec![egui::Event::PointerMoved(p)]);
    d.frame(wb, vec![]);
    d.click(wb, p);
    frames(d, wb);
}

/// A click on a ribbon button, by the words on it.
fn press(d: &mut Driver, wb: &mut Workbench, label: &str) {
    let at = d.text_pos(label).unwrap_or_else(|| panic!("no {label} on the ribbon: {:?}", d.texts()));
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    d.frame(wb, vec![]);
    d.click(wb, at);
    frames(d, wb);
}

/// A bracket: a plate 60 by 40 with a flange 20 high on its right-hand edge, seen from where its
/// top, the inside of its bend and its wall can be clicked. The plate is cut back to 55.
fn bracket(wb: &mut Workbench, d: &mut Driver) -> f64 {
    wb.exec("sheet.defaults", json!({ "thickness": T, "bend_radius": R, "k_factor": 0.44, "gap": 1 })).unwrap();
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].as_u64().unwrap();
    let r = wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 40 })).unwrap();
    let face = wb.exec("sheet.face", json!({ "sketch": sk })).unwrap()["feature"].as_u64().unwrap();
    let edge = wb
        .exec(
            "model.edge_ref",
            json!({ "faces": [{ "type": "cap", "feature": face, "end": "end" }, { "type": "side", "feature": face, "curve": r["lines"][1] }] }),
        )
        .unwrap();
    wb.exec("sheet.flange", json!({ "edges": [edge], "height": 20 })).unwrap();
    wb.chrome.tab = SHEET_TAB;
    frames(d, wb);
    wb.view.camera.look_along(Vec3::new(-1.0, -1.0, 1.5).normalized());
    frames(d, wb);
    volume(wb)
}

/// A point of the inside of the bracket's bend, half way round.
fn inside_of_bend() -> Vec3 {
    let c = FRAC_PI_2 / 2.0;
    Vec3::new(55.0 + R * c.sin(), 20.0, T + R - R * c.cos())
}

#[test]
fn rip_unfold_and_refold_are_there_to_be_used_and_say_what_they_need() {
    for id in ["sheet.rip", "sheet.unfold", "sheet.refold"] {
        assert!(find(id).is_some_and(|c| c.available()), "{id} is greyed");
    }
    assert!(find("sheet.rip").unwrap().tip.contains("closed shape can be laid flat"));
    assert!(find("sheet.unfold").unwrap().tip.contains("Lay chosen bends flat"));
    assert!(find("sheet.refold").unwrap().tip.contains("Fold back the bends"));
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    frames(&mut d, &mut wb);
    // On a part that is not sheet metal they say so; on a sheet metal part with no sheet, what
    // to do first.
    for id in ["sheet.rip", "sheet.unfold", "sheet.refold"] {
        assert!(wb.run_ui(id).unwrap_err().contains("not a sheet metal part yet"), "{id}");
    }
    wb.exec("sheet.defaults", json!({})).unwrap();
    frames(&mut d, &mut wb);
    assert!(wb.run_ui("sheet.rip").unwrap_err().contains("no sheet yet"));
    assert!(wb.run_ui("sheet.unfold").unwrap_err().contains("no sheet yet"));
    assert!(wb.run_ui("sheet.refold").unwrap_err().contains("nothing to fold back"));
}

#[test]
fn an_unfold_and_a_refold_are_set_up_by_clicks_and_edited_from_the_browser() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let folded = bracket(&mut wb, &mut d);
    let long = 55.0 + ALLOWANCE + 15.0;
    let flat_whole = long * 40.0 * T;

    // Unfold, from its button: nothing picked yet, and OK says what to do.
    press(&mut d, &mut wb, "Unfold");
    assert!(matches!(&wb.panel, Some(Panel::Unfold(p)) if p.face.is_none() && p.bends.is_empty() && !p.all), "{:?}", wb.status());
    assert!(wb.status().starts_with("Unfold: click the flat face"), "{}", wb.status());
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.status_error && wb.status().contains("click the flat face of the sheet that stays"), "{}", wb.status());

    // A click on the plate is what stays; nothing moves until a bend is picked too, and OK says so.
    click_on(&mut d, &mut wb, Vec3::new(10.0, 20.0, T));
    assert!(matches!(&wb.panel, Some(Panel::Unfold(p)) if p.face.is_some() && p.bends.is_empty()));
    assert!(near(volume(&wb), folded));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.status_error && wb.status().contains("round face of a bend"), "{}", wb.status());

    // A click on the bend, and the preview shows it flat, its allowance long.
    let p = on_screen(&wb, inside_of_bend());
    d.frame(&mut wb, vec![egui::Event::PointerMoved(p)]);
    d.frame(&mut wb, vec![]);
    assert!(matches!(wb.view.hover, Some(crate::viewport::Pick::Face { .. })), "the bend lights up: {:?}", wb.view.hover);
    d.click(&mut wb, p);
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Unfold(p)) if p.bends.len() == 1), "{}", wb.status());
    assert!(near(volume(&wb), flat_whole), "the preview: {} {flat_whole}", volume(&wb));
    assert!(wb.document().features().iter().all(|f| !matches!(f.kind, FeatureKind::Unfold(_))), "nothing is made until OK");

    // OK makes it: a row in the browser, the part partly flat.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let (unfold, name) = wb.document().features().last().map(|f| (f.id, f.name.clone())).unwrap();
    assert_eq!(name, "Unfold1");
    assert!(wb.status().starts_with("Made Unfold1"), "{}", wb.status());
    assert!(near(volume(&wb), flat_whole));
    let b = wb.scene().bbox().unwrap();
    assert!(near(b.max.x - b.min.x, long) && near(b.max.z - b.min.z, T), "{b:?}");

    // A double-click on its row opens its panel with what it has, and its preview; Esc leaves it.
    let row = browser_row(&d, "Unfold1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    let Some(Panel::Unfold(p)) = &wb.panel else { panic!("double-click edits the unfold: {}", wb.status()) };
    assert_eq!((p.editing, p.bends.len(), p.all), (Some(unfold), 1, false));
    d.tap(&mut wb, egui::Key::Escape);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none());
    assert!(near(volume(&wb), flat_whole));

    // Work across the flat bend: a hole, from its strip into the plate and the wall.
    let radius = 6.0;
    let top = wb.exec("model.face_ref", json!({ "at": [10, 20, T] })).unwrap();
    let sk = wb.exec("sketch.create", json!({ "face": top })).unwrap()["feature"].as_u64().unwrap();
    frames(&mut d, &mut wb);
    let frame = wb.sketch_frame(tenon_model::FeatureId(sk as u32)).unwrap();
    let at = frame.to_local(Vec3::new(55.0 + ALLOWANCE / 2.0, 20.0, T));
    wb.exec("sketch.circle", json!({ "sketch": sk, "cx": at.x, "cy": at.y, "r": radius })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": sk, "distance": 5, "reverse": true, "operation": "cut" })).unwrap();
    frames(&mut d, &mut wb);
    let circle = PI * radius * radius;
    assert!(near(volume(&wb), flat_whole - circle * T), "{}", volume(&wb));

    // Refold, from its button: all that is flat, at once, and the hole goes round the bend.
    press(&mut d, &mut wb, "Refold");
    let Some(Panel::Refold(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert!(p.all && p.unfold == unfold, "{p:?}");
    let half = ALLOWANCE / 2.0;
    let on_strip = 2.0 * (half * (radius * radius - half * half).sqrt() + radius * radius * (half / radius).asin());
    let refolded = folded - (circle - on_strip) * T - on_strip * T * (R + T / 2.0) / RHO;
    assert!(near(volume(&wb), refolded), "the preview: {} {refolded}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(wb.document().features().last().unwrap().name, "Refold1");
    assert!(wb.status().starts_with("Made Refold1"), "{}", wb.status());
    assert!(near(volume(&wb), refolded), "{} {refolded}", volume(&wb));
    let refold = wb.document().features().last().unwrap().id;

    // Its row opens it too. One undo takes it away, and redo puts it back.
    let row = browser_row(&d, "Refold1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    let Some(Panel::Refold(p)) = &wb.panel else { panic!("double-click edits the refold: {}", wb.status()) };
    assert_eq!((p.editing, p.all, p.unfold), (Some(refold), true, unfold));
    d.tap(&mut wb, egui::Key::Escape);
    frames(&mut d, &mut wb);
    // The unfold's row, with the refold after it: the view is the part just after the unfold.
    let row = browser_row(&d, "Unfold1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Unfold(p)) if p.editing == Some(unfold)), "{}", wb.status());
    assert!(near(volume(&wb), flat_whole), "{} {flat_whole}", volume(&wb));
    d.tap(&mut wb, egui::Key::Escape);
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), refolded));
    wb.run_ui("edit.undo").unwrap();
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), flat_whole - circle * T));
    wb.run_ui("edit.redo").unwrap();
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), refolded));
    // Suppressed from the document it is the flat sheet; so is it rolled back by its row.
    wb.exec("feature.suppress", json!({ "feature": refold.0 })).unwrap();
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), flat_whole - circle * T));
}

#[test]
fn a_refold_of_some_of_the_bends_is_clicked_on_their_flat_strips() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    bracket(&mut wb, &mut d);
    // A second flange, on the left-hand edge: a channel.
    let left = wb.exec("model.edge_ref", json!({ "at": [0, 20, T] })).unwrap();
    wb.exec("sheet.flange", json!({ "edges": [left], "height": 20 })).unwrap();
    frames(&mut d, &mut wb);
    let channel = volume(&wb);
    let whole_flat = (50.0 + 2.0 * (ALLOWANCE + 15.0)) * 40.0 * T;
    // Unfold with All Bends ticked: from the floor, with no bend clicked.
    press(&mut d, &mut wb, "Unfold");
    click_on(&mut d, &mut wb, Vec3::new(30.0, 20.0, T));
    d.click(&mut wb, pressable(&d, "tn_unfold_all"));
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), whole_flat), "{} {whole_flat}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let FeatureKind::Unfold(u) = &wb.document().features().last().unwrap().kind else { panic!() };
    assert!(u.bends.is_empty());

    // Refold, with All Bends off: the view is the flat sheet, and a click off a strip does
    // nothing, and on one picks it.
    wb.view.camera.look_along(Vec3::new(0.0, -0.2, 1.0).normalized());
    frames(&mut d, &mut wb);
    press(&mut d, &mut wb, "Refold");
    d.click(&mut wb, pressable(&d, "tn_refold_all"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Refold(p)) if !p.all && p.bends.is_empty()));
    assert!(near(volume(&wb), whole_flat), "as it was: {}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.status_error && wb.status().contains("click the flat strip"), "{}", wb.status());
    click_on(&mut d, &mut wb, Vec3::new(30.0, 20.0, T));
    assert!(matches!(&wb.panel, Some(Panel::Refold(p)) if p.bends.is_empty()), "the floor is no strip");
    click_on(&mut d, &mut wb, Vec3::new(55.0 + ALLOWANCE / 2.0, 20.0, T));
    assert!(matches!(&wb.panel, Some(Panel::Refold(p)) if p.bends.len() == 1), "{}", wb.status());
    let one_back = whole_flat - 40.0 * T * ALLOWANCE + FRAC_PI_2 / 2.0 * ((R + T) * (R + T) - R * R) * 40.0;
    assert!(near(volume(&wb), one_back), "{} {one_back}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!(near(volume(&wb), one_back));
    // A second refold of what is left brings back the channel.
    press(&mut d, &mut wb, "Refold");
    assert!(near(volume(&wb), channel), "{} {channel}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!(near(volume(&wb), channel));
    assert!(wb.scene().bodies[0].volume > 0.0);
}

/// A tube 50 long, 10 inside radius, 2 thick, along Z, seen from where its outside can be clicked.
fn tube(wb: &mut Workbench, d: &mut Driver) -> f64 {
    wb.exec("sheet.defaults", json!({ "thickness": T, "bend_radius": R, "k_factor": 0.44, "gap": 1 })).unwrap();
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].as_u64().unwrap();
    wb.exec("sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 10 })).unwrap();
    wb.exec("sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 12 })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": sk, "distance": 50 })).unwrap();
    wb.chrome.tab = SHEET_TAB;
    frames(d, wb);
    wb.view.camera.look_along(Vec3::new(0.2, -1.0, 0.2).normalized());
    frames(d, wb);
    volume(wb)
}

#[test]
fn a_rip_is_clicked_on_a_tube_and_lays_it_flat() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let whole = tube(&mut wb, &mut d);
    assert!(wb.exec("sheet.flat", json!({})).unwrap_err().contains("cannot be laid flat"));

    press(&mut d, &mut wb, "Rip");
    let Some(Panel::Rip(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert!(p.way == RipChoice::Point && p.face.is_none() && !p.own_gap && near(p.gap, 1.0), "{p:?}");
    assert!(wb.status().starts_with("Rip: click the point"), "{}", wb.status());
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.status_error && wb.status().contains("click the point on the face"), "{}", wb.status());

    // A click on the side of the tube: the slit goes through there, the preview shows it.
    let at = Vec3::new(0.0, -12.0, 25.0);
    let p = on_screen(&wb, at);
    d.frame(&mut wb, vec![egui::Event::PointerMoved(p)]);
    d.frame(&mut wb, vec![]);
    assert!(matches!(wb.view.hover, Some(crate::viewport::Pick::Face { .. })), "the face lights up: {:?}", wb.view.hover);
    d.click(&mut wb, p);
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Rip(p)) if p.face.is_some() && p.points.len() == 1), "{}", wb.status());
    let rho = 10.0 + 0.44 * T;
    assert!(near(volume(&wb), whole - 1.0 / rho / 2.0 * (144.0 - 100.0) * 50.0), "the preview: {}", volume(&wb));

    // A gap of its own, typed.
    d.click(&mut wb, pressable(&d, "tn_rip_by_rule"));
    frames(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_rip_gap_value", "2");
    frames(&mut d, &mut wb);
    assert!(near(volume(&wb), whole - 2.0 / rho / 2.0 * (144.0 - 100.0) * 50.0), "{}", volume(&wb));

    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let (id, name, kind) = wb.document().features().last().map(|f| (f.id, f.name.clone(), f.kind.clone())).unwrap();
    assert_eq!(name, "Rip1");
    let FeatureKind::Rip(r) = kind else { panic!("{kind:?}") };
    assert!(matches!(r.way, RipWay::Point { along: None, .. }) && r.gap == Some(2.0), "{r:?}");
    let f = wb.exec("sheet.flat", json!({})).unwrap();
    assert!(near(num(&f, "length"), TAU * rho - 2.0) && near(num(&f, "width"), 50.0), "{f}");

    // Its row opens it; a new gap there changes it.
    let row = browser_row(&d, "Rip1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    let Some(Panel::Rip(p)) = &wb.panel else { panic!("double-click edits the rip: {}", wb.status()) };
    assert!(p.editing == Some(id) && p.own_gap && p.way == RipChoice::Point && p.points.len() == 1, "{p:?}");
    type_into(&mut d, &mut wb, "tn_props_rip_gap_value", "3");
    frames(&mut d, &mut wb);
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let f = wb.exec("sheet.flat", json!({})).unwrap();
    assert!(near(num(&f, "length"), TAU * rho - 3.0), "{f}");
}

#[test]
fn a_rip_from_point_to_point_goes_through_the_edges_it_is_clicked_on() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    tube(&mut wb, &mut d);
    press(&mut d, &mut wb, "Rip");
    // Point to Point, from its list.
    d.click(&mut wb, pressable(&d, "tn_rip_way"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_rip_way_between"));
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Rip(p)) if p.way == RipChoice::Between));
    // Near the tube's two ends: each click is taken to be on the end, so the slit goes through it.
    click_on(&mut d, &mut wb, Vec3::new(0.0, -12.0, 0.2));
    assert!(
        matches!(&wb.panel, Some(Panel::Rip(p)) if p.points.len() == 1 && p.points[0].z.abs() < 1e-9),
        "{:?}",
        wb.panel.as_ref().map(|_| wb.status().to_owned())
    );
    assert!(wb.document().features().iter().all(|f| !matches!(f.kind, FeatureKind::Rip(_))));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.status_error && wb.status().contains("then the point where it ends"), "{}", wb.status());
    click_on(&mut d, &mut wb, Vec3::new(0.0, -12.0, 49.8));
    assert!(matches!(&wb.panel, Some(Panel::Rip(p)) if p.points.len() == 2 && (p.points[1].z - 50.0).abs() < 1e-9), "{}", wb.status());
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let FeatureKind::Rip(r) = &wb.document().features().last().unwrap().kind else { panic!() };
    assert!(matches!(r.way, RipWay::Between { .. }) && r.gap.is_none(), "{r:?}");
    let rho = 10.0 + 0.44 * T;
    let f = wb.exec("sheet.flat", json!({})).unwrap();
    assert!(near(num(&f, "length"), TAU * rho - 1.0) && near(num(&f, "width"), 50.0), "{f}");
}

#[test]
fn a_rip_takes_a_whole_face_out_and_a_slit_runs_alongside_an_edge_clicked() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let folded = bracket(&mut wb, &mut d);
    press(&mut d, &mut wb, "Rip");
    d.click(&mut wb, pressable(&d, "tn_rip_way"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_rip_way_face"));
    frames(&mut d, &mut wb);
    // The bend: it goes, and what it joined stands apart: two solids.
    click_on(&mut d, &mut wb, inside_of_bend());
    let gone = FRAC_PI_2 / 2.0 * ((R + T) * (R + T) - R * R) * 40.0;
    assert!(near(volume(&wb), folded - gone), "{} {}", volume(&wb), folded - gone);
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let FeatureKind::Rip(r) = &wb.document().features().last().unwrap().kind else { panic!() };
    assert!(matches!(r.way, RipWay::Face) && r.gap.is_none(), "{r:?}");
    wb.run_ui("edit.undo").unwrap();
    frames(&mut d, &mut wb);

    // A slit through a point of the plate: alongside the bend beside it, as it comes. Then a click
    // on an edge across it turns it to run alongside that.
    press(&mut d, &mut wb, "Rip");
    click_on(&mut d, &mut wb, Vec3::new(20.0, 20.0, T));
    let by_rule = 40.0 * 1.0 * T;
    assert!(near(volume(&wb), folded - by_rule), "{}", volume(&wb));
    click_on(&mut d, &mut wb, Vec3::new(30.0, 0.0, T));
    assert!(matches!(&wb.panel, Some(Panel::Rip(p)) if p.along.is_some()), "{}", wb.status());
    let across = 55.0 * 1.0 * T;
    assert!(near(volume(&wb), folded - across), "{} {}", volume(&wb), folded - across);
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let row = browser_row(&d, "Rip1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Rip(p)) if p.along.is_some() && p.editing.is_some()), "{}", wb.status());
}
