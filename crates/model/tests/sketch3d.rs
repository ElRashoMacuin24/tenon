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

/// A tube of radius 5 along the staircase, a sweep already there: hand, pi 25 L.
fn close_to(v: f64, length: f64) {
    let want = PI * 25.0 * length;
    assert!((v - want).abs() < 1e-6 * want, "{v} and {want}");
}

#[test]
fn a_bend_made_or_deleted_under_a_sweep_keeps_the_sweep_and_its_volume_follows() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (path, l, p) = staircase(&mut s, &mut k);
    let circle = profile(&mut s, &mut k, 10.0);
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": circle, "path_sketch": path }));
    close_to(volume(&mut s, &mut k), 150.0);
    // Rounded under it: the path gives up 2R and gets back a quarter of a circle.
    let bend = run(&mut s, &mut k, "sketch3d.bend", json!({ "sketch": path, "point": p[1], "radius": 10 }));
    let rounded = 150.0 - 20.0 + FRAC_PI_2 * 10.0;
    close_to(volume(&mut s, &mut k), rounded);
    // Undone and done again, as one edit.
    assert!(s.undo());
    close_to(volume(&mut s, &mut k), 150.0);
    assert!(s.redo());
    close_to(volume(&mut s, &mut k), rounded);
    // A larger radius goes on being swept: 2R - pi R / 2 less for each.
    run(&mut s, &mut k, "sketch3d.set_dimension", json!({ "sketch": path, "constraint": bend["radius"], "value": 20 }));
    close_to(volume(&mut s, &mut k), 150.0 - 40.0 + FRAC_PI_2 * 20.0);
    // Deleted: the corner is back, and the path is its three lines.
    run(&mut s, &mut k, "sketch3d.delete", json!({ "sketch": path, "entities": [bend["arc"]] }));
    close_to(volume(&mut s, &mut k), 150.0);
    // By its centre or its corner: the same.
    for of in ["center", "corner"] {
        let bend = run(&mut s, &mut k, "sketch3d.bend", json!({ "sketch": path, "point": p[1], "radius": 10 }));
        close_to(volume(&mut s, &mut k), rounded);
        run(&mut s, &mut k, "sketch3d.delete", json!({ "sketch": path, "entities": [bend[of]] }));
        close_to(volume(&mut s, &mut k), 150.0);
    }
    // A line drawn on from the end of the path: the tube goes on along it, 50 more.
    let on = run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": p[3], "end": [50, 50, 100] }));
    close_to(volume(&mut s, &mut k), 200.0);
    // A line away from it, or off its middle: the sweep stays where it was.
    run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": [90, 90, 90], "end": [90, 90, 120] }));
    run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": p[1], "end": [-30, 0, 50] }));
    close_to(volume(&mut s, &mut k), 200.0);
    // A curve of the path taken from the middle: said, and it says what to do.
    run(&mut s, &mut k, "sketch3d.delete", json!({ "sketch": path, "entities": [l[1]] }));
    let r = s.regen(&mut k);
    let why = r.first_error().map(|e| e.1.to_owned()).unwrap_or_default();
    assert!(why.contains("do not join") && why.contains("choose its path again"), "{why}");
    let _ = on;
}

#[test]
fn a_path_chosen_as_part_of_a_sketch_stays_that_part() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (path, l, p) = staircase(&mut s, &mut k);
    let circle = profile(&mut s, &mut k, 10.0);
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": circle, "path_sketch": path, "path": [l[0], l[1]] }));
    close_to(volume(&mut s, &mut k), 100.0);
    // A line on from the far end of the whole sketch: no part of this path.
    run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": p[3], "end": [50, 50, 100] }));
    close_to(volume(&mut s, &mut k), 100.0);
    // A bend between a line of the path and one that is not: it is cut short, and that is all.
    run(&mut s, &mut k, "sketch3d.bend", json!({ "sketch": path, "point": p[2], "radius": 10 }));
    close_to(volume(&mut s, &mut k), 90.0);
    // All of it gone: said, and nothing to save that cannot be read.
    run(&mut s, &mut k, "sketch3d.delete", json!({ "sketch": path, "entities": [l[0], l[1]] }));
    let r = s.regen(&mut k);
    let why = r.first_error().map(|e| e.1.to_owned()).unwrap_or_default();
    assert!(why.contains("has gone from the sketch") && why.contains("choose its path again"), "{why}");
    s.document().validate().unwrap();
}

