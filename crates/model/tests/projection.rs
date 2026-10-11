//! Edges of the part projected into a sketch (DEC-055): reference geometry the sketch's own is
//! dimensioned to, that follows the part when it changes, bounds profiles, and says when its
//! edge is gone. And centre lines: the axis a revolution takes, and diameters measured to them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};
use tenon_geom::{Vec2, Vec3};
use tenon_kernel_occt::OcctKernel;
use tenon_model::{FeatureId, Session};

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> String {
    s.exec(id, &p, Some(k)).unwrap_err().to_string()
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1.0)
}

fn id(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or_else(|| panic!("no `{key}` in {v}"))
}

/// A block `w` x 20 x 10 with a corner at the origin; its width is the dimension returned.
/// Returns (sketch, extrusion, width dimension).
fn block(s: &mut Session, k: &mut OcctKernel, w: f64) -> (u64, u64, u64) {
    let sk = run(s, k, "sketch.create", json!({ "plane": "xy" }));
    let (sketch, origin) = (id(&sk, "feature"), id(&sk, "origin"));
    let r = run(s, k, "sketch.rectangle", json!({ "sketch": sketch, "x1": 0, "y1": 0, "x2": w, "y2": 20 }));
    let (lines, corner) = (r["lines"].as_array().unwrap().clone(), r["corners"][0].clone());
    run(s, k, "sketch.constrain", json!({ "sketch": sketch, "constraint": { "type": "coincident", "a": corner, "b": origin } }));
    let width = run(s, k, "sketch.constrain", json!({ "sketch": sketch, "constraint": { "type": "length", "line": lines[0], "value": w } }));
    run(s, k, "sketch.constrain", json!({ "sketch": sketch, "constraint": { "type": "length", "line": lines[1], "value": 20.0 } }));
    let x = run(s, k, "model.extrude", json!({ "sketch": sketch, "distance": 10 }));
    (sketch, id(&x, "feature"), id(&width, "constraint"))
}

