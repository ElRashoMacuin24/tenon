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
    let asked =
        json!({ "sketch": sk.0, "diameter": 6, "depth": 5, "reverse": true, "type": "spotface", "spotface_diameter": 12, "spotface_depth": 1 });
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
    let wall = wb.scene().bodies[0].faces.iter().find(|(_, f)| matches!(f.surface, tenon_kernel::SurfaceKind::Cylinder { .. })).unwrap().0;
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
    let asked =
        json!({ "sketch": sk.0, "diameter": 6.8, "depth": 8, "reverse": true, "tapped": true, "pitch": 1, "designation": "M8x1 LH", "left": true });
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

/// Clicks a point of the part in the view.
fn click_on_part(d: &mut Driver, wb: &mut Workbench, q: tenon_geom::Vec3) {
    let at = crate::tests::on_screen(wb, q);
    d.frame(wb, vec![egui::Event::PointerMoved(at)]);
    d.click(wb, at);
    d.settle(wb);
}

#[test]
fn holes_are_placed_on_a_face_one_after_another() {
    use tenon_geom::Vec3;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(egui::vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 80, "y2": 40 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10 })).unwrap();
    d.settle(&mut wb);
    let block = volume(&wb);

    // The first, by H and a click, and OK.
    d.tap(&mut wb, egui::Key::H);
    d.frame(&mut wb, vec![]);
    assert!(wb.status().contains("click a flat face"), "{}", wb.status());
    click_on_part(&mut d, &mut wb, Vec3::new(20.0, 20.0, 10.0));
    ok(&mut d, &mut wb);
    let one = block - volume(&wb);
    assert!(one > 100.0, "{one}");

    // The second the same way: the first hole's sketch is not offered again.
    d.tap(&mut wb, egui::Key::H);
    d.frame(&mut wb, vec![]);
    assert!(wb.status().contains("click a flat face"), "{}", wb.status());
    let Some(Panel::Hole(p)) = wb.panel.clone() else { panic!("no hole panel: {}", wb.status()) };
    assert!(p.on_face() && p.points.is_empty());
    click_on_part(&mut d, &mut wb, Vec3::new(60.0, 20.0, 10.0));
    ok(&mut d, &mut wb);
    assert!((block - volume(&wb) - 2.0 * one).abs() < 1e-6, "{}", block - volume(&wb));

    // A sketch of centres drawn for it is still what H takes.
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let sk = sketching(&wb);
    wb.exec("sketch.point", json!({ "sketch": sk.0, "x": 40, "y": 10 })).unwrap();
    wb.finish_sketch();
    d.settle(&mut wb);
    d.tap(&mut wb, egui::Key::H);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Hole(p)) = wb.panel.clone() else { panic!("no hole panel: {}", wb.status()) };
    assert_eq!((p.sketch, p.points.len()), (sk, 1));
    d.tap(&mut wb, egui::Key::Escape);
}

/// A rectangle on the XY plane extruded 10, and a circle in a sketch on its top, as a person has
/// them when pressing E on the second.
fn block_and_a_circle_on_top() -> (Workbench, Driver, FeatureId) {
    let (mut wb, mut d, sk) = block_and_a_sketch_on_top();
    wb.exec("sketch.circle", json!({ "sketch": sk.0, "cx": 20, "cy": 20, "r": 5 })).unwrap();
    wb.finish_sketch();
    d.settle(&mut wb);
    (wb, d, sk)
}

fn extrude_panel(wb: &mut Workbench) -> &mut crate::panels::ExtrudePanel {
    match &mut wb.panel {
        Some(Panel::Extrude(p)) => p,
        _ => panic!("no extrude panel"),
    }
}

fn presses_ok_and_is_refused(d: &mut Driver, wb: &mut Workbench, words: &str) {
    let before = wb.document().features().len();
    d.tap(wb, egui::Key::Enter);
    d.frame(wb, vec![]);
    d.settle(wb);
    assert!(wb.panel.is_some(), "the panel closed: {}", wb.status());
    assert!(wb.status_error && wb.status().contains(words), "{}", wb.status());
    assert_eq!(wb.document().features().len(), before, "something was made");
}