#[test]
fn a_radius_or_a_length_that_cannot_fit_is_refused_with_nothing_changed() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    // Three lines 50 long along their axes, from a fixed start.
    let (path, l, p) = staircase(&mut s, &mut k);
    run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "fix", "point": p[0] } }));
    for (line, axis) in [(l[0], "z"), (l[1], "x"), (l[2], "y")] {
        run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "along", "line": line, "axis": axis } }));
    }
    let long = run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "length", "line": l[0], "value": 50 } }));
    let bend = run(&mut s, &mut k, "sketch3d.bend", json!({ "sketch": path, "point": p[1], "radius": 10 }));
    let before = s.document().clone();
    let e = s
        .exec("sketch3d.set_dimension", &json!({ "sketch": path, "constraint": bend["radius"], "value": 70 }), Some(&mut k))
        .unwrap_err()
        .to_string();
    assert!(e.contains("the largest radius that fits is just under 50.000 mm"), "{e}");
    assert_eq!(s.document(), &before);
    let e = s
        .exec("sketch3d.set_dimension", &json!({ "sketch": path, "constraint": long["constraint"], "value": 5 }), Some(&mut k))
        .unwrap_err()
        .to_string();
    assert!(e.contains("the largest radius that fits is just under 5.000 mm"), "{e}");
    assert_eq!(s.document(), &before);
    // And what would come by an equation.
    run(&mut s, &mut k, "param.add", json!({ "name": "r", "equation": "10 mm" }));
    run(&mut s, &mut k, "sketch3d.set_dimension", json!({ "sketch": path, "constraint": bend["radius"], "equation": "r" }));
    let e = s.exec("param.set", &json!({ "name": "r", "equation": "70 mm" }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("the largest radius that fits"), "{e}");
    // What fits fits: at 40, the line stops 10 from where it starts.
    run(&mut s, &mut k, "param.set", json!({ "name": "r", "equation": "40 mm" }));
    let sk = s.document().sketch3d(FeatureId(path as u32)).unwrap();
    let (a, b) = sk.line(tenon_sketch::EntityId(l[0] as u32)).unwrap();
    assert!(a.dist(Vec3::ZERO) < 1e-7 && b.dist(Vec3::new(0.0, 0.0, 10.0)) < 1e-7, "{a:?} {b:?}");
}

#[test]
fn a_part_that_changes_so_that_a_bend_no_longer_fits_says_so() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    // A line from a corner of the block, up 20, with its far end fixed, and on along X: bent.
    let (extrude, _) = block(&mut s, &mut k);
    let on_top = |a: Vec3, b: Vec3| a.z == 10.0 && b.z == 10.0 && a.y == 0.0 && b.y == 0.0;
    let (edge, _) = top_edge(&mut s, &mut k, on_top);
    let path = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    let up = run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": { "vertex": edge, "end": 0 }, "end": [40, 0, 30] }));
    let on = run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": up["end"], "end": [90, 0, 30] }));
    for q in [&up["end"], &on["end"]] {
        run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "fix", "point": q } }));
    }
    let bend = run(&mut s, &mut k, "sketch3d.bend", json!({ "sketch": path, "point": up["end"], "radius": 10 }));
    assert_eq!(s.regen(&mut k).first_error(), None);
    let _ = bend;
    // The block 25 tall: the corner is 5 from the line's start, and the bend needs 10 of it.
    s.edit(|d| {
        let FeatureKind::Extrude(e) = &mut d.feature_mut(FeatureId(extrude as u32)).unwrap().kind else { panic!() };
        e.extent = tenon_model::ExtrudeExtent::Distance(25.0);
        Ok(())
    })
    .unwrap();
    let r = s.regen(&mut k);
    let why = r.first_error().map(|e| e.1.to_owned()).unwrap_or_default();
    assert!(why.contains("cannot follow") && why.contains("largest radius that fits"), "{why}");
}

