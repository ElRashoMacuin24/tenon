//! Lofted Flange: a sheet between two open profiles, as press-brake facets or die formed, against
//! figures worked out by hand, and a round-to-square transition against what is known of it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};

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

/// A plate 100 x 80, 1 thick, between two lines 80 apart: x from 0 to 100, y from 0 to 1, z from 0
/// to 80.
fn plate_between(s: &mut Session, k: &mut OcctKernel, forming: &str) {
    let a = sketch_at(s, k, "xy", 0.0);
    chain(s, k, a, &[(0.0, 0.0), (100.0, 0.0)]);
    let b = sketch_at(s, k, "xy", 80.0);
    chain(s, k, b, &[(0.0, 0.0), (100.0, 0.0)]);
    run(s, k, "sheet.lofted_flange", json!({ "sketches": [a, b], "forming": forming }));
}

/// A rectangle on XZ, from z = `z0` to `z1`, as a Face in the plane of that plate.
fn tab_on_plate(s: &mut Session, k: &mut OcctKernel, z0: f64, z1: f64) {
    let sk = sketch_at(s, k, "xz", 0.0);
    run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": z0, "x2": 100, "y2": z1 }));
    run(s, k, "sheet.face", json!({ "sketch": sk, "flip": true }));
}

/// A circle at the plate's middle, cut `deep` into it from its y = 0 side.
fn pocket_in_plate(s: &mut Session, k: &mut OcctKernel, r: f64, deep: f64) {
    let sk = sketch_at(s, k, "xz", 100.0);
    run(s, k, "sketch.circle", json!({ "sketch": sk, "cx": 50, "cy": 40, "r": r }));
    run(s, k, "model.extrude", json!({ "sketch": sk, "distance": 100.0 + deep, "operation": "cut", "reverse": true }));
}

