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
pub use problem::{Held, Loose, Material, Problem, Pushed, Setup, Spin, Surface};
pub use result::{Peak, Results, von_mises};
pub use tools::{Cancel, Scratch, Tools};

/// The most nodes a mesh may have to be solved: beyond it a "quick check" is neither.
pub const MAX_SOLVE_NODES: usize = 600_000;

/// How long each program may run.
const MESH_LIMIT: Duration = Duration::from_secs(10 * 60);
const SOLVE_LIMIT: Duration = Duration::from_secs(30 * 60);

/// What to try when a program runs too long: for a stress check, and for a shape.
const QUICK: &str = "a quick check is of fewer, larger elements";
const SHAPE_SLOW: &str = "a shape is found sooner on fewer, larger elements";

/// The size of the elements a part is divided into when none is said: a twentieth of the box
/// it fits in, corner to corner.
pub fn element_size_for(extent: f64) -> f64 {
    extent / 20.0
}

/// A program's failure with what it was for; a run that was stopped is left as that.
fn said(what: &str, e: String) -> String {
    if e == tools::STOPPED { e } else { format!("{what} {e}") }
}

/// What the mesher is told for a stress check: second-order tetrahedra, sized `size` at most and
/// finer round small curves (twelve to a full turn), written in the 2.2 format as text. On one
/// thread: with more the mesher gives a slightly different mesh each time, and the same part
/// would not give the same answer twice.
fn stress_check_args(size: f64) -> Vec<String> {
    let (largest, smallest) = (format!("{size}"), format!("{}", size / 10.0));
    ["part.step", "-3", "-order", "2", "-clmax", &largest, "-clmin", &smallest, "-clcurv", "12", "-nt", "1", "-format", "msh2", "-o", "part.msh"]
        .map(str::to_owned)
        .to_vec()
}