#[test]
fn a_plane_square_to_a_round_edge_is_square_to_the_circle_and_the_ring_is_whole() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    // A disc of radius 30 and height 10. Its rim's seam is at (30, 0, 10), where the circle's
    // tangent is along Y: a chord of what is drawn leans 2.81 degrees off that.
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 30 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 10 }));
    let round = |a: Vec3, b: Vec3| a.z == 10.0 && a.dist(b) < 1e-9;
    let (edge, (seam, _)) = top_edge(&mut s, &mut k, round);
    assert!(seam.dist(Vec3::new(30.0, 0.0, 10.0)) < 1e-9, "{seam:?}");
    let path = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    let rim = run(&mut s, &mut k, "sketch3d.include", json!({ "sketch": path, "edge": edge }));
    let plane = run(&mut s, &mut k, "work.plane", json!({ "by": "square_to_curve", "sketch": path, "curve": rim["entity"], "end": rim["start"] }));
    let r = s.regen(&mut k);
    let Some(WorkGeom::Plane(f)) = r.work.get(&FeatureId(plane["feature"].as_u64().unwrap() as u32)).copied() else { panic!() };
    assert!(f.z().cross(Vec3::Y).len() < 1e-12, "{:?}", f.z());
    // A ring of radius 2 from there: pi 4 by 2 pi 30, to a part in a million, and no short.
    let section = run(&mut s, &mut k, "sketch.create", json!({ "work_plane": plane["feature"] }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": section, "cx": 0, "cy": 0, "r": 2 }));
    let before = volume(&mut s, &mut k);
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": section, "path_sketch": path, "operation": "new_body" }));
    let v = volume(&mut s, &mut k) - before;
    let want = PI * 4.0 * 2.0 * PI * 30.0;
    assert!((v - want).abs() < 1e-6 * want, "{v} and {want}");
    // A line of the sketch's own does not make the rim part of what a sweep takes (it is closed)...
    let line = run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": rim["start"], "end": [29, 40, 10] }));
    let said = run(&mut s, &mut k, "sketch3d.length", json!({ "sketch": path }));
    assert_eq!(said["curves"].as_array().unwrap().len(), 1, "{said}");
    // ...and held tangent to it, it lies along the circle's tangent: its end at x = 30, not 28.035.
    run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "tangent", "a": line["line"], "b": rim["entity"] } }));
    let at = s.document().sketch3d(FeatureId(path as u32)).unwrap().point(tenon_sketch::EntityId(line["end"].as_u64().unwrap() as u32)).unwrap();
    assert!((at.x - 30.0).abs() < 1e-8, "{at:?}");
}

