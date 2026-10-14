//! M11: studies worked out by the real mesher and solver, against cases whose answers are in
//! the textbooks. These are the checks that the whole chain is right: the part as STEP, the
//! mesh read back, its faces told apart, supports and loads put on nodes, the solver's input
//! and its results.
//!
//! They need Gmsh and CalculiX (`pixi install -e fea`, docs/stress.md). Without them they say
//! so and pass, unless `TENON_FEA_REQUIRED` is set, as the gate (`cargo xtask ci`) sets it:
//! then their absence is a failure.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use tenon_fea::Tools;
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;

/// True when the programs are there. When they are not: a failure where they are required,
/// and otherwise a line saying that the test did not run.
fn programs(test: &str) -> bool {
    match Tools::find() {
        Ok(_) => true,
        Err(why) => {
            let required = std::env::var("TENON_FEA_REQUIRED").is_ok_and(|v| !v.is_empty() && v != "0");
            assert!(!required, "{test} needs the mesher and the solver, and they are required here: {why}");
            eprintln!("NOT RUN: {test}: {why}");
            false
        }
    }
}

struct Part {
    s: Session,
    k: OcctKernel,
}

impl Part {
    fn new() -> Part {
        Part { s: Session::default(), k: OcctKernel::new() }
    }
    fn run(&mut self, id: &str, p: Value) -> Value {
        tenon_io::cmd::run(&mut self.s, id, &p, Some(&mut self.k as &mut dyn Kernel)).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }
    fn refused(&mut self, id: &str, p: Value) -> String {
        tenon_io::cmd::run(&mut self.s, id, &p, Some(&mut self.k as &mut dyn Kernel)).expect_err(&format!("{id} {p} should be refused")).0
    }
    /// The reference of the one face found.
    fn face(&mut self, with: Value) -> Value {
        let mut with = with;
        with["what"] = json!("faces");
        let found = self.run("model.find", with.clone());
        assert_eq!(found["count"], 1, "{with}: {found}");
        found["found"][0]["reference"].clone()
    }
    fn facing(&mut self, way: &str) -> Value {
        self.face(json!({ "facing": way }))
    }
}

/// Within `percent` of `want`.
fn close(got: f64, want: f64, percent: f64) -> bool {
    (got - want).abs() <= want.abs() * percent / 100.0
}

const STEEL: (f64, f64) = (200_000.0, 0.29);

#[test]
fn a_bar_pulled_along_its_length_stretches_as_hookes_law_says() {
    if !programs("a bar in tension") {
        return;
    }
    // A bar 100 x 10 x 10 of mild steel, free to slide on three faces at right angles (which
    // holds it fast without pinching it anywhere) and pulled by 1000 N on its far end.
    let mut p = Part::new();
    p.run("model.box", json!({ "width": 100, "depth": 10, "height": 10 }));
    p.run("document.material", json!({ "name": "Steel, mild" }));
    let (left, front, bottom, right) = (p.facing("left"), p.facing("front"), p.facing("bottom"), p.facing("right"));
    p.run("study.new", json!({}));
    p.run("study.frictionless", json!({ "faces": [left, front, bottom] }));
    p.run("study.force", json!({ "faces": [right], "newtons": -1000 }));
    let r = p.run("study.solve", json!({}));

    // The same stress everywhere: force over area, 10 MPa. It lengthens by FL/AE and thins by
    // Poisson's ratio: the far corner moves most.
    let (e, nu) = STEEL;
    let stress = 1000.0 / 100.0;
    let (longer, thinner) = (stress / e * 100.0, nu * stress / e * 10.0);
    let most = (longer * longer + 2.0 * thinner * thinner).sqrt();
    let (vm, moved) = (r["max_von_mises"]["megapascals"].as_f64().unwrap(), r["max_displacement"]["millimetres"].as_f64().unwrap());
    assert!(close(vm, stress, 0.1), "von Mises {vm} MPa, and force over area is {stress}: {r}");
    assert!(close(moved, most, 0.1), "moved {moved} mm, and Hooke's law gives {most}: {r}");
    let far = r["max_displacement"]["at"].as_array().unwrap();
    assert_eq!((far[0].as_f64(), far[1].as_f64(), far[2].as_f64()), (Some(100.0), Some(10.0), Some(10.0)), "the far corner: {r}");
    // Mild steel yields at 250 MPa: it holds 25 times over.
    assert!(close(r["safety_factor"].as_f64().unwrap(), 25.0, 0.1) && r["holds"] == true && r["yield_strength"] == 250.0, "{r}");
    assert!(r["nodes"].as_u64().unwrap() > 100 && r["elements"].as_u64().unwrap() > 20, "{r}");
    assert_eq!(r["note"], tenon_io::stress::NOTE);
    assert!(r["element_size"].as_f64().is_some_and(|h| close(h, 101.0f64.sqrt() * 10.0 / 20.0, 1.0)), "a twentieth of the bar's diagonal: {r}");
}

