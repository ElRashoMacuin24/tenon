//! 3D sketches (DEC-136) against figures worked out by hand: a tube along a bent path, a length, a
//! position that follows the part, what a file keeps.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};

use serde_json::{Value, json};
use tenon_geom::Vec3;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::{FeatureId, FeatureKind, FeatureStatus, Session, WorkGeom, WorkPlane, regenerate};

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = s.regen(k).clone();
    if let Some((f, why)) = r.first_error() {
        panic!("{f}: {why}");
    }
    r.bodies.iter().map(|b| k.mass_properties(b.shape, 1.0).unwrap().volume).sum()
}

/// A circle of diameter `d` about the origin on the XY plane: a profile that sets off along Z.
fn profile(s: &mut Session, k: &mut OcctKernel, d: f64) -> u64 {
    let sk = run(s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(s, k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": d / 2.0 }));
    sk
}

/// From the origin up to (0,0,50), along to (50,0,50), along to (50,50,50): three lines that lie
/// in no one plane. Returns the 3D sketch's id and the three lines and four points.
fn staircase(s: &mut Session, k: &mut OcctKernel) -> (u64, [u64; 3], [u64; 4]) {
    let sk = run(s, k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    let a = run(s, k, "sketch3d.line", json!({ "sketch": sk, "start": [0, 0, 0], "end": [0, 0, 50] }));
    let b = run(s, k, "sketch3d.line", json!({ "sketch": sk, "start": a["end"], "end": [50, 0, 50] }));
    let c = run(s, k, "sketch3d.line", json!({ "sketch": sk, "start": b["end"], "end": [50, 50, 50] }));
    let id = |v: &Value| v.as_u64().unwrap();
    (sk, [id(&a["line"]), id(&b["line"]), id(&c["line"])], [id(&a["start"]), id(&a["end"]), id(&b["end"]), id(&c["end"])])
}

#[test]
fn a_tube_along_a_bent_path_in_no_one_plane_has_the_volume_of_its_length() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (path, _, p) = staircase(&mut s, &mut k);
    for corner in [p[1], p[2]] {
        run(&mut s, &mut k, "sketch3d.bend", json!({ "sketch": path, "point": corner, "radius": 10 }));
    }
    // Along its middle: 40 up, a quarter round of radius 10, 30 along, another, 40 along.
    let length = 40.0 + 30.0 + 40.0 + 2.0 * (FRAC_PI_2 * 10.0);
    let said = run(&mut s, &mut k, "sketch3d.length", json!({ "sketch": path }));
    assert!((said["length"].as_f64().unwrap() - length).abs() < 1e-7, "{said}");
    assert_eq!(said["curves"].as_array().unwrap().len(), 5);
    // A tube of diameter 10 (radius 5): pi 5^2 L, whichever way the path bends, by Pappus.
    let circle = profile(&mut s, &mut k, 10.0);
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": circle, "path_sketch": path }));
    let v = volume(&mut s, &mut k);
    let want = PI * 25.0 * length;
    assert!((v - want).abs() < 1e-3 * want, "{v} and {want}");
    // Undone, the sketch's bends and the sweep go; redone, they come back.
    assert!(s.undo());
    assert!(s.regen(&mut k).bodies.is_empty());
    assert!(s.redo());
    assert!((volume(&mut s, &mut k) - want).abs() < 1e-3 * want);
    // A larger bend: the path is shorter by what each corner gives up, 2R - pi R / 2 for each.
    let arcs: Vec<u64> = run(&mut s, &mut k, "sketch3d.info", json!({ "sketch": path }))["constraints"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["type"] == "radius")
        .map(|c| c["id"].as_u64().unwrap())
        .collect();
    assert_eq!(arcs.len(), 2);
    run(&mut s, &mut k, "sketch3d.set_dimension", json!({ "sketch": path, "constraint": arcs[0], "value": 15 }));
    let length2 = length - (2.0 * 5.0 - FRAC_PI_2 * 5.0);
    let v = volume(&mut s, &mut k);
    assert!((v - PI * 25.0 * length2).abs() < 1e-3 * want, "{v} and {}", PI * 25.0 * length2);
}

#[test]
fn a_tube_round_sharp_corners_is_mitred_and_as_long_as_its_lines_and_what_it_cannot_do_it_says() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (path, _, _) = staircase(&mut s, &mut k);
    let circle = profile(&mut s, &mut k, 10.0);
    let sweep = run(&mut s, &mut k, "model.sweep", json!({ "sketch": circle, "path_sketch": path }))["feature"].as_u64().unwrap();
    // A corner is cut on the plane halfway between its lines: as much off as on.
    let want = PI * 25.0 * 150.0;
    let v = volume(&mut s, &mut k);
    assert!((v - want).abs() < 1e-4 * want, "{v} and {want}");
    // A profile that keeps its direction cannot go round.
    let mut kind = serde_json::to_value(&s.document().feature(FeatureId(sweep as u32)).unwrap().kind).unwrap();
    kind["fixed"] = json!(true);
    run(&mut s, &mut k, "feature.update", json!({ "feature": sweep, "kind": kind }));
    let r = s.regen(&mut k);
    assert!(r.first_error().is_some_and(|(_, why)| why.contains("cannot go round a sharp corner")), "{:?}", r.first_error());
    // Nor a profile with a hole in it.
    let mut kind = serde_json::to_value(&s.document().feature(FeatureId(sweep as u32)).unwrap().kind).unwrap();
    kind["fixed"] = json!(false);
    run(&mut s, &mut k, "feature.update", json!({ "feature": sweep, "kind": kind }));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": circle, "cx": 0, "cy": 0, "r": 2 }));
    let r = s.regen(&mut k);
    assert!(r.first_error().is_some_and(|(_, why)| why.contains("takes one closed outline with no holes")), "{:?}", r.first_error());
}

#[test]
fn a_tube_along_a_spline_is_as_long_as_the_spline() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let path = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    // Starting straight up (so that the profile is square to it), and on through space.
    let line = run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": [0, 0, 0], "end": [0, 0, 30] }));
    let sp = run(&mut s, &mut k, "sketch3d.spline", json!({ "sketch": path, "points": [line["end"], [0, 15, 60], [25, 40, 75], [60, 45, 80]] }));
    for end in ["start", "end"] {
        run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "fix", "point": line[end] } }));
    }
    run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "tangent", "a": line["line"], "b": sp["spline"] } }));
    let length = run(&mut s, &mut k, "sketch3d.length", json!({ "sketch": path }))["length"].as_f64().unwrap();
    assert!(length > 100.0 && length < 200.0, "{length}");
    let circle = profile(&mut s, &mut k, 6.0);
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": circle, "path_sketch": path }));
    let v = volume(&mut s, &mut k);
    let want = PI * 9.0 * length;
    assert!((v - want).abs() < 2e-3 * want, "{v} and {want} (length {length})");
}

