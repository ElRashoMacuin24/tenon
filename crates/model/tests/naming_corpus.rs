//! A corpus of edit-then-regenerate cases for persistent naming (docs/persistent-naming.md). Each
//! case builds the same block holding two references, a fillet on the edge between the top and
//! the right side, and a sketch on the top face with a hole cut through it. It then makes one
//! upstream edit and says where each reference must land afterwards, or that the part must break
//! with a message naming what is gone. A reference never lands somewhere else silently.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use tenon_geom::Vec3;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::{FaceOrigin, FaceRef, FeatureId, Session};

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// The block and what holds the references.
struct Base {
    sketch: u64,
    extrusion: u64,
    /// The rectangle's lines: bottom, right, top, left.
    lines: Vec<u64>,
    width: u64,
    fillet: u32,
    on_top: u32,
    cut: u32,
}

/// A 40 x 30 x 10 block (width dimensioned), the edge between its top and its right side
/// rounded (R2), and a Ø6 hole 3 deep cut from a sketch on its top face at (10, 10).
fn base(s: &mut Session, k: &mut OcctKernel) -> Base {
    let made = run(s, k, "sketch.create", json!({ "plane": "xy" }));
    let (sketch, origin) = (made["feature"].as_u64().unwrap(), made["origin"].clone());
    let rect = run(s, k, "sketch.rectangle", json!({ "sketch": sketch, "x1": 0, "y1": 0, "x2": 40, "y2": 30 }));
    let lines: Vec<u64> = rect["lines"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    // Its corner on the origin: a changed width moves the right side only.
    run(s, k, "sketch.constrain", json!({ "sketch": sketch, "constraint": { "type": "coincident", "a": rect["corners"][0], "b": origin } }));
    let c = |t: &str, line: u64, v: f64| json!({ "sketch": sketch, "constraint": { "type": t, "line": line, "value": v } });
    let width = run(s, k, "sketch.constrain", c("length", lines[0], 40.0))["constraint"].as_u64().unwrap();
    run(s, k, "sketch.constrain", c("length", lines[1], 30.0));
    let extrusion = run(s, k, "model.extrude", json!({ "sketch": sketch, "distance": 10 }))["feature"].as_u64().unwrap();
    let top = json!({ "type": "cap", "feature": extrusion, "end": "end" });
    let edge = run(s, k, "model.edge_ref", json!({ "faces": [top.clone(), { "type": "side", "feature": extrusion, "curve": lines[1] }] }));
    let fillet = run(s, k, "model.fillet", json!({ "edges": [edge], "radius": 2 }))["feature"].as_u64().unwrap();
    let face = run(s, k, "model.face_ref", json!({ "origin": top }));
    let on_top = run(s, k, "sketch.create", json!({ "face": face, "project_origin": false }))["feature"].as_u64().unwrap();
    run(s, k, "sketch.circle", json!({ "sketch": on_top, "cx": 10, "cy": 10, "r": 3 }));
    let cut =
        run(s, k, "model.extrude", json!({ "sketch": on_top, "distance": 3, "operation": "cut", "reverse": true }))["feature"].as_u64().unwrap();
    let id = |v: u64| u32::try_from(v).unwrap();
    Base { sketch, extrusion, lines, width, fillet: id(fillet), on_top: id(on_top), cut: id(cut) }
}

/// Where the two references land: the face the fillet made of its edge (centroid), and the top
/// face the sketch is on (centroid, area); or why the part does not regenerate.
#[derive(Debug)]
struct Landed {
    fillet: Vec3,
    centroid: Vec3,
    area: f64,
}

fn landed(s: &mut Session, k: &mut OcctKernel, b: &Base) -> Result<Landed, String> {
    let doc = s.document().clone();
    let kind = |f: u32| serde_json::to_value(&doc.feature(FeatureId(f)).unwrap().kind).unwrap();
    let face: FaceRef = serde_json::from_value(kind(b.on_top)["plane"]["face"].clone()).unwrap();
    let r = s.regen(k);
    if let Some((f, m)) = r.first_error() {
        return Err(format!("{f}: {m}"));
    }
    let (bf, fi) = r.resolve(&face, &*k)?;
    let top = k.face_info(r.bodies[bf].shape.face(fi)).unwrap();
    let rounds: Vec<(usize, u32)> = r
        .bodies
        .iter()
        .enumerate()
        .flat_map(|(bi, body)| {
            body.names.iter().enumerate().filter_map(move |(i, n)| match n {
                Some(FaceOrigin::From { feature, .. }) if feature.0 == b.fillet => Some((bi, u32::try_from(i).unwrap())),
                _ => None,
            })
        })
        .collect();
    let [(bi, fi)] = rounds.as_slice() else { return Err(format!("the fillet made {} faces", rounds.len())) };
    let round = k.face_info(r.bodies[*bi].shape.face(*fi)).unwrap();
    Ok(Landed { fillet: round.centroid, centroid: top.centroid, area: top.area })
}

/// The R2 fillet's face along the edge at x = `x`, z = `z` (its centroid lies 2·(2/π) in from
/// the edge along x and z), at mid-depth.
fn rounded(l: &Landed, x: f64, z: f64) -> bool {
    let inset = 2.0 * (1.0 - 2.0 / std::f64::consts::PI);
    let towards_inside = if z > 0.0 { -inset } else { inset };
    l.fillet.dist(Vec3::new(x - inset, 15.0, z + towards_inside)) < 1e-3
}

/// The hole: a Ø6 circle off the top.
const HOLE: f64 = std::f64::consts::PI * 9.0;

fn area(l: &Landed, a: f64) -> bool {
    (l.area - a).abs() < 1e-3
}

type Edit = fn(&mut Session, &mut OcctKernel, &Base);
type Check = fn(&Result<Landed, String>) -> bool;

/// Each case: what it does upstream, and where the references must be afterwards.
fn corpus() -> Vec<(&'static str, Edit, Check)> {
    fn set_feature(s: &mut Session, k: &mut OcctKernel, f: u64, edit: impl Fn(&mut Value)) {
        let mut kind = serde_json::to_value(&s.document().feature(FeatureId(u32::try_from(f).unwrap())).unwrap().kind).unwrap();
        edit(&mut kind);
        run(s, k, "feature.update", json!({ "feature": f, "kind": kind }));
    }
    vec![
        ("nothing changed", |_, _, _| {}, |l| l.as_ref().is_ok_and(|l| rounded(l, 40.0, 10.0) && area(l, 38.0 * 30.0 - HOLE))),
        (
            "an early sketch dimension made larger",
            |s, k, b| {
                run(s, k, "sketch.set_dimension", json!({ "sketch": b.sketch, "constraint": b.width, "value": 60 }));
            },
            |l| l.as_ref().is_ok_and(|l| rounded(l, 60.0, 10.0) && area(l, 58.0 * 30.0 - HOLE)),
        ),
        (
            "an early sketch dimension made smaller",
            |s, k, b| {
                run(s, k, "sketch.set_dimension", json!({ "sketch": b.sketch, "constraint": b.width, "value": 25 }));
            },
            |l| l.as_ref().is_ok_and(|l| rounded(l, 25.0, 10.0) && area(l, 23.0 * 30.0 - HOLE)),
        ),
        (
            "the extrusion made taller",
            |s, k, b| set_feature(s, k, b.extrusion, |kind| kind["extent"] = json!({ "distance": 16.0 })),
            |l| l.as_ref().is_ok_and(|l| rounded(l, 40.0, 16.0) && (l.centroid.z - 16.0).abs() < 1e-6),
        ),
        (
            "the extrusion turned the other way (a sign flipped)",
            |s, k, b| set_feature(s, k, b.extrusion, |kind| kind["reverse"] = json!(true)),
            // The end cap is now below: the references follow the face they name, not a place.
            |l| l.as_ref().is_ok_and(|l| rounded(l, 40.0, -10.0) && (l.centroid.z + 10.0).abs() < 1e-6),
        ),
        (
            "a hole added to the first sketch",
            |s, k, b| {
                run(s, k, "sketch.circle", json!({ "sketch": b.sketch, "cx": 30, "cy": 20, "r": 2 }));
            },
            |l| l.as_ref().is_ok_and(|l| rounded(l, 40.0, 10.0) && area(l, 38.0 * 30.0 - HOLE - std::f64::consts::PI * 4.0)),
        ),
        (
            "an independent hole added before the fillet",
            |s, k, b| {
                let sk = run(s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
                run(s, k, "sketch.circle", json!({ "sketch": sk, "cx": 30, "cy": 8, "r": 2 }));
                let cut = run(s, k, "model.extrude", json!({ "sketch": sk, "through_all": true, "operation": "cut" }))["feature"].clone();
                run(s, k, "feature.move", json!({ "feature": cut, "before": b.fillet }));
            },
            |l| l.as_ref().is_ok_and(|l| rounded(l, 40.0, 10.0)),
        ),
        (
            "the top face cut in two (the piece nearest the reference is kept)",
            |s, k, b| {
                let sk = run(s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].clone();
                run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 20, "y1": -1, "x2": 22, "y2": 31 }));
                let cut = run(s, k, "model.extrude", json!({ "sketch": sk, "distance": 10, "operation": "cut" }))["feature"].clone();
                run(s, k, "feature.move", json!({ "feature": cut, "before": b.fillet }));
            },
            // The left piece (20 wide, with the hole) is nearer the old centroid than the right.
            |l| l.as_ref().is_ok_and(|l| rounded(l, 40.0, 10.0) && area(l, 600.0 - HOLE)),
        ),
        (
            "two independent features reordered",
            |s, k, b| {
                run(s, k, "feature.move", json!({ "feature": b.cut, "before": b.fillet }));
            },
            |l| l.as_ref().is_ok_and(|l| rounded(l, 40.0, 10.0)),
        ),
        (
            "an unrelated feature suppressed",
            |s, k, b| {
                run(s, k, "feature.suppress", json!({ "feature": b.cut }));
            },
            |l| l.as_ref().is_ok_and(|l| rounded(l, 40.0, 10.0) && area(l, 38.0 * 30.0)),
        ),
        (
            "the line the rounded edge came from deleted and drawn again",
            |s, k, b| redraw(s, k, b, b.lines[1]),
            // A new line is a new face: the fillet says its edge is gone and what it was.
            |l| matches!(l, Err(m) if m.contains("no longer exists") && m.contains("Extrusion1")),
        ),
        (
            "the feature the references come from deleted",
            |s, k, b| {
                // Refused, naming what uses it; the part stays as it was.
                let e = s.exec("feature.delete", &json!({ "feature": b.extrusion }), Some(k)).unwrap_err().0;
                assert!(e.starts_with("Extrusion1 is used by ") && e.contains("Fillet1"), "{e}");
            },
            |l| l.as_ref().is_ok_and(|l| rounded(l, 40.0, 10.0) && area(l, 38.0 * 30.0 - HOLE)),
        ),
    ]
}

