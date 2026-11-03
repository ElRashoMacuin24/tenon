//! Optimize Shape (DEC-123): the solver run as the optimizer runs it, with each element at its
//! own stiffness level, on the cantilever the stress checks are tried on. These are the checks
//! that what it reads back is what the optimizer needs: the energy each element holds, which
//! adds up to the work of the loads, the elements' volumes (on that bar with a bore through it
//! too, where the elements are curved), and how the work changes with an element's stiffness.
//!
//! They need Gmsh and CalculiX (`pixi install -e fea`, docs/stress.md). Without them they say
//! so and pass, unless `TENON_FEA_REQUIRED` is set, as the gate (`cargo xtask ci`) sets it:
//! then their absence is a failure.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use serde_json::json;
use tenon_fea::topo::{Ccx, FLOOR, LEVELS, Stiffness, modulus};
use tenon_fea::{Cancel, Deck, Held, Material, Problem, Pushed, Scratch, Setup, Surface, Tools};
use tenon_geom::Vec3;
use tenon_kernel::{Kernel, MeshTol, SurfaceKind};
use tenon_kernel_occt::OcctKernel;
use tenon_model::Session;
use tenon_model::study::{LoadKind, SupportKind};

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

/// Mild steel, as the part's material gives it: Young's modulus (GPa), Poisson's ratio, density.
const STEEL: (f64, f64, f64) = (200.0, 0.29, 7.85);

/// The cantilever of the stress checks: a bar of mild steel 100 x 10 x 10 along x, held fast at
/// its left end, with 100 N down on its right end, divided into elements of 2.5 mm.
fn cantilever(tools: &Tools, dir: &Path) -> Problem {
    bar(tools, dir, None)
}

/// The cantilever, with a bore `bore` mm across through its height at its middle when one is
/// given. The elements on the bore are curved, as on the box's flat faces they are not.
fn bar(tools: &Tools, dir: &Path, bore: Option<f64>) -> Problem {
    let mut s = Session::default();
    let mut k = OcctKernel::new();
    tenon_io::cmd::run(&mut s, "model.box", &json!({ "width": 100, "depth": 10, "height": 10 }), Some(&mut k as &mut dyn Kernel)).unwrap();
    if let Some(d) = bore {
        let cut = json!({ "x": 50, "y": 5, "diameter": d, "height": 10, "operation": "cut" });
        tenon_io::cmd::run(&mut s, "model.cylinder", &cut, Some(&mut k as &mut dyn Kernel)).unwrap();
    }
    let r = s.regen(&mut k).clone();
    let body = r.bodies.first().unwrap();
    // What each face is, and which are the bar's ends.
    let (mut surfaces, mut left, mut right) = (Vec::new(), Vec::new(), Vec::new());
    for face in 0..u32::try_from(body.names.len()).unwrap() {
        let info = k.face_info(body.shape.face(face)).unwrap();
        surfaces.push(match info.surface {
            SurfaceKind::Plane { normal, .. } => Surface::Plane { normal },
            SurfaceKind::Cylinder { axis, .. } => Surface::Cylinder { origin: axis.origin(), dir: axis.dir() },
            _ => panic!("the bar has flat faces and its bore a round one: {info:?}"),
        });
        if info.centroid.x.abs() < 1e-9 {
            left.push(face);
        } else if (info.centroid.x - 100.0).abs() < 1e-9 {
            right.push(face);
        }
    }
    assert_eq!((left.len(), right.len()), (1, 1), "{surfaces:?}");
    let round = surfaces.iter().filter(|f| matches!(f, Surface::Cylinder { .. })).count();
    assert_eq!((surfaces.len(), round), if bore.is_some() { (7, 1) } else { (6, 0) }, "{surfaces:?}");
    let faces = surfaces.len();
    let setup = Setup {
        held: vec![Held { name: "Fixed1".into(), kind: SupportKind::Fixed, faces: left, edges: Vec::new() }],
        pushed: vec![Pushed {
            name: "Force1".into(),
            kind: LoadKind::Force { newtons: 100.0, direction: Some(-Vec3::Z) },
            faces: right,
            edges: Vec::new(),
        }],
        surfaces,
        material: Material::of(STEEL.0, STEEL.1, STEEL.2),
    };
    let step = k.export_step(&[body.shape]).unwrap();
    let drawn = k.tessellate(body.shape, &MeshTol::exact()).unwrap();
    let prepared = tenon_fea::prepare(tools, &step, &drawn, &setup, 2.5, dir, &Cancel::default()).unwrap();
    assert!(prepared.meshing >= 0.0 && prepared.on.len() == faces, "the bar's {faces} faces are found on the mesh: {:?}", prepared.on);
    let p = prepared.problem;
    assert!(p.mesh.tets.len() > 1000 && p.held.len() > 10 && p.forces.len() > 10, "{} elements", p.mesh.tets.len());
    assert!((p.total_force() - Vec3::new(0.0, 0.0, -100.0)).len() < 1e-9, "{:?}", p.total_force());
    p
}