/// A sketch on the top of extrusion `x`.
fn on_top(s: &mut Session, k: &mut OcctKernel, x: u64) -> (u64, Value) {
    let top = run(s, k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": x, "end": "end" } }));
    let sk = run(s, k, "sketch.create", json!({ "face": top }));
    (id(&sk, "feature"), top)
}

/// The sketch's entities of one type, as `sketch.info` gives them.
fn entities(s: &mut Session, k: &mut OcctKernel, sketch: u64, kind: &str) -> Vec<Value> {
    run(s, k, "sketch.info", json!({ "sketch": sketch }))["entities"].as_array().unwrap().iter().filter(|e| e["type"] == kind).cloned().collect()
}

/// A sketch point of sketch `sketch` in the part's coordinates.
fn in_part(s: &mut Session, k: &mut OcctKernel, sketch: u64, p: Vec2) -> Vec3 {
    s.regen(k).sketch_frames[&FeatureId(sketch as u32)].plane_point(p)
}

fn point(s: &Session, sketch: u64, entity: u64) -> Vec2 {
    s.document().sketch(FeatureId(sketch as u32)).unwrap().point(tenon_sketch::EntityId(entity as u32)).unwrap()
}

/// Where sketch point `entity` is on the part, as the part now stands.
fn where_is(s: &mut Session, k: &mut OcctKernel, sketch: u64, entity: u64) -> Vec3 {
    let frame = s.regen(k).sketch_frames[&FeatureId(sketch as u32)];
    frame.plane_point(point(s, sketch, entity))
}

#[test]
fn a_hole_dimensioned_from_a_projected_edge_follows_the_part() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (base, x, width) = block(&mut s, &mut k, 40.0);
    let (sk, top) = on_top(&mut s, &mut k, x);
    // The top's four edges, shown in the sketch: reference geometry, with nothing left to move.
    let shown = run(&mut s, &mut k, "sketch.project", json!({ "sketch": sk, "face": top }));
    assert_eq!((shown["entities"].as_array().unwrap().len(), shown["skipped"].as_array().unwrap().len()), (4, 0), "{shown}");
    let lines = entities(&mut s, &mut k, sk, "line");
    assert!(lines.len() == 4 && lines.iter().all(|l| l["reference"] == true && l["fully_constrained"] == true), "{lines:?}");
    assert_eq!(run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }))["dof"], 0);
    // Which is the right-hand edge (x = 40 on the part), and which the front (y = 0).
    let ends = |s: &mut Session, k: &mut OcctKernel, l: &Value| -> (Vec3, Vec3) {
        let (a, b) = (point(s, sk, id(l, "start")), point(s, sk, id(l, "end")));
        (in_part(s, k, sk, a), in_part(s, k, sk, b))
    };
    let mut right = None;
    let mut front = None;
    for l in &lines {
        let (a, b) = ends(&mut s, &mut k, l);
        assert!(near(a.z, 10.0) && near(b.z, 10.0), "on the top: {a:?} {b:?}");
        if near(a.x, 40.0) && near(b.x, 40.0) {
            right = Some(id(l, "id"));
        }
        if near(a.y, 0.0) && near(b.y, 0.0) {
            front = Some(id(l, "id"));
        }
    }
    let (right, front) = (right.expect("the right-hand edge"), front.expect("the front edge"));

    // A 6 mm hole, 8 from the right-hand edge and 5 from the front.
    let c = run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 3, "cy": 3, "r": 2 }));
    let (circle, centre) = (id(&c, "circle"), id(&entities(&mut s, &mut k, sk, "circle")[0], "center"));
    let dim =
        |s: &mut Session, k: &mut OcctKernel, c: Value| id(&run(s, k, "sketch.constrain", json!({ "sketch": sk, "constraint": c })), "constraint");
    dim(&mut s, &mut k, json!({ "type": "diameter", "curve": circle, "value": 6.0 }));
    dim(&mut s, &mut k, json!({ "type": "distance", "a": centre, "b": right, "value": 8.0 }));
    dim(&mut s, &mut k, json!({ "type": "distance", "a": centre, "b": front, "value": 5.0 }));
    assert_eq!(run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }))["dof"], 0);
    let at = where_is(&mut s, &mut k, sk, centre);
    assert!(at.near(Vec3::new(32.0, 5.0, 10.0), 1e-9), "{at:?}");
    // The projected outline bounds a profile too: the top with the hole in it, and the hole.
    let regions = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }))["regions"].as_array().unwrap().clone();
    assert_eq!(regions.len(), 2, "{regions:?}");
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "regions": [[circle]], "through_all": true, "reverse": true, "operation": "cut" }));
    assert!(near(volume(&mut s, &mut k), 8000.0 - 9.0 * PI * 10.0), "{}", volume(&mut s, &mut k));

    // The block is made 60 wide: the edge is found again 20 further on, and the hole goes with it.
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": base, "constraint": width, "value": 60.0 }));
    assert!(near(volume(&mut s, &mut k), 12000.0 - 9.0 * PI * 10.0), "{}", volume(&mut s, &mut k));
    let at = where_is(&mut s, &mut k, sk, centre);
    assert!(at.near(Vec3::new(52.0, 5.0, 10.0), 1e-9), "the document's sketch follows what regeneration found: {at:?}");
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }));
    assert!(info["dof"] == 0 && info["projections"].as_array().unwrap().iter().all(|p| p["lost"].is_null()), "{info}");
    // The hole really is there: its wall is a cylinder about (52, 5).
    let faces = run(&mut s, &mut k, "model.faces", json!({ "body": 0 }));
    let wall = faces["faces"].as_array().unwrap().iter().find(|f| f["surface"]["Cylinder"].is_object()).expect("the hole's wall").clone();
    let axis = &wall["surface"]["Cylinder"]["axis"]["origin"];
    assert!(near(axis["x"].as_f64().unwrap(), 52.0) && near(axis["y"].as_f64().unwrap(), 5.0), "{wall}");
    // Undone, the sketch is as it was before, and the part with it.
    assert!(s.undo());
    assert!(near(volume(&mut s, &mut k), 8000.0 - 9.0 * PI * 10.0));
    let at = where_is(&mut s, &mut k, sk, centre);
    assert!(at.near(Vec3::new(32.0, 5.0, 10.0), 1e-9), "{at:?}");

    // Projecting the same edges again adds nothing.
    let again = run(&mut s, &mut k, "sketch.project", json!({ "sketch": sk, "face": top }));
    assert_eq!(again["entities"], shown["entities"]);
    assert_eq!(entities(&mut s, &mut k, sk, "line").len(), 4);
    // What the sketch's own tools may not do to it, and what they may.
    let e = refused(&mut s, &mut k, "sketch.trim", json!({ "sketch": sk, "curve": right, "x": 0, "y": 0 }));
    assert!(e.contains("projected geometry") && e.contains("break its link"), "{e}");
    // An edge of the hole is made after the sketch: it cannot be shown in it.
    let hole = id(run(&mut s, &mut k, "model.tree", json!({}))["features"].as_array().unwrap().last().unwrap(), "id");
    let rim = run(
        &mut s,
        &mut k,
        "model.edge_ref",
        json!({ "faces": [{ "type": "cap", "feature": x, "end": "start" }, { "type": "side", "feature": hole, "curve": circle }] }),
    );
    let e = refused(&mut s, &mut k, "sketch.project", json!({ "sketch": sk, "edge": rim }));
    assert!(e.contains("comes after the sketch"), "{e}");
    assert!(refused(&mut s, &mut k, "sketch.project", json!({ "sketch": sk })).contains("give `edge`"));
    assert!(refused(&mut s, &mut k, "sketch.project", json!({ "sketch": x, "face": top })).contains("is not a sketch"));
}

