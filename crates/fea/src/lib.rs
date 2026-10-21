//! Stress checks (M11): a part divided into elements, a study's supports and loads put on
//! them, and what a solver makes of it.
//!
//! The dividing and the solving are done by two programs of their own, Gmsh and CalculiX,
//! which Tenon starts and exchanges files with ([`tools`]); it contains neither. What is
//! Tenon's own is everything between them: reading the mesh ([`mesh`]), telling which face of
//! the part each of its surfaces is ([`faces`]), turning supports and loads into what holds
//! and pushes each node ([`problem`]), writing that for the solver and reading its answer
//! ([`ccx`]), and what the answer comes to ([`result`]).
//!
//! A quick check is an engineering aid, not a certified analysis: linear, static, of one
//! solid of one material.
#![forbid(unsafe_code)]

pub mod ccx;
pub mod faces;
pub mod mesh;
pub mod problem;
pub mod result;
pub mod tools;
pub mod topo;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub use ccx::{Deck, Solution};
pub use mesh::TetMesh;
pub use problem::{Held, Material, Problem, Pushed, Setup, Spin, Surface};
pub use result::{Peak, Results, von_mises};
pub use tools::{Cancel, Scratch, Tools};

/// The most nodes a mesh may have to be solved: beyond it a "quick check" is neither.
pub const MAX_SOLVE_NODES: usize = 600_000;

/// How long each program may run.
const MESH_LIMIT: Duration = Duration::from_secs(10 * 60);
const SOLVE_LIMIT: Duration = Duration::from_secs(30 * 60);

/// What to try when a program runs too long: for a stress check, and for a shape.
const QUICK: &str = "a quick check is of fewer, larger elements";
const SHAPE_SLOW: &str = "a shape is found sooner at Coarse";

/// The size of the elements a part is divided into when none is said: a twentieth of the box
/// it fits in, corner to corner.
pub fn element_size_for(extent: f64) -> f64 {
    extent / 20.0
}

/// A program's failure with what it was for; a run that was stopped is left as that.
fn said(what: &str, e: String) -> String {
    if e == tools::STOPPED { e } else { format!("{what} {e}") }
}

/// Divides a part into ten-node tetrahedra about `size` across. `step` is the part as a STEP
/// file's bytes. The mesher's files are left in `dir`.
pub fn mesh_part(tools: &Tools, step: &[u8], size: f64, dir: &Path, cancel: &Cancel) -> Result<TetMesh, String> {
    if !(size.is_finite() && size > 0.0) {
        return Err("the element size must be more than 0".into());
    }
    std::fs::write(dir.join("part.step"), step).map_err(|e| format!("cannot write the part for the mesher: {e}"))?;
    let (largest, smallest) = (format!("{size}"), format!("{}", size / 10.0));
    // Second-order tetrahedra, sized `size` at most and finer round small curves (twelve to a
    // full turn), written in the 2.2 format as text. On one thread: with more the mesher
    // gives a slightly different mesh each time, and the same part would not give the same
    // answer twice.
    let args =
        ["part.step", "-3", "-order", "2", "-clmax", &largest, "-clmin", &smallest, "-clcurv", "12", "-nt", "1", "-format", "msh2", "-o", "part.msh"];
    tools::run(&tools.gmsh, &args, dir, "mesher.log", cancel, MESH_LIMIT, QUICK)
        .map_err(|e| said("The part could not be divided into elements.", e))?;
    let text = std::fs::read_to_string(dir.join("part.msh")).map_err(|_| {
        let said = std::fs::read_to_string(dir.join("mesher.log")).unwrap_or_default();
        format!("The mesher wrote no mesh. It said:\n{}", tools::tail(&said, 12))
    })?;
    mesh::read_msh(&text)
}

