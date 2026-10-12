//! Sheet metal (M8): faces, flanges with their reliefs, hems, seams, cuts across bends and the
//! flat pattern, against bend allowances worked out by hand.
//!
//! Everywhere here the sheet is 2 mm thick, bends have an inside radius of 3 mm and the K
//! factor is 0.44 unless a test says otherwise. A bend that turns the sheet by an angle `a` is
//! `a * (3 + 0.44 * 2)` wide in the flat: 1.94 pi for a quarter turn.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use serde_json::{Value, json};
use tenon_geom::Vec3;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

const T: f64 = 2.0;
const R: f64 = 3.0;
const K: f64 = 0.44;

/// The width in the flat of a bend through `angle`.
fn allowance(angle: f64) -> f64 {
    angle * (R + K * T)
}

/// The volume of a bend through `angle`, `width` wide.
fn bend_volume(angle: f64, width: f64) -> f64 {
    angle / 2.0 * ((R + T) * (R + T) - R * R) * width
}

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> String {
    s.exec(id, &p, Some(k)).unwrap_err().to_string()
}

fn id(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or_else(|| panic!("no `{key}` in {v}"))
}

fn num(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or_else(|| panic!("no number `{key}` in {v}"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1.0)
}

/// The part regenerates without an error; its volume.
fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

/// The message of the feature that fails.
fn failure(s: &mut Session, k: &mut OcctKernel) -> String {
    let r = run(s, k, "model.regenerate", json!({}));
    r["error"]["message"].as_str().unwrap_or_else(|| panic!("no feature failed: {r}")).to_owned()
}

/// Changes fields of a feature's definition and regenerates.
fn update(s: &mut Session, k: &mut OcctKernel, feature: u64, change: Value) {
    let mut kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(feature as u32)).unwrap().kind).unwrap();
    for (key, v) in change.as_object().unwrap() {
        kind[key] = v.clone();
    }
    run(s, k, "feature.update", json!({ "feature": feature, "kind": kind }));
}

/// A sheet metal part: the rule, and a `w` x `h` face on the XY plane with a corner at the
/// origin. Returns the face feature and its sketch's four lines (bottom, right, top, left).
fn plate(s: &mut Session, k: &mut OcctKernel, w: f64, h: f64) -> (u64, Vec<u64>) {
    run(s, k, "sheet.defaults", json!({ "thickness": T, "bend_radius": R }));
    let sk = id(&run(s, k, "sketch.create", json!({ "plane": "xy" })), "feature");
    let r = run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": w, "y2": h }));
    let lines = r["lines"].as_array().unwrap().iter().map(|l| l.as_u64().unwrap()).collect();
    let face = id(&run(s, k, "sheet.face", json!({ "sketch": sk })), "feature");
    (face, lines)
}

/// The edge of the plate's top (`"end"`) or bottom (`"start"`) along sketch line `line`.
fn rim(s: &mut Session, k: &mut OcctKernel, face: u64, side: &str, line: u64) -> Value {
    run(
        s,
        k,
        "model.edge_ref",
        json!({ "faces": [{ "type": "cap", "feature": face, "end": side }, { "type": "side", "feature": face, "curve": line }] }),
    )
}

/// A reference to the edge whose middle is at `mid`.
fn edge_at(s: &mut Session, k: &mut OcctKernel, mid: Vec3) -> Value {
    let edges = run(s, k, "model.edges", json!({}));
    let p = |v: &Value| Vec3::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap());
    let found: Vec<u64> = edges["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| ((p(&e["start"]) + p(&e["end"])) * 0.5).dist(mid) < 1e-6 && e["curve"]["Line"].is_object())
        .map(|e| id(e, "edge"))
        .collect();
    assert_eq!(found.len(), 1, "edges with their middle at {mid:?}: {found:?}");
    run(s, k, "model.edge_ref", json!({ "edge": found[0] }))
}

/// The flat faces of the part: (normal, centroid, area).
fn flats(s: &mut Session, k: &mut OcctKernel) -> Vec<(Vec3, Vec3, f64)> {
    let v = |x: &Value| Vec3::new(num(x, "x"), num(x, "y"), num(x, "z"));
    run(s, k, "model.faces", json!({}))["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["surface"]["Plane"].is_object())
        .map(|f| {
            let c = &f["centroid"];
            (v(&f["surface"]["Plane"]["normal"]), Vec3::new(c[0].as_f64().unwrap(), c[1].as_f64().unwrap(), c[2].as_f64().unwrap()), num(f, "area"))
        })
        .collect()
}

fn flat(s: &mut Session, k: &mut OcctKernel) -> Value {
    run(s, k, "sheet.flat", json!({ "curves": true }))
}

/// The ends of a bend's centre line, lowest first.
fn centre_line(bend: &Value) -> ((f64, f64), (f64, f64)) {
    let l = &bend["lines"][0];
    let p = |i: usize| (l[i][0].as_f64().unwrap(), l[i][1].as_f64().unwrap());
    let (a, b) = (p(0), p(1));
    if (a.1, a.0) <= (b.1, b.0) { (a, b) } else { (b, a) }
}

#[test]
fn a_bracket_is_folded_and_laid_flat_to_its_bend_allowance() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    // A part is not sheet metal until it is given a rule.
    assert!(refused(&mut s, &mut k, "sheet.face", json!({ "sketch": 1 })).contains("not a sheet metal part yet"));
    assert_eq!(run(&mut s, &mut k, "sheet.info", json!({}))["sheet_metal"], false);
    let (face, lines) = plate(&mut s, &mut k, 40.0, 30.0);
    let rule = run(&mut s, &mut k, "sheet.info", json!({}));
    assert_eq!(
        (num(&rule, "thickness"), num(&rule, "bend_radius"), num(&rule, "k_factor"), rule["corner"].as_str()),
        (2.0, 3.0, 0.44, Some("round"))
    );
    assert!(near(volume(&mut s, &mut k), 40.0 * 30.0 * T), "the face is one thickness thick");
    // The flat pattern of a flat plate is the plate.
    let f = flat(&mut s, &mut k);
    assert!(
        near(num(&f, "length"), 40.0) && near(num(&f, "width"), 30.0) && near(num(&f, "area"), 1200.0) && near(num(&f, "cut_length"), 140.0),
        "{f}"
    );
    assert_eq!((f["bends"].as_array().unwrap().len(), f["holes"].as_u64(), f["outer"].as_array().unwrap().len()), (0, Some(0), 4));

    // A flange 20 high on the right-hand edge of the top. Its outer face goes through the edge
    // (the part stays 40 long) and its top is 20 above the underside.
    let edge = rim(&mut s, &mut k, face, "end", lines[1]);
    let fl = id(&run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "height": 20 })), "feature");
    let folded = 35.0 * 30.0 * T + bend_volume(FRAC_PI_2, 30.0) + 15.0 * T * 30.0;
    assert!(near(volume(&mut s, &mut k), folded), "{} {folded}", volume(&mut s, &mut k));
    let faces = flats(&mut s, &mut k);
    let wall = faces.iter().find(|f| f.0.near(Vec3::X, 1e-9)).expect("the wall's outer face");
    assert!(wall.1.near(Vec3::new(40.0, 15.0, 12.5), 1e-9) && near(wall.2, 15.0 * 30.0), "{wall:?}");
    let tip = faces.iter().find(|f| f.0.near(Vec3::Z, 1e-9) && near(f.2, T * 30.0)).expect("the wall's top");
    assert!(tip.1.near(Vec3::new(39.0, 15.0, 20.0), 1e-9), "{tip:?}");

    // Laid flat: 35 of plate, the bend's allowance, 15 of wall. (The same as the two outside
    // lengths less the bend deduction: 40 + 20 - (2 (r + t) - allowance).)
    let ba = allowance(FRAC_PI_2);
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), 50.0 + ba) && near(num(&f, "width"), 30.0), "{f}");
    assert!(near(num(&f, "length"), 40.0 + 20.0 - (2.0 * (R + T) - ba)));
    assert!(near(num(&f, "area"), (50.0 + ba) * 30.0) && near(num(&f, "cut_length"), 2.0 * (80.0 + ba)), "{f}");
    let bends = f["bends"].as_array().unwrap();
    assert_eq!(bends.len(), 1);
    let b = &bends[0];
    assert!(near(num(b, "angle"), FRAC_PI_2) && near(num(b, "radius"), R) && near(num(b, "allowance"), ba) && b["up"] == true, "{b}");
    let (lo, hi) = centre_line(b);
    assert!(near(lo.0, 35.0 + ba / 2.0) && near(lo.1, 0.0) && near(hi.0, 35.0 + ba / 2.0) && near(hi.1, 30.0), "{b}");
    assert_eq!(b["extents"].as_array().unwrap().len(), 2, "where the bend begins and ends");
    assert_eq!(f["outer"].as_array().unwrap().iter().filter(|c| c["type"] == "line").count(), f["outer"].as_array().unwrap().len(), "{f}");

    // The K factor moves the neutral surface: the pattern grows and shrinks with it.
    for kf in [0.5, 0.33, 0.0, 1.0] {
        run(&mut s, &mut k, "sheet.defaults", json!({ "k_factor": kf }));
        assert!(near(num(&flat(&mut s, &mut k), "length"), 50.0 + FRAC_PI_2 * (R + kf * T)), "K {kf}");
    }
    run(&mut s, &mut k, "sheet.defaults", json!({ "k_factor": K }));
    // A thicker sheet: the face, the bend and the wall all follow the one parameter.
    run(&mut s, &mut k, "param.set", json!({ "name": "Thickness", "equation": "3 mm" }));
    let folded3 = 34.0 * 30.0 * 3.0 + FRAC_PI_2 / 2.0 * (36.0 - 9.0) * 30.0 + 14.0 * 3.0 * 30.0;
    assert!(near(volume(&mut s, &mut k), folded3), "{} {folded3}", volume(&mut s, &mut k));
    assert!(near(num(&flat(&mut s, &mut k), "length"), 34.0 + FRAC_PI_2 * (R + K * 3.0) + 14.0));
    assert!(s.undo());
    // The flange's own values are parameters too: its height by an equation.
    let height = run(&mut s, &mut k, "param.list", json!({}))["model"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["target"]["feature"] == fl && p["target"]["field"] == "/height")
        .map(|p| p["name"].as_str().unwrap().to_owned())
        .expect("the height is a parameter");
    run(&mut s, &mut k, "param.set", json!({ "name": height, "equation": "Thickness * 15" }));
    assert!(near(num(&flat(&mut s, &mut k), "length"), 35.0 + ba + 25.0));
    // The sheet metal sizes keep their names as long as the part is sheet metal.
    assert!(refused(&mut s, &mut k, "param.delete", json!({ "name": "KFactor" })).contains("sheet metal sizes"));
    assert!(refused(&mut s, &mut k, "param.rename", json!({ "name": "Thickness", "to": "gauge" })).contains("keeps its name"));
    assert!(refused(&mut s, &mut k, "sheet.defaults", json!({ "thickness": 0 })).contains("Thickness must be from"));
    assert!(refused(&mut s, &mut k, "sheet.defaults", json!({ "k_factor": 2 })).contains("KFactor must be from 0 to 1"));
}

#[test]
fn bends_at_any_angle_are_placed_and_measured_as_asked() {
    // (angle in degrees, position, datum): where the bend starts in from the edge, what the
    // bend takes of the height.
    let tan = |deg: f64| (deg.to_radians() / 2.0).tan();
    let cases: [(f64, &str, &str, f64, f64); 6] = [
        (90.0, "adjacent", "inner", 0.0, R),
        (90.0, "outside", "outer", R, R + T),
        (60.0, "inside", "outer", (R + T) * tan(60.0), (R + T) * tan(60.0)),
        (120.0, "adjacent", "inner", 0.0, R * tan(120.0)),
        (45.0, "outside", "tangent", R * tan(45.0), (R + T) * 45f64.to_radians().sin()),
        (135.0, "tangent", "tangent", R + T, R + T),
    ];
    for (deg, position, datum, setback, taken) in cases {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        let (face, lines) = plate(&mut s, &mut k, 40.0, 30.0);
        let edge = rim(&mut s, &mut k, face, "end", lines[1]);
        let angle = deg.to_radians();
        run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "height": 20, "angle": angle, "position": position, "datum": datum }));
        let straight = 20.0 - taken;
        let folded = (40.0 - setback) * 30.0 * T + bend_volume(angle, 30.0) + straight * T * 30.0;
        assert!(near(volume(&mut s, &mut k), folded), "{deg} {position} {datum}: {} {folded}", volume(&mut s, &mut k));
        let f = flat(&mut s, &mut k);
        assert!(near(num(&f, "length"), 40.0 - setback + allowance(angle) + straight), "{deg} {position} {datum}: {f}");
        let b = &f["bends"][0];
        assert!(near(num(b, "angle"), angle) && near(num(b, "radius"), R) && b["up"] == true, "{b}");
    }
    // A flange lower than its own bend says how much the bend takes; one of 180 degrees and one
    // on a curved edge say what a flange needs.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 40.0, 30.0);
    let edge = rim(&mut s, &mut k, face, "end", lines[1]);
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge.clone()], "height": 4 }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("its bend alone takes 5 mm"), "{e}");
    assert!(s.undo());
    assert!(refused(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge.clone()], "height": 20, "angle": PI })).contains("1 to 179 degrees"));
    assert!(refused(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "height": 20, "width": 10, "offsets": [1, 1] })).contains("not both"));
    // An upright edge of the plate's corner is not on its outline.
    let upright = run(
        &mut s,
        &mut k,
        "model.edge_ref",
        json!({ "faces": [{ "type": "side", "feature": face, "curve": lines[0] }, { "type": "side", "feature": face, "curve": lines[1] }] }),
    );
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [upright], "height": 20 }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("not on the rim of a sheet 2 mm thick"), "{e}");
    // Nor is the rim of a disc an edge for a flange: it is not straight.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": T }));
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 20 }));
    run(&mut s, &mut k, "sheet.face", json!({ "sketch": sk }));
    let round = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [0, 20, 2] }));
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [round], "height": 20 }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("goes on a straight edge"), "{e}");
}

#[test]
fn flanges_bend_up_and_down_and_grow_from_one_another() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 40.0, 30.0);
    // Up on the right; down on the left (the same top edge, bent the other way).
    let right = rim(&mut s, &mut k, face, "end", lines[1]);
    let left = rim(&mut s, &mut k, face, "end", lines[3]);
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [right], "height": 20 }));
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [left], "height": 20, "flip": true }));
    let ba = allowance(FRAC_PI_2);
    let z = 30.0 * 30.0 * T + 2.0 * (bend_volume(FRAC_PI_2, 30.0) + 15.0 * T * 30.0);
    assert!(near(volume(&mut s, &mut k), z), "{} {z}", volume(&mut s, &mut k));
    // The left wall hangs below the plate: its foot is 20 under the plate's top.
    let foot = flats(&mut s, &mut k).into_iter().find(|f| f.0.near(-Vec3::Z, 1e-9) && near(f.2, T * 30.0)).expect("the foot of the left wall");
    assert!(foot.1.near(Vec3::new(1.0, 15.0, -18.0), 1e-9), "{foot:?}");
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), 60.0 + 2.0 * ba) && near(num(&f, "width"), 30.0), "{f}");
    let bends = f["bends"].as_array().unwrap();
    // In order across the sheet: the left bend goes down (away from whoever looks at the top),
    // the right one up.
    assert_eq!((bends.len(), bends[0]["up"].as_bool(), bends[1]["up"].as_bool()), (2, Some(false), Some(true)), "{f}");
    assert!(near(centre_line(&bends[0]).0.0, 15.0 + ba / 2.0) && near(centre_line(&bends[1]).0.0, 15.0 + ba + 30.0 + ba / 2.0), "{f}");

    // A lip on the right wall's top, turned outwards: a flange on a flange. Its edge is the
    // outer one of the wall's top (x = 40, z = 20).
    let lip_edge = edge_at(&mut s, &mut k, Vec3::new(40.0, 15.0, 20.0));
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [lip_edge], "height": 10 }));
    let hat = z - 5.0 * T * 30.0 + bend_volume(FRAC_PI_2, 30.0) + 5.0 * T * 30.0;
    assert!(near(volume(&mut s, &mut k), hat), "{} {hat}", volume(&mut s, &mut k));
    // The lip's far edge is 10 out from the wall's inner face.
    let end = flats(&mut s, &mut k).into_iter().find(|f| f.0.near(Vec3::X, 1e-9) && near(f.2, T * 30.0)).expect("the lip's far edge");
    assert!(end.1.near(Vec3::new(48.0, 15.0, 19.0), 1e-9), "{end:?}");
    let f = flat(&mut s, &mut k);
    // 15 + bend + 30 + bend + 10 (the wall, less what the lip's bend took) + bend + 5.
    assert!(near(num(&f, "length"), 60.0 + 3.0 * ba), "{f}");
    let ups: Vec<bool> = f["bends"].as_array().unwrap().iter().map(|b| b["up"].as_bool().unwrap()).collect();
    assert_eq!(ups, vec![false, true, false], "the lip bends back the other way: {f}");
    // The flange under the lip changed: the lip follows, by the names of its faces.
    run(&mut s, &mut k, "sheet.defaults", json!({ "bend_radius": 2 }));
    let r2 = |angle: f64| angle * (2.0 + K * T);
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), 16.0 + 32.0 + 12.0 + 6.0 + 3.0 * r2(FRAC_PI_2)), "{f}");
}

#[test]
fn a_flange_narrower_than_its_edge_gets_reliefs_beside_it() {
    let ba = allowance(FRAC_PI_2);
    for (shape, slot) in [("straight", 2.0 * 6.0), ("round", 2.0 * 5.0 + PI / 2.0)] {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        let (face, lines) = plate(&mut s, &mut k, 40.0, 30.0);
        run(&mut s, &mut k, "sheet.defaults", json!({ "relief": shape }));
        let edge = rim(&mut s, &mut k, face, "end", lines[1]);
        // 10 wide in the middle of the 30 edge. The sheet is cut back 5 for the bend, and a
        // slot 2 wide (the thickness) runs 1 further in on each side of it.
        run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "height": 20, "width": 10 }));
        let cut = 5.0 * 10.0 + 2.0 * slot;
        let folded = (1200.0 - cut) * T + bend_volume(FRAC_PI_2, 10.0) + 15.0 * T * 10.0;
        assert!(near(volume(&mut s, &mut k), folded), "{shape}: {} {folded}", volume(&mut s, &mut k));
        let f = flat(&mut s, &mut k);
        assert!(near(num(&f, "length"), 50.0 + ba) && near(num(&f, "width"), 30.0), "{shape}: {f}");
        assert!(near(num(&f, "area"), 1200.0 - cut + (ba + 15.0) * 10.0), "{shape}: {f}");
        let (lo, hi) = centre_line(&f["bends"][0]);
        assert!(near(lo.1, 10.0) && near(hi.1, 20.0) && near(lo.0, 35.0 + ba / 2.0), "{shape}: {f}");
        // Round slots show as arcs in the outline; nothing is a polyline either way.
        let kinds = |t: &str| f["outer"].as_array().unwrap().iter().filter(|c| c["type"] == t).count();
        assert_eq!((kinds("arc"), kinds("polyline")), (if shape == "round" { 2 } else { 0 }, 0), "{shape}: {f}");
    }
    // By offsets from the ends of the edge instead; and a flange that would leave a strip 1 wide
    // beside its relief (under the 4 the rule keeps) has the sheet cut back to the end instead.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 40.0, 30.0);
    let edge = rim(&mut s, &mut k, face, "end", lines[1]);
    let fl = id(&run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "height": 20, "offsets": [8, 12] })), "feature");
    let folded = (1200.0 - 5.0 * 10.0 - 2.0 * 12.0) * T + bend_volume(FRAC_PI_2, 10.0) + 15.0 * T * 10.0;
    assert!(near(volume(&mut s, &mut k), folded), "{} {folded}", volume(&mut s, &mut k));
    update(&mut s, &mut k, fl, json!({ "width": { "offset": { "start": 3.0, "end": 12.0 } } }));
    // From 3 to 18: cut back from the start of the edge to 18, one slot after it.
    let folded = (1200.0 - 5.0 * 18.0 - 12.0) * T + bend_volume(FRAC_PI_2, 15.0) + 15.0 * T * 15.0;
    assert!(near(volume(&mut s, &mut k), folded), "{} {folded}", volume(&mut s, &mut k));
    assert!(near(num(&flat(&mut s, &mut k), "area"), 1200.0 - 5.0 * 18.0 - 12.0 + (ba + 15.0) * 15.0));
    // Wider than its edge, or nothing left of it.
    update(&mut s, &mut k, fl, json!({ "width": { "centered": { "width": 31.0 } } }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("wider than its edge, which is 30 mm long"), "{e}");
}