#[test]
fn projected_geometry_is_deleted_unlinked_and_says_when_its_edge_is_gone() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, x, _) = block(&mut s, &mut k, 40.0);
    // A pocket in the top: a 10 mm circle cut 4 deep.
    let (pocket_sk, top) = on_top(&mut s, &mut k, x);
    let c = id(&run(&mut s, &mut k, "sketch.circle", json!({ "sketch": pocket_sk, "cx": 20, "cy": 10, "r": 5 })), "circle");
    let pocket =
        id(&run(&mut s, &mut k, "model.extrude", json!({ "sketch": pocket_sk, "distance": 4, "reverse": true, "operation": "cut" })), "feature");
    assert!(near(volume(&mut s, &mut k), 8000.0 - 25.0 * PI * 4.0));
    // A sketch on the top showing the pocket's rim and one edge of the block.
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "face": top })), "feature");
    let rim = run(
        &mut s,
        &mut k,
        "model.edge_ref",
        json!({ "faces": [{ "type": "cap", "feature": x, "end": "end" }, { "type": "side", "feature": pocket, "curve": c }] }),
    );
    let side = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "side", "feature": x, "curve": 6 } }));
    let front = run(&mut s, &mut k, "model.edge_ref", json!({ "faces": [{ "type": "cap", "feature": x, "end": "end" }, side["origin"]] }));
    let shown = run(&mut s, &mut k, "sketch.project", json!({ "sketch": sk, "edges": [rim, front] }));
    let (ring, line) = (shown["entities"][0].as_u64().unwrap(), shown["entities"][1].as_u64().unwrap());
    let circles = entities(&mut s, &mut k, sk, "circle");
    assert!(circles.len() == 1 && circles[0]["reference"] == true && near(circles[0]["radius"].as_f64().unwrap(), 5.0), "{circles:?}");
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }));
    assert_eq!(info["projections"].as_array().unwrap().len(), 2);

    // The pocket suppressed: its rim is not there to find. The geometry stays where it was and
    // the sketch says which piece lost its edge; the block's own edge is unaffected.
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": pocket }));
    assert!(near(volume(&mut s, &mut k), 8000.0));
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }));
    let lost: Vec<&Value> = info["projections"].as_array().unwrap().iter().filter(|p| !p["lost"].is_null()).collect();
    assert!(lost.len() == 1 && lost[0]["entity"] == ring, "{info}");
    assert!(near(entities(&mut s, &mut k, sk, "circle")[0]["radius"].as_f64().unwrap(), 5.0));
    // Back again, it is found again.
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": pocket, "suppressed": false }));
    volume(&mut s, &mut k);
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }));
    assert!(info["projections"].as_array().unwrap().iter().all(|p| p["lost"].is_null()), "{info}");
    // The pocket cannot be deleted while a sketch shows its rim.
    let e = refused(&mut s, &mut k, "feature.delete", json!({ "feature": pocket }));
    assert!(e.contains("Sketch3") || e.contains("used by"), "{e}");

    // Its link broken, the line is the sketch's own: where it was, free to move, and forgotten
    // as a projection. Only projected geometry has a link to break.
    let dof = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }))["dof"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.break_link", json!({ "sketch": sk, "entities": [line] }));
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }));
    assert_eq!((info["dof"].as_u64(), info["projections"].as_array().unwrap().len()), (Some(dof + 4), 1), "{info}");
    assert!(entities(&mut s, &mut k, sk, "line").iter().all(|l| l["reference"].is_null()));
    assert!(refused(&mut s, &mut k, "sketch.break_link", json!({ "sketch": sk, "entities": [line] })).contains("is not projected geometry"));
    // Deleted from the sketch, the rim is forgotten too, and then the pocket may go.
    run(&mut s, &mut k, "sketch.delete", json!({ "sketch": sk, "entities": [ring] }));
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }));
    assert!(info.get("projections").is_none(), "{info}");
    run(&mut s, &mut k, "feature.delete", json!({ "feature": pocket }));
    assert!(near(volume(&mut s, &mut k), 8000.0));
}