/// Has the solver work a problem out. Its files are left in `dir`.
pub fn solve(tools: &Tools, problem: &Problem, dir: &Path, cancel: &Cancel) -> Result<Solution, String> {
    std::fs::write(dir.join("study.inp"), ccx::write_inp(problem)).map_err(|e| format!("cannot write the study for the solver: {e}"))?;
    let said =
        tools::run(&tools.ccx, &["study"], dir, "solver.log", cancel, SOLVE_LIMIT, QUICK).map_err(|e| said("The study could not be solved.", e))?;
    let text =
        std::fs::read_to_string(dir.join("study.frd")).map_err(|_| format!("The solver wrote no results. It said:\n{}", tools::tail(&said, 12)))?;
    ccx::read_frd(&text, problem.mesh.nodes.len()).map_err(|e| solver_said(&said, e))
}

/// What the solver itself said went wrong (its first error), which is worth more than what is
/// missing from its results; `e` when it said nothing of the kind.
fn solver_said(log: &str, e: String) -> String {
    match log.lines().find(|l| l.contains("*ERROR")) {
        Some(error) => format!("The solver said: {}", error.trim()),
        None => e,
    }
}

/// Has the solver work out the energy each element holds (N mm) with each at its own
/// stiffness level (`levels`, one for each element in the mesh's order; see
/// [`topo::modulus`]) of a material of Young's modulus `e0` (MPa) and Poisson's ratio `nu`;
/// with `volumes`, each element's volume as the solver reckons it too (mm^3). Its files
/// (`shape.inp`, `shape.dat`, `shape.log` and the rest) are left in `dir`, each run's in place
/// of the last's.
pub fn solve_shape(
    tools: &Tools,
    deck: &Deck,
    levels: &[u16],
    e0: f64,
    nu: f64,
    dir: &Path,
    cancel: &Cancel,
    volumes: bool,
) -> Result<(Vec<f64>, Option<Vec<f64>>), String> {
    if levels.len() != deck.elements() {
        return Err(format!("the solver was given {} stiffness levels for {} elements", levels.len(), deck.elements()));
    }
    if !(e0.is_finite() && e0 > 0.0) {
        return Err("the material's Young's modulus must be more than 0".into());
    }
    if !(nu.is_finite() && nu > -1.0 && nu < 0.5) {
        return Err("the material's Poisson's ratio must be more than -1 and less than 0.5".into());
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(tools::STOPPED.into());
    }
    // (A run that fails before it writes its results must not leave the last run's to be read.)
    let results = dir.join("shape.dat");
    match std::fs::remove_file(&results) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot remove the last results {}: {e}", results.display())),
    }
    std::fs::write(dir.join("shape.inp"), deck.shape_input(levels, e0, nu, volumes))
        .map_err(|e| format!("cannot write the shape's study for the solver: {e}"))?;
    let said = tools::run(&tools.ccx, &["shape"], dir, "shape.log", cancel, SOLVE_LIMIT, SHAPE_SLOW)
        .map_err(|e| said("The shape's study could not be solved.", e))?;
    let text = std::fs::read_to_string(&results).map_err(|_| format!("The solver wrote no results. It said:\n{}", tools::tail(&said, 12)))?;
    let read = |heading: &str| ccx::read_dat_elements(&text, heading, levels.len()).map_err(|e| solver_said(&said, e));
    let energies = read(ccx::ENERGY)?;
    let volumes = if volumes { Some(read(ccx::VOLUME)?) } else { None };
    Ok((energies, volumes))
}

/// How long the two programs took, in seconds, and how large the mesh was.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Took {
    pub meshing: f64,
    pub solving: f64,
}

/// A part divided into elements with a study put on them, ready to be solved.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub problem: Problem,
    /// Which face of the part each surface of the mesh is.
    pub on: BTreeMap<u32, u32>,
    /// How long the dividing took, in seconds.
    pub meshing: f64,
}

