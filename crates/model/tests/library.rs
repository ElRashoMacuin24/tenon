//! The standard parts library with the geometry kernel: every size of every part regenerates to
//! one valid solid of the volume its dimensions give, whether the part is taken from its first
//! size straight to that one (as an assembly does) or walked through the table row by row.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::PI;

use serde_json::{Value, json};
use tenon_kernel_occt::OcctKernel;
use tenon_model::{Session, library};

fn run(s: &mut Session, k: &mut OcctKernel, id: &str, p: Value) -> Value {
    s.exec(id, &p, Some(k)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// The volume a size's dimensions give (sections are 100 long).
fn volume_of(family: &str, v: &[f64]) -> f64 {
    let hexagon = |s: f64| 3f64.sqrt() / 2.0 * s * s;
    let disc = |d: f64| PI * d * d / 4.0;
    match family {
        "hex-screw-iso-4017" => hexagon(v[1]) * v[2] + disc(v[0]) * v[3],
        "socket-screw-iso-4762" => disc(v[1]) * v[0] - hexagon(v[2]) * v[3] + disc(v[0]) * v[4],
        "hex-nut-iso-4032" => (hexagon(v[2]) - disc(v[0] - 1.0825 * v[1])) * v[3],
        "washer-iso-7089" => (disc(v[1]) - disc(v[0])) * v[2],
        "ball-bearing-iso-15" => {
            let (d, big, b) = (v[0], v[1], v[2]);
            let ring = 0.3 * (big - d);
            (disc(big) - disc(d)) * b - 2.0 * (disc(big - ring) - disc(d + ring)) * 0.08 * b
        }
        "tube-rectangular" => (v[0] * v[1] - (v[0] - 2.0 * v[2]) * (v[1] - 2.0 * v[2])) * 100.0,
        "tube-round" => (disc(v[0]) - disc(v[0] - 2.0 * v[1])) * 100.0,
        "angle" => (v[0] * v[2] + (v[1] - v[2]) * v[2]) * 100.0,
        "flat-bar" => v[0] * v[1] * 100.0,
        "t-slot" => (v[0] * v[0] - 4.0 * (v[1] * v[2] + v[3] * v[4]) - disc(v[5])) * 100.0,
        other => panic!("no volume for {other}"),
    }
}

/// The part as it stands: one valid solid of `want` mm^3.
fn check(s: &mut Session, k: &mut OcctKernel, what: &str, want: f64) {
    let r = run(s, k, "model.regenerate", json!({}));
    assert!(r["error"].is_null(), "{what}: {r}");
    let topo = run(s, k, "model.topology", json!({}));
    assert_eq!(topo["bodies"].as_array().map(Vec::len), Some(1), "{what}: {topo}");
    assert!(topo["bodies"][0]["valid"] == true && topo["bodies"][0]["solids"] == 1, "{what}: {topo}");
    let v = run(s, k, "model.mass", json!({}))["bodies"][0]["volume"].as_f64().unwrap();
    assert!((v - want).abs() <= 1e-7 * want, "{what}: {v} mm^3, its dimensions give {want}");
}

#[test]
fn every_size_of_every_standard_part_is_one_solid_of_the_right_volume() {
    let mut k = OcctKernel::new();
    let mut sizes = 0;
    for f in library::families() {
        let doc = library::build(f.id).unwrap();
        let rows = f.rows();
        // Row by row through the table.
        let mut s = Session::default();
        s.replace_document(doc.clone(), Some(&mut k));
        for (name, values) in &rows {
            run(&mut s, &mut k, "table.activate", json!({ "row": name }));
            check(&mut s, &mut k, &format!("{} {name}", f.id), volume_of(f.id, values));
            sizes += 1;
        }
        // From the size it is built at straight to each of the others (every fourth is enough
        // to cover the small and the large).
        for (name, values) in rows.iter().step_by(4).chain(rows.last()) {
            let mut s = Session::default();
            s.replace_document(doc.clone(), Some(&mut k));
            run(&mut s, &mut k, "table.activate", json!({ "row": name }));
            check(&mut s, &mut k, &format!("{} straight to {name}", f.id), volume_of(f.id, values));
        }
    }
    assert!(sizes > 200, "{sizes} sizes");
}

#[test]
fn screws_and_nuts_carry_the_thread_their_size_names() {
    let mut k = OcctKernel::new();
    for (family, row, designation, internal) in [
        ("hex-screw-iso-4017", "M3x10", "M3x0.5", false),
        ("hex-screw-iso-4017", "M8x30", "M8x1.25", false),
        ("hex-screw-iso-4017", "M20x100", "M20x2.5", false),
        ("socket-screw-iso-4762", "M5x20", "M5x0.8", false),
        ("socket-screw-iso-4762", "M12x60", "M12x1.75", false),
        ("hex-nut-iso-4032", "M4", "M4x0.7", true),
        ("hex-nut-iso-4032", "M10", "M10x1.5", true),
        ("hex-nut-iso-4032", "M16", "M16x2", true),
    ] {
        let mut s = Session::default();
        s.replace_document(library::build(family).unwrap(), Some(&mut k));
        run(&mut s, &mut k, "table.activate", json!({ "row": row }));
        let t = run(&mut s, &mut k, "model.threads", json!({}));
        let thread = &t["threads"][0];
        assert_eq!(
            (thread["designation"].as_str(), thread["internal"].as_bool(), thread["modelled"].as_bool()),
            (Some(designation), Some(internal), Some(false)),
            "{family} {row}: {t}"
        );
    }
    // A screw's thread runs its whole length, and follows it.
    let mut s = Session::default();
    s.replace_document(library::build("hex-screw-iso-4017").unwrap(), Some(&mut k));
    for (row, length) in [("M8x16", 16.0), ("M8x80", 80.0)] {
        run(&mut s, &mut k, "table.activate", json!({ "row": row }));
        let t = run(&mut s, &mut k, "model.threads", json!({}));
        assert!((t["threads"][0]["length"].as_f64().unwrap() - length).abs() < 1e-9, "{row}: {t}");
    }
}

#[test]
fn a_standard_part_is_an_ordinary_part() {
    // It can be edited like any other: a section cut to length, a screw given a slot, and saved.
    let mut k = OcctKernel::new();
    let mut s = Session::default();
    s.replace_document(library::build("t-slot").unwrap(), Some(&mut k));
    run(&mut s, &mut k, "table.activate", json!({ "row": "30x30 slot 8" }));
    run(&mut s, &mut k, "param.set", json!({ "name": "L", "equation": "250 mm" }));
    let row = library::family("t-slot").unwrap().rows().into_iter().find(|r| r.0 == "30x30 slot 8").unwrap().1;
    check(&mut s, &mut k, "a 250 mm length of 30x30", volume_of("t-slot", &row) * 2.5);
    // The length is not a size: every row keeps it.
    run(&mut s, &mut k, "table.activate", json!({ "row": "20x20 slot 6" }));
    let row = library::family("t-slot").unwrap().rows().into_iter().find(|r| r.0 == "20x20 slot 6").unwrap().1;
    check(&mut s, &mut k, "a 250 mm length of 20x20", volume_of("t-slot", &row) * 2.5);
    // Its mass follows its material: aluminium, 2.7 g/cm^3.
    let m = run(&mut s, &mut k, "model.mass", json!({}));
    assert!(m["material"] == "Aluminium 6061" && (m["mass"].as_f64().unwrap() - volume_of("t-slot", &row) * 2.5 * 2.7 / 1000.0).abs() < 1e-6, "{m}");
    // Its parameters say what they are.
    let list = run(&mut s, &mut k, "param.list", json!({}));
    let side = list["user"].as_array().unwrap().iter().find(|p| p["name"] == "a").unwrap();
    assert_eq!(side["comment"], "side");
}
