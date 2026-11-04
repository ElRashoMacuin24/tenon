//! Sheet metal that makes sheet of its own or stamps it: Contour Roll, Lofted Flange and Punch,
//! against figures worked out by hand.
//!
//! The sheet here is 2 mm thick, bends have an inside radius of 3 mm and the K factor is 0.44
//! unless a test says otherwise.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

const T: f64 = 2.0;
const R: f64 = 3.0;
const K: f64 = 0.44;

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

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// The part regenerates without an error; its volume.
fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{r}");
    run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

fn failure(s: &mut Session, k: &mut OcctKernel) -> String {
    let r = run(s, k, "model.regenerate", json!({}));
    r["error"]["message"].as_str().unwrap_or_else(|| panic!("no feature failed: {r}")).to_owned()
}

fn flat(s: &mut Session, k: &mut OcctKernel) -> Value {
    run(s, k, "sheet.flat", json!({ "curves": true }))
}

fn start() -> (Session, OcctKernel) {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": T, "bend_radius": R }));
    (s, k)
}

/// A sketch on the XZ plane holding a chain of lines through `points`.
fn lines_on(s: &mut Session, k: &mut OcctKernel, plane: &str, points: &[(f64, f64)]) -> u64 {
    let sk = id(&run(s, k, "sketch.create", json!({ "plane": plane })), "feature");
    let mut last: Option<Value> = None;
    for w in points.windows(2) {
        let mut p = json!({ "sketch": sk, "x1": w[0].0, "y1": w[0].1, "x2": w[1].0, "y2": w[1].1 });
        if let Some(end) = &last {
            p["start"] = end.clone();
        }
        last = Some(run(s, k, "sketch.line", p)["end"].clone());
    }
    sk
}

/// The radii of the arcs in a flat pattern's outline.
fn arcs(f: &Value) -> Vec<f64> {
    f["outer"].as_array().unwrap().iter().filter(|c| c["type"] == "arc").map(|c| c["radius"].as_f64().unwrap()).collect()
}

/// A straight ring: one line along the axis. Rolled through an angle it is a rectangle in the
/// flat, as long as its neutral surface goes round and as wide as the line is long.
#[test]
fn a_ring_unrolls_to_a_rectangle_as_long_as_its_neutral_surface() {
    let (mut s, mut k) = start();
    // On XZ: x is across, y (of the sketch) is along Z. A line 40 long, 50 from the Z axis.
    let sk = lines_on(&mut s, &mut k, "xz", &[(50.0, 0.0), (50.0, 40.0)]);
    let turn = 1.5 * PI;
    let f = id(&run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": turn, "position": "flipped" })), "feature");
    // Which side the thickness went decides the radii: 50 to 52 or 48 to 50. A ring between
    // radii a and b, 40 long, turned through the angle: (b^2 - a^2) / 2 x angle x 40.
    let v = volume(&mut s, &mut k);
    let ring = |a: f64, b: f64| (b * b - a * a) / 2.0 * turn * 40.0;
    let (outside, inside) = (ring(50.0, 52.0), ring(48.0, 50.0));
    assert!(close(v, outside, 1e-5) || close(v, inside, 1e-5), "{v}: {outside} or {inside}");
    let inner_radius = if close(v, outside, 1e-5) { 50.0 } else { 48.0 };
    let p = flat(&mut s, &mut k);
    let along = turn * (inner_radius + K * T);
    let (a, b) = (num(&p, "length"), num(&p, "width"));
    assert!(close(a.max(b), along, 1e-6) && close(a.min(b), 40.0, 1e-6), "{p}");
    assert!(close(num(&p, "area"), along * 40.0, 1e-6), "{p}");
    assert_eq!(p["notes"], Value::Null, "a ring unrolls exactly: {p}");
    // It is one long bend, of that radius, through that angle.
    let bends = p["bends"].as_array().unwrap();
    assert_eq!(bends.len(), 1, "{p}");
    assert!(close(num(&bends[0], "radius"), inner_radius, 1e-6) && close(num(&bends[0], "angle"), turn, 1e-6), "{p}");
    // The feature follows its definition, and its sketch.
    let mut kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(f as u32)).unwrap().kind).unwrap();
    kind["position"] = json!("default");
    kind["angle"] = json!(PI);
    run(&mut s, &mut k, "feature.update", json!({ "feature": f, "kind": kind }));
    let v = volume(&mut s, &mut k);
    let half = |a: f64, b: f64| (b * b - a * a) / 2.0 * PI * 40.0;
    assert!(close(v, half(50.0, 52.0), 1e-5) || close(v, half(48.0, 50.0), 1e-5), "{v}");
}

