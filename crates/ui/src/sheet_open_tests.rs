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

#[test]
fn what_cannot_be_folded_back_says_so_in_the_app() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    bracket(&mut wb, &mut d);
    let top = wb.exec("model.face_ref", json!({ "at": [10, 20, T] })).unwrap();
    let bend = wb.exec("model.face_ref", json!({ "at": [inside_of_bend().x, inside_of_bend().y, inside_of_bend().z] })).unwrap();
    wb.exec("sheet.unfold", json!({ "face": top.clone(), "bends": [bend] })).unwrap();
    // A pocket in the strip, a millimetre into a sheet two thick.
    let sk = wb.exec("sketch.create", json!({ "face": top })).unwrap()["feature"].as_u64().unwrap();
    frames(&mut d, &mut wb);
    let frame = wb.sketch_frame(tenon_model::FeatureId(sk as u32)).unwrap();
    let (a, b) = (frame.to_local(Vec3::new(55.5, 10.0, T)), frame.to_local(Vec3::new(56.5, 30.0, T)));
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": a.x, "y1": a.y, "x2": b.x, "y2": b.y })).unwrap();
    wb.exec("model.extrude", json!({ "sketch": sk, "distance": 1, "reverse": true, "operation": "cut" })).unwrap();
    frames(&mut d, &mut wb);
    press(&mut d, &mut wb, "Refold");
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    // The panel stays, and says why, as on any panel whose preview has failed.
    assert!(matches!(wb.panel, Some(Panel::Refold(_))), "{}", wb.status());
    assert!(wb.status_error && wb.status().contains("not sheet of one thickness") && wb.status().contains("Make the cut go through"), "{}", wb.status());
    assert!(!wb.document().features().iter().any(|f| matches!(f.kind, tenon_model::FeatureKind::Refold(_))), "nothing is made");
    let said: Vec<String> = wb
        .scene()
        .status
        .iter()
        .filter_map(|(_, s)| match s {
            tenon_model::FeatureStatus::Error { message } => Some(message.clone()),
            _ => None,
        })
        .collect();
    assert!(said.len() == 1 && said[0].contains("not sheet of one thickness") && said[0].contains("Make the cut go through"), "{said:?}");
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

/// A box 60 by 40 and 30 high with its top and bottom out and its corners made bends: a closed
/// ring of sheet, seen from the front.
fn ring(wb: &mut Workbench, d: &mut Driver) -> f64 {
    let sk = wb.exec("sketch.create", json!({ "plane": "xy" })).unwrap()["feature"].as_u64().unwrap();
    wb.exec("sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 40 })).unwrap();
    let block = wb.exec("model.extrude", json!({ "sketch": sk, "distance": 30 })).unwrap()["feature"].as_u64().unwrap();
    let cap = |e: &str| json!({ "type": "cap", "feature": block, "end": e });
    let (top, bottom) =
        (wb.exec("model.face_ref", json!({ "origin": cap("end") })).unwrap(), wb.exec("model.face_ref", json!({ "origin": cap("start") })).unwrap());
    wb.exec("model.shell", json!({ "remove": [top, bottom], "thickness": T })).unwrap();
    wb.exec("sheet.defaults", json!({ "thickness": T, "bend_radius": R, "k_factor": 0.44, "gap": 1 })).unwrap();
    let corners: Vec<Value> = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)]
        .iter()
        .map(|(x, y)| wb.exec("model.edge_ref", json!({ "at": [x, y, 15] })).unwrap())
        .collect();
    wb.exec("sheet.sharp_bend", json!({ "edges": corners })).unwrap();
    wb.chrome.tab = SHEET_TAB;
    frames(d, wb);
    wb.view.camera.look_along(Vec3::new(0.3, -1.0, 0.3).normalized());
    frames(d, wb);
    volume(wb)
}

/// Half way round the corner of the ring at the origin, on its outside.
fn outside_of_corner() -> Vec3 {
    let c = FRAC_PI_2 / 2.0;
    Vec3::new(5.0 - (R + T) * c.sin(), 5.0 - (R + T) * c.cos(), 15.0)
}

