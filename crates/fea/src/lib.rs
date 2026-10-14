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

use std::path::Path;
use std::time::{Duration, Instant};

pub use ccx::Solution;
pub use mesh::TetMesh;
pub use problem::{Held, Material, Problem, Pushed, Setup, Spin, Surface};
pub use result::{Peak, Results, von_mises};
pub use tools::{Cancel, Scratch, Tools};

/// The most nodes a mesh may have to be solved: beyond it a "quick check" is neither.
pub const MAX_SOLVE_NODES: usize = 600_000;

/// How long each program may run.
const MESH_LIMIT: Duration = Duration::from_secs(10 * 60);
const SOLVE_LIMIT: Duration = Duration::from_secs(30 * 60);

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
    tools::run(&tools.gmsh, &args, dir, "mesher.log", cancel, MESH_LIMIT).map_err(|e| said("The part could not be divided into elements.", e))?;
    let text = std::fs::read_to_string(dir.join("part.msh")).map_err(|_| {
        let said = std::fs::read_to_string(dir.join("mesher.log")).unwrap_or_default();
        format!("The mesher wrote no mesh. It said:\n{}", tools::tail(&said, 12))
    })?;
    mesh::read_msh(&text)
}

/// Has the solver work a problem out. Its files are left in `dir`.
pub fn solve(tools: &Tools, problem: &Problem, dir: &Path, cancel: &Cancel) -> Result<Solution, String> {
    std::fs::write(dir.join("study.inp"), ccx::write_inp(problem)).map_err(|e| format!("cannot write the study for the solver: {e}"))?;
    let said = tools::run(&tools.ccx, &["study"], dir, "solver.log", cancel, SOLVE_LIMIT).map_err(|e| said("The study could not be solved.", e))?;
    let text =
        std::fs::read_to_string(dir.join("study.frd")).map_err(|_| format!("The solver wrote no results. It said:\n{}", tools::tail(&said, 12)))?;
    ccx::read_frd(&text, problem.mesh.nodes.len()).map_err(|e| {
        // (What the solver itself said went wrong is worth more than what is missing.)
        match said.lines().find(|l| l.contains("*ERROR")) {
            Some(error) => format!("The solver said: {}", error.trim()),
            None => e,
        }
    })
}

/// How long the two programs took, in seconds, and how large the mesh was.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Took {
    pub meshing: f64,
    pub solving: f64,
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
    let started = Instant::now();
    let mesh = mesh_part(tools, step, size, dir, cancel)?;
    let meshing = started.elapsed().as_secs_f64();
    if mesh.nodes.len() > MAX_SOLVE_NODES {
        return Err(format!(
            "Divided into elements of {size} mm the part has {} nodes: more than a quick check solves ({MAX_SOLVE_NODES}). Make the elements larger (Mesh Settings).",
            mesh.nodes.len()
        ));
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
    }
}