/// A straight line at a slant is a cone: unrolled about the point it comes to, it is a sector of
/// a ring, exactly, whose rims are as long as the neutral surface's circles.
#[test]
fn a_cone_unrolls_to_a_sector_of_a_ring_by_hand() {
    let (mut s, mut k) = start();
    // A slant of 50: from radius 30 at height 0 to radius 60 at height 40, a half angle whose sine
    // is 30/50 = 0.6 and cosine 0.8.
    let sk = lines_on(&mut s, &mut k, "xz", &[(60.0, 40.0), (30.0, 0.0)]);
    let turn = 2.0;
    run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": turn }));
    // Thick 2 on one side: a section 50 x 2 whose middle is at a mean radius of 45, moved 0.8 by
    // being a thickness to one side of the line. Pappus: section x mean radius x angle.
    let v = volume(&mut s, &mut k);
    let pappus = |mean: f64| 100.0 * mean * turn;
    // (The sheet is on the left of the line as drawn, which runs down and in: towards larger x
    // on the way down... either side is one of the two.)
    let (out, inn) = (pappus(45.0 + 0.8), pappus(45.0 - 0.8));
    assert!(close(v, out, 1e-4) || close(v, inn, 1e-4), "{v}: {out} or {inn}");
    let outer_side = close(v, out, 1e-4);
    // The neutral surface is K x T through from the side nearest the axis. The line drawn is one
    // side of the sheet: the side nearest the axis is the line (the sheet on the outside) or
    // 2 inside it, so the neutral surface's radii are the line's plus or minus.
    let shift = if outer_side { K * T * 0.8 } else { -(T - K * T) * 0.8 };
    let (r1, r2) = (30.0 + shift, 60.0 + shift);
    // Its development: rims of radius r / sin, an angle of turn x sin.
    let (rho1, rho2, phi) = (r1 / 0.6, r2 / 0.6, turn * 0.6);
    let p = flat(&mut s, &mut k);
    assert_eq!(p["notes"], Value::Null, "a cone unrolls exactly: {p}");
    let area = phi / 2.0 * (rho2 * rho2 - rho1 * rho1);
    assert!(close(num(&p, "area"), area, 1e-4), "{p}: {area}");
    let mut rims = arcs(&p);
    rims.sort_by(f64::total_cmp);
    assert_eq!(rims.len(), 2, "{p}");
    assert!(close(rims[0], rho1, 1e-5) && close(rims[1], rho2, 1e-5), "{rims:?} {rho1} {rho2}");
    // The size of the sector, which is less than half a turn: a chord of its outer rim one way,
    // from its inner corners to the top of its outer rim the other.
    let (a, b) = (num(&p, "length"), num(&p, "width"));
    let (chord, height) = (2.0 * rho2 * (phi / 2.0).sin(), rho2 - rho1 * (phi / 2.0).cos());
    assert!((close(a, chord, 1e-5) && close(b, height, 1e-5)) || (close(a, height, 1e-5) && close(b, chord, 1e-5)), "{a} {b}: {chord} {height}");
    // It has one bend-like entry, through the angle it turns, going across the sector.
    let bends = p["bends"].as_array().unwrap();
    assert_eq!(bends.len(), 1, "{p}");
    assert!(close(num(&bends[0], "angle"), turn, 1e-6), "{p}");
}