#[test]
fn rounded_corners_project_as_arcs_the_right_way_round() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, x, _) = block(&mut s, &mut k, 40.0);
    // The four upright edges rounded 5: the top is a rectangle with four quarter circles.
    let mut edges = Vec::new();
    for (a, b) in [(6, 7), (7, 8), (8, 9), (9, 6)] {
        edges.push(run(
            &mut s,
            &mut k,
            "model.edge_ref",
            json!({ "faces": [{ "type": "side", "feature": x, "curve": a }, { "type": "side", "feature": x, "curve": b }] }),
        ));
    }
    run(&mut s, &mut k, "model.fillet", json!({ "edges": edges, "radius": 5 }));
    let rounded = 800.0 - (4.0 - PI) * 25.0;
    assert!(near(volume(&mut s, &mut k), rounded * 10.0));
    let (sk, top) = on_top(&mut s, &mut k, x);
    let shown = run(&mut s, &mut k, "sketch.project", json!({ "sketch": sk, "face": top }));
    assert_eq!(shown["entities"].as_array().unwrap().len(), 8, "{shown}");
    let arcs = entities(&mut s, &mut k, sk, "arc");
    assert_eq!(arcs.len(), 4);
    // Each arc is a quarter turn that bulges away from the middle of the top: its middle point
    // is further from (20, 10) than its centre is.
    let sketch = s.document().sketch(FeatureId(sk as u32)).unwrap().clone();
    for a in &arcs {
        let arc = sketch.arc(tenon_sketch::EntityId(id(a, "id") as u32)).unwrap();
        assert!(near(arc.radius, 5.0) && near(arc.sweep(), PI / 2.0), "{arc:?}");
        let (centre, mid) = (in_part(&mut s, &mut k, sk, arc.center), in_part(&mut s, &mut k, sk, arc.mid_point()));
        let middle = Vec3::new(20.0, 10.0, 10.0);
        assert!(mid.dist(middle) > centre.dist(middle) + 4.0, "{centre:?} {mid:?}");
    }
    // So the outline is one closed profile of the rounded rectangle's area: a boss on it adds
    // exactly that much a millimetre.
    let regions = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }))["regions"].as_array().unwrap().clone();
    // (A region's area is measured on its outline drawn with short chords: near enough to tell regions apart.)
    assert!(regions.len() == 1 && (regions[0]["area"].as_f64().unwrap() - rounded).abs() < 1e-3 * rounded, "{regions:?}");
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 2 }));
    assert!(near(volume(&mut s, &mut k), rounded * 12.0), "{}", volume(&mut s, &mut k));
}