/// The work of the loads (N mm) at each element's stiffness level: twice the energy the
/// elements hold.
fn work(solver: &mut Ccx, levels: &[u16]) -> f64 {
    2.0 * solver.energies(levels).unwrap().iter().sum::<f64>()
}

#[test]
fn element_energies_add_up_to_half_the_work_of_the_loads() {
    if !programs("element energies") {
        return;
    }
    let (tools, scratch, cancel) = (Tools::find().unwrap(), Scratch::new().unwrap(), Cancel::default());
    let p = cantilever(&tools, &scratch.dir);
    // The stress check's answer: how far each loaded node moved, against the force on it.
    let moved = tenon_fea::solve(&tools, &p, &scratch.dir, &cancel).unwrap();
    let work_of_loads: f64 = p.forces.iter().map(|(n, f)| f.dot(moved.displacement[*n as usize])).sum();
    // The solver asked as the optimizer asks it, every element solid.
    let deck = Deck::new(&p).unwrap();
    let solid = vec![LEVELS; p.mesh.tets.len()];
    let (energies, volumes) =
        tenon_fea::solve_shape(&tools, &deck, &solid, p.material.youngs_modulus, p.material.poisson, &scratch.dir, &cancel, false).unwrap();
    assert!(volumes.is_none(), "volumes only when asked");
    assert_eq!(energies.len(), p.mesh.tets.len());
    assert!(energies.iter().all(|u| *u >= 0.0) && energies.iter().any(|u| *u > 0.0));
    let twice = 2.0 * energies.iter().sum::<f64>();
    assert!((twice - work_of_loads).abs() <= 1e-4 * work_of_loads, "twice the energies {twice} N mm, and the loads did {work_of_loads}");
    // (About 100 N times the drop of the end, a fifth of a millimetre: the stress check's own.)
    assert!(work_of_loads > 18.0 && work_of_loads < 24.0, "{work_of_loads}");
    // Each run's files in place of the last's.
    for file in ["shape.inp", "shape.dat", "shape.log", "study.inp", "study.frd"] {
        assert!(scratch.dir.join(file).is_file(), "{file} is not in {}", scratch.dir.display());
    }
}

#[test]
fn rust_volumes_are_the_solvers() {
    if !programs("element volumes") {
        return;
    }
    let (tools, scratch, cancel) = (Tools::find().unwrap(), Scratch::new().unwrap(), Cancel::default());
    // The plain bar, and the bar with a bore of 6, whose elements on the bore are curved.
    for bore in [None, Some(6.0)] {
        let p = bar(&tools, &scratch.dir, bore);
        let deck = Deck::new(&p).unwrap();
        let half = vec![500; p.mesh.tets.len()];
        let (energies, volumes) =
            tenon_fea::solve_shape(&tools, &deck, &half, p.material.youngs_modulus, p.material.poisson, &scratch.dir, &cancel, true).unwrap();
        let volumes = volumes.unwrap();
        assert_eq!((energies.len(), volumes.len()), (p.mesh.tets.len(), p.mesh.tets.len()));
        // The solver's element e is the mesh's e - 1st.
        let mut straight = 0.0f64;
        for (e, (t, theirs)) in p.mesh.tets.iter().zip(&volumes).enumerate() {
            let ours = p.mesh.volume(t);
            assert!((ours - theirs).abs() <= 1e-6 * theirs, "element {} ({bore:?}): {ours} mm^3 here, {theirs} by the solver", e + 1);
            // How far the straight tetrahedron between its corners is from it.
            straight = straight.max((p.mesh.corner_volume(t) - theirs).abs() / theirs);
        }
        let whole: f64 = p.mesh.tets.iter().map(|t| p.mesh.volume(t)).sum();
        match bore {
            // On the box every element is straight, so that its corners alone give its volume.
            None => {
                assert!(straight <= 1e-6, "the box's elements are straight: {straight}");
                assert!((whole - 10_000.0).abs() < 1e-9 * 10_000.0, "the bar is 10000 mm^3: {whole}");
            }
            // On the bore they are not: the straight one is a long way out (a quarter, here),
            // so that only the rule the solver uses agrees with it.
            Some(d) => {
                assert!(straight > 0.1, "the elements on the bore are curved: {straight}");
                let solid = 10_000.0 - std::f64::consts::PI * d * d / 4.0 * 10.0;
                assert!((whole - solid).abs() < 1e-5 * solid, "the bar with its bore is {solid} mm^3: {whole}");
            }
        }
    }
}

