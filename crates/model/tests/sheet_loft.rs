//! Lofted Flange: a sheet between two open profiles, as press-brake facets or die formed, against
//! figures worked out by hand, and a round-to-square transition against what is known of it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::FRAC_PI_2;

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

fn start(t: f64, r: f64) -> (Session, OcctKernel) {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": t, "bend_radius": r }));
    (s, k)
}

/// A chain of lines through `points` in sketch `sk`.
fn chain(s: &mut Session, k: &mut OcctKernel, sk: u64, points: &[(f64, f64)]) {
    let mut last: Option<Value> = None;
    for w in points.windows(2) {
        let mut p = json!({ "sketch": sk, "x1": w[0].0, "y1": w[0].1, "x2": w[1].0, "y2": w[1].1 });
        if let Some(end) = &last {
            p["start"] = end.clone();
        }
        last = Some(run(s, k, "sketch.line", p)["end"].clone());
    }
}

/// A new sketch on a plane `distance` from `base`.
fn sketch_at(s: &mut Session, k: &mut OcctKernel, base: &str, distance: f64) -> u64 {
    if distance == 0.0 {
        return id(&run(s, k, "sketch.create", json!({ "plane": base, "project_origin": false })), "feature");
    }
    let wp = id(&run(s, k, "work.plane", json!({ "by": "offset", "base": base, "distance": distance })), "feature");
    id(&run(s, k, "sketch.create", json!({ "work_plane": wp, "project_origin": false })), "feature")
}

/// Between two like profiles a lofted flange is what a contour flange would be: facets for lines
/// and a bend at the corner, by hand.
#[test]
fn press_brake_facets_between_two_like_profiles_are_flat_with_a_bend_between_them_by_hand() {
    let (mut s, mut k) = start(T, R);
    let v = [(0.0, 0.0), (40.0, 0.0), (40.0, 30.0)];
    let a = sketch_at(&mut s, &mut k, "xz", 0.0);
    chain(&mut s, &mut k, a, &v);
    let b = sketch_at(&mut s, &mut k, "xz", 50.0);
    chain(&mut s, &mut k, b, &v);
    run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, b], "forming": "press_brake" }));
    // The profile's left is the inside of its corner: there the sheet is, so the profile is the
    // outside of the bend (radius 5, taking 5 of each line) as with a contour flange.
    let ba = FRAC_PI_2 * (R + K * T);
    let section = T * (35.0 + 25.0) + FRAC_PI_2 / 2.0 * (25.0 - 9.0);
    let vol = volume(&mut s, &mut k);
    assert!(close(vol, section * 50.0, 1e-5), "{vol} {}", section * 50.0);
    let f = flat(&mut s, &mut k);
    assert_eq!(f["notes"], Value::Null, "facets unfold exactly: {f}");
    let (x, y) = (num(&f, "length"), num(&f, "width"));
    let across = 35.0 + ba + 25.0;
    assert!((close(x, across, 1e-6) && close(y, 50.0, 1e-6)) || (close(y, across, 1e-6) && close(x, 50.0, 1e-6)), "{x} {y}: {across}");
    assert_eq!(f["bends"].as_array().unwrap().len(), 1, "{f}");
    assert!(close(num(&f["bends"][0], "radius"), R, 1e-6) && close(num(&f["bends"][0], "angle"), FRAC_PI_2, 1e-6));
    // On the other side of the profile: the profile is the inside of the bend, which takes 3.
    let (mut s, mut k) = start(T, R);
    let a = sketch_at(&mut s, &mut k, "xz", 0.0);
    chain(&mut s, &mut k, a, &v);
    let b = sketch_at(&mut s, &mut k, "xz", 50.0);
    chain(&mut s, &mut k, b, &v);
    run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, b], "forming": "press_brake", "position": "flipped" }));
    let section = T * (37.0 + 27.0) + FRAC_PI_2 / 2.0 * (25.0 - 9.0);
    assert!(close(volume(&mut s, &mut k), section * 50.0, 1e-5));
    // Two lines in one plane are one flat plate: no bends.
    let (mut s, mut k) = start(T, R);
    let a = sketch_at(&mut s, &mut k, "xy", 0.0);
    chain(&mut s, &mut k, a, &[(0.0, 0.0), (40.0, 0.0)]);
    let b = sketch_at(&mut s, &mut k, "xy", 0.0);
    chain(&mut s, &mut k, b, &[(10.0, 30.0), (30.0, 30.0)]);
    run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, b], "forming": "press_brake" }));
    // A trapezoid 40 and 20 wide, 30 between them.
    assert!(close(volume(&mut s, &mut k), (40.0 + 20.0) / 2.0 * 30.0 * T, 1e-6));
    let f = flat(&mut s, &mut k);
    assert!(close(num(&f, "area"), 900.0, 1e-6) && f["bends"].as_array().unwrap().is_empty(), "{f}");
}