/// A die-formed sheet is laid flat by its own triangles and has nothing to follow out of them, so
/// whatever is not a cut straight through it would be left out without a word. It is refused, each
/// time beside what the same part comes to as press-brake facets.
#[test]
fn what_is_joined_to_a_die_formed_sheet_or_cut_part_way_into_it_is_refused_in_its_flat_pattern() {
    let is_refusal = |e: String| assert!(e.contains("die-formed sheet") && e.contains("Press Brake"), "{e}");
    // A tab in its plane 40 long and a flange on that, as facets: 100 x (80 + 40 + bend + straight).
    let tab_and_flange = |s: &mut Session, k: &mut OcctKernel| {
        tab_on_plate(s, k, 80.0, 120.0);
        let edge = run(s, k, "model.edge_ref", json!({ "at": [50, 0, 120] }));
        run(s, k, "sheet.flange", json!({ "edges": [edge], "height": 20 }));
    };
    let (mut s, mut k) = start(1.0, 1.0);
    plate_between(&mut s, &mut k, "press_brake");
    tab_and_flange(&mut s, &mut k);
    let v = volume(&mut s, &mut k);
    let p = flat(&mut s, &mut k);
    // The flange's straight is 20 less the bend's outside radius of 2.
    let (bend, straight) = (FRAC_PI_2 * (1.0 + K), 18.0);
    assert!(close(num(&p, "width"), 80.0 + 40.0 - 2.0 + bend + straight, 1e-6) && close(num(&p, "length"), 100.0, 1e-6), "{p}");
    let (mut s, mut k) = start(1.0, 1.0);
    plate_between(&mut s, &mut k, "die_formed");
    assert!(close(num(&flat(&mut s, &mut k), "area"), 8000.0, 1e-6), "the plate alone lays flat");
    tab_and_flange(&mut s, &mut k);
    assert!(close(volume(&mut s, &mut k), v, 1e-3), "the same solid");
    is_refusal(refused(&mut s, &mut k, "sheet.flat", json!({})));

    // A tab small enough to pass a check of the volume alone: 1 of 80.
    let (mut s, mut k) = start(1.0, 1.0);
    plate_between(&mut s, &mut k, "die_formed");
    tab_on_plate(&mut s, &mut k, 80.0, 81.0);
    assert!(close(volume(&mut s, &mut k), 8100.0, 1e-3));
    is_refusal(refused(&mut s, &mut k, "sheet.flat", json!({})));

    // A block on it: facets are refused too, as no sheet at all.
    let block = |s: &mut Session, k: &mut OcctKernel| {
        let sk = sketch_at(s, k, "xz", 0.0);
        run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 40, "y1": 30, "x2": 60, "y2": 50 }));
        run(s, k, "model.extrude", json!({ "sketch": sk, "distance": 30 }));
    };
    let (mut s, mut k) = start(1.0, 1.0);
    plate_between(&mut s, &mut k, "die_formed");
    block(&mut s, &mut k);
    assert!(close(volume(&mut s, &mut k), 8000.0 + 400.0 * 30.0, 1e-3));
    is_refusal(refused(&mut s, &mut k, "sheet.flat", json!({})));
    let (mut s, mut k) = start(1.0, 1.0);
    plate_between(&mut s, &mut k, "press_brake");
    block(&mut s, &mut k);
    assert!(refused(&mut s, &mut k, "sheet.flat", json!({})).contains("not a sheet"));

    // A pocket half way through, 20 across: half its area x 1 goes, and the two sides' mean area x
    // 1 comes to just that, so the volume alone would pass it.
    let (mut s, mut k) = start(1.0, 1.0);
    plate_between(&mut s, &mut k, "die_formed");
    pocket_in_plate(&mut s, &mut k, 10.0, 0.5);
    assert!(close(volume(&mut s, &mut k), 8000.0 - PI * 100.0 * 0.5, 1e-3));
    is_refusal(refused(&mut s, &mut k, "sheet.flat", json!({})));
    // And one far too small to show in any volume.
    let (mut s, mut k) = start(1.0, 1.0);
    plate_between(&mut s, &mut k, "die_formed");
    pocket_in_plate(&mut s, &mut k, 2.0, 0.5);
    assert!(close(volume(&mut s, &mut k), 8000.0 - PI * 4.0 * 0.5, 1e-3));
    is_refusal(refused(&mut s, &mut k, "sheet.flat", json!({})));

    // A hole straight through is what it can show: its outline in the pattern, by hand.
    let (mut s, mut k) = start(1.0, 1.0);
    plate_between(&mut s, &mut k, "die_formed");
    pocket_in_plate(&mut s, &mut k, 10.0, 200.0);
    assert!(close(volume(&mut s, &mut k), 8000.0 - PI * 100.0, 1e-3));
    let p = flat(&mut s, &mut k);
    assert_eq!(p["holes"], 1, "{p}");
    assert!(close(num(&p, "area"), 8000.0 - PI * 100.0, 1e-3), "{p}");
    assert!(close(num(&p, "length"), 100.0, 1e-6) && close(num(&p, "width"), 80.0, 1e-6), "{p}");
    assert!(close(num(&p, "cut_length"), 360.0 + 20.0 * PI, 1e-2), "{p}");

    // Two die-formed sheets joined: the pattern of one would leave out the other.
    let (mut s, mut k) = start(1.0, 1.0);
    plate_between(&mut s, &mut k, "die_formed");
    let a = sketch_at(&mut s, &mut k, "xy", 80.0);
    chain(&mut s, &mut k, a, &[(0.0, 0.0), (100.0, 0.0)]);
    let b = sketch_at(&mut s, &mut k, "xy", 120.0);
    chain(&mut s, &mut k, b, &[(0.0, 0.0), (100.0, 0.0)]);
    run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, b] }));
    assert!(close(volume(&mut s, &mut k), 12_000.0, 0.1));
    is_refusal(refused(&mut s, &mut k, "sheet.flat", json!({})));
}

/// The line from (0, 0) to (100, 0) at z = 0, and the same line turned by `degrees` about its
/// middle at z = 80.
fn twisted(s: &mut Session, k: &mut OcctKernel, degrees: f64, forming: &str) {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let a = sketch_at(s, k, "xy", 0.0);
    chain(s, k, a, &[(0.0, 0.0), (100.0, 0.0)]);
    let b = sketch_at(s, k, "xy", 80.0);
    chain(s, k, b, &[(50.0 - 50.0 * cos, -50.0 * sin), (50.0 + 50.0 * cos, 50.0 * sin)]);
    run(s, k, "sheet.lofted_flange", json!({ "sketches": [a, b], "forming": forming }));
}

