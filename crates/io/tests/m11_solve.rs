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
    /// The reference of the edge whose middle is at `mid`.
    fn edge_at(&mut self, mid: [f64; 3]) -> Value {
        let found = self.run("model.find", json!({ "what": "edges" }));
        let there = |e: &&Value| (0..3).all(|i| (e["mid"][i].as_f64().unwrap() - mid[i]).abs() < 1e-6);
        found["found"].as_array().unwrap().iter().find(there).unwrap_or_else(|| panic!("no edge with its middle at {mid:?}: {found}"))["reference"]
            .clone()
    }
    /// A steel bar 100 x 10 x 10 along x, with a study that has nothing in it yet.
    fn bar() -> Part {
        let mut p = Part::new();
        p.run("model.box", json!({ "width": 100, "depth": 10, "height": 10 }));
        p.run("document.material", json!({ "name": "Steel, mild" }));
        p.run("study.new", json!({}));
        p
    }
    fn solved(&mut self) -> (f64, f64, Value) {
        let r = self.run("study.solve", json!({}));
        (r["max_von_mises"]["megapascals"].as_f64().unwrap(), r["max_displacement"]["millimetres"].as_f64().unwrap(), r)
    }
}

/// Steel's density, in tonnes per cubic millimetre (so that mm, N and s go together).
const DENSITY: f64 = 7.85e-9;

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

#[test]
fn a_round_bar_twisted_by_a_moment_is_stressed_as_torsion_theory_says() {
    if !programs("a bar in torsion") {
        return;
    }
    // A round bar of 20 across and 100 long, held at one end, with 10 N m about its own axis
    // on the other.
    let mut p = Part::new();
    p.run("model.cylinder", json!({ "diameter": 20, "height": 100 }));
    p.run("document.material", json!({ "name": "Steel, mild" }));
    let (bottom, top) = (p.facing("bottom"), p.facing("top"));
    p.run("study.new", json!({}));
    p.run("study.fix", json!({ "faces": [bottom] }));
    p.run("study.moment", json!({ "faces": [top], "newton_millimetres": 10000 }));
    p.run("study.mesh_size", json!({ "size": 2.5 }));
    let (vm, moved, r) = p.solved();

    // Shear at the rim Tr/J, which is root three times less than von Mises; the far end
    // turns by TL/GJ, and its rim moves by that times the radius.
    let (e, nu) = STEEL;
    let (torque, radius, length) = (10_000.0, 10.0f64, 100.0);
    let polar = std::f64::consts::PI * radius.powi(4) / 2.0;
    let shear = torque * radius / polar;
    let turned = torque * length / (e / (2.0 * (1.0 + nu)) * polar);
    assert!(close(vm, 3f64.sqrt() * shear, 2.0), "von Mises {vm} MPa, and torsion gives {}: {r}", 3f64.sqrt() * shear);
    assert!(close(moved, turned * radius, 2.0), "the rim moved {moved} mm, and torsion gives {}: {r}", turned * radius);
    assert!((r["max_displacement"]["at"][2].as_f64().unwrap() - 100.0).abs() < 1e-6, "at the far end: {r}");
    // The other way round: the same, but for its sense.
    p.run("study.set", json!({ "item": 2, "newton_millimetres": -10000 }));
    assert!(close(p.solved().1, moved, 0.01));
}