#[test]
fn a_uniform_level_scales_the_work_by_its_stiffness() {
    if !programs("a uniform stiffness level") {
        return;
    }
    let (tools, scratch, cancel) = (Tools::find().unwrap(), Scratch::new().unwrap(), Cancel::default());
    let p = cantilever(&tools, &scratch.dir);
    let deck = Deck::new(&p).unwrap();
    let mut solver = Ccx { tools: &tools, deck: &deck, e0: p.material.youngs_modulus, nu: p.material.poisson, dir: &scratch.dir, cancel: &cancel };
    let n = p.mesh.tets.len();
    let solid = solver.energies(&vec![LEVELS; n]).unwrap();
    let half = solver.energies(&vec![500; n]).unwrap();
    // Every element an eighth as stiff (and a hair more, for the floor): the part bends eight
    // times as far under the same loads, and the work of the loads is eight times as much.
    let stiffness = FLOOR + 0.125 * (1.0 - FLOOR);
    assert_eq!(modulus(500, 1.0), stiffness);
    let (c_solid, c_half) = (2.0 * solid.iter().sum::<f64>(), 2.0 * half.iter().sum::<f64>());
    assert!((c_half * stiffness - c_solid).abs() <= 1e-5 * c_solid, "{c_half} N mm at half, {c_solid} solid");
    // So does each element's energy.
    let most = solid.iter().fold(0.0f64, |m, u| m.max(*u));
    for (e, (s, h)) in solid.iter().zip(&half).enumerate() {
        assert!((h * stiffness - s).abs() <= 1e-5 * most, "element {}: {h} at half, {s} solid", e + 1);
    }
    // Stopped, it says so and nothing else.
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(solver.energies(&vec![LEVELS; n]).unwrap_err(), tenon_fea::tools::STOPPED);
}

#[test]
fn sensitivities_agree_with_a_finite_difference() {
    if !programs("sensitivities") {
        return;
    }
    let (tools, scratch, cancel) = (Tools::find().unwrap(), Scratch::new().unwrap(), Cancel::default());
    let p = cantilever(&tools, &scratch.dir);
    let deck = Deck::new(&p).unwrap();
    let mut solver = Ccx { tools: &tools, deck: &deck, e0: p.material.youngs_modulus, nu: p.material.poisson, dir: &scratch.dir, cancel: &cancel };
    let n = p.mesh.tets.len();
    let base = vec![500u16; n];
    let energies = solver.energies(&base).unwrap();
    // Elements at the root, where the bar is bent most: the three that hold the most energy.
    let mut root: Vec<usize> = (0..n).filter(|e| p.mesh.centroid(&p.mesh.tets[*e]).x < 2.5).collect();
    root.sort_by(|a, b| energies[*b].total_cmp(&energies[*a]).then(a.cmp(b)));
    root.truncate(3);
    assert_eq!(root.len(), 3);
    for e in root {
        // How fast the work of the loads falls as the element's density grows (the optimizer's
        // sensitivity), at a density of a half...
        let x = 0.5;
        let analytic = -3.0 * x * x * (1.0 - FLOOR) * 2.0 * energies[e] / modulus(500, 1.0);
        // ... and as the solver finds it, ten levels either side.
        let (mut more, mut less) = (base.clone(), base.clone());
        (more[e], less[e]) = (510, 490);
        let found = (work(&mut solver, &more) - work(&mut solver, &less)) / 0.02;
        assert!(analytic < 0.0 && (found - analytic).abs() <= 0.01 * analytic.abs(), "element {}: {found} found, {analytic} from its energy", e + 1);
    }
}