#[test]
fn a_vertex_stays_the_same_corner_whichever_way_the_kernel_names_its_edge() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (extrude, base) = block(&mut s, &mut k);
    let on_top = |a: Vec3, b: Vec3| a.z == 10.0 && b.z == 10.0 && a.y == 0.0 && b.y == 0.0;
    let (edge, (start, end)) = top_edge(&mut s, &mut k, on_top);
    assert_eq!((start.x, end.x), (40.0, 0.0), "as the kernel has it now");
    let path = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    let p0 = run(&mut s, &mut k, "sketch3d.point", json!({ "sketch": path, "at": { "vertex": edge, "end": 0 } }))["point"].as_u64().unwrap();
    let p1 = run(&mut s, &mut k, "sketch3d.point", json!({ "sketch": path, "at": { "vertex": edge, "end": 1 } }))["point"].as_u64().unwrap();
    let brought = run(&mut s, &mut k, "sketch3d.include", json!({ "sketch": path, "edge": edge }));
    let vertex = |end: u64| json!({ "vertex": { "edge": edge, "end": end } });
    let wp = run(&mut s, &mut k, "work.point_at", json!({ "point": vertex(0) }))["feature"].as_u64().unwrap();
    let plane =
        run(&mut s, &mut k, "work.plane", json!({ "by": "three_points", "points": [vertex(0), vertex(1), "origin"] }))["feature"].as_u64().unwrap();
    let at = |s: &mut Session, k: &mut OcctKernel, p: u64| {
        assert_eq!(s.regen(k).first_error(), None);
        s.document().sketch3d(FeatureId(path as u32)).unwrap().point(tenon_sketch::EntityId(p as u32)).unwrap()
    };
    let (e0, e1) = (brought["start"].as_u64().unwrap(), brought["end"].as_u64().unwrap());
    let work = |s: &mut Session, k: &mut OcctKernel| match s.regen(k).work.get(&FeatureId(wp as u32)) {
        Some(WorkGeom::Point(q)) => *q,
        other => panic!("{other:?}"),
    };
    // The block made the other way from its sketch: by hand, the same corners 20 lower.
    s.edit(|d| {
        let FeatureKind::Extrude(e) = &mut d.feature_mut(FeatureId(extrude as u32)).unwrap().kind else { panic!() };
        e.reverse = true;
        Ok(())
    })
    .unwrap();
    for (p, x) in [(p0, 40.0), (p1, 0.0), (e0, 40.0), (e1, 0.0)] {
        let q = at(&mut s, &mut k, p);
        assert!(q.dist(Vec3::new(x, 0.0, -10.0)) < 1e-9, "{p}: {q:?}");
    }
    assert!(work(&mut s, &mut k).dist(Vec3::new(40.0, 0.0, -10.0)) < 1e-9);
    // The plane through the two corners and the origin: it starts at the first, heading for the second.
    let r = s.regen(&mut k);
    let Some(WorkGeom::Plane(f)) = r.work.get(&FeatureId(plane as u32)).copied() else { panic!() };
    assert!(f.origin().dist(Vec3::new(40.0, 0.0, -10.0)) < 1e-9 && f.x().dist(-Vec3::X) < 1e-9, "{f:?}");
    s.undo();
    // Ten times as wide: the end at 40 is nearer the other, and is the far one still.
    let info = run(&mut s, &mut k, "sketch.info", json!({ "sketch": base }));
    let far = info["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "point" && e["pos"]["x"] == 40.0 && e["pos"]["y"] == 0.0)
        .map(|e| e["id"].clone())
        .unwrap();
    run(&mut s, &mut k, "sketch.drag", json!({ "sketch": base, "point": far, "x": 400, "y": 0 }));
    for (p, x) in [(p0, 400.0), (p1, 0.0), (e0, 400.0), (e1, 0.0)] {
        let q = at(&mut s, &mut k, p);
        assert!(q.dist(Vec3::new(x, 0.0, 10.0)) < 1e-6, "{p}: {q:?}");
    }
}

#[test]
fn a_plane_square_to_a_line_goes_with_the_line_when_its_corner_is_rounded() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (path, l, p) = staircase(&mut s, &mut k);
    let plane = run(&mut s, &mut k, "work.plane", json!({ "by": "square_to_curve", "sketch": path, "curve": l[0], "end": p[1] }))["feature"]
        .as_u64()
        .unwrap();
    let origin = |s: &mut Session, k: &mut OcctKernel| {
        let r = s.regen(k);
        assert_eq!(r.first_error(), None);
        let Some(WorkGeom::Plane(f)) = r.work.get(&FeatureId(plane as u32)).copied() else { panic!() };
        assert!(f.z().cross(Vec3::Z).len() < 1e-9, "{f:?}");
        f.origin()
    };
    assert!(origin(&mut s, &mut k).dist(Vec3::new(0.0, 0.0, 50.0)) < 1e-9);
    let bend = run(&mut s, &mut k, "sketch3d.bend", json!({ "sketch": path, "point": p[1], "radius": 10 }));
    assert!(origin(&mut s, &mut k).dist(Vec3::new(0.0, 0.0, 40.0)) < 1e-9, "where the line stops now");
    run(&mut s, &mut k, "sketch3d.delete", json!({ "sketch": path, "entities": [bend["arc"]] }));
    assert!(origin(&mut s, &mut k).dist(Vec3::new(0.0, 0.0, 50.0)) < 1e-9, "and back at the corner");
}