#[test]
fn a_cantilever_bends_as_beam_theory_says() {
    if !programs("a cantilever") {
        return;
    }
    // The same bar held fast at one end, with 100 N down on the other.
    let mut p = Part::new();
    p.run("model.box", json!({ "width": 100, "depth": 10, "height": 10 }));
    p.run("document.material", json!({ "name": "Steel, mild" }));
    let (left, right) = (p.facing("left"), p.facing("right"));
    p.run("study.new", json!({}));
    p.run("study.fix", json!({ "faces": [left] }));
    p.run("study.force", json!({ "faces": [right], "newtons": 100, "direction": "down" }));
    p.run("study.mesh_size", json!({ "size": 2.5 }));
    let r = p.run("study.solve", json!({}));

    // Its end drops by FL^3/3EI, and a little more for shear (Timoshenko).
    let (e, nu) = STEEL;
    let (f, l, b, h) = (100.0, 100.0, 10.0, 10.0);
    let inertia = b * h * h * h / 12.0;
    let bending = f * l * l * l / (3.0 * e * inertia);
    let shear = f * l / (5.0 / 6.0 * (e / (2.0 * (1.0 + nu))) * b * h);
    let moved = r["max_displacement"]["millimetres"].as_f64().unwrap();
    assert!(close(moved, bending + shear, 1.5), "its end dropped {moved} mm, and beam theory gives {} (+ {} for shear): {r}", bending, shear);
    assert!((r["max_displacement"]["at"][0].as_f64().unwrap() - 100.0).abs() < 1e-6, "at its free end: {r}");
    // The stress is most at the root, where bending alone would give 6FL/bh^2 = 60 MPa; a
    // corner held fast adds to it (the more, the finer the mesh), so only that it is there
    // and at least that.
    let (vm, at) = (r["max_von_mises"]["megapascals"].as_f64().unwrap(), r["max_von_mises"]["at"][0].as_f64().unwrap());
    let root = 6.0 * f * l / (b * h * h);
    assert!(vm > root * 0.95 && vm < root * 2.0 && at < 5.0, "{vm} MPa at x = {at}; bending gives {root} at the root: {r}");
    assert!(r["safety_factor"].as_f64().unwrap() < 250.0 / (root * 0.95) && r["holds"] == true, "{r}");

    // Worked out again it is the same to the last figure: the same part gives the same mesh
    // and the same answer, every time.
    let again = p.run("study.solve", json!({}));
    assert_eq!((&again["nodes"], &again["max_displacement"], &again["max_von_mises"]), (&r["nodes"], &r["max_displacement"], &r["max_von_mises"]));

    // Twice the load, twice the drop: it is linear.
    p.run("study.set", json!({ "item": 2, "newtons": 200 }));
    let twice = p.run("study.solve", json!({}))["max_displacement"]["millimetres"].as_f64().unwrap();
    assert!(close(twice, 2.0 * moved, 0.01), "{twice} for twice the load, {moved} for once");
}

