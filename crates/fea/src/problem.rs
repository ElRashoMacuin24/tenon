//! A study put onto a mesh: which ways each node is held, and what force is on each.
//!
//! This is what a solver is asked: a mesh, a material, held directions, forces on nodes and
//! the pull of the part's own weight. Nothing here is a particular solver's: supports become
//! directions a node may not move along (so a fixed face, a sliding face and a pin are one
//! thing, and a node on two of them is held by both), and loads become forces on nodes, each
//! triangle's share worked out over its curved surface.

use std::collections::BTreeMap;

use tenon_geom::Vec3;
use tenon_model::study::{LoadKind, SupportKind};

use crate::mesh::TetMesh;

/// What the part is made of, in the units the solver is given: megapascals, and tonnes per
/// cubic millimetre (so that millimetres, newtons and seconds go together).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub youngs_modulus: f64,
    pub poisson: f64,
    pub density: f64,
}

impl Material {
    /// From Young's modulus in GPa and density in g/cm^3, as the part has them.
    pub fn of(youngs_modulus_gpa: f64, poisson: f64, density_g_cm3: f64) -> Material {
        Material { youngs_modulus: youngs_modulus_gpa * 1000.0, poisson, density: density_g_cm3 * 1.0e-9 }
    }
}

/// What a face of the part is, as far as a support needs to know.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Surface {
    /// Flat, looking along `normal` (out of the part).
    Plane {
        normal: Vec3,
    },
    /// Round, about the axis through `origin` along `dir`.
    Cylinder {
        origin: Vec3,
        dir: Vec3,
    },
    Other,
}

/// A support of the study, on faces of the part by their numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub name: String,
    pub kind: SupportKind,
    pub faces: Vec<u32>,
}

/// A load of the study, on faces of the part by their numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Pushed {
    pub name: String,
    pub kind: LoadKind,
    pub faces: Vec<u32>,
}

/// A study as it is put to the mesh: what is not suppressed of it, its faces found on the part.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub held: Vec<Held>,
    pub pushed: Vec<Pushed>,
    /// What each face of the part is, by its number.
    pub surfaces: Vec<Surface>,
    pub material: Material,
}

/// What a solver is asked.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub mesh: TetMesh,
    pub material: Material,
    /// For each node that is held: the directions it may not move along (unit vectors, at
    /// right angles to each other, one to three of them).
    pub held: BTreeMap<u32, Vec<Vec3>>,
    /// The force on each loaded node, N.
    pub forces: BTreeMap<u32, Vec3>,
    /// The acceleration of the part's own weight, mm/s^2, when the study has it.
    pub gravity: Option<Vec3>,
}

/// Adds `d` to a set of directions at right angles to each other, if it is not already
/// spanned by them.
fn hold(set: &mut Vec<Vec3>, d: Vec3) {
    let rest = set.iter().fold(d, |r, e| r - *e * r.dot(*e));
    if set.len() < 3 && rest.len() > 1e-6 {
        set.push(rest * (1.0 / rest.len()));
    }
}