#[test]
fn a_tray_has_its_walls_mitred_and_its_corners_relieved() {
    let ba = allowance(FRAC_PI_2);
    // All four edges of a 60 x 40 plate in one flange, 15 high: the walls of a tray. Each bend
    // is cut 5 into the sheet, so the floor is 50 x 30 and the bends are 50 and 30 long: each
    // starts where its neighbour's bend line crosses its own. The walls (10 of them after the
    // bend) reach on towards the corners until a gap of one thickness is left between them:
    // their inner faces are 2 in from the edges, so they would meet 2 along, and the gap takes
    // root 2 more from each.
    let end = 2.0 + 2f64.sqrt();
    let walls = 200.0 - 8.0 * end;
    // The corner relief is centred where the centre lines of the two bends cross: half a bend's
    // width out from each bend line, in the gap between the two bends.
    let d = ba / 2.0;
    let tray = |corner: &str, more: Value| {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
        let mut rule = json!({ "corner": corner });
        for (key, v) in more.as_object().unwrap() {
            rule[key] = v.clone();
        }
        run(&mut s, &mut k, "sheet.defaults", rule);
        let edges: Vec<Value> = lines.iter().map(|l| rim(&mut s, &mut k, face, "end", *l)).collect();
        let fl = id(&run(&mut s, &mut k, "sheet.flange", json!({ "edges": edges, "height": 15 })), "feature");
        (s, k, fl)
    };
    let kinds = |f: &Value, t: &str| f["outer"].as_array().unwrap().iter().filter(|c| c["type"] == t).count();
    // The folded part is one solid the kernel finds nothing wrong with.
    let sound = |s: &mut Session, k: &mut OcctKernel| {
        let body = &run(s, k, "model.topology", json!({}))["bodies"][0];
        body["valid"] == true && body["solids"] == 1
    };

    // Trimmed to the bends: no relief at all.
    let (mut s, mut k, fl) = tray("trim", json!({}));
    let folded = 1500.0 * T + bend_volume(FRAC_PI_2, 160.0) + 10.0 * T * walls;
    assert!(near(volume(&mut s, &mut k), folded), "trim: {} {folded}", volume(&mut s, &mut k));
    assert!(sound(&mut s, &mut k));
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), 70.0 + 2.0 * ba) && near(num(&f, "width"), 50.0 + 2.0 * ba), "{f}");
    assert!(near(num(&f, "area"), 1500.0 + 160.0 * ba + 10.0 * walls), "{f}");
    let bends = f["bends"].as_array().unwrap();
    assert!(bends.len() == 4 && bends.iter().all(|b| b["up"] == true && near(num(b, "angle"), FRAC_PI_2)), "{f}");
    // A cross whose arms widen where the walls begin: seven sides to an arm.
    assert_eq!((kinds(&f, "line"), f["outer"].as_array().unwrap().len(), f["inner"].as_array().unwrap().len()), (28, 28, 0), "{f}");
    assert!(near(num(&f, "cut_length"), 8.0 * ba + 8.0 * (5.0 - end) + 80.0 + walls), "{f}");
    // A wider gap draws the walls back: half of it, times root 2, from each end.
    run(&mut s, &mut k, "sheet.defaults", json!({ "gap": 4 }));
    assert!(near(volume(&mut s, &mut k), folded - 10.0 * T * 8.0 * 2f64.sqrt()), "{}", volume(&mut s, &mut k));
    // Not mitred, each wall ends where its bend does: the outline is a plain cross.
    update(&mut s, &mut k, fl, json!({ "miter": false }));
    assert!(near(volume(&mut s, &mut k), 1500.0 * T + bend_volume(FRAC_PI_2, 160.0) + 10.0 * T * 160.0));
    let f = flat(&mut s, &mut k);
    assert_eq!((kinds(&f, "line"), f["outer"].as_array().unwrap().len()), (12, 12), "{f}");
    assert!(near(num(&f, "cut_length"), 160.0 + 8.0 * (ba + 10.0)), "{f}");

    // Square, 8 across (four thicknesses): it reaches 4 - d past each bend line, so it takes a
    // square that size off each corner of the floor, as much off each end of each bend, and a
    // notch as deep out of the foot of each wall from its end to the same line.
    for (size, more) in [(8.0, json!({})), (12.0, json!({ "corner_size": "Thickness * 6" }))] {
        let c = size / 2.0 - d;
        let (mut s, mut k, _) = tray("square", more);
        let notch = c * (5.0 + c - end);
        let folded = (1500.0 - 4.0 * c * c) * T + bend_volume(FRAC_PI_2, 160.0 - 8.0 * c) + (10.0 * walls - 8.0 * notch) * T;
        assert!(near(volume(&mut s, &mut k), folded), "square {size}: {} {folded}", volume(&mut s, &mut k));
        assert!(sound(&mut s, &mut k), "square {size}");
        let f = flat(&mut s, &mut k);
        assert!(near(num(&f, "area"), 1500.0 - 4.0 * c * c + ba * (160.0 - 8.0 * c) + 10.0 * walls - 8.0 * notch), "square {size}: {f}");
        // Six sides to a corner (the wall's end, the notch's side, and one line across wall,
        // bend and floor, for each of the two flanges) and the four tops.
        assert_eq!(
            (kinds(&f, "line"), f["outer"].as_array().unwrap().len(), f["inner"].as_array().unwrap().len()),
            (28, 28, 0),
            "square {size}: {f}"
        );
        assert!(near(num(&f, "cut_length"), 8.0 * (10.0 - c) + 8.0 * (5.0 + c - end) + 8.0 * (ba + 2.0 * c) + walls), "square {size}: {f}");
        assert!(near(num(&f, "length"), 70.0 + 2.0 * ba), "{f}");
    }
    // A square too small to reach anything (4 across, inside the gap between the bends)
    // changes nothing.
    let (mut s, mut k, _) = tray("square", json!({ "corner_size": "Thickness * 2" }));
    assert!(near(num(&flat(&mut s, &mut k), "area"), 1500.0 + 160.0 * ba + 10.0 * walls));

    // Round, 8 across. Its centre is d from each bend's end and from each wall's foot, and
    // d root 2 (4.31) from the corner of the floor, which it so does not reach: it takes a
    // segment off the middle of each bend's end, and a bite out of the foot of each wall
    // between the wall's end and where the circle leaves it.
    let (mut s, mut k, _) = tray("round", json!({}));
    let rise = (16.0 - d * d).sqrt();
    let segment = 16.0 * (d / 4.0).acos() - d * rise;
    let under = |y: f64| (y * (16.0 - y * y).sqrt() + 16.0 * (y / 4.0).asin()) / 2.0 - d * y;
    // Along the edge, the centre is 5 - d from the corner.
    let from = end - (5.0 - d);
    let bite = under(rise) - under(from);
    assert!(segment > 3.0 && bite > 0.4, "{segment} {bite}");
    let folded = 1500.0 * T + (160.0 * ba - 8.0 * segment) * bend_volume(1.0, 1.0) / (R + K * T) + (10.0 * walls - 8.0 * bite) * T;
    let got = volume(&mut s, &mut k);
    assert!((got - folded).abs() < 1e-6 * folded, "round: {got} {folded}");
    assert!(sound(&mut s, &mut k));
    let f = flat(&mut s, &mut k);
    let area = 1500.0 + 160.0 * ba - 8.0 * segment + 10.0 * walls - 8.0 * bite;
    assert!((num(&f, "area") - area).abs() < 1e-6 * area, "round: {} {area}", num(&f, "area"));
    // At each corner, for each of the two flanges: the wall's end, an arc out of its foot, what
    // is left of the foot, the bend's end either side of the arc out of it. Everything that is
    // round is an arc of radius 4, in the bends too, where the kernel has only points.
    assert_eq!((kinds(&f, "line"), kinds(&f, "arc"), kinds(&f, "polyline"), f["inner"].as_array().unwrap().len()), (36, 16, 0, 0), "{f}");
    for arc in f["outer"].as_array().unwrap().iter().filter(|c| c["type"] == "arc") {
        assert!((num(arc, "radius") - 4.0).abs() < 1e-5, "{arc}");
    }
    let wall_arc = 4.0 * ((rise / 4.0).asin() - (from / 4.0).asin());
    let bend_arc = 4.0 * 2.0 * (d / 4.0).acos();
    let cut = 8.0 * (10.0 - ((16.0 - from * from).sqrt() - d))
        + 8.0 * (5.0 - (5.0 - d + rise))
        + 8.0 * (ba - 2.0 * rise)
        + 8.0 * (wall_arc + bend_arc)
        + walls;
    assert!((num(&f, "cut_length") - cut).abs() < 1e-5 * cut, "round: {} {cut}", num(&f, "cut_length"));
    // A larger one (12 across) reaches the floor too: the whole corner of it, and the whole
    // end of each bend. Then one arc runs from wall to wall at each corner.
    let (mut s, mut k, _) = tray("round", json!({ "corner_size": 12 }));
    let f = flat(&mut s, &mut k);
    assert_eq!((kinds(&f, "line"), kinds(&f, "arc"), kinds(&f, "polyline")), (12, 4, 0), "{f}");
    let arc = f["outer"].as_array().unwrap().iter().find(|c| c["type"] == "arc").unwrap();
    assert!(near(num(arc, "radius"), 6.0), "{arc}");
    // What it takes, corner by corner: the disc, less what of it lies in the gap between the
    // two flanges (which is no sheet), worked out here from the same circle.
    let disc_in = |x0: f64, x1: f64, y0: f64, y1: f64| -> f64 {
        // The area of the disc of radius 6 about (d, 5 - d) inside a box, by thin strips.
        let n = 200_000;
        (0..n)
            .map(|i| {
                let y = y0 + (y1 - y0) * (f64::from(i) + 0.5) / f64::from(n);
                let half = 36.0 - (y - (5.0 - d)) * (y - (5.0 - d));
                if half <= 0.0 {
                    return 0.0;
                }
                let (lo, hi) = (d - half.sqrt(), d + half.sqrt());
                (hi.min(x1) - lo.max(x0)).max(0.0) * (y1 - y0) / f64::from(n)
            })
            .sum()
    };
    // In one flange's flat: the floor is x < 0, the bend 0 to its width, the wall beyond; the
    // bend and the floor begin 5 along the edge, the wall where it is mitred to.
    let taken = disc_in(-20.0, 0.0, 5.0, 20.0) + 2.0 * disc_in(0.0, ba, 5.0, 20.0) + 2.0 * disc_in(ba, 20.0, end, 20.0);
    let area = 1500.0 + 160.0 * ba + 10.0 * walls - 4.0 * taken;
    assert!((num(&f, "area") - area).abs() < 1e-4, "round 12: {} {area}", num(&f, "area"));
    let folded = (1500.0 - 4.0 * disc_in(-20.0, 0.0, 5.0, 20.0)) * T
        + (160.0 * ba - 8.0 * disc_in(0.0, ba, 5.0, 20.0)) * bend_volume(1.0, 1.0) / (R + K * T)
        + (10.0 * walls - 8.0 * disc_in(ba, 20.0, end, 20.0)) * T;
    let got = volume(&mut s, &mut k);
    assert!((got - folded).abs() < 1e-5 * folded, "round 12: {got} {folded}");
    assert!(sound(&mut s, &mut k));

    // Two walls only, on edges that meet: one corner, mitred and relieved; the other ends run
    // to the ends of their edges.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
    run(&mut s, &mut k, "sheet.defaults", json!({ "corner": "square" }));
    let edges = [rim(&mut s, &mut k, face, "end", lines[0]), rim(&mut s, &mut k, face, "end", lines[1])];
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": edges, "height": 15 }));
    let c = 4.0 - d;
    let notch = c * (5.0 + c - end);
    let folded = (55.0 * 35.0 - c * c) * T + bend_volume(FRAC_PI_2, 90.0 - 2.0 * c) + (10.0 * (100.0 - 2.0 * end) - 2.0 * notch) * T;
    assert!(near(volume(&mut s, &mut k), folded), "{} {folded}", volume(&mut s, &mut k));
    assert!(near(num(&flat(&mut s, &mut k), "area"), 55.0 * 35.0 - c * c + ba * (90.0 - 2.0 * c) + 10.0 * (100.0 - 2.0 * end) - 2.0 * notch));
}
#[test]
fn holes_and_cuts_come_out_where_they_are_in_the_flat() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 40.0, 30.0);
    let edge = rim(&mut s, &mut k, face, "end", lines[1]);
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "height": 20 }));
    let ba = allowance(FRAC_PI_2);
    // A hole through the plate, by the ordinary Extrude: a circle on the top, cut through.
    let top = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": face, "end": "end" } }));
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "face": top })), "feature");
    let frame = s.regen(&mut k).sketch_frames[&tenon_model::FeatureId(sk as u32)];
    let at = frame.to_local(Vec3::new(10.0, 12.0, 2.0));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": at.x, "cy": at.y, "r": 3 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "through_all": true, "reverse": true, "operation": "cut" }));
    // And one through the wall, 7.5 up its flat part, from a sketch on its outer face.
    let wall = run(&mut s, &mut k, "model.faces", json!({}))["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["surface"]["Plane"]["normal"]["x"].as_f64().is_some_and(|x| near(x, 1.0)) && near(num(f, "area"), 450.0))
        .map(|f| id(f, "face"))
        .expect("the wall's outer face");
    let wall = run(&mut s, &mut k, "model.face_ref", json!({ "face": wall }));
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "face": wall })), "feature");
    let frame = s.regen(&mut k).sketch_frames[&tenon_model::FeatureId(sk as u32)];
    let at = frame.to_local(Vec3::new(40.0, 20.0, 12.5));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": at.x, "cy": at.y, "r": 2 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": T, "reverse": true, "operation": "cut" }));
    let folded = 35.0 * 30.0 * T + bend_volume(FRAC_PI_2, 30.0) + 15.0 * T * 30.0 - 9.0 * PI * T - 4.0 * PI * T;
    assert!(near(volume(&mut s, &mut k), folded), "{} {folded}", volume(&mut s, &mut k));
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "area"), (50.0 + ba) * 30.0 - 13.0 * PI) && near(num(&f, "cut_length"), 2.0 * (80.0 + ba) + 10.0 * PI), "{f}");
    let holes: Vec<&Value> = f["inner"].as_array().unwrap().iter().map(|l| &l[0]).collect();
    assert_eq!(holes.len(), 2);
    let centre = |h: &Value| (num(&h["center"], "x"), num(&h["center"], "y"), num(h, "radius"));
    // The plate's hole where it was drawn; the wall's as far along the sheet as it is up the
    // wall: 35 of plate, the bend, 7.5 of wall.
    let (a, b) = (centre(holes[0]), centre(holes[1]));
    assert!(near(a.0, 10.0) && near(a.1, 12.0) && near(a.2, 3.0), "{f}");
    assert!(near(b.0, 35.0 + ba + 7.5) && near(b.1, 20.0) && near(b.2, 2.0), "{f}");

    // A cut that runs across the bend line, made both ways from the top so that it goes through
    // the bend as well as the plate. In the flat its part on the plate is exact and its part on
    // the bend follows the cut as it is.
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "face": top })), "feature");
    let frame = s.regen(&mut k).sketch_frames[&tenon_model::FeatureId(sk as u32)];
    let at = frame.to_local(Vec3::new(35.0, 5.0, 2.0));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": at.x, "cy": at.y, "r": 2.5 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 10, "backward": 10, "operation": "cut" }));
    volume(&mut s, &mut k);
    let f = flat(&mut s, &mut k);
    let inner = f["inner"].as_array().unwrap();
    assert_eq!(inner.len(), 3, "{f}");
    let across = inner.iter().find(|l| l.as_array().unwrap().iter().any(|c| c["type"] == "polyline")).expect("the hole across the bend line");
    assert!(across.as_array().unwrap().iter().any(|c| c["type"] == "arc"), "half of it is on the plate, an arc still: {across}");
    // The cut reaches past the middle of the bend, so the bend's centre line is in two pieces,
    // one each side of it.
    let pieces = f["bends"][0]["lines"].as_array().unwrap();
    assert_eq!(pieces.len(), 2, "{f}");
    let gap = (pieces[0][1][1].as_f64().unwrap(), pieces[1][0][1].as_f64().unwrap());
    assert!(gap.0 > 2.5 && gap.0 < 5.0 && gap.1 > 5.0 && gap.1 < 7.5 && (gap.0 + gap.1 - 10.0).abs() < 0.01, "{gap:?}");
}

#[test]
fn what_cannot_be_laid_flat_is_refused_in_plain_words() {
    // A block that is not a sheet of the rule's thickness.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": T }));
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 30 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 10 }));
    let e = refused(&mut s, &mut k, "sheet.flat", json!({}));
    assert!(e.contains("not a sheet 2 mm thick all over") && e.contains("2400 mm³ of its 12000 mm³"), "{e}");
    // Made the right thickness in the rule it is one (any solid of one thickness is).
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": 10 }));
    assert!(near(num(&run(&mut s, &mut k, "sheet.flat", json!({})), "area"), 1200.0));
    // A part that is not sheet metal at all, and one with nothing in it.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    assert!(refused(&mut s, &mut k, "sheet.flat", json!({})).contains("not a sheet metal part yet"));
    run(&mut s, &mut k, "sheet.defaults", json!({}));
    assert!(refused(&mut s, &mut k, "sheet.flat", json!({})).contains("no solid to lay flat yet"));
    // Two sheets in one part: a flat pattern is of one.
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 10, "y2": 10 }));
    run(&mut s, &mut k, "sheet.face", json!({ "sketch": sk }));
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 20, "y1": 0, "x2": 30, "y2": 10 }));
    run(&mut s, &mut k, "sheet.face", json!({ "sketch": sk, "operation": "new_body" }));
    let e = refused(&mut s, &mut k, "sheet.flat", json!({}));
    assert!(e.contains("The part has 2 solids and a flat pattern is of one"), "{e}");
    assert!(near(num(&run(&mut s, &mut k, "sheet.flat", json!({ "body": 1 })), "area"), 100.0), "one of them, when it is said which");

    // Two flanges on the inner edges of an L-shaped plate, clear of each other when folded:
    // laid flat they would need the same piece of sheet.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": T, "bend_radius": R }));
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    let pts = [(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (20.0, 20.0), (20.0, 40.0), (0.0, 40.0)];
    let mut first = None;
    let mut last: Option<u64> = None;
    for i in 0..pts.len() {
        let mut p = json!({ "sketch": sk });
        match last {
            Some(end) => p["start"] = json!(end),
            None => (p["x1"], p["y1"]) = (json!(pts[0].0), json!(pts[0].1)),
        }
        if i + 1 == pts.len() {
            p["end"] = json!(first.unwrap());
        } else {
            (p["x2"], p["y2"]) = (json!(pts[i + 1].0), json!(pts[i + 1].1));
        }
        let l = run(&mut s, &mut k, "sketch.line", p);
        first.get_or_insert(id(&l, "start"));
        last = Some(id(&l, "end"));
    }
    run(&mut s, &mut k, "sheet.face", json!({ "sketch": sk }));
    assert!(near(volume(&mut s, &mut k), 1200.0 * T));
    for mid in [Vec3::new(30.0, 20.0, 2.0), Vec3::new(20.0, 30.0, 2.0)] {
        let edge = edge_at(&mut s, &mut k, mid);
        // The half of each edge away from the inside corner. (An edge runs clockwise round the
        // sheet, seen from the side the flange bends to.)
        let offsets = if mid.x > 25.0 { json!([10, 0]) } else { json!([0, 10]) };
        run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "height": 15, "position": "adjacent", "offsets": offsets }));
    }
    let folded = 1200.0 * T + 2.0 * (bend_volume(FRAC_PI_2, 10.0) + 10.0 * T * 10.0);
    assert!(near(volume(&mut s, &mut k), folded), "{} {folded}", volume(&mut s, &mut k));
    let e = refused(&mut s, &mut k, "sheet.flat", json!({}));
    assert!(e.contains("overlaps itself") && e.contains("same piece of sheet"), "{e}");
    // The same two edges in one flange, meeting in the inside corner, are refused when made.
    assert!(s.undo() && s.undo());
    let edges = [edge_at(&mut s, &mut k, Vec3::new(30.0, 20.0, 2.0)), edge_at(&mut s, &mut k, Vec3::new(20.0, 30.0, 2.0))];
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": edges, "height": 15 }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("inside corner of the sheet"), "{e}");
}

