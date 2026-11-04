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
    let line = s.document().sketch(tenon_model::FeatureId(sk as u32)).unwrap().entities().next().map(|e| e.0).unwrap();
    let _ = line;
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
