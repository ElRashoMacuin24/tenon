//! Rip, Unfold and Refold: a closed shape opened and laid flat, bends flattened for work across
//! them and folded back, against lengths and volumes worked out by hand.
//!
//! The sheet is 2 mm thick, bends have an inside radius of 3 mm and the K factor is 0.44, unless
//! a test says otherwise. A bend that turns the sheet by an angle `a` is `a * 3.88` wide in the
//! flat, and laid flat it has `3.88 / 4` of the volume it had: its neutral surface is at 3.88 from
//! the axis, the middle of the sheet at 4.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use serde_json::{Value, json};
use tenon_geom::Vec3;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

const T: f64 = 2.0;
const R: f64 = 3.0;
const K: f64 = 0.44;
const RHO: f64 = R + K * T;

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
    (a - b).abs() <= 1e-7 * b.abs().max(1.0)
}

/// Close enough for what goes round a bend as points in the flat (every 0.005 mm of deflection).
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 2e-3 * b.abs().max(1.0)
}

/// The part regenerates without an error; its volume.
fn volume(s: &mut Session, k: &mut OcctKernel) -> f64 {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{}", r["error"]);
    run(s, k, "model.mass", json!({}))["bodies"].as_array().unwrap().iter().map(|b| b["volume"].as_f64().unwrap()).sum()
}

/// The message of the feature that fails.
fn failure(s: &mut Session, k: &mut OcctKernel) -> String {
    let r = run(s, k, "model.regenerate", json!({}));
    r["error"]["message"].as_str().unwrap_or_else(|| panic!("no feature failed: {r}")).to_owned()
}

fn sound(s: &mut Session, k: &mut OcctKernel) {
    let body = &run(s, k, "model.topology", json!({}))["bodies"][0];
    assert!(body["valid"] == true && body["solids"] == 1, "{body}");
}

/// The ends of the box round the part.
fn bbox(s: &mut Session, k: &mut OcctKernel) -> (Vec3, Vec3) {
    let b = &run(s, k, "model.topology", json!({}))["bodies"][0]["bbox"];
    let p = |v: &Value| Vec3::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap());
    (p(&b["min"]), p(&b["max"]))
}

fn flat(s: &mut Session, k: &mut OcctKernel) -> Value {
    run(s, k, "sheet.flat", json!({ "curves": true }))
}

/// A `w` x `h` face on the XY plane, a corner at the origin: its feature and its sketch's lines
/// (bottom, right, top, left).
fn plate(s: &mut Session, k: &mut OcctKernel, w: f64, h: f64) -> (u64, Vec<u64>) {
    run(s, k, "sheet.defaults", json!({ "thickness": T, "bend_radius": R, "k_factor": K }));
    let sk = id(&run(s, k, "sketch.create", json!({ "plane": "xy" })), "feature");
    let r = run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": w, "y2": h }));
    let lines = r["lines"].as_array().unwrap().iter().map(|l| l.as_u64().unwrap()).collect();
    let face = id(&run(s, k, "sheet.face", json!({ "sketch": sk })), "feature");
    (face, lines)
}

fn rim(s: &mut Session, k: &mut OcctKernel, face: u64, line: u64) -> Value {
    run(
        s,
        k,
        "model.edge_ref",
        json!({ "faces": [{ "type": "cap", "feature": face, "end": "end" }, { "type": "side", "feature": face, "curve": line }] }),
    )
}

fn face_at(s: &mut Session, k: &mut OcctKernel, at: [f64; 3]) -> Value {
    run(s, k, "model.face_ref", json!({ "at": at }))
}

/// An L: a plate 60 by 40 with a flange 20 high on its right-hand edge. The plate is cut back to
/// 55 long for the bend; the wall is 15 high above it.
fn bracket() -> (Session, OcctKernel, u64) {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
    let edge = rim(&mut s, &mut k, face, lines[1]);
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [edge], "height": 20 }));
    (s, k, face)
}

/// The volume a bend through `angle`, `width` long, has when it is folded.
fn folded_bend(angle: f64, width: f64) -> f64 {
    angle / 2.0 * ((R + T) * (R + T) - R * R) * width
}

const ALLOWANCE: f64 = FRAC_PI_2 * RHO;

/// The inside of the bend of the bracket, half way round it.
fn bracket_bend() -> [f64; 3] {
    let c = FRAC_PI_2 / 2.0;
    [55.0 + R * c.sin(), 20.0, 2.0 + R - R * c.cos()]
}

/// A sketch on the top of the sheet, over the plate.
fn sketch_on_top(s: &mut Session, k: &mut OcctKernel) -> u64 {
    let top = face_at(s, k, [10.0, 20.0, T]);
    id(&run(s, k, "sketch.create", json!({ "face": top })), "feature")
}

/// Where a point of the part, by its X and Y, is in a sketch.
fn in_sketch(s: &mut Session, k: &mut OcctKernel, sk: u64, at: (f64, f64)) -> (f64, f64) {
    let frame = s.regen(k).sketch_frames[&tenon_model::FeatureId(sk as u32)];
    let p = frame.to_local(Vec3::new(at.0, at.1, T));
    (p.x, p.y)
}

/// A round hole of radius `radius` through the flat sheet, drawn on its top.
fn hole(s: &mut Session, k: &mut OcctKernel, at: (f64, f64), radius: f64) -> u64 {
    let sk = sketch_on_top(s, k);
    let (x, y) = in_sketch(s, k, sk, at);
    run(s, k, "sketch.circle", json!({ "sketch": sk, "cx": x, "cy": y, "r": radius }));
    id(&run(s, k, "model.extrude", json!({ "sketch": sk, "distance": 10, "reverse": true, "operation": "cut" })), "feature")
}

/// How much of a circle of radius `r` lies within `half` of a line through its centre.
fn circle_within(r: f64, half: f64) -> f64 {
    if half >= r {
        return PI * r * r;
    }
    2.0 * (half * (r * r - half * half).sqrt() + r * r * (half / r).asin())
}

