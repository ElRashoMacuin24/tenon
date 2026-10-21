//! Optimize Shape (DEC-123) worked out by the real mesher and solver: where material goes, and
//! that what is asked of it is kept to.
//!
//! The gated tests need Gmsh and CalculiX (`pixi install -e fea`, docs/stress.md). Without
//! them they say so and pass, unless `TENON_FEA_REQUIRED` is set, as the gate sets it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tenon_fea::topo::{Report, Stop};
use tenon_fea::{Cancel, Tools};
use tenon_io::shape::{Shaped, work_out};
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;
use tenon_model::study::{Optimize, StudyId, Target};

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
    fn shape(&mut self, o: &Optimize, cancel: &Cancel, progress: Option<&tenon_fea::topo::Progress>) -> Result<Shaped, String> {
        work_out(&mut self.s, &mut self.k, StudyId(1), o, 0, None, cancel, progress)
    }
}

/// A steel bar 60 long, 10 deep and 30 tall, held at its left end, 100 N down on its right.
fn cantilever() -> Part {
    let mut p = Part::new();
    p.run("model.box", json!({ "width": 60, "depth": 10, "height": 30 }));
    p.run("document.material", json!({ "name": "Steel, mild" }));
    let (left, right) = (p.facing("left"), p.facing("right"));
    p.run("study.new", json!({}));
    p.run("study.fix", json!({ "faces": [left] }));
    p.run("study.force", json!({ "faces": [right], "newtons": 100, "direction": "down" }));
    p
}

/// Quick settings: no sharpening, 3 mm elements, `rounds` at most.
fn quick(rounds: u32) -> Optimize {
    Optimize { target: Target::Remove(60.0), sharpen: false, iterations: Some(rounds), element_size: Some(3.0), ..Optimize::default() }
}

/// The mean density of the elements whose middles `pick` takes, by volume.
fn mean_density(shaped: &Shaped, pick: impl Fn(tenon_geom::Vec3) -> bool) -> f64 {
    let mesh = &shaped.field.mesh;
    let (mut sum, mut vol) = (0.0, 0.0);
    for (t, x) in mesh.tets.iter().zip(&shaped.optimized.density) {
        if pick(mesh.centroid(t)) {
            let v = mesh.volume(t);
            sum += x * v;
            vol += v;
        }
    }
    assert!(vol > 0.0, "no element was picked");
    sum / vol
}

#[test]
fn optimize_shape_refuses_what_it_cannot_take_before_anything_is_started() {
    // None of these needs the programs: they are said before any is started.
    let mut p = Part::new();
    p.run("model.box", json!({ "width": 60, "depth": 10, "height": 30 }));
    p.run("document.material", json!({ "name": "Steel, mild" }));
    assert!(p.refused("study.optimize", json!({})).contains("no study yet"));
    let (left, right) = (p.facing("left"), p.facing("right"));
    p.run("study.new", json!({}));
    p.run("study.fix", json!({ "faces": [left] }));
    assert!(p.refused("study.optimize", json!({})).contains("cannot be worked out yet"));
    p.run("study.force", json!({ "faces": [right], "newtons": 100, "direction": "down" }));
    for (asked, said) in [
        (json!({ "remove": 99 }), "from 5 to 95 %"),
        (json!({ "remove": 50, "grams": 20 }), "not both"),
        (json!({ "iterations": 0 }), "from 1 to 300"),
        // The bar is 60 x 10 x 30 of steel at 7.85: 141.3 g.
        (json!({ "grams": 200 }), "is not less than it"),
        (json!({ "grams": 1 }), "less than 5%"),
        (json!({ "smooth": 99 }), "0 to 50"),
        (json!({ "shown": 2 }), "0.05 to 1"),
        (json!({ "resolution": "huge" }), "coarse"),
    ] {
        let e = p.refused("study.optimize", asked.clone());
        assert!(e.contains(said), "{asked}: {e}");
    }
    // A load that depends on how much material is kept is refused by name until suppressed.
    let g = p.run("study.gravity", json!({}));
    let e = p.refused("study.optimize", json!({}));
    assert!(e.contains("does not take Gravity1 yet") && e.contains("Suppress it"), "{e}");
    let gravity = g["load"].as_u64().unwrap();
    p.run("study.set", json!({ "item": gravity, "suppressed": true }));
    // Planes of symmetry that are not at right angles are refused in words.
    p.run("study.shape", json!({ "symmetry": ["xy", "xy"] }));
    let e = p.refused("study.optimize", json!({}));
    assert!(e.contains("plane"), "{e}");
}