#[test]
fn ok_on_to_with_nothing_picked_asks_for_the_plane() {
    use crate::panels::ExtentChoice;
    // Extrude To.
    let (mut wb, mut d, _) = block_and_a_circle_on_top();
    d.tap(&mut wb, egui::Key::E);
    d.frame(&mut wb, vec![]);
    extrude_panel(&mut wb).extent = ExtentChoice::To;
    d.frame(&mut wb, vec![]);
    presses_ok_and_is_refused(&mut d, &mut wb, "Click the face or plane it goes to first");
    // Between, with where it starts and not where it goes.
    extrude_panel(&mut wb).extent = ExtentChoice::Between;
    extrude_panel(&mut wb).from = Some(tenon_model::PlaneRef::Origin(tenon_model::OriginPlane::XY));
    d.frame(&mut wb, vec![]);
    presses_ok_and_is_refused(&mut d, &mut wb, "Click the faces or planes it goes from and to first");
    d.tap(&mut wb, egui::Key::Escape);

    // Hole To.
    let (mut wb, mut d, sk) = block_with_a_point();
    d.settle(&mut wb);
    d.tap(&mut wb, egui::Key::H);
    d.frame(&mut wb, vec![]);
    let Some(Panel::Hole(p)) = &mut wb.panel else { panic!("no hole panel") };
    assert_eq!(p.sketch, sk);
    p.to_plane = true;
    d.frame(&mut wb, vec![]);
    presses_ok_and_is_refused(&mut d, &mut wb, "Click the face or plane it goes to first");
    d.tap(&mut wb, egui::Key::Escape);

    // Revolve To.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(egui::vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 10, "y1": 0, "x2": 20, "y2": 10 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.revolve", json!({ "sketch": f.0, "axis": "y", "angle": 1.0 })).unwrap();
    d.settle(&mut wb);
    wb.edit_feature(last(&wb).id).unwrap();
    d.frame(&mut wb, vec![]);
    let Some(Panel::Revolve(p)) = &mut wb.panel else { panic!("no revolve panel") };
    (p.full, p.term) = (false, crate::panels::RevTerm::To);
    d.frame(&mut wb, vec![]);
    presses_ok_and_is_refused(&mut d, &mut wb, "Click the face or plane it goes to first");
}

#[test]
fn ok_on_what_the_preview_says_cannot_be_built_says_so_again_and_stays() {
    use crate::panels::ExtentChoice;
    // A cut To Next from above the part: it cannot start.
    let (mut wb, mut d, _) = block_and_a_sketch_on_top();
    wb.exec("sketch.circle", json!({ "sketch": sketching(&wb).0, "cx": 20, "cy": 20, "r": 5 })).unwrap();
    wb.finish_sketch();
    d.settle(&mut wb);
    d.tap(&mut wb, egui::Key::E);
    d.frame(&mut wb, vec![]);
    let p = extrude_panel(&mut wb);
    p.choose_operation(tenon_model::Operation::Cut);
    p.direction = crate::panels::Direction::Default;
    p.extent = ExtentChoice::ToNext;
    d.settle(&mut wb);
    assert!(wb.status_error, "the preview says it cannot be built: {}", wb.status());
    let said = wb.status().to_owned();
    presses_ok_and_is_refused(&mut d, &mut wb, &said);
    // Put right, OK makes it.
    extrude_panel(&mut wb).extent = ExtentChoice::ThroughAll;
    extrude_panel(&mut wb).direction = crate::panels::Direction::Flipped;
    d.settle(&mut wb);
    ok(&mut d, &mut wb);
    assert!(wb.status().starts_with("Made Extrusion"), "{}", wb.status());
}