#[test]
fn the_side_that_is_up_is_chosen_and_kept() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 40.0, 30.0);
    let edge = rim(&mut s, &mut k, face, "end", lines[1]);
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "height": 20 }));
    // From the top the bend comes up; with the underside chosen as the A side it goes down, and
    // the pattern is the mirror image: the wall is on the left.
    let ba = allowance(FRAC_PI_2);
    let f = flat(&mut s, &mut k);
    assert!(f["bends"][0]["up"] == true && near(centre_line(&f["bends"][0]).0.0, 35.0 + ba / 2.0));
    let under = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": face, "end": "start" } }));
    let rule = run(&mut s, &mut k, "sheet.a_side", json!({ "face": under }));
    assert!(rule["a_side"].is_object());
    let f = flat(&mut s, &mut k);
    assert!(f["bends"][0]["up"] == false && near(num(&f, "length"), 50.0 + ba), "{f}");
    // (Seen from below, the part's X still runs across the page, so the wall stays on the right
    // and the page's Y is the part's -Y.)
    assert!(near(centre_line(&f["bends"][0]).0.0, 35.0 + ba / 2.0), "{f}");
    // It is the part's own setting: undone with one step, and gone when asked.
    assert!(s.undo());
    assert!(flat(&mut s, &mut k)["bends"][0]["up"] == true);
    assert!(s.redo());
    run(&mut s, &mut k, "sheet.a_side", json!({}));
    assert!(flat(&mut s, &mut k)["bends"][0]["up"] == true);
    // The part has a flat pattern when it is asked for one (the browser shows it).
    assert_eq!(run(&mut s, &mut k, "sheet.flat_pattern", json!({}))["flat_pattern"], true);
    assert_eq!(run(&mut s, &mut k, "sheet.flat_pattern", json!({ "on": false }))["flat_pattern"], false);
    // A face that is no longer there says so.
    let wall = run(&mut s, &mut k, "model.faces", json!({}))["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["surface"]["Plane"]["normal"]["x"].as_f64().is_some_and(|x| near(x, 1.0)) && near(num(f, "area"), 450.0))
        .map(|f| id(f, "face"))
        .unwrap();
    let wall = run(&mut s, &mut k, "model.face_ref", json!({ "face": wall }));
    run(&mut s, &mut k, "sheet.a_side", json!({ "face": wall }));
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), 30.0) && near(num(&f, "width"), 50.0 + ba), "laid out from the wall, upright on the page: {f}");
    let flange = run(&mut s, &mut k, "model.tree", json!({}))["features"].as_array().unwrap().last().map(|f| id(f, "id")).unwrap();
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": flange }));
    let e = refused(&mut s, &mut k, "sheet.flat", json!({}));
    assert!(e.contains("the A side") && e.contains("Define A-Side"), "{e}");
}

#[test]
fn a_change_of_the_rule_rebuilds_the_part_from_its_first_sheet_metal_feature() {
    use tenon_model::{RegenCache, regenerate_with};
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 40.0, 30.0);
    let right = rim(&mut s, &mut k, face, "end", lines[1]);
    let left = rim(&mut s, &mut k, face, "end", lines[3]);
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [right], "height": 20 }));
    let lip = id(&run(&mut s, &mut k, "sheet.flange", json!({ "edges": [left], "height": 10 })), "feature");
    // The app keeps the part as it stood before the feature last changed, and rebuilds from
    // there. The rule is no part of any feature, yet a face and a flange are built to it: a
    // change of the rule must not find them in what was kept.
    let mut cache = RegenCache::default();
    let built = |s: &Session, k: &mut OcctKernel, cache: &mut RegenCache| -> (f64, usize) {
        let mut r = regenerate_with(s.document(), k, Some(cache));
        assert!(r.first_error().is_none(), "{:?}", r.first_error());
        let v = r.bodies.iter().map(|b| k.mass_properties(b.shape, 1.0).unwrap().volume).sum();
        r.release(k);
        (v, cache.resumed_at)
    };
    // A plate between two walls, the left one `h` high, in sheet `t` thick.
    let folded = |t: f64, h: f64| {
        let bend = FRAC_PI_2 / 2.0 * ((R + t) * (R + t) - R * R) * 30.0;
        (40.0 - 2.0 * (R + t)) * 30.0 * t + 2.0 * bend + (20.0 - R - t) * t * 30.0 + (h - R - t) * t * 30.0
    };
    assert!(near(built(&s, &mut k, &mut cache).0, folded(2.0, 10.0)));
    // The second wall's height is changed twice: from the second change on, the three features
    // before it are taken as they were kept.
    update(&mut s, &mut k, lip, json!({ "height": 12.0 }));
    built(&s, &mut k, &mut cache);
    update(&mut s, &mut k, lip, json!({ "height": 14.0 }));
    let (v, kept) = built(&s, &mut k, &mut cache);
    assert!(near(v, folded(2.0, 14.0)) && kept == 3, "{v} kept {kept}");
    // The thickness: only the sketch is as it was. Face and flanges are built again, 3 thick.
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": 3 }));
    let (v, kept) = built(&s, &mut k, &mut cache);
    assert!(near(v, folded(3.0, 14.0)) && kept == 0, "{v} {} kept {kept}", folded(3.0, 14.0));
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": 2.5 }));
    let (v, kept) = built(&s, &mut k, &mut cache);
    assert!(near(v, folded(2.5, 14.0)) && kept == 1, "{v} {} kept {kept}", folded(2.5, 14.0));
    // Which side the flat pattern is laid out from is not the solid's business: nothing of it
    // is built again that was kept.
    let under = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": face, "end": "start" } }));
    run(&mut s, &mut k, "sheet.a_side", json!({ "face": under }));
    let (v, kept) = built(&s, &mut k, &mut cache);
    assert!(near(v, folded(2.5, 14.0)) && kept == 1, "{v} kept {kept}");
    cache.release(&mut k);
}

#[test]
fn an_edge_or_a_face_is_named_by_a_point_on_it() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 40.0, 30.0);
    let edge = rim(&mut s, &mut k, face, "end", lines[1]);
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "height": 20 }));
    // The outer edge of the wall's top, where a lip would go: its faces have names of the
    // flange's own, which a script cannot spell, so it points.
    let top = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [40, 15, 20] }));
    assert!(top["faces"][0]["type"] == "from" && near(num(&top["fingerprint"], "length"), 30.0), "{top}");
    assert_eq!(top, edge_at(&mut s, &mut k, Vec3::new(40.0, 15.0, 20.0)));
    let wall = run(&mut s, &mut k, "model.face_ref", json!({ "at": [40, 15, 12] }));
    assert!(wall["origin"]["type"] == "from" && near(num(&wall["fingerprint"], "area"), 450.0), "{wall}");
    // The plate's own top still goes by its name, and by a point as well.
    let by_point = run(&mut s, &mut k, "model.face_ref", json!({ "at": [10, 10, 2] }));
    let by_name = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": face, "end": "end" } }));
    assert_eq!(by_point, by_name);
    // A point on nothing says how far off the nearest is; one where several meet asks for another.
    let e = refused(&mut s, &mut k, "model.edge_ref", json!({ "at": [40, 15, 21] }));
    assert!(e.contains("no edge passes through (40, 15, 21): the nearest is 1 mm away"), "{e}");
    let e = refused(&mut s, &mut k, "model.edge_ref", json!({ "at": [40, 0, 20] }));
    assert!(e.contains("edges meet at (40, 0, 20)") && e.contains("away from its ends"), "{e}");
    let e = refused(&mut s, &mut k, "model.face_ref", json!({ "at": [40, 15, 20] }));
    assert!(e.contains("faces meet at") && e.contains("inside the face"), "{e}");
    assert!(refused(&mut s, &mut k, "model.face_ref", json!({ "at": [1, 2] })).contains("`at` is a point [x, y, z]"));
}

/// The corners of the box that holds the part: (min, max).
fn bbox(s: &mut Session, k: &mut OcctKernel) -> (Vec3, Vec3) {
    let b = &run(s, k, "model.topology", json!({}))["bodies"][0]["bbox"];
    let p = |v: &Value| Vec3::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap());
    (p(&b["min"]), p(&b["max"]))
}

/// The cross-section of a bend through `angle` with inside radius `r`.
fn ring(angle: f64, r: f64) -> f64 {
    angle / 2.0 * ((r + T) * (r + T) - r * r)
}

#[test]
fn hems_fold_an_edge_back_once_twice_or_into_a_roll() {
    // A 40 x 30 plate, hemmed along its right edge (30 long). Nothing is cut from the sheet:
    // the fold starts at the edge, and the part grows by it.
    let hemmed = |hem: Value| {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        let (face, lines) = plate(&mut s, &mut k, 40.0, 30.0);
        let mut p = json!({ "edges": [rim(&mut s, &mut k, face, "end", lines[1])] });
        for (key, v) in hem.as_object().unwrap() {
            p[key] = v.clone();
        }
        let id = id(&run(&mut s, &mut k, "sheet.hem", p), "feature");
        (s, k, id)
    };
    let plate_volume = 1200.0 * T;

    // Single: a gap of 1 between the faces is a fold of inside radius 0.5; 10 long overall, of
    // which the fold takes 0.5 + 2, leaving 7.5 flat.
    let (mut s, mut k, hem) = hemmed(json!({ "gap": 1, "length": 10 }));
    let section = ring(PI, 0.5) + 7.5 * T;
    assert!(near(volume(&mut s, &mut k), plate_volume + section * 30.0), "{}", volume(&mut s, &mut k));
    // It reaches 2.5 past the edge, and lies from 1 to 3 above the plate's top (at z = 2).
    let (lo, hi) = bbox(&mut s, &mut k);
    assert!(lo.near(Vec3::ZERO, 1e-6) && hi.near(Vec3::new(42.5, 30.0, 5.0), 1e-6), "{lo:?} {hi:?}");
    let under = flats(&mut s, &mut k)
        .into_iter()
        .find(|f| f.0.near(-Vec3::Z, 1e-9) && (f.1.z - 3.0).abs() < 1e-9)
        .expect("the hem's face that looks down at the plate");
    assert!(near(under.2, 7.5 * 30.0) && (under.1.x - 36.25).abs() < 1e-9, "{under:?}");
    // In the flat the fold is half a turn of the neutral surface: pi (0.5 + 0.44 x 2).
    let fold = PI * (0.5 + K * T);
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), 40.0 + fold + 7.5) && near(num(&f, "width"), 30.0) && near(num(&f, "area"), 1200.0 + (fold + 7.5) * 30.0), "{f}");
    let bends = f["bends"].as_array().unwrap();
    assert!(bends.len() == 1 && bends[0]["up"] == true && near(num(&bends[0], "angle"), PI) && near(num(&bends[0], "radius"), 0.5), "{f}");
    // Folded to the other side: under the plate, and a bend down.
    update(&mut s, &mut k, hem, json!({ "flip": true }));
    assert!(near(volume(&mut s, &mut k), plate_volume + section * 30.0));
    let (lo, hi) = bbox(&mut s, &mut k);
    assert!(lo.near(Vec3::new(0.0, 0.0, -3.0), 1e-6) && hi.near(Vec3::new(42.5, 30.0, 2.0), 1e-6), "{lo:?} {hi:?}");
    assert!(flat(&mut s, &mut k)["bends"][0]["up"] == false);
    // 10 wide in the middle of the edge: no reliefs, since nothing of the sheet is bent.
    update(&mut s, &mut k, hem, json!({ "flip": false, "width": { "centered": { "width": 10.0 } } }));
    assert!(near(volume(&mut s, &mut k), plate_volume + section * 10.0));
    assert!(near(num(&flat(&mut s, &mut k), "area"), 1200.0 + (fold + 7.5) * 10.0));
    // By what it gets from the rule when nothing is said: half a thickness of gap, four of
    // length (so 8 - 0.5 - 2 flat).
    let (mut s, mut k, _) = hemmed(json!({}));
    assert!(near(volume(&mut s, &mut k), plate_volume + (ring(PI, 0.5) + 5.5 * T) * 30.0));

    // Double: the outer fold goes round the inner layer and a gap either side of it (inside
    // radius 1 + 2 / 2 = 2), the inner fold is as a single hem's (0.5). 12 long overall leaves
    // 12 - (2 + 2) - (0.5 + 2) = 5.5 flat in each layer.
    let (mut s, mut k, _) = hemmed(json!({ "type": "double", "gap": 1, "length": 12 }));
    let section = ring(PI, 2.0) + ring(PI, 0.5) + 2.0 * 5.5 * T;
    assert!(near(volume(&mut s, &mut k), plate_volume + section * 30.0), "{}", volume(&mut s, &mut k));
    // Three layers and two gaps above the plate's top: 2 + 1 + 2 + 1 + 2.
    let (_, hi) = bbox(&mut s, &mut k);
    assert!(hi.near(Vec3::new(44.0, 30.0, 8.0), 1e-6), "{hi:?}");
    let f = flat(&mut s, &mut k);
    let folds = PI * (2.0 + K * T) + PI * (0.5 + K * T);
    assert!(near(num(&f, "length"), 40.0 + folds + 11.0), "{f}");
    let bends = f["bends"].as_array().unwrap();
    assert!(bends.len() == 2 && bends.iter().all(|b| b["up"] == true && near(num(b, "angle"), PI)), "{f}");

    // Rolled: three quarters of a turn at the part's bend radius (3), and nothing after it.
    let (mut s, mut k, _) = hemmed(json!({ "type": "rolled" }));
    assert!(near(volume(&mut s, &mut k), plate_volume + ring(1.5 * PI, 3.0) * 30.0), "{}", volume(&mut s, &mut k));
    assert!(near(num(&flat(&mut s, &mut k), "length"), 40.0 + 1.5 * PI * (3.0 + K * T)));
    // Teardrop: rolled 190 degrees at radius 1, then straight back down until it is a hundredth
    // of a millimetre off the plate.
    let (mut s, mut k, _) = hemmed(json!({ "type": "teardrop", "radius": 1 }));
    let turn = 190f64.to_radians();
    let down = (1.0 - turn.cos() - 0.01) / -turn.sin();
    assert!((down - 11.37).abs() < 0.01, "{down}");
    assert!(near(volume(&mut s, &mut k), plate_volume + (ring(turn, 1.0) + down * T) * 30.0), "{}", volume(&mut s, &mut k));
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), 40.0 + turn * (1.0 + K * T) + down), "{f}");

    // What cannot be: a hem shorter than its own fold, a teardrop that does not come back, a
    // roll that comes round into the sheet, and an edge that is not the sheet's.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 40.0, 30.0);
    let edge = rim(&mut s, &mut k, face, "end", lines[1]);
    let e = refused(&mut s, &mut k, "sheet.hem", json!({ "edges": [edge], "gap": 1, "length": 2 }));
    assert!(e.contains("2 mm long and its fold alone takes 2.5 mm"), "{e}");
    let e = refused(&mut s, &mut k, "sheet.hem", json!({ "edges": [edge], "type": "teardrop", "angle": 100f64.to_radians() }));
    assert!(e.contains("181 to 270 degrees"), "{e}");
    let e = refused(&mut s, &mut k, "sheet.hem", json!({ "edges": [edge], "type": "pressed" }));
    assert!(e.contains("unknown hem type `pressed`"), "{e}");
    let e = refused(&mut s, &mut k, "sheet.hem", json!({ "edges": [], "gap": 1, "length": 10 }));
    assert!(e.contains("`edges` needs 1 to"), "{e}");
    run(&mut s, &mut k, "sheet.hem", json!({ "edges": [edge], "type": "rolled", "radius": 1, "angle": 340f64.to_radians() }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("The hem runs into the part") && e.contains("larger gap or radius"), "{e}");
}