/// A flat ring and a cylinder with a bend between them: a hat. It cannot be laid flat without
/// stretching, and the pattern says so.
#[test]
fn a_profile_of_several_lines_is_unrolled_piece_by_piece_and_says_it_is_not_exact() {
    let (mut s, mut k) = start();
    // A flange from radius 80 in to radius 50, then up 30, about the Z axis.
    let sk = lines_on(&mut s, &mut k, "xz", &[(80.0, 0.0), (50.0, 0.0), (50.0, 30.0)]);
    let turn = 2.0;
    run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": turn }));
    // The profile is taken from its free end the way that first turns left: down the cylinder
    // from (50, 30), round to the flat, out to (80, 0). So the sheet is on the left: outside the
    // cylinder, radii 50 to 52, and above the flat, with the corner's inside (radius 3) there too.
    // The lines lose 5 each to a bend whose outside (the profile) has radius 5, centred (55, 5).
    let section = |a: f64, b: f64, h: f64| PI * (b * b - a * a) / TAU * turn * h;
    // Pappus for the round part: the quarter ring between radii 3 and 5 about (55, 5). Its area
    // is pi/4 (25 - 9) = 4 pi, its centroid at radius 55 less (4/3)(125 - 27)/(25 - 9) sin(45) / (pi/2) ...
    // by the integral: the volume of the ring is turn x the integral of r over its area.
    let (a, b) = (3.0f64, 5.0f64);
    let moment = (b * b * b - a * a * a) / 3.0;
    let ring = turn * (55.0 * (FRAC_PI_2 / 2.0) * (b * b - a * a) - moment);
    let want = section(50.0, 52.0, 25.0) + section(55.0, 80.0, 2.0) + ring;
    let v = volume(&mut s, &mut k);
    assert!(close(v, want, 1e-4), "{v} {want}");
    let p = flat(&mut s, &mut k);
    let notes = p["notes"].as_array().unwrap_or_else(|| panic!("no notes: {p}"));
    assert_eq!(notes.len(), 1, "{p}");
    let note = notes[0].as_str().unwrap();
    assert!(note.contains("does not unroll exactly") && note.contains("row by row"), "{note}");
    // Up the page: the cylinder's 25, the round part's neutral length and the flat's 25.
    let round = FRAC_PI_2 * (R + K * T);
    let across = 25.0 + round + 25.0;
    let cylinder = 50.0 + K * T;
    // Across the page, the widest row: the flat's outer rim, the angle x 80.
    let (x, y) = (num(&p, "length"), num(&p, "width"));
    assert!(close(x.max(y), turn * 80.0, 1e-5) && close(x.min(y), across, 1e-5), "{x} {y}: {} {across}", turn * 80.0);
    // The area, row by row: each piece's length x the angle x its mean neutral radius. (The round
    // part runs from the cylinder's neutral radius to the flat ring's inner rim.)
    let area = turn * (25.0 * cylinder + round * (cylinder + 55.0) / 2.0 + 25.0 * 67.5);
    assert!(close(num(&p, "area"), area, 1e-5), "{} {area}", num(&p, "area"));
    // How far the flat ring's outer rim would bow, on its own: 80 (1 - cos(turn / 2)).
    let bow = 80.0 * (1.0 - (turn / 2.0).cos());
    assert!(note.contains(&format!("{} mm", (bow * 100.0).round() / 100.0)), "{note}: {bow}");
    // The round part and the cylinder are what bends: the flat ring is not bent at all.
    let bends = p["bends"].as_array().unwrap();
    assert_eq!(bends.len(), 2, "the round part, and the cylinder: {p}");
    assert!(bends.iter().any(|b| close(num(b, "radius"), R, 1e-6) && close(num(b, "angle"), FRAC_PI_2, 1e-6)), "{p}");
    assert!(bends.iter().any(|b| close(num(b, "angle"), turn, 1e-6)), "{p}");
}

#[test]
fn a_contour_roll_says_what_it_cannot_be() {
    let (mut s, mut k) = start();
    let sk = lines_on(&mut s, &mut k, "xz", &[(50.0, 0.0), (50.0, 40.0)]);
    // A whole turn would close on itself: a sheet has a seam.
    let e = refused(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": TAU }));
    assert!(e.contains("leave it a seam"), "{e}");
    // An axis out of the sketch's plane.
    run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "y", "angle": 1.0 }));
    assert!(failure(&mut s, &mut k).contains("not in the sketch's plane"));
    run(&mut s, &mut k, "edit.undo", json!({}));
    // A profile across the axis.
    let across = lines_on(&mut s, &mut k, "xz", &[(-20.0, 0.0), (20.0, 0.0)]);
    run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": across, "axis": "z", "angle": 1.0 }));
    assert!(failure(&mut s, &mut k).contains("reaches the axis"));
}

#[test]
fn a_contour_roll_is_kept_in_the_file_and_taken_back_and_suppressed() {
    let (mut s, mut k) = start();
    let sk = lines_on(&mut s, &mut k, "xz", &[(50.0, 0.0), (50.0, 40.0)]);
    let f = id(&run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": 2.0, "radius": 4.0 })), "feature");
    let v = volume(&mut s, &mut k);
    let kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(f as u32)).unwrap().kind).unwrap();
    assert_eq!(
        kind,
        json!({ "type": "contour_roll", "sketch": sk, "axis": { "origin": "Z" }, "angle": 2.0, "direction": "default", "position": "default", "radius": 4.0, "operation": "join" }),
        "what a file says of it"
    );
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": f, "suppressed": true }));
    assert_eq!(volume(&mut s, &mut k), 0.0);
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(close(volume(&mut s, &mut k), v, 1e-9));
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert_eq!(volume(&mut s, &mut k), 0.0, "the roll itself taken back");
}