#[test]
fn a_cut_on_a_face_goes_into_the_part() {
    let (mut wb, mut d, _) = block_and_a_circle_on_top();
    let block = volume(&wb);
    d.tap(&mut wb, egui::Key::E);
    d.frame(&mut wb, vec![]);
    d.settle(&mut wb);
    assert!(extrude_panel(&mut wb).part_behind);
    assert_eq!(extrude_panel(&mut wb).direction, crate::panels::Direction::Default);

    // Cut chosen: it starts Flipped, and the preview takes material away.
    extrude_panel(&mut wb).choose_operation(tenon_model::Operation::Cut);
    assert_eq!(extrude_panel(&mut wb).direction, crate::panels::Direction::Flipped);
    d.settle(&mut wb);
    assert!(block - volume(&wb) > 100.0, "the cut removes {}", block - volume(&wb));

    // Join again: it comes back.
    extrude_panel(&mut wb).choose_operation(tenon_model::Operation::Join);
    assert_eq!(extrude_panel(&mut wb).direction, crate::panels::Direction::Default);
    d.tap(&mut wb, egui::Key::Escape);

    // A sketch through the middle of the part is left as it is.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(egui::vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "symmetric": 10 })).unwrap();
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let g = sketching(&wb);
    wb.exec("sketch.circle", json!({ "sketch": g.0, "cx": 20, "cy": 10, "r": 5 })).unwrap();
    wb.finish_sketch();
    d.settle(&mut wb);
    d.tap(&mut wb, egui::Key::E);
    d.frame(&mut wb, vec![]);
    assert!(!extrude_panel(&mut wb).part_behind);
    extrude_panel(&mut wb).choose_operation(tenon_model::Operation::Cut);
    assert_eq!(extrude_panel(&mut wb).direction, crate::panels::Direction::Default);
}

#[test]
fn extents_to_from_a_sketch_on_a_work_plane_picks_the_face_seen_through_it() {
    use tenon_geom::Vec3;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(egui::vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 60, "y2": 40 })).unwrap();
    wb.finish_sketch();
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 20 })).unwrap();
    let plane = wb.exec("work.plane", json!({ "base": "xy", "distance": 50 })).unwrap()["feature"].as_u64().unwrap();
    wb.create_sketch(json!({ "work_plane": plane })).unwrap();
    let sk = sketching(&wb);
    wb.exec("sketch.circle", json!({ "sketch": sk.0, "cx": 30, "cy": 20, "r": 5 })).unwrap();
    wb.finish_sketch();
    d.settle(&mut wb);
    wb.look_from(Vec3::new(0.3, -0.3, 1.0));
    d.settle(&mut wb);
    d.tap(&mut wb, egui::Key::E);
    d.frame(&mut wb, vec![]);
    extrude_panel(&mut wb).extent = crate::panels::ExtentChoice::To;
    d.settle(&mut wb);
    click_on_part(&mut d, &mut wb, Vec3::new(5.0, 35.0, 20.0));
    let to = extrude_panel(&mut wb).to.clone();
    assert!(matches!(to, Some(tenon_model::PlaneRef::Face(_))), "picked {to:?}; {}", wb.status());
    d.tap(&mut wb, egui::Key::Escape);
}

fn has_button(d: &Driver, key: &str) -> bool {
    d.ctx.data(|x| x.get_temp::<std::collections::BTreeMap<String, egui::Rect>>(egui::Id::new("tn_buttons"))).unwrap_or_default().contains_key(key)
}

#[test]
fn a_ribbon_wider_than_the_window_is_gone_along_by_its_arrows() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(egui::vec2(900.0, 560.0));
    wb.exec("model.box", json!({ "width": 40, "depth": 20, "height": 10 })).unwrap();
    d.settle(&mut wb);
    wb.run_ui("analysis.begin").unwrap();
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(pressable(&d, "ribbon analysis.finish").x > 900.0, "it is off the window to start with");
    assert!(has_button(&d, "tn_ribbon_on") && !has_button(&d, "tn_ribbon_back"));

    // Along by the arrow, as far as it goes.
    for _ in 0..5 {
        if pressable(&d, "ribbon analysis.finish").x < 880.0 {
            break;
        }
        d.click(&mut wb, pressable(&d, "tn_ribbon_on"));
        d.frame(&mut wb, vec![]);
        d.frame(&mut wb, vec![]);
    }
    assert!(pressable(&d, "ribbon analysis.finish").x < 880.0);
    assert!(has_button(&d, "tn_ribbon_back"));

    // There, a press on it does what it says, and no other button took the press on the arrow.
    assert!(wb.in_analysis(), "{}", wb.status());
    d.click(&mut wb, pressable(&d, "ribbon analysis.finish"));
    d.frame(&mut wb, vec![]);
    assert!(!wb.in_analysis(), "{}", wb.status());

    // In a window wide enough there are no arrows.
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(egui::vec2(1900.0, 900.0));
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(!has_button(&d, "tn_ribbon_on") && !has_button(&d, "tn_ribbon_back"));
}