#[test]
fn a_contour_flange_follows_an_open_profile_with_a_bend_at_every_corner() {
    let ba = allowance(FRAC_PI_2);
    // The profile is drawn on the XZ plane and pushed 50 out of it.
    let start = |points: &[(f64, f64)]| {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": T, "bend_radius": R }));
        let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" })), "feature");
        let mut last: Option<Value> = None;
        for w in points.windows(2) {
            let mut p = json!({ "sketch": sk, "x1": w[0].0, "y1": w[0].1, "x2": w[1].0, "y2": w[1].1 });
            if let Some(end) = &last {
                p["start"] = end.clone();
            }
            last = Some(run(&mut s, &mut k, "sketch.line", p)["end"].clone());
        }
        (s, k, sk)
    };
    // The sizes of the box that holds the part, smallest first.
    let sizes = |s: &mut Session, k: &mut OcctKernel| {
        let (lo, hi) = bbox(s, k);
        let mut d = [hi.x - lo.x, hi.y - lo.y, hi.z - lo.z];
        d.sort_by(f64::total_cmp);
        d
    };
    let close = |a: [f64; 3], b: [f64; 3]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6);

    // An L, 30 by 20. The sheet goes on the inside of its corner, so the profile is the part's
    // outside: the bend's outside radius is 5 and it takes 5 of each line.
    let l = [(0.0, 0.0), (30.0, 0.0), (30.0, 20.0)];
    let (mut s, mut k, sk) = start(&l);
    let cf = id(&run(&mut s, &mut k, "sheet.contour_flange", json!({ "sketch": sk, "distance": 50 })), "feature");
    let section = T * (25.0 + 15.0) + ring(FRAC_PI_2, R);
    assert!(near(volume(&mut s, &mut k), section * 50.0), "{}", volume(&mut s, &mut k));
    assert!(close(sizes(&mut s, &mut k), [20.0, 30.0, 50.0]), "{:?}", sizes(&mut s, &mut k));
    let f = flat(&mut s, &mut k);
    // Laid flat it is 25 + the bend + 15 one way, and the 50 it was pushed the other.
    let (across, along) = (num(&f, "length").min(num(&f, "width")), num(&f, "length").max(num(&f, "width")));
    assert!(near(across, 25.0 + ba + 15.0) && near(along, 50.0) && f["bends"].as_array().unwrap().len() == 1, "{f}");
    assert!(near(num(&f["bends"][0], "radius"), R) && near(num(&f, "area"), (40.0 + ba) * 50.0), "{f}");
    // On the outside of the profile instead, the part is a thickness bigger each way and the
    // profile is the inside of the bend (radius 3, taking 3 of each line).
    update(&mut s, &mut k, cf, json!({ "position": "flipped" }));
    assert!(near(volume(&mut s, &mut k), (T * (27.0 + 17.0) + ring(FRAC_PI_2, R)) * 50.0));
    assert!(close(sizes(&mut s, &mut k), [22.0, 32.0, 50.0]), "{:?}", sizes(&mut s, &mut k));
    // Half each side of it: the profile is the middle of the sheet.
    update(&mut s, &mut k, cf, json!({ "position": "symmetric" }));
    assert!(near(volume(&mut s, &mut k), (T * (26.0 + 16.0) + ring(FRAC_PI_2, R)) * 50.0));
    assert!(close(sizes(&mut s, &mut k), [21.0, 31.0, 50.0]), "{:?}", sizes(&mut s, &mut k));
    // Pushed half each way from the sketch plane, or behind it; a bend radius of its own.
    let frame = s.regen(&mut k).sketch_frames[&tenon_model::FeatureId(sk as u32)];
    let along = |s: &mut Session, k: &mut OcctKernel| {
        let (lo, hi) = bbox(s, k);
        let (a, b) = ((lo - frame.origin()).dot(frame.z()), (hi - frame.origin()).dot(frame.z()));
        (a.min(b), a.max(b))
    };
    update(&mut s, &mut k, cf, json!({ "position": "default", "direction": "symmetric" }));
    let (from, to) = along(&mut s, &mut k);
    assert!((from + 25.0).abs() < 1e-6 && (to - 25.0).abs() < 1e-6, "{from} {to}");
    update(&mut s, &mut k, cf, json!({ "direction": "flipped", "radius": 1.0 }));
    let (from, to) = along(&mut s, &mut k);
    assert!((from + 50.0).abs() < 1e-6 && to.abs() < 1e-6, "{from} {to}");
    assert!(near(volume(&mut s, &mut k), (T * (27.0 + 17.0) + ring(FRAC_PI_2, 1.0)) * 50.0));

    // A channel: three lines, two bends, 30 wide and 20 high outside.
    let (mut s, mut k, sk) = start(&[(0.0, 20.0), (0.0, 0.0), (30.0, 0.0), (30.0, 20.0)]);
    run(&mut s, &mut k, "sheet.contour_flange", json!({ "sketch": sk, "distance": 50 }));
    assert!(near(volume(&mut s, &mut k), (T * (15.0 + 20.0 + 15.0) + 2.0 * ring(FRAC_PI_2, R)) * 50.0), "{}", volume(&mut s, &mut k));
    assert!(close(sizes(&mut s, &mut k), [20.0, 30.0, 50.0]), "{:?}", sizes(&mut s, &mut k));
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length").max(num(&f, "width")), 50.0 + 2.0 * ba) && f["bends"].as_array().unwrap().len() == 2, "{f}");
    // It is sheet like any other: a hem along the top of one side, folded inwards. The side's
    // top is 2 wide (x from 28 to 30 at y = 20 of the sketch); the hem goes on its inner edge.
    let frame = s.regen(&mut k).sketch_frames[&tenon_model::FeatureId(sk as u32)];
    let at = frame.plane_point(tenon_geom::Vec2::new(28.0, 20.0)) + frame.z() * 25.0;
    let edge = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [at.x, at.y, at.z] }));
    let before = volume(&mut s, &mut k);
    run(&mut s, &mut k, "sheet.hem", json!({ "edges": [edge], "gap": 1, "length": 8 }));
    assert!(near(volume(&mut s, &mut k), before + (ring(PI, 0.5) + 5.5 * T) * 50.0), "{}", volume(&mut s, &mut k));
    let f = flat(&mut s, &mut k);
    assert!(
        f["bends"].as_array().unwrap().len() == 3 && near(num(&f, "length").max(num(&f, "width")), 50.0 + 2.0 * ba + PI * (0.5 + K * T) + 5.5),
        "{f}"
    );

    // A profile with an arc drawn in it: the arc is the bend, as drawn (radius 6 along the
    // profile, so 4 inside with the sheet inside it).
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": T, "bend_radius": R }));
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" })), "feature");
    let a = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 24, "y2": 0 }));
    let arc = run(&mut s, &mut k, "sketch.arc", json!({ "sketch": sk, "cx": 24, "cy": 6, "start": a["end"], "x2": 30, "y2": 6 }));
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": 30, "y1": 6, "x2": 30, "y2": 20, "start": arc["end"] }));
    run(&mut s, &mut k, "sheet.contour_flange", json!({ "sketch": sk, "distance": 50 }));
    assert!(near(volume(&mut s, &mut k), (T * (24.0 + 14.0) + ring(FRAC_PI_2, 4.0)) * 50.0), "{}", volume(&mut s, &mut k));
    assert!(near(num(&flat(&mut s, &mut k)["bends"][0], "radius"), 4.0));

    // What is no profile for one.
    let refuse = |points: &[(f64, f64)], more: &dyn Fn(&mut Session, &mut OcctKernel, u64), radius: f64| {
        let (mut s, mut k, sk) = start(points);
        more(&mut s, &mut k, sk);
        run(&mut s, &mut k, "sheet.contour_flange", json!({ "sketch": sk, "distance": 50, "radius": radius }));
        failure(&mut s, &mut k)
    };
    let nothing = |_: &mut Session, _: &mut OcctKernel, _: u64| {};
    let e = refuse(&[(0.0, 0.0), (30.0, 0.0), (30.0, 4.0)], &nothing, R);
    assert!(e.contains("4 mm long and the bends at its ends need 5 mm"), "{e}");
    let e = refuse(&l, &|s, k, sk| drop(run(s, k, "sketch.circle", json!({ "sketch": sk, "cx": 10, "cy": 10, "r": 2 }))), R);
    assert!(e.contains("has a circle in it"), "{e}");
    let e = refuse(&l, &|s, k, sk| drop(run(s, k, "sketch.line", json!({ "sketch": sk, "x1": 50, "y1": 0, "x2": 60, "y2": 0 }))), R);
    assert!(e.contains("more than one piece"), "{e}");
    let e = refuse(&[(0.0, 0.0), (30.0, 0.0), (30.0, 20.0), (0.0, 20.0), (0.0, 0.0)], &nothing, R);
    assert!(e.contains("closed") || e.contains("branches"), "{e}");
    // A hook that comes back through its own first line.
    let e = refuse(&[(0.0, 0.0), (40.0, 0.0), (40.0, 10.0), (20.0, 10.0), (20.0, -10.0)], &nothing, 1.0);
    assert!(e.contains("folds into itself"), "{e}");
    // Not a sheet metal part: it says what to do first.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" })), "feature");
    assert!(refused(&mut s, &mut k, "sheet.contour_flange", json!({ "sketch": sk, "distance": 5 })).contains("run sheet.defaults first"));
}

#[test]
fn a_six_sided_tray_is_mitred_and_relieved_at_its_blunt_corners() {
    // A regular hexagon, 40 to a side, with a wall 15 high on every edge. Its corners are 120
    // degrees: the bend lines (5 in from each edge) cross 5 / tan 60 along each edge, and the
    // walls, whose inner faces are 2 in, meet 2 / tan 60 along and give up 1 / sin 60 more each
    // for a gap of 2.
    let corner = 2.0 * PI / 3.0;
    let ba = allowance(FRAC_PI_2);
    let (back, end) = (5.0 / (corner / 2.0).tan(), 2.0 / (corner / 2.0).tan() + 1.0 / (corner / 2.0).sin());
    let tray = |relief: &str| {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": T, "bend_radius": R, "corner": relief }));
        let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy", "project_origin": false })), "feature");
        let at = |i: usize| (40.0 * (PI / 3.0 * i as f64).cos(), 40.0 * (PI / 3.0 * i as f64).sin());
        let mut lines: Vec<Value> = Vec::new();
        let mut points: Vec<Value> = Vec::new();
        for i in 0..6 {
            let (a, b) = (at(i), at(i + 1));
            let mut p = json!({ "sketch": sk, "x1": a.0, "y1": a.1, "x2": b.0, "y2": b.1 });
            if let Some(last) = points.last() {
                p["start"] = last.clone();
            }
            if i == 5 {
                p["end"] = points[0].clone();
            }
            let line = run(&mut s, &mut k, "sketch.line", p);
            if i == 0 {
                points.push(line["start"].clone());
            }
            points.push(line["end"].clone());
            lines.push(line["line"].clone());
        }
        let face = id(&run(&mut s, &mut k, "sheet.face", json!({ "sketch": sk })), "feature");
        let edges: Vec<Value> = lines.iter().map(|l| rim(&mut s, &mut k, face, "end", l.as_u64().unwrap())).collect();
        run(&mut s, &mut k, "sheet.flange", json!({ "edges": edges, "height": 15 }));
        (s, k)
    };
    // The floor is the hexagon 5 smaller all round: its sides are as long as the bends.
    let apothem = 40.0 * (PI / 6.0).cos();
    let side = 40.0 - 2.0 * back;
    let floor = 6.0 * side * (apothem - 5.0) / 2.0;
    let walls = 6.0 * (40.0 - 2.0 * end);

    let (mut s, mut k) = tray("trim");
    let folded = floor * T + bend_volume(FRAC_PI_2, 6.0 * side) + 10.0 * T * walls;
    assert!(near(volume(&mut s, &mut k), folded), "{} {folded}", volume(&mut s, &mut k));
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    let f = flat(&mut s, &mut k);
    let flat_trim = floor + ba * 6.0 * side + 10.0 * walls;
    assert!(near(num(&f, "area"), flat_trim), "{f}");
    assert!(f["bends"].as_array().unwrap().len() == 6 && f["inner"].as_array().unwrap().is_empty(), "{f}");

    // A round relief 8 across at each corner. It is laid out in the flat, so what it takes is
    // what of the disc lies on the sheet there: found here by counting the points of a fine grid
    // that are in the disc and on the floor, a bend or a wall of the two flanges that meet.
    let (mut s, mut k) = tray("round");
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    let f = flat(&mut s, &mut k);
    let d = ba / 2.0;
    let centre = (d, back - d / (corner / 2.0).tan());
    // In one flange's flat: x out from where its bend starts, y along its edge from the corner.
    let on_flange = |x: f64, y: f64| (0.0..=ba).contains(&x) && y >= back || x >= ba && x <= ba + 10.0 && y >= end;
    // The same point as the other flange has it: the two are mirrored in the line that halves
    // the corner.
    let other = |x: f64, y: f64| (5.0 - y * corner.sin() + (5.0 - x) * corner.cos(), y * corner.cos() + (5.0 - x) * corner.sin());
    let n = 3000;
    let cell = 8.0 / f64::from(n);
    let mut count = 0u64;
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (centre.0 - 4.0 + (f64::from(i) + 0.5) * cell, centre.1 - 4.0 + (f64::from(j) + 0.5) * cell);
            if (x - centre.0).powi(2) + (y - centre.1).powi(2) > 16.0 {
                continue;
            }
            let (ox, oy) = other(x, y);
            if on_flange(x, y) || on_flange(ox, oy) || (x <= 0.0 && ox <= 0.0) {
                count += 1;
            }
        }
    }
    let taken = count as f64 * cell * cell;
    assert!(taken > 5.0 && taken < 30.0, "{taken}");
    assert!((num(&f, "area") - (flat_trim - 6.0 * taken)).abs() < 0.2, "{} {}", num(&f, "area"), flat_trim - 6.0 * taken);
    // Every relief is arcs of radius 4, and nothing in the outline is left as points.
    let outer = f["outer"].as_array().unwrap();
    assert!(outer.iter().all(|c| c["type"] != "polyline") && outer.iter().any(|c| c["type"] == "arc"), "{f}");
    for arc in outer.iter().filter(|c| c["type"] == "arc") {
        assert!((num(arc, "radius") - 4.0).abs() < 1e-5, "{arc}");
    }
    // Walls that lean in 30 degrees (the sheet turns 120) are cut on the slant, and still
    // clear one another: the part is one sound solid.
    let (mut s, mut k) = tray("trim");
    let flange = s.document().features().last().unwrap().id.0;
    update(&mut s, &mut k, u64::from(flange), json!({ "angle": 2.0 * PI / 3.0 }));
    assert!(volume(&mut s, &mut k) > 0.0);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    assert_eq!(flat(&mut s, &mut k)["bends"].as_array().unwrap().len(), 6);
}

/// A 60 x 40 plate with a wall 15 high on its front edge and on its right edge: as one flange
/// (which closes its own corner as `options` say) or, `apart`, as two flanges made one after the
/// other. Returns the plate's face feature as well.
fn two_walls(apart: bool, rule: Value, options: Value) -> (Session, OcctKernel, u64) {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
    run(&mut s, &mut k, "sheet.defaults", rule);
    let (front, right) = (rim(&mut s, &mut k, face, "end", lines[0]), rim(&mut s, &mut k, face, "end", lines[1]));
    if apart {
        run(&mut s, &mut k, "sheet.flange", json!({ "edges": [front], "height": 15 }));
        let mut p = json!({ "edges": [right], "height": 15 });
        for (key, v) in options.as_object().unwrap() {
            p[key] = v.clone();
        }
        run(&mut s, &mut k, "sheet.flange", p);
    } else {
        let mut p = json!({ "edges": [front, right], "height": 15 });
        for (key, v) in options.as_object().unwrap() {
            p[key] = v.clone();
        }
        run(&mut s, &mut k, "sheet.flange", p);
    }
    (s, k, face)
}

/// A corner seam between the two walls of [`two_walls`] made apart: the front wall first.
fn seam(s: &mut Session, k: &mut OcctKernel, options: Value) -> u64 {
    // The upright edges at the ends of the two walls, where they come together.
    let front = run(s, k, "model.edge_ref", json!({ "at": [60, 0, 10] }));
    let right = run(s, k, "model.edge_ref", json!({ "at": [60, 5, 10] }));
    let mut p = json!({ "edges": [front, right] });
    for (key, v) in options.as_object().unwrap() {
        p[key] = v.clone();
    }
    id(&run(s, k, "sheet.corner_seam", p), "feature")
}

#[test]
fn a_corner_seam_closes_the_corner_between_flanges_made_apart() {
    let ba = allowance(FRAC_PI_2);
    // Two flanges, one after the other. The first runs the whole length of its edge, bend and
    // all, through the corner; the second starts where the first one's bend left its edge, so
    // the corner is left open by a bend's radius, with the end of the first bend sticking out.
    let (mut s, mut k, _) = two_walls(true, json!({ "corner": "trim" }), json!({}));
    let open = 55.0 * 35.0 * T + bend_volume(FRAC_PI_2, 60.0 + 35.0) + 10.0 * T * (60.0 + 35.0);
    assert!(near(volume(&mut s, &mut k), open), "{} {open}", volume(&mut s, &mut k));

    // A corner seam between the two ends. The bends now end where their bend lines cross (55
    // along the front, 5 along the side) and the walls are brought together with a gap of 2:
    // each stops 2 + root 2 from the corner, as in one flange.
    let end = 2.0 + 2f64.sqrt();
    let closed = |walls: f64| 55.0 * 35.0 * T + bend_volume(FRAC_PI_2, 90.0) + 10.0 * T * walls;
    let id = seam(&mut s, &mut k, json!({}));
    assert!(near(volume(&mut s, &mut k), closed(100.0 - 2.0 * end)), "{} {}", volume(&mut s, &mut k), closed(100.0 - 2.0 * end));
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "area"), 55.0 * 35.0 + ba * 90.0 + 10.0 * (100.0 - 2.0 * end)), "{f}");
    assert!(f["bends"].as_array().unwrap().len() == 2 && near(num(&f, "length"), 55.0 + ba + 10.0), "{f}");

    // Where the wall's end is: the middle of the end face of the front wall (which looks along
    // +X) and of the side wall (which looks along -Y).
    let ends = |s: &mut Session, k: &mut OcctKernel| {
        let all = flats(s, k);
        let at =
            |normal: Vec3| all.iter().find(|f| f.0.near(normal, 1e-9) && (f.2 - 10.0 * T).abs() < 1e-6).map(|f| f.1).expect("the wall's end face");
        (at(Vec3::X).x, at(-Vec3::Y).y)
    };
    let (front, side) = ends(&mut s, &mut k);
    assert!((front - (60.0 - end)).abs() < 1e-9 && (side - end).abs() < 1e-9, "{front} {side}");
    // The piece put on a wall's end is the wall: its outer face is one face its whole length,
    // and still the first flange's.
    let outer: Vec<_> = flats(&mut s, &mut k).into_iter().filter(|f| f.0.near(-Vec3::Y, 1e-9) && f.2 > 100.0).collect();
    assert!(outer.len() == 1 && near(outer[0].2, 10.0 * (60.0 - end)), "{outer:?}");
    let first = s.document().features().iter().find(|f| f.name == "Flange1").unwrap().id.0;
    let wall = run(&mut s, &mut k, "model.face_ref", json!({ "at": [56, 0, 10] }));
    assert!(wall["origin"]["type"] == "from" && wall["origin"]["feature"] == first, "{wall}");

    // An overlap: the first wall (the front one) runs right to the corner, across the end of
    // the second, which stops 2 short of its inner face. Reversed, the side wall runs across.
    update(&mut s, &mut k, id, json!({ "seam": "overlap" }));
    assert!(near(volume(&mut s, &mut k), closed(60.0 + 36.0)), "{}", volume(&mut s, &mut k));
    let (front, side) = ends(&mut s, &mut k);
    assert!((front - 60.0).abs() < 1e-9 && (side - 4.0).abs() < 1e-9, "{front} {side}");
    update(&mut s, &mut k, id, json!({ "seam": "reverse_overlap" }));
    let (front, side) = ends(&mut s, &mut k);
    assert!((front - 56.0).abs() < 1e-9 && side.abs() < 1e-9, "{front} {side}");
    assert!(near(volume(&mut s, &mut k), closed(56.0 + 40.0)));
    // Over half the other's thickness only; a gap of its own.
    update(&mut s, &mut k, id, json!({ "seam": "overlap", "overlap": 0.5, "gap": 0.5 }));
    let (front, side) = ends(&mut s, &mut k);
    assert!((front - 59.0).abs() < 1e-9 && (side - 2.5).abs() < 1e-9, "{front} {side}");
    // It is one sound solid that lays flat, whichever the seam.
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    assert!(near(num(&flat(&mut s, &mut k), "area"), 55.0 * 35.0 + ba * 90.0 + 10.0 * (59.0 + 37.5)));

    // With a relief, the seamed corner is the corner one flange makes of the same two edges:
    // the same solid and the same flat pattern, for every relief and seam.
    for (rule, options) in [
        (json!({ "corner": "round" }), json!({})),
        (json!({ "corner": "square" }), json!({})),
        (json!({ "corner": "round", "corner_size": 12 }), json!({ "seam": "overlap" })),
        (json!({ "corner": "trim" }), json!({ "seam": "reverse_overlap", "gap": 1.0, "relief": "round", "relief_size": 10.0 })),
    ] {
        let (mut one, mut k1, _) = two_walls(false, rule.clone(), options.clone());
        let (mut two, mut k2, _) = two_walls(true, rule.clone(), json!({}));
        seam(&mut two, &mut k2, options.clone());
        let (v1, v2) = (volume(&mut one, &mut k1), volume(&mut two, &mut k2));
        assert!((v1 - v2).abs() < 1e-6 * v1, "{rule} {options}: {v1} {v2}");
        let (f1, f2) = (flat(&mut one, &mut k1), flat(&mut two, &mut k2));
        for key in ["area", "cut_length", "length", "width"] {
            assert!((num(&f1, key) - num(&f2, key)).abs() < 1e-6 * num(&f1, key), "{rule} {options}: {key} {} {}", num(&f1, key), num(&f2, key));
        }
        let body = &run(&mut two, &mut k2, "model.topology", json!({}))["bodies"][0];
        assert!(body["valid"] == true && body["solids"] == 1, "{rule} {options}: {body}");
    }

    // Two flanges that are not alike: the side wall's bend has radius 1, so its bend line is 3
    // in from its edge and the two cross at (57, 5). Mitred with a gap of 2, each wall's end
    // follows the other's bend: the front one stops root 2 - 1 short of the crossing, the side
    // one reaches 3 - root 2 past it.
    let (mut s, mut k, _) = two_walls(true, json!({ "corner": "trim" }), json!({ "radius": 1 }));
    seam(&mut s, &mut k, json!({}));
    let folded = 57.0 * 35.0 * T
        + bend_volume(FRAC_PI_2, 57.0)
        + FRAC_PI_2 / 2.0 * (9.0 - 1.0) * 35.0
        + T * (10.0 * (57.0 - (2f64.sqrt() - 1.0)) + 12.0 * (35.0 + 3.0 - 2f64.sqrt()));
    assert!(near(volume(&mut s, &mut k), folded), "{} {folded}", volume(&mut s, &mut k));

    // What is no corner to close.
    let (mut s, mut k, _) = two_walls(true, json!({}), json!({}));
    let (front, right) =
        (run(&mut s, &mut k, "model.edge_ref", json!({ "at": [60, 0, 10] })), run(&mut s, &mut k, "model.edge_ref", json!({ "at": [60, 5, 10] })));
    // One edge; the top of a wall instead of its end; an edge of the plate.
    let e = refused(&mut s, &mut k, "sheet.corner_seam", json!({ "edges": [front] }));
    assert!(e.contains("takes two edges"), "{e}");
    let top = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [30, 0, 15] }));
    run(&mut s, &mut k, "sheet.corner_seam", json!({ "edges": [top, right] }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("has no bend along its foot") || e.contains("pick, on each, the straight edge at its end"), "{e}");
    run(&mut s, &mut k, "edit.undo", json!({}));
    let plate_corner = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [0, 40, 1] }));
    run(&mut s, &mut k, "sheet.corner_seam", json!({ "edges": [plate_corner, right] }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("A corner seam is between the ends of two faces of the sheet"), "{e}");
    run(&mut s, &mut k, "edit.undo", json!({}));
    let e = refused(&mut s, &mut k, "sheet.corner_seam", json!({ "edges": [front, right], "overlap": 1.5 }));
    assert!(e.contains("from 0 to 1"), "{e}");
}