#[test]
fn a_rip_takes_a_whole_face_out_and_a_slit_runs_alongside_an_edge_clicked() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let whole = ring(&mut wb, &mut d);
    press(&mut d, &mut wb, "Rip");
    d.click(&mut wb, pressable(&d, "tn_rip_way"));
    frames(&mut d, &mut wb);
    d.click(&mut wb, pressable(&d, "tn_rip_way_face"));
    frames(&mut d, &mut wb);
    // A corner of the ring: it goes, and the ring is open there.
    click_on(&mut d, &mut wb, outside_of_corner());
    let gone = FRAC_PI_2 / 2.0 * ((R + T) * (R + T) - R * R) * 30.0;
    assert!(near(volume(&wb), whole - gone), "{} {}", volume(&wb), whole - gone);
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let FeatureKind::Rip(r) = &wb.document().features().last().unwrap().kind else { panic!() };
    assert!(matches!(r.way, RipWay::Face) && r.gap.is_none(), "{r:?}");
    wb.run_ui("edit.undo").unwrap();
    frames(&mut d, &mut wb);

    // On a bracket a slit through a point of the plate, alongside the bend beside it as it comes,
    // would cut the plate's end off: the preview says so, and the part stays whole. A click on an
    // edge across it turns it to run alongside that, which leaves one piece.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let folded = bracket(&mut wb, &mut d);
    press(&mut d, &mut wb, "Rip");
    click_on(&mut d, &mut wb, Vec3::new(20.0, 20.0, T));
    assert!(wb.status_error && wb.status().contains("would cut it in two") && wb.status().contains("straight edge"), "{}", wb.status());
    assert!(near(volume(&wb), folded), "{}", volume(&wb));
    click_on(&mut d, &mut wb, Vec3::new(10.0, 0.0, T));
    assert!(matches!(&wb.panel, Some(Panel::Rip(p)) if p.along.is_some()), "{}", wb.status());
    let across = 55.0 * 1.0 * T;
    assert!(near(volume(&wb), folded - across), "{} {}", volume(&wb), folded - across);
    assert!(!wb.status_error, "{}", wb.status());
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert!(wb.exec("sheet.flat", json!({})).is_ok());
    let row = browser_row(&d, "Rip1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Rip(p)) if p.along.is_some() && p.editing.is_some()), "{}", wb.status());
}