#[test]
fn a_bar_bent_by_a_moment_or_by_a_force_from_beyond_its_end() {
    if !programs("a bar bent by a moment") {
        return;
    }
    let (e, _) = STEEL;
    let (l, c, inertia) = (100.0f64, 5.0f64, 10.0 * 10.0f64.powi(3) / 12.0);
    // 1 N m about y on the end of the bar: the same bending all along it. Its end drops (or
    // rises) by ML^2/2EI and tilts by ML/EI; the stress is Mc/I.
    let mut p = Part::bar();
    let (left, right) = (p.facing("left"), p.facing("right"));
    p.run("study.fix", json!({ "faces": [left] }));
    p.run("study.moment", json!({ "faces": [right], "newton_millimetres": 1000, "axis": "+y" }));
    p.run("study.mesh_size", json!({ "size": 2.5 }));
    let (vm, moved, r) = p.solved();
    let m = 1000.0;
    let (drop, tilt) = (m * l * l / (2.0 * e * inertia), m * l / (e * inertia));
    let most = (drop * drop + (tilt * c).powi(2)).sqrt();
    assert!(close(moved, most, 1.5), "its end moved {moved} mm, and beam theory gives {most}: {r}");
    assert!(vm > m * c / inertia * 0.97 && vm < m * c / inertia * 1.3, "{vm} MPa, and bending gives {} all along it: {r}", m * c / inertia);

    // In its place, 100 N down that acts 50 beyond the end: the force and the moment it has
    // about the end. FL^3/3EI and (Fa)L^2/2EI, with a little for shear.
    p.run("study.remove", json!({ "item": 2 }));
    p.run("study.remote_force", json!({ "faces": [right], "newtons": 100, "direction": "down", "point": [150, 5, 5] }));
    let (_, moved, r) = p.solved();
    let (f, a) = (100.0, 50.0);
    let shear = f * l / (5.0 / 6.0 * (e / (2.0 * (1.0 + STEEL.1))) * 100.0);
    let drop = f * l.powi(3) / (3.0 * e * inertia) + f * a * l * l / (2.0 * e * inertia) + shear;
    let tilt = f * l * l / (2.0 * e * inertia) + f * a * l / (e * inertia);
    let most = (drop * drop + (tilt * c).powi(2)).sqrt();
    assert!(close(moved, most, 1.5), "its end moved {moved} mm, and beam theory gives {most}: {r}");
}