#[test]
fn a_flange_gives_its_own_corners_a_seam_a_gap_and_a_relief() {
    // One flange on the front and the right edge. Overlapped, the first edge's wall runs right
    // to the corner and the second stops 2 short of its inner face.
    let closed = |walls: f64| 55.0 * 35.0 * T + bend_volume(FRAC_PI_2, 90.0) + 10.0 * T * walls;
    let (mut s, mut k, _) = two_walls(false, json!({ "corner": "trim" }), json!({ "seam": "overlap" }));
    assert!(near(volume(&mut s, &mut k), closed(60.0 + 36.0)), "{}", volume(&mut s, &mut k));
    let flange = u64::from(s.document().features().last().unwrap().id.0);
    // Half way into the other's thickness, with a gap of 1: 59 and 37.
    update(&mut s, &mut k, flange, json!({ "overlap": 0.5, "gap": 1.0 }));
    assert!(near(volume(&mut s, &mut k), closed(59.0 + 37.0)), "{}", volume(&mut s, &mut k));
    // A relief of the flange's own in place of the rule's (which has none): square, 8 across.
    update(&mut s, &mut k, flange, json!({ "seam": "symmetric", "overlap": 1.0, "gap": null, "relief": "square", "relief_size": 8.0 }));
    let (mut by_rule, mut k2, _) = two_walls(false, json!({ "corner": "square" }), json!({}));
    let (v1, v2) = (volume(&mut s, &mut k), volume(&mut by_rule, &mut k2));
    assert!(near(v1, v2), "{v1} {v2}");
    // Not mitred, the seam says nothing: each wall ends where its bend does.
    update(&mut s, &mut k, flange, json!({ "miter": false, "seam": "overlap", "relief": "trim" }));
    assert!(near(volume(&mut s, &mut k), closed(90.0)), "{}", volume(&mut s, &mut k));
    // What it cannot have.
    let (mut s, mut k, _) = two_walls(false, json!({}), json!({}));
    let front = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [30, 40, 2] }));
    let e = refused(&mut s, &mut k, "sheet.flange", json!({ "edges": [front], "height": 15, "overlap": -0.5 }));
    assert!(e.contains("from 0 to 1"), "{e}");
    let e = refused(&mut s, &mut k, "sheet.flange", json!({ "edges": [front], "height": 15, "seam": "welded" }));
    assert!(e.contains("welded"), "{e}");
}

/// A 60 x 40 plate with a wall 15 high bent up from its right edge, and from its left edge too
/// when `both`. Laid flat from the plate, the plate reaches from x = 5 (or 0) to 55, a bend is
/// a quarter turn's allowance wide beyond that, and a wall 10 beyond its bend.
fn channel(both: bool) -> (Session, OcctKernel, u64) {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
    let mut edges = vec![rim(&mut s, &mut k, face, "end", lines[1])];
    if both {
        edges.push(rim(&mut s, &mut k, face, "end", lines[3]));
    }
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": edges, "height": 15 }));
    (s, k, face)
}

/// A sketch on the underside of the plate (the XY plane) or on its top, and where the part's
/// point (x, y) of that face is in it.
fn sketch_on(s: &mut Session, k: &mut OcctKernel, face: u64, top: bool) -> (u64, impl Fn(f64, f64) -> (f64, f64) + use<>) {
    let on = if top {
        let top = run(s, k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": face, "end": "end" } }));
        json!({ "face": top })
    } else {
        json!({ "plane": "xy" })
    };
    let sk = id(&run(s, k, "sketch.create", on), "feature");
    let frame = s.regen(k).sketch_frames[&tenon_model::FeatureId(sk as u32)];
    (sk, move |x: f64, y: f64| {
        let p = frame.to_local(Vec3::new(x, y, if top { T } else { 0.0 }));
        (p.x, p.y)
    })
}

/// The volume of bend that a piece of the flat pattern `area` large becomes, cut from radius
/// `from` to radius `to` through the sheet.
fn bend_share(area: f64, from: f64, to: f64) -> f64 {
    area * (to * to - from * from) / (2.0 * (R + K * T))
}

#[test]
fn a_cut_across_a_bend_goes_round_it_and_is_its_profile_in_the_flat_pattern() {
    let ba = allowance(FRAC_PI_2);
    let (mut s, mut k, face) = channel(false);
    let whole = volume(&mut s, &mut k);
    let before = flat(&mut s, &mut k);

    // A slot 8 wide drawn on the underside, from 5 short of the bend line to 4 up the wall: 5 of
    // plate, all of the bend, 4 of wall.
    let (sk, _) = sketch_on(&mut s, &mut k, face, false);
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 50, "y1": 16, "x2": 55.0 + ba + 4.0, "y2": 24 }));
    let cut = id(&run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk })), "feature");
    assert_eq!(s.document().feature(tenon_model::FeatureId(cut as u32)).unwrap().name, "Cut1");
    let slot = 8.0 * 5.0 * T + bend_volume(FRAC_PI_2, 8.0) + 8.0 * 4.0 * T;
    assert!(near(volume(&mut s, &mut k), whole - slot), "{} {}", volume(&mut s, &mut k), whole - slot);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    // In the wall it is 8 wide and 4 high, from where the bend ends.
    let outer = flats(&mut s, &mut k).into_iter().find(|f| f.0.near(Vec3::X, 1e-9) && f.1.x > 59.9).expect("the outer face of the wall");
    assert!(near(outer.2, 400.0 - 32.0), "{outer:?}");
    // In the flat pattern it is the rectangle that was drawn: four lines.
    let f = flat(&mut s, &mut k);
    assert!(
        near(num(&f, "area"), num(&before, "area") - 8.0 * (9.0 + ba)) && near(num(&f, "cut_length"), num(&before, "cut_length") + 2.0 * (17.0 + ba)),
        "{f}"
    );
    let inner = f["inner"].as_array().unwrap();
    assert!(inner.len() == 1 && inner[0].as_array().unwrap().len() == 4 && inner[0].as_array().unwrap().iter().all(|c| c["type"] == "line"), "{f}");
    // A wall of the slot is named as the cut made it, where it goes round the bend too (half
    // way round it, half way through the sheet).
    let side = run(&mut s, &mut k, "model.face_ref", json!({ "at": [55.0 + 4.0 / 2f64.sqrt(), 16, 5.0 - 4.0 / 2f64.sqrt()] }));
    assert!(side["origin"]["type"] == "from" && side["origin"]["feature"] == cut, "{side}");

    // A round hole of radius 4 centred on the bend line: half of it on the plate, half on the
    // bend. Drawn on the underside (the outside of the bend) or on the top (its inside), it is
    // the same hole, and in the flat pattern it is the circle that was drawn.
    for top in [false, true] {
        let (mut s, mut k, face) = channel(false);
        let (sk, to) = sketch_on(&mut s, &mut k, face, top);
        let (x, y) = to(55.0, 20.0);
        run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": x, "cy": y, "r": 4 }));
        run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk }));
        let hole = 8.0 * PI * T + bend_share(8.0 * PI, R, R + T);
        assert!(near(volume(&mut s, &mut k), whole - hole), "top {top}: {} {}", volume(&mut s, &mut k), whole - hole);
        let f = flat(&mut s, &mut k);
        let inner = f["inner"].as_array().unwrap();
        assert!(inner.len() == 1 && inner[0].as_array().unwrap().len() == 1 && inner[0][0]["type"] == "circle", "top {top}: {}", f["inner"]);
        let c = &inner[0][0];
        assert!(near(num(&c["center"], "x"), 55.0) && near(num(&c["center"], "y"), 20.0) && near(num(c, "radius"), 4.0), "top {top}: {c}");
        assert!(near(num(&f, "area"), num(&before, "area") - 16.0 * PI), "{f}");
    }
}

#[test]
fn a_cut_across_bends_crosses_as_many_as_its_profile_does_and_goes_as_deep_as_asked() {
    let ba = allowance(FRAC_PI_2);
    // A channel: walls on the left and on the right. A slot 4 wide from 3 up the left wall,
    // round its bend, across the 50 of plate, round the other bend and 3 up the right wall.
    let (mut s, mut k, face) = channel(true);
    let whole = volume(&mut s, &mut k);
    let before = flat(&mut s, &mut k);
    let (sk, _) = sketch_on(&mut s, &mut k, face, false);
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 5.0 - ba - 3.0, "y1": 18, "x2": 55.0 + ba + 3.0, "y2": 22 }));
    let cut = id(&run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk })), "feature");
    let slot = 4.0 * (50.0 + 2.0 * 3.0) * T + 2.0 * bend_volume(FRAC_PI_2, 4.0);
    assert!(near(volume(&mut s, &mut k), whole - slot), "{} {}", volume(&mut s, &mut k), whole - slot);
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "area"), num(&before, "area") - 4.0 * (56.0 + 2.0 * ba)) && f["inner"].as_array().unwrap().len() == 1, "{f}");
    // Each bend is in two now, one either side of the slot, each with its own centre line.
    let bends = f["bends"].as_array().unwrap();
    assert!(bends.len() == 4 && bends.iter().all(|b| b["lines"].as_array().unwrap().len() == 1), "{f}");

    // Not through: 0.5 deep from the underside. On the plate and the walls that is a pocket
    // 0.5 deep; on the bends, whose outside the underside is, the outer 0.5 of their thickness.
    update(&mut s, &mut k, cut, json!({ "distance": 0.5 }));
    let pocket = 4.0 * 56.0 * 0.5 + 2.0 * bend_share(4.0 * ba, R + T - 0.5, R + T);
    assert!(near(volume(&mut s, &mut k), whole - pocket), "{} {}", volume(&mut s, &mut k), whole - pocket);
    // The same from the top, which is the inside of the bends: their inner 0.5. Here by an
    // equation of the thickness.
    let (mut s, mut k, face) = channel(true);
    let (sk, to) = sketch_on(&mut s, &mut k, face, true);
    let ((x1, y1), (x2, y2)) = (to(5.0 - ba - 3.0, 18.0), to(55.0 + ba + 3.0, 22.0));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": x1, "y1": y1, "x2": x2, "y2": y2 }));
    let cut = id(
        &run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk, "distance": 1, "equations": { "/distance": "Thickness / 4" } })),
        "feature",
    );
    let pocket = 4.0 * 56.0 * 0.5 + 2.0 * bend_share(4.0 * ba, R, R + 0.5);
    assert!(near(volume(&mut s, &mut k), whole - pocket), "{} {}", volume(&mut s, &mut k), whole - pocket);
    // As deep as the sheet is thick, or deeper, is through.
    update(&mut s, &mut k, cut, json!({ "distance": 5.0 }));
    assert!(near(volume(&mut s, &mut k), whole - slot), "{} {}", volume(&mut s, &mut k), whole - slot);
}

#[test]
fn a_cut_across_a_bend_keeps_an_island_on_a_face_and_says_what_it_cannot_cut() {
    let (mut s, mut k, face) = channel(false);
    let whole = volume(&mut s, &mut k);
    // A ring: a circle of radius 6 on the bend line, with an island of radius 2 that is all on
    // the plate. The island stays; the half of the ring on the bend has no hole in it.
    let (sk, _) = sketch_on(&mut s, &mut k, face, false);
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 55, "cy": 20, "r": 6 }));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 52, "cy": 20, "r": 2 }));
    run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk }));
    let ring = (18.0 * PI - 4.0 * PI) * T + bend_share(18.0 * PI, R, R + T);
    assert!(near(volume(&mut s, &mut k), whole - ring), "{} {}", volume(&mut s, &mut k), whole - ring);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 2, "the island is cut loose: {body}");
    // With the island across the bend line it stays too, partly plate and partly bend: the
    // slice of it on the plate is what a chord 1 from its centre cuts off.
    run(&mut s, &mut k, "edit.undo", json!({}));
    run(&mut s, &mut k, "edit.undo", json!({}));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 56, "cy": 20, "r": 2 }));
    run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk }));
    let slice = 4.0 * 0.5f64.acos() - 3f64.sqrt();
    let ring = (18.0 * PI - slice) * T + bend_share(18.0 * PI - (4.0 * PI - slice), R, R + T);
    assert!(near(volume(&mut s, &mut k), whole - ring), "{} {}", volume(&mut s, &mut k), whole - ring);
    // All of it on the bend: not yet.
    run(&mut s, &mut k, "edit.undo", json!({}));
    run(&mut s, &mut k, "edit.undo", json!({}));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 58, "cy": 20, "r": 2 }));
    run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("an island that lies on a bend"), "{e}");

    // A sketch that is on no side of the sheet: the XZ plane holds the front rim of the plate.
    let (mut s, mut k, _) = channel(false);
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" })), "feature");
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 30, "cy": 1, "r": 0.5 }));
    run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("must be on a flat face of the sheet"), "{e}");
    // A profile that is nowhere on the sheet laid flat.
    let (mut s, mut k, face) = channel(false);
    let (sk, _) = sketch_on(&mut s, &mut k, face, false);
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 30, "cy": 200, "r": 3 }));
    run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("The profile is not on the sheet"), "{e}");
    // What it cannot be asked.
    let e = refused(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk, "distance": 0 }));
    assert!(e.contains("must be more than 0"), "{e}");
}

/// The ends of every line of sketch `sk` that is reference geometry laid flat, as the document
/// has them, and how many such projections `sketch.info` lists.
fn laid_flat(s: &mut Session, k: &mut OcctKernel, sk: u64) -> (Vec<(f64, f64)>, usize) {
    let info = run(s, k, "sketch.info", json!({ "sketch": sk }));
    let flat: Vec<u64> =
        info["projections"].as_array().map_or(Vec::new(), |l| l.iter().filter(|p| p["flat"] == true).map(|p| id(p, "entity")).collect());
    let sketch = s.document().sketch(tenon_model::FeatureId(sk as u32)).unwrap();
    let mut ends = Vec::new();
    for e in &flat {
        if let Some(tenon_sketch::Geometry::Line { start, end }) = sketch.geometry(tenon_sketch::EntityId(*e as u32)) {
            for p in [sketch.point(*start).unwrap(), sketch.point(*end).unwrap()] {
                ends.push((p.x, p.y));
            }
        }
    }
    (ends, flat.len())
}

#[test]
fn the_sheet_beyond_a_bend_is_shown_laid_flat_in_a_sketch_and_follows_the_part() {
    let ba = allowance(FRAC_PI_2);
    let (mut s, mut k, face) = channel(false);
    let whole = volume(&mut s, &mut k);
    let (sk, to) = sketch_on(&mut s, &mut k, face, true);
    // The inside of the wall (it looks back along X) and its outside, 10 x 40 each.
    let wall = |s: &mut Session, k: &mut OcctKernel, looks: f64| {
        let faces = run(s, k, "model.faces", json!({}));
        let found = faces["faces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["surface"]["Plane"]["normal"]["x"].as_f64().is_some_and(|x| near(x, looks)) && near(num(f, "area"), 400.0))
            .map(|f| id(f, "face"))
            .expect("a face of the wall");
        run(s, k, "model.face_ref", json!({ "face": found }))
    };
    let inside = wall(&mut s, &mut k, -1.0);
    let outside = wall(&mut s, &mut k, 1.0);

    // Project Flat Pattern on the inside of the wall, in a sketch on the top of the plate: its
    // four edges, where the wall lies with the sheet laid flat from the plate. That is the
    // bend's allowance past the bend line (x = 55), and 10 further.
    let r = run(&mut s, &mut k, "sketch.project", json!({ "sketch": sk, "face": inside, "flat": true }));
    assert!(r["entities"].as_array().unwrap().len() == 4 && r["skipped"].as_array().unwrap().is_empty(), "{r}");
    let (ends, listed) = laid_flat(&mut s, &mut k, sk);
    assert_eq!((ends.len(), listed), (8, 4));
    let has = |ends: &[(f64, f64)], x: f64, y: f64| {
        let (px, py) = to(x, y);
        ends.iter().filter(|e| (e.0 - px).abs() < 1e-9 && (e.1 - py).abs() < 1e-9).count() == 2
    };
    for (x, y) in [(55.0 + ba, 0.0), (55.0 + ba, 40.0), (65.0 + ba, 0.0), (65.0 + ba, 40.0)] {
        assert!(has(&ends, x, y), "a corner of the wall at ({x}, {y}): {ends:?}");
    }
    // It is reference geometry: again is nothing new.
    let again = run(&mut s, &mut k, "sketch.project", json!({ "sketch": sk, "face": inside, "flat": true }));
    assert_eq!(again["entities"], r["entities"]);
    assert_eq!(laid_flat(&mut s, &mut k, sk).1, 4);

    // A hole drawn 5 up the wall from where the bend ends, by what is shown there, and cut
    // across the bend: it is in the wall, 5 above the bend (z = 10), and nowhere else.
    let (cx, cy) = to(55.0 + ba + 5.0, 20.0);
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": cx, "cy": cy, "r": 3 }));
    let cut = id(&run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk })), "feature");
    assert!(near(volume(&mut s, &mut k), whole - 9.0 * PI * T), "{} {}", volume(&mut s, &mut k), whole - 9.0 * PI * T);
    let hole = run(&mut s, &mut k, "model.face_ref", json!({ "at": [59, 20.0 + 3.0 / 2f64.sqrt(), 10.0 + 3.0 / 2f64.sqrt()] }));
    assert!(hole["origin"]["type"] == "from" && hole["origin"]["feature"] == cut, "{hole}");
    // A round hole in a flat face has one wall, as a drilled one has.
    let walls = run(&mut s, &mut k, "model.faces", json!({}))["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["surface"]["Cylinder"]["radius"].as_f64().is_some_and(|r| near(r, 3.0)) && near(num(f, "area"), 6.0 * PI * T))
        .count();
    assert_eq!(walls, 1);

    // The wall made 10 higher: what is shown of it follows, its far edge 10 further out. The
    // hole stays where it was drawn.
    let flange = s.document().features().iter().find(|f| f.name == "Flange1").unwrap().id.0;
    update(&mut s, &mut k, u64::from(flange), json!({ "height": 25 }));
    assert!(near(volume(&mut s, &mut k), whole + 10.0 * T * 40.0 - 9.0 * PI * T));
    let (ends, _) = laid_flat(&mut s, &mut k, sk);
    assert!(has(&ends, 75.0 + ba, 0.0) && has(&ends, 75.0 + ba, 40.0) && has(&ends, 55.0 + ba, 0.0), "{ends:?}");

    // The other side of the sheet has no place on this side laid flat; a part that is not
    // sheet metal has no flat pattern at all.
    let r = run(&mut s, &mut k, "sketch.project", json!({ "sketch": sk, "face": outside, "flat": true }));
    assert!(
        r["entities"].as_array().unwrap().is_empty()
            && r["skipped"][0].as_str().is_some_and(|e| e.contains("not on the side of the sheet the sketch is on")),
        "{r}"
    );
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 40, "y2": 30 }));
    let block = id(&run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 10 })), "feature");
    let top = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": block, "end": "end" } }));
    let on = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    let r = run(&mut s, &mut k, "sketch.project", json!({ "sketch": on, "face": top, "flat": true }));
    assert!(r["skipped"][0].as_str().is_some_and(|e| e.contains("not sheet metal")), "{r}");
}