#[test]
fn unfold_lays_a_bend_flat_to_its_allowance_and_the_sheet_comes_round() {
    let (mut s, mut k, _) = bracket();
    let folded = 55.0 * 40.0 * T + folded_bend(FRAC_PI_2, 40.0) + 15.0 * 40.0 * T;
    assert!(near(volume(&mut s, &mut k), folded), "{}", volume(&mut s, &mut k));
    let pattern = flat(&mut s, &mut k);
    let long = 55.0 + ALLOWANCE + 15.0;
    assert!(near(num(&pattern, "length"), long), "{pattern}");

    let top = face_at(&mut s, &mut k, [10.0, 20.0, T]);
    let inside = face_at(&mut s, &mut k, bracket_bend());
    let unfold = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top, "bends": [inside] })), "feature");
    assert_eq!(s.document().feature(tenon_model::FeatureId(unfold as u32)).unwrap().name, "Unfold1");

    // The partly flat sheet: one plate, as long as the flat pattern says, where the plate was.
    let v = volume(&mut s, &mut k);
    assert!(near(v, long * 40.0 * T), "{v} {}", long * 40.0 * T);
    sound(&mut s, &mut k);
    let (lo, hi) = bbox(&mut s, &mut k);
    assert!(lo.near(Vec3::ZERO, 1e-6) && hi.near(Vec3::new(long, 40.0, T), 1e-6), "{lo:?} {hi:?}");
    // The flat pattern of the partly flat sheet is the pattern it always had, without a bend now.
    let now = flat(&mut s, &mut k);
    for key in ["length", "width", "area", "cut_length"] {
        assert!(near(num(&now, key), num(&pattern, key)), "{key}: {} {}", now[key], pattern[key]);
    }
    assert_eq!(now["bends"].as_array().unwrap().len(), 0, "{now}");
    // The strip is a face of its own, named by the unfold, for a refold to be told it by.
    let strip = face_at(&mut s, &mut k, [55.0 + ALLOWANCE / 2.0, 20.0, T]);
    assert!(strip["origin"]["type"] == "from" && strip["origin"]["feature"] == unfold, "{strip}");
    // The plate and the wall keep the names they had.
    let wall = face_at(&mut s, &mut k, [55.0 + ALLOWANCE + 7.0, 20.0, T]);
    assert_ne!(wall["origin"]["feature"], unfold, "{wall}");

    // Rolled back it is the bracket again, and undone.
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": unfold }));
    assert!(near(volume(&mut s, &mut k), folded));
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": unfold, "suppressed": false }));
    assert!(near(volume(&mut s, &mut k), long * 40.0 * T));
    assert!(s.undo() && s.undo() && s.undo());
    assert!(near(volume(&mut s, &mut k), folded));
}

#[test]
fn a_bend_is_laid_flat_from_either_side_and_from_either_of_its_faces() {
    // From the wall: the wall stays where it is, upright, and the plate comes up in its plane.
    let (mut s, mut k, _) = bracket();
    let long = 55.0 + ALLOWANCE + 15.0;
    let wall = face_at(&mut s, &mut k, [58.0, 20.0, 12.0]);
    let outside = {
        let c = FRAC_PI_2 / 2.0;
        face_at(&mut s, &mut k, [55.0 + (R + T) * c.sin(), 20.0, 2.0 + R - (R + T) * c.cos()])
    };
    run(&mut s, &mut k, "sheet.unfold", json!({ "face": wall, "bends": [outside] }));
    assert!(near(volume(&mut s, &mut k), long * 40.0 * T), "{}", volume(&mut s, &mut k));
    sound(&mut s, &mut k);
    let (lo, hi) = bbox(&mut s, &mut k);
    // The wall's top is at 20; from there the sheet runs straight down, 15 + the allowance + 55.
    assert!(lo.near(Vec3::new(58.0, 0.0, 20.0 - long), 1e-6) && hi.near(Vec3::new(60.0, 40.0, 20.0), 1e-6), "{lo:?} {hi:?}");
}

#[test]
fn every_bend_of_a_channel_is_laid_flat_when_none_is_named_and_one_when_it_is() {
    // A plate with a flange on each of two opposite edges.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
    let edges = [rim(&mut s, &mut k, face, lines[1]), rim(&mut s, &mut k, face, lines[3])];
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": edges, "height": 20 }));
    let long = 50.0 + 2.0 * (ALLOWANCE + 15.0);
    let pattern = flat(&mut s, &mut k);
    assert!(near(num(&pattern, "length"), long), "{pattern}");
    let top = face_at(&mut s, &mut k, [30.0, 20.0, T]);

    let right = face_at(&mut s, &mut k, bracket_bend());
    let one = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top.clone(), "bends": [right] })), "feature");
    let half = 50.0 + ALLOWANCE + 15.0;
    let by_hand = half * 40.0 * T + folded_bend(FRAC_PI_2, 40.0) + 15.0 * 40.0 * T;
    assert!(near(volume(&mut s, &mut k), by_hand), "{} {by_hand}", volume(&mut s, &mut k));
    let (lo, hi) = bbox(&mut s, &mut k);
    assert!(lo.near(Vec3::ZERO, 1e-6) && hi.near(Vec3::new(5.0 + half, 40.0, 20.0), 1e-6), "{lo:?} {hi:?}");
    assert!(s.undo());
    let _ = one;

    let all = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top })), "feature");
    assert!(near(volume(&mut s, &mut k), long * 40.0 * T), "{} {}", volume(&mut s, &mut k), long * 40.0 * T);
    sound(&mut s, &mut k);
    let (lo, hi) = bbox(&mut s, &mut k);
    // Out from the plate's two ends: 5 + the plate is where the plate was.
    assert!(
        lo.near(Vec3::new(5.0 - ALLOWANCE - 15.0, 0.0, 0.0), 1e-6) && hi.near(Vec3::new(55.0 + ALLOWANCE + 15.0, 40.0, T), 1e-6),
        "{lo:?} {hi:?}"
    );
    // All back again, in one refold, and it is the channel it was.
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": all }));
    let back = 50.0 * 40.0 * T + 2.0 * (folded_bend(FRAC_PI_2, 40.0) + 15.0 * 40.0 * T);
    assert!(near(volume(&mut s, &mut k), back), "{} {back}", volume(&mut s, &mut k));
    sound(&mut s, &mut k);
    let (lo, hi) = bbox(&mut s, &mut k);
    assert!(lo.near(Vec3::ZERO, 1e-6) && hi.near(Vec3::new(60.0, 40.0, 20.0), 1e-6), "{lo:?} {hi:?}");
}