#[test]
fn a_bearing_load_in_a_bore_bends_the_bar_as_a_force_there_would() {
    if !programs("a bearing load") {
        return;
    }
    // A bar 100 long, 10 thick (y) and 20 high, with a bore of 4 through its height at 80:
    // held at its left end, a shaft in the bore pressing 100 N toward the front.
    let mut p = Part::new();
    p.run("model.box", json!({ "width": 100, "depth": 10, "height": 20 }));
    p.run("model.cylinder", json!({ "x": 80, "y": 5, "diameter": 4, "height": 20, "operation": "cut" }));
    p.run("document.material", json!({ "name": "Steel, mild" }));
    let left = p.facing("left");
    let bore = p.face(json!({ "kind": "cylinder" }));
    p.run("study.new", json!({}));
    p.run("study.fix", json!({ "faces": [left] }));
    p.run("study.bearing", json!({ "faces": [bore], "newtons": 100, "direction": "front" }));
    p.run("study.mesh_size", json!({ "size": 2.5 }));
    let (_, moved, r) = p.solved();
    // A cantilever with a force at a: its end moves Fa^2(3L - a)/6EI, with a little for shear,
    // and tilts by Fa^2/2EI.
    let (e, nu) = STEEL;
    let (f, a, l, inertia) = (100.0, 80.0f64, 100.0, 20.0 * 10.0f64.powi(3) / 12.0);
    let drop = f * a * a * (3.0 * l - a) / (6.0 * e * inertia) + f * a / (5.0 / 6.0 * (e / (2.0 * (1.0 + nu))) * 200.0);
    let most = (drop * drop + (f * a * a / (2.0 * e * inertia) * 5.0).powi(2)).sqrt();
    assert!(close(moved, most, 3.0), "its end moved {moved} mm, and beam theory gives {most}: {r}");
    let at: Vec<f64> = r["max_displacement"]["at"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((at[0] - 100.0).abs() < 1e-6, "at its free end: {at:?}");
    // Along the bore's axis there is nothing for it to press on: said when it is put to the
    // part (a study does not know which way its faces lie until then).
    p.run("study.set", json!({ "item": 2, "direction": "up" }));
    let told = p.refused("study.solve", json!({}));
    assert!(told.contains("Bearing1: a bearing load presses across the axis of its face"), "{told}");
    // On a flat face it is refused at once.
    let top = p.facing("top");
    assert!(p.refused("study.bearing", json!({ "faces": [top], "newtons": 1, "direction": "front" })).contains("round (cylindrical) faces"));
}

#[test]
fn a_rod_flung_round_and_a_bar_under_its_own_weight() {
    if !programs("a spinning rod") {
        return;
    }
    let (e, _) = STEEL;
    // A rod free to slide on three faces at right angles, turning at 6000 turns a minute
    // about z through the end it is held at: flung outward, it is pulled most at that end,
    // by rho omega^2 L^2 / 2, and its far end moves out by rho omega^2 L^3 / 3E.
    let mut p = Part::bar();
    let held = [p.facing("left"), p.facing("front"), p.facing("bottom")];
    p.run("study.frictionless", json!({ "faces": held }));
    p.run("study.body", json!({ "rpm": 6000 }));
    p.run("study.mesh_size", json!({ "size": 2.5 }));
    let (vm, moved, r) = p.solved();
    let omega = 6000.0 * std::f64::consts::TAU / 60.0;
    let (pull, out) = (DENSITY * omega * omega * 100.0f64.powi(2) / 2.0, DENSITY * omega * omega * 100.0f64.powi(3) / (3.0 * e));
    assert!(close(vm, pull, 1.5), "von Mises {vm} MPa at the axis, and a rod flung round gives {pull}: {r}");
    assert!(close(moved, out, 1.0), "its far end moved {moved} mm, and theory gives {out}: {r}");
    assert!(r["max_von_mises"]["at"][0].as_f64().unwrap() < 5.0 && (r["max_displacement"]["at"][0].as_f64().unwrap() - 100.0).abs() < 1e-6, "{r}");
    // Four times as fast in turns is sixteen times the pull.
    p.run("study.set", json!({ "item": 2, "rpm": 12000 }));
    assert!(close(p.solved().0, 4.0 * vm, 0.01));

    // In its place its own weight, pulling toward the end it stands on: pressed most there,
    // by rho g L, and shorter by rho g L^2 / 2E.
    p.run("study.remove", json!({ "item": 2 }));
    p.run("study.gravity", json!({ "direction": "left" }));
    let (vm, moved, r) = p.solved();
    let (pressed, shorter) = (DENSITY * 9810.0 * 100.0, DENSITY * 9810.0 * 100.0f64.powi(2) / (2.0 * e));
    assert!(close(vm, pressed, 1.0), "von Mises {vm} MPa at its foot, and its weight gives {pressed}: {r}");
    assert!(close(moved, shorter, 0.5), "it is {moved} mm shorter, and its weight gives {shorter}: {r}");
    // Both at once: the solver is given the weight and the turning together.
    p.run("study.body", json!({ "rpm": 6000 }));
    let both = p.solved().1;
    assert!(close(both, out - shorter, 1.0), "{both} mm with both, {out} flung and {shorter} pressed");
}

#[test]
fn a_force_along_an_edge_and_a_bar_held_by_its_edges() {
    if !programs("a force along an edge") {
        return;
    }
    let (e, nu) = STEEL;
    let (f, l, inertia) = (100.0, 100.0f64, 10.0 * 10.0f64.powi(3) / 12.0);
    let drop = f * l.powi(3) / (3.0 * e * inertia) + f * l / (5.0 / 6.0 * (e / (2.0 * (1.0 + nu))) * 100.0);
    // The cantilever again, its 100 N along the top edge of its free end.
    let mut p = Part::bar();
    let left = p.facing("left");
    let top_edge = p.edge_at([100.0, 5.0, 10.0]);
    p.run("study.fix", json!({ "faces": [left] }));
    let made = p.run("study.force", json!({ "edges": [top_edge], "newtons": 100, "direction": "down" }));
    assert_eq!((made["loads"][0]["edges"].as_u64(), made["loads"][0]["faces"].as_u64()), (Some(1), Some(0)));
    p.run("study.mesh_size", json!({ "size": 2.5 }));
    let (_, on_edge, r) = p.solved();
    // (A load on a line presses into the part under it: a little more than the beam's drop.)
    assert!(on_edge > drop * 0.99 && on_edge < drop * 1.04, "its end dropped {on_edge} mm, and beam theory gives {drop}: {r}");

    // Held by the four edges of its end face in place of the face: a little less stiff, for
    // the middle of that face is free; still a cantilever.
    let edges = [p.edge_at([0.0, 5.0, 0.0]), p.edge_at([0.0, 5.0, 10.0]), p.edge_at([0.0, 0.0, 5.0]), p.edge_at([0.0, 10.0, 5.0])];
    let held = p.run("study.set", json!({ "item": 1, "edges": edges }));
    assert_eq!(
        (held["supports"][0]["edges"].as_u64(), held["supports"][0]["faces"].as_u64()),
        (Some(4), Some(0)),
        "its edges took the place of its face"
    );
    let (_, by_edges, r) = p.solved();
    assert!(by_edges > on_edge && by_edges < drop * 1.08, "{by_edges} mm held by its edges, {on_edge} by its face: {r}");
    // A force square to an edge has no meaning, and says so before anything is started.
    assert!(p.refused("study.set", json!({ "item": 2, "direction": "normal" })).contains("needs a direction"));
}