#[test]
fn what_a_sweep_can_go_round_it_goes_round_and_what_it_cannot_it_says() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    // A square loop 100 on a side, bent at three corners, sharp at the one it closes at.
    let path = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    let a = run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": [0, 0, 0], "end": [0, 100, 0] }));
    let b = run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": a["end"], "end": [100, 100, 0] }));
    let c = run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": b["end"], "end": [100, 0, 0] }));
    run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": c["end"], "end": a["start"] }));
    for q in [&a["end"], &b["end"], &c["end"]] {
        run(&mut s, &mut k, "sketch3d.bend", json!({ "sketch": path, "point": q, "radius": 10 }));
    }
    let plane = run(&mut s, &mut k, "work.plane", json!({ "by": "offset", "base": "xz", "distance": -50 }))["feature"].as_u64().unwrap();
    let section = run(&mut s, &mut k, "sketch.create", json!({ "work_plane": plane }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": section, "cx": 0, "cy": 0, "r": 5 }));
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": section, "path_sketch": path }));
    close_to(volume(&mut s, &mut k), 400.0 - 3.0 * 20.0 + 3.0 * FRAC_PI_2 * 10.0);

    // A sharp corner at a spline: said, and said what to do.
    let refused = |s: &mut Session, k: &mut OcctKernel, sketch: &str| {
        let r = s.regen(k);
        let why = r.first_error().map(|e| e.1.to_owned()).unwrap_or_default();
        assert!(why.contains(sketch), "{why}");
    };
    let mut s = Session::default();
    let line = run_fresh_path(&mut s, &mut k);
    let path = line.0;
    run(&mut s, &mut k, "sketch3d.spline", json!({ "sketch": path, "points": [line.1["end"], [100, 0, 100], [200, 20, 100]] }));
    let circle = profile(&mut s, &mut k, 2.0);
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": circle, "path_sketch": path }));
    refused(&mut s, &mut k, "goes round a sharp corner only between two straight lines: make the two tangent");
    // A spline closed on itself has a corner where it closes.
    let mut s = Session::default();
    let path = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch3d.spline", json!({ "sketch": path, "points": [[30, 0, 0], [0, 30, 0], [-30, 0, 0], [0, -30, 0], [30, 0, 0]] }));
    let section = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": section, "cx": 30, "cy": 0, "r": 2 }));
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": section, "path_sketch": path }));
    refused(&mut s, &mut k, "closes on itself with a sharp corner at a curve");
    // A sharp corner further along than a spline, between lines: as long as it all is.
    let mut s = Session::default();
    let (path, l) = run_fresh_path(&mut s, &mut k);
    run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "fix", "point": l["start"] } }));
    run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "along", "line": l["line"], "axis": "z" } }));
    let sp = run(&mut s, &mut k, "sketch3d.spline", json!({ "sketch": path, "points": [l["end"], [10, 0, 200], [100, 20, 250], [200, 40, 250]] }));
    run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "tangent", "a": l["line"], "b": sp["spline"] } }));
    let first = run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": l["start"], "end": [0, 80, 0] }));
    let section = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": section, "cx": 0, "cy": 0, "r": 1 }));
    let curves = json!([first["line"], l["line"], sp["spline"]]);
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": section, "path_sketch": path, "path": curves }));
    let length = run(&mut s, &mut k, "sketch3d.length", json!({ "sketch": path, "curves": curves }))["length"].as_f64().unwrap();
    let v = volume(&mut s, &mut k);
    assert!((v - PI * length).abs() < 1e-3 * PI * length, "{v} and {}", PI * length);
}