#[test]
fn a_path_can_be_part_of_a_3d_sketch_and_run_from_the_end_nearer_the_profile() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (path, l, _) = staircase(&mut s, &mut k);
    let circle = profile(&mut s, &mut k, 10.0);
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": circle, "path_sketch": path, "path": [l[0]] }));
    assert!((volume(&mut s, &mut k) - PI * 25.0 * 50.0).abs() < 1e-6);
}

#[test]
fn a_profile_along_the_path_says_so() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let path = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": [0, 0, 0], "end": [50, 0, 0] }));
    let circle = profile(&mut s, &mut k, 10.0);
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": circle, "path_sketch": path }));
    let r = s.regen(&mut k);
    assert!(r.first_error().is_some_and(|(_, why)| why.contains("The profile lies along the path")), "{:?}", r.first_error());
}

/// A block 40 by 20 by 10 from the origin. Returns the extrusion's feature id, its sketch and the
/// height's dimension path.
fn block(s: &mut Session, k: &mut OcctKernel) -> (u64, u64) {
    let sk = run(s, k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 20 }));
    let f = run(s, k, "model.extrude", json!({ "sketch": sk, "distance": 10 }))["feature"].as_u64().unwrap();
    (f, sk)
}

/// A reference to the edge of the block that runs along X at the top, and whether it runs from x=0.
fn top_edge(s: &mut Session, k: &mut OcctKernel, want: impl Fn(Vec3, Vec3) -> bool) -> (Value, (Vec3, Vec3)) {
    let r = s.regen(k).clone();
    let body = &r.bodies[0];
    let topo = k.topology(body.shape).unwrap();
    for e in 0..topo.edges {
        let info = k.edge_info(body.shape.edge(e)).unwrap();
        if want(info.start, info.end) {
            let eref = r.edge_ref(0, e, k).unwrap();
            return (serde_json::to_value(eref).unwrap(), (info.start, info.end));
        }
    }
    panic!("no such edge");
}