#[test]
fn a_thick_tube_under_pressure_is_stressed_as_lame_found() {
    if !programs("a thick-walled tube") {
        return;
    }
    // A quarter of a tube (bore 20, outside 40, 20 long): free to slide on its two cut faces
    // and on both ends, so that it is as a whole tube that cannot lengthen. 10 MPa inside.
    let mut p = Part::new();
    p.run("model.cylinder", json!({ "diameter": 40, "height": 20 }));
    p.run("model.cylinder", json!({ "diameter": 20, "height": 20, "operation": "cut" }));
    p.run("model.box", json!({ "x": -20, "y": -20, "width": 20, "depth": 40, "height": 20, "operation": "cut" }));
    p.run("model.box", json!({ "x": 0, "y": -20, "width": 20, "depth": 20, "height": 20, "operation": "cut" }));
    p.run("document.material", json!({ "name": "Steel, mild" }));
    let ends_and_cuts = [p.facing("left"), p.facing("front"), p.facing("bottom"), p.facing("top")];
    let bore = p.face(json!({ "kind": "cylinder", "radius": 10 }));
    p.run("study.new", json!({}));
    p.run("study.frictionless", json!({ "faces": ends_and_cuts }));
    p.run("study.pressure", json!({ "faces": [bore], "megapascals": 10 }));
    p.run("study.mesh_size", json!({ "size": 1.5 }));
    let r = p.run("study.solve", json!({}));

    // Lame's solution, the tube kept from lengthening. At the bore: round it, p(b^2 + a^2)/
    // (b^2 - a^2); across it, -p; along it, Poisson's ratio times their sum.
    let (e, nu) = STEEL;
    let (a, b, pressure) = (10.0f64, 20.0f64, 10.0);
    let k = pressure * a * a / (b * b - a * a);
    let (hoop, radial) = (k * (1.0 + b * b / (a * a)), -pressure);
    let axial = nu * (hoop + radial);
    let von_mises = (0.5 * ((hoop - radial).powi(2) + (radial - axial).powi(2) + (axial - hoop).powi(2))).sqrt();
    let grows = a / e * (hoop - nu * (radial + axial));
    let (vm, moved) = (r["max_von_mises"]["megapascals"].as_f64().unwrap(), r["max_displacement"]["millimetres"].as_f64().unwrap());
    assert!(close(vm, von_mises, 2.0), "von Mises {vm} MPa at the bore, and Lame gives {von_mises}: {r}");
    assert!(close(moved, grows, 2.0), "the bore grows by {moved} mm, and Lame gives {grows}: {r}");
    let at: Vec<f64> = r["max_von_mises"]["at"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(close((at[0] * at[0] + at[1] * at[1]).sqrt(), a, 0.5), "the most is at the bore: {at:?}");
}

#[test]
fn a_hole_in_a_plate_raises_the_stress_as_the_handbooks_say() {
    if !programs("a plate with a hole") {
        return;
    }
    // A plate 100 wide and 5 thick with a hole of 20 through its middle, pulled along its
    // length by 10 MPa: a quarter of it, free to slide on its two cut faces and on its back.
    let mut p = Part::new();
    p.run("model.box", json!({ "width": 100, "depth": 50, "height": 5 }));
    p.run("model.cylinder", json!({ "diameter": 20, "height": 5, "operation": "cut" }));
    p.run("document.material", json!({ "name": "Aluminium 6061" }));
    let cuts_and_back = [p.facing("left"), p.facing("front"), p.facing("bottom")];
    let end = p.facing("right");
    p.run("study.new", json!({}));
    p.run("study.frictionless", json!({ "faces": cuts_and_back }));
    p.run("study.force", json!({ "faces": [end], "newtons": -2500 }));
    p.run("study.mesh_size", json!({ "size": 2.0 }));
    let r = p.run("study.solve", json!({}));

    // Beside a hole a fifth as wide as the plate the stress is 2 + (1 - d/W)^3 times that of
    // what is left of the section (Heywood): 3.14 times the pull.
    let (pull, d_over_w) = (10.0, 0.2f64);
    let most = (2.0 + (1.0 - d_over_w).powi(3)) * pull / (1.0 - d_over_w);
    let vm = r["max_von_mises"]["megapascals"].as_f64().unwrap();
    assert!(close(vm, most, 5.0), "von Mises {vm} MPa beside the hole, and the handbook gives {most}: {r}");
    // It is at the hole, on the line across the plate through its middle.
    let at: Vec<f64> = r["max_von_mises"]["at"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(at[0].abs() < 1.5 && close(at[1], 10.0, 2.0), "beside the hole: {at:?}");
    // 6061 yields at 275 MPa.
    assert!(close(r["safety_factor"].as_f64().unwrap(), 275.0 / vm, 0.01), "{r}");
}

#[test]
fn a_study_that_cannot_be_worked_out_says_why_before_anything_is_started() {
    // (These refusals need neither program: they are made before one is looked for.)
    let mut p = Part::new();
    p.run("model.box", json!({ "width": 100, "depth": 10, "height": 10 }));
    assert!(p.refused("study.solve", json!({})).contains("no study yet"));
    p.run("study.new", json!({}));
    let lacks = p.refused("study.solve", json!({}));
    assert!(lacks.starts_with("Study1 cannot be worked out yet. It needs a support"), "{lacks}");
    assert!(lacks.contains("a load") && lacks.contains("a material that says how strong it is"), "{lacks}");
    assert!(p.refused("study.solve", json!({ "study": 7 })).contains("no study 7"));

    if !programs("a part that is not held fast") {
        return;
    }
    // Held only against sliding off its bottom, it can still slide and turn: said so, with
    // how many ways, before the solver is asked for an answer that there is not.
    p.run("document.material", json!({ "name": "Steel, mild" }));
    let (bottom, top) = (p.facing("bottom"), p.facing("top"));
    p.run("study.frictionless", json!({ "faces": [bottom] }));
    p.run("study.pressure", json!({ "faces": [top], "megapascals": 1 }));
    let loose = p.refused("study.solve", json!({}));
    assert!(loose.contains("can still move as a whole (3 ways"), "{loose}");
    // The files of a run are left where asked.
    let left = p.facing("left");
    p.run("study.fix", json!({ "faces": [left] }));
    let dir = std::env::temp_dir().join(format!("tenon-m11-kept-{}", std::process::id()));
    let r = p.run("study.solve", json!({ "keep": dir.to_string_lossy() }));
    assert_eq!(r["kept"], json!(dir.display().to_string()));
    for file in ["part.step", "part.msh", "mesher.log", "study.inp", "study.frd", "solver.log"] {
        assert!(dir.join(file).is_file(), "{file} was not left in {}", dir.display());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