#[test]
fn a_profile_is_bounded_by_projected_edges_and_the_sketchs_own_line() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (base, x, width) = block(&mut s, &mut k, 40.0);
    let (sk, top) = on_top(&mut s, &mut k, x);
    run(&mut s, &mut k, "sketch.project", json!({ "sketch": sk, "face": top }));
    // A line across the corner at the far right, held to the two edges it ends on: 10 along
    // each from the corner.
    let lines = entities(&mut s, &mut k, sk, "line");
    let edge_at = |s: &mut Session, k: &mut OcctKernel, want: &dyn Fn(Vec3, Vec3) -> bool| -> u64 {
        for l in &lines {
            let (a, b) = (point(s, sk, id(l, "start")), point(s, sk, id(l, "end")));
            let (a, b) = (in_part(s, k, sk, a), in_part(s, k, sk, b));
            if want(a, b) {
                return id(l, "id");
            }
        }
        panic!("no such edge")
    };
    let right = edge_at(&mut s, &mut k, &|a, b| near(a.x, 40.0) && near(b.x, 40.0));
    let back = edge_at(&mut s, &mut k, &|a, b| near(a.y, 20.0) && near(b.y, 20.0));
    let frame = s.regen(&mut k).sketch_frames[&FeatureId(sk as u32)];
    let flat = |p: Vec3| {
        let l = frame.to_local(p);
        (l.x, l.y)
    };
    let ((x1, y1), (x2, y2)) = (flat(Vec3::new(40.0, 12.0, 10.0)), flat(Vec3::new(28.0, 20.0, 10.0)));
    let cut = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": x1, "y1": y1, "x2": x2, "y2": y2 }));
    for (pt, edge) in [(id(&cut, "start"), right), (id(&cut, "end"), back)] {
        run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "point_on_curve", "point": pt, "curve": edge } }));
    }
    let corner = |s: &mut Session, k: &mut OcctKernel, pt: u64, other: u64| {
        run(s, k, "sketch.constrain", json!({ "sketch": sk, "constraint": { "type": "distance", "a": pt, "b": other, "value": 10.0 } }));
    };
    corner(&mut s, &mut k, id(&cut, "start"), back);
    corner(&mut s, &mut k, id(&cut, "end"), right);
    assert_eq!(run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }))["dof"], 0);
    // Two regions: the corner triangle (50 mm^2) and the rest. The corner is cut away.
    let regions = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }))["regions"].as_array().unwrap().clone();
    assert_eq!(regions.len(), 2, "{regions:?}");
    let small = regions.iter().min_by(|a, b| a["area"].as_f64().unwrap().total_cmp(&b["area"].as_f64().unwrap())).unwrap();
    assert!(near(small["area"].as_f64().unwrap(), 50.0), "{small}");
    run(
        &mut s,
        &mut k,
        "model.extrude",
        json!({ "sketch": sk, "regions": [small["select"]], "through_all": true, "reverse": true, "operation": "cut" }),
    );
    assert!(near(volume(&mut s, &mut k), 8000.0 - 500.0), "{}", volume(&mut s, &mut k));
    // The block made wider: the corner is still cut 10 by 10, at the new corner.
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": base, "constraint": width, "value": 55.0 }));
    assert!(near(volume(&mut s, &mut k), 11000.0 - 500.0), "{}", volume(&mut s, &mut k));
    let p = where_is(&mut s, &mut k, sk, id(&cut, "start"));
    assert!(p.near(Vec3::new(55.0, 10.0, 10.0), 1e-9), "{p:?}");
}

