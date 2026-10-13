//! What a study comes to, read off a solver's answer: how hard the material is worked, how far
//! the part moves, and how far it is from yielding.

use tenon_geom::Vec3;

use crate::ccx::Solution;
use crate::mesh::TetMesh;

/// The von Mises stress of a stress (xx, yy, zz, xy, yz, zx): one figure for how hard the
/// material is worked, to hold against its yield strength.
pub fn von_mises(s: &[f64; 6]) -> f64 {
    let [xx, yy, zz, xy, yz, zx] = *s;
    (0.5 * ((xx - yy).powi(2) + (yy - zz).powi(2) + (zz - xx).powi(2)) + 3.0 * (xy * xy + yz * yz + zx * zx)).sqrt()
}

/// A value and where on the part it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak {
    pub value: f64,
    pub at: Vec3,
    pub node: u32,
}

/// A study worked out.
#[derive(Clone, Debug, PartialEq)]
pub struct Results {
    pub mesh: TetMesh,
    /// How far each node moved, mm.
    pub displacement: Vec<Vec3>,
    /// The stress at each node, MPa (xx, yy, zz, xy, yz, zx).
    pub stress: Vec<[f64; 6]>,
    /// The von Mises stress at each node, MPa.
    pub von_mises: Vec<f64>,
    pub max_von_mises: Peak,
    pub max_displacement: Peak,
}

impl Results {
    pub fn new(mesh: TetMesh, solution: Solution) -> Result<Results, String> {
        if solution.displacement.len() != mesh.nodes.len() || solution.stress.len() != mesh.nodes.len() || mesh.nodes.is_empty() {
            return Err("the solver's answer is not of this mesh".into());
        }
        let von_mises: Vec<f64> = solution.stress.iter().map(von_mises).collect();
        let peak = |values: &mut dyn Iterator<Item = f64>| {
            let (node, value) = values.enumerate().fold((0, f64::MIN), |best, (i, v)| if v > best.1 { (i, v) } else { best });
            Peak { value, at: mesh.nodes[node], node: node as u32 }
        };
        let max_von_mises = peak(&mut von_mises.iter().copied());
        let max_displacement = peak(&mut solution.displacement.iter().map(|d| d.len()));
        Ok(Results { mesh, displacement: solution.displacement, stress: solution.stress, von_mises, max_von_mises, max_displacement })
    }

    /// How many times over the material holds: its yield strength over the most it is worked.
    /// Below 1 it yields. None when nothing works it at all.
    pub fn safety_factor(&self, yield_strength: f64) -> Option<f64> {
        (self.max_von_mises.value > 0.0).then(|| yield_strength / self.max_von_mises.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::read_msh;
    use crate::mesh::tests::ONE_TET;

    #[test]
    fn von_mises_is_the_pull_of_a_bar_and_root_three_times_a_shear() {
        assert!((von_mises(&[120.0, 0.0, 0.0, 0.0, 0.0, 0.0]) - 120.0).abs() < 1e-12);
        assert!((von_mises(&[0.0, 0.0, 0.0, 40.0, 0.0, 0.0]) - 40.0 * 3f64.sqrt()).abs() < 1e-12);
        // Pressed the same from every side, nothing yields.
        assert!(von_mises(&[-50.0, -50.0, -50.0, 0.0, 0.0, 0.0]).abs() < 1e-12);
        assert!((von_mises(&[100.0, 40.0, 0.0, 30.0, 0.0, 0.0]) - (0.5 * (3600.0 + 1600.0 + 10000.0) + 2700.0f64).sqrt()).abs() < 1e-9);
    }

    #[test]
    fn results_say_where_the_most_is() {
        let mesh = read_msh(ONE_TET).unwrap();
        let mut solution = Solution { displacement: vec![Vec3::ZERO; 10], stress: vec![[0.0; 6]; 10] };
        solution.displacement[3] = Vec3::new(0.0, 0.3, -0.4);
        solution.stress[1] = [90.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        solution.stress[2] = [0.0, -30.0, 0.0, 0.0, 0.0, 0.0];
        let r = Results::new(mesh.clone(), solution.clone()).unwrap();
        assert_eq!((r.max_von_mises.value, r.max_von_mises.node, r.max_von_mises.at), (90.0, 1, Vec3::X));
        assert_eq!((r.max_displacement.value, r.max_displacement.node, r.max_displacement.at), (0.5, 3, Vec3::Z));
        assert_eq!(r.von_mises[2], 30.0);
        assert_eq!(r.safety_factor(270.0), Some(3.0));
        // Nothing works it: no factor to give. An answer of another mesh is refused.
        let idle = Results::new(mesh.clone(), Solution { displacement: vec![Vec3::ZERO; 10], stress: vec![[0.0; 6]; 10] }).unwrap();
        assert_eq!(idle.safety_factor(270.0), None);
        solution.stress.pop();
        assert!(Results::new(mesh, solution).unwrap_err().contains("not of this mesh"));
    }
}