#[test]
fn a_line_from_a_vertex_follows_the_part_when_it_changes() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (extrude, _) = block(&mut s, &mut k);
    let on_top = |a: Vec3, b: Vec3| a.z == 10.0 && b.z == 10.0 && a.y == 0.0 && b.y == 0.0;
    let (edge, (start, _)) = top_edge(&mut s, &mut k, on_top);
    let path = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    // A line from the vertex at the start of the edge, straight up, 25 long.
    let line = run(
        &mut s,
        &mut k,
        "sketch3d.line",
        json!({ "sketch": path, "start": { "vertex": edge, "end": 0 }, "end": [start.x, start.y, 35], "along": "z" }),
    );
    run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "length", "line": line["line"], "value": 25 } }));
    let dof = run(&mut s, &mut k, "sketch3d.info", json!({ "sketch": path }))["dof"].as_u64().unwrap();
    assert_eq!(dof, 0, "its one end is the part's, its direction and length are held");
    let end = |s: &mut Session, k: &mut OcctKernel| {
        s.regen(k);
        let sk = s.document().sketch3d(FeatureId(path as u32)).unwrap();
        sk.point(tenon_sketch::EntityId(line["end"].as_u64().unwrap() as u32)).unwrap()
    };
    assert!((end(&mut s, &mut k) - Vec3::new(start.x, start.y, 35.0)).len() < 1e-7, "{:?}", end(&mut s, &mut k));
    // The block made 30 tall: the vertex goes up by 20, and the end of the line with it.
    let distance = serde_json::json!({ "feature": extrude, "changes": { "/extent/distance": 30.0 } });
    let _ = distance;
    s.edit(|d| {
        let FeatureKind::Extrude(e) = &mut d.feature_mut(FeatureId(extrude as u32)).unwrap().kind else { panic!() };
        e.extent = tenon_model::ExtrudeExtent::Distance(30.0);
        Ok(())
    })
    .unwrap();
    assert!((end(&mut s, &mut k) - Vec3::new(start.x, start.y, 55.0)).len() < 1e-7, "{:?}", end(&mut s, &mut k));
    // Undone, it is back where it was.
    assert!(s.undo());
    assert!((end(&mut s, &mut k) - Vec3::new(start.x, start.y, 35.0)).len() < 1e-7, "{:?}", end(&mut s, &mut k));
}