/// What is cut in a rolled sheet is cut in its flat pattern, where it lies on the sheet unrolled.
#[test]
fn a_hole_in_a_ring_is_in_its_flat_pattern_where_it_lies_on_the_neutral_surface() {
    let (mut s, mut k) = start();
    let sk = lines_on(&mut s, &mut k, "xz", &[(50.0, 0.0), (50.0, 40.0)]);
    run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": PI, "direction": "symmetric", "position": "flipped" }));
    let whole = flat(&mut s, &mut k);
    // Half a ring either side of the plane of the sketch, so its middle is on +X at radius 50 or
    // so. A round hole of radius 5, drilled along X from a plane beside it, 20 up the ring.
    let wp = id(&run(&mut s, &mut k, "work.plane", json!({ "by": "offset", "base": "yz", "distance": 100 })), "feature");
    let c = id(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp, "project_origin": false })), "feature");
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": c, "cx": 0, "cy": 20, "r": 5 }));
    run(&mut s, &mut k, "model.extrude", json!({ "sketch": c, "distance": 200, "operation": "cut", "reverse": true }));
    let p = flat(&mut s, &mut k);
    assert_eq!(p["holes"], 1, "{p}");
    let hole = &p["inner"][0];
    let points: Vec<(f64, f64)> = hole
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["points"].as_array().cloned().unwrap_or_else(|| vec![c["start"].clone(), c["end"].clone()]))
        .map(|q| (q["x"].as_f64().unwrap(), q["y"].as_f64().unwrap()))
        .collect();
    let (x0, x1) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), q| (l.min(q.0), h.max(q.0)));
    let (y0, y1) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), q| (l.min(q.1), h.max(q.1)));
    // Along the axis it is its own 10. Round the axis, as it lies on a neutral surface of radius
    // rho: a chord of half-length 5 on the side it is laid out from, the larger, of radius 52.
    let rho = 50.0 + K * T;
    let side = 52.0;
    let round = 2.0 * rho * (5.0f64 / side).asin();
    assert!(close(y1 - y0, 10.0, 1e-2) && close(x1 - x0, round, 0.05), "{} {} {round}: {hole}", x1 - x0, y1 - y0);
    // In the middle of the sheet's length, 20 up it.
    let along = num(&whole, "length").max(num(&whole, "width"));
    assert!(close((x0 + x1) / 2.0, along / 2.0, 1e-2) && close((y0 + y1) / 2.0, 20.0, 1e-2), "{hole}");
}

/// A profile that is one arc: the round part of a profile on its own. It has no development: laid
/// flat it is the cone through its two rims, as long as its neutral surface is.
#[test]
fn a_round_profile_is_unrolled_as_long_as_its_neutral_surface_and_says_it_is_not_exact() {
    let (mut s, mut k) = start();
    // A quarter circle of radius 20 about (60, 0), from (80, 0) to (60, 20).
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" })), "feature");
    run(&mut s, &mut k, "sketch.arc", json!({ "sketch": sk, "cx": 60, "cy": 0, "x1": 80, "y1": 0, "x2": 60, "y2": 20 }));
    let turn = 2.0;
    run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": turn }));
    // The sheet goes on the left of the arc, its inside: radii 18 to 20 about (60, 0). The solid
    // is the angle x the integral of the distance from the axis over the section.
    let area = FRAC_PI_2 / 2.0 * (400.0 - 324.0);
    let moment = 60.0 * area + (8000.0 - 5832.0) / 3.0;
    let v = volume(&mut s, &mut k);
    assert!(close(v, turn * moment, 1e-3), "{v} {}", turn * moment);
    let p = flat(&mut s, &mut k);
    assert!(p["notes"][0].as_str().unwrap().contains("does not unroll exactly"), "{p}");
    // The neutral surface is 0.88 through from the inside: radius 18.88 about (60, 0). Its rims
    // are 78.88 and 60 from the axis, and it is a quarter of that circle long.
    let (near, far, long) = (60.0, 78.88, FRAC_PI_2 * 18.88);
    let (x, y) = (num(&p, "length"), num(&p, "width"));
    assert!(close(x, turn * far, 1e-5) && close(y, long, 1e-5), "{x} {y}");
    assert!(close(num(&p, "area"), turn * (near + far) / 2.0 * long, 1e-5), "{}", num(&p, "area"));
    // A bend of inside radius 18, a quarter turn: where it runs round the axis in the flat.
    let bends = p["bends"].as_array().unwrap();
    assert_eq!(bends.len(), 1, "{p}");
    assert!(close(num(&bends[0], "radius"), 18.0, 1e-6) && close(num(&bends[0], "angle"), FRAC_PI_2, 1e-6), "{p}");
}