/// A sheet metal part with a 40 x 30 plate on the XY plane, and a second face made apart from
/// it: a `w` x 30 rectangle from (`x`, 0) on a plane `z` above the XY plane (`None`: standing
/// on the YZ plane from height 10 to 40). Returns the two face features.
fn two_faces(second: Option<(f64, f64, f64)>, new_body: bool) -> (Session, OcctKernel, u64, u64) {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (first, _) = plate(&mut s, &mut k, 40.0, 30.0);
    let sk = match second {
        Some((x, w, z)) => {
            let plane = id(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": z })), "feature");
            let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": plane })), "feature");
            run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": x, "y1": 0, "x2": x + w, "y2": 30 }));
            sk
        }
        None => {
            let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "yz" })), "feature");
            run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 10, "x2": 30, "y2": 40 }));
            sk
        }
    };
    let operation = if new_body { "new_body" } else { "join" };
    let face = id(&run(&mut s, &mut k, "sheet.face", json!({ "sketch": sk, "operation": operation })), "feature");
    (s, k, first, face)
}

/// A bend between the edges at two points.
fn bend(s: &mut Session, k: &mut OcctKernel, a: [f64; 3], b: [f64; 3], options: Value) -> u64 {
    let (first, second) = (run(s, k, "model.edge_ref", json!({ "at": a })), run(s, k, "model.edge_ref", json!({ "at": b })));
    let mut p = json!({ "edges": [first, second] });
    for (key, v) in options.as_object().unwrap() {
        p[key] = v.clone();
    }
    id(&run(s, k, "sheet.bend", p), "feature")
}

fn solids(s: &mut Session, k: &mut OcctKernel) -> (u64, u64, bool) {
    let t = run(s, k, "model.topology", json!({}));
    let bodies = t["bodies"].as_array().unwrap();
    (bodies.len() as u64, bodies.iter().map(|b| b["solids"].as_u64().unwrap()).sum(), bodies.iter().all(|b| b["valid"] == true))
}

#[test]
fn a_bend_joins_a_face_made_apart_to_the_sheet_and_the_two_lay_flat_as_one() {
    let ba = allowance(FRAC_PI_2);
    for new_body in [false, true] {
        // A plate, and a wall standing over its left edge: 2 thick from x = 0, from 10 above
        // the XY plane to 40. Made apart they are two pieces of sheet (of one solid body, or
        // each a solid of its own).
        let (mut s, mut k, _, wall) = two_faces(None, new_body);
        assert!(near(volume(&mut s, &mut k), 2400.0 + 1800.0));
        assert_eq!(solids(&mut s, &mut k), (if new_body { 2 } else { 1 }, 2, true));
        // A bend between the plate's left edge and the wall's bottom edge. The wall's face
        // towards the plate is 2 in from the plate's edge and the bend's inside radius is 3,
        // so the plate is cut back by 5 and the wall extended down by 5, to where the bend
        // begins on each.
        let b = bend(&mut s, &mut k, [0.0, 15.0, 2.0], [2.0, 15.0, 10.0], json!({}));
        assert_eq!(s.document().feature(tenon_model::FeatureId(b as u32)).unwrap().name, "Bend1");
        let whole = 35.0 * 30.0 * T + bend_volume(FRAC_PI_2, 30.0) + 35.0 * 30.0 * T;
        assert!(near(volume(&mut s, &mut k), whole), "{} {whole}", volume(&mut s, &mut k));
        assert_eq!(solids(&mut s, &mut k), (1, 1, true), "one piece of sheet now");
        // It lays flat as one sheet with one bend: plate, bend, wall.
        let f = flat(&mut s, &mut k);
        assert!(near(num(&f, "length"), 70.0 + ba) && near(num(&f, "width"), 30.0) && near(num(&f, "area"), (70.0 + ba) * 30.0), "{f}");
        let bends = f["bends"].as_array().unwrap();
        assert!(bends.len() == 1 && near(num(&bends[0], "angle"), FRAC_PI_2) && near(num(&bends[0], "radius"), R), "{f}");
        // The wall's face is the wall's still, its whole height: what the bend put on its
        // foot is the wall.
        let face = run(&mut s, &mut k, "model.face_ref", json!({ "at": [2, 15, 20] }));
        assert!(face["origin"]["feature"] == wall, "{face}");
        let inner: Vec<_> = flats(&mut s, &mut k).into_iter().filter(|f| f.0.near(Vec3::X, 1e-9) && near(f.2, 35.0 * 30.0)).collect();
        assert_eq!(inner.len(), 1, "the wall's inner face, 35 high");
        // A radius of the bend's own: 1. The plate is cut back by 2 + 1, the wall let down by 7.
        update(&mut s, &mut k, b, json!({ "radius": 1 }));
        let tighter = 37.0 * 30.0 * T + FRAC_PI_2 / 2.0 * (9.0 - 1.0) * 30.0 + 37.0 * 30.0 * T;
        assert!(near(volume(&mut s, &mut k), tighter), "{} {tighter}", volume(&mut s, &mut k));
    }

    // The wall further off (its foot 20 above the plate, its near face 6 out past the plate's
    // edge): plate and wall are both extended to the bend. Picked by the other edge of the
    // wall's foot, it is the same bend.
    for at in [8.0, 6.0] {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        plate(&mut s, &mut k, 40.0, 30.0);
        let plane = id(&run(&mut s, &mut k, "work.plane", json!({ "base": "yz", "distance": -8 })), "feature");
        let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": plane })), "feature");
        run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 22, "x2": 30, "y2": 40 }));
        run(&mut s, &mut k, "sheet.face", json!({ "sketch": sk }));
        // (The wall is 2 thick from x = -8 to x = -6.)
        bend(&mut s, &mut k, [0.0, 15.0, 2.0], [-at, 15.0, 22.0], json!({}));
        // The bend turns up outside the plate's edge, its inside the plate's top and the wall's
        // near face (x = -6): 3 of plate added to reach it, and 20 - 3 of wall.
        let whole = 43.0 * 30.0 * T + bend_volume(FRAC_PI_2, 30.0) + (18.0 + 17.0) * 30.0 * T;
        assert!(near(volume(&mut s, &mut k), whole), "{at}: {} {whole}", volume(&mut s, &mut k));
        assert_eq!(solids(&mut s, &mut k), (1, 1, true));
    }
}

#[test]
fn faces_in_parallel_planes_are_joined_by_a_double_bend() {
    let ba = allowance(FRAC_PI_2);
    let ring = |angle: f64| angle / 2.0 * ((R + T) * (R + T) - R * R) * 30.0;
    // Side by side: a second plate 40 long, 12 higher than the first, its edge 20 from the
    // first one's edge and facing it.
    let side_by_side = || {
        let (s, k, _, _) = two_faces(Some((60.0, 40.0, 12.0)), false);
        (s, k)
    };
    let plates = 2.0 * 2400.0;
    // 90 Degree: a quarter turn up from the first edge, 4 of face, a quarter turn back, and
    // the second plate brought the 12 that is left to there.
    let (mut s, mut k) = side_by_side();
    let b = bend(&mut s, &mut k, [40.0, 15.0, 2.0], [60.0, 15.0, 12.0], json!({ "double": "90_degree" }));
    let z = plates + 2.0 * ring(FRAC_PI_2) + 4.0 * 30.0 * T + 12.0 * 30.0 * T;
    assert!(near(volume(&mut s, &mut k), z), "{} {z}", volume(&mut s, &mut k));
    assert_eq!(solids(&mut s, &mut k), (1, 1, true));
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), 40.0 + ba + 4.0 + ba + 12.0 + 40.0) && f["bends"].as_array().unwrap().len() == 2, "{f}");
    // One bend up and one down, seen from one side of the sheet.
    let ups: Vec<bool> = f["bends"].as_array().unwrap().iter().map(|b| b["up"] == true).collect();
    assert!(ups[0] != ups[1], "{f}");
    // The first plate is as it was (its top is 40 x 30); the second is 12 longer.
    let tops: Vec<f64> = flats(&mut s, &mut k).into_iter().filter(|f| f.0.near(Vec3::Z, 1e-9)).map(|f| f.2).collect();
    assert!(tops.iter().any(|a| near(*a, 1200.0)) && tops.iter().any(|a| near(*a, 52.0 * 30.0)), "{tops:?}");
    // With the second edge the one that stays, the first plate is the one made longer.
    update(&mut s, &mut k, b, json!({ "flip_fixed": true }));
    assert!(near(volume(&mut s, &mut k), z));
    let tops: Vec<f64> = flats(&mut s, &mut k).into_iter().filter(|f| f.0.near(Vec3::Z, 1e-9)).map(|f| f.2).collect();
    assert!(tops.iter().any(|a| near(*a, 52.0 * 30.0)) && tops.iter().any(|a| near(*a, 1200.0)), "{tops:?}");

    // 45 Degree: two eighths of a turn and the face between them on the slant.
    update(&mut s, &mut k, b, json!({ "double": "45_degree", "flip_fixed": false }));
    let quarter = std::f64::consts::FRAC_PI_4;
    let web = (12.0 - 8.0 * (1.0 - quarter.cos())) / quarter.sin();
    let run = 8.0 * quarter.sin() + web * quarter.cos();
    let z45 = plates + 2.0 * ring(quarter) + web * 30.0 * T + (20.0 - run) * 30.0 * T;
    assert!(near(volume(&mut s, &mut k), z45), "{} {z45}", volume(&mut s, &mut k));
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), 80.0 + 2.0 * allowance(quarter) + web + 20.0 - run), "{f}");

    // Fix Edges (what a bend does unless told): neither plate changes; the two bends turn
    // as far as it takes for them and the face between them to reach from edge to edge.
    update(&mut s, &mut k, b, json!({ "double": "fix_edges" }));
    let whole = volume(&mut s, &mut k);
    let f = flat(&mut s, &mut k);
    let angle = num(&f["bends"][0], "angle");
    assert!(near(angle, num(&f["bends"][1], "angle")) && angle > 0.5 && angle < FRAC_PI_2, "{f}");
    // The face between the bends is what the rise and the reach leave of each: the same.
    let web = (12.0 - 8.0 * (1.0 - angle.cos())) / angle.sin();
    assert!(near((20.0 - 8.0 * angle.sin()) / angle.cos(), web), "{angle} {web}");
    assert!(near(whole, plates + 2.0 * ring(angle) + web * 30.0 * T), "{whole}");
    assert!(near(num(&f, "length"), 80.0 + 2.0 * allowance(angle) + web), "{f}");
    let tops: Vec<f64> = flats(&mut s, &mut k).into_iter().filter(|f| f.0.near(Vec3::Z, 1e-9)).map(|f| f.2).collect();
    assert!(tops.len() == 2 && tops.iter().all(|a| near(*a, 1200.0)), "{tops:?}");
    // Full Radius is for faces one over the other.
    update(&mut s, &mut k, b, json!({ "double": "full_radius" }));
    assert!(failure(&mut s, &mut k).contains("side by side"));

    // One over the other: a second plate 34 long whose underside is 10 above the first one's
    // top, its edge 4 further out than the first one's, the same way.
    let (mut s, mut k, _, _) = two_faces(Some((10.0, 34.0, 12.0)), false);
    let b = bend(&mut s, &mut k, [40.0, 15.0, 2.0], [44.0, 15.0, 14.0], json!({ "double": "full_radius" }));
    // Half a circle, 10 across inside, from the first edge; the second plate cut back by 4.
    let half = std::f64::consts::PI / 2.0 * (49.0 - 25.0) * 30.0;
    let u = 2400.0 + 30.0 * 30.0 * T + half;
    assert!(near(volume(&mut s, &mut k), u), "{} {u}", volume(&mut s, &mut k));
    assert_eq!(solids(&mut s, &mut k), (1, 1, true));
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), 40.0 + PI * (5.0 + K * T) + 30.0) && near(num(&f["bends"][0], "radius"), 5.0), "{f}");
    // 90 Degree: two quarter turns of the rule's radius and 4 of face between them.
    update(&mut s, &mut k, b, json!({ "double": "90_degree" }));
    let c = 2400.0 + 30.0 * 30.0 * T + 2.0 * ring(FRAC_PI_2) + 4.0 * 30.0 * T;
    assert!(near(volume(&mut s, &mut k), c), "{} {c}", volume(&mut s, &mut k));
    // The second edge fixed: the first plate is 4 longer and the second keeps its 34.
    update(&mut s, &mut k, b, json!({ "flip_fixed": true }));
    assert!(near(volume(&mut s, &mut k), c + 2.0 * 4.0 * 30.0 * T), "{}", volume(&mut s, &mut k));
    // Fix Edges and 45 Degree are for faces side by side.
    update(&mut s, &mut k, b, json!({ "double": "fix_edges" }));
    assert!(failure(&mut s, &mut k).contains("one over the other"));
}

#[test]
fn a_bend_says_what_it_cannot_join_and_leaves_other_pieces_of_sheet_alone() {
    // Three pieces of sheet: the plate, a wall over its left edge and a second plate above its
    // right end. A flange goes on one of them while the others are loose, and a bend joins
    // two of the three.
    let (mut s, mut k, _, _) = two_faces(None, false);
    let plane = id(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 12 })), "feature");
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": plane })), "feature");
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 60, "y1": 0, "x2": 100, "y2": 30 }));
    run(&mut s, &mut k, "sheet.face", json!({ "sketch": sk }));
    assert_eq!(solids(&mut s, &mut k), (1, 3, true));
    let far = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [100, 15, 14] }));
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [far], "height": 10 }));
    assert_eq!(solids(&mut s, &mut k), (1, 3, true), "a flange on one piece of three");
    bend(&mut s, &mut k, [0.0, 15.0, 2.0], [2.0, 15.0, 10.0], json!({}));
    assert_eq!(solids(&mut s, &mut k), (1, 2, true), "two of the three are one now");

    // Edges that do not run alongside one another: the wall's side edge, and the plate's front.
    let (mut s, mut k, _, _) = two_faces(None, false);
    bend(&mut s, &mut k, [0.0, 15.0, 2.0], [2.0, 0.0, 25.0], json!({}));
    assert!(failure(&mut s, &mut k).contains("not parallel"));
    run(&mut s, &mut k, "edit.undo", json!({}));
    // The same edge twice; one edge only; a radius that is none.
    let edge = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [0, 15, 2] }));
    run(&mut s, &mut k, "sheet.bend", json!({ "edges": [edge.clone(), edge.clone()] }));
    assert!(failure(&mut s, &mut k).contains("picked twice"));
    run(&mut s, &mut k, "edit.undo", json!({}));
    let e = refused(&mut s, &mut k, "sheet.bend", json!({ "edges": [edge.clone()] }));
    assert!(e.contains("takes two edges"), "{e}");
    let other = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [2, 15, 10] }));
    let e = refused(&mut s, &mut k, "sheet.bend", json!({ "edges": [edge.clone(), other.clone()], "radius": 0 }));
    assert!(e.contains("bend radius must be from"), "{e}");
    let e = refused(&mut s, &mut k, "sheet.bend", json!({ "edges": [edge, other], "double": "three" }));
    assert!(e.contains("three"), "{e}");

    // Faces in one plane have nothing to bend between them; faces beside one another along
    // their edges, not across them, do not face each other.
    let (mut s, mut k, _, _) = two_faces(Some((60.0, 40.0, 0.0)), false);
    bend(&mut s, &mut k, [40.0, 15.0, 2.0], [60.0, 15.0, 2.0], json!({}));
    assert!(failure(&mut s, &mut k).contains("in one plane"));
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    plate(&mut s, &mut k, 40.0, 30.0);
    let plane = id(&run(&mut s, &mut k, "work.plane", json!({ "base": "xy", "distance": 12 })), "feature");
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": plane })), "feature");
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 60, "y1": 50, "x2": 100, "y2": 80 }));
    run(&mut s, &mut k, "sheet.face", json!({ "sketch": sk }));
    bend(&mut s, &mut k, [40.0, 15.0, 2.0], [60.0, 65.0, 12.0], json!({ "double": "90_degree" }));
    assert!(failure(&mut s, &mut k).contains("do not run alongside one another"));
}

/// A 60 x 40 plate with a line drawn on its top (or on its underside) from (40, 0) to (40, 40),
/// and the fold along it. Returns the fold feature.
fn folded(top: bool, options: Value) -> (Session, OcctKernel, u64) {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, _) = plate(&mut s, &mut k, 60.0, 40.0);
    let (sk, to) = sketch_on(&mut s, &mut k, face, top);
    let ((x1, y1), (x2, y2)) = (to(40.0, 0.0), to(40.0, 40.0));
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": x1, "y1": y1, "x2": x2, "y2": y2 }));
    let mut p = json!({ "sketch": sk });
    for (key, v) in options.as_object().unwrap() {
        p[key] = v.clone();
    }
    let fold = id(&run(&mut s, &mut k, "sheet.fold", p), "feature");
    (s, k, fold)
}

#[test]
fn a_fold_turns_the_sheet_beyond_a_line_about_a_bend_and_adds_nothing() {
    let ba = allowance(FRAC_PI_2);
    // A quarter turn up along a line 40 from the plate's left end, the line the middle of the
    // bend. Seen from above, the sheet to the left of the line as drawn (towards x = 0) is what
    // folds. The bend takes its allowance of the 60: what is left is flat or stands upright.
    let (mut s, mut k, fold) = folded(true, json!({}));
    assert_eq!(s.document().feature(tenon_model::FeatureId(fold as u32)).unwrap().name, "Fold1");
    let whole = (60.0 - ba) * 40.0 * T + bend_volume(FRAC_PI_2, 40.0);
    assert!(near(volume(&mut s, &mut k), whole), "{} {whole}", volume(&mut s, &mut k));
    assert_eq!(solids(&mut s, &mut k), (1, 1, true));
    // What folded stands up from where the bend ends: its face that was the top looks back
    // along X from 3 short of where the bend starts (40 + half the allowance), and it is as
    // long as the plate was to the left of the bend.
    let stood = 40.0 - ba / 2.0;
    let inner: Vec<_> = flats(&mut s, &mut k).into_iter().filter(|f| f.0.near(Vec3::X, 1e-9) && near(f.2, stood * 40.0)).collect();
    assert_eq!(inner.len(), 1, "the inside of what stands up");
    assert!(near(inner[0].1.x, 40.0 + ba / 2.0 - R) && near(inner[0].1.z, T + R + stood / 2.0), "{:?}", inner[0]);
    // What stayed is the 20 to the right, less half the allowance.
    let stayed: Vec<_> = flats(&mut s, &mut k).into_iter().filter(|f| f.0.near(Vec3::Z, 1e-9) && near(f.2, (20.0 - ba / 2.0) * 40.0)).collect();
    assert!(stayed.len() == 1 && near(stayed[0].1.x, 60.0 - (20.0 - ba / 2.0) / 2.0), "{stayed:?}");
    // Laid flat it is the plate it was folded from: nothing was added.
    let f = flat(&mut s, &mut k);
    // (It is laid out from its largest flat face, which is now the piece that stands up.)
    let sides = |f: &Value| (num(f, "length").max(num(f, "width")), num(f, "length").min(num(f, "width")));
    assert!(near(sides(&f).0, 60.0) && near(sides(&f).1, 40.0) && near(num(&f, "area"), 2400.0), "{f}");
    let bends = f["bends"].as_array().unwrap();
    assert!(bends.len() == 1 && near(num(&bends[0], "angle"), FRAC_PI_2) && near(num(&bends[0], "radius"), R), "{f}");
    // The bend's centre line is the line that was drawn: 40 from one end of the sheet, 20
    // from the other, right across it.
    let line = centre_line(&bends[0]);
    let across = [line.0.0, line.0.1, line.1.0, line.1.1];
    assert!(across.iter().filter(|c| near(**c, 20.0) || near(**c, 40.0)).count() >= 2, "{line:?}");

    // The line as the start of the bend: all 20 to the right stays. As its end: all 40 folds.
    update(&mut s, &mut k, fold, json!({ "location": "start" }));
    let top = |s: &mut Session, k: &mut OcctKernel| flats(s, k).into_iter().filter(|f| f.0.near(Vec3::Z, 1e-9)).map(|f| f.2).fold(0.0, f64::max);
    assert!(near(top(&mut s, &mut k), 20.0 * 40.0), "{}", top(&mut s, &mut k));
    assert!(near(volume(&mut s, &mut k), whole));
    update(&mut s, &mut k, fold, json!({ "location": "end" }));
    assert!(near(top(&mut s, &mut k), (20.0 - ba) * 40.0), "{}", top(&mut s, &mut k));
    let stands = flats(&mut s, &mut k).into_iter().filter(|f| f.0.near(Vec3::X, 1e-9)).map(|f| f.2).fold(0.0, f64::max);
    assert!(near(stands, 40.0 * 40.0), "{stands}");

    // The other side folds: the 20 stands up, at the far side of the bend, looking back along -X.
    update(&mut s, &mut k, fold, json!({ "location": "centerline", "flip_side": true }));
    assert!(near(volume(&mut s, &mut k), whole));
    let inner: Vec<_> = flats(&mut s, &mut k).into_iter().filter(|f| f.0.near(-Vec3::X, 1e-9) && near(f.2, (20.0 - ba / 2.0) * 40.0)).collect();
    assert!(inner.len() == 1 && near(inner[0].1.x, 40.0 - ba / 2.0 + R), "{inner:?}");
    // Folded down, what folds hangs below the plate: its face that was the top looks along +X
    // from 5 beyond where the bend starts.
    update(&mut s, &mut k, fold, json!({ "flip_direction": true }));
    assert!(near(volume(&mut s, &mut k), whole));
    let outer: Vec<_> = flats(&mut s, &mut k).into_iter().filter(|f| f.0.near(Vec3::X, 1e-9) && near(f.2, (20.0 - ba / 2.0) * 40.0)).collect();
    assert!(outer.len() == 1 && near(outer[0].1.x, 40.0 - ba / 2.0 + R + T) && outer[0].1.z < 0.0, "{outer:?}");
    // 45 degrees, a radius of its own: the bend is as wide as its own allowance.
    let eighth = std::f64::consts::FRAC_PI_4;
    update(&mut s, &mut k, fold, json!({ "angle": eighth, "radius": 1.0, "flip_direction": false, "flip_side": false }));
    let own = eighth * (1.0 + K * T);
    let leaning = (60.0 - own) * 40.0 * T + eighth / 2.0 * (9.0 - 1.0) * 40.0;
    assert!(near(volume(&mut s, &mut k), leaning), "{} {leaning}", volume(&mut s, &mut k));
    let f = flat(&mut s, &mut k);
    assert!(near(sides(&f).0, 60.0) && near(num(&f["bends"][0], "angle"), eighth) && near(num(&f["bends"][0], "radius"), 1.0), "{f}");

    // Drawn on the underside it is the same fold seen from below: up is then down.
    let (mut below, mut kb, _) = folded(false, json!({}));
    assert!(near(volume(&mut below, &mut kb), whole));
    let hangs = flats(&mut below, &mut kb).into_iter().filter(|f| near(f.0.x.abs(), 1.0) && f.2 > 500.0).map(|f| f.1.z).fold(0.0, f64::min);
    assert!(hangs < -10.0, "what folds goes down from a sketch on the underside: {hangs}");
}