#[test]
fn a_cantilever_puts_material_at_its_top_and_bottom() {
    if !programs("a cantilever's shape") {
        return;
    }
    let mut p = cantilever();
    let shaped = p.shape(&quick(30), &Cancel::default(), None).unwrap();
    let h = &shaped.optimized.history;
    assert!(h.len() >= 10, "{} rounds", h.len());
    // The work of the loads falls as material goes where it works, and hardly ever rises.
    let (first, last) = (h[0].compliance, h.last().unwrap().compliance);
    assert!(last < 0.5 * first, "from {first} to {last}");
    for w in h.windows(2) {
        assert!(w[1].compliance <= 1.02 * w[0].compliance, "round {} rose: {} to {}", w[1].number, w[0].compliance, w[1].compliance);
    }
    // The share asked is the share kept, and the surface encloses it.
    assert!((h.last().unwrap().fraction - 0.4).abs() < 1e-6, "{}", h.last().unwrap().fraction);
    let corner_total: f64 = shaped.field.mesh.tets.iter().map(|t| shaped.field.mesh.corner_volume(t)).sum();
    let skin = &shaped.shown.skin;
    assert!(skin.is_closed(), "the surface is closed");
    assert!(
        (skin.volume() - shaped.shown.share * corner_total).abs() < 0.01 * corner_total,
        "{} against {}",
        skin.volume(),
        shaped.shown.share * corner_total
    );
    assert!((shaped.shown.share - 0.4).abs() < 0.01, "{}", shaped.shown.share);
    // Near the root, material is at the top and the bottom, where bending stresses it, more
    // than at mid-height.
    let fibres = mean_density(&shaped, |c| c.x < 12.0 && (c.z < 6.0 || c.z > 24.0));
    let middle = mean_density(&shaped, |c| c.x < 12.0 && (12.0..18.0).contains(&c.z));
    assert!(fibres > middle + 0.2, "fibres {fibres}, middle {middle}");
    // The shape is no stiffer than the solid part, and the summary says what was asked.
    let (solid, shape) = (shaped.optimized.solid_compliance, shaped.shape_compliance.unwrap());
    assert!(shape >= solid * (1.0 - 1e-9), "{shape} against {solid}");
    let r = shaped.summary("Study1");
    assert_eq!((r["target"]["fraction"].as_f64(), r["iterations"].as_u64()), (Some(0.4), Some(h.len() as u64)), "{r}");
    assert!((r["mass"]["part_grams"].as_f64().unwrap() - 141.3).abs() < 1e-6, "{r}");
    assert!(r["surface"]["watertight"] == true && r["stiffness_kept"].as_f64().unwrap() <= 1.0 + 1e-9, "{r}");
    assert_eq!(r["note"], tenon_io::shape::NOTE);
}

#[test]
fn the_same_study_gives_the_same_shape_bit_for_bit() {
    if !programs("the same shape twice") {
        return;
    }
    let mut p = cantilever();
    let a = p.shape(&quick(5), &Cancel::default(), None).unwrap();
    let b = p.shape(&quick(5), &Cancel::default(), None).unwrap();
    assert_eq!(a.optimized.density, b.optimized.density);
    assert_eq!(
        a.optimized.history.iter().map(|i| i.compliance).collect::<Vec<_>>(),
        b.optimized.history.iter().map(|i| i.compliance).collect::<Vec<_>>()
    );
    assert_eq!(a.shown.skin, b.shown.skin);
}