/// A contour roll follows the rule and its sketch; it is deleted with what it was made of.
#[test]
fn a_contour_roll_follows_the_rule_and_its_sketch_and_can_be_deleted() {
    let (mut s, mut k) = start();
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xz" })), "feature");
    let up = run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": 50, "y1": 0, "x2": 50, "y2": 40 }));
    let f = id(&run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": 2.0 })), "feature");
    // Which side the sheet is on: on the left of a line that runs up is towards the axis.
    let ring = |a: f64, b: f64, high: f64| (b * b - a * a) * high;
    assert!(close(volume(&mut s, &mut k), ring(48.0, 50.0, 40.0), 1e-4), "{}", volume(&mut s, &mut k));
    // The rule's thickness.
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": 3 }));
    assert!(close(volume(&mut s, &mut k), ring(47.0, 50.0, 40.0), 1e-4), "{}", volume(&mut s, &mut k));
    let p = flat(&mut s, &mut k);
    let along = 2.0 * (47.0 + K * 3.0);
    assert!(close(num(&p, "length").max(num(&p, "width")), along, 1e-6), "{p}");
    // The sketch: the line's end moved makes it 60 long.
    run(&mut s, &mut k, "sketch.drag", json!({ "sketch": sk, "point": up["end"], "x": 50, "y": 60 }));
    assert!(close(volume(&mut s, &mut k), ring(47.0, 50.0, 60.0), 1e-3), "{}", volume(&mut s, &mut k));
    // Deleted, with the sketch it was made of: nothing left.
    run(&mut s, &mut k, "feature.delete", json!({ "feature": f, "consumed": true }));
    assert_eq!(volume(&mut s, &mut k), 0.0);
    assert!(s.document().sketch(tenon_model::FeatureId(sk as u32)).is_none(), "the sketch went with it");
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(close(volume(&mut s, &mut k), ring(47.0, 50.0, 60.0), 1e-3));
}

/// A flat ring turned 2 radians about Z, its middle along +X: from radius 30 to 60, z from 10 to 12.
fn flat_ring(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let sk = lines_on(s, k, "xz", &[(30.0, 10.0), (60.0, 10.0)]);
    run(s, k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": 2.0, "direction": "symmetric" }));
    (60.0 * 60.0 - 30.0 * 30.0) / 2.0 * 2.0 * T
}

/// A new sketch on a plane parallel to XY at height `z`.
fn sketch_at_height(s: &mut Session, k: &mut OcctKernel, z: f64) -> u64 {
    let wp = id(&run(s, k, "work.plane", json!({ "by": "offset", "base": "xy", "distance": z })), "feature");
    id(&run(s, k, "sketch.create", json!({ "work_plane": wp, "project_origin": false })), "feature")
}

/// An L rolled, and a flange on its flat ring: the flange is made, and no flat pattern can be had of
/// the two. It said the pattern overlapped itself and to shorten a flange, at any size, or that the
/// part was not one sheet.
#[test]
fn a_flange_on_a_ring_rolled_with_other_pieces_says_why_it_has_no_flat_pattern() {
    for (height, offsets) in [(25.0, None), (25.0, Some([10.0, 10.0])), (8.0, Some([15.0, 15.0]))] {
        let (mut s, mut k) = start();
        let sk = lines_on(&mut s, &mut k, "xz", &[(50.0, 0.0), (50.0, 40.0), (90.0, 40.0)]);
        run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": 270f64.to_radians() }));
        assert!(flat(&mut s, &mut k)["length"].as_f64().unwrap() > 400.0, "alone, the roll lays flat");
        // The straight edge of the seam, on the ring.
        let edges = run(&mut s, &mut k, "model.find", json!({ "what": "edges", "kind": "line", "along": "x" }));
        let edge =
            edges["found"].as_array().unwrap().iter().find(|e| (e["mid"][2].as_f64().unwrap() - 40.0).abs() < 1e-6).unwrap()["reference"].clone();
        let mut flange = json!({ "edges": [edge], "height": height });
        if let Some(o) = offsets {
            flange["offsets"] = json!(o);
        }
        let before = volume(&mut s, &mut k);
        run(&mut s, &mut k, "sheet.flange", flange);
        assert!(volume(&mut s, &mut k) > before, "the flange is made");
        let why = refused(&mut s, &mut k, "sheet.flat", json!({}));
        assert!(why.contains("no flat pattern") && why.contains("round another axis"), "{why}");
        assert!(!why.contains("Shorten") && !why.contains("not one sheet"), "{why}");
    }
    // A flat ring alone takes a flange and lays flat.
    let (mut s, mut k) = start();
    flat_ring(&mut s, &mut k);
    let edges = run(&mut s, &mut k, "model.find", json!({ "what": "edges", "kind": "line" }));
    let seam = edges["found"].as_array().unwrap().iter().find(|e| (e["length"].as_f64().unwrap() - 30.0).abs() < 1e-6).unwrap()["reference"].clone();
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [seam], "height": 10 }));
    assert!(flat(&mut s, &mut k)["area"].as_f64().unwrap() > 2700.0);
}