#[test]
fn a_model_edge_brought_in_follows_the_part_and_a_line_held_parallel_to_it_turns_with_it() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (extrude, _) = block(&mut s, &mut k);
    let upright = |a: Vec3, b: Vec3| (a.x - 40.0).abs() < 1e-9 && (b.x - 40.0).abs() < 1e-9 && (a.y - 20.0).abs() < 1e-9 && (b.y - 20.0).abs() < 1e-9;
    let (edge, _) = top_edge(&mut s, &mut k, upright);
    let sk = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    let included = run(&mut s, &mut k, "sketch3d.include", json!({ "sketch": sk, "edge": edge }));
    // Brought in twice: the same reference.
    let again = run(&mut s, &mut k, "sketch3d.include", json!({ "sketch": sk, "edge": edge }));
    assert_eq!(included["entity"], again["entity"]);
    let line = run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": sk, "start": [-30, 0, 0], "end": [-30, 5, 40] }));
    run(
        &mut s,
        &mut k,
        "sketch3d.constrain",
        json!({ "sketch": sk, "constraint": { "type": "parallel", "a": included["entity"], "b": line["line"] } }),
    );
    let d = |s: &Session| {
        let sketch = s.document().sketch3d(FeatureId(sk as u32)).unwrap();
        let (a, b) = sketch.line(tenon_sketch::EntityId(line["line"].as_u64().unwrap() as u32)).unwrap();
        (b - a).normalized()
    };
    assert!(d(&s).cross(Vec3::Z).len() < 1e-8, "{:?}", d(&s));
    assert_eq!(s.document().links3(FeatureId(sk as u32)).len(), 1);
    // Its length is the part's. Longer part: longer edge.
    let len = |s: &mut Session, k: &mut OcctKernel| {
        s.regen(k);
        run(s, k, "sketch3d.length", json!({ "sketch": sk, "curves": [included["entity"]] }))["length"].as_f64().unwrap()
    };
    assert!((len(&mut s, &mut k) - 10.0).abs() < 1e-9);
    s.edit(|d| {
        let FeatureKind::Extrude(e) = &mut d.feature_mut(FeatureId(extrude as u32)).unwrap().kind else { panic!() };
        e.extent = tenon_model::ExtrudeExtent::Distance(16.0);
        Ok(())
    })
    .unwrap();
    assert!((len(&mut s, &mut k) - 16.0).abs() < 1e-7, "{}", len(&mut s, &mut k));
    // Broken off, it stays where it was, and no longer follows.
    run(&mut s, &mut k, "sketch3d.break_link", json!({ "sketch": sk, "entities": [included["entity"]] }));
    assert_eq!(s.document().links3(FeatureId(sk as u32)).len(), 0);
    s.edit(|d| {
        let FeatureKind::Extrude(e) = &mut d.feature_mut(FeatureId(extrude as u32)).unwrap().kind else { panic!() };
        e.extent = tenon_model::ExtrudeExtent::Distance(30.0);
        Ok(())
    })
    .unwrap();
    assert!((len(&mut s, &mut k) - 16.0).abs() < 1e-7, "{}", len(&mut s, &mut k));
}

#[test]
fn a_circular_edge_brought_in_can_be_a_path() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    // A plate with a hole through it: the hole's rim is a whole circle of radius 15.
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 15 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5 }));
    let r = s.regen(&mut k).clone();
    let body = &r.bodies[0];
    let topo = k.topology(body.shape).unwrap();
    let rim = (0..topo.edges)
        .find(|e| {
            let i = k.edge_info(body.shape.edge(*e)).unwrap();
            i.start.z == 5.0 && (i.length - 30.0 * PI).abs() < 1e-6
        })
        .unwrap();
    let edge = serde_json::to_value(r.edge_ref(0, rim, &k).unwrap()).unwrap();
    let curves = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    let inc = run(&mut s, &mut k, "sketch3d.include", json!({ "sketch": curves, "edge": edge }));
    let l = run(&mut s, &mut k, "sketch3d.length", json!({ "sketch": curves, "curves": [inc["entity"]] }))["length"].as_f64().unwrap();
    assert!((l - 30.0 * PI).abs() < 1e-6, "{l}");
    // A small circle across the rim, swept round it: a ring of volume pi r^2 2 pi R.
    let wp = run(&mut s, &mut k, "work.plane", json!({ "by": "offset", "base": "xz", "distance": 0 }))["feature"].as_u64().unwrap();
    let round = run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp }))["feature"].as_u64().unwrap();
    // (The rim's seam is on the X axis, at x = 15, z = 5.)
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": round, "cx": 15, "cy": 5, "r": 2 }));
    let before = volume(&mut s, &mut k);
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": round, "path_sketch": curves, "path": [inc["entity"]], "operation": "new_body" }));
    let v = volume(&mut s, &mut k) - before;
    let want = PI * 4.0 * 30.0 * PI;
    assert!((v - want).abs() < 1e-2 * want, "{v} and {want}");
}