#[test]
fn a_flat_sheet_or_a_face_that_is_no_bend_is_refused_in_words() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    plate(&mut s, &mut k, 60.0, 40.0);
    let top = face_at(&mut s, &mut k, [10.0, 20.0, T]);
    run(&mut s, &mut k, "sheet.unfold", json!({ "face": top.clone() }));
    assert!(failure(&mut s, &mut k).contains("has no bend to unfold"));
    assert!(s.undo());
    run(&mut s, &mut k, "sheet.unfold", json!({ "face": top.clone(), "bends": [top.clone()] }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("is not one") && e.contains("round face of a bend"), "{e}");
    assert!(s.undo());
    let side = face_at(&mut s, &mut k, [0.0, 20.0, 1.0]);
    run(&mut s, &mut k, "sheet.unfold", json!({ "face": side }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("stationary face") && e.contains("flat side of the sheet"), "{e}");
    assert!(s.undo());
    let e = refused(&mut s, &mut k, "sheet.refold", json!({}));
    assert!(e.contains("no unfold"), "{e}");
    let e = refused(&mut s, &mut k, "sheet.refold", json!({ "unfold": 1 }));
    assert!(e.contains("is not an unfold"), "{e}");
}

#[test]
fn a_hole_cut_across_a_flat_bend_goes_round_the_bend_when_it_is_folded_back() {
    let (mut s, mut k, _) = bracket();
    let folded = volume(&mut s, &mut k);
    let top = face_at(&mut s, &mut k, [10.0, 20.0, T]);
    let inside = face_at(&mut s, &mut k, bracket_bend());
    let unfold = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top, "bends": [inside] })), "feature");
    let long = 55.0 + ALLOWANCE + 15.0;
    let flat_whole = long * 40.0 * T;
    // A hole of radius 6, centred on the middle of the strip, which is less wide than that: it
    // takes some of the plate, all that it covers of the strip, and some of the wall.
    let radius = 6.0;
    hole(&mut s, &mut k, (55.0 + ALLOWANCE / 2.0, 20.0), radius);
    let circle = PI * radius * radius;
    assert!(near(volume(&mut s, &mut k), flat_whole - circle * T), "{} {}", volume(&mut s, &mut k), flat_whole - circle * T);
    let partly = flat(&mut s, &mut k);
    assert!(near(num(&partly, "area"), long * 40.0 - circle), "{partly}");
    assert_eq!(partly["holes"].as_u64(), Some(1));

    let refold = id(&run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold })), "feature");
    assert_eq!(s.document().feature(tenon_model::FeatureId(refold as u32)).unwrap().name, "Refold1");
    // Back as the bracket: what the hole takes of the flats is taken at the sheet's thickness; what
    // it takes of the strip goes round the bend, where sheet has 4 / 3.88 of its flat volume.
    let on_strip = circle_within(radius, ALLOWANCE / 2.0);
    let taken = (circle - on_strip) * T + on_strip * T * (R + T / 2.0) / RHO;
    let v = volume(&mut s, &mut k);
    assert!(near(v, folded - taken), "{v} {} (it takes {taken})", folded - taken);
    sound(&mut s, &mut k);
    let (lo, hi) = bbox(&mut s, &mut k);
    assert!(lo.near(Vec3::ZERO, 1e-6) && hi.near(Vec3::new(60.0, 40.0, 20.0), 1e-6), "{lo:?} {hi:?}");
    // It comes back on both faces: a hole in the plate's edge, and one in the wall's.
    let holes_at = |s: &mut Session, k: &mut OcctKernel, at: [f64; 3]| {
        let r = s.exec("model.face_ref", &json!({ "at": at }), Some(k));
        r.is_err()
    };
    assert!(holes_at(&mut s, &mut k, [54.0, 20.0, T]), "the plate is holed at its edge");
    assert!(!holes_at(&mut s, &mut k, [54.0, 30.0, T]), "and whole beside the hole");
    assert!(holes_at(&mut s, &mut k, [58.0, 20.0, 6.0]), "the wall is holed at its foot");
    assert!(!holes_at(&mut s, &mut k, [58.0, 20.0, 12.0]), "and whole above the hole");

    // The flat pattern of the result is the partly flat sheet.
    let again = flat(&mut s, &mut k);
    for key in ["length", "width"] {
        assert!(near(num(&again, key), num(&partly, key)), "{key}: {} {}", again[key], partly[key]);
    }
    for key in ["area", "cut_length"] {
        assert!(close(num(&again, key), num(&partly, key)), "{key}: {} {}", again[key], partly[key]);
    }
    assert_eq!(again["holes"].as_u64(), Some(1));
    // (The hole is wider than the bend, so the bend is in two pieces either side of it.)
    assert_eq!(again["bends"].as_array().unwrap().len(), 2);
    assert!(near(num(&again["bends"][0], "angle"), FRAC_PI_2) && near(num(&again["bends"][1], "angle"), FRAC_PI_2), "{again}");
    // What goes round the bend is still a circle in the flat.
    let circle_curves: Vec<&Value> = again["inner"][0].as_array().unwrap().iter().collect();
    assert_eq!(circle_curves.len(), 1, "{:?}", again["inner"]);
    assert!(circle_curves[0]["type"] == "circle" && close(num(circle_curves[0], "radius"), radius), "{:?}", circle_curves[0]);

    // Rolled back to before the refold it is the flat sheet, holed; a change of the thickness comes
    // through it all: the hole is still the hole, and the allowance the rule's.
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": refold }));
    assert!(near(volume(&mut s, &mut k), flat_whole - circle * T));
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": refold, "suppressed": false }));
    run(&mut s, &mut k, "sheet.defaults", json!({ "bend_radius": 4 }));
    let wider = FRAC_PI_2 * (4.0 + K * T);
    let later = flat(&mut s, &mut k);
    // The plate is cut back as far as ever (the outer faces stay), the wall is higher up, and the
    // strip is wider; the hole is where it was drawn, on the plate and the strip.
    let longer = 60.0 - (4.0 + T) + wider + 20.0 - (4.0 + T);
    assert!(near(num(&later, "length"), longer), "{later}");
    sound(&mut s, &mut k);
}

#[test]
fn a_hole_all_on_the_strip_is_a_hole_in_the_bend() {
    let (mut s, mut k, _) = bracket();
    let folded = volume(&mut s, &mut k);
    let top = face_at(&mut s, &mut k, [10.0, 20.0, T]);
    let inside = face_at(&mut s, &mut k, bracket_bend());
    let unfold = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top, "bends": [inside] })), "feature");
    let radius = 1.5;
    assert!(radius < ALLOWANCE / 2.0);
    hole(&mut s, &mut k, (55.0 + ALLOWANCE / 2.0, 12.0), radius);
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold }));
    let taken = PI * radius * radius * T * (R + T / 2.0) / RHO;
    let v = volume(&mut s, &mut k);
    assert!(near(v, folded - taken), "{v} {}", folded - taken);
    sound(&mut s, &mut k);
    let f = flat(&mut s, &mut k);
    assert_eq!(f["holes"].as_u64(), Some(1));
    let long = 55.0 + ALLOWANCE + 15.0;
    assert!(close(num(&f, "area"), long * 40.0 - PI * radius * radius), "{f}");
}