/// A flat ring is flat faces like any other, both ways up: each looks out of the sheet, the other
/// side one thickness behind it, and all that goes on a flat side goes on it.
#[test]
fn a_flat_ring_is_a_side_of_the_sheet_both_ways_up() {
    let (mut s, mut k) = start();
    let whole = flat_ring(&mut s, &mut k);
    assert!(close(volume(&mut s, &mut k), whole, 1e-6));
    let planes = run(&mut s, &mut k, "model.find", json!({ "what": "faces", "kind": "plane" }));
    let mut looks: Vec<(f64, f64)> = planes["found"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["area"].as_f64().unwrap() > 2000.0)
        .map(|f| (f["centroid"][2].as_f64().unwrap(), f["normal"][2].as_f64().unwrap()))
        .collect();
    looks.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(looks.len(), 2, "{planes}");
    assert!(close(looks[0].0, 10.0, 1e-9) && close(looks[0].1, -1.0, 1e-9), "{looks:?}");
    assert!(close(looks[1].0, 12.0, 1e-9) && close(looks[1].1, 1.0, 1e-9), "{looks:?}");
    let flat_area = (60.0 * 60.0 - 30.0 * 30.0) / 2.0 * 2.0;

    let mut made = Vec::new();
    for z in [10.0, 12.0] {
        // A punch on that side: a hole of its own area in the solid and the flat.
        let (mut s, mut k) = start();
        flat_ring(&mut s, &mut k);
        let sk = sketch_at_height(&mut s, &mut k, z);
        run(&mut s, &mut k, "sketch.point", json!({ "sketch": sk, "x": 45, "y": 0 }));
        run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk, "shape": "round", "diameter": 6 }));
        assert!(close(volume(&mut s, &mut k), whole - PI * 9.0 * T, 1e-5), "{z}");
        let p = flat(&mut s, &mut k);
        assert!(close(num(&p, "area"), flat_area - PI * 9.0, 1e-5) && p["holes"] == 1 && p["notes"].is_null(), "{z}: {p}");
        assert_eq!(p["marks"][0]["name"].as_str().unwrap(), "Round 6", "{p}");

        // A formed shape stands out of that side, by hand.
        let (mut s, mut k) = start();
        flat_ring(&mut s, &mut k);
        let sk = sketch_at_height(&mut s, &mut k, z);
        run(&mut s, &mut k, "sketch.point", json!({ "sketch": sk, "x": 45, "y": 0 }));
        run(&mut s, &mut k, "sheet.punch", json!({ "sketch": sk, "shape": "dimple", "diameter": 10, "depth": 2 }));
        assert!(close(volume(&mut s, &mut k), whole + PI * 4.0 * T, 1e-5), "{z}: {}", volume(&mut s, &mut k));
        let top = run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0]["bbox"].clone();
        let (lo, hi) = (top["min"][2].as_f64().unwrap(), top["max"][2].as_f64().unwrap());
        assert!(if z > 11.0 { close(hi, 14.0, 1e-5) } else { close(lo, 8.0, 1e-5) }, "{z}: {top}");

        // A cut normal, from far off on that side: a circle of 5.
        let (mut s, mut k) = start();
        flat_ring(&mut s, &mut k);
        let sk = sketch_at_height(&mut s, &mut k, if z > 11.0 { 100.0 } else { -100.0 });
        run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 45, "cy": 0, "r": 5 }));
        run(&mut s, &mut k, "sheet.cut_normal", json!({ "sketch": sk }));
        assert!(close(volume(&mut s, &mut k), whole - PI * 25.0 * T, 1e-5), "{z}");
        assert!(close(num(&flat(&mut s, &mut k), "area"), flat_area - PI * 25.0, 1e-5));

        // A cut across bends, on that side: nothing to go across, 10 x 10.
        let (mut s, mut k) = start();
        flat_ring(&mut s, &mut k);
        let sk = sketch_at_height(&mut s, &mut k, z);
        run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 40, "y1": -5, "x2": 50, "y2": 5 }));
        run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk }));
        assert!(close(volume(&mut s, &mut k), whole - 100.0 * T, 1e-5), "{z}");
        assert!(close(num(&flat(&mut s, &mut k), "area"), flat_area - 100.0, 1e-5));

        // A flange on a straight edge of that side, from its seam at angle 0: the edge from radius
        // 30 to 60. It takes the ring back by the bend's outside radius of 5, a strip a little more
        // than 5 x 30 where the ring's arcs lean in; then a quarter ring and 15 straight, 30 wide.
        let (mut s, mut k) = start();
        let sk = lines_on(&mut s, &mut k, "xz", &[(30.0, 10.0), (60.0, 10.0)]);
        run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": 2.0 }));
        let edge = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [45, 0, z] }));
        run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "height": 20 }));
        let bend = FRAC_PI_2 / 2.0 * ((R + T) * (R + T) - R * R) * 30.0;
        let strip = 150.0 + 125.0 / 360.0;
        let v = volume(&mut s, &mut k);
        let want = whole - strip * T + bend + 15.0 * T * 30.0;
        assert!(close(v, want, 2.0), "{z}: {v} {want}");
        let p = flat(&mut s, &mut k);
        let wide = FRAC_PI_2 * (R + K * T) + 15.0;
        assert!(close(num(&p, "area"), flat_area - strip + wide * 30.0, 1.0), "{z}: {p}");
        assert_eq!(p["bends"].as_array().unwrap().len(), 1, "{p}");
        // Whichever side it is on, the same sheet.
        made.push((v, num(&p, "length"), num(&p, "width"), num(&p, "area")));
    }
    assert!(
        close(made[0].0, made[1].0, 1e-3)
            && close(made[0].1, made[1].1, 1e-6)
            && close(made[0].2, made[1].2, 1e-6)
            && close(made[0].3, made[1].3, 1e-6),
        "{made:?}"
    );
}