#[test]
fn after_undo_the_status_does_not_go_on_saying_what_was_undone() {
    let (mut wb, mut d, sk) = block_and_a_sketch_on_top();
    wb.exec("sketch.circle", json!({ "sketch": sk.0, "cx": 20, "cy": 20, "r": 5 })).unwrap();
    wb.finish_sketch();
    d.settle(&mut wb);
    d.tap(&mut wb, egui::Key::E);
    d.frame(&mut wb, vec![]);
    ok(&mut d, &mut wb);
    assert!(wb.status().starts_with("Made"), "{}", wb.status());
    crate::tests::ctrl(&mut d, &mut wb, egui::Key::Z);
    d.settle(&mut wb);
    assert_eq!(wb.status(), "Undone.");
    crate::tests::ctrl(&mut d, &mut wb, egui::Key::Y);
    d.settle(&mut wb);
    assert_eq!(wb.status(), "Redone.");
}

#[test]
fn nothing_to_undo_is_no_fault_in_an_assembly_or_a_drawing() {
    for example in ["m3-pivot/pivot.tenonasm", "m4-plate/plate.tenondrw"] {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(example);
        let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
        let mut d = Driver::new(egui::vec2(1400.0, 860.0));
        wb.open(&file).unwrap();
        d.settle(&mut wb);
        crate::tests::ctrl(&mut d, &mut wb, egui::Key::Z);
        d.frame(&mut wb, vec![]);
        assert!(!wb.status_error, "{example}: {}", wb.status());
        assert_eq!(wb.status(), "Nothing to undo.", "{example}");
    }
}

#[test]
fn a_work_planes_status_goes_with_its_method_and_its_picks() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(egui::vec2(1400.0, 860.0));
    wb.exec("model.box", json!({ "width": 40, "depth": 20, "height": 10 })).unwrap();
    d.settle(&mut wb);
    wb.run_ui("work.plane").unwrap();
    d.frame(&mut wb, vec![]);
    assert!(wb.status().contains("set the offset"), "{}", wb.status());
    let Some(Panel::Work(w)) = &mut wb.panel else { panic!("no work panel") };
    w.method = crate::work::WorkMethod::ThreePoints;
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.status().contains("click three points"), "{}", wb.status());

    // OK too soon is an error; the first pick takes it away.
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    assert!(wb.status_error, "{}", wb.status());
    let Some(Panel::Work(w)) = &mut wb.panel else { panic!("no work panel") };
    w.points[0] = Some(tenon_model::PointSel::Origin);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(!wb.status_error && wb.status().contains("click three points"), "{}", wb.status());
}

#[test]
fn the_hole_panel_says_where_holes_on_a_face_are() {
    use tenon_geom::Vec3;
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(egui::vec2(1400.0, 860.0));
    wb.exec("model.box", json!({ "width": 40, "depth": 20, "height": 10 })).unwrap();
    d.settle(&mut wb);
    d.tap(&mut wb, egui::Key::H);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(d.text_pos("click a flat face").is_some(), "{:?}", d.texts());
    assert!(d.text_pos("click sketch points").is_none(), "{:?}", d.texts());
    let top = wb.scene().bodies[0].bbox.unwrap();
    click_on_part(&mut d, &mut wb, Vec3::new(top.min.x + 10.0, top.min.y + 10.0, top.max.z));
    d.frame(&mut wb, vec![]);
    assert!(d.text_pos("1 on the face").is_some(), "{}: {:?}", wb.status(), d.texts());
    d.tap(&mut wb, egui::Key::Escape);
}

/// The feature `id` opened and OK pressed: what comes of it, when the file changed or it was refused.
fn after_ok_on(d: &mut Driver, wb: &mut Workbench, id: FeatureId) -> Option<String> {
    let before = saved(wb);
    if wb.edit_feature(id).is_err() || wb.panel.is_none() {
        wb.panel = None;
        return None;
    }
    d.frame(wb, vec![]);
    d.frame(wb, vec![]);
    d.tap(wb, egui::Key::Enter);
    d.frame(wb, vec![]);
    d.settle(wb);
    if wb.panel.is_some() {
        let said = wb.status().to_owned();
        d.tap(wb, egui::Key::Escape);
        d.frame(wb, vec![]);
        wb.panel = None;
        return Some(format!("refused: {said}"));
    }
    let after = saved(wb);
    if after == before {
        return None;
    }
    let lines = |s: &str| s.lines().map(str::to_owned).collect::<std::collections::BTreeSet<_>>();
    let (a, b) = (lines(&before), lines(&after));
    Some(format!("{:?} -> {:?}", a.difference(&b).collect::<Vec<_>>(), b.difference(&a).collect::<Vec<_>>()))
}