/// The part (`step`, as a STEP file's bytes) divided into elements about `size` across, and
/// `setup` put on them. `part` is the part as the kernel draws it, to tell its faces on the
/// mesh. A mesh of more than `max_nodes` nodes is refused, and so is a part not held fast.
pub fn prepare(
    tools: &Tools,
    step: &[u8],
    part: &tenon_kernel::Mesh,
    setup: &Setup,
    size: f64,
    dir: &Path,
    cancel: &Cancel,
    max_nodes: usize,
) -> Result<Prepared, String> {
    let started = Instant::now();
    let mesh = mesh_part(tools, step, size, dir, cancel)?;
    let meshing = started.elapsed().as_secs_f64();
    if mesh.nodes.len() > max_nodes {
        return Err(too_many_nodes(size, mesh.nodes.len(), max_nodes));
    }
    let on = faces::match_surfaces(&mesh, part)?;
    let curves = faces::match_curves(&mesh, part);
    let problem = Problem::build(mesh, &on, &curves, setup)?;
    match problem.free_motions() {
        0 => {}
        n => {
            return Err(format!(
                "Held as it is, the part can still move as a whole ({n} way{} of sliding or turning are left free): there is no one answer. Hold it more: a fixed face holds it fast.",
                if n == 1 { "" } else { "s" }
            ));
        }
    }
    Ok(Prepared { problem, on, meshing })
}

/// Why a mesh of `nodes` nodes is more than is taken (`max_nodes`), and what to do: in the
/// words of a shape being worked out when that is its limit ([`topo::MAX_SHAPE_NODES`]), and
/// otherwise of a stress check.
fn too_many_nodes(size: f64, nodes: usize, max_nodes: usize) -> String {
    if max_nodes == topo::MAX_SHAPE_NODES {
        format!(
            "Divided into elements of {size} mm the part has {nodes} nodes: more than a shape is worked out on ({max_nodes}). Choose Coarse, or a larger smallest member."
        )
    } else {
        format!(
            "Divided into elements of {size} mm the part has {nodes} nodes: more than a quick check solves ({max_nodes}). Make the elements larger (Mesh Settings)."
        )
    }
}