#[test]
fn an_equation_reads_a_dimension_to_a_projected_edge() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (base, x, width) = block(&mut s, &mut k, 40.0);
    let (sk, top) = on_top(&mut s, &mut k, x);
    run(&mut s, &mut k, "sketch.project", json!({ "sketch": sk, "face": top }));
    // A driven dimension reads how long the top's front edge is; a post on the top is as high
    // as a quarter of it.
    let lines = entities(&mut s, &mut k, sk, "line");
    let long = lines
        .iter()
        .find(|l| {
            let (a, b) = (point(&s, sk, id(l, "start")), point(&s, sk, id(l, "end")));
            near(a.dist(b), 40.0)
        })
        .map(|l| id(l, "id"))
        .unwrap();
    let read = run(
        &mut s,
        &mut k,
        "sketch.constrain",
        json!({ "sketch": sk, "constraint": { "type": "length", "line": long, "value": 1.0 }, "driven": true }),
    );
    let name = run(&mut s, &mut k, "param.list", json!({}))["model"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["driven"] == true)
        .map(|p| p["name"].as_str().unwrap().to_owned())
        .expect("the driven dimension is a parameter");
    assert!(read["driven"] == true);
    let frame = s.regen(&mut k).sketch_frames[&FeatureId(sk as u32)];
    let l = frame.to_local(Vec3::new(10.0, 10.0, 10.0));
    let post = id(&run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": l.x, "cy": l.y, "r": 2 })), "circle");
    run(
        &mut s,
        &mut k,
        "model.extrude",
        json!({ "sketch": sk, "regions": [[post]], "distance": 10, "equations": { "/extent/distance": format!("{name} / 4") } }),
    );
    assert!(near(volume(&mut s, &mut k), 8000.0 + 4.0 * PI * 10.0), "{}", volume(&mut s, &mut k));
    // The block is made 60 wide. The edge is found 60 long, the dimension reads it, the
    // equation follows, and the part is rebuilt with a post of 15: all in one regeneration.
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": base, "constraint": width, "value": 60.0 }));
    assert!(near(volume(&mut s, &mut k), 12000.0 + 4.0 * PI * 15.0), "{}", volume(&mut s, &mut k));
    assert!(s.undo());
    assert!(near(volume(&mut s, &mut k), 8000.0 + 4.0 * PI * 10.0));
}

#[test]
fn an_edge_is_projected_without_a_kernel_from_what_the_caller_says_it_is() {
    // The app's kernel is on another thread: it says what the edge is on the sketch plane (it
    // has the part on screen), and regeneration puts it right if that was off.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, x, _) = block(&mut s, &mut k, 40.0);
    let (sk, _) = on_top(&mut s, &mut k, x);
    let side = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "side", "feature": x, "curve": 6 } }));
    let front = run(&mut s, &mut k, "model.edge_ref", json!({ "faces": [{ "type": "cap", "feature": x, "end": "end" }, side["origin"]] }));
    let e = s.exec("sketch.project", &json!({ "sketch": sk, "edge": front }), None).unwrap_err().to_string();
    assert!(e.contains("needs the geometry kernel, or `geometry`"), "{e}");
    let said = json!([{ "type": "line", "start": { "x": 1.0, "y": 2.0 }, "end": { "x": 30.0, "y": 3.0 } }]);
    let shown = s.exec("sketch.project", &json!({ "sketch": sk, "edges": [front], "geometry": said }), None).unwrap();
    let line = shown["entities"][0].as_u64().unwrap();
    let ends = |s: &Session| {
        let sketch = s.document().sketch(FeatureId(sk as u32)).unwrap();
        sketch.line(tenon_sketch::EntityId(line as u32)).unwrap()
    };
    assert_eq!(ends(&s).0, Vec2::new(1.0, 2.0));
    // Regenerated: the line is where the edge is, 40 long on the top's front.
    volume(&mut s, &mut k);
    let (a, b) = ends(&s);
    let (a, b) = (in_part(&mut s, &mut k, sk, a), in_part(&mut s, &mut k, sk, b));
    assert!(near(a.dist(b), 40.0) && near(a.y, 0.0) && near(b.y, 0.0) && near(a.z, 10.0), "{a:?} {b:?}");
    // The lists must match one for one.
    let e = s.exec("sketch.project", &json!({ "sketch": sk, "edges": [front], "geometry": [] }), None).unwrap_err().to_string();
    assert!(e.contains("one for one"), "{e}");
}