/// What the mesher is told for a shape, elements no smaller than `floor`: see [`mesh_for_shape`]
/// for how, and why, it differs. `to_a_turn`: how many elements go to a full turn of a round face.
fn shape_args(size: f64, floor: f64, to_a_turn: u32) -> Vec<String> {
    let (largest, smallest, to_a_turn) = (format!("{size}"), format!("{floor}"), format!("{to_a_turn}"));
    [
        "part.step",
        "-3",
        "-order",
        "2",
        "-clmax",
        &largest,
        "-clmin",
        &smallest,
        "-clcurv",
        &to_a_turn,
        "-setnumber",
        "Mesh.MeshSizeExtendFromBoundary",
        "0",
        "-setnumber",
        "Mesh.SecondOrderLinear",
        "1",
        "-nt",
        "1",
        "-format",
        "msh2",
        "-o",
        "part.msh",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Has the mesher divide the part (`step`, as a STEP file's bytes) as `args` say, elements about
/// `size` across. Its files are left in `dir`.
fn run_mesher(tools: &Tools, step: &[u8], size: f64, args: &[String], dir: &Path, cancel: &Cancel, slow: &str) -> Result<TetMesh, String> {
    if !(size.is_finite() && size > 0.0) {
        return Err("the element size must be more than 0".into());
    }
    std::fs::write(dir.join("part.step"), step).map_err(|e| format!("cannot write the part for the mesher: {e}"))?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    tools::run(&tools.gmsh, &args, dir, "mesher.log", cancel, MESH_LIMIT, slow)
        .map_err(|e| said("The part could not be divided into elements.", mesher_failed(e, dir)))?;
    let text = std::fs::read_to_string(dir.join("part.msh")).map_err(|_| {
        let said = std::fs::read_to_string(dir.join("mesher.log")).unwrap_or_default();
        format!("The mesher wrote no mesh. It said:\n{}", tools::tail(&said, 12))
    })?;
    mesh::read_msh(&text)
}

/// A failure of the mesher with what it said went wrong in place of how its log ends: it ends
/// with a summary of how many errors there were, which says nothing of them.
fn mesher_failed(e: String, dir: &Path) -> String {
    let log = std::fs::read_to_string(dir.join("mesher.log")).unwrap_or_default();
    match (e.split_once(" It said:"), mesher_error(&log)) {
        (Some((failed, _)), Some(error)) => format!("{failed} It said: {error}"),
        _ => e,
    }
}

/// The first error the mesher's log has (`Error   : Invalid boundary mesh ...`), without its
/// colours and without the summary at the end.
fn mesher_error(log: &str) -> Option<String> {
    let plain = |line: &str| {
        let mut out = String::with_capacity(line.len());
        let mut rest = line;
        while let Some((before, after)) = rest.split_once('\u{1b}') {
            out.push_str(before);
            rest = after.split_once('m').map_or("", |(_, tail)| tail);
        }
        out.push_str(rest);
        out
    };
    log.lines().map(plain).filter_map(|l| l.trim().strip_prefix("Error").map(|e| e.trim_start_matches([' ', ':']).trim().to_owned())).find(|e| {
        !(e.is_empty() || e.starts_with("---") || e.starts_with("Mesh generation error summary") || e.starts_with("Check the full log"))
            && !e.ends_with(" warning")
            && !e.ends_with(" warnings")
            && !e.ends_with(" error")
            && !e.ends_with(" errors")
    })
}

/// Divides a part into ten-node tetrahedra about `size` across, for a stress check. `step` is
/// the part as a STEP file's bytes. The mesher's files are left in `dir`.
pub fn mesh_part(tools: &Tools, step: &[u8], size: f64, dir: &Path, cancel: &Cancel) -> Result<TetMesh, String> {
    run_mesher(tools, step, size, &stress_check_args(size), dir, cancel, QUICK)
}

/// The smallest element a shape's mesh may have: a third of the size, and a third of the part's
/// shortest closed edge (`closed_edge`, as [`faces::shortest_closed_edge`] finds it). The elements
/// of a round hole's wall are made as large as they may be, and on a hole much narrower than that
/// the wall folds flat: the mesher then fails, or worse, gives a mesh with no hole there at all.
fn shape_floor(size: f64, closed_edge: Option<f64>) -> f64 {
    let third = size / 3.0;
    closed_edge.filter(|c| c.is_finite() && *c > 0.0).map_or(third, |c| third.min(c / 3.0))
}

/// How many elements go to a full turn of a round face, as a stress check has it.
const TO_A_TURN: u32 = 12;

/// How many times smaller than the size a stress check's elements may be, and so a shape's when
/// what [`shape_floor`] says will not do.
const STRESS_CHECK_FLOOR: f64 = 10.0;

/// How many elements go to a full turn of a round face, at the first try. The floor is the mesher's
/// for the whole part, so one taken from a small closed edge lets every round face go by its
/// curvature down to it, and at twelve to a full turn one small hole doubled the nodes of a plate
/// with others. So with such a floor it is four: the small hole has what its floor gives it, and
/// the other round faces about what they would have had. (The second try is twelve whatever the
/// floor: what it costs in nodes is better than no mesh.)
fn to_a_turn_at_first(size: f64, floor: f64) -> u32 {
    if floor < size / 3.0 { 4 } else { TO_A_TURN }
}

/// How small the elements may go when `floor` would not do: as small as a stress check's, and in
/// any case half the first floor.
fn second_floor(size: f64, floor: f64) -> f64 {
    (size / STRESS_CHECK_FLOOR).min(floor / 2.0)
}

/// Whether both tries at a size go by the closed edge (`closed_edge`, its length) and not by the
/// size: a hole that closes up then does so whatever size of element is asked for, near this one.
pub fn only_the_closed_edge_matters(size: f64, closed_edge: Option<f64>) -> bool {
    closed_edge.is_some() && second_floor(size, shape_floor(size, closed_edge)) < size / STRESS_CHECK_FLOOR
}

/// A mesh by `divide` with elements no smaller than `floor`. Should that fail, or come out with a
/// hole closed up, once more with elements as small as [`second_floor`] says: what it costs in
/// nodes is better than no mesh. (Not when the mesher ran out of time: the second try is the
/// slower.) What is said of the first failure stands if both fail. A mesh is sound when its
/// boundary is closed, but for the part's own lines of no thickness (`own_lines`).
fn divided_at_floor(
    size: f64,
    floor: f64,
    own_lines: &[(tenon_geom::Vec3, tenon_geom::Vec3)],
    divide: &mut dyn FnMut(f64) -> Result<TetMesh, String>,
) -> Result<TetMesh, String> {
    let sound = |mesh: TetMesh| {
        if faces::all_along(&mesh, &mesh.open_sides(), own_lines, 0.1 * size) {
            Ok(mesh)
        } else {
            Err(format!(
                "The part could not be divided into elements of {} mm: a small hole or round closed up at that size.",
                (size * 1000.0).round() / 1000.0
            ))
        }
    };
    match divide(floor).and_then(sound) {
        Err(e) if e != tools::STOPPED && !tools::ran_too_long(&e) => match divide(second_floor(size, floor)).and_then(sound) {
            Ok(mesh) => Ok(mesh),
            Err(later) if later == tools::STOPPED => Err(later),
            Err(_) => Err(e),
        },
        first => first,
    }
}

/// Divides a part into ten-node tetrahedra about `size` across, for a shape to be worked out on.
/// `closed_edge` is the length of the part's shortest closed edge, if it has one, and `own_lines`
/// the lines where the part touches itself ([`faces::lines_of_no_thickness`]). The mesher's files
/// are left in `dir`.
///
/// It differs from [`mesh_part`] in three ways: no finer than a third of `size` round small
/// details (unless a closed edge takes it, see [`shape_floor`]), the size not carried in from the
/// boundary, and the mid-side nodes straight. The density filter works over at least one and a
/// half elements, so a shape cannot use finer detail than that, and a stress check's grading only
/// costs nodes (a plate with a few small holes came to three times the nodes the size was picked
/// for, and one with many to seventy times). Straight mid-side nodes cannot turn an element
/// inside out, as curved ones do on a small hole at these sizes (the solver refuses those), and a
/// shape is a guide: it does not need the bore's roundness.
pub fn mesh_for_shape(
    tools: &Tools,
    step: &[u8],
    size: f64,
    closed_edge: Option<f64>,
    own_lines: &[(tenon_geom::Vec3, tenon_geom::Vec3)],
    dir: &Path,
    cancel: &Cancel,
) -> Result<TetMesh, String> {
    let first = shape_floor(size, closed_edge);
    divided_at_floor(size, first, own_lines, &mut |floor| {
        let to_a_turn = if floor == first { to_a_turn_at_first(size, first) } else { TO_A_TURN };
        run_mesher(tools, step, size, &shape_args(size, floor, to_a_turn), dir, cancel, SHAPE_SLOW)
    })
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

/// The part (`step`, as a STEP file's bytes) divided into elements about `size` across as for a
/// stress check, and `setup` put on them. `part` is the part as the kernel draws it, to tell its
/// faces on the mesh. A mesh of more than [`MAX_SOLVE_NODES`] nodes is refused, and so is a part
/// not held fast.
pub fn prepare(
    tools: &Tools,
    step: &[u8],
    part: &tenon_kernel::Mesh,
    setup: &Setup,
    size: f64,
    dir: &Path,
    cancel: &Cancel,
) -> Result<Prepared, String> {
    let started = Instant::now();
    let mesh = mesh_part(tools, step, size, dir, cancel)?;
    let meshing = started.elapsed().as_secs_f64();
    if mesh.nodes.len() > MAX_SOLVE_NODES {
        return Err(too_many_nodes(size, mesh.nodes.len()));
    }
    put_on(mesh, part, setup, meshing)
}

/// `setup` put on a mesh already made (`meshing`: how long that took, in seconds). `part` is the
/// part as the kernel draws it, to tell its faces on the mesh. A part not held fast is refused.
pub fn put_on(mesh: TetMesh, part: &tenon_kernel::Mesh, setup: &Setup, meshing: f64) -> Result<Prepared, String> {
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
    let Loose { pieces, loose } = problem.loose_pieces();
    if loose > 0 {
        let which = if loose == 1 { "one of them is".to_owned() } else { format!("{loose} of them are") };
        return Err(format!("The part is in {pieces} pieces, and {which} not held fast: there is no one answer. Hold every piece, or join them up."));
    }
    Ok(Prepared { problem, on, meshing })
}

/// Why a mesh of `nodes` nodes is more than a stress check takes, and what to do. (A shape has
/// words of its own, with what helps for what it was asked: `tenon_io::shape`.)
fn too_many_nodes(size: f64, nodes: usize) -> String {
    format!(
        "Divided into elements of {size} mm the part has {nodes} nodes: more than a quick check solves ({MAX_SOLVE_NODES}). Make the elements larger (Mesh Settings)."
    )
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
    let Prepared { problem, meshing, .. } = prepare(tools, step, part, setup, size, dir, cancel)?;
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
            assert!(mesh_for_shape(&tools, b"", size, None, &[], &scratch.dir, &Cancel::default()).unwrap_err().contains("more than 0"));
        }
        // A mesher that is not there says so, as the reason the part was not divided.
        let e = mesh_part(&tools, b"", 1.0, &scratch.dir, &Cancel::default()).unwrap_err();
        assert!(e.starts_with("The part could not be divided into elements.") && e.contains("cannot start"), "{e}");
        let e = mesh_for_shape(&tools, b"", 1.0, None, &[], &scratch.dir, &Cancel::default()).unwrap_err();
        assert!(e.starts_with("The part could not be divided into elements.") && e.contains("cannot start"), "{e}");
        // Preparing a study starts with the dividing, and says the same.
        let part = tenon_kernel::Mesh::default();
        let setup = Setup { held: Vec::new(), pushed: Vec::new(), surfaces: Vec::new(), material: Material::of(200.0, 0.3, 7.85) };
        let e = prepare(&tools, b"", &part, &setup, 1.0, &scratch.dir, &Cancel::default()).unwrap_err();
        assert!(e.starts_with("The part could not be divided into elements.") && e.contains("cannot start"), "{e}");
    }

    #[test]
    fn a_stress_check_and_a_shape_are_meshed_as_each_needs() {
        // A stress check's mesh is what its results were judged on: these words do not change.
        assert_eq!(stress_check_args(2.5).join(" "), "part.step -3 -order 2 -clmax 2.5 -clmin 0.25 -clcurv 12 -nt 1 -format msh2 -o part.msh");
        // A shape's has a floor on the size, and straight mid-side nodes.
        assert_eq!(
            shape_args(3.0, shape_floor(3.0, None), to_a_turn_at_first(3.0, shape_floor(3.0, None))).join(" "),
            "part.step -3 -order 2 -clmax 3 -clmin 1 -clcurv 12 -setnumber Mesh.MeshSizeExtendFromBoundary 0 -setnumber Mesh.SecondOrderLinear 1 -nt 1 -format msh2 -o part.msh"
        );
        // With a floor taken from a small closed edge, the other round faces do not go down to it.
        assert_eq!(
            shape_args(3.0, shape_floor(3.0, Some(1.5)), to_a_turn_at_first(3.0, shape_floor(3.0, Some(1.5)))).join(" "),
            "part.step -3 -order 2 -clmax 3 -clmin 0.5 -clcurv 4 -setnumber Mesh.MeshSizeExtendFromBoundary 0 -setnumber Mesh.SecondOrderLinear 1 -nt 1 -format msh2 -o part.msh"
        );
    }

    #[test]
    fn a_shapes_elements_stay_small_enough_to_keep_a_closed_edge_open() {
        // A third of the size, unless a closed edge is shorter than the size.
        assert_eq!(shape_floor(3.0, None), 1.0);
        assert_eq!(shape_floor(3.0, Some(30.0)), 1.0);
        assert_eq!(shape_floor(6.0, Some(3.0)), 1.0);
        // (Nothing to go by, nothing taken from it.)
        assert_eq!(shape_floor(3.0, Some(0.0)), 1.0);
        assert_eq!(shape_floor(3.0, Some(f64::NAN)), 1.0);
    }

    /// A mesh of one tetrahedron, closed.
    fn closed() -> TetMesh {
        mesh::read_msh(mesh::tests::ONE_TET).unwrap()
    }

    /// That mesh with a surface folded onto one of its sides, as a hole closed up leaves it.
    fn folded() -> TetMesh {
        let mut m = closed();
        let t = m.tris[0];
        m.tris.push(t);
        m
    }

    #[test]
    fn a_shape_is_divided_again_more_finely_when_its_floor_will_not_do() {
        let run = |answers: Vec<Result<TetMesh, String>>| {
            let mut floors = Vec::new();
            let mut answers = answers.into_iter();
            let got = divided_at_floor(3.0, 1.0, &[], &mut |floor| {
                floors.push(floor);
                answers.next().unwrap()
            });
            (got, floors)
        };
        // Good at once: one mesh.
        let (got, floors) = run(vec![Ok(closed())]);
        assert!(got.is_ok() && floors == [1.0], "{floors:?}");
        // The mesher fails at the floor: again, as finely as a stress check.
        let (got, floors) = run(vec![Err("it failed".into()), Ok(closed())]);
        assert!(got.is_ok() && floors == [1.0, 0.3], "{floors:?}");
        // A hole closed up with no word from the mesher is the same.
        let (got, floors) = run(vec![Ok(folded()), Ok(closed())]);
        assert!(got.is_ok() && floors == [1.0, 0.3], "{floors:?}");
        // Failing twice: what the first said.
        let (got, floors) = run(vec![Err("it failed".into()), Err("it failed again".into())]);
        assert_eq!((got.unwrap_err().as_str(), floors), ("it failed", vec![1.0, 0.3]));
        let (got, _) = run(vec![Ok(folded()), Ok(folded())]);
        assert_eq!(got.unwrap_err(), "The part could not be divided into elements of 3 mm: a small hole or round closed up at that size.");
        // Being stopped is being stopped, and nothing more is started.
        let (got, floors) = run(vec![Err(tools::STOPPED.into())]);
        assert_eq!((got.unwrap_err().as_str(), floors), (tools::STOPPED, vec![1.0]));
        let (got, floors) = run(vec![Err("it failed".into()), Err(tools::STOPPED.into())]);
        assert_eq!((got.unwrap_err().as_str(), floors), (tools::STOPPED, vec![1.0, 0.3]));
        // The mesher out of time: the second try is the slower, and would only come to the same.
        let late = "The part could not be divided into elements. gmsh ran for more than 10 minutes and was stopped: a shape is found sooner";
        let (got, floors) = run(vec![Err(late.into())]);
        assert_eq!((got.unwrap_err().as_str(), floors), (late, vec![1.0]));
    }

    #[test]
    fn a_line_of_no_thickness_the_part_has_is_not_a_hole_closed_up() {
        // Sides with more than two triangles, all along a line of the part's own: that mesh stands.
        let mesh = folded();
        let t = mesh.tris[0];
        let at = |i: usize| mesh.nodes[t.nodes[i] as usize];
        let own = [(at(0), at(1)), (at(1), at(2)), (at(2), at(0))];
        let mut tries = 0;
        let got = divided_at_floor(3.0, 1.0, &own, &mut |_| {
            tries += 1;
            Ok(folded())
        });
        assert!(got.is_ok() && tries == 1, "{tries}");
        // A line of the part's own elsewhere, or along only some of them, does not excuse it.
        let far = tenon_geom::Vec3::new(50.0, 50.0, 50.0);
        for elsewhere in [vec![(far, far + tenon_geom::Vec3::X)], vec![(at(0), at(1))]] {
            let got = divided_at_floor(3.0, 1.0, &elsewhere, &mut |_| Ok(folded()));
            assert!(got.unwrap_err().contains("closed up"));
        }
    }

    #[test]
    fn a_floor_taken_from_a_closed_edge_still_has_a_finer_second_try() {
        // A floor from a small closed edge may be under a tenth of the size already. The second
        // try goes finer still, not back to the same (or up): a hole that closed up at the first
        // holds at half of it.
        let mut floors = Vec::new();
        let got = divided_at_floor(3.0, 0.1, &[], &mut |floor| {
            floors.push(floor);
            if floors.len() == 1 { Ok(folded()) } else { Ok(closed()) }
        });
        assert!(got.is_ok() && floors == [0.1, 0.05], "{floors:?}");
        // Exactly at a tenth of the size, too.
        let mut floors = Vec::new();
        let _ = divided_at_floor(3.0, 0.3, &[], &mut |floor| {
            floors.push(floor);
            Err("it failed".into())
        });
        assert_eq!(floors, [0.3, 0.15]);
        // While the tenth is the smaller, it is the tenth.
        assert_eq!(second_floor(3.0, 1.0), 0.3);
        // Whether the size of the elements has any say. A closed edge of 0.3 at elements of 2.4:
        // the floors are 0.1 and 0.05, and at elements of 1.2 they are the same.
        assert!(only_the_closed_edge_matters(2.4, Some(0.3)));
        assert!(only_the_closed_edge_matters(1.2, Some(0.3)));
        // A closed edge not much under the size: the second floor is the size's tenth.
        assert!(!only_the_closed_edge_matters(3.0, Some(2.0)));
        // None to go by.
        assert!(!only_the_closed_edge_matters(3.0, None));
    }

    #[test]
    fn what_the_mesher_said_is_found_in_its_log() {
        let log = "Info    : Meshing 3D...\n\u{1b}[1m\u{1b}[31mError   : Invalid boundary mesh (overlapping facets) on surface 7 surface 7\u{1b}[0m\n\u{1b}[35mWarning : No elements in volume 1\u{1b}[0m\nInfo    : [100%] Meshing volume 1 order 2\nError   : ------------------------------\nError   : Mesh generation error summary\nError   :     1 warning\nError   :     1 error\nError   : Check the full log for details\nError   : ------------------------------\nInfo    : Done\n";
        assert_eq!(mesher_error(log).as_deref(), Some("Invalid boundary mesh (overlapping facets) on surface 7 surface 7"));
        assert_eq!(mesher_error("Info    : all well\n"), None);
        assert_eq!(mesher_error("Error   : Mesh generation error summary\nError   :     2 errors\n"), None);
        // Put in place of how the log ends, and left as it is when it does not say.
        let scratch = Scratch::new().unwrap();
        std::fs::write(scratch.dir.join("mesher.log"), log).unwrap();
        let tail = "gmsh failed (exit code: 1). It said:\nInfo    : Done".to_owned();
        assert_eq!(
            mesher_failed(tail.clone(), &scratch.dir),
            "gmsh failed (exit code: 1). It said: Invalid boundary mesh (overlapping facets) on surface 7 surface 7"
        );
        assert_eq!(mesher_failed("cannot start gmsh".into(), &scratch.dir), "cannot start gmsh");
        std::fs::write(scratch.dir.join("mesher.log"), "Info    : Done\n").unwrap();
        assert_eq!(mesher_failed(tail.clone(), &scratch.dir), tail);
    }

    #[test]
    fn too_many_nodes_is_said_in_the_words_of_a_stress_check() {
        assert_eq!(
            too_many_nodes(1.5, 612_345),
            "Divided into elements of 1.5 mm the part has 612345 nodes: more than a quick check solves (600000). Make the elements larger (Mesh Settings)."
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