#[test]
fn of_two_bends_flattened_either_can_be_folded_back_first_and_the_other_after() {
    // A plate with a flange on each of two neighbouring ends of a strip: a U. Flattened one after
    // the other from the plate, and folded back one at a time by a click on each strip.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
    let edges = [rim(&mut s, &mut k, face, lines[1]), rim(&mut s, &mut k, face, lines[3])];
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": edges, "height": 20 }));
    let whole = volume(&mut s, &mut k);
    let top = face_at(&mut s, &mut k, [30.0, 20.0, T]);
    let right = face_at(&mut s, &mut k, bracket_bend());
    let left = {
        let c = FRAC_PI_2 / 2.0;
        face_at(&mut s, &mut k, [5.0 - R * c.sin(), 20.0, 2.0 + R - R * c.cos()])
    };
    let unfold = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top, "bends": [right, left] })), "feature");
    let long = 50.0 + 2.0 * (ALLOWANCE + 15.0);
    assert!(near(volume(&mut s, &mut k), long * 40.0 * T));
    // A slot across each strip, in one sketch: 8 by 30 round the middle of each.
    let strip_right = face_at(&mut s, &mut k, [55.0 + ALLOWANCE / 2.0, 20.0, T]);
    let strip_left = face_at(&mut s, &mut k, [5.0 - ALLOWANCE / 2.0, 20.0, T]);
    // The right one first, then the left.
    let first = id(&run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold, "bends": [strip_right] })), "feature");
    let half_right = folded_bend(FRAC_PI_2, 40.0) + 15.0 * 40.0 * T;
    let flat_left = (ALLOWANCE + 15.0) * 40.0 * T;
    assert!(near(volume(&mut s, &mut k), 50.0 * 40.0 * T + half_right + flat_left), "{}", volume(&mut s, &mut k));
    let (lo, hi) = bbox(&mut s, &mut k);
    assert!(lo.near(Vec3::new(5.0 - ALLOWANCE - 15.0, 0.0, 0.0), 1e-6) && hi.near(Vec3::new(60.0, 40.0, 20.0), 1e-6), "{lo:?} {hi:?}");
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold, "bends": [strip_left] }));
    assert!(near(volume(&mut s, &mut k), whole), "{} {whole}", volume(&mut s, &mut k));
    sound(&mut s, &mut k);
    let (lo, hi) = bbox(&mut s, &mut k);
    assert!(lo.near(Vec3::ZERO, 1e-6) && hi.near(Vec3::new(60.0, 40.0, 20.0), 1e-6), "{lo:?} {hi:?}");
    let _ = first;
    // A second refold of a bend that is folded back has nothing to do, and says so.
    let last = id(&run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold })), "feature");
    let e = failure(&mut s, &mut k);
    assert!(e.contains("no bend laid flat") && e.contains("folded back already"), "{e}");
    let _ = last;
}

#[test]
fn a_bend_laid_flat_with_another_beyond_it_carries_the_other_round_and_back() {
    // A Z: a plate, a wall up from it, and a flange out from the top of the wall.
    let (mut s, mut k, _) = bracket();
    let tip = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [60.0, 20.0, 20.0] }));
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [tip], "height": 10 }));
    let whole = volume(&mut s, &mut k);
    let pattern = flat(&mut s, &mut k);
    let top = face_at(&mut s, &mut k, [10.0, 20.0, T]);
    let first = face_at(&mut s, &mut k, bracket_bend());
    // Only the first bend: the wall and all beyond it comes round, and the second bend with it.
    let unfold = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top, "bends": [first] })), "feature");
    let v = volume(&mut s, &mut k);
    let gained = 40.0 * T * ALLOWANCE - folded_bend(FRAC_PI_2, 40.0);
    assert!(near(v, whole + gained), "{v} {}", whole + gained);
    let now = flat(&mut s, &mut k);
    assert!(near(num(&now, "length"), num(&pattern, "length")), "{now}");
    assert_eq!(now["bends"].as_array().unwrap().len(), 1);
    // And back: the second bend is where it was.
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold }));
    assert!(near(volume(&mut s, &mut k), whole), "{}", volume(&mut s, &mut k));
    let again = flat(&mut s, &mut k);
    assert!(near(num(&again, "length"), num(&pattern, "length")) && near(num(&again, "area"), num(&pattern, "area")), "{again}");
    assert_eq!(again["bends"].as_array().unwrap().len(), 2);

    // Two unfolds, the nearer last: the farther's strip moves with what the nearer brings round,
    // and a refold of the farther alone finds it where it now is.
    assert!(s.undo());
    let z_top = face_at(&mut s, &mut k, [10.0, 20.0, T]);
    assert!(s.undo());
    let second = {
        let c = FRAC_PI_2 / 2.0;
        // The second bend turns the other way round its own axis, at the wall's top: its outside
        // looks up and out from the corner.
        let _ = c;
        let faces = run(&mut s, &mut k, "model.faces", json!({}));
        let second =
            faces["faces"].as_array().unwrap().iter().find(|f| f["surface"]["Cylinder"].is_object() && f["centroid"][2].as_f64().unwrap() > 10.0);
        run(&mut s, &mut k, "model.face_ref", json!({ "face": second.expect("the second bend")["face"] }))
    };
    let far_first = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": z_top.clone(), "bends": [second] })), "feature");
    let first = face_at(&mut s, &mut k, bracket_bend());
    let near_last = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": z_top, "bends": [first] })), "feature");
    let flat_all = num(&pattern, "length") * 40.0 * T;
    assert!(near(volume(&mut s, &mut k), flat_all), "{} {flat_all}", volume(&mut s, &mut k));
    sound(&mut s, &mut k);
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": far_first }));
    let v = volume(&mut s, &mut k);
    let second_bend = folded_bend(FRAC_PI_2, 40.0) - 40.0 * T * ALLOWANCE;
    assert!(near(v, flat_all + second_bend), "{v} {}", flat_all + second_bend);
    sound(&mut s, &mut k);
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": near_last }));
    assert!(near(volume(&mut s, &mut k), whole), "{} {whole}", volume(&mut s, &mut k));
    sound(&mut s, &mut k);
    let again = flat(&mut s, &mut k);
    assert!(near(num(&again, "length"), num(&pattern, "length")) && near(num(&again, "area"), num(&pattern, "area")), "{again}");
}