impl Problem {
    /// Puts a study onto the mesh. `on` says which face of the part each surface of the mesh is.
    pub fn build(mesh: TetMesh, on: &BTreeMap<u32, u32>, setup: &Setup) -> Result<Problem, String> {
        let face_of = |surface: u32| on.get(&surface).copied();
        let surface_of = |face: u32| setup.surfaces.get(face as usize).copied().unwrap_or(Surface::Other);
        let outward = mesh.outward()?;

        // ---- supports: the directions each node is held along
        let mut held: BTreeMap<u32, Vec<Vec3>> = BTreeMap::new();
        for s in &setup.held {
            let mut any = false;
            for t in mesh.tris.iter().filter(|t| face_of(t.surface).is_some_and(|f| s.faces.contains(&f))) {
                any = true;
                let shape = face_of(t.surface).map_or(Surface::Other, surface_of);
                for n in t.nodes {
                    let p = mesh.nodes[n as usize];
                    let set = held.entry(n).or_default();
                    match (s.kind, shape) {
                        (SupportKind::Fixed, _) => [Vec3::X, Vec3::Y, Vec3::Z].into_iter().for_each(|d| hold(set, d)),
                        (SupportKind::Frictionless, Surface::Plane { normal }) => hold(set, normal),
                        (SupportKind::Frictionless, Surface::Cylinder { origin, dir }) => {
                            let r = (p - origin) - dir * (p - origin).dot(dir);
                            hold(set, r);
                        }
                        (SupportKind::Pin { radial, axial, tangential }, Surface::Cylinder { origin, dir }) => {
                            let r = (p - origin) - dir * (p - origin).dot(dir);
                            if radial {
                                hold(set, r);
                            }
                            if axial {
                                hold(set, dir);
                            }
                            if tangential {
                                hold(set, dir.cross(r));
                            }
                        }
                        (SupportKind::Pin { .. }, _) => {
                            return Err(format!("{}: a pin holds round (cylindrical) faces, and one of its faces is not", s.name));
                        }
                        (SupportKind::Frictionless, Surface::Other) => {
                            return Err(format!(
                                "{}: a frictionless support is on flat or round (cylindrical) faces, and one of its faces is neither",
                                s.name
                            ));
                        }
                    }
                }
            }
            if !any {
                return Err(format!("{}: none of the mesh lies on its faces", s.name));
            }
        }
        held.retain(|_, set| !set.is_empty());

        // ---- loads: the force on each node
        let mut forces: BTreeMap<u32, Vec3> = BTreeMap::new();
        let mut gravity = None;
        for l in &setup.pushed {
            if let LoadKind::Gravity { direction, acceleration } = l.kind {
                gravity = Some(direction.normalized() * acceleration);
                continue;
            }
            let on_it: Vec<usize> = (0..mesh.tris.len()).filter(|i| face_of(mesh.tris[*i].surface).is_some_and(|f| l.faces.contains(&f))).collect();
            let area: f64 = on_it.iter().map(|i| mesh.area(&mesh.tris[*i])).sum();
            if area <= 0.0 {
                return Err(format!("{}: none of the mesh lies on its faces", l.name));
            }
            // What pushes on a unit of area: along one direction, or square to the surface
            // and into it.
            let (along, into): (Option<Vec3>, f64) = match l.kind {
                LoadKind::Force { newtons, direction: Some(d) } => (Some(d.normalized() * (newtons / area)), 0.0),
                LoadKind::Force { newtons, direction: None } => (None, newtons / area),
                LoadKind::Pressure { megapascals } => (None, megapascals),
                LoadKind::Gravity { .. } => (None, 0.0),
            };
            for i in on_it {
                let t = &mesh.tris[i];
                for (da, weights) in mesh.quadrature(t) {
                    // (da is square to the surface as the triangle winds: `outward` turns it
                    // out of the solid, and a push is the other way.)
                    let push = match along {
                        Some(per_area) => per_area * da.len(),
                        None => da * (-into * outward[i]),
                    };
                    for (k, n) in t.nodes.iter().enumerate() {
                        let f = forces.entry(*n).or_insert(Vec3::ZERO);
                        *f = *f + push * weights[k];
                    }
                }
            }
        }
        Ok(Problem { mesh, material: setup.material, held, forces, gravity })
    }

    /// The whole of the forces on nodes (not the part's weight).
    pub fn total_force(&self) -> Vec3 {
        self.forces.values().fold(Vec3::ZERO, |sum, f| sum + *f)
    }