#[test]
fn study_optimize_writes_a_closed_stl_and_3mf() {
    if !programs("a shape written") {
        return;
    }
    let mut p = cantilever();
    let dir = std::env::temp_dir().join(format!("tenon-shape-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (stl, threemf) = (dir.join("shape.stl"), dir.join("shape.3mf"));
    p.run("study.shape", json!({ "remove": 60, "sharpen": false, "element_size": 3, "iterations": 4 }));
    let r = p.run("study.optimize", json!({ "stl": stl.display().to_string(), "threemf": threemf.display().to_string(), "smooth": 2 }));
    assert_eq!(r["written"]["stl"], stl.display().to_string());
    let data = std::fs::read(&stl).unwrap();
    assert_eq!(tenon_io::stl::binary_triangle_count(&data).map(u64::from), r["surface"]["triangles"].as_u64(), "{r}");
    assert!(r["surface"]["watertight"] == true && r["stopped"] == "limit" && r["iterations"] == 4, "{r}");
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("most rounds")), "{r}");
    let zip = std::fs::read(&threemf).unwrap();
    assert!(zip.starts_with(b"PK") && zip.len() > 1000, "a 3MF package");
    // Another share for the files: fewer triangles, and the summary still of the run.
    let r2 = p.run("study.optimize", json!({ "stl": stl.display().to_string(), "shown": 0.2, "smooth": 2 }));
    let fewer = tenon_io::stl::binary_triangle_count(&std::fs::read(&stl).unwrap()).unwrap();
    assert!(u64::from(fewer) != r2["surface"]["triangles"].as_u64().unwrap(), "the file is of the other share");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_stopped_optimization_keeps_the_shape_so_far() {
    if !programs("a stopped shape") {
        return;
    }
    let mut p = cantilever();
    let cancel = Cancel::default();
    let progress = Arc::new(Mutex::new(Report::default()));
    // Stopped once two rounds are told.
    let watcher = {
        let (cancel, progress) = (cancel.clone(), progress.clone());
        std::thread::spawn(move || {
            for _ in 0..60_000 {
                if progress.lock().unwrap().last.is_some_and(|i| i.number >= 2) {
                    cancel.store(true, Ordering::Relaxed);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        })
    };
    let shaped = p.shape(&quick(30), &cancel, Some(&progress)).unwrap();
    watcher.join().unwrap();
    assert_eq!(shaped.optimized.stop, Stop::Stopped);
    assert!((2..30).contains(&shaped.optimized.history.len()), "{}", shaped.optimized.history.len());
    assert!(shaped.shape_compliance.is_none() && shaped.warnings.iter().any(|w| w.contains("stopped")), "{:?}", shaped.warnings);
    assert!(shaped.shown.skin.is_closed() && !shaped.shown.skin.triangles.is_empty());
}

#[test]
fn a_kept_bore_keeps_a_ring_of_the_depth_asked() {
    if !programs("a kept bore") {
        return;
    }
    // A plate 80 x 40 x 10 held at its left end, with a bore of 8 at (60, 20) pulled down.
    let mut p = Part::new();
    p.run("model.box", json!({ "width": 80, "depth": 40, "height": 10 }));
    p.run("model.cylinder", json!({ "x": 60, "y": 20, "diameter": 8, "height": 10, "operation": "cut" }));
    p.run("document.material", json!({ "name": "Steel, mild" }));
    let left = p.facing("left");
    let bore = p.face(json!({ "kind": "cylinder" }));
    p.run("study.new", json!({}));
    p.run("study.fix", json!({ "faces": [left] }));
    p.run("study.force", json!({ "faces": [bore], "newtons": 100, "direction": "-y" }));
    p.run("study.keep", json!({ "faces": [bore], "depth": 3 }));
    // (The kept faces are the study's own settings, with quick ones for the rest.)
    let kept = p.s.document().study(StudyId(1)).unwrap().optimize.clone().unwrap().kept;
    assert_eq!(kept.len(), 1);
    let shaped = p.shape(&Optimize { kept, ..quick(8) }, &Cancel::default(), None).unwrap();
    // Every element whose middle is within the depth of the bore stays solid.
    let mesh = &shaped.field.mesh;
    let mut near = 0;
    for (t, x) in mesh.tets.iter().zip(&shaped.optimized.density) {
        let c = mesh.centroid(t);
        let from_axis = ((c.x - 60.0).powi(2) + (c.y - 20.0).powi(2)).sqrt();
        if from_axis < 4.0 + 2.5 {
            near += 1;
            assert_eq!(*x, 1.0, "an element {from_axis} mm from the bore's axis is at {x}");
        }
    }
    assert!(near > 20, "{near} elements round the bore");
}

#[test]
fn a_mirrored_study_gives_a_mirrored_shape() {
    if !programs("a mirrored shape") {
        return;
    }
    // A beam 80 long about the origin, held at both ends, pushed down on its top: the same on
    // both sides of the YZ plane.
    let mut p = Part::new();
    p.run("model.box", json!({ "width": 80, "depth": 10, "height": 20, "centered": true }));
    p.run("document.material", json!({ "name": "Steel, mild" }));
    let (left, right, top) = (p.facing("left"), p.facing("right"), p.facing("top"));
    p.run("study.new", json!({}));
    p.run("study.fix", json!({ "faces": [left, right] }));
    p.run("study.force", json!({ "faces": [top], "newtons": 100, "direction": "down" }));
    let mut o = quick(8);
    o.symmetry = vec![tenon_model::PlaneRef::Origin(tenon_model::OriginPlane::YZ)];
    let shaped = p.shape(&o, &Cancel::default(), None).unwrap();
    // The material kept is centred on the plane.
    let mesh = &shaped.field.mesh;
    let (mut moment, mut mass) = (0.0, 0.0);
    for (t, x) in mesh.tets.iter().zip(&shaped.optimized.density) {
        let v = mesh.volume(t) * x;
        moment += v * mesh.centroid(t).x;
        mass += v;
    }
    assert!((moment / mass).abs() < 1e-3 * 80.0, "the material's middle is {} off the plane", moment / mass);
}

#[test]
fn a_part_that_is_not_symmetric_is_refused() {
    if !programs("an asymmetric part") {
        return;
    }
    // The cantilever is not the same on both sides of the YZ plane (it lies from x = 0 to 60).
    let mut p = cantilever();
    let mut o = quick(3);
    o.symmetry = vec![tenon_model::PlaneRef::Origin(tenon_model::OriginPlane::YZ)];
    let e = p.shape(&o, &Cancel::default(), None).unwrap_err();
    assert!(e.contains("is not the same on both sides of YZ"), "{e}");
}

#[test]
fn sharpening_makes_the_shape_crisper_and_no_less_stiff() {
    if !programs("a sharpened shape") {
        return;
    }
    let mut p = cantilever();
    // (Sharpening goes through its five stages in at most 100 rounds.)
    let mut plain = quick(40);
    plain.element_size = Some(5.0);
    let mut sharp = plain.clone();
    sharp.sharpen = true;
    sharp.iterations = Some(100);
    let (a, b) = (p.shape(&plain, &Cancel::default(), None).unwrap(), p.shape(&sharp, &Cancel::default(), None).unwrap());
    let (grey_a, grey_b) = (a.optimized.history.last().unwrap().grey, b.optimized.history.last().unwrap().grey);
    assert!(grey_b < 0.5 * grey_a, "grey {grey_b} sharpened against {grey_a}");
    assert!(b.optimized.history.iter().any(|i| i.sharpness > 1.0), "it sharpened");
    let (ca, cb) = (a.shape_compliance.unwrap(), b.shape_compliance.unwrap());
    assert!(cb <= 1.02 * ca, "the sharpened shape's work {cb} against {ca}");
}

#[test]
fn the_shape_solved_once_keeps_what_must_stay_then_the_densest() {
    // (No programs: which elements are solid in the check.)
    let (density, volumes, passive) = ([0.2, 0.9, 0.5, 0.1], [1.0, 1.0, 1.0, 1.0], [false, false, false, true]);
    let solid = tenon_fea::topo::LEVELS;
    assert_eq!(tenon_io::shape::kept_levels(&density, &volumes, &passive, 0.5), vec![0, solid, 0, solid]);
    assert_eq!(tenon_io::shape::kept_levels(&density, &volumes, &passive, 0.75), vec![0, solid, solid, solid]);
    // What must stay is kept however little is asked; equal densities go by order.
    assert_eq!(tenon_io::shape::kept_levels(&density, &volumes, &passive, 0.0), vec![0, 0, 0, solid]);
    assert_eq!(tenon_io::shape::kept_levels(&[0.5, 0.5], &[1.0, 1.0], &[false, false], 0.5), vec![solid, 0]);
}