#[test]
fn what_cannot_be_folded_back_is_said_in_words() {
    // A cut that takes all of the strip.
    let (mut s, mut k, _) = bracket();
    let top = face_at(&mut s, &mut k, [10.0, 20.0, T]);
    let inside = face_at(&mut s, &mut k, bracket_bend());
    let unfold = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top.clone(), "bends": [inside.clone()] })), "feature");
    let cut = |s: &mut Session, k: &mut OcctKernel, x1: f64, y1: f64, x2: f64, y2: f64, depth: f64| {
        let sk = sketch_on_top(s, k);
        let (a, b) = (in_sketch(s, k, sk, (x1, y1)), in_sketch(s, k, sk, (x2, y2)));
        run(s, k, "sketch.rectangle", json!({ "sketch": sk, "x1": a.0, "y1": a.1, "x2": b.0, "y2": b.1 }));
        id(&run(s, k, "model.extrude", json!({ "sketch": sk, "distance": depth, "reverse": true, "operation": "cut" })), "feature")
    };
    // Right across the strip: what is beyond would come away.
    let across = cut(&mut s, &mut k, 50.0, -1.0, 55.0 + ALLOWANCE + 5.0, 41.0, 10.0);
    let refold = id(&run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold })), "feature");
    let e = failure(&mut s, &mut k);
    assert!(e.contains("Nothing is left") && e.contains("flat strip"), "{e}");
    run(&mut s, &mut k, "feature.delete", json!({ "feature": refold }));
    run(&mut s, &mut k, "feature.delete", json!({ "feature": across }));
    assert!(volume(&mut s, &mut k) > 0.0);

    // A pocket in the strip that does not go through.
    let pocket = cut(&mut s, &mut k, 55.5, 10.0, 56.5, 30.0, 1.0);
    let refold = id(&run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold })), "feature");
    let e = failure(&mut s, &mut k);
    assert!(e.contains("not sheet of one thickness") && e.contains("cannot go round a bend"), "{e}");
    run(&mut s, &mut k, "feature.delete", json!({ "feature": refold }));
    run(&mut s, &mut k, "feature.delete", json!({ "feature": pocket }));

    // Sheet severed by a cut right along the far edge of the strip: nothing joins the wall, with a
    // strip left all the same.
    let severed = cut(&mut s, &mut k, 55.0 + ALLOWANCE * 0.5, -1.0, 55.0 + ALLOWANCE + 2.0, 41.0, 10.0);
    let refold = id(&run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold })), "feature");
    let e = failure(&mut s, &mut k);
    assert!(e.contains("does not hold together") && e.contains("no longer be joined"), "{e}");
    run(&mut s, &mut k, "feature.delete", json!({ "feature": refold }));
    run(&mut s, &mut k, "feature.delete", json!({ "feature": severed }));

    // A boss on the strip alone: only the sheet goes round a bend.
    let on_strip = face_at(&mut s, &mut k, [55.0 + ALLOWANCE / 2.0, 20.0, T]);
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "face": on_strip })), "feature");
    let at = in_sketch(&mut s, &mut k, sk, (55.0 + ALLOWANCE / 2.0, 20.0));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": at.0, "cy": at.1, "r": 0.5 }));
    let boss = id(&run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 5 })), "feature");
    let refold = id(&run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold })), "feature");
    let e = failure(&mut s, &mut k);
    assert!(e.contains("cannot be folded back") && e.contains("joined to its flat strip alone"), "{e}");
    run(&mut s, &mut k, "feature.delete", json!({ "feature": refold }));
    run(&mut s, &mut k, "feature.delete", json!({ "feature": boss }));
    run(&mut s, &mut k, "feature.delete", json!({ "feature": sk }));

    // Sheet added across the strip: its two sides are joined some other way.
    let sk = sketch_on_top(&mut s, &mut k);
    let (a, b) = (in_sketch(&mut s, &mut k, sk, (50.0, 40.0)), in_sketch(&mut s, &mut k, sk, (65.0, 45.0)));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": a.0, "y1": a.1, "x2": b.0, "y2": b.1 }));
    let added = id(&run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": T, "reverse": true })), "feature");
    assert!(volume(&mut s, &mut k) > 0.0);
    let refold = id(&run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold })), "feature");
    let e = failure(&mut s, &mut k);
    assert!(e.contains("something else joins the sheet"), "{e}");
    run(&mut s, &mut k, "feature.delete", json!({ "feature": refold }));
    run(&mut s, &mut k, "feature.delete", json!({ "feature": added }));

    // With its unfold suppressed there is nothing flat.
    let refold = id(&run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold })), "feature");
    assert!(volume(&mut s, &mut k) > 0.0);
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": unfold }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("no bend laid flat") && e.contains("suppressed"), "{e}");
    let _ = refold;
}

#[test]
fn a_tube_that_cannot_be_laid_flat_can_after_a_slit() {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": T, "bend_radius": R, "k_factor": K, "gap": 1 }));
    // A tube 50 long, 10 inside radius.
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 10 }));
    run(&mut s, &mut k, "sketch.circle", json!({ "sketch": sk, "cx": 0, "cy": 0, "r": 12 }));
    let tube = id(&run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 50 })), "feature");
    let whole = volume(&mut s, &mut k);
    assert!(near(whole, PI * (144.0 - 100.0) * 50.0), "{whole}");
    let e = refused(&mut s, &mut k, "sheet.flat", json!({}));
    assert!(e.contains("cannot be laid flat"), "{e}");

    let outside = face_at(&mut s, &mut k, [0.0, 12.0, 20.0]);
    let rip = id(&run(&mut s, &mut k, "sheet.rip", json!({ "face": outside, "at": [0, 12, 20] })), "feature");
    assert_eq!(s.document().feature(tenon_model::FeatureId(rip as u32)).unwrap().name, "Rip1");
    // The slit is as wide as the gap at the neutral surface, with its sides square to the sheet.
    let rho = 10.0 + K * T;
    let opened = 1.0 / rho;
    let by_hand = whole - opened / 2.0 * (144.0 - 100.0) * 50.0;
    let v = volume(&mut s, &mut k);
    assert!(near(v, by_hand), "{v} {by_hand}");
    sound(&mut s, &mut k);
    let f = flat(&mut s, &mut k);
    let long = TAU * rho - 1.0;
    assert!(near(num(&f, "length"), long) && near(num(&f, "width"), 50.0) && near(num(&f, "area"), long * 50.0), "{f}");
    assert_eq!(f["holes"].as_u64(), Some(0));
    let _ = tube;

    // It follows: a wider gap, a wider slit.
    run(&mut s, &mut k, "sheet.defaults", json!({ "gap": 3 }));
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), TAU * rho - 3.0), "{f}");
    // Its own gap, from the panel or a script, in place of the rule's.
    let mut kind = serde_json::to_value(&s.document().feature(tenon_model::FeatureId(rip as u32)).unwrap().kind).unwrap();
    kind["gap"] = json!(0.5);
    run(&mut s, &mut k, "feature.update", json!({ "feature": rip, "kind": kind }));
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), TAU * rho - 0.5), "{f}");
    // Suppressed it is the closed tube again.
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": rip }));
    assert!(refused(&mut s, &mut k, "sheet.flat", json!({})).contains("cannot be laid flat"));
    run(&mut s, &mut k, "feature.suppress", json!({ "feature": rip, "suppressed": false }));

    // Point to point along the tube, from end to end, is the same slit.
    assert!(s.undo() && s.undo() && s.undo() && s.undo() && s.undo());
    let inside = face_at(&mut s, &mut k, [0.0, 10.0, 20.0]);
    run(&mut s, &mut k, "sheet.rip", json!({ "face": inside, "way": "between", "from": [0, 10, 0], "to": [0, 10, 50], "gap": 2 }));
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "length"), TAU * rho - 2.0) && near(num(&f, "width"), 50.0), "{f}");
    sound(&mut s, &mut k);
    assert!(s.undo());
    // Stopping short of an end leaves it closed there.
    let inside = face_at(&mut s, &mut k, [0.0, 10.0, 20.0]);
    run(&mut s, &mut k, "sheet.rip", json!({ "face": inside, "way": "between", "from": [0, 10, 0], "to": [0, 10, 40], "gap": 2 }));
    let e = refused(&mut s, &mut k, "sheet.flat", json!({}));
    assert!(e.contains("closes on itself") && e.contains("open seam"), "{e}");
    assert!(s.undo());
    // What cannot be: no gap, the whole tube, a point off the face.
    let outside = face_at(&mut s, &mut k, [0.0, 12.0, 20.0]);
    run(&mut s, &mut k, "sheet.rip", json!({ "face": outside.clone(), "at": [0, 12, 20], "gap": 0 }));
    assert!(failure(&mut s, &mut k).contains("no gap opens nothing"));
    assert!(s.undo());
    run(&mut s, &mut k, "sheet.rip", json!({ "face": outside.clone(), "way": "face" }));
    assert!(failure(&mut s, &mut k).contains("whole of a tube"));
    assert!(s.undo());
    run(&mut s, &mut k, "sheet.rip", json!({ "face": outside.clone(), "at": [0, 12, 70] }));
    assert!(failure(&mut s, &mut k).contains("is not on the face"));
    assert!(s.undo());
    let e = refused(&mut s, &mut k, "sheet.rip", json!({ "face": outside.clone(), "way": "face", "gap": 1 }));
    assert!(e.contains("no gap of its own"), "{e}");
    let e = refused(&mut s, &mut k, "sheet.rip", json!({ "face": outside.clone(), "way": "slit", "at": [0, 12, 20] }));
    assert!(e.contains("`way` must be one of"), "{e}");
    let e = refused(&mut s, &mut k, "sheet.rip", json!({ "face": outside.clone(), "at": [0, 12, "x"] }));
    assert!(e.contains("`at`"), "{e}");
    // Nothing that is not a place reaches the kernel, however it comes.
    assert!(
        tenon_model::sheet::Rip {
            face: serde_json::from_value(outside).unwrap(),
            way: tenon_model::sheet::RipWay::Point { at: Vec3::new(f64::NAN, 0.0, 0.0), along: None },
            gap: None
        }
        .check()
        .is_err()
    );
}