/// Half of the classic duct transition: three sides of a 100 square below a half circle of
/// radius 40, 80 above it. The developable surface between them is known: a flat triangle on
/// each side of the square, with its point where the circle's tangent runs along that side, and
/// a piece of an oblique cone at each corner.
fn known_round_to_square() -> f64 {
    // The triangles: bases of 50, 100 and 50, each of height sqrt(10^2 + 80^2) up to the circle.
    let h = (100.0f64 + 6400.0).sqrt();
    let triangles = (50.0 + 100.0 + 50.0) * h / 2.0;
    // A corner: from the corner (50, 50, 0) to the quarter of the circle from (40, 0, 80) to
    // (0, 40, 80), summed as very many thin triangles.
    let on = |a: f64| [40.0 * a.cos(), 40.0 * a.sin(), 80.0];
    let n = 20_000;
    let mut cone = 0.0;
    for i in 0..n {
        let (a, b) = (FRAC_PI_2 * f64::from(i) / f64::from(n), FRAC_PI_2 * f64::from(i + 1) / f64::from(n));
        let (p, q) = (on(a), on(b));
        let u = [p[0] - 50.0, p[1] - 50.0, p[2]];
        let v = [q[0] - 50.0, q[1] - 50.0, q[2]];
        let c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        cone += 0.5 * (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
    }
    triangles + 2.0 * cone
}

/// The sketches of that transition: the square's three sides on the XY plane, the half circle
/// (two quarter arcs) 80 above.
fn round_to_square(s: &mut Session, k: &mut OcctKernel) -> (u64, u64) {
    let square = sketch_at(s, k, "xy", 0.0);
    chain(s, k, square, &[(50.0, 0.0), (50.0, 50.0), (-50.0, 50.0), (-50.0, 0.0)]);
    let round = sketch_at(s, k, "xy", 80.0);
    let first = run(s, k, "sketch.arc", json!({ "sketch": round, "cx": 0, "cy": 0, "x1": 40, "y1": 0, "x2": 0, "y2": 40 }));
    run(s, k, "sketch.arc", json!({ "sketch": round, "cx": 0, "cy": 0, "start": first["end"], "x2": -40, "y2": 0 }));
    (square, round)
}

#[test]
fn a_round_to_square_transition_as_press_brake_facets_comes_to_the_known_sheet() {
    // Thin, with small bends, so that what the bends add and the reliefs take are small.
    let (mut s, mut k) = start(0.5, 0.5);
    let (square, round) = round_to_square(&mut s, &mut k);
    run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [square, round], "forming": "press_brake", "chord": 0.5 }));
    let v = volume(&mut s, &mut k);
    let known = known_round_to_square();
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    assert!(close(v, known * 0.5, known * 0.5 * 0.02), "{v} {}", known * 0.5);
    // Each quarter of the circle in chords no further than 0.5 from it: half an angle of
    // acos(1 - 0.5/40) each.
    let step = 2.0 * (1.0f64 - 0.5 / 40.0).acos();
    let chords = (FRAC_PI_2 / step).ceil();
    let f = flat(&mut s, &mut k);
    assert_eq!(f["notes"], Value::Null, "facets unfold exactly: {f}");
    let bends = f["bends"].as_array().unwrap();
    // A facet for each chord and each side of the square, and a bend between each two.
    assert_eq!(bends.len() as f64, 2.0 * chords + 3.0 - 1.0, "{chords} chords a quarter: {} bends", bends.len());
    // It only ever folds one way, as the sheet round a corner does: not a bump among them.
    assert!(bends.iter().all(|b| b["up"] == bends[0]["up"]), "{f}");
    let area = num(&f, "area");
    assert!(close(area, known, known * 0.02), "{area} {known}");
    // A tolerance that would make too many is said.
    let (mut s, mut k) = start(0.5, 0.5);
    let (square, round) = round_to_square(&mut s, &mut k);
    run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [square, round], "forming": "press_brake", "chord": 0.001 }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("make the tolerance larger"), "{e}");
}

#[test]
fn a_round_to_square_transition_die_formed_comes_to_the_known_sheet_and_says_how_close() {
    let (mut s, mut k) = start(0.5, 2.0);
    let (square, round) = round_to_square(&mut s, &mut k);
    let f = id(&run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [square, round] })), "feature");
    let known = known_round_to_square();
    let v = volume(&mut s, &mut k);
    let body = &run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
    // (The corners rounded by a bend of 2 take a little of it.)
    assert!(close(v, known * 0.5, known * 0.5 * 0.02), "{v} {}", known * 0.5);
    let p = flat(&mut s, &mut k);
    let area = num(&p, "area");
    assert!(close(area, known, known * 0.02), "{area} {known}");
    let note = p["notes"][0].as_str().unwrap_or_else(|| panic!("no note: {p}"));
    assert!(note.contains("Die formed") && note.contains("triangles") && note.contains("how close"), "{note}");
    assert!(p["bends"].as_array().unwrap().is_empty(), "{p}");
    // What a file says of it.
    let kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(f as u32)).unwrap().kind).unwrap();
    assert_eq!(
        kind,
        json!({ "type": "lofted_flange", "sketches": [square, round], "forming": "die_formed", "position": "default", "operation": "join" })
    );
}