/// The areas of the two largest faces: the two sides of a sheet.
fn sides(s: &mut Session, k: &mut OcctKernel) -> [f64; 2] {
    let found = run(s, k, "model.find", json!({ "what": "faces" }));
    let mut areas: Vec<f64> = found["found"].as_array().unwrap().iter().map(|f| f["area"].as_f64().unwrap()).collect();
    areas.sort_by(|a, b| b.total_cmp(a));
    [areas[0], areas[1]]
}

/// A surface that twists has no offset that is ruled, and the far side here is ruled between its
/// offset rims: it comes nearer the near side the more the sheet twists. The solid's own figures
/// hold it to the rule, and past what a sheet can be it is refused with the thickness it would be.
#[test]
fn a_twisted_die_formed_sheet_keeps_to_its_thickness_or_says_how_thin_it_would_be() {
    // Held to its sides' area x its thickness to the share a flat pattern holds a solid to.
    for degrees in [0.0, 5.0, 10.0, 15.0] {
        let (mut s, mut k) = start(1.0, 1.0);
        twisted(&mut s, &mut k, degrees, "die_formed");
        let v = volume(&mut s, &mut k);
        let [a, b] = sides(&mut s, &mut k);
        let want = (a + b) / 2.0;
        assert!(v <= want * 1.001 && v >= want * 0.98, "{degrees} degrees: {v} of {want}");
        let note = flat(&mut s, &mut k)["notes"][0].as_str().unwrap().to_owned();
        // What it says of it is what the solid comes to.
        if degrees >= 10.0 {
            assert!(note.contains("twists by up to") && note.contains("thick on the whole"), "{note}");
            let said: f64 = note.split("It comes to ").nth(1).unwrap().split(" mm").next().unwrap().parse().unwrap();
            assert!(close(said, v / want, 0.003), "{said} and {}", v / want);
        } else {
            assert!(!note.contains("twists"), "{note}");
        }
    }
    // Past it: refused with what it would come to, which was a third thin at a quarter turn.
    for (degrees, thin) in [(30.0, 0.94), (60.0, 0.81), (90.0, 0.67)] {
        let (mut s, mut k) = start(1.0, 1.0);
        twisted(&mut s, &mut k, degrees, "die_formed");
        let e = failure(&mut s, &mut k);
        assert!(e.contains("twist") && e.contains("Press Brake") && e.contains("where the rule says 1 mm"), "{e}");
        let said: f64 = e.split("come to only ").nth(1).unwrap().split(" mm").next().unwrap().parse().unwrap();
        assert!(close(said, thin, 0.01), "{e}");
        // As facets the same two lines keep their thickness.
        let (mut s, mut k) = start(1.0, 1.0);
        twisted(&mut s, &mut k, degrees, "press_brake");
        let v = volume(&mut s, &mut k);
        let p = flat(&mut s, &mut k);
        assert!(close(v, num(&p, "area"), 0.02 * v), "{degrees} degrees: {v} {p}");
    }
}

/// Half a cone, of radius 20 at z = 0 and radius 10 at z = 80.
fn half_cone(s: &mut Session, k: &mut OcctKernel, position: &str) {
    let half = |s: &mut Session, k: &mut OcctKernel, sk: u64, r: f64| {
        let q = run(s, k, "sketch.arc", json!({ "sketch": sk, "cx": 0, "cy": 0, "x1": r, "y1": 0, "x2": 0, "y2": r }));
        run(s, k, "sketch.arc", json!({ "sketch": sk, "cx": 0, "cy": 0, "start": q["end"], "x2": -r, "y2": 0 }));
    };
    let a = sketch_at(s, k, "xy", 0.0);
    half(s, k, a, 20.0);
    let b = sketch_at(s, k, "xy", 80.0);
    half(s, k, b, 10.0);
    run(s, k, "sheet.lofted_flange", json!({ "sketches": [a, b], "position": position }));
}