#[test]
fn work_features_from_its_points_and_lines_are_where_they_should_be() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (sk, l, p) = staircase(&mut s, &mut k);
    let point = |q: u64| json!({ "sketch3d_point": { "sketch": sk, "point": q } });
    let at = run(&mut s, &mut k, "work.point_at", json!({ "point": point(p[2]) }))["feature"].as_u64().unwrap();
    let axis = run(&mut s, &mut k, "work.axis", json!({ "axis": { "sketch3d": sk, "line": l[1] } }))["feature"].as_u64().unwrap();
    let plane = run(&mut s, &mut k, "work.plane", json!({ "by": "three_points", "points": [point(p[0]), point(p[1]), point(p[2])] }))["feature"]
        .as_u64()
        .unwrap();
    let square =
        run(&mut s, &mut k, "work.plane", json!({ "by": "square_to_curve", "sketch": sk, "curve": l[2], "end": p[3] }))["feature"].as_u64().unwrap();
    let r = s.regen(&mut k);
    assert_eq!(r.first_error(), None);
    let work = |id: u64| r.work.get(&FeatureId(id as u32)).copied().unwrap();
    let WorkGeom::Point(q) = work(at) else { panic!() };
    assert!(q.dist(Vec3::new(50.0, 0.0, 50.0)) < 1e-9, "{q:?}");
    let WorkGeom::Axis(a) = work(axis) else { panic!() };
    assert!(a.dir().cross(Vec3::X).len() < 1e-9 && (a.origin() - Vec3::new(0.0, 0.0, 50.0)).cross(Vec3::X).len() < 1e-9, "{a:?}");
    let WorkGeom::Plane(f) = work(plane) else { panic!() };
    assert!(f.z().cross(Vec3::Y).len() < 1e-9, "the XZ plane: {:?}", f.z());
    let WorkGeom::Plane(f) = work(square) else { panic!() };
    assert!(f.z().cross(Vec3::Y).len() < 1e-9 && f.origin().dist(Vec3::new(50.0, 50.0, 50.0)) < 1e-9, "{f:?}");
    assert!(matches!(s.document().feature(FeatureId(square as u32)).unwrap().kind, FeatureKind::WorkPlane(WorkPlane::SquareToCurve { .. })));
    // A curve that is no end of it: refused.
    let e =
        s.exec("work.plane", &json!({ "by": "square_to_curve", "sketch": sk, "curve": l[0], "end": p[3] }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("is no end of a curve"), "{e}");
}

#[test]
fn a_profile_on_a_plane_square_to_the_path_makes_a_tube_that_starts_there() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    // The path sets off at an angle to everything: from (10,20,30) by (3,4,12), 13 long, times 4.
    let sk = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    let line = run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": sk, "start": [10, 20, 30], "end": [22, 36, 78] }));
    let plane =
        run(&mut s, &mut k, "work.plane", json!({ "by": "square_to_curve", "sketch": sk, "curve": line["line"], "end": line["start"] }))["feature"]
            .as_u64()
            .unwrap();
    let on = run(&mut s, &mut k, "sketch.create", json!({ "work_plane": plane }))["feature"].as_u64().unwrap();
    // (The plane's own origin is the end of the line.)
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": on, "cx": 0, "cy": 0, "r": 5 }));
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": on, "path_sketch": sk }));
    let v = volume(&mut s, &mut k);
    let want = PI * 25.0 * 52.0;
    assert!((v - want).abs() < 1e-6 * want, "{v} and {want}");
    // The path dragged longer: the tube with it, and the plane still square to it.
    run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": sk, "constraint": { "type": "fix", "point": line["start"] } }));
    run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": line["line"], "value": 104 } }));
    let v = volume(&mut s, &mut k);
    assert!((v - 2.0 * want).abs() < 1e-5 * want, "{v} and {}", 2.0 * want);
}