/// A quarter of a cone between two arcs: developable, so laid flat by triangles it is the sector
/// of a ring, by hand.
#[test]
fn a_die_formed_cone_lays_flat_as_the_sector_it_is() {
    let (mut s, mut k) = start(0.5, 1.0);
    let low = sketch_at(&mut s, &mut k, "xy", 0.0);
    run(&mut s, &mut k, "sketch.arc", json!({ "sketch": low, "cx": 0, "cy": 0, "x1": 40, "y1": 0, "x2": 0, "y2": 40 }));
    let high = sketch_at(&mut s, &mut k, "xy", 30.0);
    run(&mut s, &mut k, "sketch.arc", json!({ "sketch": high, "cx": 0, "cy": 0, "x1": 20, "y1": 0, "x2": 0, "y2": 20 }));
    run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [low, high] }));
    // A slant of sqrt(20^2 + 30^2): the mean radius 30 x the angle x the slant x the thickness.
    let slant = (400.0f64 + 900.0).sqrt();
    let want = 30.0 * FRAC_PI_2 * slant;
    let v = volume(&mut s, &mut k);
    assert!(close(v, want * 0.5, want * 0.5 * 0.01), "{v} {}", want * 0.5);
    let p = flat(&mut s, &mut k);
    assert!(close(num(&p, "area"), want, want * 0.01), "{} {want}", num(&p, "area"));
    // Next to nothing stretched: a cone does not.
    let note = p["notes"][0].as_str().unwrap();
    assert!(note.contains("0.00 %") || note.contains("0.01 %"), "{note}");
}

#[test]
fn a_lofted_flange_says_what_it_cannot_be() {
    let (mut s, mut k) = start(T, R);
    let a = sketch_at(&mut s, &mut k, "xz", 0.0);
    chain(&mut s, &mut k, a, &[(0.0, 0.0), (40.0, 0.0)]);
    let e = refused(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, a] }));
    assert!(e.contains("two profiles, in two sketches"), "{e}");
    let e = refused(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a] }));
    assert!(e.contains("the two sketches"), "{e}");
    let closed = sketch_at(&mut s, &mut k, "xy", 0.0);
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": closed, "x1": 0, "y1": 0, "x2": 10, "y2": 10 }));
    run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, closed] }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("The profile is closed, and a lofted flange follows an open one"), "{e}");
    run(&mut s, &mut k, "edit.undo", json!({}));
    let e = refused(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, closed], "chord": 0.1 }));
    assert!(e.contains("press-brake"), "{e}");
    let e = refused(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, closed], "forming": "press_brake", "chord": 0 }));
    assert!(e.contains("chord tolerance"), "{e}");
}

/// A lofted flange follows the rule, and its sketches; it is taken back, suppressed and deleted.
#[test]
fn a_lofted_flange_follows_the_rule_and_its_sketches_and_can_be_taken_back() {
    let (mut s, mut k) = start(T, R);
    let v = [(0.0, 0.0), (40.0, 0.0), (40.0, 30.0)];
    let a = sketch_at(&mut s, &mut k, "xz", 0.0);
    chain(&mut s, &mut k, a, &v);
    let b = sketch_at(&mut s, &mut k, "xz", 50.0);
    chain(&mut s, &mut k, b, &v);
    let f = id(&run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, b], "forming": "press_brake" })), "feature");
    let section = |t: f64, r: f64| {
        // The profile is the outside of the bend, so the bend takes r + t of each line.
        t * (40.0 - (r + t) + 30.0 - (r + t)) + FRAC_PI_2 / 2.0 * ((r + t) * (r + t) - r * r)
    };
    assert!(close(volume(&mut s, &mut k), section(T, R) * 50.0, 1e-5));
    // A thickness and a radius of the rule.
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": 1, "bend_radius": 2 }));
    assert!(close(volume(&mut s, &mut k), section(1.0, 2.0) * 50.0, 1e-5), "{}", volume(&mut s, &mut k));
    let p = flat(&mut s, &mut k);
    assert!(close(num(&p, "area"), (40.0 - 3.0 + 30.0 - 3.0 + FRAC_PI_2 * (2.0 + K)) * 50.0, 1e-5), "{p}");
    // Suppressed, and back; deleted, and back.
    let whole = volume(&mut s, &mut k);
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": f, "suppressed": true }));
    assert_eq!(volume(&mut s, &mut k), 0.0);
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(close(volume(&mut s, &mut k), whole, 1e-6));
    run(&mut s, &mut k, "feature.delete", json!({ "feature": f }));
    assert_eq!(volume(&mut s, &mut k), 0.0);
    run(&mut s, &mut k, "edit.undo", json!({}));
    assert!(close(volume(&mut s, &mut k), whole, 1e-6));
}
