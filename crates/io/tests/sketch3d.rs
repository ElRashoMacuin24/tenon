//! A 3D sketch in a part file (DEC-136): laid out as a sketch is, read back to the same bytes, and
//! what it makes comes out the same.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Map, Value, json};
use tenon_io::{diff, project};
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::{Document, Session, regenerate};

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// A block; a 3D sketch that brings in an edge and a vertex of it, with a bend and a spline, held by
/// constraints; a tube along part of it.
fn part(k: &mut OcctKernel) -> Session {
    let mut s = Session::default();
    let sk = run(&mut s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(&mut s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }));
    run(&mut s, k, "model.extrude", json!({ "sketch": sk, "distance": 10 }));
    let r = s.regen(k).clone();
    let body = &r.bodies[0];
    let (mut upright, mut along) = (None, None);
    for e in 0..k.topology(body.shape).unwrap().edges {
        let i = k.edge_info(body.shape.edge(e)).unwrap();
        if (i.length - 10.0).abs() < 1e-9 && upright.is_none() {
            upright = Some(serde_json::to_value(r.edge_ref(0, e, k).unwrap()).unwrap());
        }
        if (i.length - 40.0).abs() < 1e-9 && along.is_none() {
            along = Some(serde_json::to_value(r.edge_ref(0, e, k).unwrap()).unwrap());
        }
    }
    let path = run(&mut s, k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    run(&mut s, k, "sketch3d.include", json!({ "sketch": path, "edge": upright.unwrap() }));
    let first = run(
        &mut s,
        k,
        "sketch3d.line",
        json!({ "sketch": path, "start": { "vertex": along.unwrap(), "end": 0 }, "end": [100, 100, 40.123456789012] }),
    );
    let second = run(&mut s, k, "sketch3d.line", json!({ "sketch": path, "start": first["end"], "end": [150, 100, 40] }));
    run(&mut s, k, "sketch3d.bend", json!({ "sketch": path, "point": first["end"], "radius": 7.5 }));
    let spline = run(&mut s, k, "sketch3d.spline", json!({ "sketch": path, "points": [second["end"], [170, 110, 45], [190, 90, 60]] }));
    run(&mut s, k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "tangent", "a": second["line"], "b": spline["spline"] } }));
    run(
        &mut s,
        k,
        "sketch3d.constrain",
        json!({ "sketch": path, "constraint": { "type": "coordinate", "point": spline["points"][2], "axis": "z", "value": 64.25 } }),
    );
    run(&mut s, k, "sketch3d.construction", json!({ "sketch": path, "entity": spline["spline"] }));
    s
}

#[test]
fn a_part_with_a_3d_sketch_reads_back_to_the_same_bytes() {
    let mut k = OcctKernel::new();
    let mut s = part(&mut k);
    assert_eq!(s.regen(&mut k).first_error(), None);
    let bytes = project::to_bytes(s.document(), &Map::new()).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    // Laid out as a sketch is: one record for each entity and constraint, solved positions rounded.
    assert!(text.contains("type = \"sketch3d\""), "{text}");
    assert!(text.contains("next_entity = "), "{text}");
    assert!(text.contains("links = ["), "{text}");
    assert!(!text.contains("40.123456789012"), "computed positions are rounded: {text}");
    let (back, extra) = project::from_bytes(&bytes).unwrap();
    assert_eq!(project::to_bytes(&back, &extra).unwrap(), bytes, "the same bytes");
    back.validate().unwrap();
    // And it rebuilds to the same: with no change in the sketch.
    let mut again = Session::new(back);
    assert_eq!(again.regen(&mut k).first_error(), None);
    assert_eq!(project::to_bytes(again.document(), &Map::new()).unwrap(), bytes, "a rebuild changes nothing written");
}

#[test]
fn a_file_that_says_what_cannot_be_is_told_not_believed() {
    let mut k = OcctKernel::new();
    let s = part(&mut k);
    let text = String::from_utf8(project::to_bytes(s.document(), &Map::new()).unwrap()).unwrap();
    // A line that refers to a point it does not have.
    let at = text.find("type = \"sketch3d\"").unwrap();
    let bad = format!("{}{}", &text[..at], text[at..].replacen("type = \"line\", start = ", "type = \"line\", start = 9", 1));
    let e = project::from_bytes(bad.as_bytes()).unwrap_err().to_string();
    assert!(e.contains("3D Sketch1"), "it says which: {e}");
    // A bend of no radius.
    let bad = text.replacen("value = 7.5", "value = -7.5", 1);
    let e = project::from_bytes(bad.as_bytes()).unwrap_err().to_string();
    assert!(e.contains("3D Sketch1") && e.contains("radius"), "{e}");
    // A word that is no constraint.
    let bad = text.replacen("type = \"tangent\"", "type = \"touching\"", 1);
    assert!(project::from_bytes(bad.as_bytes()).is_err());
}

#[test]
fn a_part_without_one_is_written_as_it_was() {
    // (What is added does not touch files that have none.)
    let doc = Document::new("Plain");
    let text = String::from_utf8(project::to_bytes(&doc, &Map::new()).unwrap()).unwrap();
    assert!(!text.contains("sketch3d"));
    let _ = regenerate;
}

#[test]
fn comparing_two_versions_says_what_changed_in_a_3d_sketch() {
    let mut k = OcctKernel::new();
    let mut s = part(&mut k);
    let before = serde_json::to_value(s.document()).unwrap();
    let id = s.document().features().iter().find(|f| f.name == "3D Sketch1").unwrap().id;
    let info = run(&mut s, &mut k, "sketch3d.info", json!({ "sketch": id.0 }));
    let c = info["constraints"].as_array().unwrap().iter().find(|c| c["type"] == "radius").unwrap()["id"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch3d.set_dimension", json!({ "sketch": id.0, "constraint": c, "value": 9.5 }));
    run(&mut s, &mut k, "sketch3d.point", json!({ "sketch": id.0, "at": [1, 2, 3] }));
    let after = serde_json::to_value(s.document()).unwrap();
    let text = diff::diff_documents("part", &before, &after).text();
    assert!(text.contains("7.5 -> 9.5"), "{text}");
    assert!(text.contains("1 entity added"), "{text}");
}
