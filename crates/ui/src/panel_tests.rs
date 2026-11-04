//! A feature's panel opened again: OK with nothing touched changes nothing, and what is typed
//! takes.

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::{CapEnd, FaceOrigin, Feature, FeatureId, FeatureKind};

use crate::Workbench;
use crate::panels::{Fit, Panel};
use crate::tests::{Driver, pressable, type_into};
use crate::viewport::Pick;
use crate::workbench::Mode;

fn sketching(wb: &Workbench) -> FeatureId {
    match &wb.mode {
        Mode::Sketch(s) => s.feature,
        Mode::Model => panic!("not in sketch mode"),
    }
}

/// A block 40 x 20 x 10, and a sketch on it of a point at (20, 10).
fn block_with_a_point() -> (Workbench, Driver, FeatureId) {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let d = Driver::new(egui::vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let sk = sketching(&wb);
    wb.exec("sketch.point", json!({ "sketch": sk.0, "x": 20, "y": 10 })).unwrap();
    wb.finish_sketch();
    (wb, d, sk)
}

/// The last feature, as it is.
fn last(wb: &Workbench) -> &Feature {
    wb.document().features().last().unwrap()
}

/// The part file's text as it stands.
fn saved(wb: &Workbench) -> String {
    String::from_utf8(tenon_io::project::to_bytes(wb.document(), &serde_json::Map::new()).unwrap()).unwrap()
}

/// Opens the last feature from the model, as a double-click on its row does.
fn reopen(d: &mut Driver, wb: &mut Workbench) {
    let id = last(wb).id;
    wb.edit_feature(id).unwrap();
    d.frame(wb, vec![]);
    d.frame(wb, vec![]);
    assert!(wb.panel.is_some(), "{}", wb.status());
}

/// Presses OK on the panel, and says that it closed.
fn ok(d: &mut Driver, wb: &mut Workbench) {
    d.tap(wb, egui::Key::Enter);
    d.frame(wb, vec![]);
    d.settle(wb);
    assert!(wb.panel.is_none(), "the panel stayed open: {}", wb.status());
    assert!(!wb.status_error, "{}", wb.status());
}

/// Opens the last feature, says every value the panel sends is a parameter of it, presses OK with
/// nothing touched, and says the file is as it was.
fn ok_changes_nothing(d: &mut Driver, wb: &mut Workbench) {
    let before = saved(wb);
    reopen(d, wb);
    let kind = last(wb).kind.clone();
    let json = serde_json::to_value(&kind).unwrap();
    let sent = wb.panel.as_ref().unwrap().eq_pointers();
    assert!(!sent.is_empty());
    for (_, pointer) in sent {
        if json.pointer(pointer).is_some() {
            assert!(tenon_model::params::field_unit(&kind, pointer).is_some(), "{pointer} is sent, and is not a parameter");
        }
    }
    ok(d, wb);
    assert_eq!(saved(wb), before, "OK with nothing touched changed the file");
}

/// A value of the last feature, by its pointer.
fn value_of(wb: &Workbench, pointer: &str) -> f64 {
    serde_json::to_value(&last(wb).kind).unwrap().pointer(pointer).and_then(Value::as_f64).unwrap_or_else(|| panic!("no {pointer}"))
}

#[test]
fn a_spotface_hole_opens_again_and_takes_what_is_typed() {
    let (mut wb, mut d, sk) = block_with_a_point();
    let asked = json!({ "sketch": sk.0, "diameter": 6, "depth": 5, "reverse": true, "type": "spotface", "spotface_diameter": 12, "spotface_depth": 1 });
    wb.exec("model.hole", asked).unwrap();
    d.settle(&mut wb);
    ok_changes_nothing(&mut d, &mut wb);
    reopen(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_seat_dia", "16");
    type_into(&mut d, &mut wb, "tn_props_bore_depth", "2");
    ok(&mut d, &mut wb);
    assert_eq!((value_of(&wb, "/kind/spotface/diameter"), value_of(&wb, "/kind/spotface/depth")), (16.0, 2.0));
}

#[test]
fn a_thread_depth_opens_again_and_takes_what_is_typed_and_an_equation() {
    let (mut wb, mut d, sk) = block_with_a_point();
    wb.exec("param.add", json!({ "name": "q", "equation": "3" })).unwrap();
    let asked = json!({ "sketch": sk.0, "diameter": 6.8, "depth": 8, "reverse": true, "tapped": true, "thread_length": 6, "equations": { "/tapped/length/distance": "q * 2" } });
    wb.exec("model.hole", asked).unwrap();
    d.settle(&mut wb);
    assert_eq!(value_of(&wb, "/tapped/length/distance"), 6.0);
    wb.exec("param.set", json!({ "name": "q", "equation": "2" })).unwrap();
    d.settle(&mut wb);
    assert_eq!(value_of(&wb, "/tapped/length/distance"), 4.0, "it goes by the equation");
    ok_changes_nothing(&mut d, &mut wb);
    reopen(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_thread_depth", "7");
    ok(&mut d, &mut wb);
    assert_eq!(value_of(&wb, "/tapped/length/distance"), 7.0);
}

#[test]
fn a_spotface_depth_takes_an_equation_from_a_script() {
    let (mut wb, mut d, sk) = block_with_a_point();
    wb.exec("param.add", json!({ "name": "q", "equation": "3" })).unwrap();
    let asked = json!({ "sketch": sk.0, "diameter": 6, "depth": 5, "reverse": true, "type": "spotface", "spotface_diameter": 12, "spotface_depth": 1, "equations": { "/kind/spotface/depth": "q / 3" } });
    wb.exec("model.hole", asked).unwrap();
    d.settle(&mut wb);
    assert_eq!(value_of(&wb, "/kind/spotface/depth"), 1.0);
}

#[test]
fn a_variable_fillet_opens_again_and_takes_what_is_typed() {
    let (mut wb, mut d, _) = block_with_a_point();
    let edges = wb.exec("model.edges", json!({})).unwrap();
    let edge = wb.exec("model.edge_ref", json!({ "faces": edges["edges"][0]["faces"].clone() })).unwrap();
    wb.exec("model.fillet", json!({ "edges": [edge], "radius": 2, "end_radius": 4 })).unwrap();
    d.settle(&mut wb);
    ok_changes_nothing(&mut d, &mut wb);
    reopen(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_end_radius", "3");
    ok(&mut d, &mut wb);
    assert_eq!(value_of(&wb, "/radii/0/radius"), 3.0);
}

#[test]
fn an_asymmetric_revolution_opens_again_and_takes_what_is_typed() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(egui::vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 10, "y1": 0, "x2": 20, "y2": 10 })).unwrap();
    wb.finish_sketch();
    let (quarter, eighth) = (std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_4);
    wb.exec("model.revolve", json!({ "sketch": f.0, "axis": "y", "angle": quarter, "backward": eighth })).unwrap();
    d.settle(&mut wb);
    reopen(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_angle", "60+30");
    type_into(&mut d, &mut wb, "tn_props_angle_b", "30");
    ok(&mut d, &mut wb);
    assert!((value_of(&wb, "/angle/two_sided/forward") - quarter).abs() < 1e-12);
    assert!((value_of(&wb, "/angle/two_sided/backward") - 30f64.to_radians()).abs() < 1e-12);
}

fn tapping(wb: &Workbench) -> Option<tenon_model::Tapping> {
    match &last(wb).kind {
        FeatureKind::Hole(h) => h.tapped.clone(),
        k => panic!("not a hole: {k:?}"),
    }
}

fn threads(wb: &Workbench) -> Vec<String> {
    wb.scene().threads.iter().map(|t| t.mark.designation.clone()).collect()
}

fn screw(wb: &Workbench) -> Option<(usize, Fit)> {
    match &wb.panel {
        Some(Panel::Hole(p)) => p.screw,
        _ => panic!("no hole panel: {}", wb.status()),
    }
}

/// Opens a list by what it shows and chooses from it.
fn choose(d: &mut Driver, wb: &mut Workbench, shown: &str, wanted: &str) {
    let at = d.text_pos(shown).unwrap_or_else(|| panic!("no {shown}: {:?}", d.texts()));
    d.click(wb, at);
    d.frame(wb, vec![]);
    let at = d.text_pos(wanted).unwrap_or_else(|| panic!("no {wanted} in the list: {:?}", d.texts()));
    d.click(wb, at);
    d.frame(wb, vec![]);
    d.frame(wb, vec![]);
}

#[test]
fn a_plain_hole_as_wide_as_a_tap_drill_stays_plain_until_tapped_is_chosen() {
    let (mut wb, mut d, sk) = block_with_a_point();
    wb.exec("model.hole", json!({ "sketch": sk.0, "diameter": 5, "depth": 8, "reverse": true })).unwrap();
    d.settle(&mut wb);
    ok_changes_nothing(&mut d, &mut wb);
    assert_eq!(tapping(&wb), None);
    assert!(threads(&wb).is_empty());

    // It says Simple. Tapped is the M6's, whose tap drill it is: its size stays.
    reopen(&mut d, &mut wb);
    assert_eq!(screw(&wb), None);
    choose(&mut d, &mut wb, "Simple", "Tapped");
    ok(&mut d, &mut wb);
    assert!(tapping(&wb).is_some());
    assert_eq!(value_of(&wb, "/diameter"), 5.0);
    assert_eq!(threads(&wb), ["M6x1"]);
}

#[test]
fn a_tap_drill_with_its_own_thread_feature_does_not_gain_a_second() {
    let (mut wb, mut d, sk) = block_with_a_point();
    wb.exec("model.hole", json!({ "sketch": sk.0, "diameter": 6.8, "depth": 8, "reverse": true })).unwrap();
    d.settle(&mut wb);
    let hole = last(&wb).id;
    let wall = wb.scene().bodies[0].faces.iter().find(|(_, f)| matches!(f.surface, tenon_kernel::SurfaceKind::Cylinder { .. })).unwrap().0.clone();
    let face = wb.exec("model.face_ref", json!({ "origin": wall })).unwrap();
    wb.exec("model.thread", json!({ "face": face })).unwrap();
    d.settle(&mut wb);
    assert_eq!(threads(&wb), ["M8x1.25"]);
    let before = saved(&wb);
    wb.edit_feature(hole).unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    ok(&mut d, &mut wb);
    assert_eq!(saved(&wb), before);
    assert_eq!(threads(&wb), ["M8x1.25"]);
}

#[test]
fn a_scripts_thread_is_kept_through_ok_and_a_new_depth_and_let_go_for_another_screw() {
    let (mut wb, mut d, sk) = block_with_a_point();
    let asked = json!({ "sketch": sk.0, "diameter": 6.8, "depth": 8, "reverse": true, "tapped": true, "pitch": 1, "designation": "M8x1 LH", "left": true });
    wb.exec("model.hole", asked).unwrap();
    d.settle(&mut wb);
    ok_changes_nothing(&mut d, &mut wb);
    assert_eq!(threads(&wb), ["M8x1 LH"]);

    // It says what it is, and a depth changes the depth and no more.
    reopen(&mut d, &mut wb);
    assert!(d.text_pos("M8x1 LH").is_some(), "{:?}", d.texts());
    d.click(&mut wb, pressable(&d, "tn_hole_thread_full"));
    d.frame(&mut wb, vec![]);
    type_into(&mut d, &mut wb, "tn_props_thread_depth", "5");
    ok(&mut d, &mut wb);
    let t = tapping(&wb).unwrap();
    assert_eq!((t.pitch, t.designation.as_deref(), t.left), (Some(1.0), Some("M8x1 LH"), true));
    assert_eq!(t.length, tenon_model::ThreadLength::Distance(5.0));

    // Another screw is another thread: the one it is named for no longer fits.
    reopen(&mut d, &mut wb);
    choose(&mut d, &mut wb, "M8", "M10");
    assert!(d.text_pos("M10x1.5").is_some(), "{:?}", d.texts());
    ok(&mut d, &mut wb);
    let t = tapping(&wb).unwrap();
    assert_eq!((t.pitch, t.designation.as_deref(), t.left), (None, None, false));
}

#[test]
fn a_thread_on_a_size_of_its_own_opens_as_tapped_and_keeps_both() {
    let (mut wb, mut d, sk) = block_with_a_point();
    let asked = json!({ "sketch": sk.0, "diameter": 7, "depth": 8, "reverse": true, "tapped": true, "pitch": 1, "designation": "M8x1" });
    wb.exec("model.hole", asked).unwrap();
    d.settle(&mut wb);
    ok_changes_nothing(&mut d, &mut wb);
    reopen(&mut d, &mut wb);
    assert!(matches!(screw(&wb), Some((_, Fit::TapDrill))));
    d.tap(&mut wb, egui::Key::Escape);
    assert_eq!(threads(&wb), ["M8x1"]);
}

/// A block 80 x 40 x 10, in a new sketch on its top.
fn block_and_a_sketch_on_top() -> (Workbench, Driver, FeatureId) {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(egui::vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 80, "y2": 40 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    d.frame(&mut wb, vec![]);
    let top = wb.scene().bodies[0].faces.iter().position(|(n, _)| matches!(n, Some(FaceOrigin::Cap { end: CapEnd::End, .. }))).unwrap() as u32;
    wb.view.selection = vec![Pick::Face { body: 0, face: top }];
    wb.run_ui("sketch.new").unwrap();
    let sk = sketching(&wb);
    (wb, d, sk)
}

fn volume(wb: &Workbench) -> f64 {
    wb.scene().bodies.iter().map(|b| b.volume).sum()
}

#[test]
fn text_alone_on_a_face_is_its_letters_and_not_the_face() {
    // How much the letters come to, on a plane of their own.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(egui::vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    let words = json!({ "sketch": f.0, "x": 10, "y": 10, "text": "TENON", "height": 10 });
    wb.exec("sketch.text", words).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 2 })).unwrap();
    d.settle(&mut wb);
    let letters = volume(&wb);
    assert!(letters > 100.0 && letters < 2000.0, "{letters}");

    // On a face. The panel says five letters, and the preview raises them and not the face.
    let (mut wb, mut d, sk) = block_and_a_sketch_on_top();
    let words = json!({ "sketch": sk.0, "x": 10, "y": 10, "text": "TENON", "height": 10 });
    wb.exec("sketch.text", words).unwrap();
    wb.finish_sketch();
    d.settle(&mut wb);
    let block = volume(&wb);
    d.tap(&mut wb, egui::Key::E);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(d.text_pos("5 selected").is_some(), "{:?}", d.texts());
    d.tap(&mut wb, egui::Key::Escape);
    d.settle(&mut wb);

    // Cut into the block: the letters alone are gone.
    wb.exec("model.extrude", json!({ "sketch": sk.0, "distance": 2, "operation": "cut", "reverse": true })).unwrap();
    d.settle(&mut wb);
    assert!((block - volume(&wb) - letters).abs() < 1.0, "removed {} where the letters are {letters}", block - volume(&wb));
}