/// A box 60 by 40 outside and 30 high, shelled 2 thick with its top and bottom taken away, its four
/// upright corners made bends of inside radius 3: a closed ring of sheet.
fn shelled_ring() -> (Session, OcctKernel, f64) {
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy" })), "feature");
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": 0, "y1": 0, "x2": 60, "y2": 40 }));
    let block = id(&run(&mut s, &mut k, "model.extrude", json!({ "sketch": sk, "distance": 30 })), "feature");
    let end = |e: &str| json!({ "type": "cap", "feature": block, "end": e });
    let (top, bottom) = (
        run(&mut s, &mut k, "model.face_ref", json!({ "origin": end("end") })),
        run(&mut s, &mut k, "model.face_ref", json!({ "origin": end("start") })),
    );
    run(&mut s, &mut k, "model.shell", json!({ "remove": [top, bottom], "thickness": T }));
    run(&mut s, &mut k, "sheet.defaults", json!({ "thickness": T, "bend_radius": R, "k_factor": K, "gap": 1 }));
    let corners: Vec<Value> = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)]
        .iter()
        .map(|(x, y)| run(&mut s, &mut k, "model.edge_ref", json!({ "at": [x, y, 15] })))
        .collect();
    run(&mut s, &mut k, "sheet.sharp_bend", json!({ "edges": corners }));
    let v = volume(&mut s, &mut k);
    // Each corner is cleared 5 back on either face, and has a quarter turn of bend.
    let by_hand = 2.0 * (50.0 + 30.0) * T * 30.0 + 4.0 * folded_bend(FRAC_PI_2, 30.0);
    assert!(near(v, by_hand), "{v} {by_hand}");
    (s, k, v)
}

#[test]
fn a_shelled_box_that_closes_on_itself_is_laid_flat_after_a_rip() {
    let (mut s, mut k, whole) = shelled_ring();
    let e = refused(&mut s, &mut k, "sheet.flat", json!({}));
    assert!(e.contains("closes on itself") && e.contains("open seam"), "{e}");
    // One bend cannot be laid flat from a wall either: the ring holds it.
    let front = face_at(&mut s, &mut k, [30.0, 0.0, 15.0]);
    let corner = {
        let c = FRAC_PI_2 / 2.0;
        face_at(&mut s, &mut k, [5.0 - (R + T) * c.sin(), 5.0 - (R + T) * c.cos(), 15.0])
    };
    run(&mut s, &mut k, "sheet.unfold", json!({ "face": front.clone(), "bends": [corner.clone()] }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("closes round on itself") && e.contains("Rip"), "{e}");
    assert!(s.undo());

    // A slit across the front wall, top to bottom, through a point of it: its flat length is
    // the round of the neutral surface, less the gap.
    run(&mut s, &mut k, "sheet.rip", json!({ "face": front.clone(), "at": [30, 0, 15] }));
    let v = volume(&mut s, &mut k);
    assert!(near(v, whole - 1.0 * T * 30.0), "{v} {}", whole - 1.0 * T * 30.0);
    sound(&mut s, &mut k);
    let f = flat(&mut s, &mut k);
    let long = 2.0 * (50.0 + 30.0) + 4.0 * ALLOWANCE - 1.0;
    assert!(near(num(&f, "length"), long) && near(num(&f, "width"), 30.0) && near(num(&f, "area"), long * 30.0), "{f}");
    assert_eq!(f["bends"].as_array().unwrap().len(), 4);

    // And so Unfold can lay every bend flat now: one strip of sheet, from what was the back wall.
    let back = face_at(&mut s, &mut k, [30.0, 40.0, 15.0]);
    run(&mut s, &mut k, "sheet.unfold", json!({ "face": back }));
    assert!(near(volume(&mut s, &mut k), long * 30.0 * T), "{}", volume(&mut s, &mut k));
    sound(&mut s, &mut k);
    let (lo, hi) = bbox(&mut s, &mut k);
    assert!(near(hi.x - lo.x, long) && near(hi.y, 40.0) && near(lo.y, 38.0) && near(hi.z - lo.z, 30.0), "{lo:?} {hi:?}");
    assert!(s.undo() && s.undo());

    // The whole of a bend taken out: one corner open, three bends left.
    let rip_bend = id(&run(&mut s, &mut k, "sheet.rip", json!({ "face": corner, "way": "face" })), "feature");
    let v = volume(&mut s, &mut k);
    assert!(near(v, whole - folded_bend(FRAC_PI_2, 30.0)), "{v}");
    sound(&mut s, &mut k);
    let f = flat(&mut s, &mut k);
    let long = 2.0 * (50.0 + 30.0) + 3.0 * ALLOWANCE;
    assert!(near(num(&f, "length"), long) && near(num(&f, "area"), long * 30.0), "{f}");
    assert_eq!(f["bends"].as_array().unwrap().len(), 3);
    assert!(s.undo());
    let _ = rip_bend;

    // A point-to-point slit, from one rim to the other at a slant: a slot across the strip, and
    // what it takes is its length by the gap.
    run(&mut s, &mut k, "sheet.rip", json!({ "face": front.clone(), "way": "between", "from": [20, 0, 0], "to": [30, 0, 30] , "gap": 1 }));
    let f = flat(&mut s, &mut k);
    sound(&mut s, &mut k);
    // Its sides run the length of the slant, so it is 1 wide square to that: in the flat, 1 / cos
    // along the strip. (10 across for 30 up.)
    let along = 1.0 * (900f64 + 100.0).sqrt() / 30.0;
    let round = 2.0 * (50.0 + 30.0) + 4.0 * ALLOWANCE;
    assert!(near(num(&f, "area"), (round - along) * 30.0), "{} {}", num(&f, "area"), (round - along) * 30.0);
    assert!(s.undo());
    // Point to point, stopping short: the wall is slit and the ring still closed.
    run(&mut s, &mut k, "sheet.rip", json!({ "face": front.clone(), "way": "between", "from": [30, 0, 0], "to": [30, 0, 20], "gap": 1 }));
    let v = volume(&mut s, &mut k);
    assert!(near(v, whole - 1.0 * T * 20.0), "{v}");
    assert!(refused(&mut s, &mut k, "sheet.flat", json!({})).contains("closes on itself"));
    assert!(s.undo());
    // Alongside an edge the panel clicks: an edge across the wall runs a slit round it instead,
    // which leaves the ring whole and in two rings.
    let rim_edge = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [30, 0, 30] }));
    run(&mut s, &mut k, "sheet.rip", json!({ "face": front, "at": [30, 0, 15], "along": rim_edge }));
    let v = volume(&mut s, &mut k);
    assert!(near(v, whole - 1.0 * T * 50.0), "{v} {}", whole - 1.0 * T * 50.0);
}