/// Inside a curve the sheet goes towards its centre, and past that it would cross itself. By hand,
/// what a half cone comes to on either side: its middle surface's area x the thickness.
#[test]
fn a_die_formed_sheet_thicker_than_the_curve_it_is_inside_is_refused_by_that_curves_radius() {
    let slant = (100.0f64 + 6400.0).sqrt();
    let cos = 80.0 / slant;
    let by_hand = |t: f64, way: f64| PI * ((20.0 + way * t / 2.0 * cos) + (10.0 + way * t / 2.0 * cos)) / 2.0 * slant * t;
    for t in [5.0, 9.0] {
        let (mut s, mut k) = start(t, 1.0);
        half_cone(&mut s, &mut k, "default");
        let v = volume(&mut s, &mut k);
        assert!(close(v, by_hand(t, -1.0), 2e-3 * v), "{t}: {v} {}", by_hand(t, -1.0));
    }
    // Past the small rim's centre, and then past both.
    for (t, radius) in [(12.0, "10"), (30.0, "20")] {
        let (mut s, mut k) = start(t, 1.0);
        half_cone(&mut s, &mut k, "default");
        let e = failure(&mut s, &mut k);
        assert!(e.contains(&format!("radius of {radius} mm")) && e.contains("crosses itself") && e.contains("Position"), "{t}: {e}");
    }
    // On the outside of the curve nothing is in the way.
    let (mut s, mut k) = start(12.0, 1.0);
    half_cone(&mut s, &mut k, "flipped");
    let v = volume(&mut s, &mut k);
    assert!(close(v, by_hand(12.0, 1.0), 2e-3 * v), "{v} {}", by_hand(12.0, 1.0));
}

/// The figure after "by up to" or "by less than" in a die-formed sheet's note.
fn said_figure(note: &str) -> f64 {
    let after = note.split("moves the pattern by ").nth(1).unwrap();
    let figure = after.trim_start_matches("up to ").trim_start_matches("less than ");
    figure.split(" mm").next().unwrap().parse().unwrap()
}

/// Against shapes whose flat is known exactly: the pattern is the sheet it should be, measured
/// square to it, and what the note says of how close it is holds.
#[test]
fn a_die_formed_pattern_is_the_known_flat_to_within_what_its_note_says() {
    // Two like Ls, 1 thick, bends of 1: 2 x 48 and the bend at the neutral radius, 80 wide.
    let (mut s, mut k) = start(1.0, 1.0);
    let ell = [(0.0, 0.0), (50.0, 0.0), (50.0, 50.0)];
    let a = sketch_at(&mut s, &mut k, "xy", 0.0);
    chain(&mut s, &mut k, a, &ell);
    let b = sketch_at(&mut s, &mut k, "xy", 80.0);
    chain(&mut s, &mut k, b, &ell);
    run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, b] }));
    let v = volume(&mut s, &mut k);
    let section = 2.0 * 48.0 + FRAC_PI_2 / 2.0 * (4.0 - 1.0);
    assert!(close(v, section * 80.0, 0.2), "{v} {}", section * 80.0);
    let p = flat(&mut s, &mut k);
    let long = 2.0 * 48.0 + FRAC_PI_2 * (1.0 + K);
    let off = (num(&p, "length") - long).abs().max((num(&p, "width") - 80.0).abs());
    assert!(off < 0.002, "{off}: {p}");
    assert!(off <= said_figure(p["notes"][0].as_str().unwrap()).max(0.001), "{off}: {p}");

    // A half cylinder of radius 40, the sheet inside it: as long as the neutral surface goes round.
    let (mut s, mut k) = start(1.0, 1.0);
    let round = |s: &mut Session, k: &mut OcctKernel, sk: u64| {
        let q = run(s, k, "sketch.arc", json!({ "sketch": sk, "cx": 0, "cy": 0, "x1": 40, "y1": 0, "x2": 0, "y2": 40 }));
        run(s, k, "sketch.arc", json!({ "sketch": sk, "cx": 0, "cy": 0, "start": q["end"], "x2": -40, "y2": 0 }));
    };
    let a = sketch_at(&mut s, &mut k, "xy", 0.0);
    round(&mut s, &mut k, a);
    let b = sketch_at(&mut s, &mut k, "xy", 30.0);
    round(&mut s, &mut k, b);
    run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, b] }));
    let v = volume(&mut s, &mut k);
    assert!(close(v, PI * 39.5 * 30.0, 2.0), "{v}");
    let p = flat(&mut s, &mut k);
    let off = (num(&p, "length") - PI * (39.0 + K)).abs();
    assert!(off < 0.02 && off <= said_figure(p["notes"][0].as_str().unwrap()) + 1e-3, "{off}: {p}");
    assert!(close(num(&p, "width"), 30.0, 0.01), "{p}");

    // A trapezoid in one plane: the two parallel rims along X, whichever is drawn first, and
    // the same box as the same part in facets.
    let (mut s, mut k) = start(1.0, 1.0);
    let a = sketch_at(&mut s, &mut k, "xy", 0.0);
    chain(&mut s, &mut k, a, &[(0.0, 0.0), (100.0, 0.0)]);
    let b = sketch_at(&mut s, &mut k, "xy", 80.0);
    chain(&mut s, &mut k, b, &[(20.0, 0.0), (80.0, 0.0)]);
    run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, b] }));
    assert!(close(volume(&mut s, &mut k), 6400.0, 1e-3));
    let p = flat(&mut s, &mut k);
    assert!(close(num(&p, "length"), 100.0, 1e-4) && close(num(&p, "width"), 80.0, 1e-4), "{p}");
    assert!(close(num(&p, "area"), 6400.0, 1e-3), "{p}");

    // The same across a tilted plane: the rims lie 100 long and 60 long, some way apart in space.
    let tilted = |forming: &str| {
        let (mut s, mut k) = start(1.0, 1.0);
        let a = sketch_at(&mut s, &mut k, "xy", 0.0);
        chain(&mut s, &mut k, a, &[(0.0, -50.0), (100.0, -50.0)]);
        let w = id(&run(&mut s, &mut k, "work.plane", json!({ "by": "angle", "base": "xy", "axis": "x", "angle": 1.0 })), "feature");
        let b = id(&run(&mut s, &mut k, "sketch.create", json!({ "work_plane": w, "project_origin": false })), "feature");
        chain(&mut s, &mut k, b, &[(20.0, 50.0), (80.0, 50.0)]);
        run(&mut s, &mut k, "sheet.lofted_flange", json!({ "sketches": [a, b], "forming": forming }));
        assert!(run(&mut s, &mut k, "model.regenerate", json!({}))["error"].is_null());
        let p = flat(&mut s, &mut k);
        (num(&p, "length"), num(&p, "width"), num(&p, "area"))
    };
    let (die, brake) = (tilted("die_formed"), tilted("press_brake"));
    assert!(close(die.0, brake.0, 1e-3) && close(die.1, brake.1, 1e-3) && close(die.2, brake.2, 1e-3), "{die:?} {brake:?}");
}

