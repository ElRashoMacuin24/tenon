//! Sheet metal (M8): faces, flanges with their reliefs, and the flat pattern, against bend
//! allowances worked out by hand.
//!
//! Everywhere here the sheet is 2 mm thick, bends have an inside radius of 3 mm and the K
//! factor is 0.44 unless a test says otherwise. A bend that turns the sheet by an angle `a` is
//! `a * (3 + 0.44 * 2)` wide in the flat: 1.94 pi for a quarter turn.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};

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
fn a_tray_has_its_corners_relieved_and_lies_flat_as_a_cross() {
    let ba = allowance(FRAC_PI_2);
    // (corner relief, what it takes out of the sheet at each corner)
    for (corner, hole) in [("trim", 0.0), ("square", 64.0), ("round", 16.0 + 12.0 * PI)] {
        let (mut s, mut k) = (Session::default(), OcctKernel::new());
        let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
        run(&mut s, &mut k, "sheet.defaults", json!({ "corner": corner }));
        // All four edges in one flange, 15 high: the walls of a tray. Each bend is cut 5 into
        // the sheet, so the floor is 50 x 30 and each wall starts where its neighbour's bend
        // line crosses its own.
        let edges: Vec<Value> = lines.iter().map(|l| rim(&mut s, &mut k, face, "end", *l)).collect();
        run(&mut s, &mut k, "sheet.flange", json!({ "edges": edges, "height": 15 }));
        let walls = 2.0 * (50.0 + 30.0);
        let folded = (1500.0 - 4.0 * hole) * T + bend_volume(FRAC_PI_2, walls) + 10.0 * T * walls;
        assert!(near(volume(&mut s, &mut k), folded), "{corner}: {} {folded}", volume(&mut s, &mut k));
        let f = flat(&mut s, &mut k);
        assert!(near(num(&f, "length"), 70.0 + 2.0 * ba) && near(num(&f, "width"), 50.0 + 2.0 * ba), "{corner}: {f}");
        assert!(near(num(&f, "area"), 1500.0 - 4.0 * hole + walls * (ba + 10.0)), "{corner}: {f}");
        let bends = f["bends"].as_array().unwrap();
        assert!(bends.len() == 4 && bends.iter().all(|b| b["up"] == true && near(num(b, "angle"), FRAC_PI_2)), "{corner}: {f}");
        let outer = f["outer"].as_array().unwrap();
        let kinds = |t: &str| outer.iter().filter(|c| c["type"] == t).count();
        // The outline is a cross, three sides to each arm, whatever the relief: a relief is a
        // hole that touches the outline at the corner where the two bend lines cross.
        assert_eq!((outer.len(), kinds("line")), (12, 12), "{corner}: {f}");
        let reliefs = f["inner"].as_array().unwrap();
        let shapes: Vec<Vec<&str>> = reliefs.iter().map(|l| l.as_array().unwrap().iter().map(|c| c["type"].as_str().unwrap()).collect()).collect();
        match corner {
            "trim" => {
                assert!(reliefs.is_empty(), "{f}");
                assert!(near(num(&f, "cut_length"), walls + 8.0 * (ba + 10.0)), "{f}");
            }
            // Each corner notched 8 along both bend lines.
            "square" => assert_eq!(shapes, vec![vec!["line"; 4]; 4], "{f}"),
            // Each corner a hole 8 across that touches both bend lines, opened to the corner.
            _ => {
                assert_eq!(shapes.len(), 4, "{f}");
                for (relief, shape) in reliefs.iter().zip(&shapes) {
                    let mut kinds = shape.clone();
                    kinds.sort_unstable();
                    assert_eq!(kinds, vec!["arc", "line", "line"], "{f}");
                    let arc = relief.as_array().unwrap().iter().find(|c| c["type"] == "arc").unwrap();
                    assert!(near(num(arc, "radius"), 4.0), "{arc}");
                }
            }
        }
    }
    // The relief's size is the rule's too.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
    run(&mut s, &mut k, "sheet.defaults", json!({ "corner": "square", "corner_size": "Thickness * 2" }));
    let edges: Vec<Value> = lines.iter().map(|l| rim(&mut s, &mut k, face, "end", *l)).collect();
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": edges, "height": 15 }));
    assert!(near(num(&flat(&mut s, &mut k), "area"), 1500.0 - 4.0 * 16.0 + 160.0 * (ba + 10.0)));
    // Two walls only, on edges that meet: one corner, relieved; the other ends run to the ends
    // of their edges.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
    run(&mut s, &mut k, "sheet.defaults", json!({ "corner": "square" }));
    let edges = [rim(&mut s, &mut k, face, "end", lines[0]), rim(&mut s, &mut k, face, "end", lines[1])];
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": edges, "height": 15 }));
    let folded = (55.0 * 35.0 - 64.0) * T + (bend_volume(FRAC_PI_2, 1.0) + 10.0 * T) * (55.0 + 35.0);
    assert!(near(volume(&mut s, &mut k), folded), "{} {folded}", volume(&mut s, &mut k));
    assert!(near(num(&flat(&mut s, &mut k), "area"), 55.0 * 35.0 - 64.0 + 90.0 * (ba + 10.0)));
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
