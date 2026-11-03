//! Optimize Shape (DEC-123) worked out by the real mesher and solver: where material goes, and
//! that what is asked of it is kept to.
//!
//! The gated tests need Gmsh and CalculiX (`pixi install -e fea`, docs/stress.md). Without
//! them they say so and pass, unless `TENON_FEA_REQUIRED` is set, as the gate sets it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use tenon_fea::topo::{Phase, Report, Stop};
use tenon_fea::{Cancel, Tools};
use tenon_io::shape::{Shaped, work_out};
use tenon_kernel::Kernel;
use tenon_kernel_occt::OcctKernel;
use tenon_model::study::{FACE_PLANE_REFUSED, Optimize, Resolution, StudyId, Target};
use tenon_model::{Document, Session};

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

/// A plate centred on the origin, with bores (x, y, across) right through it, of mild steel.
fn plate(width: f64, depth: f64, thick: f64, bores: &[(f64, f64, f64)]) -> Part {
    let mut p = Part::new();
    p.run("model.box", json!({ "width": width, "depth": depth, "height": thick, "centered": true }));
    for (x, y, across) in bores {
        p.run("model.cylinder", json!({ "x": x, "y": y, "diameter": across, "height": thick, "operation": "cut" }));
    }
    p.run("document.material", json!({ "name": "Steel, mild" }));
    p
}

/// Held at its left end, 100 N down on its right.
fn held_at_the_left(p: &mut Part) {
    let (left, right) = (p.facing("left"), p.facing("right"));
    p.run("study.new", json!({}));
    p.run("study.fix", json!({ "faces": [left] }));
    p.run("study.force", json!({ "faces": [right], "newtons": 100, "direction": "down" }));
}

/// The 120 x 80 x 6 plate with eight bores of 4: bolt holes, as Keep Faces is for.
fn bolted_plate() -> Part {
    let bores: Vec<(f64, f64, f64)> = [-50.0, -17.0, 17.0, 50.0].iter().flat_map(|x| [(*x, -30.0, 4.0), (*x, 30.0, 4.0)]).collect();
    let mut p = plate(120.0, 80.0, 6.0, &bores);
    held_at_the_left(&mut p);
    p
}

/// A few rounds of a shape at a resolution, on what it asks of the mesh.
fn few_rounds(resolution: Resolution) -> Optimize {
    Optimize { target: Target::Remove(60.0), sharpen: false, iterations: Some(3), resolution, ..Optimize::default() }
}

/// The settings of a study made to hold a face as a plane of symmetry, as a file edited by hand
/// has it, the document opened from those bytes.
fn with_a_face_as_a_plane_in_its_file(p: &mut Part, face: &Value) {
    let mut v = serde_json::to_value(p.s.document()).unwrap();
    v["studies"][0]["optimize"]["symmetry"] = json!([{ "face": face }]);
    let doc: Document = serde_json::from_value(v).unwrap();
    let bytes = tenon_io::project::to_bytes(&doc, &Map::new()).unwrap();
    let (opened, _) = tenon_io::project::from_bytes(&bytes).unwrap();
    p.s = Session::new(opened);
}