#[test]
fn a_rip_and_what_it_cannot_be() {
    // On a plate, a slit across its short way through a point of it; a whole face out; the
    // rim of a sheet is no place for one.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    plate(&mut s, &mut k, 60.0, 40.0);
    run(&mut s, &mut k, "sheet.defaults", json!({ "gap": 1 }));
    let top = face_at(&mut s, &mut k, [10.0, 20.0, T]);
    run(&mut s, &mut k, "sheet.rip", json!({ "face": top.clone(), "at": [10, 20, 2] }));
    // It cuts the plate in two: 40 long, 1 wide.
    let v = volume(&mut s, &mut k);
    assert!(near(v, (60.0 * 40.0 - 40.0) * T), "{v}");
    assert_eq!(run(&mut s, &mut k, "model.topology", json!({}))["bodies"][0]["solids"], 2);
    let e = refused(&mut s, &mut k, "sheet.flat", json!({}));
    assert!(e.contains("is of one solid") && e.contains("has 2"), "{e}");
    assert!(s.undo());
    let rim_face = face_at(&mut s, &mut k, [0.0, 20.0, 1.0]);
    run(&mut s, &mut k, "sheet.rip", json!({ "face": rim_face, "at": [0, 20, 1] }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("rim of the sheet"), "{e}");
    assert!(s.undo());
    run(&mut s, &mut k, "sheet.rip", json!({ "face": top.clone(), "at": [100, 20, 2] }));
    assert!(failure(&mut s, &mut k).contains("is not on the face"));
    assert!(s.undo());
    run(&mut s, &mut k, "sheet.rip", json!({ "face": top.clone(), "way": "between", "from": [10, 20, 2], "to": [10, 20, 2] }));
    assert!(failure(&mut s, &mut k).contains("are one point"));
    assert!(s.undo());
    // A slit from the rim into the middle: a notch, past the rim.
    run(&mut s, &mut k, "sheet.rip", json!({ "face": top.clone(), "way": "between", "from": [30, 0, 2], "to": [30, 10, 2], "gap": 2 }));
    assert!(near(volume(&mut s, &mut k), (60.0 * 40.0 - 20.0) * T), "{}", volume(&mut s, &mut k));
    sound(&mut s, &mut k);
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "cut_length"), 200.0 + 20.0), "{f}");
    assert!(s.undo());
    // The whole face out takes all of a plate, and that is refused.
    run(&mut s, &mut k, "sheet.rip", json!({ "face": top, "way": "face" }));
    let e = failure(&mut s, &mut k);
    assert!(e.contains("leaves nothing") || e.contains("could not be"), "{e}");
    assert!(s.undo());
    // Not yet sheet metal.
    let (mut p, mut q) = (Session::default(), OcctKernel::new());
    let e = refused(&mut p, &mut q, "sheet.rip", json!({ "face": {}, "at": [0, 0, 0] }));
    assert!(e.contains("not a sheet metal part yet"), "{e}");
    assert!(refused(&mut p, &mut q, "sheet.unfold", json!({ "face": {} })).contains("not a sheet metal part yet"));
    assert!(refused(&mut p, &mut q, "sheet.refold", json!({})).contains("not a sheet metal part yet"));
    let _ = k.live_shapes();
}

#[test]
fn a_tray_with_relieved_corners_is_laid_flat_whole_and_folded_back() {
    // Four flanges mitred at the corners, each corner with a round relief: what is laid flat
    // and rolled up again has arcs in its outline.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
    let edges: Vec<Value> = lines.iter().map(|l| rim(&mut s, &mut k, face, *l)).collect();
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": edges, "height": 20 }));
    let whole = volume(&mut s, &mut k);
    let pattern = flat(&mut s, &mut k);
    let floor = face_at(&mut s, &mut k, [30.0, 20.0, T]);
    let unfold = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": floor })), "feature");
    let v = volume(&mut s, &mut k);
    assert!(close(v, num(&pattern, "area") * T), "{v} {}", num(&pattern, "area") * T);
    sound(&mut s, &mut k);
    let (lo, hi) = bbox(&mut s, &mut k);
    assert!(near(hi.z - lo.z, T) && close(hi.x - lo.x, num(&pattern, "length")) && close(hi.y - lo.y, num(&pattern, "width")), "{lo:?} {hi:?}");
    let laid = flat(&mut s, &mut k);
    for key in ["area", "cut_length", "length", "width"] {
        assert!(close(num(&laid, key), num(&pattern, key)), "{key}: {} {}", laid[key], pattern[key]);
    }
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold }));
    let back = volume(&mut s, &mut k);
    assert!(close(back, whole), "{back} {whole}");
    sound(&mut s, &mut k);
    let (lo, hi) = bbox(&mut s, &mut k);
    assert!(lo.near(Vec3::ZERO, 1e-5) && hi.near(Vec3::new(60.0, 40.0, 20.0), 1e-5), "{lo:?} {hi:?}");
    let again = flat(&mut s, &mut k);
    for key in ["area", "cut_length", "length", "width"] {
        assert!(close(num(&again, key), num(&pattern, key)), "{key}: {} {}", again[key], pattern[key]);
    }
    assert_eq!(again["bends"].as_array().unwrap().len(), 4, "{again}");
}