#[test]
fn a_centre_line_is_the_axis_and_dimensions_to_it_are_diameters() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    // Half the section of a stepped pin, above its axis along X: 30 long, 20 across for the
    // first 10 and 12 across for the rest.
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" }));
    let (sketch, origin) = (id(&sk, "feature"), id(&sk, "origin"));
    let pts: [(f64, f64); 6] = [(0.0, 0.0), (30.0, 0.0), (30.0, 5.0), (10.0, 5.0), (10.0, 9.0), (0.0, 9.0)];
    let mut lines = Vec::new();
    let mut nodes: Vec<u64> = Vec::new();
    for i in 0..pts.len() {
        let mut p = json!({ "sketch": sketch });
        match nodes.last() {
            Some(start) => p["start"] = json!(start),
            None => {
                p["x1"] = json!(pts[0].0);
                p["y1"] = json!(pts[0].1);
            }
        }
        if i + 1 == pts.len() {
            p["end"] = json!(nodes[0]);
        } else {
            p["x2"] = json!(pts[i + 1].0);
            p["y2"] = json!(pts[i + 1].1);
        }
        let l = run(&mut s, &mut k, "sketch.line", p);
        if nodes.is_empty() {
            nodes.push(id(&l, "start"));
        }
        if i + 1 < pts.len() {
            nodes.push(id(&l, "end"));
        }
        let next = pts[(i + 1) % pts.len()];
        let kind = if (next.1 - pts[i].1).abs() < 1e-9 { "horizontal" } else { "vertical" };
        run(&mut s, &mut k, "sketch.constrain", json!({ "sketch": sketch, "constraint": { "type": kind, "line": id(&l, "line") } }));
        lines.push(id(&l, "line"));
    }
    let axis = lines[0];
    let hold = |s: &mut Session, k: &mut OcctKernel, c: Value| {
        id(&run(s, k, "sketch.constrain", json!({ "sketch": sketch, "constraint": c })), "constraint")
    };
    hold(&mut s, &mut k, json!({ "type": "coincident", "a": nodes[0], "b": origin }));
    hold(&mut s, &mut k, json!({ "type": "length", "line": axis, "value": 30.0 }));
    hold(&mut s, &mut k, json!({ "type": "length", "line": lines[4], "value": 10.0 }));
    // Without a centre line a revolution must be told its axis.
    let e = refused(&mut s, &mut k, "model.revolve", json!({ "sketch": sketch }));
    assert!(e.contains("no centre line"), "{e}");
    run(&mut s, &mut k, "sketch.centerline", json!({ "sketch": sketch, "entity": axis }));
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sketch }));
    assert_eq!(info["entities"].as_array().unwrap().iter().filter(|e| e["centerline"] == true).count(), 1);
    assert_eq!(info["regions"].as_array().unwrap().len(), 1, "the centre line still closes the half section");
    // The two sizes are given across the pin, as on its drawing: diameters to the centre line.
    let large = hold(&mut s, &mut k, json!({ "type": "linear_diameter", "point": nodes[5], "axis": axis, "value": 20.0 }));
    let small = hold(&mut s, &mut k, json!({ "type": "linear_diameter", "point": nodes[2], "axis": axis, "value": 12.0 }));
    assert_eq!(run(&mut s, &mut k, "sketch.info", json!({ "sketch": sketch }))["dof"], 0);
    assert!(near(point(&s, sketch, nodes[5]).y, 10.0) && near(point(&s, sketch, nodes[2]).y, 6.0));
    // The revolution takes the centre line for its axis.
    let rev = run(&mut s, &mut k, "model.revolve", json!({ "sketch": sketch }));
    let pin = |d1: f64, d2: f64| PI * (d1 * d1 / 4.0 * 10.0 + d2 * d2 / 4.0 * 20.0);
    assert!(near(volume(&mut s, &mut k), pin(20.0, 12.0)), "{}", volume(&mut s, &mut k));
    let made = s.document().feature(FeatureId(id(&rev, "feature") as u32)).unwrap().kind.clone();
    let tenon_model::FeatureKind::Revolve(made) = made else { panic!("{made:?}") };
    assert_eq!(made.axis, tenon_model::AxisRef::SketchLine(tenon_sketch::EntityId(axis as u32)));
    // A diameter is a parameter like any dimension: 12 becomes 16.
    run(&mut s, &mut k, "sketch.set_dimension", json!({ "sketch": sketch, "constraint": small, "value": 16.0 }));
    assert!(near(volume(&mut s, &mut k), pin(20.0, 16.0)), "{}", volume(&mut s, &mut k));
    let params = run(&mut s, &mut k, "param.list", json!({}));
    let named: Vec<f64> = params["model"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["of"].as_str().is_some_and(|o| o.contains("Sketch1")))
        .map(|p| p["value"].as_f64().unwrap())
        .collect();
    assert!(named.contains(&20.0) && named.contains(&16.0), "{params}");
    let _ = large;
    // A second centre line and the revolution must be told which; an ordinary line again and
    // there is one. Only a line can be one.
    run(&mut s, &mut k, "sketch.centerline", json!({ "sketch": sketch, "entity": lines[2] }));
    assert!(refused(&mut s, &mut k, "model.revolve", json!({ "sketch": sketch })).contains("more than one centre line"));
    run(&mut s, &mut k, "sketch.centerline", json!({ "sketch": sketch, "entity": lines[2], "on": false }));
    assert!(refused(&mut s, &mut k, "sketch.centerline", json!({ "sketch": sketch, "entity": nodes[0] })).contains("only a line"));
}