/// A new 3D sketch with one line up Z, 100 long.
fn run_fresh_path(s: &mut Session, k: &mut OcctKernel) -> (u64, Value) {
    let path = run(s, k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    let line = run(s, k, "sketch3d.line", json!({ "sketch": path, "start": [0, 0, 0], "end": [0, 0, 100] }));
    (path, line)
}

#[test]
fn a_curved_edge_is_a_path_alone_or_held_tangent_to_a_line_and_said_to_be_neither_otherwise() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    // Half of an ellipse's rim: neither straight nor round, so a spline through its mesh.
    let sk = run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.ellipse", json!({ "sketch": sk, "cx": 0, "cy": 0, "x": 30, "y": 0, "minor": 20 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 12 }));
    run(&mut s, &mut k, "model.split", json!({ "plane": "xz", "keep": "front" }));
    let found = run(&mut s, &mut k, "model.find", json!({ "what": "edges", "kind": "other", "most": "top" }));
    let path = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    let inc = run(&mut s, &mut k, "sketch3d.include", json!({ "sketch": path, "edge": found["found"][0]["reference"] }));
    let edge = run(&mut s, &mut k, "sketch3d.length", json!({ "sketch": path, "curves": [inc["entity"]] }))["length"].as_f64().unwrap();
    assert!((edge - 79.327).abs() < 0.01, "half of 158.65: {edge}");
    let plane =
        run(&mut s, &mut k, "work.plane", json!({ "by": "square_to_curve", "sketch": path, "curve": inc["entity"], "end": inc["start"] }))["feature"]
            .as_u64()
            .unwrap();
    let section = run(&mut s, &mut k, "sketch.create", json!({ "work_plane": plane }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": section, "cx": 0, "cy": 0, "r": 1 }));
    let before = volume(&mut s, &mut k);
    // Alone, by default, there being nothing else: pi L, to a part in ten thousand.
    let sweep =
        run(&mut s, &mut k, "model.sweep", json!({ "sketch": section, "path_sketch": path, "operation": "new_body" }))["feature"].as_u64().unwrap();
    let v = volume(&mut s, &mut k) - before;
    assert!((v - PI * edge).abs() < 1e-4 * PI * edge, "{v} and {}", PI * edge);
    // With a line off its end, sharp: refused, in words.
    let line = run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": inc["end"], "end": [-20, 35, 12] }));
    let both = json!([inc["entity"], line["line"]]);
    let mut kind = serde_json::to_value(&s.document().feature(FeatureId(sweep as u32)).unwrap().kind).unwrap();
    kind["path"]["curves"] = both.clone();
    run(&mut s, &mut k, "feature.update", json!({ "feature": sweep, "kind": kind }));
    let why = s.regen(&mut k).first_error().map(|e| e.1.to_owned()).unwrap_or_default();
    assert!(why.contains("make the two tangent"), "{why}");
    // Held tangent to it (it comes to that end going +Y, by the mesh's own tangent): pi (L + l).
    run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "tangent", "a": line["line"], "b": inc["entity"] } }));
    let all = run(&mut s, &mut k, "sketch3d.length", json!({ "sketch": path, "curves": both }))["length"].as_f64().unwrap();
    let v = volume(&mut s, &mut k) - before;
    assert!((v - PI * all).abs() < 1e-3 * PI * all, "{v} and {}", PI * all);
}

#[test]
fn edges_brought_in_are_a_path_by_themselves_and_with_what_joins_them() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    block(&mut s, &mut k);
    let along_x = |a: Vec3, b: Vec3| a.z == 10.0 && b.z == 10.0 && a.y == 0.0 && b.y == 0.0;
    let along_y = |a: Vec3, b: Vec3| a.z == 10.0 && b.z == 10.0 && a.x == 0.0 && b.x == 0.0;
    let (ex, _) = top_edge(&mut s, &mut k, along_x);
    let (ey, _) = top_edge(&mut s, &mut k, along_y);
    let path = run(&mut s, &mut k, "sketch.new_3d", json!({}))["feature"].as_u64().unwrap();
    let a = run(&mut s, &mut k, "sketch3d.include", json!({ "sketch": path, "edge": ex }));
    let b = run(&mut s, &mut k, "sketch3d.include", json!({ "sketch": path, "edge": ey }));
    // The two meet at a corner of the block: 40 and 20.
    let said = run(&mut s, &mut k, "sketch3d.length", json!({ "sketch": path }));
    assert!((said["length"].as_f64().unwrap() - 60.0).abs() < 1e-9, "{said}");
    // A line of its own on from one of them joins the three: 30 more.
    let sk = s.document().sketch3d(FeatureId(path as u32)).unwrap();
    let end_of_b = if sk.point(tenon_sketch::EntityId(b["start"].as_u64().unwrap() as u32)).unwrap().y > 10.0 { &b["start"] } else { &b["end"] };
    run(&mut s, &mut k, "sketch3d.line", json!({ "sketch": path, "start": end_of_b, "end": [0, 20, 40] }));
    let said = run(&mut s, &mut k, "sketch3d.length", json!({ "sketch": path }));
    assert!((said["length"].as_f64().unwrap() - 90.0).abs() < 1e-9, "{said}");
    // A tube along all of it, from a plane square to its far end: pi by 90, mitred round the two corners.
    let start_of_a = if sk_x(&s, path, &a["start"]) > 10.0 { &a["start"] } else { &a["end"] };
    let plane =
        run(&mut s, &mut k, "work.plane", json!({ "by": "square_to_curve", "sketch": path, "curve": a["entity"], "end": start_of_a }))["feature"]
            .as_u64()
            .unwrap();
    let section = run(&mut s, &mut k, "sketch.create", json!({ "work_plane": plane }))["feature"].as_u64().unwrap();
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": section, "cx": 0, "cy": 0, "r": 1 }));
    let before = volume(&mut s, &mut k);
    run(&mut s, &mut k, "model.sweep", json!({ "sketch": section, "path_sketch": path, "operation": "new_body" }));
    let v = volume(&mut s, &mut k) - before;
    assert!((v - PI * 90.0).abs() < 1e-6 * PI * 90.0, "{v}");
    // Marked as construction it is not taken: what is left of the run does not reach it.
    run(&mut s, &mut k, "sketch3d.construction", json!({ "sketch": path, "entity": b["entity"] }));
    let said = run(&mut s, &mut k, "sketch3d.length", json!({ "sketch": path }));
    assert!((said["length"].as_f64().unwrap() - 30.0).abs() < 1e-9, "{said}");
}