#[test]
fn a_fold_a_rolled_hem_and_a_cut_across_a_bend_are_laid_flat_too() {
    // A fold: the two faces it made have one name between them, and only one of them is what stays.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, _) = plate(&mut s, &mut k, 80.0, 40.0);
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "plane": "xy", "project_origin": false })), "feature");
    run(&mut s, &mut k, "sketch.line", json!({ "sketch": sk, "x1": 50, "y1": 0, "x2": 50, "y2": 40 }));
    run(&mut s, &mut k, "sheet.fold", json!({ "sketch": sk, "location": "start" }));
    let whole = volume(&mut s, &mut k);
    let pattern = flat(&mut s, &mut k);
    assert!(near(num(&pattern, "length"), 80.0), "{pattern}");
    let bend = run(&mut s, &mut k, "model.faces", json!({}))["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["surface"]["Cylinder"].is_object())
        .unwrap()["face"]
        .clone();
    let bend = run(&mut s, &mut k, "model.face_ref", json!({ "face": bend }));
    let top = run(&mut s, &mut k, "model.find", json!({ "what": "faces", "kind": "plane", "facing": "bottom" }))["found"][0]["reference"].clone();
    let _ = face;
    let unfold = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top, "bends": [bend] })), "feature");
    assert!(near(volume(&mut s, &mut k), 80.0 * 40.0 * T), "{}", volume(&mut s, &mut k));
    sound(&mut s, &mut k);
    let (lo, hi) = bbox(&mut s, &mut k);
    assert!(lo.near(Vec3::ZERO, 1e-6) && hi.near(Vec3::new(80.0, 40.0, T), 1e-6), "{lo:?} {hi:?}");
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold }));
    assert!(near(volume(&mut s, &mut k), whole), "{} {whole}", volume(&mut s, &mut k));
    sound(&mut s, &mut k);

    // A rolled hem has nothing beyond its bend: it is laid flat as a strip, and rolled up again.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let (face, lines) = plate(&mut s, &mut k, 60.0, 40.0);
    let edge = rim(&mut s, &mut k, face, lines[1]);
    let turn = 270f64.to_radians();
    run(&mut s, &mut k, "sheet.hem", json!({ "edges": [edge], "type": "rolled", "radius": 3, "angle": turn }));
    let whole = volume(&mut s, &mut k);
    let top = face_at(&mut s, &mut k, [10.0, 20.0, T]);
    let unfold = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top })), "feature");
    let long = 60.0 + turn * RHO;
    assert!(near(volume(&mut s, &mut k), long * 40.0 * T), "{} {}", volume(&mut s, &mut k), long * 40.0 * T);
    sound(&mut s, &mut k);
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold }));
    assert!(near(volume(&mut s, &mut k), whole), "{} {whole}", volume(&mut s, &mut k));
    sound(&mut s, &mut k);

    // A slot cut across a bend while it was folded: laid flat, it is the slot that was drawn.
    let (mut s, mut k, plate_face) = bracket();
    let whole = volume(&mut s, &mut k);
    let on_top = run(&mut s, &mut k, "model.face_ref", json!({ "origin": { "type": "cap", "feature": plate_face, "end": "end" } }));
    let sk = id(&run(&mut s, &mut k, "sketch.create", json!({ "face": on_top })), "feature");
    let (a, b) = (in_sketch(&mut s, &mut k, sk, (50.0, 16.0)), in_sketch(&mut s, &mut k, sk, (55.0 + ALLOWANCE + 4.0, 24.0)));
    run(&mut s, &mut k, "sketch.rectangle", json!({ "sketch": sk, "x1": a.0, "y1": a.1, "x2": b.0, "y2": b.1 }));
    run(&mut s, &mut k, "sheet.cut_across_bend", json!({ "sketch": sk }));
    let slot = 8.0 * 5.0 * T + folded_bend(FRAC_PI_2, 8.0) + 8.0 * 4.0 * T;
    assert!(near(volume(&mut s, &mut k), whole - slot), "{}", volume(&mut s, &mut k));
    let top = face_at(&mut s, &mut k, [10.0, 20.0, T]);
    let unfold = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top })), "feature");
    let long = 55.0 + ALLOWANCE + 15.0;
    let v = volume(&mut s, &mut k);
    assert!(near(v, (long * 40.0 - 8.0 * (9.0 + ALLOWANCE)) * T), "{v}");
    sound(&mut s, &mut k);
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold }));
    assert!(near(volume(&mut s, &mut k), whole - slot), "{} {}", volume(&mut s, &mut k), whole - slot);
    sound(&mut s, &mut k);
    let f = flat(&mut s, &mut k);
    assert!(near(num(&f, "area"), long * 40.0 - 8.0 * (9.0 + ALLOWANCE)), "{f}");
}

#[test]
fn work_can_go_on_after_a_refold_and_refer_to_the_bend_as_it_was() {
    // The bend's round faces are called again what they were before it was laid flat.
    let (mut s, mut k, _) = bracket();
    let inside = face_at(&mut s, &mut k, bracket_bend());
    let top = face_at(&mut s, &mut k, [10.0, 20.0, T]);
    let unfold = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top, "bends": [inside.clone()] })), "feature");
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold }));
    let again = face_at(&mut s, &mut k, bracket_bend());
    assert_eq!(again["origin"], inside["origin"]);
    // And a flange goes on the wall's edge as ever.
    let tip = run(&mut s, &mut k, "model.edge_ref", json!({ "at": [60.0, 20.0, 20.0] }));
    run(&mut s, &mut k, "sheet.flange", json!({ "edges": [tip], "height": 10 }));
    assert!(volume(&mut s, &mut k) > 0.0);
    sound(&mut s, &mut k);
}

#[test]
fn the_kernel_keeps_no_shapes_of_a_failed_refold() {
    let (mut s, mut k, _) = bracket();
    let top = face_at(&mut s, &mut k, [10.0, 20.0, T]);
    let inside = face_at(&mut s, &mut k, bracket_bend());
    let unfold = id(&run(&mut s, &mut k, "sheet.unfold", json!({ "face": top, "bends": [inside] })), "feature");
    hole(&mut s, &mut k, (55.0 + ALLOWANCE / 2.0, 20.0), 6.0);
    run(&mut s, &mut k, "sheet.refold", json!({ "unfold": unfold }));
    let v = volume(&mut s, &mut k);
    let before = k.live_shapes();
    for _ in 0..3 {
        run(&mut s, &mut k, "feature.suppress", json!({ "feature": unfold + 3 }));
        run(&mut s, &mut k, "model.regenerate", json!({}));
        run(&mut s, &mut k, "feature.suppress", json!({ "feature": unfold + 3, "suppressed": false }));
        assert!(near(volume(&mut s, &mut k), v));
    }
    let after = k.live_shapes();
    assert!(after <= before + 8, "{before} {after}");
}