#[test]
fn a_new_sketch_on_a_face_shows_the_faces_edges_from_the_start() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (_, x, _) = block(&mut s, &mut k, 40.0);
    let top = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": x, "end": "end" } }));
    let made = run(&mut s, &mut k, "sketch.create", json!({ "face": top, "project_edges": true }));
    let sk = id(&made, "feature");
    assert_eq!(made["projected"].as_array().unwrap().len(), 4, "{made}");
    let lines = entities(&mut s, &mut k, sk, "line");
    assert!(lines.len() == 4 && lines.iter().all(|l| l["reference"] == true), "{lines:?}");
    // The outline is a profile at once: the top's own area.
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": sk }));
    assert!(near(info["regions"][0]["area"].as_f64().unwrap(), 800.0) && info["dof"] == 0, "{info}");
    // One step back and the sketch is gone, edges and all.
    assert!(s.undo());
    assert!(s.document().sketch(FeatureId(sk as u32)).is_none());
    assert!(s.redo());
    assert_eq!(s.document().projections(FeatureId(sk as u32)).len(), 4);
    // Without a kernel the caller must say what the edges are; on an origin plane there is
    // nothing to show, and that is no error.
    let e = s.exec("sketch.create", &json!({ "face": top, "project_edges": true }), None).unwrap_err().to_string();
    assert!(e.contains("needs the geometry kernel, or `edges` and `geometry`"), "{e}");
    let flat = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz", "project_edges": true }));
    assert_eq!(flat["projected"], json!([]));
    // A sketch made the plain way shows nothing of the part, as before.
    let plain = run(&mut s, &mut k, "sketch.create", json!({ "face": top }));
    assert_eq!(plain["projected"], json!([]));
    assert!(entities(&mut s, &mut k, id(&plain, "feature"), "line").is_empty());
}
