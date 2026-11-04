//! A feature's panel opened again: OK with nothing touched changes nothing, and what is typed
//! takes.

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::{Feature, FeatureId};

use crate::Workbench;
use crate::tests::{Driver, type_into};
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