#[test]
fn references_land_where_they_belong_after_each_upstream_edit_or_break_clearly() {
    let mut k = OcctKernel::new();
    let mut failures = Vec::new();
    for (name, edit, check) in corpus() {
        let mut s = Session::default();
        let b = base(&mut s, &mut k);
        edit(&mut s, &mut k, &b);
        let l = landed(&mut s, &mut k, &b);
        if !check(&l) {
            failures.push(format!("{name}: {l:?}"));
        }
    }
    assert!(failures.is_empty(), "{} case(s) failed:\n{}", failures.len(), failures.join("\n"));
}

/// Deletes sketch line `line` of the first sketch and draws it again between the same points:
/// the face it swept is a new face, so references to the old one break.
fn redraw(s: &mut Session, k: &mut OcctKernel, b: &Base, line: u64) {
    let ends = run(s, k, "sketch.info", json!({ "sketch": b.sketch }))["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == line)
        .map(|e| (e["start"].clone(), e["end"].clone()))
        .unwrap();
    run(s, k, "sketch.delete", json!({ "sketch": b.sketch, "entities": [line] }));
    run(s, k, "sketch.line", json!({ "sketch": b.sketch, "start": ends.0, "end": ends.1 }));
}

#[test]
fn a_broken_reference_is_named_with_the_nearest_replacements_and_repaired_in_one_step() {
    let mut k = OcctKernel::new();
    let mut s = Session::default();
    let b = base(&mut s, &mut k);
    redraw(&mut s, &mut k, &b, b.lines[1]);
    let found = run(&mut s, &mut k, "model.broken", json!({}));
    assert_eq!((found["feature"].as_u64(), found["name"].as_str()), (Some(u64::from(b.fillet)), Some("Fillet1")), "{found}");
    let broken = found["broken"].as_array().unwrap();
    assert_eq!(broken.len(), 1, "{found}");
    assert_eq!((broken[0]["path"].as_str(), broken[0]["kind"].as_str()), (Some("/edges/0"), Some("edge")));
    // The nearest: the edge between the top and the side the new line made, where the old one was.
    let best = &broken[0]["candidates"][0];
    assert!(best["distance"].as_f64().unwrap() < 1e-6, "{best}");
    assert!(best["reference"]["faces"].as_array().unwrap().iter().any(|f| f["type"] == "cap"), "{best}");
    run(&mut s, &mut k, "model.repair", json!({ "feature": b.fillet, "path": "/edges/0", "with": best["reference"] }));
    let l = landed(&mut s, &mut k, &b);
    assert!(l.as_ref().is_ok_and(|l| rounded(l, 40.0, 10.0)), "{l:?}");
    assert!(run(&mut s, &mut k, "model.broken", json!({}))["broken"].as_array().unwrap().is_empty());
    // One undo: broken again.
    s.undo();
    assert_eq!(run(&mut s, &mut k, "model.broken", json!({}))["broken"].as_array().unwrap().len(), 1);
    // A reference of the wrong sort, or to nothing, is refused.
    let face = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": b.extrusion, "end": "end" } }));
    let e = s.exec("model.repair", &json!({ "feature": b.fillet, "path": "/edges/0", "with": face }), Some(&mut k)).unwrap_err().0;
    assert!(e.contains("needs an edge"), "{e}");
    assert!(s.exec("model.repair", &json!({ "feature": b.fillet, "path": "/edges/7", "with": best["reference"] }), Some(&mut k)).is_err());
}

#[test]
fn a_broken_face_reference_finds_the_face_in_the_same_place() {
    let mut k = OcctKernel::new();
    let mut s = Session::default();
    let b = base(&mut s, &mut k);
    // A shell opened on the left side.
    let left = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "side", "feature": b.extrusion, "curve": b.lines[3] } }));
    let shell = run(&mut s, &mut k, "model.shell", json!({ "remove": [left], "thickness": 1 }))["feature"].as_u64().unwrap();
    redraw(&mut s, &mut k, &b, b.lines[3]);
    let found = run(&mut s, &mut k, "model.broken", json!({}));
    assert_eq!(found["feature"].as_u64(), Some(shell), "{found}");
    let broken = &found["broken"][0];
    assert_eq!((broken["path"].as_str(), broken["kind"].as_str()), (Some("/remove/0"), Some("face")), "{found}");
    let best = &broken["candidates"][0];
    // The new left side: the same plane, in the same place.
    assert!(best["distance"].as_f64().unwrap() < 1e-6, "{best}");
    assert_eq!(best["reference"]["origin"]["type"], "side");
    run(&mut s, &mut k, "model.repair", json!({ "feature": shell, "path": "/remove/0", "with": best["reference"] }));
    assert!(run(&mut s, &mut k, "model.regenerate", json!({}))["error"].is_null());
}