    /// How many ways the part can still move as a whole, held as it is: 0 when it is held
    /// fast; up to 6 (three ways to slide, three to turn) when nothing holds it. A part that
    /// can still move has no one answer.
    pub fn free_motions(&self) -> usize {
        // A move of the whole part is a slide t and a turn w about a point c: a node at p
        // moves by t + w x (p - c). Held along d, it gives d.t + (r x d).w = 0. The ways left
        // are six less the number of such conditions that are independent.
        let Some(first) = self.mesh.nodes.first() else { return 6 };
        let (lo, hi) = self.mesh.nodes.iter().fold((*first, *first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        let (c, size) = ((lo + hi) * 0.5, (hi - lo).len().max(1e-9));
        let mut rows: Vec<[f64; 6]> = Vec::new();
        for (n, dirs) in &self.held {
            // (Lengths scaled by the part's size, so that sliding and turning weigh alike.)
            let r = (self.mesh.nodes[*n as usize] - c) * (1.0 / size);
            for d in dirs {
                let m = r.cross(*d);
                let mut row = [d.x, d.y, d.z, m.x, m.y, m.z];
                for e in &rows {
                    let dot: f64 = (0..6).map(|i| row[i] * e[i]).sum();
                    (0..6).for_each(|i| row[i] -= dot * e[i]);
                }
                let len = row.iter().map(|v| v * v).sum::<f64>().sqrt();
                if len > 1e-6 {
                    rows.push(row.map(|v| v / len));
                    if rows.len() == 6 {
                        return 0;
                    }
                }
            }
        }
        6 - rows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::read_msh;
    use crate::mesh::tests::ONE_TET;

    /// The unit tetrahedron: the mesher's surfaces 1 to 4 (z = 0, y = 0, x = 0, slanted) as the
    /// part's faces 0 to 3.
    fn tet() -> (TetMesh, BTreeMap<u32, u32>, Vec<Surface>) {
        let on = (1..=4).map(|s| (s, s - 1)).collect();
        let slant = Vec3::new(1.0, 1.0, 1.0).normalized();
        let surfaces = vec![
            Surface::Plane { normal: -Vec3::Z },
            Surface::Plane { normal: -Vec3::Y },
            Surface::Plane { normal: -Vec3::X },
            Surface::Plane { normal: slant },
        ];
        (read_msh(ONE_TET).unwrap(), on, surfaces)
    }

    fn steel() -> Material {
        Material::of(200.0, 0.29, 7.85)
    }

    fn setup(held: Vec<Held>, pushed: Vec<Pushed>, surfaces: Vec<Surface>) -> Setup {
        Setup { held, pushed, surfaces, material: steel() }
    }

    fn fixed(faces: &[u32]) -> Held {
        Held { name: "Fixed1".into(), kind: SupportKind::Fixed, faces: faces.to_vec() }
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).len() < 1e-12
    }

    #[test]
    fn a_materials_units_are_the_solvers() {
        let m = steel();
        assert_eq!((m.youngs_modulus, m.poisson), (200_000.0, 0.29));
        assert!((m.density - 7.85e-9).abs() < 1e-20);
    }

    #[test]
    fn a_force_along_a_direction_is_spread_over_the_face_and_adds_up() {
        let (mesh, on, surfaces) = tet();
        let push =
            Pushed { name: "Force1".into(), kind: LoadKind::Force { newtons: 30.0, direction: Some(Vec3::new(0.0, 0.0, -2.0)) }, faces: vec![3] };
        let p = Problem::build(mesh, &on, &setup(vec![fixed(&[0])], vec![push], surfaces)).unwrap();
        assert!(near(p.total_force(), Vec3::new(0.0, 0.0, -30.0)), "{:?}", p.total_force());
        // A flat six-node triangle under an even push: nothing on its corners, a third on the
        // node in the middle of each side.
        let on_node = |n: u32| p.forces.get(&n).copied().unwrap_or(Vec3::ZERO);
        for corner in [1, 2, 3] {
            assert!(on_node(corner).len() < 1e-12, "{:?}", on_node(corner));
        }
        for mid in [5, 8, 9] {
            assert!(near(on_node(mid), Vec3::new(0.0, 0.0, -10.0)), "{:?}", on_node(mid));
        }
        assert_eq!(p.forces.len(), 6);
        assert_eq!(p.gravity, None);
    }

    #[test]
    fn a_pressure_and_a_force_square_to_a_face_push_into_it() {
        let (mesh, on, surfaces) = tet();
        // 4 MPa on the face at y = 0 (area a half): 2 N along +y, into the part.
        let press = Pushed { name: "Pressure1".into(), kind: LoadKind::Pressure { megapascals: 4.0 }, faces: vec![1] };
        let p = Problem::build(mesh.clone(), &on, &setup(vec![fixed(&[0])], vec![press], surfaces.clone())).unwrap();
        assert!(near(p.total_force(), Vec3::new(0.0, 2.0, 0.0)), "{:?}", p.total_force());
        // A force of 6 N square to the slanted face: into the part, along -(1, 1, 1).
        let force = Pushed { name: "Force1".into(), kind: LoadKind::Force { newtons: 6.0, direction: None }, faces: vec![3] };
        let p = Problem::build(mesh.clone(), &on, &setup(vec![fixed(&[0])], vec![force], surfaces.clone())).unwrap();
        assert!(near(p.total_force(), Vec3::new(1.0, 1.0, 1.0).normalized() * -6.0), "{:?}", p.total_force());
        // A negative one pulls. Spread over two faces it is shared by their areas.
        let pull = Pushed { name: "Force1".into(), kind: LoadKind::Force { newtons: -8.0, direction: None }, faces: vec![1, 2] };
        let p = Problem::build(mesh.clone(), &on, &setup(vec![fixed(&[0])], vec![pull], surfaces.clone())).unwrap();
        assert!(near(p.total_force(), Vec3::new(-4.0, -4.0, 0.0)), "{:?}", p.total_force());
        // The same whichever way the mesher wound the triangle.
        let mut wound = mesh;
        wound.tris[1].nodes = [0, 3, 1, 7, 9, 4];
        let press = Pushed { name: "Pressure1".into(), kind: LoadKind::Pressure { megapascals: 4.0 }, faces: vec![1] };
        let p = Problem::build(wound, &on, &setup(vec![fixed(&[0])], vec![press], surfaces)).unwrap();
        assert!(near(p.total_force(), Vec3::new(0.0, 2.0, 0.0)), "{:?}", p.total_force());
    }

    #[test]
    fn the_parts_weight_is_an_acceleration_and_not_forces_on_nodes() {
        let (mesh, on, surfaces) = tet();
        let weight =
            Pushed { name: "Gravity1".into(), kind: LoadKind::Gravity { direction: Vec3::new(0.0, 0.0, -3.0), acceleration: 9810.0 }, faces: vec![] };
        let p = Problem::build(mesh, &on, &setup(vec![fixed(&[0])], vec![weight], surfaces)).unwrap();
        assert_eq!((p.gravity, p.forces.len()), (Some(Vec3::new(0.0, 0.0, -9810.0)), 0));
    }

    #[test]
    fn supports_are_directions_a_node_may_not_move_along() {
        let (mesh, on, surfaces) = tet();
        let slide = |faces: &[u32]| Held { name: "Frictionless1".into(), kind: SupportKind::Frictionless, faces: faces.to_vec() };
        // Fixed on z = 0: its six nodes held every way; the part cannot move.
        let p = Problem::build(mesh.clone(), &on, &setup(vec![fixed(&[0])], vec![], surfaces.clone())).unwrap();
        assert_eq!(p.held.len(), 6);
        assert!(p.held.values().all(|d| d.len() == 3));
        assert_eq!(p.free_motions(), 0);
        // Sliding on z = 0: held along z only. It can still slide two ways and turn one.
        let p = Problem::build(mesh.clone(), &on, &setup(vec![slide(&[0])], vec![], surfaces.clone())).unwrap();
        assert!(p.held.values().all(|d| d.len() == 1 && near(d[0], -Vec3::Z)));
        assert_eq!(p.free_motions(), 3);
        // Sliding on z = 0 and on y = 0: the nodes of the edge they share are held both ways.
        // The part can only slide along x.
        let p = Problem::build(mesh.clone(), &on, &setup(vec![slide(&[0, 1])], vec![], surfaces.clone())).unwrap();
        assert_eq!(p.held[&0].len(), 2, "the corner on both faces");
        assert_eq!(p.held[&2].len(), 1, "a corner on one");
        assert_eq!(p.free_motions(), 1);
        // On three faces at right angles: held fast, though no node is fixed but the corner.
        let p = Problem::build(mesh.clone(), &on, &setup(vec![slide(&[0, 1, 2])], vec![], surfaces.clone())).unwrap();
        assert_eq!((p.held[&0].len(), p.free_motions()), (3, 0));
        // Nothing held: six ways.
        let p = Problem::build(mesh.clone(), &on, &setup(vec![], vec![], surfaces.clone())).unwrap();
        assert_eq!(p.free_motions(), 6);
        // A support with none of the mesh on its faces says so.
        let e = Problem::build(mesh, &on, &setup(vec![fixed(&[9])], vec![], surfaces)).unwrap_err();
        assert!(e.contains("Fixed1: none of the mesh lies on its faces"), "{e}");
    }

    #[test]
    fn a_pin_holds_across_along_or_round_its_axis() {
        let (mesh, on, mut surfaces) = tet();
        // (For the test, the face at z = 0 said to be round about the z axis through 0.25, 0.25.)
        surfaces[0] = Surface::Cylinder { origin: Vec3::new(0.25, 0.25, 0.0), dir: Vec3::Z };
        let pin = |radial, axial, tangential| Held { name: "Pin1".into(), kind: SupportKind::Pin { radial, axial, tangential }, faces: vec![0] };
        let p = Problem::build(mesh.clone(), &on, &setup(vec![pin(true, true, false)], vec![], surfaces.clone())).unwrap();
        // Node 1 at (1, 0, 0): across the pin is along (0.75, -0.25, 0); along it is z.
        let d = &p.held[&1];
        assert_eq!(d.len(), 2);
        assert!(near(d[0], Vec3::new(0.75, -0.25, 0.0).normalized()) && near(d[1], Vec3::Z), "{d:?}");
        // Free to turn: one way left.
        assert_eq!(p.free_motions(), 1);
        // Held from turning too: none.
        let p = Problem::build(mesh.clone(), &on, &setup(vec![pin(true, true, true)], vec![], surfaces.clone())).unwrap();
        assert_eq!((p.held[&1].len(), p.free_motions()), (3, 0));
        // Along only: it slides across and turns.
        let p = Problem::build(mesh.clone(), &on, &setup(vec![pin(false, true, false)], vec![], surfaces.clone())).unwrap();
        assert!(p.held.values().all(|d| d.len() == 1 && near(d[0], Vec3::Z)));
        // On a flat face a pin is refused, and a sliding support on a face that is neither.
        let flat = tet().2;
        assert!(Problem::build(mesh.clone(), &on, &setup(vec![pin(true, true, false)], vec![], flat)).unwrap_err().contains("a pin holds round"));
        surfaces[0] = Surface::Other;
        let slide = Held { name: "Frictionless1".into(), kind: SupportKind::Frictionless, faces: vec![0] };
        assert!(Problem::build(mesh, &on, &setup(vec![slide], vec![], surfaces)).unwrap_err().contains("flat or round"));
    }
}