#[test]
fn a_pin_and_a_bearing_load_need_no_mid_side_node_on_the_bore() {
    if !programs("a pin and a bearing load on a shape's mesh") {
        return;
    }
    // A plate 100 x 40 x 10 about the origin, on a pin through a bore of 16 at one end, a bearing
    // load of 500 N across a bore of 8 at the other.
    let (mut s, mut k) = (Session::default(), OcctKernel::new());
    let mut go = |s: &mut Session, id: &str, p: serde_json::Value| tenon_io::cmd::run(s, id, &p, Some(&mut k as &mut dyn Kernel)).unwrap();
    go(&mut s, "model.box", json!({ "width": 100, "depth": 40, "height": 10, "centered": true }));
    go(&mut s, "model.cylinder", json!({ "x": -30, "y": 0, "diameter": 16, "height": 10, "operation": "cut" }));
    go(&mut s, "model.cylinder", json!({ "x": 30, "y": 0, "diameter": 8, "height": 10, "operation": "cut" }));
    let r = s.regen(&mut k).clone();
    let body = r.bodies.first().unwrap();
    let (mut surfaces, mut pin, mut bore) = (Vec::new(), Vec::new(), Vec::new());
    for face in 0..u32::try_from(body.names.len()).unwrap() {
        let info = k.face_info(body.shape.face(face)).unwrap();
        surfaces.push(match info.surface {
            SurfaceKind::Plane { normal, .. } => Surface::Plane { normal },
            SurfaceKind::Cylinder { axis, .. } => {
                if axis.origin().x < 0.0 {
                    pin.push(face)
                } else {
                    bore.push(face)
                }
                Surface::Cylinder { origin: axis.origin(), dir: axis.dir() }
            }
            _ => panic!("the plate has flat faces and round ones: {info:?}"),
        });
    }
    assert_eq!((pin.len(), bore.len()), (1, 1), "{surfaces:?}");
    let setup = Setup {
        held: vec![Held {
            name: "Pin1".into(),
            kind: SupportKind::Pin { radial: true, axial: true, tangential: true },
            faces: pin,
            edges: Vec::new(),
        }],
        pushed: vec![Pushed {
            name: "Bearing1".into(),
            kind: LoadKind::Bearing { newtons: 500.0, direction: -Vec3::Y },
            faces: bore.clone(),
            edges: Vec::new(),
        }],
        surfaces,
        material: Material::of(STEEL.0, STEEL.1, STEEL.2),
    };
    let step = k.export_step(&[body.shape]).unwrap();
    let drawn = k.tessellate(body.shape, &MeshTol::exact()).unwrap();
    let (tools, cancel) = (Tools::find().unwrap(), Cancel::default());
    let (scratch_a, scratch_b) = (Scratch::new().unwrap(), Scratch::new().unwrap());
    let size = 3.0;
    // The stress check's mesh, and a shape's: the same study put on each.
    let curved = tenon_fea::prepare(&tools, &step, &drawn, &setup, size, &scratch_a.dir, &cancel).unwrap().problem;
    let mesh = tenon_fea::mesh_for_shape(&tools, &step, size, &scratch_b.dir, &cancel).unwrap();
    let straight = tenon_fea::put_on(mesh, &drawn, &setup, 0.0).unwrap().problem;
    // Its mid-side nodes are off the bore, by enough to tell, where the stress check's are on it.
    let off_bore = |p: &Problem, radius: f64| {
        p.mesh
            .tris
            .iter()
            .flat_map(|t| t.nodes[3..].iter())
            .map(|n| p.mesh.nodes[*n as usize])
            .map(|q| {
                let from_axis = ((q.x - 30.0).powi(2) + q.y.powi(2)).sqrt();
                // (Only those on this bore: nearer its wall than a tenth of the way to the next thing.)
                if from_axis < radius + 0.5 { radius - from_axis } else { 0.0 }
            })
            .fold(0.0f64, f64::max)
    };
    assert!(off_bore(&curved, 4.0) < 1e-6, "{}", off_bore(&curved, 4.0));
    assert!(off_bore(&straight, 4.0) > 0.01, "{}", off_bore(&straight, 4.0));
    // What is held and what pushes are as they were: the load adds up, and the part is held fast.
    for p in [&curved, &straight] {
        assert_eq!(p.free_motions(), 0);
        // (All of it along the load, by how it is scaled; across it, what the mesh's not being the
        // same on both sides comes to.)
        let f = p.total_force();
        assert!((f.y + 500.0).abs() < 1e-6 * 500.0 && f.x.abs() + f.z.abs() < 1e-2 * 500.0, "{f:?}");
        assert!(p.held.len() > 10, "{}", p.held.len());
        for dirs in p.held.values() {
            assert_eq!(dirs.len(), 3, "a pin held every way holds each of its nodes so");
        }
    }
    // And the part comes out about as stiff.
    let work_of = |p: &Problem, dir: &Path| {
        let deck = Deck::new(p).unwrap();
        let mut solver = Ccx { tools: &tools, deck: &deck, e0: p.material.youngs_modulus, nu: p.material.poisson, dir, cancel: &cancel };
        work(&mut solver, &vec![LEVELS; p.mesh.tets.len()])
    };
    let (a, b) = (work_of(&curved, &scratch_a.dir), work_of(&straight, &scratch_b.dir));
    assert!((a - b).abs() < 0.05 * a, "{a} against {b}");
}