/// A cone with a flat tab on it, tangent along a seam: by hand 12160 mm³, and 6070.4 mm² flat.
/// (The cone: rims 30 and 60 at 40 apart, turned 2 radians; a slant of 50. The tab: 30 x 50 x 2.)
fn cone_and_tab(s: &mut Session, k: &mut OcctKernel) -> u64 {
    let sk = lines_on(s, k, "xz", &[(60.0, 40.0), (30.0, 0.0)]);
    run(s, k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": 2.0 }));
    let tilt = 0.6f64.atan2(0.8);
    let q0 = id(&run(s, k, "work.plane", json!({ "by": "angle", "base": "yz", "axis": "y", "angle": tilt })), "feature");
    let q = id(&run(s, k, "work.plane", json!({ "by": "offset", "base": { "work": q0 }, "distance": 24 })), "feature");
    let tab = id(&run(s, k, "sketch.create", json!({ "work_plane": q, "project_origin": false })), "feature");
    run(s, k, "sketch.rectangle", json!({ "sketch": tab, "x1": -30, "y1": 18, "x2": 0, "y2": 68 }));
    run(s, k, "sheet.face", json!({ "sketch": tab }));
    q
}

/// A cut across bends or normal to the sheet has nothing to go by on a cone or a round part. What
/// reaches one is refused, not left half cut; what is on the flat is cut, by hand; a cylinder it
/// follows as it does a bend.
#[test]
fn a_cut_that_reaches_a_rolled_part_is_refused_and_one_on_the_flat_or_a_cylinder_is_cut_by_hand() {
    let rectangle = |s: &mut Session, k: &mut OcctKernel, q: u64, x1: f64, x2: f64| {
        let c = id(&run(s, k, "sketch.create", json!({ "work_plane": q, "project_origin": false })), "feature");
        run(s, k, "sketch.rectangle", json!({ "sketch": c, "x1": x1, "y1": 30, "x2": x2, "y2": 40 }));
        c
    };
    for cut in ["sheet.cut_across_bend", "sheet.cut_normal"] {
        // Over the seam, half on the tab and half on the cone.
        let (mut s, mut k) = start();
        let q = cone_and_tab(&mut s, &mut k);
        assert!(close(volume(&mut s, &mut k), 12_160.0, 1e-3));
        assert!(close(num(&flat(&mut s, &mut k), "area"), 6070.4, 1e-3));
        let c = rectangle(&mut s, &mut k, q, -10.0, 10.0);
        run(&mut s, &mut k, cut, json!({ "sketch": c }));
        let e = failure(&mut s, &mut k);
        assert!(e.contains("rolled part") && e.contains("ordinary Cut"), "{cut}: {e}");
        // All on the tab: 10 x 10 goes.
        let (mut s, mut k) = start();
        let q = cone_and_tab(&mut s, &mut k);
        let c = rectangle(&mut s, &mut k, q, -20.0, -10.0);
        run(&mut s, &mut k, cut, json!({ "sketch": c }));
        assert!(close(volume(&mut s, &mut k), 12_160.0 - 100.0 * T, 1e-3), "{cut}");
        assert!(close(num(&flat(&mut s, &mut k), "area"), 6070.4 - 100.0, 1e-3), "{cut}");
    }
    // Over the seam of a tab and a cylinder it is followed all the way: a square of 20 x 10 in
    // the flat, half of it round the cylinder.
    let (mut s, mut k) = start();
    let wp = id(&run(&mut s, &mut k, "work.plane", json!({ "by": "offset", "base": "yz", "distance": 48 })), "feature");
    let t = id(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp, "project_origin": false })), "feature");
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": t, "x1": -30, "y1": 0, "x2": 0, "y2": 40 }));
    run(&mut s, &mut k, "sheet.face", json!({ "sketch": t }));
    let sk = lines_on(&mut s, &mut k, "xz", &[(50.0, 0.0), (50.0, 40.0)]);
    run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": 3.0 }));
    let (before, area) = (volume(&mut s, &mut k), num(&flat(&mut s, &mut k), "area"));
    let c = id(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": wp, "project_origin": false })), "feature");
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": c, "x1": -10, "y1": 10, "x2": 10, "y2": 20 }));
    run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": c }));
    // On the tab 10 x 10 x 2; on the cylinder 10 long along the neutral radius 48.88, which is
    // 10 / 48.88 radians, of a ring from 48 to 50, 10 wide.
    let on_ring = 10.0 / (48.0 + K * T) * (50.0 * 50.0 - 48.0 * 48.0) / 2.0 * 10.0;
    assert!(close(volume(&mut s, &mut k), before - 200.0 - on_ring, 1e-3), "{} {}", before - volume(&mut s, &mut k), 200.0 + on_ring);
    assert!(close(num(&flat(&mut s, &mut k), "area"), area - 200.0, 1e-3));

    // A hat: from its flat ring over the round part to the cylinder is refused; on the ring, cut.
    let hat = |s: &mut Session, k: &mut OcctKernel| {
        let sk = lines_on(s, k, "xz", &[(80.0, 0.0), (50.0, 0.0), (50.0, 30.0)]);
        run(s, k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": 2.0, "direction": "symmetric" }));
        sketch_at_height(s, k, T)
    };
    let (mut s, mut k) = start();
    let c = hat(&mut s, &mut k);
    let before = volume(&mut s, &mut k);
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": c, "x1": 45, "y1": -5, "x2": 65, "y2": 5 }));
    run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": c }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("rolled part"), "{e}");
    let (mut s, mut k) = start();
    let c = hat(&mut s, &mut k);
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": c, "x1": 60, "y1": -5, "x2": 70, "y2": 5 }));
    run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": c }));
    assert!(close(volume(&mut s, &mut k), before - 100.0 * T, 1e-3));
}

/// Lines that run straight on from one another are one line, and so the surface they look like.
#[test]
fn a_cone_or_a_cylinder_drawn_in_pieces_is_one_and_unrolls_exactly() {
    let cone = |points: &[(f64, f64)]| {
        let (mut s, mut k) = start();
        let sk = lines_on(&mut s, &mut k, "xz", points);
        let f = id(&run(&mut s, &mut k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": 2.0 })), "feature");
        let v = volume(&mut s, &mut k);
        let p = flat(&mut s, &mut k);
        let doc = s.document();
        let sketch = doc.sketch(tenon_model::FeatureId(sk as u32)).unwrap();
        let tenon_model::FeatureKind::ContourRoll(c) = &doc.feature(tenon_model::FeatureId(f as u32)).unwrap().kind else { panic!() };
        (v, p, tenon_model::regen::roll_words(c, sketch).unwrap())
    };
    let (one, whole, _) = cone(&[(30.0, 0.0), (60.0, 40.0)]);
    let (two, pieces, words) = cone(&[(30.0, 0.0), (45.0, 20.0), (60.0, 40.0)]);
    assert!(close(one, two, 1e-6), "{one} {two}");
    assert_eq!(pieces["notes"], Value::Null, "{pieces}");
    assert!(words.contains("unrolls exactly"), "{words}");
    for key in ["length", "width", "area", "cut_length"] {
        assert!(close(num(&whole, key), num(&pieces, key), 1e-6), "{key}: {whole} {pieces}");
    }
    assert_eq!(pieces["bends"].as_array().unwrap().len(), 1, "{pieces}");
    let (_, p, words) = cone(&[(50.0, 0.0), (50.0, 20.0), (50.0, 30.0), (50.0, 40.0)]);
    assert!(words.contains("unrolls exactly") && p["notes"].is_null(), "{words} {p}");
    assert!(close(num(&p, "length"), 2.0 * (48.0 + K * T), 1e-6) || close(num(&p, "length"), 2.0 * (50.0 + K * T), 1e-6), "{p}");
    assert_eq!(p["bends"].as_array().unwrap().len(), 1, "{p}");
    // A line that turns back on itself, or goes off at an angle, is no such thing.
    let (_, _, words) = cone(&[(50.0, 0.0), (50.0, 20.0), (60.0, 40.0)]);
    assert!(words.contains("does not unroll exactly"), "{words}");
}

/// A sheet with no flat side at all is told so by the cuts that start from one, and what it is
/// takes an ordinary cut, by hand.
#[test]
fn a_sheet_with_no_flat_side_is_told_to_take_an_ordinary_cut() {
    let ring = |s: &mut Session, k: &mut OcctKernel| {
        let sk = lines_on(s, k, "xz", &[(50.0, 0.0), (50.0, 40.0)]);
        run(s, k, "sheet.contour_roll", json!({ "sketch": sk, "axis": "z", "angle": 3.0, "direction": "symmetric", "position": "flipped" }));
        let wp = id(&run(s, k, "work.plane", json!({ "by": "offset", "base": "yz", "distance": 100 })), "feature");
        let c = id(&run(s, k, "sketch.create", json!({ "work_plane": wp, "project_origin": false })), "feature");
        run(s, k, "sketch.circle", json!({ "sketch": c, "cx": 0, "cy": 20, "r": 5 }));
        c
    };
    for cut in ["sheet.cut_normal", "sheet.cut_across_bend"] {
        let (mut s, mut k) = start();
        let c = ring(&mut s, &mut k);
        run(&mut s, &mut k, cut, json!({ "sketch": c }));
        let e = failure(&mut s, &mut k);
        assert!(e.contains("rolled or die-formed all over") && e.contains("ordinary Cut"), "{cut}: {e}");
    }
}