#[test]
fn a_3d_sketch_can_be_suppressed_deleted_and_its_dimensions_are_parameters() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (path, l, _) = staircase(&mut s, &mut k);
    let c = run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "length", "line": l[0], "value": 50 } }));
    // Named as a sketch's are, and driven by an equation.
    run(&mut s, &mut k, "param.add", json!({ "name": "rise", "equation": "70 mm" }));
    let named = run(&mut s, &mut k, "sketch3d.set_dimension", json!({ "sketch": path, "constraint": c["constraint"], "equation": "rise" }));
    assert!(named["name"].is_string(), "{named}");
    let sketch = s.document().sketch3d(FeatureId(path as u32)).unwrap();
    let (a, b) = sketch.line(tenon_sketch::EntityId(l[0] as u32)).unwrap();
    assert!((a.dist(b) - 70.0).abs() < 1e-7, "{}", a.dist(b));
    // Suppressed: the sweep along it cannot be, and says so.
    let circle = profile(&mut s, &mut k, 6.0);
    let sweep = run(&mut s, &mut k, "model.sweep", json!({ "sketch": circle, "path_sketch": path }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": path }));
    let r = s.regen(&mut k);
    assert!(matches!(r.status_of(FeatureId(path as u32)), Some(FeatureStatus::Suppressed)));
    assert!(r.first_error().is_some_and(|(f, why)| f.0 == sweep as u32 && why.contains("suppressed")), "{:?}", r.first_error());
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": path, "suppressed": false }));
    assert_eq!(s.regen(&mut k).first_error(), None);
    // Shown or hidden.
    run(&mut s, &mut k, "feature.visibility", json!({ "feature": path, "visible": false }));
    assert_eq!(s.document().feature(FeatureId(path as u32)).unwrap().visible, Some(false));
    // Deleted while the sweep needs it: refused; with the sweep: fine.
    assert!(s.exec("feature.delete", &json!({ "feature": path }), Some(&mut k)).unwrap_err().to_string().contains("is used by"));
    assert_eq!(s.document().deletion(FeatureId(sweep as u32)).consumed.len(), 2, "the profile and the path go with it, if asked");
    run(&mut s, &mut k, "feature.delete", json!({ "feature": sweep }));
    run(&mut s, &mut k, "feature.delete", json!({ "feature": path }));
    assert!(s.document().feature(FeatureId(path as u32)).is_none());
}

#[test]
fn hostile_input_is_refused_with_nothing_changed() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (sk, l, p) = staircase(&mut s, &mut k);
    let before = s.document().clone();
    for (id, params) in [
        ("sketch3d.line", json!({ "sketch": sk, "start": [0, 0, 0], "end": [0, 0, 0] })),
        ("sketch3d.line", json!({ "sketch": sk, "start": [1e300, 0, 0], "end": [0, 0, 0] })),
        ("sketch3d.line", json!({ "sketch": 999, "start": [1, 0, 0], "end": [0, 0, 0] })),
        ("sketch3d.spline", json!({ "sketch": sk, "points": [[0, 0, 0]] })),
        ("sketch3d.spline", json!({ "sketch": sk, "points": [[0, 0, 0], [0, 0, 0], [1, 0, 0]] })),
        ("sketch3d.bend", json!({ "sketch": sk, "point": p[1], "radius": -1 })),
        ("sketch3d.bend", json!({ "sketch": sk, "point": p[1], "radius": 1e9 })),
        ("sketch3d.constrain", json!({ "sketch": sk, "constraint": { "type": "length", "line": l[0], "value": -5 } })),
        ("sketch3d.constrain", json!({ "sketch": sk, "constraint": { "type": "tangent", "a": l[0], "b": l[1] } })),
        ("sketch3d.move", json!({ "sketch": sk, "point": l[0], "at": [0, 0, 1] })),
        ("sketch3d.length", json!({ "sketch": sk, "curves": [p[0]] })),
        ("sketch3d.point", json!({ "sketch": sk, "at": { "work": 999 } })),
    ] {
        assert!(s.exec(id, &params, Some(&mut k)).is_err(), "{id} {params}");
        assert_eq!(s.document(), &before, "{id} {params}");
    }
}

#[test]
fn it_regenerates_with_nothing_but_the_document() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (path, _, p) = staircase(&mut s, &mut k);
    run(&mut s, &mut k, "sketch3d.bend", json!({ "sketch": path, "point": p[1], "radius": 10 }));
    let r = regenerate(s.document(), &mut k);
    assert_eq!(r.first_error(), None);
    assert_eq!(r.sketches3d.get(&FeatureId(path as u32)).map(|a| a.as_ref()), s.document().sketch3d(FeatureId(path as u32)));
}