/// The X of a point of a 3D sketch.
fn sk_x(s: &Session, sketch: u64, point: &Value) -> f64 {
    s.document().sketch3d(FeatureId(sketch as u32)).unwrap().point(tenon_sketch::EntityId(point.as_u64().unwrap() as u32)).unwrap().x
}

#[test]
fn a_3d_sketch_is_no_profile_and_says_so_before_there_is_a_file_that_cannot_be_read() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (path, _, _) = staircase(&mut s, &mut k);
    let plane = run(&mut s, &mut k, "work.plane", json!({ "by": "offset", "base": "xy", "distance": 5 }))["feature"].as_u64().unwrap();
    let (extrude, _) = block(&mut s, &mut k);
    let before = s.document().clone();
    for (id, params, says) in [
        ("model.extrude", json!({ "sketch": path, "distance": 5 }), "is a 3D sketch"),
        ("model.extrude", json!({ "sketch": plane, "distance": 5 }), "is not a sketch"),
        ("model.sweep", json!({ "sketch": path, "path_sketch": path }), "is a 3D sketch"),
        ("model.sweep", json!({ "sketch": plane, "path_sketch": path, "path": [1] }), "is not a sketch"),
        ("model.revolve", json!({ "sketch": path, "axis": "z" }), "is a 3D sketch"),
        ("model.coil", json!({ "sketch": path, "axis": "z", "pitch": 10, "turns": 2 }), "is a 3D sketch"),
    ] {
        let e = s.exec(id, &params, Some(&mut k)).unwrap_err().to_string();
        assert!(e.contains(says), "{id} {params}: {e}");
        assert_eq!(s.document(), &before);
    }
    // Asked for by its whole definition too.
    let mut kind = serde_json::to_value(&s.document().feature(FeatureId(extrude as u32)).unwrap().kind).unwrap();
    kind["sketch"] = json!(path);
    let e = s.exec("feature.add", &json!({ "kind": kind }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("is a 3D sketch"), "{e}");
}

#[test]
fn a_point_that_cannot_move_says_why() {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    let (path, _, p) = staircase(&mut s, &mut k);
    run(&mut s, &mut k, "sketch3d.constrain", json!({ "sketch": path, "constraint": { "type": "fix", "point": p[0] } }));
    let e = s.exec("sketch3d.move", &json!({ "sketch": path, "point": p[0], "at": [1, 2, 3] }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("is fixed"), "{e}");
    let on = run(&mut s, &mut k, "sketch3d.point", json!({ "sketch": path, "at": "origin" }))["point"].clone();
    let e = s.exec("sketch3d.move", &json!({ "sketch": path, "point": on, "at": [1, 2, 3] }), Some(&mut k)).unwrap_err().to_string();
    assert!(e.contains("follows the part"), "{e}");
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