/// A study worked out from start to end: the part (`step`, as a STEP file's bytes) divided
/// into elements about `size` across, `setup` put on them, solved. `part` is the part as the
/// kernel draws it, to tell its faces on the mesh.
pub fn work_out(
    tools: &Tools,
    step: &[u8],
    part: &tenon_kernel::Mesh,
    setup: &Setup,
    size: f64,
    dir: &Path,
    cancel: &Cancel,
) -> Result<(Results, Took), String> {
    let Prepared { problem, meshing, .. } = prepare(tools, step, part, setup, size, dir, cancel, MAX_SOLVE_NODES)?;
    let started = Instant::now();
    let solution = solve(tools, &problem, dir, cancel)?;
    let solving = started.elapsed().as_secs_f64();
    Ok((Results::new(problem.mesh, solution)?, Took { meshing, solving }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_are_sized_from_the_part_when_no_size_is_said() {
        assert_eq!(element_size_for(100.0), 5.0);
        // A size that is no size is refused before anything is started.
        let tools = Tools { gmsh: "nowhere".into(), ccx: "nowhere".into() };
        let scratch = Scratch::new().unwrap();
        for size in [0.0, -1.0, f64::NAN] {
            assert!(mesh_part(&tools, b"", size, &scratch.dir, &Cancel::default()).unwrap_err().contains("more than 0"));
        }
        // A mesher that is not there says so, as the reason the part was not divided.
        let e = mesh_part(&tools, b"", 1.0, &scratch.dir, &Cancel::default()).unwrap_err();
        assert!(e.starts_with("The part could not be divided into elements.") && e.contains("cannot start"), "{e}");
        // Preparing a study starts with the dividing, and says the same.
        let part = tenon_kernel::Mesh::default();
        let setup = Setup { held: Vec::new(), pushed: Vec::new(), surfaces: Vec::new(), material: Material::of(200.0, 0.3, 7.85) };
        let e = prepare(&tools, b"", &part, &setup, 1.0, &scratch.dir, &Cancel::default(), MAX_SOLVE_NODES).unwrap_err();
        assert!(e.starts_with("The part could not be divided into elements.") && e.contains("cannot start"), "{e}");
    }

    #[test]
    fn too_many_nodes_is_said_in_the_words_of_what_was_asked() {
        assert_eq!(
            too_many_nodes(1.5, 612_345, MAX_SOLVE_NODES),
            "Divided into elements of 1.5 mm the part has 612345 nodes: more than a quick check solves (600000). Make the elements larger (Mesh Settings)."
        );
        assert_eq!(
            too_many_nodes(1.2, 140_000, topo::MAX_SHAPE_NODES),
            "Divided into elements of 1.2 mm the part has 140000 nodes: more than a shape is worked out on (100000). Choose Coarse, or a larger smallest member."
        );
    }

    #[test]
    fn a_shape_is_solved_from_fresh_files_and_what_it_cannot_take_is_refused() {
        let mesh = TetMesh::block([1, 1, 1], tenon_geom::Vec3::new(1.0, 1.0, 1.0));
        let p = Problem {
            mesh,
            material: Material::of(200.0, 0.3, 7.85),
            held: std::collections::BTreeMap::new(),
            forces: std::collections::BTreeMap::new(),
            gravity: None,
            spin: None,
        };
        let deck = Deck::new(&p).unwrap();
        let scratch = Scratch::new().unwrap();
        let tools = Tools { gmsh: "nowhere".into(), ccx: scratch.dir.join("no-solver-here") };
        let cancel = Cancel::default();
        let solve = |levels: &[u16], e0: f64, nu: f64| solve_shape(&tools, &deck, levels, e0, nu, &scratch.dir, &cancel, true).unwrap_err();
        // What the solver could not take is refused before it is started.
        assert_eq!(solve(&[1000; 7], 2e5, 0.3), "the solver was given 7 stiffness levels for 6 elements");
        for e0 in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(solve(&[1000; 6], e0, 0.3), "the material's Young's modulus must be more than 0");
        }
        for nu in [0.5, -1.0, f64::NAN] {
            assert_eq!(solve(&[1000; 6], 2e5, nu), "the material's Poisson's ratio must be more than -1 and less than 0.5");
        }
        assert!(!scratch.dir.join("shape.inp").exists(), "nothing written for what is refused");
        // The last run's results are not left to be read as this run's: they go first, and
        // this run's input is written, volumes asked for.
        std::fs::write(scratch.dir.join("shape.dat"), "the last run's").unwrap();
        let e = solve(&[0, 1000, 500, 500, 1000, 0], 2e5, 0.3);
        assert!(e.starts_with("The shape's study could not be solved.") && e.contains("cannot start"), "{e}");
        assert!(!scratch.dir.join("shape.dat").exists());
        let written = std::fs::read_to_string(scratch.dir.join("shape.inp")).unwrap();
        assert!(written.contains("*ELSET, ELSET=L500\n3, 4\n") && written.ends_with("ELSE, EVOL\n*END STEP\n"), "{written}");
        // Stopped: just that, before the solver is started or while it runs.
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(solve(&[1000; 6], 2e5, 0.3), tools::STOPPED);
        assert_eq!(said("The shape's study could not be solved.", tools::STOPPED.into()), tools::STOPPED);
        assert_eq!(said("The shape's study could not be solved.", "ccx failed".into()), "The shape's study could not be solved. ccx failed");
        // What the solver said was wrong, when it said, is worth more than what is missing.
        let log = " Using up to 4 cpu(s)\n *ERROR reading *SOLID SECTION: no material\n *ERROR again\n";
        assert_eq!(solver_said(log, "missing".into()), "The solver said: *ERROR reading *SOLID SECTION: no material");
        assert_eq!(solver_said(" Job finished\n", "missing".into()), "missing");
    }
}