#[test]
fn a_fold_carries_what_is_on_the_sheet_beyond_it_and_says_what_it_cannot_fold() {
    // A flange on the plate's left end, then a fold along a line 40 along: the flange goes
    // with the piece that folds, and laid flat the part is what it was before the fold.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
    let left = rim(&mut s, &mut k, face, "end", lines[3]);
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [left], "height": 12 }));
    let before = flat(&mut s, &mut k);
    let was = volume(&mut s, &mut k);
    let (sk, to) = sketch_on(&mut s, &mut k, face, true);
    let ((x1, y1), (x2, y2)) = (to(40.0, 0.0), to(40.0, 40.0));
    let line = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": x1, "y1": y1, "x2": x2, "y2": y2 }));
    run(&mut s, &mut k, "sheet.fold", json!({ "sketch": sk, "line": line["line"], "flip_direction": true }));
    let ba = allowance(FRAC_PI_2);
    assert!(near(volume(&mut s, &mut k), was - ba * 40.0 * T + bend_volume(FRAC_PI_2, 40.0)), "{}", volume(&mut s, &mut k));
    assert_eq!(solids(&mut s, &mut k), (1, 1, true));
    let after = flat(&mut s, &mut k);
    let long = |f: &Value| num(f, "length").max(num(f, "width"));
    assert!(near(long(&after), long(&before)), "{} {}", long(&after), long(&before));
    for key in ["area", "cut_length"] {
        assert!(near(num(&after, key), num(&before, key)), "{key}: {} {}", num(&after, key), num(&before, key));
    }
    assert_eq!(after["bends"].as_array().unwrap().len(), 2);
    // The flange's wall is the flange's still, wherever it went.
    let flange = s.document().features().iter().find(|f| f.name == "Flange1").unwrap().id.0;
    let faces = run(&mut s, &mut k, "model.faces", json!({}));
    assert!(faces["faces"].as_array().unwrap().iter().any(|f| f["name"]["feature"] == flange), "{faces}");

    // A line that stops short of the face's edges cuts nothing in two.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, _) = plate(&mut s, &mut k, 60.0, 40.0);
    let (sk, to) = sketch_on(&mut s, &mut k, face, true);
    let ((x1, y1), (x2, y2)) = (to(40.0, 5.0), to(40.0, 35.0));
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": x1, "y1": y1, "x2": x2, "y2": y2 }));
    run(&mut s, &mut k, "sheet.fold", json!({ "sketch": sk }));
    assert!(failure(&mut s, &mut k).contains("does not cut the face in two"));
    run(&mut s, &mut k, "edit.undo", json!({}));
    // Two lines and no word of which; no line at all; an angle that is none.
    let ((x1, y1), (x2, y2)) = (to(20.0, 0.0), to(20.0, 40.0));
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": x1, "y1": y1, "x2": x2, "y2": y2 }));
    run(&mut s, &mut k, "sheet.fold", json!({ "sketch": sk }));
    assert!(failure(&mut s, &mut k).contains("more than one line"));
    run(&mut s, &mut k, "edit.undo", json!({}));
    let e = refused(&mut s, &mut k, "sheet.fold", json!({ "sketch": sk, "angle": 0 }));
    assert!(e.contains("from 1 to 179 degrees"), "{e}");
    let empty = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    run(&mut s, &mut k, "sheet.fold", json!({ "sketch": empty }));
    assert!(failure(&mut s, &mut k).contains("no line of its own"));
    run(&mut s, &mut k, "edit.undo", json!({}));
    // A sketch that is on no side of the sheet.
    let off = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" })), "feature");
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": off, "x1": 40, "y1": -5, "x2": 40, "y2": 5 }));
    run(&mut s, &mut k, "sheet.fold", json!({ "sketch": off }));
    assert!(failure(&mut s, &mut k).contains("must be on a flat face of the sheet"));
}

/// A 60 x 40 plate and, in a sketch on the XZ plane (square to the plate's right and left
/// edges, at their front ends), an open profile through the given points of the part's X and Z.
/// Returns the plate's face feature, its sketch lines and the profile's sketch.
fn plate_and_profile(points: &[(f64, f64)]) -> (Session, OcctKernel, u64, Vec<u64>, u64) {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" })), "feature");
    let frame = s.regen(&mut k).sketch_frames[&tenon_model::FeatureId(sk as u32)];
    let at = |(x, z): (f64, f64)| {
        let p = frame.to_local(Vec3::new(x, 0.0, z));
        (p.x, p.y)
    };
    let mut last: Option<Value> = None;
    for pair in points.windows(2) {
        let ((x1, y1), (x2, y2)) = (at(pair[0]), at(pair[1]));
        let line = match &last {
            Some(end) => run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "start": end, "x2": x2, "y2": y2 })),
            None => run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": x1, "y1": y1, "x2": x2, "y2": y2 })),
        };
        last = Some(line["end"].clone());
    }
    (s, k, face, lines, sk)
}

#[test]
fn a_contour_flange_runs_along_an_edge_of_the_sheet_from_a_profile_that_starts_there() {
    let ba = allowance(FRAC_PI_2);
    // The profile starts at the plate's upper right edge, goes 15 straight up and then 15
    // out. Along the edge it is a wall and a lip: the upper side of the plate runs on up the
    // wall's inside (a bend of inside radius 3 at the edge, for which the plate is cut back by
    // 3) and over the top of the lip (a bend the other way, the profile its outside).
    let (mut s, mut k, face, lines, sk) = plate_and_profile(&[(60.0, 2.0), (60.0, 17.0), (75.0, 17.0)]);
    let right = rim(&mut s, &mut k, face, "end", lines[1]);
    let cf = id(&run(&mut s, &mut k, "sheet.contour_flange", json!({ "sketch": sk, "edges": [right.clone()] })), "feature");
    let bend = bend_volume(FRAC_PI_2, 1.0);
    let strip = |width: f64| -3.0 * T * width + (2.0 * bend + (7.0 + 10.0) * T) * width;
    assert!(near(volume(&mut s, &mut k), 4800.0 + strip(40.0)), "{} {}", volume(&mut s, &mut k), 4800.0 + strip(40.0));
    assert_eq!(solids(&mut s, &mut k), (1, 1, true));
    // The wall's inside is where the profile's first line is (x = 60), 7 of it flat; the top
    // of the lip is where its second line is (z = 17), 10 of it flat.
    let all = flats(&mut s, &mut k);
    assert!(all.iter().any(|f| f.0.near(-Vec3::X, 1e-9) && near(f.1.x, 60.0) && near(f.2, 7.0 * 40.0)), "{all:?}");
    assert!(all.iter().any(|f| f.0.near(Vec3::Z, 1e-9) && near(f.1.z, 17.0) && near(f.2, 10.0 * 40.0)), "{all:?}");
    // It lays flat with the plate: 57 of plate, a bend, 7, a bend, 10.
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), 57.0 + ba + 7.0 + ba + 10.0) && near(num(&f, "width"), 40.0) && f["bends"].as_array().unwrap().len() == 2, "{f}");

    // 20 wide in the middle of the edge: the plate is cut back only there.
    update(&mut s, &mut k, cf, json!({ "width": { "centered": { "width": 20.0 } } }));
    assert!(near(volume(&mut s, &mut k), 4800.0 + strip(20.0)), "{}", volume(&mut s, &mut k));
    // Along the left edge too: the same profile seen from that edge, a mirror image of the first.
    let left = rim(&mut s, &mut k, face, "end", lines[3]);
    update(&mut s, &mut k, cf, json!({ "width": "edge", "edges": [right.clone(), left] }));
    assert!(near(volume(&mut s, &mut k), 4800.0 + 2.0 * strip(40.0)), "{}", volume(&mut s, &mut k));
    let all = flats(&mut s, &mut k);
    assert!(all.iter().any(|f| f.0.near(Vec3::X, 1e-9) && near(f.1.x, 0.0) && near(f.2, 7.0 * 40.0)), "the left wall's inside: {all:?}");
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), 54.0 + 2.0 * (ba + 7.0 + ba + 10.0)) && f["bends"].as_array().unwrap().len() == 4, "{f}");

    // A profile that runs straight on out of the plate before it turns up: nothing is cut
    // back, and what runs straight on is the plate (its top one face, 72 long to the bend).
    let (mut s, mut k, face, lines, sk) = plate_and_profile(&[(60.0, 2.0), (75.0, 2.0), (75.0, 14.0)]);
    let right = rim(&mut s, &mut k, face, "end", lines[1]);
    run(&mut s, &mut k, "sheet.contour_flange", json!({ "sketch": sk, "edges": [right] }));
    assert!(near(volume(&mut s, &mut k), 4800.0 + (12.0 * T + bend + 9.0 * T) * 40.0), "{}", volume(&mut s, &mut k));
    let top: Vec<_> = flats(&mut s, &mut k).into_iter().filter(|f| f.0.near(Vec3::Z, 1e-9) && near(f.1.z, 2.0)).collect();
    assert!(top.len() == 1 && near(top[0].2, 72.0 * 40.0), "{top:?}");
    let named = run(&mut s, &mut k, "model.face_ref", json!({ "at": [70, 20, 2] }));
    assert!(named["origin"]["type"] == "cap" && named["origin"]["feature"] == face, "{named}");
}