#[test]
fn ok_with_nothing_touched_changes_nothing_in_any_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files = Vec::new();
    for dir in std::fs::read_dir(&root).unwrap().filter_map(Result::ok) {
        for f in std::fs::read_dir(dir.path()).into_iter().flatten().filter_map(Result::ok) {
            if f.path().extension().is_some_and(|e| e == "tenon") {
                files.push(f.path());
            }
        }
    }
    assert!(files.len() > 5, "{files:?}");
    let mut wrong = Vec::new();
    for file in files {
        let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
        let mut d = Driver::new(egui::vec2(1400.0, 860.0));
        wb.open(&file).unwrap();
        d.settle(&mut wb);
        let ids: Vec<FeatureId> = wb.document().features().iter().filter(|f| !matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id).collect();
        for id in ids {
            let name = wb.document().feature(id).map(|f| f.name.clone()).unwrap_or_default();
            if let Some(what) = after_ok_on(&mut d, &mut wb, id) {
                wrong.push(format!("{} {name}: {what}", file.display()));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn an_angle_comes_back_as_it_was_through_the_panels_degrees() {
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    let mut d = Driver::new(egui::vec2(1400.0, 860.0));
    wb.create_sketch(json!({ "plane": "xy" })).unwrap();
    let f = sketching(&wb);
    wb.exec("sketch.rectangle", json!({ "sketch": f.0, "x1": 0, "y1": 0, "x2": 40, "y2": 20 })).unwrap();
    wb.finish_sketch();
    // (These two are off by a bit through degrees and back.)
    wb.exec("model.extrude", json!({ "sketch": f.0, "distance": 10, "taper": 0.05235987755982988 })).unwrap();
    d.settle(&mut wb);
    ok_changes_nothing(&mut d, &mut wb);
    assert_eq!(value_of(&wb, "/taper"), 0.05235987755982988);
    // What is changed takes.
    reopen(&mut d, &mut wb);
    type_into(&mut d, &mut wb, "tn_props_taper", "2");
    ok(&mut d, &mut wb);
    assert!((value_of(&wb, "/taper") - 2f64.to_radians()).abs() < 1e-12);
}

#[test]
fn command_search_finds_what_a_button_reads_and_keeps_enter_to_itself() {
    assert!(crate::commands::search("go to flat").iter().any(|c| c.id == "sheet.flat_view"));
    let (mut wb, mut d, _) = block_and_a_circle_on_top();
    d.tap(&mut wb, egui::Key::E);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    // Nothing found, with a panel open: Enter does not press the panel's OK.
    let at = d.text_pos("Search commands...").unwrap_or_else(|| panic!("{:?}", d.texts()));
    d.click(&mut wb, at);
    d.frame(&mut wb, vec![egui::Event::Text("zzzz".into())]);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.chrome.search, "zzzz");
    d.tap(&mut wb, egui::Key::Enter);
    d.frame(&mut wb, vec![]);
    d.frame(&mut wb, vec![]);
    assert!(wb.panel.is_some(), "the panel took it: {}", wb.status());
    assert_eq!(wb.chrome.search, "", "the next search starts afresh");
}

/// An equation typed in a box is what was typed, not the "fx:" the box shows afterwards: whatever
/// happens once it has let go of the keyboard, nobody is told that is no number.
#[test]
fn an_equation_in_a_box_is_not_taken_for_what_the_box_shows_after() {
    let (mut wb, mut d, sk) = block_with_a_point();
    wb.exec("param.add", json!({ "name": "big", "equation": "30 mm" })).unwrap();
    let hole = json!({ "sketch": sk.0, "diameter": 6, "through_all": true, "reverse": true, "type": "counterbore", "counterbore_diameter": 12, "counterbore_depth": 3 });
    wb.exec("model.hole", hole).unwrap();
    d.settle(&mut wb);
    let id = last(&wb).id;

    // Enter, and the hole refuses the value: that is what goes on being said.
    wb.edit_feature(id).unwrap();
    type_into(&mut d, &mut wb, "tn_props_hole_dia", "big");
    d.tap(&mut wb, egui::Key::Enter);
    const WIDER: &str = "Hole1: the counterbore must be wider than the hole";
    assert_eq!(wb.status(), WIDER);
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    assert_eq!(wb.status(), WIDER);
    assert!(wb.panel.is_some());
    wb.run_ui("ui.cancel").unwrap();
    d.settle(&mut wb);

    // On to the next box with the mouse: no word against it, and OK takes it.
    wb.edit_feature(id).unwrap();
    type_into(&mut d, &mut wb, "tn_props_hole_dia", "big / 10");
    let said = wb.status().to_owned();
    let next = d.ctx.read_response(egui::Id::new("tn_props_seat_dia")).expect("the bore's box").rect.center();
    d.click(&mut wb, next);
    for _ in 0..3 {
        d.frame(&mut wb, vec![]);
    }
    assert_eq!(wb.status(), said);
    assert!(!wb.status_error, "{}", wb.status());
    wb.run_ui("ui.ok").unwrap();
    d.settle(&mut wb);
    assert!(wb.panel.is_none(), "{}", wb.status());
    let FeatureKind::Hole(h) = &last(&wb).kind else { panic!() };
    assert!((h.diameter - 3.0).abs() < 1e-9, "{}", h.diameter);
    let list = wb.exec("param.list", json!({})).unwrap();
    assert!(list["model"].as_array().unwrap().iter().any(|m| m["equation"] == "big / 10"), "{list}");

    // And what was typed is still judged, when the box is left with it wrong.
    wb.edit_feature(id).unwrap();
    type_into(&mut d, &mut wb, "tn_props_hole_dia", "nothing + 1");
    let next = d.ctx.read_response(egui::Id::new("tn_props_seat_dia")).expect("the bore's box").rect.center();
    d.click(&mut wb, next);
    d.frame(&mut wb, vec![]);
    assert_eq!(wb.status(), "nothing + 1 is not a number, or an equation of the parameters.");
    assert!(wb.status_error);
}

/// A part saved with a size no feature can have opens: the rebuild says what is wrong, the panel
/// opens on it, and what is typed there puts it right.
#[test]
fn a_size_a_parameter_pushed_too_far_is_put_right_in_its_panel() {
    let (mut wb, mut d, sk) = block_with_a_point();
    wb.exec("model.hole", json!({ "sketch": sk.0, "diameter": 6, "through_all": true, "reverse": true })).unwrap();
    d.settle(&mut wb);
    let id = last(&wb).id;
    let name =
        wb.exec("param.list", json!({})).unwrap()["model"].as_array().unwrap().iter().find(|m| m["target"]["field"] == "/diameter").unwrap()["name"]
            .as_str()
            .unwrap()
            .to_owned();
    wb.exec("param.set", json!({ "name": name, "equation": "0" })).unwrap();
    d.settle(&mut wb);
    let bytes = tenon_io::project::to_bytes(wb.document(), &serde_json::Map::new()).unwrap();

    let dir = std::env::temp_dir().join(format!("tenon-ui-pushed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("pushed.tenon");
    std::fs::write(&file, &bytes).unwrap();
    let mut wb = Workbench::headless(Box::new(OcctKernel::new()));
    wb.open(&file).unwrap();
    d.settle(&mut wb);
    assert!(wb.status().contains("Hole1: the hole diameter must be positive"), "{}", wb.status());
    wb.edit_feature(id).unwrap();
    d.settle(&mut wb);
    assert!(wb.panel.is_some());
    type_into(&mut d, &mut wb, "tn_props_hole_dia", "6");
    wb.run_ui("ui.ok").unwrap();
    d.settle(&mut wb);
    assert!(wb.panel.is_none() && !wb.status_error, "{}", wb.status());
    let FeatureKind::Hole(h) = &last(&wb).kind else { panic!() };
    assert_eq!(h.diameter, 6.0);
    let _ = std::fs::remove_dir_all(&dir);
}