#[test]
fn a_plate_with_small_holes_is_worked_out_near_its_resolution() {
    if !programs("a plate with small holes") {
        return;
    }
    let mut p = bolted_plate();
    let bores = p.run("model.find", json!({ "what": "faces", "kind": "cylinder" }));
    assert_eq!(bores["count"], 8, "{bores}");
    // One bore kept, 2 mm deep; which one is told from its wall.
    p.run("study.keep", json!({ "faces": [bores["found"][0]["reference"].clone()], "depth": 2 }));
    let kept = p.s.document().study(StudyId(1)).unwrap().optimize.clone().unwrap().kept;
    let (mut x, mut y) = (0.0, 0.0);
    {
        let r = p.s.regen(&mut p.k).clone();
        let (body, face) = r.resolve(&kept[0].faces[0], &p.k).unwrap();
        let c = p.k.face_info(r.bodies[body].shape.face(face)).unwrap().centroid;
        x += c.x;
        y += c.y;
    }
    let shaped = p.shape(&Optimize { kept, ..few_rounds(Resolution::Coarse) }, &Cancel::default(), None).unwrap();
    let mesh = &shaped.field.mesh;
    // Under one and a half times the nodes it asks for, and the elements made larger to get
    // there, which the run says; the filter goes with the size it came to.
    let nodes = mesh.nodes.len();
    assert!((6_000..=18_000).contains(&nodes), "{nodes} nodes");
    let first = (9.0 * shaped.part_volume / 12_000.0).cbrt();
    assert!(shaped.element_size > first, "{} against {first}", shaped.element_size);
    assert_eq!(shaped.member_size, 3.0 * shaped.element_size);
    let said = format!("so the elements were made larger ({} mm)", (shaped.element_size * 1000.0).round() / 1000.0);
    assert!(shaped.warnings.iter().any(|w| w.contains(&said)), "{:?}", shaped.warnings);
    // Straight mid-side nodes: no element is any but its corners' shape, and none is inside out.
    for t in &mesh.tets {
        let (curved, corners) = (mesh.volume(t), mesh.corner_volume(t));
        assert!(corners > 0.0 && (curved - corners).abs() < 1e-9 * corners.max(1.0), "{curved} against {corners}");
    }
    // The kept bore still found on such a mesh: every element about it stays solid.
    let mut near = 0;
    for (t, density) in mesh.tets.iter().zip(&shaped.optimized.density) {
        let c = mesh.centroid(t);
        let from_axis = ((c.x - x).powi(2) + (c.y - y).powi(2)).sqrt();
        if from_axis < 2.0 + 1.5 {
            near += 1;
            assert_eq!(*density, 1.0, "an element {from_axis} mm from the bore's axis is at {density}");
        }
    }
    assert!(near > 5, "{near} elements round the bore");
    assert!(shaped.shape_compliance.is_some() && shaped.shown.skin.is_closed());
}