#[test]
fn a_contour_flange_along_an_edge_says_what_its_profile_must_be() {
    // A profile that does not start at the edge.
    let (mut s, mut k, face, lines, sk) = plate_and_profile(&[(62.0, 2.0), (62.0, 17.0)]);
    let right = rim(&mut s, &mut k, face, "end", lines[1]);
    run(&mut s, &mut k, "sheet.contour_flange", json!({ "sketch": sk, "edges": [right.clone()] }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("does not start at the edge") && e.contains("2 mm"), "{e}");
    // A sketch that is not square to the edge: the front edge runs along X, in the sketch's plane.
    run(&mut s, &mut k, "edit.undo", json!({}));
    let front = rim(&mut s, &mut k, face, "end", lines[0]);
    run(&mut s, &mut k, "sheet.contour_flange", json!({ "sketch": sk, "edges": [front] }));
    assert!(failure(&mut s, &mut k).contains("not square to the edge"));
    run(&mut s, &mut k, "edit.undo", json!({}));
    // Wider than the edge (with a profile that does start at it); and without edges a
    // distance is still asked for.
    let (mut s, mut k, face, lines, sk) = plate_and_profile(&[(60.0, 2.0), (60.0, 17.0)]);
    let right = rim(&mut s, &mut k, face, "end", lines[1]);
    run(&mut s, &mut k, "sheet.contour_flange", json!({ "sketch": sk, "edges": [right], "width": 50 }));
    assert!(failure(&mut s, &mut k).contains("wider than its edge"));
    let e = refused(&mut s, &mut k, "sheet.contour_flange", json!({ "sketch": sk }));
    assert!(e.contains("distance"), "{e}");
}

/// As `near`, for what is made of curves through points: to a part in a hundred thousand.
fn about(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-5 * b.abs().max(1.0)
}

/// As `near`, for an area of the flat pattern whose outline has such curves in it: the flat
/// pattern has them as chains of short lines, each within 0.005 of the curve.
fn roughly(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-3 * b.abs().max(1.0)
}

/// A sketch on a work plane, and where the part's point `p` is seen in it (looking along the
/// sketch's normal).
fn sketch_over(s: &mut Session, k: &mut OcctKernel, plane: Value) -> (u64, impl Fn(Vec3) -> (f64, f64) + use<>) {
    let wp = id(&run(s, k, "work.plane", plane), "feature");
    let sk = id(&run(s, k, "sketch.create", json!({ "work_plane": wp })), "feature");
    let frame = s.regen(k).sketch_frames[&tenon_model::FeatureId(sk as u32)];
    (sk, move |p: Vec3| {
        let at = frame.to_local(p);
        (at.x, at.y)
    })
}

/// A circle of radius `r` in sketch `sk` where the part's point `p` is seen in it: its centre
/// point.
fn circle_over(s: &mut Session, k: &mut OcctKernel, sk: u64, to: &impl Fn(Vec3) -> (f64, f64), p: Vec3, r: f64) -> u64 {
    let (x, y) = to(p);
    id(&run(s, k, "sketch.circle", json!({ "sketch": sk, "cx": x, "cy": y, "r": r })), "center")
}

/// How wide a circle of radius `r` is where a bend of inside radius `R` is seen under it from
/// above, summed over the angle the bend turns (a quarter turn): the circle's middle is `c`
/// past the bend line, and where the bend has turned by an angle it is seen `R sin angle` past
/// that line. Times a radius, this is the area of the hole on the bend's cylinder of that radius.
fn seen_on_bend(c: f64, r: f64) -> f64 {
    let steps = 400_000;
    let wide = |turn: f64| 2.0 * (r * r - (R * turn.sin() - c).powi(2)).max(0.0).sqrt();
    let h = FRAC_PI_2 / steps as f64;
    (0..=steps)
        .map(|i| {
            wide(i as f64 * h)
                * if i == 0 || i == steps {
                    1.0
                } else if i % 2 == 1 {
                    4.0
                } else {
                    2.0
                }
        })
        .sum::<f64>()
        * h
        / 3.0
}

#[test]
fn a_cut_normal_is_the_profile_as_it_is_seen_on_the_sheet_with_walls_square_to_it() {
    // Seen square on, from a plane well above the plate: the profile itself, straight through.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    plate(&mut s, &mut k, 60.0, 40.0);
    let before = flat(&mut s, &mut k);
    let (sk, to) = sketch_over(&mut s, &mut k, json!({ "base": "xy", "distance": 30 }));
    circle_over(&mut s, &mut k, sk, &to, Vec3::new(20.0, 20.0, T), 4.0);
    let cut = id(&run(&mut s, &mut k, "sheet.cut_normal", json!({ "sketch": sk })), "feature");
    assert_eq!(s.document().feature(tenon_model::FeatureId(cut as u32)).unwrap().name, "Cut1");
    assert!(near(volume(&mut s, &mut k), 4800.0 - 16.0 * PI * T), "{}", volume(&mut s, &mut k));
    let f = flat(&mut s, &mut k);
    let inner = f["inner"].as_array().unwrap();
    assert!(inner.len() == 1 && inner[0].as_array().unwrap().len() == 1 && inner[0][0]["type"] == "circle", "{}", f["inner"]);
    assert!(near(num(&inner[0][0], "radius"), 4.0) && near(num(&f, "area"), num(&before, "area") - 16.0 * PI), "{f}");
    // Half a millimetre deep from below: a pocket in the underside, which is the side that
    // looks at a sketch under the plate.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    plate(&mut s, &mut k, 60.0, 40.0);
    let (sk, to) = sketch_over(&mut s, &mut k, json!({ "base": "xy", "distance": -10 }));
    circle_over(&mut s, &mut k, sk, &to, Vec3::new(20.0, 20.0, 0.0), 4.0);
    run(&mut s, &mut k, "sheet.cut_normal", json!({ "sketch": sk, "distance": 0.5 }));
    assert!(near(volume(&mut s, &mut k), 4800.0 - 16.0 * PI * 0.5), "{}", volume(&mut s, &mut k));
    let sides = flats(&mut s, &mut k);
    let area = |z: f64, level: f64| sides.iter().find(|f| f.0.near(Vec3::Z * z, 1e-9) && (f.1.z - level).abs() < 1e-9).map(|f| f.2);
    assert!(area(1.0, T).is_some_and(|a| near(a, 2400.0)) && area(-1.0, 0.0).is_some_and(|a| near(a, 2400.0 - 16.0 * PI)), "{sides:?}");
    assert!(area(-1.0, 0.5).is_some_and(|a| near(a, 16.0 * PI)), "the floor of the pocket: {sides:?}");

    // Seen at an angle, from a plane through the X axis turned 30 degrees up from the plate:
    // the circle is seen on the plate as an oval, 4 by 4 / cos 30, and the hole goes through
    // square to the plate (not along the line of sight, as an extrusion would cut it).
    let tilt = 30f64.to_radians();
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    plate(&mut s, &mut k, 60.0, 40.0);
    let (sk, to) = sketch_over(&mut s, &mut k, json!({ "by": "angle", "base": "xy", "axis": "x", "angle": tilt }));
    let centre = circle_over(&mut s, &mut k, sk, &to, Vec3::new(30.0, 20.0, T), 4.0);
    let cut = id(&run(&mut s, &mut k, "sheet.cut_normal", json!({ "sketch": sk })), "feature");
    let oval = 16.0 * PI / tilt.cos();
    // (The kernel sums the volume of a hole with such walls less closely than it does the
    // areas of the faces it is cut in, which say what the hole is.)
    assert!(roughly(4800.0 - volume(&mut s, &mut k), oval * T), "{} {}", 4800.0 - volume(&mut s, &mut k), oval * T);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    // Both sides of the plate lose the same oval, in the same place: what is left of each has
    // its middle where the plate's is, pulled away from the hole by as much.
    let sides = flats(&mut s, &mut k);
    for (normal, level) in [(Vec3::Z, T), (-Vec3::Z, 0.0)] {
        let side = sides.iter().find(|f| f.0.near(normal, 1e-9) && (f.1.z - level).abs() < 1e-9).expect("a side of the plate");
        assert!(about(2400.0 - side.2, oval), "{side:?}");
        assert!((side.1.x - 30.0).abs() < 1e-6 && (side.1.y - 20.0).abs() < 1e-6, "{side:?}");
    }
    let f = flat(&mut s, &mut k);
    assert!(roughly(2400.0 - num(&f, "area"), oval) && f["inner"].as_array().unwrap().len() == 1, "{f}");
    // Its walls are the cut's: at the end of the oval's long side, half way through the sheet.
    let wall = run(&mut s, &mut k, "model.face_ref", json!({ "at": [30, 20.0 + 4.0 / tilt.cos(), 1] }));
    assert!(wall["origin"]["type"] == "from" && wall["origin"]["feature"] == cut, "{wall}");
    // The cut follows its sketch: with the circle dragged to be seen 10 along the plate, the
    // oval is there.
    let (x, y) = to(Vec3::new(40.0, 20.0, T));
    run(&mut s, &mut k, "sketch.drag", json!({ "sketch": sk, "point": centre, "x": x, "y": y }));
    assert!(roughly(4800.0 - volume(&mut s, &mut k), oval * T), "{}", volume(&mut s, &mut k));
    let wall = run(&mut s, &mut k, "model.face_ref", json!({ "at": [40, 20.0 + 4.0 / tilt.cos(), 1] }));
    assert!(wall["origin"]["type"] == "from" && wall["origin"]["feature"] == cut, "{wall}");
}

#[test]
fn a_cut_normal_goes_onto_the_bends_it_is_seen_on_and_leaves_what_is_seen_edge_on() {
    let ba = allowance(FRAC_PI_2);
    let rho = R + K * T;
    let through = ((R + T).powi(2) - R * R) / 2.0;
    // A plate with a wall bent up along its right edge, seen from above. The plate's top is
    // seen as it is; the inside of the bend is seen from x = 55 to 58 (three of radius), all of
    // a quarter turn; the wall is seen edge on.
    let (mut s, mut k, _) = channel(false);
    let whole = volume(&mut s, &mut k);
    let before = flat(&mut s, &mut k);
    // A slot 8 wide from x = 40 to past the wall: 15 of plate, all of the bend, none of the wall.
    let (sk, to) = sketch_over(&mut s, &mut k, json!({ "base": "xy", "distance": 30 }));
    let ((x1, y1), (x2, y2)) = (to(Vec3::new(40.0, 16.0, T)), to(Vec3::new(70.0, 24.0, T)));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": x1, "y1": y1, "x2": x2, "y2": y2 }));
    run(&mut s, &mut k, "sheet.cut_normal", json!({ "sketch": sk }));
    let slot = 8.0 * 15.0 * T + bend_volume(FRAC_PI_2, 8.0);
    assert!(near(volume(&mut s, &mut k), whole - slot), "{} {}", volume(&mut s, &mut k), whole - slot);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    // The wall is whole: 40 by 10 on its outside.
    let outer = flats(&mut s, &mut k).into_iter().find(|f| f.0.near(Vec3::X, 1e-9) && f.1.x > 59.9).expect("the outer face of the wall");
    assert!(near(outer.2, 400.0), "{outer:?}");
    // In the flat pattern the slot is a rectangle from 40 to where the bend ends: four lines.
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "area"), num(&before, "area") - 8.0 * (15.0 + ba)), "{f}");
    let inner = f["inner"].as_array().unwrap();
    assert!(inner.len() == 1 && inner[0].as_array().unwrap().len() == 4 && inner[0].as_array().unwrap().iter().all(|c| c["type"] == "line"), "{f}");

    // A round hole of radius 4 seen from above with its middle on the bend line: half of it
    // is on the plate. The rest is seen on the bend out to x = 58, where the bend is seen more
    // and more edge on, so laid flat the hole is longer there than the circle that was drawn.
    let (mut s, mut k, _) = channel(false);
    let (sk, to) = sketch_over(&mut s, &mut k, json!({ "base": "xy", "distance": 30 }));
    circle_over(&mut s, &mut k, sk, &to, Vec3::new(55.0, 20.0, T), 4.0);
    run(&mut s, &mut k, "sheet.cut_normal", json!({ "sketch": sk }));
    let on_bend = seen_on_bend(0.0, 4.0);
    let hole = 8.0 * PI * T + on_bend * through;
    assert!(about(whole - volume(&mut s, &mut k), hole), "{} {}", whole - volume(&mut s, &mut k), hole);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    let f = flat(&mut s, &mut k);
    let gone = num(&before, "area") - num(&f, "area");
    assert!(roughly(gone, 8.0 * PI + rho * on_bend) && rho * on_bend > 1.2 * 8.0 * PI, "{gone} {}", 8.0 * PI + rho * on_bend);
    // In the flat pattern: half the circle that was drawn, curves from its ends out to the end
    // of the bend, and a straight end there, sqrt 7 either side of the middle.
    let inner = f["inner"].as_array().unwrap();
    assert_eq!(inner.len(), 1, "{f}");
    let kinds: Vec<&str> = inner[0].as_array().unwrap().iter().map(|c| c["type"].as_str().unwrap()).collect();
    assert!(
        kinds.contains(&"arc") && kinds.iter().filter(|k| **k == "line").count() == 1 && kinds.iter().any(|k| *k != "arc" && *k != "line"),
        "{kinds:?}"
    );
    let end = inner[0].as_array().unwrap().iter().find(|c| c["type"] == "line").unwrap();
    let (a, b) = (&end["start"], &end["end"]);
    assert!(near(num(a, "x"), 55.0 + ba) && near(num(b, "x"), 55.0 + ba) && about((num(a, "y") - num(b, "y")).abs(), 2.0 * 7f64.sqrt()), "{end}");

    // A hole of radius 1 that is seen on the bend alone, its middle 1.5 past the bend line.
    let (mut s, mut k, _) = channel(false);
    let (sk, to) = sketch_over(&mut s, &mut k, json!({ "base": "xy", "distance": 30 }));
    circle_over(&mut s, &mut k, sk, &to, Vec3::new(56.5, 20.0, T), 1.0);
    run(&mut s, &mut k, "sheet.cut_normal", json!({ "sketch": sk }));
    let on_bend = seen_on_bend(1.5, 1.0);
    assert!(about(whole - volume(&mut s, &mut k), on_bend * through), "{} {}", whole - volume(&mut s, &mut k), on_bend * through);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    let f = flat(&mut s, &mut k);
    assert!(roughly(num(&before, "area") - num(&f, "area"), rho * on_bend) && f["inner"].as_array().unwrap().len() == 1, "{f}");

    // Seen at 45 degrees into the corner of the channel, from a plane through the Y axis: the
    // top of the plate, the inside of the bend and the inside of the wall are all seen, and a
    // circle of radius 5 whose middle is over the middle of the bend is on all three.
    let (mut s, mut k, _) = channel(false);
    let sides = flats(&mut s, &mut k);
    let (sk, to) = sketch_over(&mut s, &mut k, json!({ "by": "angle", "base": "xy", "axis": "y", "angle": -FRAC_PI_4 }));
    let frame = s.regen(&mut k).sketch_frames[&tenon_model::FeatureId(sk as u32)];
    let middle = Vec3::new(56.0, 20.0, 4.0);
    circle_over(&mut s, &mut k, sk, &to, middle, 5.0);
    let cut = id(&run(&mut s, &mut k, "sheet.cut_normal", json!({ "sketch": sk })), "feature");
    assert!(volume(&mut s, &mut k) < whole - 200.0);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    // Looked at from the sketch, the rim of the hole on the side that is seen is the circle
    // that was drawn, on the plate, round the bend and up the wall: four edges of it, all
    // their ends 5 from the circle's middle. (The rim on the far side is not: the hole goes
    // through square to the sheet, not along the line of sight.)
    let seen = |p: Vec3| {
        let (at, mid) = (frame.to_local(p), frame.to_local(middle));
        ((at.x - mid.x).powi(2) + (at.y - mid.y).powi(2)).sqrt()
    };
    let point = |v: &Value| Vec3::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap());
    let edges = run(&mut s, &mut k, "model.edges", json!({}));
    let rim: Vec<&Value> =
        edges["edges"].as_array().unwrap().iter().filter(|e| e["faces"].as_array().unwrap().iter().any(|f| f["feature"] == cut)).collect();
    let near_side: Vec<&&Value> = rim.iter().filter(|e| [&e["start"], &e["end"]].iter().all(|end| (seen(point(end)) - 5.0).abs() < 1e-6)).collect();
    assert_eq!(near_side.len(), 4, "{rim:?}");
    assert!(rim.iter().any(|e| (seen(point(&e["start"])) - 5.0).abs() > 0.5), "{rim:?}");
    // The view is the same to either side of the bend, so the plate and the wall lose as much.
    let after = flats(&mut s, &mut k);
    let lost = |normal: Vec3, level: f64| {
        let area = |all: &[(Vec3, Vec3, f64)]| {
            all.iter().find(|f| f.0.near(normal, 1e-9) && (f.1.dot(normal) - level).abs() < 1e-9).map(|f| f.2).expect("a side of the sheet")
        };
        area(&sides) - area(&after)
    };
    let (plate_lost, wall_lost) = (lost(Vec3::Z, T), lost(-Vec3::X, -58.0));
    assert!(plate_lost > 20.0 && about(plate_lost, wall_lost), "{plate_lost} {wall_lost}");
    // And the flat pattern has the one hole.
    let f = flat(&mut s, &mut k);
    assert!(f["inner"].as_array().unwrap().len() == 1 && num(&f, "area") < num(&before, "area") - 100.0, "{f}");
}

#[test]
fn a_cut_normal_says_what_it_cannot_see_or_cut() {
    let (mut s, mut k, _) = channel(false);
    // Nothing of the sheet under the profile.
    let (sk, to) = sketch_over(&mut s, &mut k, json!({ "base": "xy", "distance": 30 }));
    let centre = circle_over(&mut s, &mut k, sk, &to, Vec3::new(100.0, 100.0, T), 0.5);
    let cut = id(&run(&mut s, &mut k, "sheet.cut_normal", json!({ "sketch": sk })), "feature");
    let e = failure(&mut s, &mut k);
    assert!(e.contains("not over the sheet") && e.contains("no face and no bend of the sheet is seen under it"), "{e}");
    // Over the top of the wall alone, which is seen edge on from above: nothing to cut either.
    let (x, y) = to(Vec3::new(59.0, 20.0, T));
    run(&mut s, &mut k, "sketch.drag", json!({ "sketch": sk, "point": centre, "x": x, "y": y }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("not over the sheet"), "{e}");
    // A depth of nothing.
    let e = refused(&mut s, &mut k, "sheet.cut_normal", json!({ "sketch": sk, "distance": 0 }));
    assert!(e.contains("more than 0"), "{e}");
    // Moved over the plate it cuts, and the part follows.
    let whole = 4800.0 + bend_volume(FRAC_PI_2, 40.0) + 40.0 * 10.0 * T - 5.0 * 40.0 * T;
    let (x, y) = to(Vec3::new(20.0, 20.0, T));
    run(&mut s, &mut k, "sketch.drag", json!({ "sketch": sk, "point": centre, "x": x, "y": y }));
    assert!(near(volume(&mut s, &mut k), whole - 0.25 * PI * T), "{} {}", volume(&mut s, &mut k), whole - 0.25 * PI * T);
    let kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(cut as u32)).unwrap().kind).unwrap();
    assert_eq!(kind, json!({ "type": "cut_normal", "sketch": sk, "regions": "default" }), "what a file says of it");

    // A sketch that sees the sheet edge on: no side of it to cut.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    plate(&mut s, &mut k, 60.0, 40.0);
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" })), "feature");
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 4 }));
    run(&mut s, &mut k, "sheet.cut_normal", json!({ "sketch": sk }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("No side of the sheet looks at the sketch") && e.contains("edge on"), "{e}");

    // Not a sheet metal part.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 4 }));
    let e = refused(&mut s, &mut k, "sheet.cut_normal", json!({ "sketch": sk }));
    assert!(e.contains("not a sheet metal part"), "{e}");
}

/// A box 60 by 40 by 20 with a corner at the origin, shelled to the sheet's thickness with its
/// top open: a floor and four walls that meet in square corners with no seam. Its volume.
fn shelled_box() -> (Session, OcctKernel, f64) {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": T, "bend_radius": R }));
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 40 }));
    let block = id(&run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 20 })), "feature");
    let top = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": block, "end": "end" } }));
    run(&mut s, &mut k, "model.shell", json!({ "remove": [top], "thickness": T }));
    let whole = volume(&mut s, &mut k);
    assert!(near(whole, 60.0 * 40.0 * 20.0 - 56.0 * 36.0 * 18.0), "{whole}");
    (s, k, whole)
}

#[test]
fn a_corner_rip_opens_the_square_corner_of_a_shelled_box_along_its_edge() {
    // Symmetric, with a gap of 1: each wall stops where its inner face is 1 from the other's,
    // which is 2 + 1 / sqrt 2 from the outside of the corner, and the corner between them goes.
    let reach = T + 1.0 / 2f64.sqrt();
    let walls = reach * reach - (reach - T) * (reach - T);
    // Along the outside edge, which runs down through the floor: the floor loses its corner.
    let (mut s, mut k, whole) = shelled_box();
    let edge = edge_at(&mut s, &mut k, Vec3::new(0.0, 0.0, 10.0));
    let rip = id(&run(&mut s, &mut k, "sheet.corner_rip", json!({ "edges": [edge], "gap": 1 })), "feature");
    assert_eq!(s.document().feature(tenon_model::FeatureId(rip as u32)).unwrap().name, "Corner Rip1");
    let gone = walls * 18.0 + reach * reach * T;
    assert!(near(volume(&mut s, &mut k), whole - gone), "{} {}", volume(&mut s, &mut k), whole - gone);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    // The ends of the two walls, square to them where the walls stop: 18 high above the floor,
    // and one face with the side of the notch in the floor below.
    let sides = flats(&mut s, &mut k);
    for (normal, across) in [(-Vec3::X, Vec3::X), (-Vec3::Y, Vec3::Y)] {
        let end = sides
            .iter()
            .find(|f| f.0.near(normal, 1e-9) && (f.1.dot(across) - reach).abs() < 1e-9)
            .unwrap_or_else(|| panic!("the end of a wall: {sides:?}"));
        assert!(near(end.2, T * 18.0 + reach * T), "{end:?}");
    }
    let end = run(&mut s, &mut k, "model.face_ref", json!({ "at": [reach, 1, 10] }));
    assert!(end["origin"]["type"] == "from" && end["origin"]["feature"] == rip, "{end}");
    // What a file says of it.
    let kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(rip as u32)).unwrap().kind).unwrap();
    assert!(
        kind["type"] == "corner_rip" && kind["gap"] == 1.0 && kind["edges"].as_array().unwrap().len() == 1 && kind.get("seam").is_none(),
        "{kind}"
    );
    // The gap follows the rule where the rip has none of its own.
    run(&mut s, &mut k, "sheet.defaults", json!({ "gap": 2 }));
    update(&mut s, &mut k, rip, json!({ "gap": null }));
    let wider = T + 2.0 / 2f64.sqrt();
    let gone = (wider * wider - (wider - T) * (wider - T)) * 18.0 + wider * wider * T;
    assert!(near(volume(&mut s, &mut k), whole - gone), "{} {}", volume(&mut s, &mut k), whole - gone);

    // Along the inside edge, which starts at the floor: the walls are opened and the floor is whole.
    let (mut s, mut k, whole) = shelled_box();
    let edge = edge_at(&mut s, &mut k, Vec3::new(T, T, 11.0));
    run(&mut s, &mut k, "sheet.corner_rip", json!({ "edges": [edge], "gap": 1 }));
    assert!(near(volume(&mut s, &mut k), whole - walls * 18.0), "{} {}", volume(&mut s, &mut k), whole - walls * 18.0);
    let floor = flats(&mut s, &mut k).into_iter().find(|f| f.0.near(-Vec3::Z, 1e-9)).expect("the underside");
    assert!(near(floor.2, 2400.0), "{floor:?}");

    // All four corners in one rip: four walls on a floor, each clear of the next.
    let (mut s, mut k, whole) = shelled_box();
    let edges: Vec<Value> =
        [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)].iter().map(|(x, y)| edge_at(&mut s, &mut k, Vec3::new(*x, *y, 10.0))).collect();
    run(&mut s, &mut k, "sheet.corner_rip", json!({ "edges": edges, "gap": 1 }));
    let gone = 4.0 * (walls * 18.0 + reach * reach * T);
    assert!(near(volume(&mut s, &mut k), whole - gone), "{} {}", volume(&mut s, &mut k), whole - gone);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
}

#[test]
fn a_corner_rip_lets_one_wall_keep_the_corner_and_says_what_is_no_corner() {
    // Overlap: one wall keeps the corner whole and the other stops the gap short of its inner
    // face, a slit one thickness long. Reverse overlap: the other wall keeps it.
    let ends = |seam: &str, overlap: f64| {
        let (mut s, mut k, whole) = shelled_box();
        let edge = edge_at(&mut s, &mut k, Vec3::new(0.0, 0.0, 10.0));
        let rip = id(&run(&mut s, &mut k, "sheet.corner_rip", json!({ "edges": [edge], "seam": seam, "overlap": overlap, "gap": 1 })), "feature");
        let gone = whole - volume(&mut s, &mut k);
        let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
        assert!(body["valid"] == true && body["solids"] == 1, "{seam}: {body}");
        // Which wall was cut short: the one along X ends in a face that looks back along X,
        // 3 from the outside of the corner (its thickness and the gap).
        let x_wall_cut = flats(&mut s, &mut k).iter().any(|f| f.0.near(-Vec3::X, 1e-9) && (f.1.x - (T + 1.0)).abs() < 1e-9);
        let y_wall_cut = flats(&mut s, &mut k).iter().any(|f| f.0.near(-Vec3::Y, 1e-9) && (f.1.y - (T + 1.0)).abs() < 1e-9);
        let _ = rip;
        (gone, x_wall_cut, y_wall_cut)
    };
    let (gone, x_cut, y_cut) = ends("overlap", 1.0);
    assert!(near(gone, T * 1.0 * 20.0) && x_cut != y_cut, "{gone} {x_cut} {y_cut}");
    let (back, x_back, y_back) = ends("reverse_overlap", 1.0);
    assert!(near(back, gone) && x_back == y_cut && y_back == x_cut, "{back} {x_back} {y_back}");
    // Half an overlap: the wall that keeps the corner covers half the other's thickness, so
    // it loses the other half of the corner as well.
    let (half, ..) = ends("overlap", 0.5);
    assert!(near(half, T * 1.0 * 20.0 + T * 1.0 * 20.0), "{half}");
    // No overlap at all: it stops at the other's inner face, and the whole corner goes.
    let (none, ..) = ends("overlap", 0.0);
    assert!(near(none, T * 1.0 * 20.0 + T * T * 20.0), "{none}");

    let (mut s, mut k, _) = shelled_box();
    // An edge of the rim is no corner between two walls.
    let rim = edge_at(&mut s, &mut k, Vec3::new(30.0, 0.0, 20.0));
    run(&mut s, &mut k, "sheet.corner_rip", json!({ "edges": [rim], "gap": 1 }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("not both flat sides of the sheet") && e.contains("rim"), "{e}");
    assert!(s.undo());
    // No gap, nothing opened.
    let edge = edge_at(&mut s, &mut k, Vec3::new(0.0, 0.0, 10.0));
    run(&mut s, &mut k, "sheet.corner_rip", json!({ "edges": [edge.clone()], "gap": 0 }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("no gap leaves the corner as it is"), "{e}");
    assert!(s.undo());
    // A relief belongs to a corner with bends.
    let e = refused(&mut s, &mut k, "sheet.corner_rip", json!({ "edges": [edge.clone()], "relief": "round" }));
    assert!(e.contains("no relief"), "{e}");
    let e = refused(&mut s, &mut k, "sheet.corner_rip", json!({ "edges": [edge.clone()], "overlap": 2 }));
    assert!(e.contains("from 0 to 1"), "{e}");
    // The same edge twice.
    run(&mut s, &mut k, "sheet.corner_rip", json!({ "edges": [edge.clone(), edge], "gap": 1 }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("picked twice"), "{e}");

    // A part that is not sheet metal.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 40 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 20 }));
    let edge = edge_at(&mut s, &mut k, Vec3::new(0.0, 0.0, 10.0));
    let e = refused(&mut s, &mut k, "sheet.corner_rip", json!({ "edges": [edge] }));
    assert!(e.contains("not a sheet metal part"), "{e}");
}
