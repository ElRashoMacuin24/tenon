//! The solver the optimizer asks in use: CalculiX, run once a round on the study's mesh with
//! each element at its own stiffness level (DEC-098). The mesh and the study's supports and
//! loads are written out once ([`Deck`]); each round only the levels change.

use std::path::Path;

use super::Stiffness;
use crate::ccx::Deck;
use crate::tools::{Cancel, Tools};

/// CalculiX as the optimizer's [`Stiffness`]: the energy each element holds, from one run.
/// Its files are left in `dir`, each run's in place of the last's. A run stopped by `cancel`
/// is an error of [`crate::tools::STOPPED`].
#[derive(Clone, Copy, Debug)]
pub struct Ccx<'a> {
    pub tools: &'a Tools,
    pub deck: &'a Deck,
    /// The material's Young's modulus (MPa) and Poisson's ratio.
    pub e0: f64,
    pub nu: f64,
    pub dir: &'a Path,
    pub cancel: &'a Cancel,
}

impl Stiffness for Ccx<'_> {
    fn energies(&mut self, levels: &[u16]) -> Result<Vec<f64>, String> {
        crate::solve_shape(self.tools, self.deck, levels, self.e0, self.nu, self.dir, self.cancel, false).map(|(energies, _)| energies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::TetMesh;
    use crate::problem::{Material, Problem};
    use crate::tools::{STOPPED, Scratch};
    use std::collections::BTreeMap;
    use std::sync::atomic::Ordering;
    use tenon_geom::Vec3;

    fn deck() -> Deck {
        let mesh = TetMesh::block([1, 1, 1], Vec3::new(1.0, 1.0, 1.0));
        let p = Problem { mesh, material: Material::of(200.0, 0.3, 7.85), held: BTreeMap::new(), forces: BTreeMap::new(), gravity: None, spin: None };
        Deck::new(&p).unwrap()
    }

    #[test]
    fn a_stopped_run_says_stopped_and_a_missing_solver_says_so() {
        let (deck, scratch) = (deck(), Scratch::new().unwrap());
        let tools = Tools { gmsh: "nowhere".into(), ccx: scratch.dir.join("no-solver-here") };
        let cancel = Cancel::default();
        let mut solver = Ccx { tools: &tools, deck: &deck, e0: 200_000.0, nu: 0.3, dir: &scratch.dir, cancel: &cancel };
        // A solver that is not there: said as the reason the shape's study was not solved, with
        // the input written for it all the same.
        let e = solver.energies(&[1000; 6]).unwrap_err();
        assert!(e.starts_with("The shape's study could not be solved.") && e.contains("cannot start"), "{e}");
        assert_eq!(std::fs::read_to_string(scratch.dir.join("shape.inp")).unwrap(), deck.shape_input(&[1000; 6], 200_000.0, 0.3, false));
        // Stopped: just that, for the optimizer to tell apart from a failure.
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(solver.energies(&[1000; 6]).unwrap_err(), STOPPED);
        // A level for each element, no more and no fewer.
        cancel.store(false, Ordering::Relaxed);
        assert_eq!(solver.energies(&[1000; 5]).unwrap_err(), "the solver was given 5 stiffness levels for 6 elements");
    }
}