#[test]
fn many_small_holes_at_fine_are_worked_out_or_refused_in_words_that_help() {
    if !programs("many small holes") {
        return;
    }
    // The 200 x 100 x 5 plate with 32 bores of 3. It is stopped once it is divided: how many
    // nodes it took is what is looked at, and a whole round at this size is many seconds.
    let bores: Vec<(f64, f64, f64)> =
        (0..8).flat_map(|i| (0..4).map(move |j| (-84.0 + 24.0 * f64::from(i), -36.0 + 24.0 * f64::from(j), 3.0))).collect();
    let mut p = plate(200.0, 100.0, 5.0, &bores);
    held_at_the_left(&mut p);
    let cancel = Cancel::default();
    let progress = Arc::new(Mutex::new(Report::default()));
    let watcher = {
        let (cancel, progress) = (cancel.clone(), progress.clone());
        std::thread::spawn(move || {
            for _ in 0..60_000 {
                if progress.lock().unwrap().phase == Phase::Solid {
                    cancel.store(true, Ordering::Relaxed);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        })
    };
    let dir = std::env::temp_dir().join(format!("tenon-shape-fine-{}", std::process::id()));
    let e = work_out(&mut p.s, &mut p.k, StudyId(1), &few_rounds(Resolution::Fine), 0, Some(&dir), &cancel, Some(&progress)).unwrap_err();
    cancel.store(true, Ordering::Relaxed);
    watcher.join().unwrap();
    if e == tenon_fea::tools::STOPPED {
        let mesh = tenon_fea::mesh::read_msh(&std::fs::read_to_string(dir.join("part.msh")).unwrap()).unwrap();
        assert!(mesh.nodes.len() <= tenon_fea::topo::MAX_SHAPE_NODES, "{} nodes", mesh.nodes.len());
    } else {
        assert!(e.ends_with("Choose Medium."), "{e}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_plain_solid_comes_to_about_its_resolutions_nodes() {
    if !programs("a plain solid") {
        return;
    }
    let mut p = plate(60.0, 30.0, 10.0, &[]);
    held_at_the_left(&mut p);
    let shaped = p.shape(&few_rounds(Resolution::Coarse), &Cancel::default(), None).unwrap();
    let nodes = shaped.field.mesh.nodes.len();
    assert!((9_000..=15_000).contains(&nodes), "{nodes} nodes");
    // The size it was first given, with nothing to say of it.
    assert!((shaped.element_size - (9.0 * shaped.part_volume / 12_000.0).cbrt()).abs() < 1e-9, "{}", shaped.element_size);
    assert!(!shaped.warnings.iter().any(|w| w.contains("made larger")), "{:?}", shaped.warnings);
}

#[test]
fn a_size_asked_is_kept_and_a_resolution_given_takes_its_place() {
    if !programs("a size asked") {
        return;
    }
    let mut p = cantilever();
    p.run("study.shape", json!({ "element_size": 6, "remove": 60, "sharpen": false, "iterations": 1 }));
    let r = p.run("study.optimize", json!({}));
    assert_eq!(r["element_size"], 6.0, "{r}");
    // A resolution given with no size: its size, for this run, and the file's still 6.
    let by_resolution = (9.0 * 18_000.0f64 / 12_000.0).cbrt();
    let r = p.run("study.optimize", json!({ "resolution": "coarse" }));
    assert!((r["element_size"].as_f64().unwrap() - by_resolution).abs() < 1e-6, "{r}");
    let r = p.run("study.optimize", json!({ "element_size": null }));
    assert!((r["element_size"].as_f64().unwrap() - by_resolution).abs() < 1e-6, "{r}");
    assert_eq!(p.s.document().study(StudyId(1)).unwrap().optimize.as_ref().unwrap().element_size, Some(6.0));
    // Both given: the size, as it is documented.
    let r = p.run("study.optimize", json!({ "resolution": "coarse", "element_size": 5 }));
    assert_eq!(r["element_size"], 5.0, "{r}");
}

#[test]
fn stopped_while_the_shape_is_solved_once_says_so() {
    if !programs("a shape stopped at its check") {
        return;
    }
    let mut p = cantilever();
    let cancel = Cancel::default();
    let progress = Arc::new(Mutex::new(Report::default()));
    let watcher = {
        let (cancel, progress) = (cancel.clone(), progress.clone());
        std::thread::spawn(move || {
            for _ in 0..60_000 {
                if progress.lock().unwrap().phase == Phase::Checking {
                    cancel.store(true, Ordering::Relaxed);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        })
    };
    let shaped = p.shape(&quick(3), &cancel, Some(&progress)).unwrap();
    watcher.join().unwrap();
    // It ended as it would have, and the shape stands; what it lacks is said.
    assert_eq!(shaped.optimized.stop, Stop::Limit);
    assert!(shaped.shape_compliance.is_none() && shaped.shown.skin.is_closed() && !shaped.shown.skin.triangles.is_empty());
    assert!(shaped.warnings.iter().any(|w| w.contains("stopped while the shape was being solved once")), "{:?}", shaped.warnings);
    let r = shaped.summary("Study1");
    assert!(r["stiffness_kept"].is_null() && r["compliance"]["shape"].is_null(), "{r}");
}

#[test]
fn too_much_kept_for_a_mass_asked_says_the_mass() {
    if !programs("too much kept") {
        return;
    }
    let mut p = cantilever();
    let (top, bottom) = (p.facing("top"), p.facing("bottom"));
    p.run("study.keep", json!({ "faces": [top, bottom], "depth": 10 }));
    p.run("study.shape", json!({ "grams": 80, "sharpen": false, "element_size": 3 }));
    let e = p.refused("study.optimize", json!({}));
    assert!(e.contains("Keep1 and the layers under the supports and loads are already"), "{e}");
    assert!(e.contains("% kept. Ask for more than ") && e.ends_with(" g, or keep less."), "{e}");
}

#[test]
fn a_face_held_as_a_plane_in_a_file_is_refused_before_anything_is_started() {
    // (No programs: it is said before any is looked for.)
    let mut p = Part::new();
    p.run("model.box", json!({ "width": 80, "depth": 10, "height": 20, "centered": true }));
    p.run("document.material", json!({ "name": "Steel, mild" }));
    let (left, right, front) = (p.facing("left"), p.facing("right"), p.facing("front"));
    p.run("study.new", json!({}));
    p.run("study.fix", json!({ "faces": [left, right] }));
    p.run("study.force", json!({ "faces": [front], "newtons": 100, "direction": "back" }));
    p.run("study.shape", json!({ "remove": 60 }));
    with_a_face_as_a_plane_in_its_file(&mut p, &front);
    let e = p.refused("study.optimize", json!({}));
    assert_eq!(e, format!("A shape of Study1 cannot be worked out yet. {FACE_PLANE_REFUSED}"));
    // A stress check of that study is not held up: it goes on to look for its programs, or runs.
    let c = p.run("study.check", json!({}));
    assert_eq!(c["ready"], true, "{c}");

    // The corner the asymmetry check cannot see: a plate thinner than a tenth of its elements
    // lies within that of its own image, and it would have run to the end on a meaningless plane.
    let mut p = Part::new();
    p.run("model.box", json!({ "width": 120, "depth": 120, "height": 1 }));
    p.run("document.material", json!({ "name": "Steel, mild" }));
    let (left, right, bottom) = (p.facing("left"), p.facing("right"), p.facing("bottom"));
    p.run("study.new", json!({}));
    p.run("study.fix", json!({ "faces": [left] }));
    p.run("study.force", json!({ "faces": [right], "newtons": 100, "direction": "down" }));
    p.run("study.shape", json!({ "remove": 60, "element_size": 12 }));
    with_a_face_as_a_plane_in_its_file(&mut p, &bottom);
    assert_eq!(p.refused("study.optimize", json!({})), format!("A shape of Study1 cannot be worked out yet. {FACE_PLANE_REFUSED}"));
}

#[test]
fn what_only_a_shape_needs_is_said_before_anything_is_started() {
    // (No programs: it is said before any is looked for.)
    let mut p = plate(80.0, 40.0, 10.0, &[]);
    let cut = p.run("model.cylinder", json!({ "x": 20, "y": 0, "diameter": 8, "height": 10, "operation": "cut" }))["feature"].as_u64().unwrap();
    held_at_the_left(&mut p);
    let bore = p.face(json!({ "kind": "cylinder" }));
    p.run("study.keep", json!({ "faces": [bore] }));
    let plane = p.run("work.plane", json!({ "base": "yz", "distance": 0 }))["feature"].as_u64().unwrap();
    p.run("study.shape", json!({ "symmetry": [{ "work": plane }] }));
    assert_eq!(p.run("study.check", json!({}))["shape_needs"], json!([]));
    // The work plane taken away: its name, or its number when the feature is gone.
    p.run("feature.delete", json!({ "feature": plane }));
    let e = p.refused("study.optimize", json!({}));
    assert!(e.starts_with("A shape of Study1 cannot be worked out yet. "), "{e}");
    assert!(e.contains("cannot be a plane of symmetry: it is no longer on the part. Choose another (study.shape, with symmetry)."), "{e}");
    p.run("study.shape", json!({ "symmetry": [] }));
    // The bore's feature taken away: the kept set by name, each thing said as a sentence.
    p.run("feature.delete", json!({ "feature": cut }));
    let e = p.refused("study.optimize", json!({}));
    assert!(e.contains("Keep1: its face is no longer on the part. Give it its faces again (study.set, with faces)."), "{e}");
    assert!(!e.contains(".."), "{e}");
    // Settings given that are not the study's own say the same of a plane.
    let mut p = plate(80.0, 40.0, 10.0, &[]);
    held_at_the_left(&mut p);
    let top = p.facing("top");
    let face: tenon_model::FaceRef = serde_json::from_value(top).unwrap();
    let o = Optimize { symmetry: vec![tenon_model::PlaneRef::Face(face)], ..Optimize::default() };
    assert_eq!(p.shape(&o, &Cancel::default(), None).unwrap_err(), FACE_PLANE_REFUSED);
    let o = Optimize { symmetry: vec![tenon_model::PlaneRef::Work(tenon_model::FeatureId(99))], ..Optimize::default() };
    let e = p.shape(&o, &Cancel::default(), None).unwrap_err();
    assert_eq!(e, "work plane 99 cannot be a plane of symmetry: it is not a plane that is on the part");
}

#[test]
fn where_the_files_go_is_refused_before_anything_is_started() {
    // (No programs: it is said before any is looked for.)
    let mut p = cantilever();
    let dir = std::env::temp_dir().join(format!("tenon-shape-refused-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let at = |name: &str| dir.join(name).display().to_string();
    for (asked, said) in [
        (json!({ "stl": at("shape.obj") }), "`stl` must be a file ending in .stl"),
        (json!({ "stl": at("y.3mf") }), "`stl` must be a file ending in .stl"),
        (json!({ "threemf": at("x.stl") }), "`threemf` must be a file ending in .3mf"),
        (json!({ "stl": dir.join("no").join("such").join("z.stl").display().to_string() }), "there is no folder"),
    ] {
        let e = p.refused("study.optimize", asked.clone());
        assert!(e.contains(said), "{asked}: {e}");
    }
    // The first file is good and is there; the second is refused: the first is as it was.
    std::fs::write(dir.join("there.stl"), b"what was there").unwrap();
    let e = p.refused("study.optimize", json!({ "stl": at("there.stl"), "threemf": at("x.obj") }));
    assert!(e.contains("`threemf`"), "{e}");
    assert_eq!(std::fs::read(dir.join("there.stl")).unwrap(), b"what was there");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "nothing is left behind");
    std::fs::remove_dir_all(&dir).ok();
}