/// A round on a thickness edge of a die-formed plate takes (1 - pi / 4) r^2 x 1 off it, leaves
/// every face a proper area and nothing past the part's end. At some radii the kernel gives a
/// solid that passes every check with a face of negative area: that is refused, in words.
#[test]
fn a_rounded_corner_of_a_die_formed_sheet_measures_right_or_is_refused() {
    let mut refused_any = false;
    for radius in [0.5, 1.0, 2.0, 3.0, 3.98, 4.5, 5.0] {
        let (mut s, mut k) = start(1.0, 1.0);
        plate_between(&mut s, &mut k, "die_formed");
        let edge = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [0, 0.5, 80] }));
        run(&mut s, &mut k, "model.fillet", json!({ "edges": [edge], "radius": radius }));
        let r = run(&mut s, &mut k, "model.regenerate", json!({}));
        if let Some(e) = r["error"]["message"].as_str() {
            assert!((e.contains("cannot measure") && e.contains("different size")) || e.contains("could not be made"), "{radius}: {e}");
            refused_any = true;
            continue;
        }
        let mass = run(&mut s, &mut k, "model.mass", json!({}));
        let v = num(&mass["bodies"][0], "volume");
        // (A little apart from one build of the kernel to another: 0.063 of 8000 was seen on Linux.)
        assert!(close(v, 8000.0 - (1.0 - PI / 4.0) * radius * radius, 0.1), "{radius}: {v}");
        assert!(num(&mass["bodies"][0], "area") > 16_000.0, "{radius}: {mass}");
        let b = run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0]["bbox"].clone();
        assert!(b["max"][2].as_f64().unwrap() < 80.0 + 1e-3, "{radius}: {b}");
    }
    // (Which radii it is goes with the kernel's build, and not with anything here.)
    let _ = refused_any;
}

/// What a die-formed sheet's rim cannot take is said as what it is, even where it looks straight.
#[test]
fn a_flange_on_the_rim_of_a_die_formed_sheet_is_refused_for_what_it_is() {
    let (mut s, mut k) = start(1.0, 1.0);
    plate_between(&mut s, &mut k, "die_formed");
    let edge = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [50, 0, 80] }));
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "height": 20 }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("die-formed") && e.contains("Press Brake") && !e.contains("curved"), "{e}");
}