#[test]
fn a_rip_with_an_unfold_after_it_is_edited_on_the_sheet_as_it_was() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    let whole = ring(&mut wb, &mut d);
    let (front, slit) = (Vec3::new(30.0, 0.0, 15.0), 1.0 * T * 30.0);
    press(&mut d, &mut wb, "Rip");
    click_on(&mut d, &mut wb, front);
    assert!(near(volume(&wb), whole - slit), "{}", volume(&wb));
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let rip = wb.document().features().last().unwrap().id;
    // Everything laid flat after it, from the back wall.
    let back = wb.exec("model.face_ref", json!({ "at": [30, 40, 15] })).unwrap();
    wb.exec("sheet.unfold", json!({ "face": back })).unwrap();
    frames(&mut d, &mut wb);
    let long = 2.0 * (50.0 + 30.0) + 4.0 * ALLOWANCE - 1.0;
    let laid_flat = long * 30.0 * T;
    assert!(near(volume(&wb), laid_flat), "{}", volume(&wb));

    // Its row: the view is the ring with its slit, where what was picked still is.
    let row = browser_row(&d, "Rip1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Rip(p)) if p.editing == Some(rip)), "{}", wb.status());
    assert!(near(volume(&wb), whole - slit), "not the flat sheet: {}", volume(&wb));
    let b = wb.scene().bbox().unwrap();
    assert!(near(b.max.x - b.min.x, 60.0) && near(b.max.y - b.min.y, 40.0), "{b:?}");
    assert!(!wb.status_error, "{}", wb.status());

    // A click further along the wall moves the slit there.
    wb.view.camera.look_along(Vec3::new(0.3, -1.0, 0.3).normalized());
    frames(&mut d, &mut wb);
    click_on(&mut d, &mut wb, Vec3::new(40.0, 0.0, 15.0));
    let Some(Panel::Rip(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert!((p.points[0].x - 40.0).abs() < 0.1, "{:?}", p.points);
    assert!(wb.document().feature(rip).is_some() && !wb.status_error, "{}", wb.status());
    assert!(near(volume(&wb), whole - slit), "{}", volume(&wb));

    // OK: the rip is there, and the unfold after it is worked out on it, in the same length.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let FeatureKind::Rip(r) = &wb.document().feature(rip).unwrap().kind else { panic!() };
    assert!(matches!(&r.way, RipWay::Point { at, .. } if (at.x - 40.0).abs() < 0.1), "{r:?}");
    assert!(near(volume(&wb), laid_flat), "{} {laid_flat}", volume(&wb));
    assert!(!wb.status_error, "{}", wb.status());
}

#[test]
fn the_strip_an_unfold_shows_is_nothing_to_pick_and_it_says_so() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    bracket(&mut wb, &mut d);
    let flat_whole = (55.0 + ALLOWANCE + 15.0) * 40.0 * T;
    press(&mut d, &mut wb, "Unfold");
    click_on(&mut d, &mut wb, Vec3::new(10.0, 20.0, T));
    click_on(&mut d, &mut wb, inside_of_bend());
    assert!(near(volume(&wb), flat_whole), "{}", volume(&wb));
    let picked = |wb: &Workbench| match &wb.panel {
        Some(Panel::Unfold(p)) => (p.face.clone(), p.bends.clone()),
        _ => panic!("the panel has gone: {}", wb.status()),
    };
    let was = picked(&wb);
    // The strip, in the preview: nothing lights up, nothing changes, and it says why.
    let at = on_screen(&wb, Vec3::new(55.0 + ALLOWANCE / 2.0, 20.0, T));
    d.frame(&mut wb, vec![egui::Event::PointerMoved(at)]);
    d.frame(&mut wb, vec![]);
    assert!(wb.view.hover.is_none(), "{:?}", wb.view.hover);
    d.click(&mut wb, at);
    frames(&mut d, &mut wb);
    assert_eq!(picked(&wb), was);
    assert!(wb.status_error && wb.status().contains("what this unfold makes"), "{}", wb.status());
    assert!(near(volume(&wb), flat_whole), "the preview stays: {}", volume(&wb));
    // And OK makes it as it was set up.
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    assert_eq!(wb.document().features().last().unwrap().name, "Unfold1");
    assert!(near(volume(&wb), flat_whole));

    // The same, from its row: the faces it makes are nothing to it there either.
    let row = browser_row(&d, "Unfold1");
    double_click(&mut d, &mut wb, row.center());
    frames(&mut d, &mut wb);
    assert!(matches!(&wb.panel, Some(Panel::Unfold(p)) if p.editing.is_some()), "{}", wb.status());
    let was = picked(&wb);
    click_on(&mut d, &mut wb, Vec3::new(55.0 + ALLOWANCE / 2.0, 20.0, T));
    assert_eq!(picked(&wb), was);
    assert!(wb.status_error && wb.status().contains("what this unfold makes"), "{}", wb.status());
    d.tap(&mut wb, egui::Key::Escape);
}

#[test]
fn an_edge_is_nothing_to_a_slit_in_a_round_face() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    bracket(&mut wb, &mut d);
    press(&mut d, &mut wb, "Rip");
    click_on(&mut d, &mut wb, inside_of_bend());
    assert!(matches!(&wb.panel, Some(Panel::Rip(p)) if p.face.as_ref().is_some_and(|f| f.fingerprint.surface == "cylinder")));
    // (A slit right along an open bracket's bend would part it, and says so; what is to be seen here
    // is that a click on an edge takes up nothing.)
    assert!(wb.status_error && wb.status().contains("would cut it in two"), "{}", wb.status());
    // A click on the plate's edge: that is a click on the plate, and no edge is taken up.
    click_on(&mut d, &mut wb, Vec3::new(10.0, 0.0, T));
    let Some(Panel::Rip(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert!(p.along.is_none(), "{p:?}");
    assert!(p.face.as_ref().is_some_and(|f| f.fingerprint.surface == "plane"), "{p:?}");
    assert!(d.texts().iter().any(|t| t == "Alongside"), "a flat face takes an edge");
    // Back on the bend the row for one is gone.
    click_on(&mut d, &mut wb, inside_of_bend());
    assert!(!d.texts().iter().any(|t| t == "Alongside"), "{:?}", d.texts());
}

#[test]
fn refold_opens_on_the_unfold_that_still_has_a_bend_flat() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(vec2(1400.0, 860.0));
    bracket(&mut wb, &mut d);
    let left = wb.exec("model.edge_ref", json!({ "at": [0, 20, T] })).unwrap();
    wb.exec("sheet.flange", json!({ "edges": [left], "height": 20 })).unwrap();
    frames(&mut d, &mut wb);
    let floor = wb.exec("model.face_ref", json!({ "at": [30, 20, T] })).unwrap();
    let c = FRAC_PI_2 / 2.0;
    let right = wb.exec("model.face_ref", json!({ "at": [55.0 + R * c.sin(), 20.0, T + R - R * c.cos()] })).unwrap();
    let left = wb.exec("model.face_ref", json!({ "at": [5.0 - R * c.sin(), 20.0, T + R - R * c.cos()] })).unwrap();
    let first = wb.exec("sheet.unfold", json!({ "face": floor.clone(), "bends": [right] })).unwrap()["feature"].as_u64().unwrap();
    let second = wb.exec("sheet.unfold", json!({ "face": floor, "bends": [left] })).unwrap()["feature"].as_u64().unwrap();
    frames(&mut d, &mut wb);
    // The second folded back by naming its one strip.
    let strip = wb.exec("model.face_ref", json!({ "at": [5.0 - ALLOWANCE / 2.0, 20.0, T] })).unwrap();
    wb.exec("sheet.refold", json!({ "unfold": second, "bends": [strip] })).unwrap();
    frames(&mut d, &mut wb);
    // The button: on the first, not on the one with nothing left, and no red words.
    press(&mut d, &mut wb, "Refold");
    let Some(Panel::Refold(p)) = &wb.panel else { panic!("{}", wb.status()) };
    assert_eq!(u64::from(p.unfold.0), first, "{p:?}");
    assert!(!wb.status_error, "{}", wb.status());
    d.tap(&mut wb, egui::Key::Enter);
    frames(&mut d, &mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let flat_left = wb.exec("sheet.flat", json!({})).unwrap();
    assert_eq!(flat_left["bends"].as_array().unwrap().len(), 2, "{flat_left}");
}
